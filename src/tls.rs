use std::path::Path;

use tokio::fs;
use tonic::transport::{Certificate, ClientTlsConfig, Identity};

use anyhow::Result;

// @TODO: do some validation
pub struct TlsPair {
    pub root_pem: Vec<u8>,
    pub cert_pem: Vec<u8>,
    pub key_pem: Vec<u8>,
}

impl TlsPair {
    pub fn new(root_pem: Vec<u8>, cert_pem: Vec<u8>, key_pem: Vec<u8>) -> Result<Self> {
        Ok(Self {
            root_pem,
            cert_pem,
            key_pem,
        })
    }

    pub fn bundle(&self) -> Vec<u8> {
        let mut bundle = self.cert_pem.clone();
        bundle.push(b'\n');
        bundle.extend_from_slice(&self.root_pem);
        bundle
    }

    pub fn client_config(&self) -> ClientTlsConfig {
        let root = Certificate::from_pem(&self.root_pem);
        let client = Identity::from_pem(&self.cert_pem, &self.key_pem);

        ClientTlsConfig::new().ca_certificate(root).identity(client)
    }

    pub async fn from_files(
        root: impl AsRef<Path>,
        cert: impl AsRef<Path>,
        key: impl AsRef<Path>,
    ) -> Result<TlsPair> {
        let root_pem = fs::read(root.as_ref()).await?;
        let cert_pem = fs::read(cert.as_ref()).await?;
        let key_pem = fs::read(key.as_ref()).await?;

        TlsPair::new(root_pem, cert_pem, key_pem)
    }
}

pub async fn client_config(
    root: impl AsRef<Path>,
    client_cert: impl AsRef<Path>,
    client_key: impl AsRef<Path>,
) -> Result<ClientTlsConfig> {
    let pair = TlsPair::from_files(root, client_cert, client_key).await?;
    Ok(pair.client_config())
}
