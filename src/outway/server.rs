use std::{net::SocketAddr, sync::Arc};

use async_channel::Receiver;
use hyper::Client;
use hyper_rustls::HttpsConnectorBuilder;
use rustls::{Certificate, ClientConfig, PrivateKey, RootCertStore};
use tokio::sync::RwLock;
use warp::Filter;

use crate::{filters::with_request, reverse_proxy, tls::TlsPair};

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
                                        .map(|inway| format!("{}{}/", inway.address, service.name))
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

        let cert_bundle_der = pem::parse_many(self.tls_pair.bundle())?
            .into_iter()
            .map(|pem| Certificate(pem.contents))
            .collect::<Vec<_>>();
        let key_der = pem::parse(self.tls_pair.key_pem)?.contents;

        let ca_cert_der = Certificate(pem::parse(self.tls_pair.root_pem)?.contents);
        let mut store = RootCertStore::empty();
        store.add(&ca_cert_der)?;

        let tls_config = ClientConfig::builder()
            .with_safe_default_cipher_suites()
            .with_safe_default_kx_groups()
            .with_safe_default_protocol_versions()?
            .with_root_certificates(store)
            .with_single_cert(cert_bundle_der, PrivateKey(key_der))?;
        let https = HttpsConnectorBuilder::new()
            .with_tls_config(tls_config)
            .https_or_http()
            .enable_http2()
            .build();
        let client = Client::builder()
            .http2_adaptive_window(true)
            .http2_only(true)
            .retry_canceled_requests(true)
            .build(https);
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
