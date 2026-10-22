//! §1.7 Going alone and joining: a peer with no server is its own
//! authority from where it was; joining makes the chain it made its
//! pending intents, pushed to a server that judges each as a push — at
//! the fork or after what it has since — so one refused loses that entry
//! and not the chain.

#[path = "demo.rs"]
mod demo;
mod support {
    pub mod wire;
}

use ark::frames::{Frame, Verdict};
use ark::log::{EntryId, Origin};
use ark::peer::{Input, Output, Peer, Role};
use demo::{create_input, pair_input, pos_of, ADD, CREATE};
use support::wire::*;

fn road() -> [u8; 16] {
    EntryId::of(Origin::named("eve-session"), 1).draw(0)
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
    assert!(eve.pending.is_empty() && eve.log().head().0 == 2);
    assert_eq!(eve.confirmed_hash(), eve.store.state_hash());
    assert_eq!(pos_of(&eve.store, &road(), "t5"), Some(1));
    eve
}

/// Hands eve's chain to `server`, `tamper` touching its pushes on the wire,
/// and delivers what the server answers back to her.
fn join(eve: &mut Peer, server: &mut Peer, tamper: impl Fn(&mut Frame)) -> Vec<Frame> {
    let n = eve.log().entries().len();
    let o = eve.step(Input::Join);
    assert!(matches!(eve.role, Role::Replica) && eve.pending.len() == n);
    let mut paged = sends(&o).into_iter().flat_map(|f| match f {
        Frame::Page { logged, .. } => logged,
        _ => vec![],
    });
    assert!(paged.all(|l| l.seq == 0), "the chain goes as intents, not as a chain");
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

/// Going alone, then joining a server still at the fork: each intent of
/// the chain pushed is judged as it was alone, and eve's chain is the
/// server's. Falsified by `join` not truncating eve's log to the fork: the
/// server's entries came back as ones she had, and hers stayed pending.
#[test]
fn alone_then_joined_at_the_fork() {
    let mut eve = eve_alone();
    let chain = eve.log().entries();
    let mut server = authority(demo::cas());
    join(&mut eve, &mut server, |_| {});
    assert_eq!(server.log().entries(), chain, "the server's chain is the one she made");
    assert!(eve.pending.is_empty() && eve.log() == server.log() && eve.store == server.store);
}

/// A chain with one entry the server refuses loses that entry, not the
/// chain: the entries around it are sequenced, and eve's store is the
/// server's. The track's playlist is made unknown on the wire. Falsified by
/// the server answering the rest of an origin's pushes after a refusal as a
/// gap (`judged` not passing a refused `local`): the playlist after it
/// stayed pending.
#[test]
fn a_chain_with_one_refused_entry_loses_that_entry() {
    let mut eve = eve_alone();
    eve.step(mutate(CREATE, create_input("Trip"), 3));
    let mut server = authority(demo::cas());
    let back = join(&mut eve, &mut server, |f| {
        if let Frame::Page { logged, .. } = f {
            logged[1].entry.input = pair_input(&[9; 16], "t5");
        }
    });
    let refused = verdicts(&back)
        .into_iter()
        .flatten()
        .filter(|v| matches!(v, Verdict::Refused { .. }))
        .count();
    assert_eq!(refused, 1, "{back:?}");
    assert_eq!(server.log().head().0, 2, "the playlists before and after it");
    assert_eq!(pos_of(&server.store, &road(), "t5"), None);
    assert!(eve.pending.is_empty() && eve.store == server.store);
}

/// A chain naming a function the server does not run is pushed as intents
/// like any other: what it runs is sequenced, and the one it lacks is
/// `Held`, pending still on the device. Falsified by `push` refusing an
/// intent whose function it lacks: the track came off eve's store.
#[test]
fn a_chain_naming_a_function_the_server_lacks_is_held() {
    let mut eve = eve_alone();
    let mut server = authority(demo::only(&[CREATE]));
    let back = join(&mut eve, &mut server, |_| {});
    assert!(
        matches!(verdicts(&back).into_iter().flatten().last(), Some(Verdict::Held { .. })),
        "{back:?}"
    );
    assert_eq!(server.log().head().0, 1);
    assert_eq!(eve.pending.len(), 1);
    assert_eq!(pos_of(&eve.store, &road(), "t5"), Some(1));
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
    let shared = EntryId::of(Origin::named("bob-session"), 1).draw(0);
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
    assert_eq!(server.log().head().0, 3);
    assert_eq!(pos_of(&server.store, &shared, "t5"), Some(2));
    assert!(eve.pending.is_empty());
    assert_eq!((eve.log(), eve.store.clone()), (server.log(), server.store.clone()));
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
    assert_eq!(eve.log().head().0, 2);
    let o = eve.step(mutate(CREATE, create_input("   "), 3));
    assert!(o
        .iter()
        .any(|x| matches!(x, Output::Refused { why, .. } if why == "a playlist needs a name")));
}
