use std::{net::SocketAddr, sync::Arc};

use reqwest::{Certificate, ClientBuilder, Identity};
use tokio::sync::{
    broadcast::{error::RecvError, Receiver},
    RwLock,
};
use warp::Filter;

use crate::{
    filters::with_request,
    reverse_proxy,
    tls::{self, TlsPair},
};

use super::{
    config::{self},
    Config,
};

type OutwayConfig = Arc<RwLock<config::Config>>;

async fn handle_events(config: OutwayConfig, mut rx: Receiver<Config>) {
    loop {
        match rx.recv().await {
            Ok(new_config) => {
                let mut lock = config.write().await;
                *lock = new_config;

                log::info!("outway config updated");
            }
            Err(RecvError::Lagged(num)) => {
                log::warn!("server is lagging, missed {} outway events", num);
            }
            Err(RecvError::Closed) => break,
        }
    }
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
        let config = OutwayConfig::default();

        // Handle config changes
        tokio::spawn(handle_events(Arc::clone(&config), self.rx));

        let ca_cert = Certificate::from_pem(&self.tls_pair.root_pem)?;
        let bundle = tls::pem_bundle(&self.tls_pair.cert_pem, &self.tls_pair.key_pem);
        let identity = Identity::from_pem(&bundle)?;

        // Build warp filters
        let client = ClientBuilder::new()
            .use_rustls_tls()
            .trust_dns(true)
            .http2_prior_knowledge()
            .http1_only()
            .tls_built_in_root_certs(false)
            .add_root_certificate(ca_cert)
            .identity(identity)
            .https_only(true)
            .http2_adaptive_window(true)
            .http2_max_frame_size(16777215)
            .build()?;
        let with_config = warp::any().map(move || Arc::clone(&config));
        let with_client = warp::any().map(move || client.clone());
        let route = warp::any()
            .and(with_config)
            .and(with_client)
            .and(warp::path!(String / String))
            .and(with_request!())
            .and_then(
                |config: OutwayConfig, client, oin, service_name, request| async move {
                    let inway_address = {
                        let lock = config.read().await;
                        let service = lock.services.get(&oin).and_then(|services| {
                            services.iter().find(|service| service.name == service_name)
                        });

                        service.and_then(|s| {
                            s.inways
                                .first()
                                // @TODO: re-enable this when the Inway monitoring stuff works
                                //.find(|inway| inway.state == State::Up)
                                .map(|inway| inway.address.clone())
                        })
                    };

                    match inway_address {
                        Some(address) => {
                            log::debug!("proxy {}: {}", service_name, request);
                            reverse_proxy::handle(
                                client,
                                request,
                                &[address, service_name].join("/"),
                            )
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

        warp::serve(route).run(addr).await;

        Ok(())
    }
}
