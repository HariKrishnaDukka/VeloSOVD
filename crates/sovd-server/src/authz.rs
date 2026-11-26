//! R2: role-based authorization from the mTLS client certificate.
//!
//! The TLS layer already proves *who* the client is (certificate signed by
//! the Client CA). This module decides *what* that client may do.
//!
//! Identity: the leaf certificate's DNS subjectAltName is matched against the
//! principals in the access policy file (`SDV_ACCESS_POLICY`).
//! Deny by default: a certificate that matches no principal gets 403.

use std::{error::Error, fs, io, path::Path, sync::Arc};

use axum::{
    Json,
    body::Body,
    extract::State,
    http::{Method, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use axum_server::tls::TlsInfo;
use rustls_pki_types::{CertificateDer, ServerName};
use serde::Deserialize;

type PolicyResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

/// Roles in increasing privilege order. `PartialOrd` follows declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    /// Read-only: GET on every resource.
    Viewer,
    /// Viewer + clear faults + start/stop operation executions.
    Technician,
    /// Technician + raw UDS passthrough (still limited by the R1 allow-list).
    Engineer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientIdentity {
    /// Device identity: the mTLS certificate's DNS SAN.
    pub principal: String,
    /// Effective role: min(device role, user role) when a user token is used.
    pub role: Role,
    /// M1: user name from a verified bearer token, if any.
    pub user: Option<String>,
    /// M1: token id (`jti`) for audit correlation.
    pub token_id: Option<String>,
}

impl Role {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "viewer" => Some(Self::Viewer),
            "technician" => Some(Self::Technician),
            "engineer" => Some(Self::Engineer),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyFile {
    #[serde(rename = "principal", default)]
    principals: Vec<PrincipalEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrincipalEntry {
    dns_name: String,
    role: Role,
    /// M1: requests from this device must carry a valid user bearer token.
    #[serde(default)]
    require_user_token: bool,
}

#[derive(Clone)]
pub struct AccessPolicy {
    principals: Vec<(ServerName<'static>, String, Role)>,
    /// Principals (device DNS names) that require a user token.
    token_required: Vec<String>,
    /// M1: public key that verifies user tokens; `None` = tokens not accepted.
    verifier: Option<sovd_auth::TokenVerifier>,
}

impl std::fmt::Debug for AccessPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccessPolicy")
            .field("principals", &self.principals.len())
            .field("token_required", &self.token_required)
            .field("verifier", &self.verifier.is_some())
            .finish()
    }
}

impl AccessPolicy {
    pub fn load(path: &Path) -> PolicyResult<Self> {
        Self::from_toml(&fs::read_to_string(path)?)
    }

    pub fn from_toml(text: &str) -> PolicyResult<Self> {
        let file: PolicyFile = toml::from_str(text)?;
        if file.principals.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "access policy defines no principals",
            )
            .into());
        }

        let mut principals = Vec::with_capacity(file.principals.len());
        let mut token_required = Vec::new();
        for entry in file.principals {
            if entry.require_user_token {
                token_required.push(entry.dns_name.clone());
            }
            let name = ServerName::try_from(entry.dns_name.clone())?;
            if principals.iter().any(|(_, p, _)| *p == entry.dns_name) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("duplicate principal '{}'", entry.dns_name),
                )
                .into());
            }
            principals.push((name, entry.dns_name, entry.role));
        }
        Ok(Self {
            principals,
            token_required,
            verifier: None,
        })
    }

    /// M1: accept user tokens signed by the auth-service key.
    pub fn with_token_verifier(mut self, verifier: Option<sovd_auth::TokenVerifier>) -> Self {
        self.verifier = verifier;
        self
    }

    /// Principals that need tokens but cannot get them (no verifier) - a startup error.
    pub fn unsatisfiable_token_principals(&self) -> Vec<String> {
        if self.verifier.is_some() {
            Vec::new()
        } else {
            self.token_required.clone()
        }
    }

    /// Map the client's leaf certificate to a configured principal.
    /// Returns `None` (deny) when nothing matches or the DER is unparsable.
    pub fn identify(&self, peer_certificates: &[Vec<u8>]) -> Option<ClientIdentity> {
        let leaf = CertificateDer::from(peer_certificates.first()?.as_slice());
        let cert = webpki::EndEntityCert::try_from(&leaf).ok()?;
        self.principals
            .iter()
            .find(|(name, _, _)| cert.verify_is_valid_for_subject_name(name).is_ok())
            .map(|(_, principal, role)| ClientIdentity {
                principal: principal.clone(),
                role: *role,
                user: None,
                token_id: None,
            })
    }
}

