//! A view over the live space (§1.9, §1.10): a store holds a key whose
//! opening names the live space, its rows the device's live machine's —
//! who is here, as the server's `on_join` and `on_leave` say it — moving as
//! that space does, read afresh when the live machine is made anew, and no
//! rows (not a refusal) while there is none.

use std::sync::Arc;

use ark::schema::Space;
use ark_peer::store::{Opening, Store, ViewKey};
use ark_peer::{dev, Config, Deliver, Memory, Module, Peer};
use arkdb::demo::demo as domain;
use arkdb::demo::{arrival, room, Here};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    /// Who is here, over the live space.
    Present,
    /// The same function over the log's state, which holds none of it.
    Logged,
}

impl ViewKey for Key {
    fn open(&self) -> Option<Opening> {
        let o = Opening::of::<Here>("present", ark::format::Encode::to_bytes(&()));
        Some(match self {
            Key::Present => o.live(),
            Key::Logged => o,
        })
    }
}

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

fn device(token: &str, name: &str, live: bool) -> Peer {
    let mut c = Config::of(Arc::new(domain()));
    if live {
        c.live = Some(ark_peer::Live {
            device: Some(name.as_bytes().to_vec()),
            ..ark_peer::Live::new(Module::of(&room()))
        });
    }
    Peer::open(c, dev(token).unwrap(), Box::new(Memory::default()), false)
}

/// Every frame between the hub and its devices carried until nothing more
/// is said; each device is its connection's.
fn carry(hub: &mut Peer, ds: &mut [(&mut Peer, u64)], also: Vec<Deliver>) {
    let mut q = also;
    loop {
        for (d, c) in ds.iter_mut() {
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

fn names(s: &Store<Key>, k: &Key) -> Vec<String> {
    let mut n: Vec<String> = s.rows::<Here>(k).iter().map(|h| h.name.clone()).collect();
    n.sort();
    n
}

/// A device's live view holds who is here and moves as they come and go;
/// the same function over the log's state holds nothing. Falsified by
/// `Opening::live` opening over the log's state (`Peer::view`): `Present`
/// had no rows.
#[test]
fn a_live_view_moves_with_the_live_space() {
    let mut hub = hub();
    let (mut a, mut b) = (device("u0@a", "phone", true), device("u0@b", "laptop", true));
    let mut s = Store::<Key>::new();
    s.hold(&mut a, Key::Present);
    s.hold(&mut a, Key::Logged);
    assert!(names(&s, &Key::Present).is_empty(), "nothing is here before a connection");
    let ca = join(&mut hub, &mut a);
    carry(&mut hub, &mut [(&mut a, ca)], vec![]);
    assert_eq!(s.settle(&mut a), vec![Key::Present]);
    assert_eq!(names(&s, &Key::Present), ["phone"]);
    let cb = join(&mut hub, &mut b);
    carry(&mut hub, &mut [(&mut a, ca), (&mut b, cb)], vec![]);
    s.settle(&mut a);
    assert_eq!(names(&s, &Key::Present), ["laptop", "phone"]);
    let gone = hub.leave(cb, 1);
    b.disconnected();
    carry(&mut hub, &mut [(&mut a, ca)], gone);
    assert!(s.settle(&mut a).contains(&Key::Present));
    assert_eq!(names(&s, &Key::Present), ["phone"]);
    assert!(names(&s, &Key::Logged).is_empty(), "the log's state holds nobody here");
    assert_eq!(s.refusal(), None);
}

/// A sign-in makes the live machine anew, and the view is read afresh
/// from it — empty until it is connected again, then what the server says.
/// Falsified by `Peer::sign_in` not making its live views again
/// (`remade`): the view went on answering from the machine before.
#[test]
fn a_new_live_machine_is_read_afresh() {
    let mut hub = hub();
    let mut a = device("u0@a", "phone", true);
    let mut s = Store::<Key>::new();
    s.hold(&mut a, Key::Present);
    let ca = join(&mut hub, &mut a);
    carry(&mut hub, &mut [(&mut a, ca)], vec![]);
    s.settle(&mut a);
    assert_eq!(names(&s, &Key::Present), ["phone"]);
    let gone = hub.leave(ca, 1);
    a.disconnected();
    carry(&mut hub, &mut [], gone);
    a.sign_in(dev("u0@a").unwrap());
    assert!(s.settle(&mut a).contains(&Key::Present));
    assert!(names(&s, &Key::Present).is_empty(), "a new live machine holds nothing yet");
    let ca = join(&mut hub, &mut a);
    carry(&mut hub, &mut [(&mut a, ca)], vec![]);
    s.settle(&mut a);
    assert_eq!(names(&s, &Key::Present), ["phone"]);
}

/// While it was away, somebody came: a new connection's live space has
/// them, and so does the view.
#[test]
fn a_new_connection_is_read_into_the_view() {
    let mut hub = hub();
    let (mut a, mut b) = (device("u0@a", "phone", true), device("u0@b", "laptop", true));
    let mut s = Store::<Key>::new();
    s.hold(&mut a, Key::Present);
    let ca = join(&mut hub, &mut a);
    carry(&mut hub, &mut [(&mut a, ca)], vec![]);
    let gone = hub.leave(ca, 1);
    a.disconnected();
    carry(&mut hub, &mut [], gone);
    let cb = join(&mut hub, &mut b);
    carry(&mut hub, &mut [(&mut b, cb)], vec![]);
    let ca = join(&mut hub, &mut a);
    carry(&mut hub, &mut [(&mut a, ca), (&mut b, cb)], vec![]);
    s.settle(&mut a);
    assert_eq!(names(&s, &Key::Present), ["laptop", "phone"]);
}

/// A device with no live space (alone, a demo) holds the view as no rows,
/// and says nothing is wrong. Falsified by the view's refusal kept as the
/// store's: `refusal` said "no machine of that space".
#[test]
fn no_live_machine_is_no_rows_and_no_refusal() {
    let mut a = device("u0@a", "phone", false);
    let mut s = Store::<Key>::new();
    s.hold(&mut a, Key::Present);
    assert!(s.holds(&Key::Present));
    assert!(s.rows::<Here>(&Key::Present).is_empty());
    assert_eq!(s.settle(&mut a), Vec::<Key>::new());
    assert_eq!(s.refusal(), None);
    assert_eq!(Key::Present.open().map(|o| o.space()), Some(Space::Volatile));
    assert_eq!(Key::Logged.open().map(|o| o.space()), Some(Space::Durable));
}

/// A change an app sends is sent where its function is: a live one as a
/// call on the live space, any other as an intent on the log. Falsified by
/// `Peer::change` sending every change as an intent: the rename was
/// refused, the room's function being none of the log's.
#[test]
fn a_change_goes_to_its_functions_space() {
    use ark::format::Encode;
    let mut hub = hub();
    let mut a = device("u0@a", "phone", true);
    let ca = join(&mut hub, &mut a);
    carry(&mut hub, &mut [(&mut a, ca)], vec![]);
    let rename = arkdb::demo::Rename { name: "kitchen".into() };
    a.change("room.rename", rename.to_bytes()).unwrap();
    let playlist = arkdb::demo::CreatePlaylist { name: "mix".into() };
    a.change("demo.create_playlist", playlist.to_bytes()).unwrap();
    assert_eq!(a.pending_len(), 1, "the playlist is an intent on the log");
    carry(&mut hub, &mut [(&mut a, ca)], vec![]);
    let mut s = Store::<Key>::new();
    s.hold(&mut a, Key::Present);
    assert_eq!(names(&s, &Key::Present), ["kitchen"]);
    assert_eq!(a.pending_len(), 0);
}
