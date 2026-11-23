use std::{error::Error, fs, io, path::Path, sync::Arc};

use axum_server::tls_rustls::RustlsConfig;
use rustls::{RootCertStore, ServerConfig, server::WebPkiClientVerifier};
use rustls_pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};

type TlsResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

pub fn load_server_config(
    client_ca_path: &Path,
    server_cert_path: &Path,
    server_key_path: &Path,
) -> TlsResult<RustlsConfig> {
    let client_ca_pem = fs::read(client_ca_path)?;
    let client_ca_certs =
        CertificateDer::pem_slice_iter(&client_ca_pem).collect::<Result<Vec<_>, _>>()?;

    if client_ca_certs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "No client CA certificates found",
        )
        .into());
    }

    let mut root_store = RootCertStore::empty();

    for cert in client_ca_certs {
        root_store.add(cert)?;
    }

    let cert_pem = fs::read(server_cert_path)?;
    let certificate_chain =
        CertificateDer::pem_slice_iter(&cert_pem).collect::<Result<Vec<_>, _>>()?;

    if certificate_chain.is_empty() {
        return Err(
            io::Error::new(io::ErrorKind::InvalidData, "No server certificates found").into(),
        );
    }

    let key_pem = fs::read(server_key_path)?;
    let private_key = PrivateKeyDer::from_pem_slice(&key_pem)?;

    let provider = Arc::new(rustls::crypto::ring::default_provider());

    let client_verifier =
        WebPkiClientVerifier::builder_with_provider(Arc::new(root_store), Arc::clone(&provider))
            .build()?;

    let server_config = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_client_cert_verifier(client_verifier)
        .with_single_cert(certificate_chain, private_key)?;

    let mut server_config = server_config;
    server_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    Ok(RustlsConfig::from_config(Arc::new(server_config)))
}
