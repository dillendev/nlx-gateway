use std::{net::SocketAddr, sync::Arc};

use anyhow::Result;
use rocket::routes;
use tokio::sync::{mpsc::Receiver, RwLock};

use super::{config, Event};

type InwayConfig = Arc<RwLock<config::InwayConfig>>;

async fn handle_events(config: InwayConfig, mut rx: Receiver<Event>) {
    while let Some(event) = rx.recv().await {
        match event {
            Event::InwayRegistered => log::info!("inway registered"),
            Event::ConfigUpdated(new_config) => {
                let mut lock = config.write().await;
                *lock = new_config;

                log::info!("inway config updated");
            }
        }
    }
}

pub struct Server {
    rx: Receiver<Event>,
}

impl Server {
    pub fn new(rx: Receiver<Event>) -> Self {
        Self { rx }
    }

    pub async fn run(self, addr: SocketAddr) -> Result<()> {
        let figment = rocket::Config::figment()
            .merge(("address", addr.ip()))
            .merge(("port", addr.port()));

        let config = InwayConfig::default();

        // Handle config changes
        tokio::spawn(handle_events(config.clone(), self.rx));

        let _ = rocket::custom(figment)
            .mount("/.nlx", routes![routes::health])
            .manage(config)
            .launch()
            .await?;

        Ok(())
    }
}

mod routes {
    use rocket::{get, serde::json::Json, State};
    use serde::Serialize;

    use super::InwayConfig;

    #[derive(Serialize)]
    pub struct Health {
        pub healthy: bool,
        pub version: String,
    }

    #[get("/health/<service>")]
    pub async fn health(config: &State<InwayConfig>, service: String) -> Json<Health> {
        let healthy = { config.read().await.services.contains_key(&service) };

        Json(Health {
            healthy,
            version: "".to_string(),
        })
    }
}
