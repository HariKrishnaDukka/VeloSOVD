//! M1: user access tokens shared by `auth-service` (issuer) and `sovd-server` (verifier).
//!
//! Format: JWT (RFC 7519) signed with ES256 (ECDSA P-256 + SHA-256).
//! A token is `base64url(header).base64url(claims).base64url(signature)`.
//!
//! In C terms: a small struct `{user, role, expiry}` plus a signature. Only the
//! auth-service holds the private key and can create one; anyone with the
//! public key can check it, offline.
//!
//! Verification is strict: the header must be exactly ES256/JWT (no `alg:none`,
//! no algorithm switching), and issuer, audience and expiry are always checked.

use std::{
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use ring::{
    rand::SystemRandom,
    signature::{
        ECDSA_P256_SHA256_FIXED, ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair,
        UnparsedPublicKey,
    },
};
use rustls_pki_types::{PrivateKeyDer, pem::PemObject};
use serde::{Deserialize, Serialize};

pub const ISSUER: &str = "velosovd-auth";
pub const AUDIENCE: &str = "velosovd-central";
/// Clock tolerance between auth-service and central.
pub const LEEWAY_SECS: u64 = 30;

const HEADER_JSON: &str = r#"{"alg":"ES256","typ":"JWT"}"#;

/// P-256 SubjectPublicKeyInfo prefix (DER) before the 65-byte uncompressed point.
const P256_SPKI_PREFIX: [u8; 26] = [
    0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a,
    0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    /// Issuer, must be [`ISSUER`].
    pub iss: String,
    /// Audience, must be [`AUDIENCE`].
    pub aud: String,
    /// User name.
    pub sub: String,
    /// Role name: `viewer`, `technician` or `engineer`.
    pub role: String,
    /// Issued at (unix seconds).
    pub iat: u64,
    /// Expiry (unix seconds).
    pub exp: u64,
    /// Unique token id (for audit correlation).
    pub jti: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenError {
    Malformed,
    UnsupportedHeader,
    BadSignature,
    WrongIssuer,
    WrongAudience,
    Expired,
    NotYetValid,
    Key(String),
}

impl fmt::Display for TokenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Malformed => "token is malformed",
            Self::UnsupportedHeader => "token header is not ES256/JWT",
            Self::BadSignature => "token signature is invalid",
            Self::WrongIssuer => "token issuer is not accepted",
            Self::WrongAudience => "token audience is not this server",
            Self::Expired => "token has expired",
            Self::NotYetValid => "token is not valid yet",
            Self::Key(e) => return write!(f, "key error: {e}"),
        };
        f.write_str(text)
    }
}

impl std::error::Error for TokenError {}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// Issues tokens. Held only by the auth-service.
pub struct TokenSigner {
    key: EcdsaKeyPair,
    rng: SystemRandom,
}

impl TokenSigner {
    /// Load a PKCS#8 P-256 private key in PEM form (`-----BEGIN PRIVATE KEY-----`).
    pub fn from_pkcs8_pem(pem: &[u8]) -> Result<Self, TokenError> {
        let der = match PrivateKeyDer::from_pem_slice(pem) {
            Ok(PrivateKeyDer::Pkcs8(der)) => der,
            Ok(_) => return Err(TokenError::Key("expected a PKCS#8 private key".into())),
            Err(e) => return Err(TokenError::Key(e.to_string())),
        };
        let rng = SystemRandom::new();
        let key = EcdsaKeyPair::from_pkcs8(
            &ECDSA_P256_SHA256_FIXED_SIGNING,
            der.secret_pkcs8_der(),
            &rng,
        )
        .map_err(|e| TokenError::Key(format!("not a P-256 key: {e}")))?;
        Ok(Self { key, rng })
    }

    /// Public key as a verifier (used in tests and by tooling).
    pub fn verifier(&self) -> TokenVerifier {
        TokenVerifier {
            point: self.key.public_key().as_ref().to_vec(),
        }
    }

    pub fn sign(&self, claims: &Claims) -> Result<String, TokenError> {
        let payload = serde_json::to_vec(claims).map_err(|_| TokenError::Malformed)?;
        let signing_input = format!("{}.{}", B64.encode(HEADER_JSON), B64.encode(payload));
        let signature = self
            .key
            .sign(&self.rng, signing_input.as_bytes())
            .map_err(|_| TokenError::Key("signing failed".into()))?;
        Ok(format!(
            "{signing_input}.{}",
            B64.encode(signature.as_ref())
        ))
    }

