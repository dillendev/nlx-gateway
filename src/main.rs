use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, ValueEnum};

use crate::inway::{ConfigPoller, Server};

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

    #[clap(long, env = "SELF_ADDRESS")]
    self_address: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();

    let opts = Opts::parse();
    let tls_config = tls::client_config(opts.tls_root_cert, opts.tls_cert, opts.tls_key).await?;

    match opts.mode {
        Mode::Inway => {
            let poller = ConfigPoller::new(
                opts.inway_name,
                opts.self_address,
                opts.management_api_address,
                tls_config,
            );
            let rx = poller.poll_start()?;

            log::info!("starting server on {}", opts.listen_address);

            let server = Server::new(rx);
            server.run(opts.listen_address).await?;
        }
        Mode::Outway => unreachable!("outway is not implemented yet"),
    }

    Ok(())
}
