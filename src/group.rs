use super::{errors, observer, protomitch, protomitch_pb, reactor, MemberInfo};
use byteorder::{NetworkEndian, ReadBytesExt};
use futures::prelude::*;
use futures::{future, stream, sync::mpsc, sync::oneshot};
use native_tls;
use std::{io, mem, net, time};
use stream_cancel;
use tokio;
use tokio_tls;

use stream_cancel::{Trigger, Valve};
use tokio::executor::Executor;
use tokio::prelude::*;

pub(crate) type TlsTcpStream = tokio_tls::TlsStream<tokio::net::TcpStream>;

pub(crate) type FutureSpawn = Box<Future<Item = (), Error = ()> + Send + 'static>;
pub(crate) type FutureTLS =
    Box<Future<Item = TlsTcpStream, Error = errors::Error> + Send + 'static>;
pub(crate) type FutureMitchMsg = Box<
    Future<Item = (TlsTcpStream, protomitch_pb::MitchMsg), Error = errors::Error> + Send + 'static,
>;

/// Configuration builder for a swarm member.
pub struct MitchConfig {
    listener: Option<tokio::net::tcp::TcpListener>,
    local: MemberInfo,
    protocol_period: time::Duration,
    tls_acceptor: Option<native_tls::TlsAcceptor>,
    tls_connector: Option<native_tls::TlsConnector>,
    notifications_tx: Option<mpsc::Sender<observer::SwarmNotification>>,
}

impl Default for MitchConfig {
    fn default() -> Self {
        Self {
            listener: None,
            notifications_tx: None,
            local: MemberInfo::default(),
            protocol_period: time::Duration::from_secs(1),
            tls_acceptor: None,
            tls_connector: None,
        }
    }
}

impl MitchConfig {
    /// Set metadata to be advertised by local member.
    pub fn local_metadata(mut self, metadata: Vec<u8>) -> errors::Result<Self> {
        if metadata.len() > MemberInfo::MAX_METADATA {
            bail!("metadata larger than maximum size");
        }
        self.local.metadata = metadata;
        Ok(self)
    }

    /// Set a custom TCP listener.
    pub fn listener(mut self, listener: Option<tokio::net::tcp::TcpListener>) -> Self {
        self.listener = listener;
        self
    }

    /// Set a custom TLS acceptor (with server certificate).
    pub fn tls_acceptor(mut self, tls_acceptor: Option<native_tls::TlsAcceptor>) -> Self {
        self.tls_acceptor = tls_acceptor;
        self
    }

    /// Set a custom TLS connector (with client certificate).
    pub fn tls_connector(mut self, tls_connector: Option<native_tls::TlsConnector>) -> Self {
        self.tls_connector = tls_connector;
        self
    }

    /// Set a channel where to receive notifications about swarm changes.
    pub fn notifications_channel(
        mut self,
        tx: Option<mpsc::Sender<observer::SwarmNotification>>,
    ) -> Self {
        self.notifications_tx = tx;
        self
    }

    /// Finalize and create the node for the swarm.
    pub fn build(self) -> super::FutureSwarm {
        // Cancellation helpers for internal tasks.
        let (trigger, valve) = stream_cancel::Valve::new();
        // TLS, client-side.
        let tls_connector = match self.tls_connector {
            Some(tc) => tc,
            None => {
                let fut_err = future::err("Client TLS configuration missing".into());
                return Box::new(fut_err);
            }
        };
        // TLS, server-side
        let tls_acceptor = match self.tls_acceptor {
            Some(ta) => ta,
            None => {
                let fut_err = future::err("Server TLS configuration missing".into());
                return Box::new(fut_err);
            }
        };

        let (tx, rx) = mpsc::channel(200);
        let cluster = SwarmMembers {
            members: Some(vec![]),
            reactor_tx: tx,
            events_rx: Some(rx),
        };

        let swarm = MitchSwarm {
            notifications_tx: self.notifications_tx,
            local: self.local,
            members: cluster,
            period: self.protocol_period,
            trigger: Some(trigger),
            valve,
            tls_connector,
        };

        swarm.start(self.listener, tls_acceptor)
    }
}

