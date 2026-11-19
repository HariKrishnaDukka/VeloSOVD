use std::{error::Error, io, path::Path, sync::Arc};

use axum_server::tls_rustls::RustlsConfig;
use rustls::{RootCertStore, ServerConfig, server::WebPkiClientVerifier};
use rustls_pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};

type TlsResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

/// Load a zonal server identity and require client certificates
/// issued by the configured development CA.
pub fn load_server_config(
    ca_path: &str,
    cert_path: &str,
    key_path: &str,
) -> TlsResult<RustlsConfig> {
    // Load trusted CA certificates.
    let ca_certs =
        CertificateDer::pem_file_iter(Path::new(ca_path))?.collect::<Result<Vec<_>, _>>()?;

    if ca_certs.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "No CA certificates found").into());
    }

    let mut root_store = RootCertStore::empty();

    for certificate in ca_certs {
        root_store.add(certificate)?;
    }

    // Load the zonal server certificate chain.
    let certificate_chain =
        CertificateDer::pem_file_iter(Path::new(cert_path))?.collect::<Result<Vec<_>, _>>()?;

    if certificate_chain.is_empty() {
        return Err(
            io::Error::new(io::ErrorKind::InvalidData, "No server certificates found").into(),
        );
    }

    // Load the matching private key.
    let private_key = PrivateKeyDer::from_pem_file(Path::new(key_path))?;

    // Select the Ring provider explicitly. Do not rely on an
    // implicitly installed process-wide cryptographic provider.
    let provider = Arc::new(rustls::crypto::ring::default_provider());

    // Require every connecting client to present a certificate
    // issued by the trusted development CA.
    let client_verifier =
        WebPkiClientVerifier::builder_with_provider(Arc::new(root_store), Arc::clone(&provider))
            .build()?;

    let mut server_config = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_client_cert_verifier(client_verifier)
        .with_single_cert(certificate_chain, private_key)?;

    // Required for HTTP/2 and HTTP/1.1 negotiation.
    server_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    Ok(RustlsConfig::from_config(Arc::new(server_config)))
}
