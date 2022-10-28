use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use anyhow::Result;
use async_channel::Sender;
use futures_util::future::try_join_all;
use tonic::{async_trait, transport::Channel};

use crate::{
    pb::management::{
        management_client::ManagementClient, GetInwayConfigRequest, GetInwayConfigResponse,
    },
    poller::Poll,
};

use super::{Config, Service};

fn map_config(response: GetInwayConfigResponse) -> Config {
    Config {
        services: response
            .services
            .into_iter()
            .map(|s| {
                (
                    s.name.clone(),
                    Service {
                        name: s.name,
                        internal: s.internal,
                        endpoint_url: s.endpoint_url,
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

pub struct ConfigPoller {
    inway_name: String,
    config_hash: Option<u64>,
    management: ManagementClient<Channel>,
    subscribers: Vec<Sender<Config>>,
}

impl ConfigPoller {
    pub fn new(management: ManagementClient<Channel>, inway_name: String) -> Self {
        ConfigPoller {
            management,
            config_hash: None,
            inway_name,
            subscribers: vec![],
        }
    }

    pub fn subscribe(&mut self, tx: Sender<Config>) {
        self.subscribers.push(tx);
    }
}

#[async_trait]
impl Poll for ConfigPoller {
    async fn poll(&mut self) -> Result<()> {
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

        if self.config_hash != Some(new_hash) {
            log::debug!("config changed");

            try_join_all(
                self.subscribers
                    .iter_mut()
                    .map(|tx| tx.send(config.clone())),
            )
            .await?;

            self.config_hash = Some(new_hash);
        }

        Ok(())
    }
}
