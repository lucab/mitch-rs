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
    let port = 12421;
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

    // Watch for and print swarm membership events.
    let (tx, rx) = futures::sync::mpsc::channel(200);
    let fut_events = rx.for_each(|ev| Ok(println!(" * Swarm event: {:?}", ev)));
    runner.spawn(fut_events);

    // Start running a swarm, waiting for other peers to reach us.
    let cfg = mitch::MitchConfig::default()
        .listener(Some(tcp))
        .tls_acceptor(Some(tls_server))
        .tls_connector(Some(tls_client))
        .notifications_channel(Some(tx));
    let fut_swarm = cfg.build();
    let swarm = runner.block_on(fut_swarm)?;

    // Snapshot and print swarm members every 5 secs.
    let membership = futures::stream::repeat(swarm.membership());
    let period = time::Duration::from_secs(5);
    let fut_dump = tokio::timer::Interval::new_interval(period)
        .from_err()
        .zip(membership)
        .and_then(move |(_, mems)| mems.snapshot())
        .map_err(|_| ())
        .for_each(|mems| Ok(println!(" * Swarm members: {:#?}", mems)));
    runner.spawn(fut_dump);

    // Keep running for 45 secs.
    let delay = time::Instant::now() + time::Duration::from_secs(45);
    let fut_stop = swarm.stop();
    let fut_delayed_stop = timer::Delay::new(delay)
        .map_err(|e| format!("{}", e).into())
        .and_then(|_| fut_stop);
    runner.block_on_all(fut_delayed_stop)
}
