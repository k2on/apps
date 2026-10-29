//! What the demo's own library comes out as, walked the way the demo walks
//! it. A silent fallback is the hardest kind of broken to see — a grid of
//! derived squares is the same picture whether covers work or never once
//! ran — so these are tests rather than readings.
use std::collections::{BTreeMap, BTreeSet};

use arkui::images::Images;

use crate::peer::Peer;
use crate::places::{Place, Source};
use crate::{licences, media_url, seed};

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
    let names: Vec<String> = peer.album_rows(&peer.album_id("The Four Seasons")).into_iter().map(|i| i.title).collect();
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
    let album = peer.album_rows(&peer.album_id("Concertos"));
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

/// …and the licence is still on screen, once, at the foot of the record:
/// a credit nobody draws is a condition nobody met. Every licensed track's
/// terms are in its album's foot, beside whoever gave them where the album
/// has more than one; a record that reserves nothing has no terms. Falsified
/// by dropping the licence from `licences`.
#[test]
fn the_terms_are_still_drawn_beside_whoever_gave_them() {
    let details = fresh().track_details();
    assert!(details.iter().filter(|d| !d.licence.is_empty()).count() > 5);
    let mut albums: BTreeMap<&str, Vec<crate::rows::TrackDetail>> = BTreeMap::new();
    for d in &details {
        albums.entry(&d.album).or_default().push(d.clone());
    }
    let mut several = false;
    for (album, on_it) in &albums {
        let foot = licences(on_it);
        let terms: BTreeSet<&str> = on_it.iter().map(|d| d.licence.as_str()).filter(|l| !l.is_empty()).collect();
        assert_eq!(foot.len(), terms.len(), "{album}: {foot:?}");
        for d in on_it.iter().filter(|d| !d.licence.is_empty()) {
            let line = foot
                .iter()
                .find(|f| f.starts_with(&d.licence))
                .unwrap_or_else(|| panic!("{album}: {}", d.licence));
            if terms.len() > 1 && !d.performer.is_empty() {
                several = true;
                assert!(line.contains(&d.performer), "{album}: {line}");
            }
        }
    }
    assert!(several, "an album of several terms says whose each is");
    assert!(albums.values().any(|on_it| licences(on_it).is_empty()), "most of it reserves nothing");
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
    let water = peer.album_id("Water Music");
    assert_eq!(peer.source_of(&Place::Album(water.clone())), Source::Album(water, "Water Music".into()));
    assert_eq!(
        peer.source_of(&Place::Album("Water Music".into())),
        Source::Library,
        "an album by its key, not its name"
    );
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

/// The demo has a podcast and a series of sermons: a card each, and a
/// show's page its episodes newest first, which the Songs page leaves
/// out. An episode's row goes to its show.
#[test]
fn the_demo_has_a_podcast_and_sermons() {
    let mut app = super::demo_app();
    let sermons = app.peer.shows_of("sermon");
    assert_eq!(sermons.len(), 1);
    assert!(sermons[0].title.contains("Gospel in Life"), "{}", sermons[0].title);
    let podcasts = app.peer.shows_of("podcast");
    assert_eq!(podcasts.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["Full Time Nix"]);
    assert!(app.peer.items().iter().all(|i| i.kind == "song"), "the Songs page is the music");

    let _ = app.update(crate::Message::Select(Source::Podcasts));
    assert_eq!(app.peer.source.place(), Place::Podcasts);
    let show = Source::Show("podcast/full-time-nix".into(), "Full Time Nix".into());
    let _ = app.update(crate::Message::Select(show.clone()));
    let rows = app.peer.rows().to_vec();
    assert_eq!(rows.len(), 11, "the newest ten, and an older one with a transcript");
    assert!(rows.iter().all(|r| r.kind == "podcast" && r.creator == "Full Time Nix"));
    assert_eq!(rows[0].title, "NixCon 2026 Day Ⅰ", "newest first");
    assert_eq!(app.peer.source_of(&Place::Show("podcast/full-time-nix".into())), show);
    let (_, entries) = app.row_entries(rows[0].id).unwrap();
    assert!(entries.iter().any(|e| e.label == "Go to Full Time Nix"));
}

/// Who is on an episode is beside its row but its host, who is said once,
/// in its show's header; the keyboard steps from a row onto who is on it,
/// as its menu goes to them. A guest's page — opened so, or by a link
/// arriving cold — is every episode they are on, newest first, with their
/// web page; and an episode there plays as a show's do, through the
/// episodes on the page. Falsified by `guests` keeping the hosts: the host
/// was beside every row; and by `source_of` knowing only the artists and
/// the composers: the link landed on the library.
#[test]
fn a_guests_page_is_the_episodes_they_are_on() {
    use crate::nav::Mark;
    use crate::places::Pane;
    use arkui::vim::{Action, Motion};
    let jacek = "Jacek Galowicz";
    let mut app = super::demo_app();
    let show = Source::Show("podcast/full-time-nix".into(), "Full Time Nix".into());
    let _ = app.update(crate::Message::Select(show));
    let hosts: Vec<String> = app.hosts().into_iter().map(|h| h.name).collect();
    assert_eq!(hosts, ["Shahar \"Dawn\" Or"], "the host, from the newest episode");
    let rows = app.peer.rows().to_vec();
    assert!(
        rows.iter().flat_map(|r| app.guests(r)).all(|p| !hosts.contains(&p.name)),
        "nor on any row"
    );
    assert!(app.layout().cell(Mark::By(0)).is_some(), "the host in the header opens their page too");

    // From the newest episode's row, `l` along who is on it to Jacek, and
    // `<Enter>` opens his page.
    let k = app.guests(&rows[0]).iter().position(|p| p.name == jacek).expect("Jacek was at NixCon");
    let (_, entries) = app.row_entries(rows[0].id).unwrap();
    assert!(entries.iter().any(|e| e.label == format!("Go to {jacek}")), "its menu goes to him");
    app.pane = Pane::Tracks;
    app.cursors[Pane::Tracks as usize] = 0;
    let _ = app.act(Action::Move(Motion::Right(k + 1)));
    assert_eq!(Some(app.at(Pane::Tracks)), app.layout().cell(Mark::Guest(0, k)));
    let _ = app.act(Action::Activate);
    assert_eq!(app.peer.source, Source::Artist(jacek.into()));
    assert_eq!(
        app.peer.source_of(&Place::Artist(jacek.into())),
        Source::Artist(jacek.into()),
        "a link to him lands here"
    );

    let on: Vec<String> = app.peer.appearances().iter().map(|i| i.title.clone()).collect();
    assert_eq!(on, ["NixCon 2026 Day Ⅰ", "Nix Freaks 51", "Nix 2.23.0 release with Jacek Galowicz"]);
    let link = app.layout().cell(Mark::Link).and_then(|c| app.layout().message(c).cloned());
    assert!(matches!(link, Some(crate::Message::OpenLink(url)) if url == "https://galowicz.de"));
    // Not beside his own rows: who else was on them is.
    assert!(app.peer.appearances().iter().flat_map(|i| app.guests(i)).all(|p| p.name != jacek));

    let second = app.layout().cell(Mark::Episode(1)).unwrap();
    let _ = app.update(crate::Message::HoverAt(second));
    let _ = app.act(Action::Activate);
    let queue: Vec<String> = app.queue.iter().map(|t| t.title.clone()).collect();
    assert_eq!(queue, on, "the queue is his episodes");
    assert_eq!(app.now().map(|n| n.track().title.clone()).as_deref(), Some("Nix Freaks 51"));
}

/// An episode's transport skips back fifteen seconds and on thirty; a
/// song's goes a track either way. Falsified by `jogs` reading the album
/// alone: a song of no album jogged.
#[test]
fn an_episode_skips_seconds_and_a_song_a_track() {
    let mut app = super::demo_app();
    let _ = app.update(crate::Message::Select(Source::Show(
        "podcast/full-time-nix".into(),
        "Full Time Nix".into(),
    )));
    let first = app.peer.rows()[0].id;
    let _ = app.update(crate::Message::PlayItem(first));
    assert!(app.jogs(), "an episode plays");
    assert!(matches!(app.forward(), crate::Message::Jog(30)));
    assert!(matches!(app.back(), crate::Message::Jog(-15)));
    let _ = app.update(crate::Message::Select(Source::Library));
    let song = app.peer.items()[0].id;
    let _ = app.update(crate::Message::PlayItem(song));
    assert!(!app.jogs(), "a song plays");
    assert!(matches!(app.forward(), crate::Message::Skip(1)));
}

/// A feed followed from the dialog a shows page's + opens is the library's,
/// as the kind of show that page lists: a feed's address pasted, the one
/// row it is; words, what the directory found, `j` onto one and `<Enter>`
/// following it; an answer for words typed over since, not listed. A
/// subscription keeps the newest twenty, its show's page offering older
/// episodes under its list once the server has said the feed has more and
/// its Unfollow in its ⋯; an episode whose file is not on the server yet
/// says so where its length goes; unfollowed, the show is gone. Falsified
/// by following as a podcast whatever the page: the sermons' feed was a
/// podcast; and by `length_of` reading the row's length alone: the queued
/// episode said its length.
#[test]
fn a_feed_is_followed_from_its_shows_page() {
    use crate::directory::Found;
    use crate::nav::Mark;
    use crate::places::Focus;
    const FEED: &str = "https://feeds.transistor.fm/full-time-nix";
    let mut app = super::demo_app();
    let _ = app.update(crate::Message::Select(Source::Sermons));
    let add = app.layout().cell(Mark::Add).expect("the + is a cell");
    let _ = app.update(crate::Message::HoverAt(add));
    app.keyed = None;
    let _ = app.act(arkui::vim::Action::Activate);
    assert_eq!(app.focus(), Focus::Adding, "<Enter> on the + opens the dialog, and it has the keys");
    let _ = app.update(crate::Message::Query("https://example.org/sermons.xml".into()));
    assert_eq!(app.adding.as_ref().unwrap().shows().len(), 1, "an address is the one row");
    let _ = app.act(arkui::vim::Action::Activate);
    assert_eq!(app.adding.as_ref().unwrap().said, None, "followed, nothing to say");
    let _ = app.act(arkui::vim::Action::Cancel);
    assert!(app.adding.is_none(), "<Esc> shuts it");

    let _ = app.update(crate::Message::Select(Source::Podcasts));
    let _ = app.update(crate::Message::Adding(true));
    let _ = app.update(crate::Message::Query("full time nix".into()));
    let nix = Found {
        title: "Full Time Nix".into(),
        author: "Shahar \"Dawn\" Or".into(),
        feed: FEED.into(),
        art: String::new(),
    };
    let _ = app.update(crate::Message::Searched("full time".into(), Ok(vec![Found::default()])));
    assert!(app.adding.as_ref().unwrap().found.is_empty(), "an answer for words typed over");
    let _ = app.update(crate::Message::Searched("full time nix".into(), Ok(vec![Found::default(), nix])));
    let _ = app.act(arkui::vim::Action::Move(arkui::vim::Motion::Down(1)));
    let _ = app.act(arkui::vim::Action::Activate);
    let _ = app.act(arkui::vim::Action::Cancel);

    let subs: Vec<(String, String, i64)> = app.peer.subscriptions().iter().map(|s| (s.url.clone(), s.kind.clone(), s.keep)).collect();
    assert_eq!(subs.len(), 2, "{subs:?}");
    assert!(subs.contains(&("https://example.org/sermons.xml".into(), "sermon".into(), 20)));
    assert!(subs.contains(&(FEED.into(), "podcast".into(), 20)));
    assert!(app.peer.subscription_of("sermon/gospel-in-life").is_none(), "nobody follows its feed");
    // Not read yet: on its page over the shows, with its Unfollow.
    let _ = app.update(crate::Message::Select(Source::Sermons));
    assert_eq!(app.unread_feeds().len(), 1);
    assert!(app.layout().cell(Mark::Unfollow(0)).is_some());

    // The server's read: the feed has fifty, and the library keeps twenty.
    let read = crate::domain::FeedRead {
        url: FEED.into(),
        title: "Full Time Nix".into(),
        items: 50,
        error: String::new(),
    };
    assert!(app.author("say a read".into(), "feed_read", &read));
    app.peer.refresh();
    let id = "podcast/full-time-nix";
    let sub = app.peer.subscription_of(id).expect("its show followed").clone();
    assert!(sub.has_older());
    let _ = app.update(crate::Message::Select(Source::Show(id.into(), "Full Time Nix".into())));
    let older = app.layout().cell(Mark::Older).expect("older ones under the list");
    assert!(older > app.layout().page.main, "after the episodes");
    let _ = app.update(crate::Message::HoverAt(older));
    let _ = app.act(arkui::vim::Action::Activate);
    assert_eq!(app.peer.subscription_of(id).unwrap().keep, 40);
    let first = app.peer.rows()[0].clone();
    assert!(app.peer.episode_of(first.id).is_some(), "what each episode is about, beside it");

    // An episode in the library before its file: queued, then how far.
    let queue = crate::domain::QueueDownload {
        file: first.file.clone(),
        source: "https://media/1.mp3".into(),
    };
    assert!(app.author("queue".into(), "queue_download", &queue));
    app.peer.refresh();
    assert_eq!(app.length_of(&first), "queued");
    assert_eq!(app.peer.download_of(first.id).unwrap().said(), "Not on the server yet");
    let going = crate::domain::DownloadProgress {
        file: first.file.clone(),
        state: "downloading".into(),
        bytes: 25,
        total: 100,
        error: String::new(),
    };
    assert!(app.author("progress".into(), "download_progress", &going));
    app.peer.refresh();
    assert_eq!(app.length_of(&first), "25%");
    assert!(app.author("here".into(), "downloaded", &crate::domain::Downloaded { file: first.file.clone() }));
    app.peer.refresh();
    assert_ne!(app.length_of(&first), "25%", "its length again");

    let _ = app.update(crate::Message::ShowMenu(id.into()));
    let _ = app.update(crate::Message::MenuActivate);
    assert!(
        app.peer.shows_of("podcast").is_empty(),
        "unfollowed from its ⋯, the show went with its feed"
    );
    assert_eq!(app.peer.subscriptions().len(), 1);
}

/// An episode begun says how far its listener has got where its album
/// would be, and its menu resumes it, marks it played or not, and goes to
/// each of its chapters. Falsified by `listened` reading another person's
/// row as the signed-in one's: "you" was nobody.
#[test]
fn an_episode_keeps_each_listeners_place_and_its_chapters() {
    let mut app = super::demo_app();
    let id = "podcast/full-time-nix";
    let _ = app.update(crate::Message::Select(Source::Show(id.into(), "Full Time Nix".into())));
    let first = app.peer.rows()[0].clone();
    let place = crate::domain::ReportProgress {
        media_id: crate::peer::key(first.id),
        position_ms: first.duration_ms / 2,
        duration_ms: first.duration_ms,
        name: "Ada".into(),
    };
    assert!(app.author("keep the place".into(), "report_progress", &place));
    app.peer.refresh();
    assert_eq!(app.listened(first.id), "you 50%");
    let label = |app: &crate::App| app.row_entries(first.id).unwrap().1.iter().map(|e| e.label.clone()).collect::<Vec<_>>();
    assert!(label(&app)[0].starts_with("Resume from "), "{:?}", label(&app));
    assert!(label(&app).contains(&"Mark as played".to_string()));

    let _ = app.update(crate::Message::MarkPlayed(first.id, true));
    assert_eq!(app.listened(first.id), "you played");
    assert_eq!(label(&app)[0], "Play", "played: from the start");
    assert!(label(&app).contains(&"Mark as unplayed".to_string()));

    let chapters = crate::domain::SetChapters {
        file: first.file.clone(),
        chapters: vec![
            crate::domain::ChapterAt {
                start_ms: 0,
                title: "Intro".into(),
                ..Default::default()
            },
            crate::domain::ChapterAt {
                start_ms: 65_000,
                title: "Flakes".into(),
                ..Default::default()
            },
        ],
    };
    assert!(app.author("chapters".into(), "set_chapters", &chapters));
    app.peer.refresh();
    let labels = label(&app);
    assert!(
        labels.contains(&"0:00  Intro".to_string()) && labels.contains(&"1:05  Flakes".to_string()),
        "{labels:?}"
    );
}