#[derive(Debug)]
pub(crate) struct SwarmMembers {
    pub(crate) members: Option<Vec<MemberInfo>>,
    pub(crate) reactor_tx: mpsc::Sender<reactor::Event>,
    pub(crate) events_rx: Option<mpsc::Receiver<reactor::Event>>,
}

/// Local swarm member.
pub struct MitchSwarm {
    // Local node information.
    pub(crate) local: MemberInfo,
    // Swarm peers and membership handling.
    pub(crate) members: SwarmMembers,
    // TLS, client-side.
    pub(crate) tls_connector: native_tls::TlsConnector,
    // Notitications to external observers.
    pub(crate) notifications_tx: Option<mpsc::Sender<observer::SwarmNotification>>,
    // Protocol period.
    pub(crate) period: time::Duration,
    pub(crate) trigger: Option<Trigger>,
    pub(crate) valve: Valve,
}

impl MitchSwarm {
    // Main server task.
    fn serve_incoming(
        &mut self,
        listener: Option<tokio::net::TcpListener>,
        tls_acceptor: tokio_tls::TlsAcceptor,
    ) -> FutureSpawn {
        // Initialize TCP listener.
        let listener = match listener {
            Some(l) => l,
            None => {
                let tcp_listener = tokio::net::tcp::TcpListener::bind(&self.local.target);
                match tcp_listener {
                    Ok(tl) => tl,
                    Err(e) => {
                        error!("{}", e);
                        return Box::new(future::err(()));
                    }
                }
            }
        };
        self.local.target = match listener.local_addr() {
            Ok(addr) => addr,
            Err(e) => {
                error!("unable to get local socket address: {}", e);
                return Box::new(future::err(()));
            }
        };

        let tx = self.members.reactor_tx.clone();
        let fut_server = self
            .valve
            .wrap(listener.incoming().map_err(errors::Error::from))
            .and_then(move |tcp| {
                let tx = tx.clone();
                let fut = tls_acceptor
                    .accept(tcp)
                    .map_err(errors::Error::from)
                    .inspect(|tls| trace!("TLS accepted: {:?}", tls.get_ref().get_ref()))
                    .and_then(|tls| read_protomsg(tls, 5))
                    .and_then(move |(tls, msg)| dispatch(tls, msg, tx, 5))
                    .map_err(|e| errors::Error::from(format!("serve_incoming error: {}", e)));
                Ok(fut)
            }).buffer_unordered(10)
            .then(|res| match res {
                Ok(r) => Ok(Some(r)),
                Err(err) => {
                    error!("server error: {}", err);
                    Ok(None)
                }
            }).filter_map(|x| x)
            .for_each(|_| Ok(()));
        Box::new(fut_server)
    }

    // Failure detector task.
    // TODO(lucab): unused and unfinished.
    fn failure_detector(&self) -> FutureSpawn {
        let tls_connector = self.tls_connector.clone();
        let fut_detector = self
            .valve
            .wrap(tokio::timer::Interval::new_interval(self.period).from_err())
            .and_then(move |_| {
                // XXX
                let dst = net::SocketAddr::new(net::Ipv4Addr::LOCALHOST.into(), 6666);
                let fut = tls_connect(tls_connector.clone(), &dst, 8)
                    .and_then(move |tls| {
                        trace!("Pinging {}", dst);
                        protomitch::ping()
                            .and_then(|payload| tokio::io::write_all(tls, payload).from_err())
                    }).and_then(|(tls, _)| {
                        let buf = Vec::<u8>::new();
                        tokio::io::read_to_end(tls, buf).from_err()
                    }).timeout(time::Duration::from_secs(7))
                    .map_err(|e| errors::Error::from(format!("detector connection error: {}", e)));
                Ok(fut)
            }).buffer_unordered(10)
            .then(|res| match res {
                Ok(r) => Ok(Some(r)),
                Err(err) => {
                    error!("detector error: {}", err);
                    Ok(None)
                }
            }).filter_map(|x| x)
            .for_each(|_| Ok(()));
        Box::new(fut_detector)
    }

