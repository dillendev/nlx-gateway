use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    time::Duration,
};

use anyhow::Result;
use tokio::{sync::broadcast::Sender, task::JoinHandle, time};
use tonic::transport::Channel;

use crate::pb::management::{
    management_client::ManagementClient, GetInwayConfigRequest, GetInwayConfigResponse,
};

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
    pub async fn poll(&mut self, tx: Sender<Event>) -> Result<()> {
        let mut interval = time::interval(Duration::from_secs(10));
        let mut old_hash = None;

        loop {
            interval.tick().await;
            log::trace!("retrieving config from management API");

            let response = self
                .management
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
                tx.send(Event::ConfigUpdated(config))?;

                old_hash = Some(new_hash);
            }
        }
    }

    pub fn poll_start(mut self, tx: Sender<Event>) -> Result<JoinHandle<()>> {
        log::info!("start polling for config changes");

        Ok(tokio::spawn(async move {
            if let Err(e) = self.poll(tx).await {
                log::error!("error polling config: {:?}", e);
            }
        }))
    }
}

fn map_config(response: GetInwayConfigResponse) -> InwayConfig {
    InwayConfig {
        services: response
            .services
            .into_iter()
            .map(|s| {
                (
                    s.name.clone(),
                    Service {
                        name: s.name,
                        internal: s.internal,
                        documentation_url: s.documentation_url,
                        tech_support_contact: s.tech_support_contact,
                        public_support_contact: s.public_support_contact,
                        one_time_costs: s.one_time_costs,
                        monthly_costs: s.monthly_costs,
                        request_costs: s.request_costs,
                    },
                )
            })
            .collect(),
    }
}