/// Minimum role for a request. Unknown combinations need the highest role.
pub fn required_role(method: &Method, path: &str) -> Role {
    if method == Method::GET || method == Method::HEAD {
        return Role::Viewer;
    }
    if path.ends_with("/diagnostics/requests") {
        return Role::Engineer;
    }
    let writes = method == Method::POST || method == Method::PUT || method == Method::DELETE;
    let technician_resource = path.contains("/faults")
        || path.contains("/operations/")
        || path.ends_with("/operations/clear-faults");
    if writes && technician_resource {
        return Role::Technician;
    }
    Role::Engineer
}

#[derive(Debug, PartialEq, Eq)]
pub struct Denial {
    pub status: StatusCode,
    pub error_code: &'static str,
    pub message: String,
    /// Who was refused, when known (device, and user if the token was valid).
    pub identity: Option<Box<ClientIdentity>>,
}

/// Pure decision function without a user token (tests and token-free devices).
#[cfg(test)]
pub fn decide(
    policy: &AccessPolicy,
    peer_certificates: Option<&[Vec<u8>]>,
    method: &Method,
    path: &str,
) -> Result<ClientIdentity, Denial> {
    decide_with_token(
        policy,
        peer_certificates,
        None,
        method,
        path,
        sovd_auth::unix_now(),
    )
}

fn unauthorized(error_code: &'static str, message: impl Into<String>) -> Denial {
    Denial {
        status: StatusCode::UNAUTHORIZED,
        error_code,
        message: message.into(),
        identity: None,
    }
}

/// Pure decision function (unit-tested without TLS).
///
/// 1. Device: mTLS certificate DNS SAN -> principal + device role (else 403).
/// 2. User (M1): `Authorization: Bearer <token>` verified with the auth-service
///    public key; required for principals with `require_user_token` (else 401).
/// 3. Effective role = min(device role, user role); compared with the route's need (else 403).
pub fn decide_with_token(
    policy: &AccessPolicy,
    peer_certificates: Option<&[Vec<u8>]>,
    bearer: Option<&str>,
    method: &Method,
    path: &str,
    now: u64,
) -> Result<ClientIdentity, Denial> {
    let mut identity = peer_certificates
        .and_then(|certs| policy.identify(certs))
        .ok_or_else(|| Denial {
            status: StatusCode::FORBIDDEN,
            error_code: "insufficient-access-rights",
            message: "client certificate is not mapped to any role".to_string(),
            identity: None,
        })?;

    match bearer {
        Some(token) => {
            let verifier = policy.verifier.as_ref().ok_or_else(|| {
                unauthorized(
                    "invalid-token",
                    "user tokens are not accepted by this server",
                )
            })?;
            let claims = verifier
                .verify(token, now)
                .map_err(|e| unauthorized("invalid-token", e.to_string()))?;
            let user_role = Role::parse(&claims.role)
                .ok_or_else(|| unauthorized("invalid-token", "token carries an unknown role"))?;
            identity.role = identity.role.min(user_role);
            identity.user = Some(claims.sub);
            identity.token_id = Some(claims.jti);
        }
        None if policy.token_required.contains(&identity.principal) => {
            let mut denial = unauthorized(
                "user-token-required",
                format!(
                    "device '{}' requires a logged-in user (Bearer token)",
                    identity.principal
                ),
            );
            denial.identity = Some(Box::new(identity));
            return Err(denial);
        }
        None => {}
    }

    let required = required_role(method, path);
    if identity.role < required {
        return Err(Denial {
            status: StatusCode::FORBIDDEN,
            error_code: "insufficient-access-rights",
            message: format!(
                "role {:?} is not allowed; {method} {path} requires {required:?}",
                identity.role
            ),
            identity: Some(Box::new(identity)),
        });
    }
    Ok(identity)
}

