//! What the demo's own library comes out as, walked the way the demo walks
//! it. A silent fallback is the hardest kind of broken to see — a grid of
//! derived squares is the same picture whether covers work or never once
//! ran — so these are tests rather than readings.
use std::collections::BTreeSet;

use arkui::images::Images;

use crate::peer::Peer;
use crate::places::{Place, Source};
use crate::{credit, media_url, seed};

/// A seeded demo replica, from nothing.
fn fresh() -> Peer {
    super::demo_app().peer
}

/// The seeded covers come back on the lists they are drawn on, and every one
/// is a fetch the window would make — through `media_url("", …)`, the demo's
/// join, which passes a Wikimedia URL through unchanged.
///
/// Falsified by sending `album_art` empty in the seed: "Water Music" has no
/// art and the first assertion fails.
#[test]
fn the_seeded_art_reaches_a_fetch() {
    let peer = fresh();
    let albums: Vec<&str> = peer.albums.iter().filter(|a| !a.art.is_empty()).map(|a| a.name.as_str()).collect();
    let artists: Vec<&str> = peer.artists.iter().filter(|a| !a.art.is_empty()).map(|a| a.name.as_str()).collect();
    assert!(albums.contains(&"Water Music"), "albums with art: {albums:?}");
    assert!(artists.contains(&"George Frideric Handel"), "artists with art: {artists:?}");
    let mut want = Images::new("harken-test");
    let asked = peer
        .albums
        .iter()
        .map(|a| a.art.clone())
        .chain(peer.artists.iter().map(|a| a.art.clone()))
        .filter(|art| !art.is_empty())
        .filter(|art| want.want(media_url("", art)).is_some())
        .count();
    assert_eq!(asked, albums.len() + artists.len(), "every seeded cover is a fetch");
}

/// …and the window asks for them itself, on the first update after boot,
/// because the lists moved — not because anything arrived on a wire the demo
/// does not have. Falsified by comparing `art_gen` against itself in
/// `want_covers_if_moved`: nothing is asked.
#[test]
fn the_window_asks_for_the_covers_the_lists_carry() {
    let mut app = super::demo_app();
    assert_eq!(app.art_seen, 0);
    let _ = app.update(crate::Message::Tick);
    assert_eq!(app.art_seen, app.peer.art_gen, "the generation was seen");
    let url = media_url("", &app.peer.albums.iter().find(|a| a.name == "Water Music").unwrap().art);
    assert!(app.covers.want(url).is_none(), "…because it was already asked for");
}

/// The demo is composers with works: the Goldbergs one work, one recording,
/// thirty-two movements. Falsified by sending `catalogue` empty in the seed:
/// BWV 988 is gone.
#[test]
fn the_demo_is_composers_with_works() {
    let peer = fresh();
    assert!(
        peer.composers.len() >= 5,
        "{:?}",
        peer.composers.iter().map(|c| &c.name).collect::<Vec<_>>()
    );
    let bach = peer
        .composers
        .iter()
        .find(|c| c.name == "Johann Sebastian Bach")
        .expect("Bach is in the demo");
    assert!(bach.works >= 3, "{}", bach.works);
    let works = peer.works_of("Johann Sebastian Bach");
    let goldbergs = works.iter().find(|w| w.catalogue == "BWV 988").expect("BWV 988");
    assert_eq!(goldbergs.recordings, 1);
    assert!(goldbergs.tracks > 20, "{}", goldbergs.tracks);
    let takes = peer.recordings_of(&goldbergs.id);
    assert_eq!(takes.len(), 1);
    assert_eq!(takes[0].performers, "Kimiko Ishizaka");
}

/// A work has a name of its own, not the record's: Book I is twenty-four
/// works with twenty-four names. Falsified by passing the album as
/// `work_title` in the seed: the set of titles collapses to one.
#[test]
fn a_work_is_named_for_itself() {
    let works = fresh().works_of("Johann Sebastian Bach");
    let wtc: Vec<&str> = works.iter().filter(|w| w.form == "Prelude and Fugue").map(|w| w.title.as_str()).collect();
    assert_eq!(wtc.len(), 24);
    assert_eq!(wtc.iter().collect::<BTreeSet<_>>().len(), 24, "{wtc:?}");
    assert!(wtc.contains(&"Prelude and Fugue No. 1 in C Major"));
    let brandenburgs: Vec<&str> = works
        .iter()
        .filter(|w| w.catalogue.starts_with("BWV 10"))
        .map(|w| w.title.as_str())
        .collect();
    assert_eq!(brandenburgs.len(), 6);
    assert!(brandenburgs.contains(&"Brandenburg Concerto No. 4 in G Major"));
}