    // Start swarming.
    fn start(
        self,
        listener: Option<tokio::net::TcpListener>,
        tls_acceptor: native_tls::TlsAcceptor,
    ) -> super::FutureSwarm {
        let tls = tokio_tls::TlsAcceptor::from(tls_acceptor);
        let fut_start = future::ok(self)
            .inspect(|sw| {
                info!(
                    "Starting local node {}, nickname \"{}\"",
                    sw.local.id, sw.local.nickname
                )
            }).and_then(move |mut sw| {
                // Internal tasks.
                let _fut_detector = sw.failure_detector();
                let fut_server = sw.serve_incoming(listener, tls);
                let fut_mems = super::reactor::membership_task(&mut sw);

                // Spawn all internal tasks.
                //let detector_task = tokio::executor::DefaultExecutor::current().spawn(fut_detector);
                let server_task = tokio::executor::DefaultExecutor::current().spawn(fut_server);
                let membership_task = tokio::executor::DefaultExecutor::current().spawn(fut_mems);

                // Chain all results and pass the MitchSwarm through.
                future::ok(sw)
                    //.then(|g| detector_task.and(sw))
                    .then(|sw| server_task.and(sw))
                    .then(|sw| membership_task.and(sw))
                    .from_err()
            }).inspect(|_| debug!("MitchSwarm started"));
        Box::new(fut_start)
    }

    /// Return a membership observer.
    pub fn membership(&self) -> observer::Membership {
        observer::Membership {
            reactor_tx: self.members.reactor_tx.clone(),
        }
    }

    /// Stop this swarm member.
    pub fn stop(self) -> super::FutureTask {
        let fut_stop = future::ok(self)
            .and_then(|swarm| {
                let (tx, rx) = oneshot::channel();
                let ch = swarm.members.reactor_tx.clone();
                ch.send(reactor::Event::Shutdown(tx))
                    .and_then(|mut ch| ch.close())
                    .map_err(|_| errors::Error::from("stop: send error"))
                    .and_then(|_| rx.from_err())
                    .map(move |_| swarm)
            }).and_then(|mut swarm| {
                // Cancel all internal tasks.
                if let Some(trigger) = swarm.trigger.take() {
                    trigger.cancel();
                }
                // Consume this swarm member.
                drop(swarm);
                Ok(())
            }).inspect(|_| debug!("MitchSwarm stopped"));
        Box::new(fut_stop)
    }

