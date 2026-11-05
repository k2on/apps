//! What the fuzzer (`arkdb`) found in reopening and the horizon, and what
//! a replica long offline costs, each held here at the machine: a push of
//! an intent sequenced below a reopened log's horizon is acknowledged
//! there; a replica opened from its disk puts its pending intents back
//! without running them; and one with many pending, connected at last, is
//! confirmed as it previewed, nothing undone.

#[path = "demo.rs"]
mod demo;
mod support {
    pub mod wire;
}

use ark::frames::Verdict;
use ark::log::{EntryId, Origin, Record};
use ark::peer::Input;
use demo::{create_input, pair_input, ADD, CREATE};
use support::wire::*;

fn road() -> [u8; 16] {
    EntryId::of(Origin::named("eve-session"), 1).draw(0)
}

/// A replica opened from its disk puts its pending intents' changes back
/// as they were made, running none, when its log stands where they were
/// made ([`Durable::at`]); standing elsewhere, it runs them again. The
/// pending intent stands for one whose preview a run would not give again:
/// its changes are another input's. Falsified by `open` running every
/// pending intent again: the store held "Road", not "Trip".
#[test]
fn a_replica_opens_its_pending_without_running_them() {
    let mut eve = replica("eve", demo::cas());
    eve.step(mutate(CREATE, create_input("Road"), 1));
    let mut other = replica("eve", demo::cas());
    other.step(mutate(CREATE, create_input("Trip"), 1));
    let mut p = eve.pending[0].clone();
    p.facts.clone_from(&other.pending[0].facts);
    let durable = |at| {
        let pending = vec![p.clone()];
        vec![Record::Rows(eve.confirmed_rows()), Record::Pending { pending, at, local: 1 }]
    };
    let mut kept = replica("eve", demo::cas());
    kept.step(Input::Open(durable(Some(eve.log().head()))));
    assert_eq!(kept.store, other.store, "the changes put back");
    let mut run = replica("eve", demo::cas());
    run.step(Input::Open(durable(None)));
    assert_eq!(run.store, eve.store, "the intent run again");
}

/// A replica offline for long — a playlist and 300 adds pending, more than
/// a page — connects to a server that has nothing: each intent is
/// confirmed as it previewed it, nothing undone, run again or told.
/// Falsified by `confirm` taking the oldest pending intent's own entry
/// for a rebase (`previewed` false): it ran the pending intents again for
/// every page.
#[test]
fn a_replica_long_offline_is_confirmed_as_it_previewed() {
    let mut eve = replica("eve", demo::cas());
    eve.step(mutate(CREATE, create_input("Road"), 1));
    for i in 0..300 {
        eve.step(mutate(ADD, pair_input(&road(), &format!("t{i}")), 2 + i));
    }
    let reruns = eve.reruns;
    let o = eve.step(Input::Connected);
    let mut server = authority(demo::cas());
    let (back, fan) = deliver_up(&mut server, &who("eve"), &o);
    let mut told = 0;
    for b in fan.iter().chain(&back) {
        told += changes(&eve.step(down(b))).len();
    }
    assert!(eve.pending.is_empty() && eve.store == server.store && server.log().head().0 == 301);
    assert_eq!((eve.reruns, told), (reruns, 0));
}

/// A log reopened from a snapshot above an intent's sequence has no verdict
/// for it; pushed again, it is acknowledged at the horizon, not held as a
/// gap. Falsified by `push` without the rule: nothing answered it.
#[test]
fn a_push_below_a_reopened_horizon_is_acknowledged_there() {
    let mut server = authority(demo::cas());
    let mut bob = replica("bob", demo::cas());
    let o = bob.step(mutate(CREATE, create_input("Mix"), 1));
    deliver_up(&mut server, &who("bob"), &o);
    let o2 = bob.step(mutate(CREATE, create_input("Two"), 2));
    deliver_up(&mut server, &who("bob"), &o2);
    server.compact(2);
    let mut reopened = authority(demo::cas());
    reopened.step(Input::Open(vec![Record::Rows(server.confirmed_rows())]));
    if let ark::peer::Role::Authority { journal } = &mut reopened.role {
        journal.pass(Origin::named("bob-session"), "bob", 2);
    }
    let (back, _) = deliver_up(&mut reopened, &who("bob"), &o);
    assert!(matches!(&verdicts(&back)[..], [Some(Verdict::At { seq: 2, .. })]), "{back:?}");
}
