//! What the fuzzer (`arkdb`) found in going alone, joining and the
//! horizon, each held here at the machine: an intent refused alone draws
//! no `local`, and one pending that refuses as the device goes alone is
//! kept for its server, so a joined chain has no gap; a `Join` lost on the
//! way is pushed again on the next connection, the intents made since
//! after it; a push of an intent
//! sequenced below a reopened log's horizon is acknowledged there; and a
//! volatile push overtaken by a later one of its session is refused.

#[path = "demo.rs"]
mod demo;
mod support {
    pub mod wire;
}

use ark::frames::{Frame, Verdict};
use ark::log::{Durable, EntryId, Origin};
use ark::peer::{Input, Output};
use demo::{create_input, pair_input, ADD, CREATE};
use support::wire::*;

fn road() -> [u8; 16] {
    EntryId::of(Origin::named("eve-session"), 1).draw(0)
}

/// Alone, a refused intent leaves the next to take its `local`. Falsified
/// by `mutate` drawing the `local` whatever the verdict: the playlist after
/// the refusal was eve's second.
#[test]
fn an_intent_refused_alone_draws_no_local() {
    let mut eve = replica("eve", demo::cas());
    eve.step(Input::Alone);
    eve.step(mutate(CREATE, create_input("  "), 1));
    eve.step(mutate(CREATE, create_input("Road"), 2));
    assert_eq!(eve.log().entries()[0].entry.local, 1);
}

/// An intent pending as the device went alone that refused then is kept,
/// unrun, and pushed in its place at the join: the server, which never saw
/// it, judges it and takes the chain after it, with no gap. It stands for
/// one a rebase left refusing (its input made to name no playlist).
/// Falsified by `alone` refusing it as an authority's own (`decide`): it
/// was told refused, and (that check dropped) its `local` was a gap at the
/// server, the playlist after it pending still.
#[test]
fn an_intent_refused_going_alone_is_the_servers_to_judge() {
    let mut server = authority(demo::cas());
    let mut bob = replica("bob", demo::cas());
    let o = bob.step(mutate(CREATE, create_input("Bob's"), 1));
    let (_, f) = deliver_up(&mut server, &who("bob"), &o);
    let mut eve = replica("eve", demo::cas());
    eve.step(down(&f[0]));
    let bobs = EntryId::of(Origin::named("bob-session"), 1).draw(0);
    eve.step(mutate(ADD, pair_input(&bobs, "t1"), 2));
    eve.pending[0].entry.input = pair_input(&[9; 16], "t1");
    let o = eve.step(Input::Alone);
    assert!(!o.iter().any(|x| matches!(x, Output::Refused { .. })) && eve.pending.len() == 1, "{o:?}");
    eve.step(mutate(CREATE, create_input("Road"), 3));
    let o = eve.step(Input::Join);
    let (back, fan) = deliver_up(&mut server, &who("eve"), &o);
    for b in fan.iter().chain(&back) {
        eve.step(down(b));
    }
    assert_eq!(server.log().head().0, 2, "{back:?}");
    assert!(eve.pending.is_empty() && eve.store == server.store);
}

/// A `Join` lost on the way: the chain is pending intents, pushed again on
/// the next connection, an intent made since after it. Falsified by `join`
/// leaving the chain out of the pending intents: the next connection pushed
/// one intent, and the playlist was lost.
#[test]
fn a_lost_join_is_pushed_again() {
    let mut eve = replica("eve", demo::cas());
    eve.step(Input::Alone);
    eve.step(mutate(CREATE, create_input("Road"), 1));
    eve.step(Input::Join);
    eve.step(mutate(ADD, pair_input(&road(), "t5"), 2));
    let o = eve.step(Input::Connected);
    let f = sends(&o);
    assert!(
        matches!(&f[..], [Frame::Hello { .. }, Frame::Page { logged, .. }] if logged.iter().map(|l| (l.seq, l.entry.local)).eq([(0, 1), (0, 2)])),
        "{f:?}"
    );
    let mut server = authority(demo::cas());
    let (back, fan) = deliver_up(&mut server, &who("eve"), &o);
    for b in fan.iter().chain(&back) {
        eve.step(down(b));
    }
    assert!(eve.pending.is_empty() && eve.store == server.store && server.log().head().0 == 2);
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
    server.step(Input::Compact(2));
    let mut reopened = authority(demo::cas());
    reopened.step(Input::Open(Durable {
        rows: server.confirmed_rows(),
        ..Durable::default()
    }));
    if let ark::peer::Role::Authority { journal } = &mut reopened.role {
        journal.pass(Origin::named("bob-session"), "bob", 2);
    }
    let (back, _) = deliver_up(&mut reopened, &who("bob"), &o);
    assert!(matches!(&verdicts(&back)[..], [Some(Verdict::Ok { seq: 2 })]), "{back:?}");
}
