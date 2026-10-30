//! This window's peer: an `ark_peer::Peer` over money's domain, alone —
//! its own authority, as the demo is, opened on the seeded household
//! (`money_domain::seed::open`). There is no live state and nothing to
//! dial; everything a window asks of it — a function by name, a view and
//! what it did, an intent's standing — is `ark_peer`'s.

pub use ark_peer::Peer as Replica;