    /// Join an existing swarm, synchronizing the initial set of peers.
    pub fn join(
        &self,
        dests: Vec<net::SocketAddr>,
        initial_pull_size: Option<u32>,
    ) -> super::FutureTask {
        let tls_connector = self.tls_connector.clone();
        let tx = self.members.reactor_tx.clone();
        let local = self.local.clone();
        let par_reqs = dests.len().saturating_add(1);
        let fut_join = stream::iter_ok(dests)
            .and_then(move |dst| {
                let tls = tls_connector.clone();
                trace!("Joining {:?}", &dst);
                let info = local.clone();
                tls_connect(tls.clone(), &dst, 8)
                    .and_then(move |tls| {
                        protomitch::join(&info)
                            .and_then(|payload| tokio::io::write_all(tls, payload).from_err())
                    }).timeout(time::Duration::from_secs(10))
                    .map_err(|e| errors::Error::from(format!("ping error: {}", e)))
                    .and_then(move |_| Ok((tls, dst)))
            }).and_then(move |(tls_connector, dst)| {
                trace!("Pulling from {:?}", &dst);
                tls_connect(tls_connector.clone(), &dst, 8)
                    .and_then(move |tls| {
                        protomitch::pull(initial_pull_size)
                            .and_then(|payload| tokio::io::write_all(tls, payload).from_err())
                    }).timeout(time::Duration::from_secs(10))
                    .map_err(|e| errors::Error::from(format!("pull error: {}", e)))
            }).and_then(move |(tls, _)| {
                trace!("Syncing");
                read_protomsg(tls, 5)
            }).and_then(|(_tls, msg)| {
                trace!("Parsing protobuf");
                match msg.value {
                    Some(protomitch_pb::mitch_msg::Value::Sync(s)) => Ok(s),
                    _ => Err("foo".into()),
                }
            }).and_then(move |msg| {
                let reactor_tx = tx.clone();
                futures::stream::iter_ok(msg.members).for_each(move |member| {
                    let reactor_tx = reactor_tx.clone();
                    future::result(protomitch::join_info(member)).and_then(move |member_info| {
                        let join_event = reactor::Event::Join(member_info);
                        reactor_tx
                            .clone()
                            .send(join_event)
                            .map(|_| ())
                            .map_err(|_| "sync error".into())
                    })
                })
            }).map(|_| Ok(()))
            .buffer_unordered(par_reqs)
            .for_each(|_| Ok(()))
            .from_err();
        Box::new(fut_join)
    }
}

pub(crate) fn dispatch(
    tls: tokio_tls::TlsStream<tokio::net::TcpStream>,
    msg: protomitch_pb::MitchMsg,
    tx: mpsc::Sender<reactor::Event>,
    _timeout: u64,
) -> FutureTLS {
    use protomitch_pb::mitch_msg::Value;

    let fut_void = future::ok((tls, msg))
        .and_then(|(tls, msg)| match msg.value {
            None => future::err("dispatch: None value".into()),
            Some(value) => future::ok((tls, value)),
        }).and_then(|(tls, value)| match value {
            Value::Failed(msg) => process_failed(tls, msg, tx),
            Value::Join(msg) => process_join(tls, msg, tx),
            Value::Ping(_) => process_ping(tls),
            Value::Pull(msg) => process_pull(tls, msg, tx),
            Value::Sync(msg) => process_sync(tls, msg, tx),
            // _ => Box::new(future::err("dispatch: unknown value".into())),
        });
    Box::new(fut_void)
}

pub(crate) fn process_ping(tls: tokio_tls::TlsStream<tokio::net::TcpStream>) -> FutureTLS {
    let fut_tls = future::ok(tls);
    Box::new(fut_tls)
}

pub(crate) fn process_failed(
    tls: tokio_tls::TlsStream<tokio::net::TcpStream>,
    msg: protomitch_pb::FailedMsg,
    tx: mpsc::Sender<reactor::Event>,
) -> FutureTLS {
    let fut_tls = future::ok((tls, msg, tx)).and_then(|(tls, msg, tx)| {
        let event = reactor::Event::Failed(msg.id);
        tx.send(event)
            .map_err(|e| errors::Error::from(format!("process_failed error: {}", e)))
            .and_then(move |_| Ok(tls))
    });
    Box::new(fut_tls)
}

pub(crate) fn process_sync(
    tls: tokio_tls::TlsStream<tokio::net::TcpStream>,
    msg: protomitch_pb::SyncMsg,
    tx: mpsc::Sender<reactor::Event>,
) -> FutureTLS {
    let fut_tls = future::ok((tls, msg, tx)).and_then(|(tls, msg, tx)| {
        futures::stream::iter_ok(msg.members)
            .and_then(move |member| {
                let ch = tx.clone();
                let mi = MemberInfo {
                    id: member.id,
                    nickname: member.nickname,
                    min_proto: member.min_proto,
                    max_proto: member.max_proto,
                    target: ([127, 0, 0, 1], 0).into(),
                    metadata: member.metadata,
                };
                let event = reactor::Event::Join(mi);
                ch.send(event)
                    .map_err(|e| errors::Error::from(format!("join error: {}", e)))
            }).for_each(|_| Ok(()))
            .map_err(|e| errors::Error::from(format!("join error: {}", e)))
            .and_then(move |_| Ok(tls))
    });
    Box::new(fut_tls)
}

