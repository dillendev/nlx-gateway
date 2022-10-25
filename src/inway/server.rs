use std::{net::SocketAddr, sync::Arc};

use reqwest::ClientBuilder;

use serde::Serialize;
use tokio::sync::{
    broadcast::{error::RecvError, Receiver},
    RwLock,
};
use warp::Filter;

use crate::{filters::with_request, reverse_proxy, tls::TlsPair};

use super::{config, Config};

type InwayConfig = Arc<RwLock<config::Config>>;

async fn handle_events(config: InwayConfig, mut rx: Receiver<Config>) {
    loop {
        match rx.recv().await {
            Ok(new_config) => {
                let mut lock = config.write().await;
                *lock = new_config;

                log::info!("inway config updated");
            }
            Err(RecvError::Lagged(num)) => {
                log::warn!("server is lagging, missed {} inway events", num);
            }
            Err(RecvError::Closed) => break,
        }
    }
}

#[derive(Serialize)]
pub struct Health {
    pub healthy: bool,
    pub version: String,
}

pub struct Server {
    tls_pair: TlsPair,
    rx: Receiver<Config>,
}

impl Server {
    pub fn new(tls_pair: TlsPair, rx: Receiver<Config>) -> Self {
        Self { tls_pair, rx }
    }

    pub async fn run(self, addr: SocketAddr) -> anyhow::Result<()> {
        let config = InwayConfig::default();

        // Handle config changes
        tokio::spawn(handle_events(Arc::clone(&config), self.rx));

        // Build warp filters
        let client = ClientBuilder::new()
            .use_rustls_tls()
            .trust_dns(true)
            .http2_adaptive_window(true)
            .http2_max_frame_size(16777215)
            .build()?;
        let with_config = warp::any().map(move || Arc::clone(&config));
        let with_client = warp::any().map(move || client.clone());

        // Setup routes
        let proxy = warp::any()
            .and(with_config.clone())
            .and(with_client)
            .and(warp::path::param())
            .and(with_request!())
            .and_then(
                |config: InwayConfig, client, service: String, request| async move {
                    let upstream = {
                        config
                            .read()
                            .await
                            .services
                            .get(&service)
                            .map(|svc| svc.endpoint_url.clone())
                    };

                    match upstream {
                        Some(upstream) => {
                            log::debug!("proxy {}: {}", service, request);
                            reverse_proxy::handle(client, request, &upstream)
                                .await
                                .map_err(|e| {
                                    log::error!("proxy failed: {:?}", e);
                                    e
                                })
                        }
                        None => Err(warp::reject::not_found()),
                    }
                },
            );
        let health = warp::get()
            .and(warp::path(".nlx"))
            .and(warp::path("health"))
            .and(with_config)
            .and(warp::path::param())
            .then(|config: InwayConfig, service: String| async move {
                let healthy = { config.read().await.services.contains_key(&service) };

                warp::reply::json(&Health {
                    healthy,
                    version: String::new(),
                })
            });

        // Run the server
        warp::serve(health.or(proxy))
            .tls()
            .cert(&self.tls_pair.cert_pem)
            .key(&self.tls_pair.key_pem)
            .client_auth_required(self.tls_pair.bundle())
            .run(addr)
            .await;

        Ok(())
    }
}
