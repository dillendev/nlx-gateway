use std::net::SocketAddr;

use tokio::sync::broadcast::Receiver;
use warp::Filter;

use crate::tls::TlsPair;

use super::Config;

pub struct Server {
    tls_pair: TlsPair,
    rx: Receiver<Config>,
}

impl Server {
    pub fn new(tls_pair: TlsPair, rx: Receiver<Config>) -> Self {
        Self { tls_pair, rx }
    }

    pub async fn run(self, addr: SocketAddr) -> anyhow::Result<()> {
        let route = warp::get().map(|| "Hello, world!");

        warp::serve(route).run(addr).await;

        Ok(())
    }
}
