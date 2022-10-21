use std::{net::SocketAddr, sync::Arc};

use anyhow::Result;
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
            .mount("/", routes![routes::proxy_get])
            .mount("/.nlx", routes![routes::health])
            .manage(Arc::clone(&config))
            .launch()
            .await?;

        Ok(())
    }
}

mod routes {
    use std::{
        io::{self, ErrorKind},
        net::IpAddr,
    };

    use bytes::Bytes;
    use futures_util::{Stream, StreamExt};
    use rocket::{
        get,
        http::{uri::Origin, Method},
        response::stream::stream,
        serde::json::Json,
        State,
    };
    use serde::Serialize;

    use crate::inway::stream::ByteStream;

    use super::InwayConfig;

    #[derive(Serialize)]
    pub struct Health {
        pub healthy: bool,
        pub version: String,
    }

    pub struct ServiceInfo {
        name: String,
        endpoint: String,
    }

    impl ServiceInfo {
        pub fn new(name: String, endpoint: String) -> Self {
            Self { name, endpoint }
        }
    }

    pub async fn proxy(
        method: Method,
        service: ServiceInfo,
        uri: &Origin<'_>,
        _ip_addr: IpAddr,
    ) -> ByteStream<impl Stream<Item = Result<Bytes, io::Error>>> {
        let path = uri
            .path()
            .strip_prefix('/')
            .and_then(|p| p.strip_prefix(service.name.as_str()))
            .map(|s| s.to_string())
            .unwrap_or_default();

        log::debug!("proxy [{}] {} {}", service.name, method, path);

        ByteStream(stream! {
            let response = reqwest::get(format!("{}/{}", service.endpoint, path))
                .await
                .map_err(|e| io::Error::new(ErrorKind::Other, e))?;
            let mut response_stream = response.bytes_stream();

            while let Some(item) = response_stream.next().await {
                yield item.map_err(|e| io::Error::new(ErrorKind::Other, e));
            }
        })
    }

    #[get("/<service>/<_..>")]
    pub async fn proxy_get(
        service: String,
        uri: &Origin<'_>,
        ip_addr: IpAddr,
        config: &State<InwayConfig>,
    ) -> ByteStream<impl Stream<Item = Result<Bytes, io::Error>>> {
        let backend = {
            let lock = config.read().await;
            lock.services
                .get(&service)
                .map(|service| service.endpoint_url.clone())
        };

        match backend {
            Some(endpoint) => {
                let info = ServiceInfo::new(service, endpoint);
                proxy(Method::Get, info, uri, ip_addr).await
            }
            // @TODO: error handling
            None => {
                log::debug!("service {} not available", service);
                panic!("fail")
            }
        }
    }

    #[get("/health/<service>")]
    pub async fn health(config: &State<InwayConfig>, service: String) -> Json<Health> {
        let healthy = { config.read().await.services.contains_key(&service) };

        Json(Health {
            healthy,
            version: String::new(),
        })
    }
}
