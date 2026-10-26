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
    let albums: Vec<&str> = peer.albums().iter().filter(|a| !a.art.is_empty()).map(|a| a.name.as_str()).collect();
    let artists: Vec<&str> = peer.artists().iter().filter(|a| !a.art.is_empty()).map(|a| a.name.as_str()).collect();
    assert!(albums.contains(&"Water Music"), "albums with art: {albums:?}");
    assert!(artists.contains(&"George Frideric Handel"), "artists with art: {artists:?}");
    let mut want = Images::new("harken-test");
    let asked = peer
        .albums()
        .iter()
        .map(|a| a.art.clone())
        .chain(peer.artists().iter().map(|a| a.art.clone()))
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
    let _ = app.update(crate::Message::Wake);
    assert_eq!(app.art_seen, app.peer.art_gen, "the generation was seen");
    let url = media_url("", &app.peer.albums().iter().find(|a| a.name == "Water Music").unwrap().art);
    assert!(app.covers.want(url).is_none(), "…because it was already asked for");
}

/// A scanned library's covers are paths, as its tracks are — a picture
/// taken out of a file at `.art/…`, one beside a record at `.cover/<v>/…`
/// — and the window asks for each at `<server>/media/<path>`, the join it
/// makes for a track, then draws whatever arrives under that URL. The
/// server's half (`harken-server`'s `tests/library.rs`) fetches the same
/// values through the same join, `harken::listening::url`.
///
/// Falsified by asking for the bare `art` in `want_covers`: nothing was
/// asked at the media route.
#[test]
fn a_scanned_cover_is_asked_for_at_the_media_route_and_drawn() {
    let mut app = super::demo_app();
    app.server = "https://harken.example.com".into();
    let (extracted, beside) = (".art/5f3c.png", ".cover/0123456789abcdef/music/Handel/Water Music/cover.jpg");
    app.peer.albums_mut().iter_mut().find(|a| a.name == "Water Music").unwrap().art = extracted.into();
    app.peer
        .artists_mut()
        .iter_mut()
        .find(|a| a.name == "George Frideric Handel")
        .unwrap()
        .art = beside.into();
    let _ = app.want_covers();
    let at = |path: &str| format!("https://harken.example.com/media/{path}");
    let beside_url = at(".cover/0123456789abcdef/music/Handel/Water%20Music/cover.jpg");
    for url in [at(extracted), beside_url.clone()] {
        assert_eq!(app.art_url(if url.contains(".art") { extracted } else { beside }), url);
        assert!(app.covers.want(url.clone()).is_none(), "{url} was already asked for");
    }
    assert!(app.covers.handle(&app.art_url(extracted)).is_none(), "nothing to draw until it arrives");
    app.covers.loaded(arkui::images::Loaded {
        url: at(extracted),
        image: Ok(arkui::images::Rgba {
            width: 1,
            height: 1,
            pixels: vec![0; 4],
        }),
    });
    assert!(
        app.covers.handle(&app.art_url(extracted)).is_some(),
        "what arrived is what the card draws"
    );
}

