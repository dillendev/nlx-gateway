use std::{net::SocketAddr, sync::Arc};

use async_channel::Receiver;
use reqwest::{Certificate, ClientBuilder, Identity, Url};
use tokio::sync::RwLock;
use warp::Filter;

use crate::{
    filters::with_request,
    reverse_proxy,
    tls::{self, TlsPair},
};

use super::{config::ServiceInways, Config};

type ServiceInwaysState = Arc<RwLock<ServiceInways>>;

async fn handle_events(state: ServiceInwaysState, rx: Receiver<Config>) {
    loop {
        match rx.recv().await {
            Ok(new_config) => {
                let mut lock = state.write().await;
                *lock = new_config
                    .services
                    .into_iter()
                    .map(|(oin, services)| {
                        (
                            oin,
                            services
                                .into_iter()
                                .map(|service| {
                                    let services = service
                                        .inways
                                        .first()
                                        .and_then(|inway| {
                                            Url::parse(&format!(
                                                "{}{}/",
                                                inway.address, service.name
                                            ))
                                            .ok()
                                        })
                                        .map(Arc::new);

                                    (service.name, services)
                                })
                                .collect(),
                        )
                    })
                    .collect();

                log::info!("outway config updated");
            }
            Err(_) => {
                log::debug!("config channel closed");
                break;
            }
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
        let config = ServiceInwaysState::default();

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
            .tls_built_in_root_certs(false)
            .add_root_certificate(ca_cert)
            .identity(identity)
            .https_only(true)
            .http2_adaptive_window(true)
            .build()?;
        let with_config = warp::any().map(move || Arc::clone(&config));
        let with_client = warp::any().map(move || client.clone());
        let route =
            warp::any()
                .and(with_config)
                .and(with_client)
                .and(warp::path::param())
                .and(warp::path::param())
                .and(with_request!())
                .and_then(
                    |state: ServiceInwaysState,
                     client,
                     oin: String,
                     service: String,
                     request| async move {
                        let upstream = {
                            let lock = state.read().await;
                            lock.get(&oin).and_then(|services| {
                                services
                                    .get(&service)
                                    .map(|endpoint| endpoint.as_ref().map(Arc::clone))
                            })
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

        warp::serve(route).run(addr).await;

        Ok(())
    }
}
