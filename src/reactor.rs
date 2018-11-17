use super::{errors, group, membership, MemberInfo};

use futures::future;
use futures::prelude::*;
use futures::sync::oneshot;

/// Internal reactor event.
#[derive(Debug)]
pub(crate) enum Event {
    /// Notification, a failed member is leaving the swarm.
    Failed(u32),
    /// Notification, a new member is joining to swarm.
    Join(MemberInfo),
    /// Request, all swarm peers (*not* including this node).
    Peers(oneshot::Sender<Vec<MemberInfo>>),
    /// Request, all swarm members (including this node)
    Snapshot(oneshot::Sender<Vec<MemberInfo>>),
    /// Notification, local swarm member is shutting down.
    Shutdown(oneshot::Sender<()>),
    /// Request, probe a random peer from the swarm (with timeout).
    Probe(u64),
}

// Membership handling.
pub(crate) fn membership_task(swarm: &mut group::MitchSwarm) -> group::FutureSpawn {
    // Maximum number of parallel (buffered) in-flight events.
    let inflight_events = 100;
    let local_member = swarm.local.clone();
    let tls_client = swarm.tls_connector.clone();
    let notifications_tx = swarm.notifications_tx.clone();

    // Members list.
    let mut members = match swarm.members.members.take() {
        Some(mems) => mems,
        None => return Box::new(future::err(())),
    };
    // Source of events for this reactor.
    let rx = match swarm.members.events_rx.take() {
        Some(ch) => ch,
        None => return Box::new(future::err(())),
    };

    let fut_membership = swarm
        .valve
        .wrap(rx)
        .map_err(|_e| errors::Error::from("membership"))
        .and_then(move |msg| {
            trace!("membership event: {:?}", msg);
            let tx = notifications_tx.clone();
            let fut = match msg {
                Event::Failed(id) => membership::failed(&mut members, id, tx),
                Event::Join(mi) => membership::join(&mut members, &local_member, mi, tx),
                Event::Probe(timeout) => membership::probe(&members, tls_client.clone(), timeout),
                Event::Peers(ch) => membership::peers(&members, ch),
                Event::Snapshot(ch) => membership::snapshot(&members, &local_member, ch),
                Event::Shutdown(ch) => {
                    membership::shutdown(members.clone(), local_member.clone(), tls_client.clone(), ch)
                }
                // _ => Box::new(future::err(errors::Error::from("unknown"))),
            };
            Ok(fut)
        }).buffer_unordered(inflight_events)
        .inspect_err(|e| error!("membership error: {}", e))
        .map_err(|_| ())
        .then(|res| match res {
            Ok(_) => Ok(Some(())),
            Err(_err) => Ok(None),
        }).filter_map(|x| x)
        .for_each(|_| Ok(()));
    Box::new(fut_membership)
}
