//! A peer driven on its own thread (`ark_peer::driven`): what its server
//! sends is taken in with nobody pumping it, the app told; what the app
//! has it do through the lock is sent; and a peer held twice on one thread
//! is said, not waited on for ever.

mod support;

use std::sync::Arc;
use std::time::Duration;

use ark::schema::Space;
use ark_peer::driven::Driven;
use ark_peer::link::bell;
use ark_peer::{dev, Memory, Peer};
use support::{config, named};

fn hub() -> ark_peer::Handle {
    ark_peer::Handle::spawn(Peer::serve(config(), Box::new(Memory::default()), 0))
}

/// `token`'s device, linked to `hub`, driven.
fn driven(token: &str, hub: &ark_peer::Handle) -> Driven {
    let mut p = Peer::open(config(), dev(token).unwrap(), Box::new(Memory::default()), false);
    p.set_token(Some(token.into()));
    p.connect_with("", hub.dialer(Arc::new(dev)));
    Driven::new(p)
}

/// Waits up to two seconds for `f` to hold of the driven peer, reading it
/// through the lock as an app does.
fn until(d: &Driven, what: &str, f: impl Fn(&Peer) -> bool) {
    let held = ark_peer::link::until(Duration::from_secs(2), Duration::from_millis(5), || f(&d.lock()));
    assert!(held, "never: {what}");
}

/// Bob's playlist reaches alice's driven device with nobody pumping it,
/// and her app is told; hers, made through the lock, reaches the server
/// and is confirmed, unpumped. Falsified by the driver not pumping when
/// the connection rings: alice never had bob's playlist.
#[test]
fn a_driven_peer_takes_in_and_sends_with_nobody_pumping() {
    let hub = hub();
    let (alice, bob) = (driven("alice@laptop", &hub), driven("bob@phone", &hub));
    let (told, rang) = bell();
    alice.set_waker(told);
    until(&alice, "alice linked", |p| p.linked);
    until(&bob, "bob linked", |p| p.linked);
    let _ = rang.try_iter().count();
    bob.lock().author(Space::Durable, "demo.create_playlist", named("Road"), 1).unwrap();
    until(&alice, "alice has bob's playlist", |p| p.log.log().head().0 >= 1);
    assert!(rang.try_iter().count() > 0, "and her app was told");
    alice.lock().author(Space::Durable, "demo.create_playlist", named("Hymns"), 2).unwrap();
    until(&alice, "hers confirmed", |p| p.log.pending.is_empty() && p.log.log().head().0 >= 2);
    let at = hub.with(|p| p.log.log().head().0).unwrap();
    assert_eq!(at, 2, "the server has both");
}

/// A peer held again on the thread that holds it panics in a debug build
/// rather than waiting on itself. Falsified by no check: the test hung.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "held already on this thread")]
fn a_peer_held_twice_on_one_thread_is_said() {
    let p = Peer::open(config(), dev("alice@laptop").unwrap(), Box::new(Memory::default()), false);
    let d = Driven::new(p);
    let _held = d.lock();
    let _again = d.lock();
}

/// A fill's parts are taken in without the app being told of each: only
/// of what it can see move — the link, the fill's end — and not of rows
/// no view of its shows. Eighty playlists of 40 KB, a part each, and the
/// app told a handful of times. Falsified by telling the app whenever
/// anything arrived (`Pumped::moved`): it was told once a part.
#[test]
fn a_fill_is_taken_in_without_telling_the_app_of_every_part() {
    let hub = hub();
    hub.with(|p| {
        for i in 0..80 {
            let name = format!("{i:02}{}", "x".repeat(40_000));
            p.author(Space::Durable, "demo.create_playlist", named(&name), 1).unwrap();
        }
        let head = p.log.machine.log().head().0;
        p.log.machine.step(ark::peer::Input::Compact(head));
    })
    .unwrap();
    let mut p = Peer::open(config(), dev("alice@laptop").unwrap(), Box::new(Memory::default()), false);
    p.log.machine.part = Some(16 << 10);
    p.set_token(Some("alice@laptop".into()));
    p.connect_with("", hub.dialer(Arc::new(dev)));
    let alice = Driven::new(p);
    let (told, rang) = bell();
    alice.set_waker(told);
    until(&alice, "alice filled", |p| {
        p.linked && p.log.machine.fill.is_none() && p.log.log().head().0 >= 80
    });
    let told = rang.try_iter().count();
    assert!(told >= 1, "told the fill was in");
    assert!(told < 20, "told {told} times for eighty parts");
}