    /// Build claims for `user` with `role`, valid for `ttl_secs` from now.
    pub fn issue(
        &self,
        user: &str,
        role: &str,
        ttl_secs: u64,
    ) -> Result<(String, Claims), TokenError> {
        let now = unix_now();
        let mut id = [0u8; 12];
        ring::rand::SecureRandom::fill(&self.rng, &mut id)
            .map_err(|_| TokenError::Key("random generation failed".into()))?;
        let claims = Claims {
            iss: ISSUER.into(),
            aud: AUDIENCE.into(),
            sub: user.into(),
            role: role.into(),
            iat: now,
            exp: now + ttl_secs,
            jti: id.iter().map(|b| format!("{b:02x}")).collect(),
        };
        Ok((self.sign(&claims)?, claims))
    }
}

/// Checks tokens. Held by central (public key only).
#[derive(Clone)]
pub struct TokenVerifier {
    point: Vec<u8>,
}

impl TokenVerifier {
    /// Load a P-256 public key in PEM SubjectPublicKeyInfo form (`-----BEGIN PUBLIC KEY-----`).
    pub fn from_spki_pem(pem: &str) -> Result<Self, TokenError> {
        let body: String = pem
            .lines()
            .filter(|l| !l.starts_with("-----"))
            .map(str::trim)
            .collect();
        let der = base64::engine::general_purpose::STANDARD
            .decode(body)
            .map_err(|_| TokenError::Key("public key PEM is not base64".into()))?;
        if der.len() != P256_SPKI_PREFIX.len() + 65
            || der[..P256_SPKI_PREFIX.len()] != P256_SPKI_PREFIX
        {
            return Err(TokenError::Key(
                "expected an uncompressed P-256 public key".into(),
            ));
        }
        Ok(Self {
            point: der[P256_SPKI_PREFIX.len()..].to_vec(),
        })
    }

    /// Verify signature, header, issuer, audience and time. Returns the claims.
    pub fn verify(&self, token: &str, now: u64) -> Result<Claims, TokenError> {
        let mut parts = token.split('.');
        let (Some(h), Some(p), Some(s), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(TokenError::Malformed);
        };
        let header = B64.decode(h).map_err(|_| TokenError::Malformed)?;
        let header: serde_json::Value =
            serde_json::from_slice(&header).map_err(|_| TokenError::Malformed)?;
        let expected: serde_json::Value = serde_json::from_str(HEADER_JSON).expect("constant JSON");
        if header != expected {
            return Err(TokenError::UnsupportedHeader);
        }
        let signature = B64.decode(s).map_err(|_| TokenError::Malformed)?;
        UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, &self.point)
            .verify(format!("{h}.{p}").as_bytes(), &signature)
            .map_err(|_| TokenError::BadSignature)?;

        let payload = B64.decode(p).map_err(|_| TokenError::Malformed)?;
        let claims: Claims = serde_json::from_slice(&payload).map_err(|_| TokenError::Malformed)?;
        if claims.iss != ISSUER {
            return Err(TokenError::WrongIssuer);
        }
        if claims.aud != AUDIENCE {
            return Err(TokenError::WrongAudience);
        }
        if now > claims.exp + LEEWAY_SECS {
            return Err(TokenError::Expired);
        }
        if claims.iat > now + LEEWAY_SECS {
            return Err(TokenError::NotYetValid);
        }
        Ok(claims)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ring::signature::EcdsaKeyPair;
    use std::sync::OnceLock;

    /// PEM-wrap DER bytes (test helper; no key files are committed).
    fn pem(label: &str, der: &[u8]) -> String {
        let b64 = base64::engine::general_purpose::STANDARD.encode(der);
        let lines: Vec<&str> = b64
            .as_bytes()
            .chunks(64)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect();
        format!(
            "-----BEGIN {label}-----\n{}\n-----END {label}-----\n",
            lines.join("\n")
        )
    }

