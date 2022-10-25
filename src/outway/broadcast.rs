use std::time::Duration;

use anyhow::Result;
use tokio::{task::JoinHandle, time};
use tonic::transport::Channel;

use crate::{
    backoff::retry_backoff,
    pb::{
        directory::{self, directory_client::DirectoryClient},
        management::{self, management_client::ManagementClient},
    },
};

const REGISTRATION_INTERVAL: Duration = Duration::from_secs(10);
const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct Broadcast {
    outway_name: String,
    public_key_pem: String,
    management: ManagementClient<Channel>,
    directory: DirectoryClient<Channel>,
}

impl Broadcast {
    pub fn new(
        management: ManagementClient<Channel>,
        directory: DirectoryClient<Channel>,
        public_key_pem: String,
        outway_name: String,
    ) -> Self {
        Self {
            management,
            directory,
            public_key_pem,
            outway_name,
        }
    }

    async fn announce(&mut self) -> Result<()> {
        log::trace!("announcing outway");

        tokio::try_join!(
            self.management
                .register_outway(management::RegisterOutwayRequest {
                    name: self.outway_name.clone(),
                    public_key_pem: self.public_key_pem.clone(),
                    version: VERSION.to_string(),
                    self_address_api: "http://localhost".to_string(),
                }),
            self.directory
                .register_outway(directory::RegisterOutwayRequest {
                    name: self.outway_name.clone(),
                })
        )?;

        Ok(())
    }

    async fn broadcast(&mut self) -> Result<()> {
        let response = self.directory.get_version(()).await?;
        log::info!("directory version: {}", response.into_inner().version);

        let mut announce_interval = time::interval(REGISTRATION_INTERVAL);

        loop {
            announce_interval.tick().await;
            self.announce().await?;
        }
    }

    pub fn broadcast_start(mut self) -> Result<JoinHandle<()>> {
        log::info!("start broadcasting");

        Ok(tokio::spawn(async move {
            retry_backoff!(self.broadcast(), |err, duration: Duration| log::warn!(
                "broadcast failed: {:?}, retrying in {}s",
                err,
                duration.as_secs()
            ));
        }))
    }
}
