use super::{errors, reactor, MemberInfo};

use futures::prelude::*;
use futures::sync::{mpsc, oneshot};

/// Membership observer.
#[derive(Clone, Debug)]
pub struct Membership {
    /// Channel to MitchSwarm internal membership handler.
    pub(crate) reactor_tx: mpsc::Sender<reactor::Event>,
}

impl Membership {
    /// Get current set of swarm members, including this local node.
    pub fn snapshot(self) -> super::FutureMembers {
        let (snapshot_tx, snapshot_rx) = oneshot::channel();
        let snapshot_event = reactor::Event::Snapshot(snapshot_tx);
        let fut_snapshot = self
            .reactor_tx
            .send(snapshot_event)
            .map_err(|e| errors::Error::from(format!("snapshot: observer error: {}", e)))
            .and_then(move |_| snapshot_rx.from_err());
        Box::new(fut_snapshot)
    }

    /// Get current set of swarm peers, without this local node.
    pub fn peers(self) -> super::FutureMembers {
        let (peers_tx, peers_rx) = oneshot::channel();
        let peers_event = reactor::Event::Peers(peers_tx);
        let fut_peers = self
            .reactor_tx
            .send(peers_event)
            .map_err(|e| errors::Error::from(format!("peers: observer error: {}", e)))
            .and_then(move |_| peers_rx.from_err());
        Box::new(fut_peers)
    }
}

/// Notification for a swarm event.
#[derive(Debug)]
pub enum SwarmNotification {
    /// A new member joined the swarm.
    Joined(MemberInfo),
    /// A swarm member failed and left the swarm.
    Failed(u32),
}
