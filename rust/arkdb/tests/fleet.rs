//! `rebase/fleet-seed-7` through the real hub and the real client: K1's
//! scripted fleet of three over the demo — adds from everyone, a
//! partition, more adds, 200 random deliveries with duplicates and drops —
//! must reach the head and the state K1 pinned over a bare machine, with
//! one chain on every peer and nothing pending; and forty other seeds the
//! same.

use arkdb::vectors::fleet;

/// K1's pin (`rust/ark/tests/fleet.rs`, now moved here): twenty entries,
/// this state. N1 moved the state (`ff04f56d…` before): rows are their
/// JSON and ids drawn without the codec; the twenty entries are as they were.
/// N4 made rows postcard (`8316d1f4…` as JSON). A build saying its schema
/// at each connection put a frame more on the wire, and seed 7's drops and
/// repeats fell elsewhere (`54e6f160…` before): twenty entries still.
const HEAD: i64 = 20;
const HASH: &str = "c8ad6c12356e4cbbc585eaff62ad8ac06182833806f2d1b924db4022cfa93cdb";

/// Seed 7 converges to K1's pin through the hub: every client's log is the
/// hub's, entry for entry and hash for hash, every store is its store,
/// nothing is pending or refused, every frame read back as sent. Falsified
/// by the hub sending a `Send` to every connection rather than the one the
/// frame came from: seed 7 did not reach the pin.
#[test]
fn fleet_seed_7_through_the_hub() {
    let net = fleet(7);
    net.converged().unwrap();
    assert!(net.refused.is_empty() && net.misread.is_empty(), "{:?}", net.refused);
    assert!(net.dropped > 0 && net.doubled > 0, "the network lost and repeated frames");
    assert_eq!(net.hub.log.log().head().0, HEAD);
    assert_eq!(net.hub.log.store.state_hash().hex(), HASH);
}

/// Forty other seeds converge to twenty entries, nothing refused, every
/// client of each — 123 with seed 7's — with nothing pending. Falsified by
/// the client dropping its frames on `connected` after the machine queued
/// them (`frames.clear()` after the steps): no fleet settled.
#[test]
fn fleets_of_other_seeds_converge() {
    let mut clients = 0;
    for seed in 0..=40 {
        let net = fleet(seed);
        net.converged().unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        assert_eq!((net.hub.log.log().head().0, net.refused.len()), (HEAD, 0), "seed {seed}");
        for (i, c) in &net.clients {
            assert!(c.log.pending().is_empty(), "seed {seed} client {i}: pending");
            clients += 1;
        }
    }
    assert_eq!(clients, 123);
}
