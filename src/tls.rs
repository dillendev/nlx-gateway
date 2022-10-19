use std::path::{Path, PathBuf};

use tokio::fs;
use tonic::transport::{Certificate, ClientTlsConfig, Identity};

use anyhow::Result;

pub struct TlsPair {
    pub root_pem: Vec<u8>,
    pub cert_pem: Vec<u8>,
    pub key_pem: Vec<u8>,
}

impl TlsPair {
    pub fn bundle(&self) -> Vec<u8> {
        let mut bundle = self.cert_pem.clone();
        bundle.push(b'\n');
        bundle.extend_from_slice(&self.root_pem);
        bundle
    }
}

pub async fn read_pair(
    root: impl AsRef<Path>,
    cert: impl AsRef<Path>,
    key: impl AsRef<Path>,
) -> Result<TlsPair> {
    let root_pem = fs::read(root.as_ref()).await?;
    let cert_pem = fs::read(cert.as_ref()).await?;
    let key_pem = fs::read(key.as_ref()).await?;

    Ok(TlsPair {
        root_pem,
        cert_pem,
        key_pem,
    })
}

pub async fn client_config(
    root: PathBuf,
    client_cert: PathBuf,
    client_key: PathBuf,
) -> Result<ClientTlsConfig> {
    let pair = read_pair(root, client_cert, client_key).await?;
    let root = Certificate::from_pem(pair.root_pem);
    let client = Identity::from_pem(pair.cert_pem, pair.key_pem);
    let tls_config = ClientTlsConfig::new().ca_certificate(root).identity(client);

    Ok(tls_config)
}
