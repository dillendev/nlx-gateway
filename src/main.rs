use std::path::PathBuf;
use std::{net::SocketAddr, time::Duration};

use anyhow::{Context, Result};
use async_channel::unbounded;
use clap::{Parser, ValueEnum};
use futures_util::TryFutureExt;
use pb::{
    directory::directory_client::DirectoryClient, management::management_client::ManagementClient,
};
use tls::TlsPair;
use tonic::transport::{Channel, ClientTlsConfig};

use crate::poller::Poller;

mod backoff;
mod filters;
mod inway;
mod outway;
mod poller;
mod reverse_proxy;
mod tls;

pub mod pb {
    pub mod management {
        tonic::include_proto!("nlx.management");
    }

    pub mod directory {
        tonic::include_proto!("directoryapi");
    }
}

#[derive(ValueEnum, Clone)]
enum Mode {
    Inway,
    Outway,
}

#[derive(Parser)]
struct Opts {
    #[clap(subcommand)]
    cmd: Cmd,

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

    #[clap(long, env = "DIRECTORY_ADDRESS")]
    directory_address: String,

    #[clap(long, env = "MANAGEMENT_API_ADDRESS")]
    management_api_address: String,
}

#[derive(Parser)]
pub enum Cmd {
    Inway(InwayOpts),
    Outway(OutwayOpts),
}

#[derive(Parser)]
pub struct InwayOpts {
    #[clap(long, env = "INWAY_NAME")]
    name: String,

    #[clap(long, env = "LISTEN_ADDRESS")]
    listen_address: SocketAddr,

    #[clap(long, env = "SELF_ADDRESS")]
    self_address: String,
}

#[derive(Parser)]
pub struct OutwayOpts {
    #[clap(long, env = "OUTWAY_NAME")]
    name: String,

    #[clap(long, env = "LISTEN_ADDRESS")]
    listen_address: SocketAddr,
}

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();

    let opts = Opts::parse();

    let (internal_tls_config, org_tls_pair) = tokio::try_join!(
        tls::client_config(opts.tls_root_cert, opts.tls_cert, opts.tls_key),
        TlsPair::from_files(opts.tls_nlx_root_cert, opts.tls_org_cert, opts.tls_org_key),
    )?;

    let (management, directory) = tokio::try_join!(
        connect(opts.management_api_address, internal_tls_config).map_ok(ManagementClient::new),
        connect(opts.directory_address, org_tls_pair.client_config()).map_ok(DirectoryClient::new),
    )?;

    match opts.cmd {
        Cmd::Inway(opts) => {
            let ((tx, rx), (tx2, rx2)) = (unbounded(), unbounded());

            let mut config_poller = inway::ConfigPoller::new(management.clone(), opts.name.clone());
            config_poller.subscribe(tx);
            config_poller.subscribe(tx2);

            let poller = Poller::new(config_poller, Duration::from_secs(10));
            poller.poll_start();

            let broadcast =
                inway::Broadcast::new(management, directory, opts.name, opts.self_address);
            broadcast.broadcast_start(rx2);

            log::info!("starting server on {}", opts.listen_address);

            let server = inway::Server::new(org_tls_pair, rx);
            server.run(opts.listen_address).await?;
        }
        Cmd::Outway(opts) => {
            let (tx, rx) = unbounded();

            let poller = Poller::new(
                outway::ConfigPoller::new(directory.clone(), tx),
                Duration::from_secs(10),
            );
            poller.poll_start();

            let broadcast = outway::Broadcast::new(
                management,
                directory,
                org_tls_pair.public_key_pem()?,
                opts.name,
            );
            broadcast.broadcast_start()?;

            log::info!("starting server on {}", opts.listen_address);

            let server = outway::Server::new(org_tls_pair, rx);
            server.run(opts.listen_address).await?;
        }
    }

    Ok(())
}

async fn connect(addr: String, tls_config: ClientTlsConfig) -> Result<Channel> {
    let endpoint = Channel::from_shared(addr)?
        .tls_config(tls_config)
        .with_context(|| "failed to setup TLS config")?
        .keep_alive_while_idle(true)
        .http2_adaptive_window(true)
        .http2_keep_alive_interval(Duration::from_secs(30));

    log::debug!("connecting to: {}", endpoint.uri());

    Ok(endpoint.connect().await?)
}
