//! This window's peer: an `ark_peer::Peer` over money's domain, alone —
//! its own authority, as the demo is. There is no live state and nothing
//! to dial; everything a window asks of it — a function by name, a view
//! and what it did, an intent's standing — is `ark_peer`'s.

use std::sync::Arc;

use ark::domain::Domain;
use ark_peer::{Config, Disk};

pub use ark_peer::Peer as Replica;

/// money's domain, registered once.
pub fn domain() -> Arc<Domain> {
    money_domain::shared()
}

/// The peer `disk` holds, as whoever last signed in on it.
pub fn open(disk: Box<dyn Disk>) -> Replica {
    Replica::open_as(Config::of(domain()), disk, None)
}
