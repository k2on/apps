//! §1.7 Going alone and joining: a peer with no server is its own
//! authority from where it was; joining hands the chain it made to a
//! server, which adopts it when its own run of each entry gives that
//! entry's facts, refuses it whole when one does not, and sequences it
//! anew when it has moved on since the fork.

#[path = "demo.rs"]
mod demo;
mod support {
    pub mod wire;
}

use ark5::frames::Frame;
use ark5::hash::EntryHash;
use ark5::log::EntryId;
use ark5::peer::{Input, Output, Peer, Role};
use ark5::store::Change;
use demo::{create_input, pair_input, pos_of, ADD, CREATE};
use support::wire::*;

fn road() -> [u8; 16] {
    EntryId::of("eve-session", 1).draw(0)
}

/// Eve, alone from the start: a playlist and a track, confirmed by herself.
fn eve_alone() -> Peer {
    let mut eve = replica("eve", demo::cas());
    eve.step(Input::Alone);
    let mut confirmed = vec![];
    for (name, input) in [(CREATE, create_input("Road")), (ADD, pair_input(&road(), "t5"))] {
        let o = eve.step(mutate(name, input, 1));
        assert!(fanouts(&o).len() == 1 && sends(&o).is_empty(), "alone, she tells no server");
        confirmed.extend(o.into_iter().filter(|x| matches!(x, Output::Confirmed(_))));
    }
    assert_eq!(confirmed.len(), 2, "alone, eve confirms her own intents");
    assert!(eve.pending.is_empty() && eve.log.head().0 == 2);
    assert_eq!(eve.confirmed_hash(), eve.store.state_hash());
    assert_eq!(pos_of(&eve.store, &road(), "t5"), Some(1));
    eve
}

/// Hands eve's chain to `server`, `tamper` touching the `Join` on the wire,
/// and delivers what the server answers back to her.
fn join(eve: &mut Peer, server: &mut Peer, tamper: impl Fn(&mut Frame)) -> Vec<Frame> {
    let o = eve.step(Input::Join);
    assert!(matches!(eve.role, Role::Replica) && eve.pending.len() == 2);
    let mut back = vec![];
    for mut f in sends(&o) {
        tamper(&mut f);
        let a = server.step(up(&who("eve"), &f));
        back.extend(fanouts(&a));
        back.extend(sends(&a));
    }
    for f in &back {
        eve.step(down(f));
    }
    back
}

/// Going alone, then joining a server still at the fork: it adopts the
/// chain as it is, and eve's chain is the server's. Falsified by `join` not
/// truncating eve's log to the fork: the server's entries came back as ones
/// she had, and hers stayed pending.
#[test]
fn alone_then_adopted() {
    let mut eve = eve_alone();
    let chain = eve.log.clone();
    let mut server = authority(demo::cas());
    join(&mut eve, &mut server, |_| {});
    assert_eq!(server.log, chain, "the server's chain is the one she made");
    assert!(eve.pending.is_empty() && eve.log == chain && eve.store == server.store);
}

/// A chain whose facts were touched is refused whole: the server's log does
/// not move, and eve's intents come off. Falsified by `adopt` checking
/// nothing (`sequence` without `expect`): the tampered chain was adopted.
#[test]
fn a_chain_whose_facts_differ_is_refused() {
    let mut eve = eve_alone();
    let mut server = authority(demo::cas());
    let back = join(&mut eve, &mut server, |f| {
        if let Frame::Join { logged, .. } = f {
            if let Change::Add { row, .. } = &mut logged[1].facts[0] {
                *row = demo::item_row(&road(), "t5", 99);
            }
        }
    });
    let rejects: Vec<String> = back
        .iter()
        .filter_map(|f| match f {
            Frame::Reject { why, .. } => Some(why.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(rejects.len(), 2, "{back:?}");
    assert!(rejects[0].contains("facts at 2"));
    assert_eq!(server.log.head(), (0, EntryHash::default()));
    assert!(eve.pending.is_empty() && eve.store == server.store);
}

/// A server that moved on since the fork sequences the chain anew after
/// what it has, and eve rebases onto it: bob's playlist, which both had,
/// took a track on the server while eve, alone, put one on it too — so her
/// track's facts in her chain (`pos` 1) are not the server's (`pos` 2), and
/// that is the server's answer, not a refusal. Falsified by `adopt` taking
/// the chain whole whenever its entries are new (the head test dropped):
/// her chain was refused for facts that differ.
#[test]
fn a_server_that_moved_on_sequences_the_chain_anew() {
    let mut server = authority(demo::cas());
    let (mut bob, mut eve) = (replica("bob", demo::cas()), replica("eve", demo::cas()));
    let o = bob.step(mutate(CREATE, create_input("Shared"), 1));
    let (_, f) = deliver_up(&mut server, &who("bob"), &o);
    eve.step(down(&f[0]));
    let shared = EntryId::of("bob-session", 1).draw(0);
    eve.step(Input::Alone);
    eve.step(mutate(ADD, pair_input(&shared, "t5"), 2));
    assert_eq!(pos_of(&eve.store, &shared, "t5"), Some(1));
    let o = bob.step(mutate(ADD, pair_input(&shared, "t1"), 3));
    deliver_up(&mut server, &who("bob"), &o);
    let o = eve.step(Input::Join);
    for f in sends(&o) {
        for back in [fanouts, sends].map(|k| k(&server.step(up(&who("eve"), &f)))).concat() {
            eve.step(down(&back));
        }
    }
    assert_eq!(server.log.head().0, 3);
    assert_eq!(pos_of(&server.store, &shared, "t5"), Some(2));
    assert!(eve.pending.is_empty());
    assert_eq!((eve.log.clone(), eve.store.clone()), (server.log.clone(), server.store.clone()));
}

/// Going alone with intents pending sequences them; one that refuses then
/// is told. Falsified by `alone` keeping the pending intents pending: the
/// log stayed at 1.
#[test]
fn going_alone_sequences_what_was_pending() {
    let mut eve = replica("eve", demo::cas());
    eve.step(mutate(CREATE, create_input("Road"), 1));
    eve.step(mutate(ADD, pair_input(&road(), "t5"), 2));
    let o = eve.step(Input::Alone);
    assert_eq!(o.iter().filter(|x| matches!(x, Output::Confirmed(_))).count(), 2);
    assert_eq!(eve.log.head().0, 2);
    let o = eve.step(mutate(CREATE, create_input("   "), 3));
    assert!(o
        .iter()
        .any(|x| matches!(x, Output::Refused { why, .. } if why == "a playlist needs a name")));
}
