use super::{errors, group, observer, protomitch, FutureTask, MemberInfo};

use futures::future;
use futures::prelude::*;
use futures::sync::{mpsc, oneshot};

pub(crate) fn join(
    members: &mut Vec<MemberInfo>,
    local_member: &MemberInfo,
    mi: MemberInfo,
    tx: Option<mpsc::Sender<observer::SwarmNotification>>,
) -> FutureTask {
    // TODO(lucab): consider a map instead.
    let joined_index = members.iter().position(|ref m| m.id == mi.id);
    if joined_index.is_some() || (mi.id == local_member.id) {
        // This is already a known peer at this point.
        return Box::new(future::ok(()));
    }

    // Add a node to the swarm.
    members.push(mi.clone());

    // Notify external subscribers.
    match tx {
        Some(ch) => {
            let fut_join = ch
                .send(observer::SwarmNotification::Joined(mi))
                .map_err(|e| errors::Error::from(format!("observer-joined error: {}", e)))
                .map(|_| ());
            Box::new(fut_join)
        }
        None => {
            let fut_join = futures::future::ok(());
            Box::new(fut_join)
        }
    }
}

pub(crate) fn failed(
    members: &mut Vec<MemberInfo>,
    id: u32,
    tx: Option<mpsc::Sender<observer::SwarmNotification>>,
) -> FutureTask {
    // TODO(lucab): consider a map instead.
    match members.iter().position(|ref m| m.id == id) {
        Some(index) => {
            // Remove failed node from the swarm and proceed.
            members.swap_remove(index);
        }
        None => {
            // This is not a known peer at this point.
            return Box::new(future::ok(()));
        }
    }

    // Notify external subscribers.
    let fut_task: FutureTask = match tx {
        Some(ch) => {
            let fut_join = ch
                .send(observer::SwarmNotification::Failed(id))
                .map_err(|e| errors::Error::from(format!("observer-failed error: {}", e)))
                .map(|_| ());
            Box::new(fut_join)
        }
        None => {
            let fut_join = futures::future::ok(());
            Box::new(fut_join)
        }
    };
    Box::new(fut_task)
}

// Return a list of all peers currently known by this local node.
pub(crate) fn peers(peers: &[MemberInfo], ch: oneshot::Sender<Vec<MemberInfo>>) -> FutureTask {
    let res = ch.send(peers.to_vec());
    let fut_sync = future::result(res)
        .map(|_| ())
        .map_err(|_mems| errors::Error::from("membership-peers error"));
    Box::new(fut_sync)
}

// Return a snapshot of all current swarm members.
pub(crate) fn snapshot(
    peers: &[MemberInfo],
    local_member: &MemberInfo,
    ch: oneshot::Sender<Vec<MemberInfo>>,
) -> FutureTask {
    // Include this local node in the member list.
    let mut members = peers.to_vec();
    members.push(local_member.clone());

    let res = ch.send(members);
    let fut_sync = future::result(res)
        .map(|_| ())
        .map_err(|_mems| errors::Error::from("membership-snapshot error"));
    Box::new(fut_sync)
}

// Tell swarm peers that this member is shutting down.
pub(crate) fn shutdown(
    peers: Vec<MemberInfo>,
    local_member: MemberInfo,
    tls_client: native_tls::TlsConnector,
    ch: oneshot::Sender<()>,
) -> FutureTask {
    let local_id = local_member.id;
    let inflight = peers.len();
    let fut_task = futures::stream::iter_ok(peers)
        .and_then(move |peer| {
            trace!("forwarding shutdown to peer {}", peer.id);
            let fut =
                group::tls_connect(tls_client.clone(), &peer.target, 5).and_then(move |tls| {
                    protomitch::failed(local_id)
                        .and_then(|payload| tokio::io::write_all(tls, payload).from_err())
                });
            Ok(fut)
        }).buffer_unordered(inflight)
        .for_each(|_| Ok(()))
        .and_then(move |_| ch.send(()).map_err(|_| "membership-shutdown error".into()));
    Box::new(fut_task)
}