/// **An album is a release, and a release may carry four works**: the Four
/// Seasons, four concertos of three movements on one record, asserted whole
/// (three are called "III. Allegro"). Falsified by giving the twelve one
/// catalogue number in the seed: the count drops to one.
#[test]
fn one_release_can_carry_four_works() {
    let peer = fresh();
    let works = peer.works_of("Antonio Vivaldi");
    let seasons: Vec<_> = works.iter().filter(|w| w.title.starts_with("The Four Seasons")).collect();
    assert_eq!(seasons.len(), 4, "{:?}", works.iter().map(|w| &w.title).collect::<Vec<_>>());
    for w in &seasons {
        assert_eq!(w.tracks, 3, "{}", w.catalogue);
        assert_eq!(w.recordings, 1);
    }
    let names: Vec<String> = peer.album_rows("The Four Seasons").into_iter().map(|i| i.title).collect();
    assert_eq!(
        names,
        [
            "I. Allegro",
            "II. Largo",
            "III. Allegro",
            "I. Allegro non molto",
            "II. Adagio",
            "III. Presto",
            "I. Allegro",
            "II. Adagio molto",
            "III. Allegro",
            "I. Allegro non molto",
            "II. Largo",
            "III. Allegro",
        ]
    );
}

/// **A compilation is a release too, and a work can be one track long.**
/// Falsified by moving RV 425 to the Modena orchestra in the seed: the set of
/// performers drops to one.
#[test]
fn a_compilation_carries_six_works_and_two_performers() {
    let peer = fresh();
    assert_eq!(peer.album_rows("Concertos").len(), 16);
    let works = peer.works_of("Antonio Vivaldi");
    let on_it: Vec<_> = ["RV 425", "RV 498", "RV 532", "RV 536", "RV 558", "RV 580"]
        .iter()
        .map(|cat| works.iter().find(|w| w.catalogue == *cat).unwrap_or_else(|| panic!("{cat}")))
        .collect();
    assert_eq!(on_it[0].tracks, 1, "RV 425 arrived whole, in one file");
    for w in &on_it[1..] {
        assert_eq!(w.tracks, 3, "{}", w.catalogue);
    }
    let mut who = BTreeSet::new();
    for w in &on_it {
        let takes = peer.recordings_of(&w.id);
        assert_eq!(takes.len(), 1, "{}", w.catalogue);
        who.insert(takes[0].performers.clone());
    }
    assert_eq!(
        who,
        ["The Milan Baroque Soloists", "The Modena Chamber Orchestra"].map(str::to_owned).into()
    );
}

/// **Nobody is called `(CC BY-SA 3.0)`**: the terms are the recording's, not
/// a person's. Looked for in `credit`, where such a person would sit — not in
/// `artists()`, which reads the composer. Falsified by crediting a licensed
/// recording to `({licence})` in the seed, which is what the old lumped
/// string did; putting the licence back into the performer string itself now
/// trips the seed's own assertion before this test runs — the mechanism
/// working.
#[test]
fn nobody_is_named_after_a_licence() {
    let peer = fresh();
    let mut credited = Vec::new();
    for c in &peer.composers {
        for w in peer.works_of(&c.name) {
            for take in peer.recordings_of(&w.id) {
                credited.extend(peer.credits(&take.id).into_iter().map(|c| c.name));
            }
        }
    }
    assert!(credited.len() > 5, "{credited:?}");
    let wrong: Vec<&String> = credited.iter().filter(|n| n.contains("CC BY") || n.contains('(')).collect();
    assert!(wrong.is_empty(), "{wrong:?}");
}

/// …and the licence is still on screen beside whoever gave it: a credit
/// nobody draws is a condition nobody met. Falsified by dropping the licence
/// from `credit`: the drawn string has no "CC BY".
#[test]
fn the_terms_are_still_drawn_beside_whoever_gave_them() {
    let details = fresh().track_details();
    let licensed: Vec<_> = details.iter().filter(|d| !d.licence.is_empty()).collect();
    assert!(licensed.len() > 5, "{}", licensed.len());
    assert!(credit(&licensed[0].performer, &licensed[0].licence).contains("CC BY"));
    let free = details
        .iter()
        .find(|d| d.licence.is_empty() && !d.performer.is_empty())
        .expect("most of it reserves nothing");
    assert_eq!(credit(&free.performer, &free.licence), free.performer);
}

