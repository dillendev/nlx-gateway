use std::path::PathBuf;

use tokio::fs;
use tonic::transport::{Certificate, ClientTlsConfig, Identity};

use anyhow::Result;

pub async fn client_config(
    root: PathBuf,
    client_cert: PathBuf,
    client_key: PathBuf,
) -> Result<ClientTlsConfig> {
    let root = Certificate::from_pem(fs::read_to_string(root).await?);
    let client_cert = fs::read_to_string(client_cert).await?;
    let client_key = fs::read_to_string(client_key).await?;
    let client = Identity::from_pem(client_cert, client_key);
    let tls_config = ClientTlsConfig::new()
        .ca_certificate(root)
        .identity(client);

    Ok(tls_config)
}
