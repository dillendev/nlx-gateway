use std::convert::Infallible;
use std::net::SocketAddr;

use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};

async fn hello_world(_req: Request<Body>) -> Result<Response<Body>, Infallible> {
    Ok(Response::new("Hello, World".into()))
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C signal handler");

    log::info!("shutdown received");
}

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

    let make_svc = make_service_fn(|_conn| async { Ok::<_, Infallible>(service_fn(hello_world)) });

    let server = Server::bind(&addr).serve(make_svc);
    let graceful = server.with_graceful_shutdown(shutdown_signal());

    log::info!("starting server on :3000");

    if let Err(e) = graceful.await {
        log::error!("server error: {}", e);
    }
}
