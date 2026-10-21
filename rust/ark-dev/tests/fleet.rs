//! `rebase/fleet-seed-7` through the real hub and the real client: K1's
//! scripted fleet of three over the demo — adds from everyone, a
//! partition, more adds, 200 random deliveries with duplicates and drops —
//! must reach the head and the state K1 pinned over a bare machine, with
//! one chain on every peer and nothing pending; and the trace corpus:
//! every client of every fleet session, recorded and replayed into fresh
//! machines to the same outputs.

use ark_dev::trace::{record, replay};
use ark_dev::vectors::fleet;

/// K1's pin (`rust/ark/tests/fleet.rs`, now moved here): twenty entries,
/// this state.
const HEAD: i64 = 20;
const HASH: &str = "ff04f56d613ee73dad721e02cb6ebe125fa93fe801b2f1713499038f55475e95";

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

/// Forty other seeds converge to twenty entries, nothing refused, and every
/// client of each — 123 traces with seed 7's — replays to what it said.
/// Falsified by the client dropping its frames on `connected` after the
/// machine queued them (`frames.clear()` after the steps): no fleet
/// settled; and by the client recording its open as a login with no
/// session: no trace replayed.
#[test]
fn fleets_of_other_seeds_converge_and_every_client_replays() {
    let mut traces = 0;
    for seed in 0..=40 {
        let net = fleet(seed);
        net.converged().unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        assert_eq!((net.hub.log.log().head().0, net.refused.len()), (HEAD, 0), "seed {seed}");
        for (i, c) in &net.clients {
            replay(&record(&format!("fleet-seed-{seed}, client {i}"), c)).unwrap_or_else(|e| panic!("seed {seed} client {i}: {e}"));
            traces += 1;
        }
    }
    assert_eq!(traces, 123);
}

/// The demo in plain Rust (§1.16) is the ArkLang demo: its schema hashes
/// as `demo.ark`'s, and seed 7 and forty other seeds reach the very states
/// the ArkLang fleet reaches, entry for entry. Falsified by stepping
/// `add_to_playlist`'s place by two: seed 7's state moved.
#[test]
fn the_demo_in_rust_reaches_the_states_the_arklang_demo_does() {
    assert_eq!(ark_dev::native::demo().schema_hash(), ark_dev::demo::domain().schema.hash());
    for seed in [7, 0, 1, 2, 3, 11, 29, 40] {
        let (lang, native) = (fleet(seed), ark_dev::vectors::fleet_native(seed));
        native.converged().unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        assert!(native.refused.is_empty(), "seed {seed}: {:?}", native.refused);
        let states = |n: &ark_dev::sim::Net| n.hub.log.log().entries().iter().map(|l| l.state).collect::<Vec<_>>();
        assert_eq!(states(&native), states(&lang), "seed {seed}");
    }
    assert_eq!(ark_dev::vectors::fleet_native(7).hub.log.store.state_hash().hex(), HASH);
}