    /// Fresh P-256 key pair per test run: (private PKCS#8 PEM, public SPKI PEM).
    fn key_pair() -> (String, String) {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
        let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng)
            .unwrap();
        let mut spki = P256_SPKI_PREFIX.to_vec();
        spki.extend_from_slice(pair.public_key().as_ref());
        (pem("PRIVATE KEY", pkcs8.as_ref()), pem("PUBLIC KEY", &spki))
    }

    fn keys_a() -> &'static (String, String) {
        static A: OnceLock<(String, String)> = OnceLock::new();
        A.get_or_init(key_pair)
    }

    fn signer() -> TokenSigner {
        TokenSigner::from_pkcs8_pem(keys_a().0.as_bytes()).unwrap()
    }

    fn verifier() -> TokenVerifier {
        TokenVerifier::from_spki_pem(&keys_a().1).unwrap()
    }

    #[test]
    fn issued_token_verifies_with_public_key_file() {
        let (token, claims) = signer().issue("hari", "technician", 900).unwrap();
        let got = verifier().verify(&token, unix_now()).unwrap();
        assert_eq!(got, claims);
        assert_eq!(got.sub, "hari");
        assert_eq!(got.exp - got.iat, 900);
    }

    #[test]
    fn rejects_expired_and_future_tokens() {
        let (token, claims) = signer().issue("hari", "viewer", 60).unwrap();
        let v = verifier();
        assert!(v.verify(&token, claims.exp + LEEWAY_SECS).is_ok());
        assert_eq!(
            v.verify(&token, claims.exp + LEEWAY_SECS + 1),
            Err(TokenError::Expired)
        );
        assert_eq!(
            v.verify(&token, claims.iat - LEEWAY_SECS - 1),
            Err(TokenError::NotYetValid)
        );
    }

    #[test]
    fn rejects_token_from_another_key() {
        let other = TokenSigner::from_pkcs8_pem(key_pair().0.as_bytes()).unwrap();
        let (token, _) = other.issue("hari", "engineer", 900).unwrap();
        assert_eq!(
            verifier().verify(&token, unix_now()),
            Err(TokenError::BadSignature)
        );
    }

    #[test]
    fn rejects_modified_claims() {
        let (token, _) = signer().issue("hari", "viewer", 900).unwrap();
        let parts: Vec<&str> = token.split('.').collect();
        let mut claims: Claims = serde_json::from_slice(&B64.decode(parts[1]).unwrap()).unwrap();
        claims.role = "engineer".into();
        let forged = format!(
            "{}.{}.{}",
            parts[0],
            B64.encode(serde_json::to_vec(&claims).unwrap()),
            parts[2]
        );
        assert_eq!(
            verifier().verify(&forged, unix_now()),
            Err(TokenError::BadSignature)
        );
    }

    #[test]
    fn rejects_alg_none_and_other_headers() {
        let (token, _) = signer().issue("hari", "viewer", 900).unwrap();
        let parts: Vec<&str> = token.split('.').collect();
        for header in [
            r#"{"alg":"none","typ":"JWT"}"#,
            r#"{"alg":"HS256","typ":"JWT"}"#,
        ] {
            let forged = format!("{}.{}.{}", B64.encode(header), parts[1], parts[2]);
            assert_eq!(
                verifier().verify(&forged, unix_now()),
                Err(TokenError::UnsupportedHeader)
            );
        }
        let unsigned = format!(
            "{}.{}.",
            B64.encode(r#"{"alg":"none","typ":"JWT"}"#),
            parts[1]
        );
        assert!(verifier().verify(&unsigned, unix_now()).is_err());
    }

    #[test]
    fn rejects_wrong_issuer_or_audience() {
        let s = signer();
        let now = unix_now();
        let base = Claims {
            iss: ISSUER.into(),
            aud: AUDIENCE.into(),
            sub: "hari".into(),
            role: "viewer".into(),
            iat: now,
            exp: now + 60,
            jti: "x".into(),
        };
        let mut c = base.clone();
        c.iss = "someone-else".into();
        assert_eq!(
            verifier().verify(&s.sign(&c).unwrap(), now),
            Err(TokenError::WrongIssuer)
        );
        let mut c = base;
        c.aud = "other-server".into();
        assert_eq!(
            verifier().verify(&s.sign(&c).unwrap(), now),
            Err(TokenError::WrongAudience)
        );
    }

    #[test]
    fn rejects_malformed_input_and_keys() {
        let v = verifier();
        for bad in ["", "a.b", "a.b.c.d", "!!!.###.$$$"] {
            assert!(v.verify(bad, unix_now()).is_err(), "{bad}");
        }
        assert!(
            TokenVerifier::from_spki_pem(
                "-----BEGIN PUBLIC KEY-----\nAAAA\n-----END PUBLIC KEY-----"
            )
            .is_err()
        );
        assert!(TokenSigner::from_pkcs8_pem(b"not a key").is_err());
    }

    #[test]
    fn signer_public_key_matches_pub_file() {
        let (token, _) = signer().issue("hari", "viewer", 60).unwrap();
        assert!(signer().verifier().verify(&token, unix_now()).is_ok());
        assert!(verifier().verify(&token, unix_now()).is_ok());
    }
}
