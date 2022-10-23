use std::time::Duration;

use anyhow::Result;
use tokio::{
    sync::broadcast::{error::RecvError, Receiver},
    task::JoinHandle,
    time,
};
use tonic::{transport::Channel, Request};

use crate::{
    backoff::retry_backoff,
    pb::{
        directory::{
            directory_client::DirectoryClient, register_inway_request::RegisterService,
            RegisterInwayRequest,
        },
        management::{management_client::ManagementClient, Inway},
    },
};

use super::{Event, InwayConfig};

fn get_hostname() -> Result<String> {
    hostname::get()
        .map_err(|e| anyhow::anyhow!("failed to get hostname: {}", e))
        .and_then(|h| {
            h.into_string()
                .map_err(|_| anyhow::anyhow!("hostname is not valid utf-8"))
        })
}

const BROADCAST_INTERVAL: Duration = Duration::from_secs(10);
const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct Broadcast {
    inway_name: String,
    inway_address: String,
    management: ManagementClient<Channel>,
    directory: DirectoryClient<Channel>,
}

impl Broadcast {
    pub fn new(
        management: ManagementClient<Channel>,
        directory: DirectoryClient<Channel>,
        inway_name: String,
        inway_address: String,
    ) -> Self {
        Self {
            inway_name,
            inway_address,
            management,
            directory,
        }
    }

    async fn register_inway(&mut self) -> Result<()> {
        self.management
            .register_inway(Inway {
                name: self.inway_name.clone(),
                version: VERSION.to_string(),
                hostname: get_hostname()?,
                self_address: self.inway_address.clone(),
                services: vec![],
                ip_address: String::new(),
            })
            .await?;

        Ok(())
    }

    async fn announce(&mut self, config: &InwayConfig) -> Result<()> {
        log::trace!("announcing services to directory");

        let mut request = Request::new(RegisterInwayRequest {
            inway_address: self.inway_address.clone(),
            services: config
                .services
                .iter()
                .map(|(_, service)| RegisterService {
                    name: service.name.clone(),
                    documentation_url: service.documentation_url.clone(),
                    api_specification_type: String::new(),
                    api_specification_document_url: String::new(),
                    internal: service.internal,
                    public_support_contact: service.public_support_contact.clone(),
                    tech_support_contact: service.tech_support_contact.clone(),
                    one_time_costs: service.one_time_costs,
                    monthly_costs: service.monthly_costs,
                    request_costs: service.request_costs,
                })
                .collect(),
            inway_name: self.inway_name.clone(),
            // @TODO: manage state for organization inway
            is_organization_inway: true,
            management_api_proxy_address: String::new(),
        });

        let metadata = request.metadata_mut();
        metadata.append("nlx-component", "inway".parse()?);
        metadata.append("nlx-version", VERSION.parse()?);

        let response = self.directory.register_inway(request).await?;
        let result = response.get_ref();

        if !result.error.is_empty() {
            return Err(anyhow::anyhow!(
                "failed to announce inway: {}",
                result.error
            ));
        }

        Ok(())
    }

    async fn broadcast(&mut self, rx: &mut Receiver<Event>) -> Result<()> {
        self.register_inway().await?;
        log::info!("inway registered");

        let response = self.directory.get_version(()).await?;
        log::info!("directory version: {}", response.into_inner().version);

        // We don't want to announce services without receiving the configuration as we
        // would clear the services in the directory otherwise.
        let mut inway_config = None;
        let mut announce_interval = time::interval(BROADCAST_INTERVAL);

        loop {
            tokio::select! {
                _ = announce_interval.tick(), if inway_config.is_some() => {
                    self.announce(inway_config.as_ref().unwrap()).await?;
                }
                result = rx.recv() => match result {
                    Ok(event) => match event {
                        Event::ConfigUpdated(config) => {
                            inway_config = Some(config);
                        }
                    }
                    // In this case we've missed some events so we should assume the `inway_config` is no longer valid
                    Err(RecvError::Lagged(num)) => {
                        log::warn!("broadcast is lagging, missed {} inway events", num);
                        inway_config = None;
                    }
                    Err(RecvError::Closed) => {
                        log::info!("broadcast channel closed");
                        return Ok(());
                    }
                }
            }
        }
    }

    pub fn broadcast_start(mut self, mut rx: Receiver<Event>) -> Result<JoinHandle<()>> {
        log::info!("start broadcasting");

        Ok(tokio::spawn(async move {
            retry_backoff!(
                self.broadcast(&mut rx),
                |err, duration: Duration| log::warn!(
                    "broadcast failed: {:?}, retrying in {} seconds",
                    err,
                    duration.as_secs()
                )
            );
        }))
    }
}
