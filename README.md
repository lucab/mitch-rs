# Mitch

[![Build Status](https://travis-ci.org/lucab/mitch-rs.svg?branch=master)](https://travis-ci.org/lucab/mitch-rs)
[![crates.io](https://img.shields.io/crates/v/mitch.svg)](https://crates.io/crates/mitch)
[![Documentation](https://docs.rs/mitch/badge.svg)](https://docs.rs/mitch)

Asynchronous membership-gossiping library, based on SWIM/Lifeguard.

This crate provides a [tokio]-based swarm membership library, with
gossiping behavior and weak consistency. Its protocol is based on
[SWIM][swim] / [Lifeguard][lifeguard], using protobuf and TLS for
transport.

[tokio]: https://tokio.rs/
[swim]: http://www.cs.cornell.edu/projects/Quicksilver/public_pdfs/SWIM.pdf
[lifeguard]: https://arxiv.org/abs/1707.00788

## Example

```rust
extern crate mitch;
extern crate futures;
extern crate tokio;
use futures::prelude::*;

fn main() {
    let mut runner = tokio::runtime::Runtime::new().unwrap();

    // Start running a swarm, waiting for other peers to reach us.
    let cfg = mitch::MitchConfig::default();
    let fut_swarm = cfg.build();
    let swarm = runner.block_on(fut_swarm).unwrap();

    // Snapshot and print swarm members every 5 secs.
    let membership = futures::stream::repeat(swarm.membership());
    let period = std::time::Duration::from_secs(5);
    let fut_snapshot = tokio::timer::Interval::new_interval(period)
        .from_err()
        .zip(membership)
        .and_then(move |(_, mems)| mems.snapshot())
        .map_err(|_| ())
        .for_each(|mems| Ok(println!(" * Swarm members: {:#?}", mems)));
    runner.spawn(fut_snapshot);

    // Keep running for 45 secs.
    let delay = std::time::Instant::now() + std::time::Duration::from_secs(45);
    let fut_stop = swarm.stop();
    let fut_delayed_stop = tokio::timer::Delay::new(delay)
        .map_err(|e| format!("{}", e).into())
        .and_then(|_| fut_stop);
    runner.block_on_all(fut_delayed_stop);
}
```

Some more examples are available under [examples](examples).

## License

Licensed under either of

 * MIT license - <http://opensource.org/licenses/MIT>
 * Apache License, Version 2.0 - <http://www.apache.org/licenses/LICENSE-2.0>

at your option.