pub(crate) fn process_join(
    tls: tokio_tls::TlsStream<tokio::net::TcpStream>,
    msg: protomitch_pb::JoinMsg,
    tx: mpsc::Sender<reactor::Event>,
) -> FutureTLS {
    let target = tls.get_ref().get_ref().peer_addr();
    let fut_tls = future::result(target)
        .from_err()
        .map(|target| (tls, msg, tx, target))
        .and_then(|(tls, msg, tx, target)| {
            let mi = MemberInfo {
                id: msg.id,
                nickname: msg.nickname,
                min_proto: msg.min_proto,
                max_proto: msg.max_proto,
                target,
                metadata: msg.metadata,
            };
            let event = reactor::Event::Join(mi);
            tx.send(event)
                .map_err(|e| errors::Error::from(format!("join error: {}", e)))
                .and_then(move |_| Ok(tls))
        });
    Box::new(fut_tls)
}

pub(crate) fn process_pull(
    tls: tokio_tls::TlsStream<tokio::net::TcpStream>,
    msg: protomitch_pb::PullMsg,
    ch: mpsc::Sender<reactor::Event>,
) -> FutureTLS {
    let fut_tls = future::ok((tls, msg, ch))
        .and_then(|(tls, _, ch)| {
            let (tx, rx) = oneshot::channel();
            let ev = reactor::Event::Snapshot(tx);
            ch.send(ev)
                .map(|_| (tls, rx))
                .map_err(|e| errors::Error::from(format!("pull error: {}", e)))
        }).and_then(|(tls, rx)| {
            rx.and_then(move |sync| Ok((tls, sync)))
                .map_err(|e| errors::Error::from(format!("sync error: {}", e)))
        }).and_then(|(tls, sync)| {
            protomitch::sync(&sync)
                .and_then(|payload| tokio::io::write_all(tls, payload).from_err())
        }).map(|(tls, _)| tls);
    Box::new(fut_tls)
}

pub(crate) fn tls_connect(
    tls_connector: native_tls::TlsConnector,
    dst: &net::SocketAddr,
    timeout: u64,
) -> FutureTLS {
    let fut_tls_connect = tokio::net::TcpStream::connect(dst)
        .map_err(errors::Error::from)
        .and_then(move |tcp| {
            let cx = tokio_tls::TlsConnector::from(tls_connector);
            cx.connect("mitch-rs", tcp).from_err()
        }).inspect(|_| trace!("TLS connected"))
        .timeout(time::Duration::from_secs(timeout))
        .map_err(|e| errors::Error::from(format!("tls_connect error: {}", e)));
        ;

    Box::new(fut_tls_connect)
}

pub(crate) fn read_protomsg(
    tls: tokio_tls::TlsStream<tokio::net::TcpStream>,
    timeout: u64,
) -> FutureMitchMsg {
    let fut_protomsg = future::ok(tls)
        .and_then(|tls| {
            let buf = vec![0x00; mem::size_of::<u32>()];
            tokio::io::read_exact(tls, buf)
        }).and_then(|(tls, len)| {
            let buflen = io::Cursor::new(len).read_u32::<NetworkEndian>();
            future::result(buflen).from_err().and_then(|buflen| {
                let buf = vec![0x00; buflen as usize];
                tokio::io::read_exact(tls, buf)
            })
        }).timeout(time::Duration::from_secs(timeout))
        .map_err(|e| errors::Error::from(format!("read_protomsg error: {}", e)))
        .and_then(|(tls, msg)| protomitch::try_parse(&msg).map(|protomsg| (tls, protomsg)))
        .inspect(|(_tls, protomsg)| trace!("got protomitch {:?}", protomsg));

    Box::new(fut_protomsg)
}
