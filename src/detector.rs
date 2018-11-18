//! Failure detector with local-health-aware probing.
//!
//! This is modeled after Lifeguard paper sec. 4.A.

use super::{errors, group, reactor, FutureTask};
use futures::prelude::*;
use futures::{future, stream, sync::mpsc};
use std::time;
use stream_cancel;
use tokio;

/// Base seconds between liveness probes.
static PROBE_BASE_INTERVAL_SECS: u64 = 1;

/// Base milliseconds for `ack` timeout on each probe.
static PROBE_BASE_TIMEOUT_MILLIS: u64 = 500;

/// Maximum local-health multiplier.
static PROBE_MAX_HEALTH: u64 = 8;

/// Initial local health.
static PROBE_INITIAL_HEALTH: u64 = 0;

/// Internal detector event.
#[derive(Debug)]
pub(crate) enum Event {
    /// Notification, probing period tick.
    Tick,
    /// Notification, successful probe.
    ProbeOk,
    /// Notification, failed probe.
    ProbeFailed,
    /// Notification, missed probe.
    ProbeMissed,
    /// Notification, refuting a suspect.
    SuspectSelf,
}

// Failure detector task.
pub(crate) fn detector_task(swarm: &mut group::MitchSwarm) -> group::FutureSpawn {
    // Local health.
    let mut local_health = PROBE_INITIAL_HEALTH;
    // Maximum number of parallel (buffered) in-flight events.
    let inflight_events = 100;

    // Source of events for this task.
    let det_rx = match swarm.detector.det_rx.take() {
        Some(ch) => ch,
        None => return Box::new(future::err(())),
    };

    // Start periodic ticker.
    let det_tx = swarm.detector.det_tx.clone();
    spawn_detector_ticker(&swarm.valve, &local_health, det_tx);

    let mems_tx = swarm.members.reactor_tx.clone();
    let fut_detector = swarm
        .valve
        .wrap(det_rx)
        .map_err(|_| errors::Error::from("detector rx"))
        .and_then(move |msg| {
            trace!("detector event: {:?}", msg);
            let mems_tx = mems_tx.clone();
            let fut = match msg {
                Event::Tick => tick(&local_health, mems_tx),
                Event::ProbeOk => update_health(&mut local_health, false),
                Event::ProbeFailed | Event::ProbeMissed | Event::SuspectSelf => {
                    update_health(&mut local_health, true)
                }
                // _ => Box::new(future::err(errors::Error::from("detector event unknown"))),
            };
            Ok(fut)
        }).buffer_unordered(inflight_events)
        .then(|res| match res {
            Ok(r) => Ok(Some(r)),
            Err(err) => {
                error!("detector-reactor error: {}", err);
                Ok(None)
            }
        }).filter_map(|x| x)
        .for_each(|_| Ok(()));

    Box::new(fut_detector)
}

/// Start the health-aware interval ticker.
fn spawn_detector_ticker(
    valve: &stream_cancel::Valve,
    local_health: &u64,
    det_tx: mpsc::Sender<Event>,
) {
    let health = *local_health;
    let intervals = valve
        .wrap(stream::repeat(det_tx.clone()))
        .and_then(move |ch| {
            let interval = health
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
}

/// Dispatch a detector tick to start probing a peer.
pub(crate) fn tick(local_health: &u64, ch: mpsc::Sender<reactor::Event>) -> FutureTask {
    let fut_tick = future::ok((*local_health, ch))
        .and_then(|(local_health, ch)| {
            let timeout = local_health
                .saturating_add(1)
                .saturating_mul(PROBE_BASE_TIMEOUT_MILLIS);
            ch.send(reactor::Event::Probe(timeout))
                .map_err(|_| errors::Error::from("detector-tick probe error"))
        }).map(|_| ())
        .map_err(|_mems| errors::Error::from("detector-tick error"));
    Box::new(fut_tick)
}

/// Update health after probe result.
pub(crate) fn update_health(local_health: &mut u64, increase: bool) -> FutureTask {
    if increase {
        *local_health = local_health
            .saturating_add(1)
            .wrapping_rem(PROBE_MAX_HEALTH);
    } else {
        *local_health = local_health.saturating_sub(1);
    }

    let fut_update = future::ok(());
    Box::new(fut_update)
}
