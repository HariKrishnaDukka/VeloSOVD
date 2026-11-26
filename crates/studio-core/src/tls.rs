//! mTLS client with a pinned server identity.
//!
//! Studio dials `127.0.0.1:<port>` (a TCP tunnel into the vehicle network)
//! but the server certificate is issued for the vehicle-side name/IP
//! (`10.77.10.1`). Instead of switching hostname checks off, the verifier
//! checks the certificate chain AND the configured `tls_server_name`.
//! TLS stays end-to-end: the tunnel only forwards encrypted bytes.

use std::{fs, sync::Arc};

use rustls::{
    ClientConfig, DigitallySignedStruct, Error as TlsError, RootCertStore, SignatureScheme,
    client::{
        WebPkiServerVerifier,
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    },
};
use rustls_pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime, pem::PemObject};

use crate::StudioConfig;

#[derive(Debug)]
struct PinnedNameVerifier {
    inner: Arc<WebPkiServerVerifier>,
    expected: ServerName<'static>,
}

impl ServerCertVerifier for PinnedNameVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _dialled_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        self.inner.verify_server_cert(
            end_entity,
            intermediates,
            &self.expected,
            ocsp_response,
            now,
        )
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

fn read(path: &str) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("{path}: {e}"))
}

/// Build the HTTPS client: TLS 1.3 only, custom CA only, device certificate,
/// pinned server identity.
pub fn build_client(cfg: &StudioConfig) -> Result<reqwest::Client, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());

    let mut roots = RootCertStore::empty();
    for cert in CertificateDer::pem_slice_iter(&read(&cfg.ca_cert)?) {
        roots
            .add(cert.map_err(|e| format!("{}: {e}", cfg.ca_cert))?)
            .map_err(|e| format!("{}: {e}", cfg.ca_cert))?;
    }
    if roots.is_empty() {
        return Err(format!("{}: no CA certificates", cfg.ca_cert));
    }
    let inner = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), Arc::clone(&provider))
        .build()
        .map_err(|e| e.to_string())?;
    let expected = ServerName::try_from(cfg.tls_server_name.clone())
        .map_err(|e| format!("tls_server_name: {e}"))?;

    let chain = CertificateDer::pem_slice_iter(&read(&cfg.device_cert)?)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("{}: {e}", cfg.device_cert))?;
    let key = PrivateKeyDer::from_pem_slice(&read(&cfg.device_key)?)
        .map_err(|e| format!("{}: {e}", cfg.device_key))?;

    let tls = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| e.to_string())?
        .dangerous() // only to install the pinned-name verifier; chain checks stay on
        .with_custom_certificate_verifier(Arc::new(PinnedNameVerifier { inner, expected }))
        .with_client_auth_cert(chain, key)
        .map_err(|e| format!("device certificate/key: {e}"))?;

    reqwest::Client::builder()
        .use_preconfigured_tls(tls)
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())
}
