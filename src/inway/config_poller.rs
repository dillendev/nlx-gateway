use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    time::Duration,
};

use anyhow::{Context, Result};
use tokio::{
    select,
    sync::mpsc::{channel, Receiver, Sender},
    time,
};
use tonic::transport::{Channel, ClientTlsConfig};

use crate::pb::{self, management_client::ManagementClient, GetInwayConfigRequest, Inway};

use super::{Event, InwayConfig, Service};

fn get_hostname() -> Result<String> {
    hostname::get()
        .map_err(|e| anyhow::anyhow!("failed to get hostname: {}", e))
        .and_then(|h| {
            h.into_string()
                .map_err(|_| anyhow::anyhow!("hostname is not valid utf-8"))
        })
}

pub struct ConfigPoller {
    inway_name: String,
    inway_addr: String,
    management_api_addr: String,
    tls_config: ClientTlsConfig,
}

impl ConfigPoller {
    pub fn new(
        inway_name: String,
        inway_addr: String,
        management_api_addr: String,
        tls_config: ClientTlsConfig,
    ) -> Self {
        ConfigPoller {
            inway_name,
            inway_addr,
            management_api_addr,
            tls_config,
        }
    }

    fn get_inway(&self) -> Result<Inway> {
        Ok(Inway {
            name: self.inway_name.clone(),
            version: "0.0.1".to_string(),
            hostname: get_hostname()?,
            self_address: self.inway_addr.to_string(),
            services: vec![],
            ip_address: "".to_string(),
        })
    }

    async fn connect_management_api(&self) -> Result<ManagementClient<Channel>> {
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

        let mut client = self.connect_management_api().await?;

        // register the inway
        client.register_inway(self.get_inway()?).await?;

        tx.send(Event::InwayRegistered).await?;

        let mut interval = time::interval(Duration::from_secs(10));
        let mut old_hash = None;

        loop {
            select! {
                _ = interval.tick() => {
                    log::trace!("retrieving config from management API");

                    let response = client
                        .get_inway_config(GetInwayConfigRequest {
                            name: self.inway_name.clone(),
                        })
                        .await?;
                    let config = map_config(response.into_inner());

                    let mut hasher = DefaultHasher::new();
                    config.hash(&mut hasher);
                    let new_hash = hasher.finish();

                    if old_hash != Some(new_hash) {
                        log::debug!("config changed");
                        tx.send(Event::ConfigUpdated(config)).await?;

                        old_hash = Some(new_hash);
                    }
                }
                _ = tx.closed() => {
                    log::debug!("config poller stopped");
                    break;
                }
            }
        }

        Ok(())
    }

    pub fn poll_start(self) -> Result<Receiver<Event>> {
        log::info!("start polling for config changes");

        let (tx, rx) = channel(10);

        tokio::spawn(async move {
            if let Err(e) = self.poll(tx).await {
                log::error!("error polling config: {:#?}", e);
            }
        });

        Ok(rx)
    }
}

fn map_config(response: pb::GetInwayConfigResponse) -> InwayConfig {
    InwayConfig {
        services: response
            .services
            .into_iter()
            .map(|s| (s.name.clone(), Service { name: s.name }))
            .collect(),
    }
}
