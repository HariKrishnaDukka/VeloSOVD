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
    pub principal: String,
    pub role: Role,
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
}

#[derive(Debug, Clone)]
pub struct AccessPolicy {
    principals: Vec<(ServerName<'static>, String, Role)>,
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
        for entry in file.principals {
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
        Ok(Self { principals })
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
}

/// Pure decision function (unit-tested without TLS).
pub fn decide(
    policy: &AccessPolicy,
    peer_certificates: Option<&[Vec<u8>]>,
    method: &Method,
    path: &str,
) -> Result<ClientIdentity, Denial> {
    let identity = peer_certificates
        .and_then(|certs| policy.identify(certs))
        .ok_or_else(|| Denial {
            status: StatusCode::FORBIDDEN,
            error_code: "insufficient-access-rights",
            message: "client certificate is not mapped to any role".to_string(),
        })?;

    let required = required_role(method, path);
    if identity.role < required {
        return Err(Denial {
            status: StatusCode::FORBIDDEN,
            error_code: "insufficient-access-rights",
            message: format!(
                "role {:?} is not allowed; {method} {path} requires {required:?}",
                identity.role
            ),
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

    match decide(&policy, certs.as_deref(), &method, &path) {
        Ok(identity) => {
            request.extensions_mut().insert(identity);
            next.run(request).await
        }
        Err(denial) => {
            eprintln!(
                "[AUTHZ] deny {method} {path}: {} ({})",
                denial.message, denial.error_code
            );
            (
                denial.status,
                Json(serde_json::json!({
                    "error_code": denial.error_code,
                    "message": denial.message,
                })),
            )
                .into_response()
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
}