/// The demo is composers with works: the Goldbergs one work, one recording,
/// thirty-two movements. Falsified by sending `catalogue` empty in the seed:
/// BWV 988 is gone.
#[test]
fn the_demo_is_composers_with_works() {
    let peer = fresh();
    assert!(
        peer.composers().len() >= 5,
        "{:?}",
        peer.composers().iter().map(|c| &c.name).collect::<Vec<_>>()
    );
    let bach = peer
        .composers()
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
    let album = peer.album_rows("Concertos");
    assert_eq!(album.len(), 16);
    let files: BTreeSet<_> = album.iter().map(|i| i.file.clone()).collect();
    let works = peer.works_of("Antonio Vivaldi");
    // A work may be recorded elsewhere too (RV 580 is on L'estro armonico):
    // the takes are the ones whose tracks are this album's.
    let takes: Vec<_> = ["RV 425", "RV 498", "RV 532", "RV 536", "RV 558", "RV 580"]
        .iter()
        .map(|cat| {
            let w = works.iter().find(|w| w.catalogue == *cat).unwrap_or_else(|| panic!("{cat}"));
            let here: Vec<_> = peer
                .recordings_of(&w.id)
                .into_iter()
                .filter(|t| peer.recording_rows(&t.id).iter().any(|i| files.contains(&i.file)))
                .collect();
            assert_eq!(here.len(), 1, "{cat}");
            (*cat, here[0].clone())
        })
        .collect();
    assert_eq!(takes[0].1.tracks, 1, "RV 425 arrived whole, in one file");
    for (cat, t) in &takes[1..] {
        assert_eq!(t.tracks, 3, "{cat}");
    }
    let who: BTreeSet<_> = takes.iter().map(|(_, t)| t.performers.clone()).collect();
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
    for c in peer.composers() {
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
/// from `credit`: the drawn string has no "CC BY", nor any licence.
#[test]
fn the_terms_are_still_drawn_beside_whoever_gave_them() {
    let details = fresh().track_details();
    let licensed: Vec<_> = details.iter().filter(|d| !d.licence.is_empty()).collect();
    assert!(licensed.len() > 5, "{}", licensed.len());
    let cc = licensed.iter().find(|d| d.licence.contains("CC BY")).expect("a CC BY recording");
    assert!(credit(&cc.performer, &cc.licence).contains("CC BY"));
    for d in &licensed {
        assert!(credit(&d.performer, &d.licence).contains(&d.licence), "{}", d.licence);
    }
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
    assert!(peer.composers().iter().any(|c| c.name == "Johann Sebastian Bach"));

    peer.source = Source::Artist("Johann Sebastian Bach".into());
    peer.open_page();
    let goldbergs = peer
        .works()
        .iter()
        .find(|w| w.catalogue == "BWV 988")
        .expect("Bach's page lists the Goldbergs")
        .clone();
    assert_eq!(goldbergs.title, "Goldberg Variations");

    peer.source = Source::Work(goldbergs.id.clone(), goldbergs.title.clone());
    peer.open_page();
    assert_eq!(peer.works().len(), 1, "the work's own row, for the header");
    assert_eq!(peer.works()[0].catalogue, "BWV 988");
    assert_eq!(peer.recordings().len(), 1);
    let take = peer.recordings()[0].clone();

    peer.source = Source::Recording(take.id.clone(), String::new());
    peer.open_page();
    assert!(peer.rows().len() > 20, "{}", peer.rows().len());
    assert_eq!(
        peer.recordings().iter().find(|r| r.id == take.id).map(|r| r.tracks),
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
    let id = crate::domain::host::work_key("Johann Sebastian Bach", "BWV 988", "Goldberg Variations");
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

    let mut bare = super::alone("demo");
    bare.ensure_playlist();
    let lines: Vec<&str> = bare.choices.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(lines, ["Songs", "Favorites"], "an empty library");
}

/// A track names its work by catalogue number alone, so two works sharing
/// one would silently give a work somebody else's title (the catalogue's
/// own checks refuse it too).
#[test]
fn the_catalogue_is_the_key() {
    let mut seen = BTreeSet::new();
    for w in &seed::catalogue().works {
        assert!(seen.insert(&w.catalogue), "two works both claim {}: {}", w.catalogue, w.title);
    }
}

/// A performance's Play, on a work's page where no track is listed, queues
/// its movements from the first and says it came from the performance; a
/// work's Play is its most complete performance's. Falsified by queueing
/// `rows()`, as a track's Play does: the work's page has none, and nothing
/// was queued.
#[test]
fn a_featured_play_queues_the_performance() {
    let mut app = super::demo_app();
    let id = crate::domain::host::work_key("Johann Sebastian Bach", "BWV 988", "Goldberg Variations");
    let _ = app.update(crate::Message::Select(Source::Work(id.clone(), "Goldberg Variations".into())));
    assert!(app.peer.rows().is_empty(), "a work's page lists performances");
    let take = app.peer.recordings()[0].clone();
    let titles = |q: &[crate::listening::Track]| q.iter().map(|t| t.title.clone()).collect::<Vec<_>>();
    let movements: Vec<String> = app.peer.recording_rows(&take.id).into_iter().map(|i| i.title).collect();

    let _ = app.update(crate::Message::PlayRecording(take.id.clone()));
    assert_eq!(titles(&app.queue), movements);
    assert_eq!(app.queue.len() as i64, take.tracks);
    assert_eq!(app.queued_from, arkui::route::Route::fragment(&Place::Recording(take.id.clone())));

    app.queue.clear();
    let _ = app.update(crate::Message::PlayWork(id));
    assert_eq!(titles(&app.queue), movements, "the most complete performance");
}

/// A composer's page lists their best known works first and the rest
/// behind "See All", by catalogue; a page opened again lists the best known
/// again. Falsified by ranking on tracks alone: the work of two short
/// performances came after the one long one.
#[test]
fn the_best_known_works_are_the_most_recorded() {
    let work = |title: &str, recordings: i64, tracks: i64| crate::rows::Work {
        title: title.into(),
        recordings,
        tracks,
        ..Default::default()
    };
    let works = [work("Air", 1, 9), work("Bourrée", 2, 2), work("Chaconne", 1, 9), work("Gigue", 2, 4)];
    let ranked: Vec<&str> = crate::known(&works).iter().map(|w| w.title.as_str()).collect();
    assert_eq!(ranked, ["Gigue", "Bourrée", "Air", "Chaconne"]);

    let mut app = super::demo_app();
    let _ = app.update(crate::Message::Select(Source::Artist("Johann Sebastian Bach".into())));
    let all = app.peer.works().len();
    assert_eq!(app.listed_works().len(), all.min(crate::KNOWN_WORKS));
    let _ = app.update(crate::Message::AllWorks(true));
    assert_eq!(app.listed_works().len(), all);
    let _ = app.update(crate::Message::Select(Source::Artist("Johann Sebastian Bach".into())));
    assert!(!app.all_works, "gone with the page");
}

/// A person has one page: a composer's card, an old `#composer/…` link and
/// the artist's card all open it, and it lists their works where some are
/// by them and their songs where none are. Falsified by opening `works`
/// only for a composer's card: the artist's card opened a page of no works.
#[test]
fn a_composer_and_an_artist_are_one_page() {
    let mut app = super::demo_app();
    let bach = "Johann Sebastian Bach".to_string();
    assert_eq!(app.peer.source_of(&Place::Composer(bach.clone())), Source::Artist(bach.clone()));
    assert_eq!(app.peer.source_of(&Place::Artist(bach.clone())), Source::Artist(bach.clone()));
    let _ = app.update(crate::Message::Select(Source::Composers));
    let at = app.peer.composers().iter().position(|c| c.name == bach).unwrap();
    assert_eq!(app.card_under_cursor(at), Some(Source::Artist(bach.clone())));

    let _ = app.update(crate::Message::Select(Source::Artist(bach.clone())));
    assert!(!app.peer.works().is_empty(), "a composer's page lists their works");
    let first = app.listed_works()[0].clone();
    assert_eq!(app.card_under_cursor(0), Some(Source::Work(first.id, first.title)));

    let pop = app
        .peer
        .artists()
        .iter()
        .find(|a| !app.peer.composers().iter().any(|c| c.name == a.name))
        .map(|a| a.name.clone());
    if let Some(pop) = pop {
        let _ = app.update(crate::Message::Select(Source::Artist(pop)));
        assert!(app.peer.works().is_empty());
        assert_eq!(app.card_under_cursor(0), None, "<Enter> plays a song");
    }
}

/// The catalogue's words reach the pages: a composer's description on
/// their page, and a work's on its own. Falsified by `describe_person` or
/// `describe_work` leaving `description` out: the rows had none.
#[test]
fn the_catalogue_describes_its_people_and_works() {
    let mut app = super::demo_app();
    let bach = app
        .peer
        .composers()
        .iter()
        .find(|c| c.name == "Johann Sebastian Bach")
        .cloned()
        .expect("Bach composed something here");
    assert!(bach.description.contains("Leipzig"), "{}", bach.description);
    let id = crate::domain::host::work_key("Johann Sebastian Bach", "BWV 988", "Goldberg Variations");
    let _ = app.update(crate::Message::Select(Source::Work(id.clone(), "Goldberg Variations".into())));
    let work = app.peer.works().iter().find(|w| w.id == id).cloned().expect("the work's own row");
    assert!(work.description.contains("thirty variations"), "{}", work.description);
}

/// Falsified by `add_song` dropping the catalogue's year: the Goldberg
/// Variations came out in nobody-said, and by a track without one wiping
/// the year its album has.
#[test]
fn the_catalogue_dates_its_albums() {
    let app = super::demo_app();
    let year = |name: &str| app.peer.albums().iter().find(|a| a.name == name).map(|a| a.released);
    assert_eq!(year("Goldberg Variations, BWV 988"), Some(2012));
    assert_eq!(year("Messiah"), Some(0), "nobody said");
    let goldberg = app
        .peer
        .albums()
        .iter()
        .find(|a| a.name == "Goldberg Variations, BWV 988")
        .cloned()
        .unwrap();
    assert_eq!(goldberg.performer, "Kimiko Ishizaka", "who made it");
}
