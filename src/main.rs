use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use pb::management_client::ManagementClient;
use tokio::sync::mpsc::channel;
use tonic::transport::{Channel, ClientTlsConfig};

use crate::inway::{Broadcast, ConfigPoller, Server};

mod inway;
mod tls;

pub mod pb {
    tonic::include_proto!("nlx.management");
}

#[derive(ValueEnum, Clone)]
enum Mode {
    Inway,
    Outway,
}

#[derive(Parser)]
struct Opts {
    #[clap(long, env = "MODE")]
    mode: Mode,

    #[clap(long, env = "INWAY_NAME")]
    inway_name: String,

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

    #[clap(long, env = "TLS_NLX_ROOT_CERT")]
    tls_nlx_root_cert: PathBuf,

    #[clap(long, env = "TLS_ORG_CERT")]
    tls_org_cert: PathBuf,

    #[clap(long, env = "TLS_ORG_KEY")]
    tls_org_key: PathBuf,

    #[clap(long, env = "SELF_ADDRESS")]
    self_address: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();

    let opts = Opts::parse();
    let internal_tls_config =
        tls::client_config(opts.tls_root_cert, opts.tls_cert, opts.tls_key).await?;
    let org_tls_pair =
        tls::read_pair(opts.tls_nlx_root_cert, opts.tls_org_cert, opts.tls_org_key).await?;

    let management =
        ManagementClient::new(connect(opts.management_api_address, internal_tls_config).await?);

    match opts.mode {
        Mode::Inway => {
            let (tx, rx) = channel(10);

            let poller = ConfigPoller::new(management.clone(), opts.inway_name.clone());
            poller.poll_start(tx)?;

            let broadcast = Broadcast::new(management, opts.inway_name, opts.self_address);
            broadcast.broadcast_start()?;

            log::info!("starting server on {}", opts.listen_address);

            let server = Server::new(org_tls_pair, rx);
            server.run(opts.listen_address).await?;
        }
        Mode::Outway => unreachable!("outway is not implemented yet"),
    }

    Ok(())
}

async fn connect(addr: String, tls_config: ClientTlsConfig) -> Result<Channel> {
    let endpoint = Channel::from_shared(addr)?
        .tls_config(tls_config)
        .with_context(|| "failed to setup TLS config")?;

    log::debug!("connecting to: {}", endpoint.uri());

    Ok(endpoint.connect().await?)
}
