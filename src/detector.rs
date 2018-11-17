//! Failure detector with local-health-aware probing.
//!
//! This is modeled after Lifeguard paper sec. 4.A.

use super::{errors, group, reactor, FutureTask};
use futures::prelude::*;
use futures::{future, stream, sync::mpsc};
use std::time;
use tokio;

/// Base seconds between liveness probes.
pub(crate) static PROBE_BASE_INTERVAL_SECS: u64 = 1;

/// Base milliseconds for `ack` timeout on each probe.
pub(crate) static PROBE_BASE_TIMEOUT_MILLIS: u64 = 500;

/// Internal detector event.
#[derive(Debug)]
pub(crate) enum Event {
    /// Notification, probing period tick.
    Tick,
}

// Failure detector task.
pub(crate) fn detector_task(swarm: &mut group::MitchSwarm) -> group::FutureSpawn {
    // Maximum number of parallel (buffered) in-flight events.
    let inflight_events = 100;
    // Local health.
    let local_health = 0u64;

    // Periodic probe.
    let tx = swarm.detector.det_tx.clone();
    let intervals = swarm
        .valve
        .wrap(stream::repeat(tx.clone()))
        .and_then(move |ch| {
            let interval = local_health
                .saturating_add(1)
                .saturating_mul(PROBE_BASE_INTERVAL_SECS);
            let pause = time::Instant::now() + time::Duration::from_secs(interval);
            tokio::timer::Delay::new(pause)
                .map_err(|_| errors::Error::from("detector delay"))
                .and_then(|_| Ok(ch))
        }).and_then(|ch| {
            ch.send(Event::Tick)
                .map_err(|_| errors::Error::from("detector delay event"))
        }).map_err(|_| ())
        .for_each(|_| Ok(()));
    tokio::spawn(intervals);

    // Source of events for this task.
    let rx = match swarm.detector.det_rx.take() {
        Some(ch) => ch,
        None => return Box::new(future::err(())),
    };

    let mems_tx = swarm.members.reactor_tx.clone();
    let fut_detector = swarm
        .valve
        .wrap(rx)
        .map_err(|_| errors::Error::from("detector rx"))
        .and_then(move |msg| {
            trace!("detector event: {:?}", msg);
            let mems_tx = mems_tx.clone();
            let fut = match msg {
                Event::Tick => tick(local_health, mems_tx),
                // _ => Box::new(future::err(errors::Error::from("detector event unknown"))),
            };
            Ok(fut)
        }).buffer_unordered(inflight_events)
        .inspect_err(|e| error!("membership error: {}", e))
        .map_err(|_| ())
        .then(|res| match res {
            Ok(r) => Ok(Some(r)),
            Err(err) => {
                error!("detector error: {:?}", err);
                Ok(None)
            }
        }).filter_map(|x| x)
        .for_each(|_| Ok(()));

    Box::new(fut_detector)
}

/// Dispatch a detector tick to start probing a peer.
pub(crate) fn tick(local_health: u64, ch: mpsc::Sender<reactor::Event>) -> FutureTask {
    let fut_tick = future::ok((local_health, ch))
        .and_then(|(local_health, ch)| {
            let timeout = local_health
                .saturating_add(1)
                .saturating_mul(PROBE_BASE_TIMEOUT_MILLIS);
            ch.send(reactor::Event::Probe(timeout))
                .map_err(|_| errors::Error::from("detector probe tick"))
        }).map(|_| ())
        .map_err(|_mems| errors::Error::from("detector-tick error"));
    Box::new(fut_tick)
}
