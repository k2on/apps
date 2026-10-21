//! What the fuzzer (`ark-dev`) found in going alone, joining and the
//! horizon, each held here at the machine: an intent refused alone draws
//! no `local`; a joined chain with a gap is taken whole by a server that
//! moved on; a `Join` lost on the way is handed over again on the next
//! connection, the intents made since pushed after it; a push of an intent
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

/// A chain whose `local`s skip one — an intent eve refused herself as she
/// went alone — is sequenced anew whole by a server that moved on, the gap
/// not held for. Falsified by `adopt` pushing the chain without taking a
/// gap as final: the entry after it waited and stayed pending.
#[test]
fn a_chain_with_a_gap_is_taken_whole() {
    let mut server = authority(demo::cas());
    let mut bob = replica("bob", demo::cas());
    let o = bob.step(mutate(CREATE, create_input("Bob's"), 1));
    deliver_up(&mut server, &who("bob"), &o);
    let mut eve = replica("eve", demo::cas());
    eve.step(Input::Alone);
    eve.step(mutate(CREATE, create_input("Road"), 1));
    eve.step(mutate(ADD, pair_input(&road(), "t5"), 2));
    let mut o = eve.step(Input::Join);
    for f in &mut o {
        if let Output::Send(b) = f {
            if let Ok(Frame::Page { space, from, mut logged }) = Frame::dec(b) {
                logged[1].entry.local = 3;
                logged[1].entry.id = EntryId::of(Origin::named("eve-session"), 3);
                *b = Frame::Page { space, from, logged }.enc();
            }
        }
    }
    let (acks, _) = deliver_up(&mut server, &who("eve"), &o);
    assert_eq!(
        acks.iter()
            .filter(|f| matches!(
                f,
                Frame::Verdict {
                    verdict: Verdict::Ok { .. },
                    ..
                }
            ))
            .count(),
        2,
        "{acks:?}"
    );
    assert_eq!(server.log().head().0, 3);
}

/// A `Join` lost on the way: the next connection hands the chain over
/// again rather than pushing its entries one by one, and an intent made
/// since is pushed after it. Falsified by `connected` pushing the pending
/// intents whatever the join: its frames were a `Hello` and three pushes.
#[test]
fn a_lost_join_is_handed_over_again() {
    let mut eve = replica("eve", demo::cas());
    eve.step(Input::Alone);
    eve.step(mutate(CREATE, create_input("Road"), 1));
    eve.step(Input::Join);
    eve.step(mutate(ADD, pair_input(&road(), "t5"), 2));
    let o = eve.step(Input::Connected);
    let f = sends(&o);
    assert!(
        matches!(&f[..], [Frame::Hello { .. }, Frame::Page { logged, .. }, Frame::Page { logged: pushed, .. }] if logged.len() == 1 && logged[0].seq == 1 && pushed[0].seq == 0 && pushed[0].entry.local == 2),
        "{f:?}"
    );
    let mut server = authority(demo::cas());
    let (back, fan) = deliver_up(&mut server, &who("eve"), &o);
    for b in fan.iter().chain(&back) {
        eve.step(down(b));
    }
    assert!(eve.pending.is_empty() && eve.store == server.store);
    let again = sends(&eve.step(Input::Connected));
    assert!(
        !again
            .iter()
            .any(|f| matches!(f, Frame::Page { logged, .. } if logged.iter().any(|l| l.seq > 0))),
        "answered, it is not handed over again"
    );
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
