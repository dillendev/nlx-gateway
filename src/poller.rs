use std::time::Duration;

use anyhow::Result;
use tokio::{sync::broadcast::Sender, task::JoinHandle, time};
use tonic::async_trait;

use crate::backoff::retry_backoff;

#[async_trait]
pub trait Poll {
    type Event;

    async fn poll(&mut self, tx: &mut Sender<Self::Event>) -> Result<()>
    where
        Self::Event: Send + Sync + 'static;
}

pub struct Poller<T: Poll> {
    poll: T,
    duration: Duration,
}

impl<T: Poll + Sync + Send + 'static> Poller<T>
where
    T::Event: Send + Sync + 'static,
{
    pub fn new(poll: T, duration: Duration) -> Self {
        Self { poll, duration }
    }

    pub async fn poll(&mut self, tx: &mut Sender<T::Event>) -> Result<()> {
        let mut interval = time::interval(self.duration);

        loop {
            interval.tick().await;
            self.poll.poll(tx).await?;
        }
    }

    pub fn poll_start(mut self, mut tx: Sender<T::Event>) -> JoinHandle<()> {
        log::info!("start polling for changes");

        tokio::spawn(async move {
            retry_backoff!(self.poll(&mut tx), |err, duration: Duration| log::warn!(
                "failed to poll: {}, retrying in {:?}s",
                err,
                duration.as_secs()
            ));
        })
    }
}
