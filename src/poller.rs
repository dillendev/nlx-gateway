use std::time::Duration;

use anyhow::Result;
use tokio::{task::JoinHandle, time};
use tonic::async_trait;

use crate::backoff::retry_backoff;

#[async_trait]
pub trait Poll {
    async fn poll(&mut self) -> Result<()>;
}

pub struct Poller<T: Poll> {
    poll: T,
    duration: Duration,
}

impl<T: Poll + Sync + Send + 'static> Poller<T> {
    pub fn new(poll: T, duration: Duration) -> Self {
        Self { poll, duration }
    }

    pub async fn poll(&mut self) -> Result<()> {
        let mut interval = time::interval(self.duration);

        loop {
            interval.tick().await;
            self.poll.poll().await?;
        }
    }

    pub fn poll_start(mut self) -> JoinHandle<()> {
        log::info!("start polling for changes");

        tokio::spawn(async move {
            retry_backoff!(self.poll(), |err, duration: Duration| log::warn!(
                "failed to poll: {}, retrying in {:?}s",
                err,
                duration.as_secs()
            ));
        })
    }
}
