use std::{net::SocketAddr, sync::Arc};

use reqwest::{Client, ClientBuilder};

use serde::Serialize;
use tokio::sync::{
    broadcast::{error::RecvError, Receiver},
    RwLock,
};
use warp::{path::Tail, Filter};

use crate::tls::TlsPair;

use super::{
    config,
    reverse_proxy::{self, Request},
    Event,
};

type InwayConfig = Arc<RwLock<config::InwayConfig>>;

async fn handle_events(config: InwayConfig, mut rx: Receiver<Event>) {
    loop {
        match rx.recv().await {
            Ok(event) => match event {
                Event::ConfigUpdated(new_config) => {
                    let mut lock = config.write().await;
                    *lock = new_config;

                    log::info!("inway config updated");
                }
            },
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
    rx: Receiver<Event>,
}

impl Server {
    pub fn new(tls_pair: TlsPair, rx: Receiver<Event>) -> Self {
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
            .build()?;
        let with_config = warp::any().map(move || Arc::clone(&config));
        let with_client = warp::any().map(move || client.clone());
        let optional_query = warp::filters::query::raw()
            .or(warp::any().map(String::default))
            .unify();
        let with_request = warp::any()
            .and(warp::method())
            .and(warp::filters::path::tail())
            .and(optional_query)
            .and(warp::header::headers_cloned())
            .and(warp::body::stream())
            .map(|method, path: Tail, query, headers, body| {
                Request::new(method, path, query, headers, body)
            });

        // Setup routes
        let proxy = warp::any()
            .and(with_config.clone())
            .and(with_client)
            .and(warp::path::param())
            .and(with_request)
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
                            reverse_proxy::handle(client, request, &upstream).await
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
