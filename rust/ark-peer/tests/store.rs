//! An app's views held in one place (`ark_peer::store`): held by key and
//! counted, a page held as its views, settled into the keys that moved,
//! and a change's refusal kept.

use std::sync::Arc;

use ark::format::Encode;
use ark::prelude::*;
use ark_peer::store::{Opening, Store, ViewKey};
use ark_peer::{Config, Memory, Peer};
use arkdb::demo::{add_to_playlist, create_playlist, items, AddToPlaylist, CreatePlaylist, Demo, Item, Items};

/// The demo's views, as an app names them: one playlist's items, or a page
/// of two playlists side by side.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum View {
    Items([u8; 16]),
    Both([u8; 16], [u8; 16]),
}

impl ViewKey for View {
    fn parts(&self) -> Vec<View> {
        match self {
            View::Both(a, b) => vec![View::Items(*a), View::Items(*b)],
            v => vec![v.clone()],
        }
    }

    fn open(&self) -> Option<Opening> {
        match self {
            View::Items(p) => Some(Opening::of::<Item>("demo.items", Items { playlist_id: Id::new(*p) }.to_bytes())),
            View::Both(..) => None,
        }
    }
}

fn peer() -> Peer {
    let domain = ark::register!(Domain::new::<Demo>("demo", "1"); create_playlist, add_to_playlist, items);
    let mut p = Peer::open_as(Config::of(Arc::new(domain)), Box::new(Memory::default()), None);
    p.alone();
    p
}

fn playlist(p: &mut Peer, name: &str) -> [u8; 16] {
    let id = p.mutate("demo.create_playlist", &CreatePlaylist { name: name.into() }).unwrap();
    id.draw(0)
}

fn add(p: &mut Peer, list: [u8; 16], track: &str) {
    let a = AddToPlaylist {
        playlist_id: Id::new(list),
        track_id: track.into(),
    };
    p.mutate("demo.add_to_playlist", &a).unwrap();
}

fn tracks(s: &Store<View>, k: &View) -> Vec<String> {
    s.rows::<Item>(k).iter().map(|i| i.track_id.clone()).collect()
}

/// A view held twice is opened once and closed when the second hold goes;
/// its rows are read whole when held and spliced as it moves, and its
/// version grows only when it moved. Falsified by closing at the first
/// release: the rows were gone while a screen still held them.
#[test]
fn a_view_is_held_by_count_and_spliced() {
    let mut p = peer();
    let road = playlist(&mut p, "Road");
    add(&mut p, road, "t1");
    let mut s = Store::new();
    let k = View::Items(road);
    assert_eq!(s.version(&k), 0, "nothing holds it");
    s.hold(&mut p, k.clone());
    s.hold(&mut p, k.clone());
    assert_eq!(tracks(&s, &k), ["t1"], "read whole");
    let v = s.version(&k);
    assert!(s.settle(&mut p).is_empty(), "the first read is not news");
    add(&mut p, road, "t2");
    assert_eq!(s.settle(&mut p), std::slice::from_ref(&k));
    assert_eq!(tracks(&s, &k), ["t1", "t2"], "spliced");
    assert!(s.version(&k) > v);
    s.release(&mut p, &k);
    assert_eq!(tracks(&s, &k), ["t1", "t2"], "still held once");
    s.release(&mut p, &k);
    assert!(!s.holds(&k));
    assert!(tracks(&s, &k).is_empty(), "closed");
    assert_eq!(s.version(&k), 0);
}

/// A page holds each of its views, and moves when one of them does; a view
/// it shares with another hold stays when the page goes. Falsified by a
/// page that held its views without counting them: releasing the page
/// closed the view held on its own too.
#[test]
fn a_page_is_its_views() {
    let mut p = peer();
    let (road, sea) = (playlist(&mut p, "Road"), playlist(&mut p, "Sea"));
    let mut s = Store::new();
    let page = View::Both(road, sea);
    s.hold(&mut p, View::Items(sea));
    s.hold(&mut p, page.clone());
    assert!(s.holds(&page) && s.holds(&View::Items(road)));
    add(&mut p, road, "t1");
    let moved = s.settle(&mut p);
    assert_eq!(moved, [View::Items(road), page.clone()], "the view, then the page made of it");
    assert_eq!(s.version(&page), s.version(&View::Items(road)));
    s.release(&mut p, &page);
    assert!(!s.holds(&page) && !s.holds(&View::Items(road)));
    assert!(s.holds(&View::Items(sea)), "held on its own still");
    add(&mut p, sea, "t9");
    assert_eq!(s.settle(&mut p), [View::Items(sea)]);
    assert_eq!(tracks(&s, &View::Items(sea)), ["t9"]);
}

/// A change's refusal is kept until the next change is taken.
#[test]
fn a_refusal_is_kept_until_the_next_change() {
    let mut p = peer();
    let mut s: Store<View> = Store::new();
    assert!(s.send(&mut p, "demo.create_playlist", &CreatePlaylist { name: " ".into() }).is_err());
    assert_eq!(s.refusal(), Some("a playlist needs a name"));
    assert!(s.send(&mut p, "demo.create_playlist", &CreatePlaylist { name: "Road".into() }).is_ok());
    assert_eq!(s.refusal(), None);
}
