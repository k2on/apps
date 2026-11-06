//! A gap in a device's pushes (§1.7): a push says where its device's
//! pending intents begin (its page's `from`), and what its origin had up to
//! there is passed — the device no longer holds it — so a phone whose
//! intents before 18 were nowhere is not stuck behind them, as one was, its
//! photo and its edits behind them for a day.

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
/// count: its next intent is 18 while the authority has judged 1. Its push
/// says its pending begin after 17; the authority passes 2 to 17, judges 18
/// and sequences it, its number and the ids it drew as they were. Falsified
/// by `verdict_on` not passing a push's `from` (18 answered as a gap, not
/// judged), or by the device's push saying it begins after 0.
#[test]
fn a_device_past_a_gap_goes_on_from_where_the_authority_stands() {
    let (mut server, mut alice) = (authority(demo::cas()), replica("alice", demo::cas()));
    let (_, f) = deliver_up(&mut server, &who("alice"), &alice.step(mutate(CREATE, create_input("Road"), 1)));
    alice.step(down(&f[0]));
    assert!(alice.pending().is_empty());
    alice.local = 17;
    let o = alice.step(mutate(ADD, pair_input(&pid(), "t1"), 2));
    let id = alice.pending()[0].entry.id;
    let (s, f) = deliver_up(&mut server, &who("alice"), &o);
    assert!(matches!(verdicts(&s)[..], [Some(Verdict::At { seq: 2, last: 18 })]), "{s:?}");
    alice.step(down(&f[0]));
    alice.step(down(&s[0]));
    assert_eq!(
        (server.log().head().0, server.log().at(2).unwrap().entry.id),
        (2, id),
        "sequenced as it was numbered"
    );
    assert!(alice.pending().is_empty() && alice.store == server.store);
    assert_eq!(pos_of(&alice.store, &pid(), "t1"), Some(1));
}
