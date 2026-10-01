//! Every list the window holds is its query, after every kind of change the
//! window makes or receives — and a change costs the lists it touches, patch
//! by patch, and no others (`docs/plan-v4.md` §1.5).
use std::collections::{BTreeSet, HashMap};

use ark_client::{args, Value};
use harken_domain::view::Item;

use crate::peer::{Moved, Peer};
use crate::places::Source;
use crate::rows::{Album, Artist, Composer, Playlist, TrackDetail};

/// Every open list against its query, read once.
fn check(peer: &Peer, what: &str) {
    let lib = args([("playlist_id", Value::Id(peer.playlist))]);
    assert_eq!(peer.items, peer.ask("library", lib, Item::from_value), "library, {what}");
    assert_eq!(
        peer.playlists(),
        peer.ask("playlists", args([]), Playlist::from_value),
        "playlists, {what}"
    );
    assert_eq!(peer.albums, peer.ask("albums", args([]), Album::from_value), "albums, {what}");
    assert_eq!(peer.artists, peer.ask("artists", args([]), Artist::from_value), "artists, {what}");
    assert_eq!(peer.composers, peer.ask("composers", args([]), Composer::from_value), "composers, {what}");
    let details: HashMap<_, _> = peer
        .ask("track_details", args([]), TrackDetail::from_value)
        .into_iter()
        .map(|t| (t.media_id, t))
        .collect();
    assert_eq!(peer.details, details, "track_details, {what}");
    let Source::Album(name) = &peer.source else { unreachable!() };
    assert_eq!(peer.shown, peer.album_rows(name), "the album page, {what}");
    assert_eq!(
        peer.choices[0].count,
        Some(peer.items.len() as i64),
        "the Songs line counts the library, {what}"
    );
}

/// Which lists the last refresh moved, and that each was patched rather than
/// read again.
fn patched(peer: &Peer) -> BTreeSet<&'static str> {
    for (name, m) in &peer.moved {
        assert!(matches!(m, Moved::Patched(_)), "{name} was taken whole");
    }
    peer.moved.iter().map(|(name, _)| *name).collect()
}

/// Over the demo library, with an album page open: a playlist toggle, a
/// person described, a song added to the open album, the toggle undone and a
/// song removed — after each, every list equals its query, and the first two
/// moved exactly the lists they touch, by patches. A toggle costs the
/// sidebar nothing, which is what the `lists_only` special case in `refresh`
/// used to buy by hand.
///
/// Falsified three ways: dropping the `Update` arm of `peer::splice` (the
/// toggle leaves the old `playlist_pos`, and "library, a toggle" fails);
/// making `Open::update` answer `Reset` always ("playlists was taken whole");
/// and dropping `patch_details`' map insert (track_details, a song added).
#[test]
fn every_list_on_screen_is_its_query_after_every_change() {
    let mut peer = super::demo_app().peer;
    peer.source = Source::Album("Water Music".into());
    peer.open_page();
    check(&peer, "opened");
    let favorites = peer.playlist;
    let off = peer.shown.iter().find(|i| !i.on_playlist()).expect("a track not on Favorites").id;
    let on = |id| args([("playlist_id", Value::Id(favorites)), ("media_id", Value::Id(id))]);

    peer.client.mutate("add_to_playlist", on(off)).unwrap();
    assert!(peer.refresh());
    check(&peer, "a toggle");
    assert_eq!(patched(&peer), BTreeSet::from(["library", "album"]));

    let t = Value::text;
    peer.client
        .mutate(
            "describe_person",
            args([
                ("name", t("George Frideric Handel")),
                ("sort_name", t("")),
                ("born", Value::Int(0)),
                ("died", Value::Int(0)),
                ("art", t("handel.jpg")),
            ]),
        )
        .unwrap();
    assert!(peer.refresh());
    check(&peer, "a person described");
    assert_eq!(patched(&peer), BTreeSet::from(["artists", "composers"]));

    let mut song = super::song("Hornpipe again", "George Frideric Handel", "Water Music", "again.mp3");
    song.insert("track".into(), Value::Int(30));
    peer.client.mutate("add_song", song).unwrap();
    peer.refresh();
    check(&peer, "a song added");
    assert!(peer.shown.iter().any(|i| i.title == "Hornpipe again"), "on the open page");

    peer.client.mutate("remove_from_playlist", on(off)).unwrap();
    peer.refresh();
    check(&peer, "the toggle undone");

    let gone = peer.items[3].id;
    peer.client.mutate("remove_media", args([("id", Value::Id(gone))])).unwrap();
    peer.refresh();
    check(&peer, "a song removed");
    assert!(!peer.details.contains_key(&gone));

    assert!(!peer.refresh(), "nothing moved, nothing to splice");
}

/// `docs/plan-db.md` D4: the search box drives a view. On the Songs page `/`
/// and a needle narrow the list, keystroke by keystroke, to the domain's
/// `search` — the tracks whose title or creator holds what is typed, case
/// folded — and that list is kept like any other: a song added that matches
/// is patched in. `<Enter>` keeps the search and lands on its first row;
/// `<Esc>` widens the page back to the library. Falsified by leaving
/// `Peer::rows` on the library while a search is up: "narrowed as typed"
/// fails at the first letter.
#[test]
fn the_search_box_drives_a_view() {
    use iced::keyboard::{key::Named, Key, Modifiers};

    use crate::places::Pane;
    use crate::{App, Message};

    let mut app = super::demo_app();
    app.pane = Pane::Tracks;
    let press = |app: &mut App, k: Key| {
        let _ = app.update(Message::Key(k, Modifiers::empty()));
    };
    let search = |app: &App, needle: &str| {
        app.peer.ask(
            "search",
            args([("playlist_id", Value::Id(app.peer.playlist)), ("needle", Value::text(needle))]),
            Item::from_value,
        )
    };
    let all = app.peer.rows().len();
    press(&mut app, Key::Character("/".into()));
    assert_eq!(app.peer.rows().len(), all, "nothing typed yet: the library");
    let mut typed = String::new();
    for c in "HANDEL".chars() {
        typed.push(c);
        press(&mut app, Key::Character(c.to_string().into()));
        assert_eq!(app.peer.rows(), &search(&app, &typed)[..], "narrowed as typed: {typed}");
    }
    let n = app.peer.rows().len();
    assert!(n > 0 && n < all, "{n} of {all}");
    for i in app.peer.rows() {
        let hay = format!("{} {}", i.title, i.creator).to_lowercase();
        assert!(hay.contains("handel"), "{hay}");
    }
    press(&mut app, Key::Named(Named::Enter));
    assert_eq!(app.peer.search.as_deref(), Some("HANDEL"), "<Enter> keeps it");
    assert_eq!(app.at(Pane::Tracks), 0);

    app.peer
        .client
        .mutate(
            "add_song",
            super::song("Hornpipe again", "George Frideric Handel", "", "music/hornpipe.mp3"),
        )
        .unwrap();
    assert!(app.peer.refresh());
    assert_eq!(app.peer.rows().len(), n + 1, "a match arriving is in the list");
    assert_eq!(app.peer.rows(), &search(&app, "HANDEL")[..]);

    press(&mut app, Key::Named(Named::Escape));
    assert_eq!(app.peer.search, None);
    assert_eq!(app.peer.rows().len(), all + 1, "<Esc> is the library again");
}
