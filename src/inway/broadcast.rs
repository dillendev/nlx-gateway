use std::time::Duration;

use anyhow::Result;
use tokio::{task::JoinHandle, time};
use tonic::transport::Channel;

use crate::pb::{management_client::ManagementClient, Inway};

fn get_hostname() -> Result<String> {
    hostname::get()
        .map_err(|e| anyhow::anyhow!("failed to get hostname: {}", e))
        .and_then(|h| {
            h.into_string()
                .map_err(|_| anyhow::anyhow!("hostname is not valid utf-8"))
        })
}

const BROADCAST_INTERVAL: Duration = Duration::from_secs(10);

pub struct Broadcast {
    inway_name: String,
    inway_address: String,
    management: ManagementClient<Channel>,
}

impl Broadcast {
    pub fn new(
        management: ManagementClient<Channel>,
        inway_name: String,
        inway_address: String,
    ) -> Self {
        Self {
            inway_name,
            inway_address,
            management,
        }
    }

    async fn register_inway(&mut self) -> Result<()> {
        self.management
            .register_inway(Inway {
                name: self.inway_name.clone(),
                version: "0.0.1".to_string(),
                hostname: get_hostname()?,
                self_address: self.inway_address.clone(),
                services: vec![],
                ip_address: "".to_string(),
            })
            .await?;

        Ok(())
    }

    async fn broadcast(&mut self) -> Result<()> {
        self.register_inway().await?;
        log::info!("inway registered");

        let mut announce_interval = time::interval(BROADCAST_INTERVAL);

        loop {
            announce_interval.tick().await;

            log::info!("broadcast");
        }
    }

    pub fn broadcast_start(mut self) -> Result<JoinHandle<()>> {
        log::info!("start broadcasting");

        Ok(tokio::spawn(async move {
            if let Err(e) = self.broadcast().await {
                log::error!("broadcasting failed: {}", e);
            }
        }))
    }
}
