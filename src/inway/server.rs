use std::{net::SocketAddr, sync::Arc};

use anyhow::Result;
use reqwest::Client;
use rocket::{
    config::{MutualTls, TlsConfig},
    routes,
};
use tokio::sync::{
    broadcast::{error::RecvError, Receiver},
    RwLock,
};

use crate::tls::TlsPair;

use super::{config, Event};

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

pub struct Server {
    tls_pair: TlsPair,
    rx: Receiver<Event>,
}

impl Server {
    pub fn new(tls_pair: TlsPair, rx: Receiver<Event>) -> Self {
        Self { tls_pair, rx }
    }

    pub async fn run(self, addr: SocketAddr) -> Result<()> {
        let certs_bundle = self.tls_pair.bundle();
        let figment = rocket::Config::figment()
            .merge(("address", addr.ip()))
            .merge(("port", addr.port()))
            .merge((
                "tls",
                TlsConfig::from_bytes(&certs_bundle, &self.tls_pair.key_pem)
                    .with_mutual(MutualTls::from_bytes(&certs_bundle).mandatory(true)),
            ));

        let config = InwayConfig::default();

        // Handle config changes
        tokio::spawn(handle_events(Arc::clone(&config), self.rx));

        let _ = rocket::custom(figment)
            .mount(
                "/",
                routes![
                    routes::head::proxy,
                    routes::options::proxy,
                    routes::get::proxy,
                    routes::post::proxy,
                    routes::put::proxy,
                    routes::patch::proxy,
                    routes::delete::proxy,
                ],
            )
            .mount("/.nlx", routes![routes::health])
            .manage(Arc::clone(&config))
            .manage(Client::new())
            .launch()
            .await?;

        Ok(())
    }
}

mod routes {
    use std::io;

    use bytes::Bytes;
    use futures_util::Stream;
    use reqwest::Client;
    use rocket::{
        get,
        http::{uri::Origin, RawStr, Status},
        response::status,
        serde::json::Json,
        Data, State,
    };
    use serde::Serialize;

    use super::InwayConfig;
    use crate::inway::reverse_proxy::{self, Request};
    use crate::inway::stream::ByteStreamResponse;

    #[derive(Serialize)]
    pub struct Health {
        pub healthy: bool,
        pub version: String,
    }

    macro_rules! proxy_impl {
        ($method:ident) => {
            pub mod $method {
                use super::*;

                #[rocket::$method("/<service>/<_..>", data = "<body>")]
                pub async fn proxy<'r>(
                    service: String,
                    mut request: Request<'r>,
                    uri: &'r Origin<'_>,
                    body: Data<'r>,
                    config: &State<InwayConfig>,
                    http: &State<Client>,
                ) -> Result<
                    ByteStreamResponse<'r, impl Stream<Item = Result<Bytes, io::Error>>>,
                    status::Custom<String>,
                > {
                    let upstream = {
                        let lock = config.read().await;
                        lock.services
                            .get(&service)
                            .map(|service| service.endpoint_url.clone())
                    };

                    match upstream {
                        Some(endpoint) => {
                            let raw_path = uri.path();
                            let path = raw_path
                                .strip_prefix('/')
                                .and_then(|p| p.strip_prefix(service.as_str()))
                                .map(|p| p.strip_suffix('/').unwrap_or(p))
                                .unwrap_or_else(|| RawStr::new(""));

                            request.set_path(path.to_string());

                            log::debug!("proxy [{}] {}", service, request);

                            reverse_proxy::handle(http.inner().clone(), request, &endpoint, body).await
                        }
                        None => {
                            log::debug!("service {} not available", service);

                            Err(status::Custom(
                                Status::NotFound,
                                format!("service {} not available", service),
                            ))
                        }
                    }
                }
            }
        };

        ($($method:ident),+) => {
            $(proxy_impl!($method);)+
        };
    }

    // Creates proxy routes for each HTTP method (unfortunately there is no `any` method in Rocket)
    proxy_impl!(head, options, get, post, delete, put, patch);

    #[get("/health/<service>")]
    pub async fn health(config: &State<InwayConfig>, service: String) -> Json<Health> {
        let healthy = { config.read().await.services.contains_key(&service) };

        Json(Health {
            healthy,
            version: String::new(),
        })
    }
}
