use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use anyhow::Result;
use itertools::Itertools;
use tokio::sync::broadcast::Sender;
use tonic::{async_trait, transport::Channel};

use crate::{
    pb::directory::{directory_client::DirectoryClient, ListServicesRequest, ListServicesResponse},
    poller::Poll,
};

use super::{
    config::{Costs, Inway, Organization, Service},
    Config,
};

fn normalize_address(address: String) -> String {
    if !address.contains("://") {
        // @TODO: assume HTTPS?
        return format!("https://{}", address);
    }

    address
}

fn map_config(response: ListServicesResponse) -> Config {
    Config {
        services: response
            .services
            .into_iter()
            .group_by(|service| {
                service
                    .organization
                    .as_ref()
                    .map(|organization| organization.serial_number.clone())
                    .unwrap_or_default()
            })
            .into_iter()
            .map(|(organization, services)| {
                (
                    organization,
                    services
                        .into_iter()
                        .map(|service| {
                            let costs = service.costs.unwrap_or_default();
                            let org = service.organization.unwrap_or_default();

                            Service {
                                name: service.name,
                                documentation_url: service.documentation_url,
                                api_specification_type: service.api_specification_type,
                                internal: service.internal,
                                public_support_contact: service.public_support_contact,
                                inways: service
                                    .inways
                                    .into_iter()
                                    .map(|inway| Inway {
                                        address: normalize_address(inway.address),
                                        state: inway.state.into(),
                                    })
                                    .collect(),
                                costs: Costs {
                                    one_time: costs.one_time,
                                    monthly: costs.monthly,
                                    request: costs.request,
                                },
                                organization: Organization {
                                    name: org.name,
                                    serial_number: org.serial_number,
                                },
                            }
                        })
                        .collect(),
                )
            })
            .collect(),
    }
}

pub struct ConfigPoller {
    config_hash: Option<u64>,
    directory: DirectoryClient<Channel>,
}

impl ConfigPoller {
    pub fn new(directory: DirectoryClient<Channel>) -> Self {
        Self {
            config_hash: None,
            directory,
        }
    }
}

#[async_trait]
impl Poll for ConfigPoller {
    type Event = Config;

    async fn poll(&mut self, tx: &mut Sender<Config>) -> Result<()> {
        log::trace!("retrieving config from directory");

        let response = self.directory.list_services(ListServicesRequest {}).await?;
        let config = map_config(response.into_inner());

        let mut hasher = DefaultHasher::new();
        config.hash(&mut hasher);
        let new_hash = hasher.finish();

        if Some(new_hash) != self.config_hash {
            log::debug!("config changed");
            tx.send(config)?;
            self.config_hash = Some(new_hash);
        }

        Ok(())
    }
}
