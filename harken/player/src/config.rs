//! harken's peers, configured once (§1.9, §1.16): what is runtime, not
//! domain, so it is here beside the player rather than in `harken-domain`.
//! A device's peer is the log and the listening session over harken's
//! domain, saying what device it is in its volatile `Hello`, `scrub`
//! relayed as ephemeral; the hub's is the same two spaces as their
//! authority, `on_join`, `on_leave` and `on_restore` the domain's and
//! `host::arrive` reading a device. The desktop, the browser, the phones,
//! `harken-peer`, the server and the phones' in-process hall each open
//! theirs from these.

use std::sync::Arc;

use ark::domain::Domain;
use ark::format::Encode;
use ark_peer::{Config, Live, Module};
use harken_domain::Device;

/// The milliseconds between two ephemeral calls the relay sends — a
/// scrubber dragged — twenty a second; the last always goes.
pub const THROTTLE_MS: i64 = 50;

/// The one ephemeral transition.
const SCRUB: &str = "listening.scrub";

/// What a device says it is in its volatile `Hello`: the server makes it
/// `on_join`'s input with the login's user and session.
pub fn device(can_play: bool, kind: &str, name: &str) -> Vec<u8> {
    Device {
        can_play,
        id: String::new(),
        kind: kind.into(),
        name: name.into(),
        user: String::new(),
    }
    .to_bytes()
}

/// A device's peer over `domain`, saying `device` (a [`Device`]'s bytes) in
/// its volatile `Hello`, its relay sending at most every `every_ms`.
pub fn peer(domain: &Arc<Domain>, device: Vec<u8>, every_ms: i64) -> Config {
    let live = Live {
        device: Some(device),
        ephemeral: [SCRUB.to_string()].into(),
        every_ms,
        ..Live::new(Module::of(domain))
    };
    Config {
        live: Some(live),
        ..Config::of(domain.clone())
    }
}

/// The hub over `domain`, keeping `keep` entries of the log above its
/// snapshot (all of them when none) and 256 of the listening session's.
pub fn hub(domain: &Arc<Domain>, keep: Option<usize>) -> Config {
    let get = |n: &str| domain.binding(&format!("listening.{n}")).map(String::from);
    Config {
        live: Some(Live {
            on_join: get("on_join"),
            on_leave: get("on_leave"),
            on_restore: get("on_restore"),
            arrive: Arc::new(harken_domain::host::arrive),
            keep: Some(256),
            ..Live::new(Module::of(domain))
        }),
        keep,
        ..Config::of(domain.clone())
    }
}
