use anyhow::{Context, Result};
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tonic::transport::{Channel, ClientTlsConfig};

use crate::pb::{management_client::ManagementClient, Inway};

fn get_hostname() -> Result<String> {
    hostname::get()
        .map_err(|e| anyhow::anyhow!("failed to get hostname: {}", e))
        .and_then(|h| {
            h.into_string()
                .map_err(|_| anyhow::anyhow!("hostname is not valid utf-8"))
        })
}

#[derive(Debug)]
pub enum Event {
    InwayRegistered,
}

pub struct ConfigPoller {
    inway_addr: String,
    management_api_addr: String,
    tls_config: ClientTlsConfig,
}

impl ConfigPoller {
    pub fn new(
        inway_addr: String,
        management_api_addr: String,
        tls_config: ClientTlsConfig,
    ) -> Self {
        ConfigPoller {
            inway_addr,
            management_api_addr,
            tls_config,
        }
    }

    fn inway(&self) -> Result<Inway> {
        Ok(Inway {
            name: "NLX-Gateway".to_string(),
            version: "0.0.1".to_string(),
            hostname: get_hostname()?,
            self_address: self.inway_addr.to_string(),
            services: vec![],
            ip_address: "".to_string(),
        })
    }

    async fn connect_to_management_api(&self) -> Result<ManagementClient<Channel>> {
        let channel = Channel::from_shared(self.management_api_addr.clone())?
            .tls_config(self.tls_config.clone())
            .with_context(|| "failed to setup TLS config")?
            .connect()
            .await
            .with_context(|| "failed to connect")?;

        Ok(ManagementClient::new(channel))
    }

    // @TODO: autorestart on failure
    pub async fn poll(self, tx: Sender<Event>) -> Result<()> {
        log::debug!(
            "connecting to management API at: {}",
            self.management_api_addr
        );

        let mut client = self.connect_to_management_api().await?;

        // register the inway
        client
            .register_inway(self.inway()?)
            .await
            .with_context(|| "failed to register inway")?;

        tx.send(Event::InwayRegistered).await?;

        Ok(())
    }

    pub fn poll_start(self) -> Result<Receiver<Event>> {
        log::debug!("start polling for config changes");

        let (tx, rx) = channel(10);

        tokio::spawn(async move {
            if let Err(e) = self.poll(tx).await {
                log::error!("error polling config: {:#?}", e);
            }
        });

        Ok(rx)
    }
}
