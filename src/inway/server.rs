use std::{net::SocketAddr, sync::Arc};

use async_channel::Receiver;
use reqwest::{ClientBuilder, Url};

use serde::Serialize;
use tokio::sync::RwLock;
use warp::Filter;

use crate::{filters::with_request, reverse_proxy, tls::TlsPair};

use super::{config::ServiceInwayMap, Config};

type ServiceInwayMapState = Arc<RwLock<ServiceInwayMap>>;

async fn handle_events(state: ServiceInwayMapState, rx: Receiver<Config>) {
    loop {
        match rx.recv().await {
            Ok(new_config) => {
                let mut lock = state.write().await;
                *lock = new_config
                    .services
                    .into_iter()
                    .map(|(name, service)| {
                        (name, Url::parse(&service.endpoint_url).ok().map(Arc::new))
                    })
                    .collect();

                log::info!("inway config updated");
            }
            Err(_) => {
                log::debug!("config channel closed");
                break;
            }
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
        let state = ServiceInwayMapState::default();

        // Handle config changes
        tokio::spawn(handle_events(Arc::clone(&state), self.rx));

        // Build warp filters
        let client = ClientBuilder::new()
            .use_rustls_tls()
            .trust_dns(true)
            .http2_adaptive_window(true)
            .build()?;
        let with_state = warp::any().map(move || Arc::clone(&state));
        let with_client = warp::any().map(move || client.clone());

        // Setup routes
        let proxy = warp::any()
            .and(with_state.clone())
            .and(with_client)
            .and(warp::path::param())
            .and(with_request!())
            .and_then(
                |state: ServiceInwayMapState, client, service: String, request| async move {
                    let upstream = {
                        state
                            .read()
                            .await
                            .get(&service)
                            .map(|endpoint| endpoint.as_ref().map(Arc::clone))
                    };

                    match upstream {
                        Some(Some(upstream)) => {
                            log::debug!("proxy {}: {}", service, request);
                            reverse_proxy::handle(client, request, &upstream)
                                .await
                                .map_err(|e| {
                                    log::error!("proxy failed: {:?}", e);
                                    e
                                })
                        }
                        Some(None) => {
                            log::warn!("service {} has an invalid endpoint", service);
                            Err(warp::reject::not_found())
                        }
                        None => Err(warp::reject::not_found()),
                    }
                },
            );
        let health = warp::get()
            .and(warp::path(".nlx"))
            .and(warp::path("health"))
            .and(with_state)
            .and(warp::path::param())
            .then(|state: ServiceInwayMapState, service: String| async move {
                let healthy = { state.read().await.contains_key(&service) };

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
