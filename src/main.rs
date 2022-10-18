use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};

use crate::config_poller::ConfigPoller;

mod config_poller;
mod tls;

pub mod pb {
    tonic::include_proto!("nlx.management");
}

async fn hello_world(_req: Request<Body>) -> Result<Response<Body>, Infallible> {
    Ok(Response::new("Hello, World".into()))
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C signal handler");

    log::info!("shutdown received");
}

#[derive(Parser)]
struct Opts {
    #[clap(long, env = "LISTEN_ADDRESS")]
    listen_address: SocketAddr,

    #[clap(long, env = "MANAGEMENT_API_ADDRESS")]
    management_api_address: String,

    #[clap(long, env = "TLS_ROOT_CERT")]
    tls_root_cert: PathBuf,

    #[clap(long, env = "TLS_CERT")]
    tls_cert: PathBuf,

    #[clap(long, env = "TLS_KEY")]
    tls_key: PathBuf,

    #[clap(long, env = "SELF_ADDRESS")]
    self_address: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();

    let opts = Opts::parse();
    let tls_config = tls::client_config(opts.tls_root_cert, opts.tls_cert, opts.tls_key).await?;

    let poller = ConfigPoller::new(opts.self_address, opts.management_api_address, tls_config);
    let mut rx = poller.poll_start()?;

    // @TODO: handle config properly
    if let Some(event) = rx.recv().await {
        log::debug!("received event: {:?}", event);
    }

    let make_svc = make_service_fn(|_conn| async { Ok::<_, Infallible>(service_fn(hello_world)) });
    let server = Server::bind(&opts.listen_address).serve(make_svc);
    let graceful = server.with_graceful_shutdown(shutdown_signal());

    log::info!("starting server on {}", opts.listen_address);

    graceful.await?;

    Ok(())
}
