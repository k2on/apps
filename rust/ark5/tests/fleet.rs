//! `rebase/fleet-seed-7`, re-derived: v4's scripted fleet of three over the
//! demo — adds from everyone, a partition, more adds, 200 random deliveries
//! with duplicates and drops — through v5's one machine and the ported
//! scheduler. It must converge to one state with nothing pending and one
//! chain on every peer. The head and state it reaches are pinned.

#[path = "demo.rs"]
mod demo;
mod support {
    pub mod sim;
}

use ark5::log::EntryId;
use demo::{create_input, h, pair_input, ADD, CREATE};
use support::sim::Net;

/// Twenty entries: the playlist and nineteen adds; their order is the
/// seed's.
const HEAD: i64 = 20;
const HASH: &str = "2f24495f091532deac89459a755d3935c3f06d499b56aa09f62bf8b47e98c01d";

enum Move {
    Add(i64, i64),
    Partition(i64),
    Heal(i64),
    Step,
}

/// v4's script (`ark/src/bin/vectors/rebase.rs`, `fleet`), move for move.
fn script() -> Vec<Move> {
    let mut s = vec![];
    for k in 1..=4 {
        s.extend((0..=2).map(|i| Move::Add(i, k)));
    }
    s.push(Move::Partition(2));
    s.extend((5..=7).map(|k| Move::Add(2, k)));
    s.extend([Move::Add(0, 8), Move::Add(1, 9)]);
    s.extend((0..60).map(|_| Move::Step));
    s.push(Move::Partition(0));
    s.extend([Move::Add(1, 10), Move::Add(0, 11)]);
    s.extend((0..60).map(|_| Move::Step));
    s.extend([Move::Heal(0), Move::Heal(2)]);
    s.extend((0..80).map(|_| Move::Step));
    s
}

fn run(seed: u64) -> Net {
    let mut net = Net::new(demo::shared(), demo::cas(), 3, seed);
    net.mutate(0, h(CREATE), create_input("Fleet"), 0);
    net.settle().unwrap();
    let pid = EntryId::of("peer-0-session", 1).draw(0);
    for (n, m) in script().iter().enumerate() {
        match m {
            Move::Add(i, k) => net.mutate(*i, h(ADD), pair_input(&pid, &format!("{i}-{k}")), n as i64),
            Move::Partition(i) => net.partition(*i),
            Move::Heal(i) => net.heal(*i),
            Move::Step => net.step(),
        }
    }
    net.settle().unwrap();
    net
}

/// Seed 7 converges: every client's log is the authority's, entry for
/// entry and hash for hash, every store is its store, nothing is pending,
/// nothing was refused, and every track is on the playlist once, at
/// positions 1 to 19. Falsified by dropping the authority's dedupe (`push`
/// never finding a verdict): seed 7 reached another state.
#[test]
fn fleet_seed_7() {
    let net = run(7);
    net.converged().unwrap();
    assert!(net.refused.is_empty(), "{:?}", net.refused);
    assert!(net.dropped > 0 && net.doubled > 0, "the network lost and repeated frames");
    assert_eq!(net.server.log.head().0, HEAD);
    let mut pos: Vec<i64> = net.server.store.rows("item").map(|r| demo::item_of(r).2).collect();
    pos.sort();
    assert_eq!(pos, (1..=19).collect::<Vec<_>>());
    assert_eq!(net.server.store.state_hash().hex(), HASH);
}

/// Other seeds converge too (their states differ: the order of the adds is
/// the seed's). Falsified by a replica not pushing its pending intents
/// again when it connects: a push the network dropped was never sequenced,
/// and no fleet settled.
#[test]
fn fleets_of_other_seeds_converge() {
    for seed in 0..40 {
        let net = run(seed);
        net.converged().unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        assert_eq!((net.server.log.head().0, net.refused.len()), (HEAD, 0), "seed {seed}");
    }
}
