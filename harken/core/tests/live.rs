//! The listening session as a device holds it (§1.9, §1.10):
//! `HarkenView::Listening`, the session, its devices and their scrubbers,
//! each a view over the device's live space, read into the same
//! `host::Listening` the domain's own reading of the live store gives —
//! computed on the device, with nothing asked of the server.
// A test reads its fixtures and times itself: the kernel's and the core's
// purity (clippy.toml) is their library's, not their tests'.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::sync::Arc;

use ark::format::Encode;
use ark_peer::store::{Store, ViewKey};
use ark_peer::{dev, Config, Deliver, Live, Memory, Module, Peer};
use harken_domain::host::Listening;
use harken_domain::*;

fn domain() -> Arc<ark::domain::Domain> {
    harken_domain::shared()
}

fn hub() -> Peer {
    let get = |n: &str| Some(format!("listening.{n}"));
    let c = Config {
        live: Some(Live {
            on_join: get("on_join"),
            on_leave: get("on_leave"),
            on_restore: get("on_restore"),
            arrive: Arc::new(harken_domain::host::arrive),
            ..Live::new(Module::of(&domain()))
        }),
        ..Config::of(domain())
    };
    Peer::serve(c, Box::new(Memory::default()), 0)
}

fn device(token: &str, name: &str, live: bool) -> Peer {
    let said = Device {
        can_play: true,
        id: String::new(),
        kind: "phone".into(),
        name: name.into(),
        user: String::new(),
    };
    let mut c = Config::of(domain());
    if live {
        c.live = Some(Live {
            device: Some(said.to_bytes()),
            ephemeral: ["listening.scrub".to_string()].into(),
            ..Live::new(Module::of(&domain()))
        });
    }
    Peer::open(c, dev(token).unwrap(), Box::new(Memory::default()), false)
}

fn carry(hub: &mut Peer, ds: &mut [(&mut Peer, u64)], also: Vec<Deliver>) {
    let mut q = also;
    loop {
        for (d, c) in ds.iter_mut() {
            d.flush();
            for f in d.take_frames() {
                q.extend(hub.frame(*c, f, 0));
            }
        }
        if q.is_empty() {
            return;
        }
        for (to, b) in std::mem::take(&mut q) {
            if let Some((d, _)) = ds.iter_mut().find(|(_, c)| *c == to) {
                d.receive(b);
            }
        }
    }
}

fn join(hub: &mut Peer, d: &mut Peer) -> u64 {
    let c = hub.accept(d.me().clone());
    d.linked = true;
    d.connected();
    c
}

/// What the store holds of `HarkenView::Listening`.
fn held(s: &Store<HarkenView>) -> Listening {
    let rows = |k: HarkenView| s.rows::<Vec<u8>>(&k).to_vec();
    Listening::read(&rows(HarkenView::Session), &rows(HarkenView::Devices), &rows(HarkenView::Scrubbing))
}

/// What the device's live store holds of its account, read the domain's
/// way.
fn truth(d: &Peer) -> Listening {
    Listening::of(&d.live.as_ref().expect("a live machine").store, &d.me().actor)
}

fn track(n: u8) -> Track {
    Track {
        album: "Goldberg".into(),
        creator: "Bach".into(),
        duration_ms: 180_000,
        file: format!("bach/{n}.flac"),
        id: Id::new([n; 16]),
        title: format!("Variation {n}"),
    }
}

/// The session a phone starts, the devices standing in it and a scrubber
/// dragged on the other are all in the phone's `Listening`, as the domain
/// reads its live store; and another account's are not. Falsified by
/// `HarkenView::open` leaving the live part over the log's state: the
/// views held nothing.
#[test]
fn the_listening_view_is_the_live_session() {
    let mut hub = hub();
    let mut phone = device("alice@phone", "Phone", true);
    let mut laptop = device("alice@laptop", "Laptop", true);
    let mut other = device("bob@tablet", "Tablet", true);
    let mut s = Store::<HarkenView>::new();
    s.hold(&mut phone, HarkenView::Listening);
    assert_eq!(held(&s), Listening::default());
    let (cp, cl, co) = (join(&mut hub, &mut phone), join(&mut hub, &mut laptop), join(&mut hub, &mut other));
    carry(&mut hub, &mut [(&mut phone, cp), (&mut laptop, cl), (&mut other, co)], vec![]);
    let start = Start {
        at: 0,
        from: "Goldberg".into(),
        position: 0,
        queue: vec![track(1), track(2)],
    };
    phone.call("start", start.to_bytes()).unwrap();
    other
        .call(
            "start",
            Start {
                queue: vec![track(9)],
                ..start.clone()
            }
            .to_bytes(),
        )
        .unwrap();
    laptop.call("scrub", Scrub { at_ms: Some(42_000) }.to_bytes()).unwrap();
    carry(&mut hub, &mut [(&mut phone, cp), (&mut laptop, cl), (&mut other, co)], vec![]);
    let moved = s.settle(&mut phone);
    assert!(moved.contains(&HarkenView::Listening), "{moved:?}");
    let l = held(&s);
    assert_eq!(l, truth(&phone));
    let names: Vec<&str> = l.devices.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["Laptop", "Phone"]);
    let session = l.session.as_ref().expect("the session the phone started");
    assert_eq!(session.queue, vec![track(1), track(2)]);
    assert_eq!(l.scrub.get(&laptop.me().session), Some(&42_000));
    // The session goes on moving: the laptop leaves.
    let gone = hub.leave(cl, 1);
    laptop.disconnected();
    carry(&mut hub, &mut [(&mut phone, cp)], gone);
    s.settle(&mut phone);
    assert_eq!(held(&s), truth(&phone));
    assert!(held(&s).scrub.is_empty(), "the laptop's scrubber went with it");
}

/// A phone alone (or a demo) has no live space: the session is idle and
/// nothing is refused.
#[test]
fn alone_the_session_is_idle() {
    let mut phone = device("alice@phone", "Phone", false);
    let mut s = Store::<HarkenView>::new();
    s.hold(&mut phone, HarkenView::Listening);
    s.settle(&mut phone);
    assert_eq!(held(&s), Listening::default());
    assert_eq!(s.refusal(), None);
    assert_eq!(
        HarkenView::Listening.parts(),
        [HarkenView::Session, HarkenView::Devices, HarkenView::Scrubbing]
    );
}