/// Axum middleware: apply with `middleware::from_fn_with_state(policy, authorize)`.
pub async fn authorize(
    State(policy): State<Arc<AccessPolicy>>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let certs = request
        .extensions()
        .get::<Arc<TlsInfo>>()
        .map(|info| info.peer_certificates.clone());
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let bearer = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| t.trim().to_string());

    match decide_with_token(
        &policy,
        certs.as_deref(),
        bearer.as_deref(),
        &method,
        &path,
        sovd_auth::unix_now(),
    ) {
        Ok(identity) => {
            request.extensions_mut().insert(identity.clone());
            let mut response = next.run(request).await;
            // Let the outer audit layer record the verified user.
            response.extensions_mut().insert(identity);
            response
        }
        Err(denial) => {
            eprintln!(
                "[AUTHZ] deny {method} {path}: {} ({})",
                denial.message, denial.error_code
            );
            let mut response = (
                denial.status,
                Json(serde_json::json!({
                    "error_code": denial.error_code,
                    "message": denial.message,
                })),
            )
                .into_response();
            if let Some(identity) = denial.identity {
                response.extensions_mut().insert(*identity);
            }
            if denial.status == StatusCode::UNAUTHORIZED {
                response.headers_mut().insert(
                    axum::http::header::WWW_AUTHENTICATE,
                    axum::http::HeaderValue::from_static("Bearer realm=\"velosovd\""),
                );
            }
            response
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls_pki_types::pem::PemObject;

    const POLICY: &str = r#"
[[principal]]
dns_name = "harness-client"
role = "engineer"

[[principal]]
dns_name = "workshop-viewer"
role = "viewer"
"#;

    fn der(pem: &str) -> Vec<Vec<u8>> {
        vec![
            CertificateDer::from_pem_slice(pem.as_bytes())
                .unwrap()
                .as_ref()
                .to_vec(),
        ]
    }

    fn engineer() -> Vec<Vec<u8>> {
        der(include_str!("../testdata/harness-client.crt"))
    }

    fn viewer() -> Vec<Vec<u8>> {
        der(include_str!("../testdata/workshop-viewer.crt"))
    }

    fn policy() -> AccessPolicy {
        AccessPolicy::from_toml(POLICY).unwrap()
    }

    #[test]
    fn identifies_principal_by_dns_san() {
        let p = policy();
        assert_eq!(p.identify(&engineer()).unwrap().role, Role::Engineer);
        assert_eq!(p.identify(&viewer()).unwrap().role, Role::Viewer);
    }

    #[test]
    fn unknown_certificate_is_denied() {
        let only_engineer = AccessPolicy::from_toml(
            "[[principal]]\ndns_name = \"harness-client\"\nrole = \"engineer\"\n",
        )
        .unwrap();
        assert!(only_engineer.identify(&viewer()).is_none());
        assert!(only_engineer.identify(&[b"not a cert".to_vec()]).is_none());
        assert!(only_engineer.identify(&[]).is_none());
    }

    #[test]
    fn missing_tls_info_is_denied() {
        let d = decide(&policy(), None, &Method::GET, "/api/ecus").unwrap_err();
        assert_eq!(d.status, StatusCode::FORBIDDEN);
    }

    #[test]
    fn required_roles_match_policy_table() {
        use Role::*;
        let cases = [
            (Method::GET, "/api/ecus/ECM/diagnostics/faults", Viewer),
            (
                Method::DELETE,
                "/api/ecus/ECM/diagnostics/faults",
                Technician,
            ),
            (
                Method::DELETE,
                "/api/ecus/ECM/diagnostics/faults/P0101",
                Technician,
            ),
            (
                Method::POST,
                "/api/ecus/ECM/diagnostics/operations/clear-faults",
                Technician,
            ),
            (
                Method::POST,
                "/sovd/v1/components/ECM/operations/clear-faults/executions",
                Technician,
            ),
            (
                Method::PUT,
                "/sovd/v1/components/ECM/operations/clear-faults/executions/1",
                Technician,
            ),
            (Method::POST, "/api/ecus/ECM/diagnostics/requests", Engineer),
            (Method::PATCH, "/api/ecus/ECM", Engineer),
            (Method::POST, "/api/unknown", Engineer),
        ];
        for (method, path, role) in cases {
            assert_eq!(required_role(&method, path), role, "{method} {path}");
        }
    }

    #[test]
    fn viewer_can_read_but_not_write() {
        let p = policy();
        let v = viewer();
        assert!(
            decide(
                &p,
                Some(&v),
                &Method::GET,
                "/api/ecus/ECM/diagnostics/faults"
            )
            .is_ok()
        );
        let d = decide(
            &p,
            Some(&v),
            &Method::DELETE,
            "/api/ecus/ECM/diagnostics/faults",
        )
        .unwrap_err();
        assert_eq!(d.status, StatusCode::FORBIDDEN);
        assert!(
            decide(
                &p,
                Some(&v),
                &Method::POST,
                "/api/ecus/ECM/diagnostics/requests"
            )
            .is_err()
        );
    }

    #[test]
    fn engineer_can_use_raw_uds() {
        let p = policy();
        let id = decide(
            &p,
            Some(&engineer()),
            &Method::POST,
            "/api/ecus/ECM/diagnostics/requests",
        )
        .unwrap();
        assert_eq!(id.principal, "harness-client");
    }

    #[test]
    fn rejects_bad_policy_files() {
        assert!(AccessPolicy::from_toml("").is_err());
        assert!(
            AccessPolicy::from_toml("[[principal]]\ndns_name = \"a\"\nrole = \"admin\"\n").is_err()
        );
        assert!(
            AccessPolicy::from_toml(
                "[[principal]]\ndns_name = \"a\"\nrole = \"viewer\"\n\
                 [[principal]]\ndns_name = \"a\"\nrole = \"engineer\"\n"
            )
            .is_err()
        );
        assert!(
            AccessPolicy::from_toml(
                "[[principal]]\ndns_name = \"a\"\nrole = \"viewer\"\nextra = 1\n"
            )
            .is_err()
        );
    }

    // ---- M1: user tokens ----

    fn token_keys() -> (sovd_auth::TokenSigner, sovd_auth::TokenVerifier) {
        use base64::Engine;
        use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair};
        let rng = ring::rand::SystemRandom::new();
        let der = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
        let b64 = base64::engine::general_purpose::STANDARD.encode(der.as_ref());
        let pem = format!("-----BEGIN PRIVATE KEY-----\n{b64}\n-----END PRIVATE KEY-----\n");
        let signer = sovd_auth::TokenSigner::from_pkcs8_pem(pem.as_bytes()).unwrap();
        let verifier = signer.verifier();
        (signer, verifier)
    }

    /// harness-client: engineer device that requires a user token.
    fn token_policy(verifier: Option<sovd_auth::TokenVerifier>) -> AccessPolicy {
        AccessPolicy::from_toml(
            "[[principal]]\ndns_name = \"harness-client\"\nrole = \"engineer\"\nrequire_user_token = true\n\
             [[principal]]\ndns_name = \"workshop-viewer\"\nrole = \"viewer\"\n",
        )
        .unwrap()
        .with_token_verifier(verifier)
    }

    const FAULTS: &str = "/api/ecus/ECM/diagnostics/faults";
    const RAW: &str = "/api/ecus/ECM/diagnostics/requests";

    #[test]
    fn device_requiring_token_without_token_gets_401() {
        let (_, v) = token_keys();
        let d = decide_with_token(
            &token_policy(Some(v)),
            Some(&engineer()),
            None,
            &Method::GET,
            FAULTS,
            sovd_auth::unix_now(),
        )
        .unwrap_err();
        assert_eq!(d.status, StatusCode::UNAUTHORIZED);
        assert_eq!(d.error_code, "user-token-required");
    }

    #[test]
    fn effective_role_is_minimum_of_device_and_user() {
        let (s, v) = token_keys();
        let p = token_policy(Some(v));
        let now = sovd_auth::unix_now();
        let (tech, _) = s.issue("hari", "technician", 900).unwrap();
        let id = decide_with_token(
            &p,
            Some(&engineer()),
            Some(&tech),
            &Method::DELETE,
            FAULTS,
            now,
        )
        .unwrap();
        assert_eq!(
            (id.role, id.user.as_deref()),
            (Role::Technician, Some("hari"))
        );
        // technician user on an engineer device still cannot use raw UDS
        let d = decide_with_token(&p, Some(&engineer()), Some(&tech), &Method::POST, RAW, now)
            .unwrap_err();
        assert_eq!(d.status, StatusCode::FORBIDDEN);
        // engineer user on a viewer device is capped at viewer
        let (eng, _) = s.issue("ravi", "engineer", 900).unwrap();
        let id =
            decide_with_token(&p, Some(&viewer()), Some(&eng), &Method::GET, FAULTS, now).unwrap();
        assert_eq!(id.role, Role::Viewer);
        assert!(
            decide_with_token(
                &p,
                Some(&viewer()),
                Some(&eng),
                &Method::DELETE,
                FAULTS,
                now
            )
            .is_err()
        );
    }

    #[test]
    fn bad_expired_or_foreign_tokens_get_401() {
        let (s, v) = token_keys();
        let p = token_policy(Some(v));
        let now = sovd_auth::unix_now();
        let (token, claims) = s.issue("hari", "engineer", 60).unwrap();
        let later = claims.exp + sovd_auth::LEEWAY_SECS + 1;
        for (tok, at) in [(token.as_str(), later), ("garbage", now)] {
            let d = decide_with_token(&p, Some(&engineer()), Some(tok), &Method::GET, FAULTS, at)
                .unwrap_err();
            assert_eq!(
                (d.status, d.error_code),
                (StatusCode::UNAUTHORIZED, "invalid-token")
            );
        }
        let (other, _) = token_keys();
        let (foreign, _) = other.issue("hari", "engineer", 900).unwrap();
        assert_eq!(
            decide_with_token(
                &p,
                Some(&engineer()),
                Some(&foreign),
                &Method::GET,
                FAULTS,
                now
            )
            .unwrap_err()
            .status,
            StatusCode::UNAUTHORIZED
        );
    }

    #[test]
    fn devices_without_token_requirement_work_unchanged() {
        let (_, v) = token_keys();
        let id = decide_with_token(
            &token_policy(Some(v)),
            Some(&viewer()),
            None,
            &Method::GET,
            FAULTS,
            sovd_auth::unix_now(),
        )
        .unwrap();
        assert_eq!((id.role, id.user), (Role::Viewer, None));
    }

    #[test]
    fn token_requirement_without_public_key_is_reported() {
        assert_eq!(
            token_policy(None).unsatisfiable_token_principals(),
            vec!["harness-client".to_string()]
        );
        let (_, v) = token_keys();
        assert!(
            token_policy(Some(v))
                .unsatisfiable_token_principals()
                .is_empty()
        );
    }

    #[test]
    fn denial_names_the_refused_user_for_audit() {
        let (s, v) = token_keys();
        let p = token_policy(Some(v));
        let (tech, _) = s.issue("hari", "technician", 900).unwrap();
        let d = decide_with_token(
            &p,
            Some(&engineer()),
            Some(&tech),
            &Method::POST,
            RAW,
            sovd_auth::unix_now(),
        )
        .unwrap_err();
        let who = d.identity.expect("refused identity must be recorded");
        assert_eq!(
            (who.user.as_deref(), who.role),
            (Some("hari"), Role::Technician)
        );
        // token-required denial still names the device
        let d = decide_with_token(
            &p,
            Some(&engineer()),
            None,
            &Method::GET,
            FAULTS,
            sovd_auth::unix_now(),
        )
        .unwrap_err();
        assert_eq!(d.identity.unwrap().principal, "harness-client");
    }
}
