//! `rebase/fleet-seed-7` through the real hub and the real client: K1's
//! scripted fleet of three over the demo — adds from everyone, a
//! partition, more adds, 200 random deliveries with duplicates and drops —
//! must reach the head and the state K1 pinned over a bare machine, with
//! one chain on every peer and nothing pending.

use ark5::codec::bytes_enc;
use ark5::log::EntryId;
use ark5_client::Config;
use ark5_dev::demo::{domain, module};
use ark5_dev::sim::{Net, Op};
use ark5_server::hub::Config as HubConfig;

/// K1's pin (`rust/ark5/tests/fleet.rs`): twenty entries, this state.
const HEAD: i64 = 20;
const HASH: &str = "2f24495f091532deac89459a755d3935c3f06d499b56aa09f62bf8b47e98c01d";

fn pair(id: &[u8], t: &str) -> Vec<u8> {
    [bytes_enc(id), bytes_enc(t.as_bytes())].concat()
}

/// v4's script, move for move.
fn script(pid: &[u8]) -> Vec<Op> {
    let add = |i: i64, k: i64| Op::Mutate(i, domain().get("demo.add_to_playlist"), pair(pid, &format!("{i}-{k}")));
    let mut s = vec![];
    for k in 1..=4 {
        s.extend((0..=2).map(|i| add(i, k)));
    }
    s.push(Op::Partition(2));
    s.extend((5..=7).map(|k| add(2, k)));
    s.extend([add(0, 8), add(1, 9)]);
    s.extend((0..60).map(|_| Op::Step));
    s.push(Op::Partition(0));
    s.extend([add(1, 10), add(0, 11)]);
    s.extend((0..60).map(|_| Op::Step));
    s.extend([Op::Heal(0), Op::Heal(2)]);
    s.extend((0..80).map(|_| Op::Step));
    s
}

pub fn run(seed: u64) -> Net {
    let d = domain();
    let config = Config { log: module(&d), live: None };
    let mut net = Net::new(HubConfig::new(module(&d)), |_| config.clone(), 3, seed);
    net.run(&Op::Mutate(0, d.get("demo.create_playlist"), bytes_enc(b"Fleet")));
    net.settle().unwrap();
    let pid = EntryId::of("peer-0-session", 1).draw(0);
    for op in script(&pid) {
        net.run(&op);
    }
    net.settle().unwrap();
    net
}

/// Seed 7 converges to K1's pin through the hub: every client's log is the
/// hub's, entry for entry and hash for hash, every store is its store,
/// nothing is pending or refused, every frame read back as sent. Falsified
/// by the hub sending a `Send` to every connection rather than the one the
/// frame came from: seed 7 did not reach the pin.
#[test]
fn fleet_seed_7_through_the_hub() {
    let net = run(7);
    net.converged().unwrap();
    assert!(net.refused.is_empty() && net.misread.is_empty(), "{:?}", net.refused);
    assert!(net.dropped > 0 && net.doubled > 0, "the network lost and repeated frames");
    assert_eq!(net.hub.log.log.head().0, HEAD);
    assert_eq!(net.hub.log.store.state_hash().hex(), HASH);
}

/// Forty other seeds converge to twenty entries, nothing refused.
/// Falsified by the client dropping its frames on `connected` after the
/// machine queued them (`frames.clear()` after the steps): no fleet
/// settled.
#[test]
fn fleets_of_other_seeds_converge_through_the_hub() {
    for seed in 0..40 {
        let net = run(seed);
        net.converged().unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        assert_eq!((net.hub.log.log.head().0, net.refused.len()), (HEAD, 0), "seed {seed}");
    }
}
