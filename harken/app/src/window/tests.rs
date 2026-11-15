//! harken in a window its host draws itself, over the demo library on the
//! host's test place: the address says where it is and brings a fresh
//! window there, Settings is the window's own, and every name the shell
//! hosts is drawn from harken's views.

use ark_app::host::{Hosting, Link};
use ark_app::view::View;
use ark_app::window::Windowed;

use super::HarkenMsg;
use crate::host::tests::{hosted, Demo};
use crate::host::Harken;
use crate::library::LibraryMsg;
use crate::shell::ShellMsg;
use crate::testing::NOW;

fn send(h: &mut Hosting<Harken>, place: &mut Demo, msg: HarkenMsg) {
    h.with(place, |app, cx| app.take(msg, cx));
    for n in h.take_natives() {
        h.with(place, |app, cx| app.native(n, cx));
    }
}

fn address(h: &Hosting<Harken>, place: &Demo) -> String {
    h.app().address(place, NOW)
}

/// A fresh window brought to `address`, as a page loaded at it is.
fn arrive(address: &str) -> (Hosting<Harken>, Demo) {
    let (mut h, mut place) = hosted();
    h.link(Link::Address(address.into()), &mut place);
    for n in h.take_natives() {
        h.with(&mut place, |app, cx| app.native(n, cx));
    }
    (h, place)
}

/// The address follows the place: a section, the library's kind, a page
/// pushed, Settings' pane; and a fresh window brought to each is there
/// again. Falsified by an address that stays put, or one a window cannot
/// arrive back at.
#[test]
fn the_address_says_the_place_and_brings_a_window_back() {
    let (mut h, mut place) = hosted();
    assert_eq!(address(&h, &place), "/");
    send(&mut h, &mut place, HarkenMsg::Shell(ShellMsg::Select { tag: "search".into() }));
    assert_eq!(address(&h, &place), "/search/");
    send(&mut h, &mut place, HarkenMsg::Shell(ShellMsg::Select { tag: "library".into() }));
    assert_eq!(address(&h, &place), "/library/playlists/");
    send(&mut h, &mut place, HarkenMsg::Library(LibraryMsg::Show { kind: "albums".into() }));
    assert_eq!(address(&h, &place), "/albums/");
    let album = h.app().library.page(&place, NOW).albums[0].clone();
    let open = LibraryMsg::OpenAlbum {
        key: album.key.clone(),
        title: album.title.clone(),
    };
    send(&mut h, &mut place, HarkenMsg::Library(open));
    let pushed = address(&h, &place);
    assert!(pushed.starts_with("/album/"), "{pushed}");

    for a in ["/", "/search/", "/library/playlists/", "/albums/", pushed.as_str()] {
        let (h, place) = arrive(a);
        assert_eq!(address(&h, &place), a, "a window brought to {a}");
    }
}

/// Settings is the window's place: Who Is Signed In opens it on General,
/// Done puts it away, and its pane is in the address.
#[test]
fn settings_is_the_windows_own() {
    let (mut h, mut place) = hosted();
    assert!(h.app().settings(&place, NOW).is_none());
    send(&mut h, &mut place, HarkenMsg::Shell(ShellMsg::Account));
    let (_, done) = h.app().settings(&place, NOW).expect("Settings up");
    assert_eq!(address(&h, &place), "/settings/?pane=general");
    send(&mut h, &mut place, HarkenMsg::Shell(ShellMsg::Pane { title: "Diagnostics".into() }));
    assert_eq!(address(&h, &place), "/settings/?pane=diagnostics");
    send(&mut h, &mut place, done);
    assert!(h.app().settings(&place, NOW).is_none());
    let (h, place) = arrive("/settings/?pane=diagnostics");
    assert!(h.app().settings(&place, NOW).is_some(), "a window brought to Settings");
    assert_eq!(address(&h, &place), "/settings/?pane=diagnostics");
}

/// Every name the shell hosts is one of harken's views, none the empty
/// place; a launch's look is the window's.
#[test]
fn every_hosted_name_is_drawn_and_a_launch_sets_the_look() {
    let (mut h, mut place) = hosted();
    for name in ["home", "library", "search", "page", "player.bar", "player"] {
        let v = h.app().hosted(name, &place, NOW);
        assert!(!matches!(v, View::Empty), "{name}");
        let words = format!("{v:?}");
        assert!(!words.contains("Nothing Selected"), "{name} is drawn as nothing chosen");
    }
    assert_eq!(h.app().appearance(), None);
    h.launched(vec!["-appearance".into(), "dark".into(), "-screen".into(), "library".into()], &mut place);
    assert_eq!(h.app().appearance(), Some("dark"));
    assert_eq!(address(&h, &place), "/library/playlists/");
    h.launched(vec!["-screen".into(), "albums".into()], &mut place);
    assert_eq!(address(&h, &place), "/albums/");
}
