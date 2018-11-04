//! Asynchronous membership-gossiping library, based on SWIM/Lifeguard.
//!
//! This crate provides a [tokio]-based swarm membership library, with
//! gossiping behavior and weak consistency. Its protocol is based on
//! [SWIM][swim] / [Lifeguard][lifeguard], using protobuf and TLS for
//! transport.
//!
//! [tokio]: https://tokio.rs/
//! [swim]: http://www.cs.cornell.edu/projects/Quicksilver/public_pdfs/SWIM.pdf
//! [lifeguard]: https://arxiv.org/abs/1707.00788
//!
//! ## Example
//!
//! ```rust,no_run
//! extern crate mitch;
//! extern crate futures;
//! extern crate tokio;
//! use futures::prelude::*;
//!
//! fn main() {
//!     let mut runner = tokio::runtime::Runtime::new().unwrap();
//!
//!     // Start running a group, waiting for other peers to reach us.
//!     let cfg = mitch::MitchConfig::default();
//!     let fut_swarm = cfg.build();
//!     let swarm = runner.block_on(fut_swarm).unwrap();
//!
//!     // Snapshot and print swarm members every 5 secs.
//!     let membership = futures::stream::repeat(swarm.membership());
//!     let period = std::time::Duration::from_secs(5);
//!     let fut_snapshot = tokio::timer::Interval::new_interval(period)
//!         .from_err()
//!         .zip(membership)
//!         .and_then(move |(_, mems)| mems.snapshot())
//!         .map_err(|_| ())
//!         .for_each(|mems| Ok(println!(" * Swarm members: {:#?}", mems)));
//!     runner.spawn(fut_snapshot);
//!
//!     // Keep running for 45 secs.
//!     let delay = std::time::Instant::now() + std::time::Duration::from_secs(45);
//!     let fut_stop = swarm.stop();
//!     let fut_delayed_stop = tokio::timer::Delay::new(delay)
//!         .map_err(|e| format!("{}", e).into())
//!         .and_then(|_| fut_stop);
//!     runner.block_on_all(fut_delayed_stop);
//! }
//! ```

#![deny(missing_docs)]

extern crate byteorder;
#[macro_use]
extern crate error_chain;
extern crate futures;
#[macro_use]
extern crate log;
extern crate names;
extern crate native_tls;
extern crate prost;
#[macro_use]
extern crate prost_derive;
extern crate rand;
extern crate stream_cancel;
extern crate tokio;
extern crate tokio_tls;

use futures::prelude::*;

pub mod errors;
mod group;
mod member_info;
mod membership;
mod observer;
mod protomitch;
mod protomitch_pb;
mod reactor;

pub use group::{MitchConfig, MitchSwarm};
pub use member_info::MemberInfo;
pub use observer::{Membership, SwarmNotification};

/// Minimum protocol version supported by this library.
pub static MIN_PROTO: u32 = 0;
/// Maximum protocol version supported by this library.
pub static MAX_PROTO: u32 = 0;

/// Future mitch swarm.
pub type FutureSwarm = Box<Future<Item = MitchSwarm, Error = errors::Error> + Send + 'static>;
/// Future swarm task, generic empty result.
pub type FutureTask = Box<Future<Item = (), Error = errors::Error> + Send + 'static>;
/// Future set of swarm members.
pub type FutureMembers =
    Box<Future<Item = Vec<MemberInfo>, Error = errors::Error> + Send + 'static>;
/// Stream of swarm notifications.
pub type StreamNotifications =
    Box<Stream<Item = SwarmNotification, Error = errors::Error> + Send + 'static>;