/// One lumped string becomes two people with two roles, in billing order,
/// and the lumped fallback is displaced rather than left beside them.
/// Falsified by removing the Messiah rows from the seed's `CREDITS`.
#[test]
fn the_messiah_has_an_orchestra_and_a_conductor() {
    let peer = fresh();
    let works = peer.works_of("George Frideric Handel");
    let messiah = works.iter().find(|w| w.catalogue == "HWV 56").expect("HWV 56");
    let takes = peer.recordings_of(&messiah.id);
    assert_eq!(takes[0].performers, "London Symphony Orchestra, Hermann Scherchen");
    let credits = peer.credits(&takes[0].id);
    assert_eq!(
        credits.iter().map(|c| (c.name.as_str(), c.role.as_str())).collect::<Vec<_>>(),
        [("London Symphony Orchestra", "orchestra"), ("Hermann Scherchen", "conductor")]
    );
}

/// The whole path a person takes — Composers, a composer, a work, a
/// recording, its tracks — with every page's own rows loaded. Each page reads
/// a different list, so the way to get this wrong is a page drawing the last
/// one's rows, or none: invisible on any one page, obvious from a walk.
///
/// Falsified by reading the work's header row from `works_of` rather than by
/// key in `open_page`: the work page's `works` is empty.
#[test]
fn the_whole_path_from_a_composer_to_a_movement() {
    let mut peer = fresh();
    peer.source = Source::Composers;
    peer.open_page();
    assert!(peer.composers.iter().any(|c| c.name == "Johann Sebastian Bach"));

    peer.source = Source::Works("Johann Sebastian Bach".into());
    peer.open_page();
    let goldbergs = peer
        .works
        .iter()
        .find(|w| w.catalogue == "BWV 988")
        .expect("Bach's page lists the Goldbergs")
        .clone();
    assert_eq!(goldbergs.title, "Goldberg Variations");

    peer.source = Source::Work(goldbergs.id.clone(), goldbergs.title.clone());
    peer.open_page();
    assert_eq!(peer.works.len(), 1, "the work's own row, for the header");
    assert_eq!(peer.works[0].catalogue, "BWV 988");
    assert_eq!(peer.recordings.len(), 1);
    let take = peer.recordings[0].clone();

    peer.source = Source::Recording(take.id.clone(), String::new());
    peer.open_page();
    assert!(peer.rows().len() > 20, "{}", peer.rows().len());
    assert_eq!(
        peer.recordings.iter().find(|r| r.id == take.id).map(|r| r.tracks),
        Some(peer.rows().len() as i64)
    );
    assert_eq!(
        peer.source,
        Source::Recording(take.id, "Kimiko Ishizaka".into()),
        "a page titled with nothing"
    );
}

/// A work link resolves by its key, and one naming nothing lands on the
/// library. Falsified by resolving `Place::Work` against `works` (one
/// composer's, empty here) instead of by query.
#[test]
fn a_work_link_resolves_by_its_key() {
    let peer = fresh();
    let id = harken_domain::keys::work_key("Johann Sebastian Bach", "BWV 988", "Goldberg Variations");
    assert!(id.contains('/'), "{id}");
    assert_eq!(peer.source_of(&Place::Work(id.clone())), Source::Work(id, "Goldberg Variations".into()));
    assert_eq!(peer.source_of(&Place::Work("nobody/nothing".into())), Source::Library);
    assert_eq!(peer.source_of(&Place::Album("Water Music".into())), Source::Album("Water Music".into()));
    assert_eq!(peer.source_of(&Place::Album("Not a Record".into())), Source::Library);
}

/// The sidebar draws a line only when there are rows behind it — and an
/// empty library alone offers Songs and its own Favorites. Falsified by
/// pushing the Composers line unconditionally in `Peer::choose`.
#[test]
fn the_sidebar_only_offers_what_there_is() {
    let peer = fresh();
    let lines: Vec<&str> = peer.choices.iter().map(|c| c.label.as_str()).collect();
    assert!(lines.contains(&"Composers") && lines.contains(&"Albums"), "{lines:?}");

    let mut bare = super::peer(ark_client::Options::alone("demo"));
    bare.ensure_playlist();
    let lines: Vec<&str> = bare.choices.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(lines, ["Songs", "Favorites"], "an empty library");
}

/// `WORKS` is keyed by the catalogue number alone, so two sharing one would
/// silently give a work somebody else's title.
#[test]
fn the_catalogue_is_the_key() {
    let mut seen = BTreeSet::new();
    for w in seed::WORKS {
        assert!(seen.insert(w.catalogue), "two works both claim {}: {}", w.catalogue, w.title);
    }
}
