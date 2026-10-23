//! A page reloaded while its server runs: the same device, opened again
//! from its disk, calls into the live space.

use std::sync::Arc;

use ark::format::Encode;
use ark::schema::Space;
use ark_dev::demo::demo as domain;
use ark_dev::demo::{arrival, room, Point};
use ark_peer::{dev, Config, Memory, Module, Peer};

fn hub() -> Peer {
    let mut c = Config::new(Module::of(&domain()));
    c.live = Some(ark_peer::Live {
        on_join: Some("room.on_join".into()),
        on_leave: Some("room.on_leave".into()),
        arrive: Arc::new(arrival),
        ..ark_peer::Live::new(Module::of(&room()))
    });
    Peer::serve(c, Box::new(Memory::default()), 0)
}

fn device() -> Peer {
    let mut c = Config::new(Module::of(&domain()));
    c.live = Some(ark_peer::Live {
        device: Some(b"d0".to_vec()),
        ..ark_peer::Live::new(Module::of(&room()))
    });
    let mut p = Peer::open(c, dev("u0@d0").unwrap(), Box::new(Memory::default()), false);
    p.linked = true;
    p
}

fn carry(hub: &mut Peer, d: &mut Peer, c: u64) {
    loop {
        let mut q = vec![];
        for f in d.take_frames() {
            q.extend(hub.frame(c, f, 0));
        }
        if q.is_empty() {
            return;
        }
        for (to, b) in q {
            if to == c {
                d.receive(b);
            }
        }
    }
}

/// A page reloaded while its server runs is heard: its live calls reach
/// the server's live space. Falsified by the live machine opening under the
/// log's origin, which the disk keeps: the reloaded page's `local`s started
/// again at 1, below what the page before it had pushed, and every call was
/// refused as overtaken — harken's presses and hand-offs after a reload.
#[test]
fn a_reloaded_page_is_heard_in_the_live_space() {
    let mut hub = hub();
    let mut d = device();
    let c = hub.accept(d.me().clone());
    d.connected();
    carry(&mut hub, &mut d, c);
    for i in 0..5 {
        d.author(Space::Volatile, "room.point", Point { x: i }.to_bytes(), i).unwrap();
        carry(&mut hub, &mut d, c);
    }
    // The page reloads: its connection goes, it opens again from its disk.
    let _ = hub.leave(c, 10);
    let mut d = d.reopen();
    d.linked = true;
    let c = hub.accept(d.me().clone());
    d.connected();
    carry(&mut hub, &mut d, c);
    let before = hub.machine(Space::Volatile).unwrap().log().head();
    d.author(Space::Volatile, "room.point", Point { x: 99 }.to_bytes(), 20).unwrap();
    carry(&mut hub, &mut d, c);
    let after = hub.machine(Space::Volatile).unwrap().log().head();
    assert_ne!(before, after, "the reloaded page's call never reached the server's live space");
}
