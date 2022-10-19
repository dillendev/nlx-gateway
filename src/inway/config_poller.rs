use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    time::Duration,
};

use anyhow::Result;
use tokio::{select, sync::mpsc::Sender, task::JoinHandle, time};
use tonic::transport::Channel;

use crate::pb::{self, management_client::ManagementClient, GetInwayConfigRequest};

use super::{Event, InwayConfig, Service};

pub struct ConfigPoller {
    inway_name: String,
    management: ManagementClient<Channel>,
}

impl ConfigPoller {
    pub fn new(management: ManagementClient<Channel>, inway_name: String) -> Self {
        ConfigPoller {
            management,
            inway_name,
        }
    }

    // @TODO: autorestart on failure
    pub async fn poll(mut self, tx: Sender<Event>) -> Result<()> {
        let mut interval = time::interval(Duration::from_secs(10));
        let mut old_hash = None;

        loop {
            select! {
                _ = interval.tick() => {
                    log::trace!("retrieving config from management API");

                    let response = self.management
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

    pub fn poll_start(self, tx: Sender<Event>) -> Result<JoinHandle<()>> {
        log::info!("start polling for config changes");

        Ok(tokio::spawn(async move {
            if let Err(e) = self.poll(tx).await {
                log::error!("error polling config: {:#?}", e);
            }
        }))
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
