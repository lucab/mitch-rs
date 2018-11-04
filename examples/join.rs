extern crate env_logger;
extern crate futures;
extern crate log;
extern crate mitch;
extern crate native_tls;
extern crate tokio;
extern crate tokio_tls;

use futures::prelude::*;
use mitch::errors;
use std::{net, time};
use tokio::{runtime, timer};

fn main() -> errors::Result<()> {
    // Initialize logging and tokio runtime.
    env_logger::Builder::new()
        .filter(Some("mitch"), log::LevelFilter::Info)
        .init();
    let mut runner = runtime::Runtime::new()?;

    // Initialize TCP listener.
    let port = 0;
    let sock_addr = net::SocketAddr::new(net::Ipv4Addr::LOCALHOST.into(), port);
    let tcp = tokio::net::tcp::TcpListener::bind(&sock_addr)?;
    println!("-> Peer listening at '{:?}'", tcp.local_addr());

    // Initialize TLS, server side.
    let peer_p12 = include_bytes!("../fixtures/certs/swarm-peer.p12");
    let server_cert = native_tls::Identity::from_pkcs12(peer_p12, "")?;
    let tls_server = native_tls::TlsAcceptor::builder(server_cert).build()?;

    // Initialize TLS, client side.
    let ca_bytes = include_bytes!("../fixtures/certs/ca.pem");
    let ca_cert = native_tls::Certificate::from_pem(ca_bytes)?;
    let client_cert = native_tls::Identity::from_pkcs12(peer_p12, "")?;
    let tls_client = native_tls::TlsConnector::builder()
        .identity(client_cert)
        .add_root_certificate(ca_cert)
        .build()?;

    // Watch for and print group membership events.
    let (tx, rx) = futures::sync::mpsc::channel(200);
    let fut_events = rx.for_each(|ev| Ok(println!(" * Swarm event: {:?}", ev)));
    runner.spawn(fut_events);

    // Start running a local peer.
    let cfg = mitch::MitchConfig::default()
        .listener(Some(tcp))
        .tls_acceptor(Some(tls_server))
        .tls_connector(Some(tls_client))
        .notifications_channel(Some(tx));
    let fut_group = cfg.build();
    let swimgroup = runner.block_on(fut_group)?;

    // Join a remote group.
    let dest_port = 8888;
    let dest_addr = net::SocketAddr::new(net::Ipv4Addr::LOCALHOST.into(), dest_port);
    let fut_join = swimgroup
        .join(vec![dest_addr], None)
        .map_err(|e| eprintln!("-> Swarm join error: {}", e));
    runner.spawn(fut_join);

    // Keep running for 15 secs.
    let delay_secs = 15;
    let delay = time::Instant::now() + time::Duration::from_secs(delay_secs);
    let fut_stop = swimgroup.stop();
    let fut_delayed_stop = timer::Delay::new(delay)
        .inspect(|_| println!("-> Stopping peer after a fixed time"))
        .map_err(|e| format!("{}", e).into())
        .and_then(|_| fut_stop);
    runner.block_on_all(fut_delayed_stop)
}
