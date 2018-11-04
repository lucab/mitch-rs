use super::{errors, group, membership, reactor, MemberInfo};

use futures::future;
use futures::prelude::*;
use futures::sync::oneshot;

/// Internal reactor event.
#[derive(Debug)]
pub(crate) enum Event {
    /// Notification, a new member is joining to swarm.
    Join(MemberInfo),
    /// Notification, a failed member is leaving the swarm.
    Failed(u32),
    /// Request, all swarm peers (*not* including this node).
    Peers(oneshot::Sender<Vec<MemberInfo>>),
    /// Request, all swarm members (including this node)
    Snapshot(oneshot::Sender<Vec<MemberInfo>>),
}

// Membership handling.
pub(crate) fn membership_task(swarm: &mut group::MitchSwarm) -> group::FutureSpawn {
    // Maximum number of parallel (buffered) in-flight events.
    let inflight_events = 100;
    let local_member = swarm.local.clone();
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
                reactor::Event::Failed(id) => membership::failed(&mut members, id, tx),
                reactor::Event::Join(mi) => membership::join(&mut members, &local_member, mi, tx),
                reactor::Event::Peers(ch) => membership::peers(&members, ch),
                reactor::Event::Snapshot(ch) => membership::snapshot(&members, &local_member, ch),
                //_ => Box::new(future::err(errors::Error::from("unknown"))),
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
