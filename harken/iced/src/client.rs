//! This device's peer (`docs/plan-v5.md` §1.7, §1.9, §1.11): an
//! `ark_peer::Peer` over harken's domain — the log's machine and the
//! listening session's, over one connection — and the little of it that is
//! harken's: the device it says it is in its volatile `Hello`, the
//! scrubber relayed as ephemeral, and the listening session read from its
//! live store.
//!
//! Everything else a window asks of it — a function by name, a view and
//! what it did, an intent's standing, the link dialled again with backoff
//! and ringing the window's bell, nobody signed in — is `ark_peer`'s
//! (`ark_peer::app`, `ark_peer::link`).

use std::sync::{Arc, OnceLock};

use ark::domain::Domain;
use ark::peer::Who;
pub use ark_peer::link::Wake;
#[cfg_attr(all(feature = "demo", not(test)), allow(unused_imports))]
pub use ark_peer::nobody;
pub use ark_peer::{monotonic_ms, now_ms, Peer as Replica, Standing, Update};
use ark_peer::{Config, Disk, Memory};

/// The milliseconds between two ephemeral calls the relay sends — a
/// scrubber dragged — twenty a second; the last always goes.
pub const THROTTLE_MS: u64 = harken_player::config::THROTTLE_MS as u64;

/// harken's domain, registered once.
pub fn domain() -> &'static Arc<Domain> {
    static D: OnceLock<Arc<Domain>> = OnceLock::new();
    D.get_or_init(harken_domain::shared)
}

/// What this device says it is in its volatile `Hello`: a computer.
pub fn device(can_play: bool, name: &str) -> Vec<u8> {
    harken_player::config::device(can_play, crate::domain::host::Kind::Computer.name(), name)
}

/// The peer's two modules, and the relay (`harken_player::config::peer`).
pub fn config(device: Vec<u8>) -> Config {
    harken_player::config::peer(domain(), device, THROTTLE_MS as i64)
}

/// The peer `disk` holds ([`Replica::open_as`]), saying `device` in its
/// volatile `Hello`.
pub fn open(disk: Box<dyn Disk>, login: Option<(Who, String)>, device: Vec<u8>) -> Replica {
    Replica::open_as(config(device), disk, login)
}

/// A peer in memory, as `me`, its token `token` when there is one.
#[cfg_attr(all(feature = "demo", not(test)), allow(dead_code))]
pub fn memory(me: Who, token: Option<&str>) -> Replica {
    memory_as(me, token, crate::listening::device())
}

/// [`memory`], saying `device` in its volatile `Hello`.
#[cfg_attr(all(feature = "demo", not(test)), allow(dead_code))]
pub fn memory_as(me: Who, token: Option<&str>, device: Vec<u8>) -> Replica {
    let mut disk = Memory::default();
    let (actor, session) = (Some(me.actor.clone()), me.session.clone());
    let _ = ark_peer::local::preset(&mut disk, |r| (r.actor, r.session) = (actor, session));
    let mut r = open(Box::new(disk), None, device);
    r.set_roles(me.roles);
    r.link.token = token.map(Into::into);
    r
}

/// The listening session's machine, in numbers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveStatus {
    pub seq: i64,
    pub pending: usize,
    /// This device stands in the session: its `on_join` has landed.
    pub synced: bool,
}

/// The account's listening session as this device holds it: the server's,
/// with its own calls previewed on top.
pub fn listening(r: &Replica) -> crate::domain::host::Listening {
    match &r.live {
        Some(l) => crate::domain::host::Listening::of(&l.store, &r.me().actor),
        None => Default::default(),
    }
}

/// Whether this device stands in its account's listening session: its
/// row is in the volatile store, which only `on_join` writes.
pub fn present(r: &Replica) -> bool {
    listening(r).device(&r.me().session).is_some()
}

/// The listening session's machine, in numbers.
pub fn live_status(r: &Replica) -> Option<LiveStatus> {
    let l = r.live.as_ref()?;
    Some(LiveStatus {
        seq: l.log().head().0,
        pending: l.pending.len(),
        synced: present(r),
    })
}
