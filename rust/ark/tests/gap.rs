//! A gap in a device's pushes is answered (§1.7): `Gap { last }`, where its
//! origin stands at the authority. A device that holds the ones between
//! waits for them; one that no longer does numbers its pending intents on
//! from `last` and pushes them again, rather than pushing past the gap
//! forever with nothing ever said — what a phone whose intents before 18
//! were nowhere did, its photo and its edits behind them for a day.

#[path = "demo.rs"]
mod demo;
mod support {
    pub mod wire;
}

use ark::frames::Verdict;
use demo::{create_input, pair_input, pos_of, ADD, CREATE};
use support::wire::*;

/// The playlist alice's first intent creates.
fn pid() -> [u8; 16] {
    ark::log::EntryId::of(ark::log::Origin::named("alice-session"), 1).draw(0)
}

/// Alice's device lost what it had pending past its first, but kept its
/// count: its next intent is 18 while the authority has judged 1. The push
/// is answered with the gap, and the device's intents go on from 2 — run
/// again, pushed again, sequenced. Falsified by `verdict_on` answering a
/// gap with nothing (the first assertion: no verdict at all), or by
/// `close_gap` keeping the numbers (pushed at 18 again, a gap again).
#[test]
fn a_device_past_a_gap_goes_on_from_where_the_authority_stands() {
    let (mut server, mut alice) = (authority(demo::cas()), replica("alice", demo::cas()));
    let (_, f) = deliver_up(&mut server, &who("alice"), &alice.step(mutate(CREATE, create_input("Road"), 1)));
    alice.step(down(&f[0]));
    assert!(alice.pending.is_empty());
    alice.local = 17;
    let o = alice.step(mutate(ADD, pair_input(&pid(), "t1"), 2));
    let (s, f) = deliver_up(&mut server, &who("alice"), &o);
    assert!(matches!(verdicts(&s)[..], [Some(Verdict::Gap { last: 1 })]) && f.is_empty(), "{s:?}");
    let o = alice.step(down(&s[0]));
    assert_eq!(
        (alice.pending[0].entry.local, alice.local),
        (2, 2),
        "numbered on from the authority's last"
    );
    let (s, f) = deliver_up(&mut server, &who("alice"), &o);
    assert!(matches!(verdicts(&s)[..], [Some(Verdict::Ok { seq: 2 })]), "{s:?}");
    alice.step(down(&f[0]));
    alice.step(down(&s[0]));
    assert!(alice.pending.is_empty() && alice.store == server.store);
    assert_eq!(pos_of(&alice.store, &pid(), "t1"), Some(1));
}
