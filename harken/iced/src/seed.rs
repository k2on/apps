//! What an empty demo library starts with: the demo's library as
//! `harken-seed` (`harken/demo`, beside the app, not in it) makes it — the classical
//! catalogue and the shows, as the records of a peer alone — opened here as
//! this window's replica.
//!
//! Only the demo has this: the real client is built with no features and
//! never compiles the file, nor links the seed. The library there is
//! whatever its server's scanner found and its catalogue imported.

use ark_peer::Memory;
#[cfg(test)]
pub use harken_seed::catalogue;
pub use harken_seed::seeded_disk;

use crate::client::Replica;

/// The demo's replica: every seeded row, as the confirmed state of a peer that
/// is its own authority.
pub fn seeded() -> Replica {
    open(seeded_disk(1))
}

/// The demo's replica opened from its records: its own authority, as the
/// library's identity.
pub fn open(disk: Memory) -> Replica {
    let mut peer = crate::client::open(Box::new(disk), None, crate::listening::device());
    // The demo's identity is the library's, as its seed was
    // (`docs/plan-guards.md` D1).
    peer.set_roles([crate::domain::LIBRARY]);
    peer.alone();
    peer
}

/// The demo's library `copies` times over ([`seeded_disk`]), for measuring
/// how a list's cost grows with the library. `1` is the demo.
#[cfg(test)]
pub fn seeded_times(copies: usize) -> Replica {
    open(seeded_disk(copies))
}
