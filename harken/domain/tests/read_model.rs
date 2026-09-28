//! The read model, against rows the domain's own procedures wrote — every
//! call also held to the interpreter (see `common`). Ported from the old
//! harken's `tests/read_model.rs`, assertion for assertion where the
//! assertion still means something.

mod common;

use ark::value::Value;
use common::{args, ints, texts, track, Lib, Song};

fn with(file: &str, s: Song) -> Song {
    Song { file: file.into(), ..s }
}

#[test]
fn library_and_a_playlist_agree_with_what_apply_wrote() {
    let mut c = Lib::new();
    let favs = c.playlist("alice", "Favorites");
    for (t, a) in [("Glue", "Bicep"), ("Opal", "Bicep"), ("Gosh", "Jamie xx")] {
        c.add(Song::new(t, a, ""));
    }
    let all = c.library(favs);
    assert_eq!(
        texts(&all, "title"),
        ["Glue", "Opal", "Gosh"],
        "library order is pos, and pos counts up from MAX(pos)"
    );
    assert_eq!(all[0].field("creator"), Value::text("Bicep"));
    assert_eq!(all[0].field("user_id"), Value::text("alice"));
    assert_eq!(all[0].field("kind"), Value::text("song"));
    assert!(all.iter().all(|s| s.field("playlist_pos").is_null()), "nothing on the playlist yet");
    let on = |c: &Lib| c.list("playlist", args([("playlist_id", Value::Id(favs))]));
    assert!(on(&c).is_empty());

    // The third, then the first: the playlist is in the order they joined it.
    let (third, first) = (all[2].field("id"), all[0].field("id"));
    let add = |media: &Value| args([("playlist_id", Value::Id(favs)), ("media_id", media.clone())]);
    c.mutate("alice", "add_to_playlist", add(&third)).unwrap();
    c.mutate("alice", "add_to_playlist", add(&first)).unwrap();
    let playlist = on(&c);
    assert_eq!(texts(&playlist, "title"), ["Gosh", "Glue"], "playlist order is the item's pos");
    assert_eq!(ints(&playlist, "playlist_pos"), [1, 2]);

    // The library keeps its own order and carries the position on the rows
    // that have one: the left join, and the option that makes it one.
    let all = c.library(favs);
    let pos: Vec<Value> = all.iter().map(|s| s.field("playlist_pos")).collect();
    assert_eq!(pos, [Value::int(2), Value::Null, Value::int(1)]);
    assert_eq!(all[0].field("id"), playlist[1].field("id"));

    // Taking it off leaves the song; adding it twice keeps its first place.
    c.mutate("alice", "add_to_playlist", add(&third)).unwrap();
    assert_eq!(ints(&on(&c), "playlist_pos"), [1, 2], "a playlist holds an item once");
    c.mutate("alice", "remove_from_playlist", add(&first)).unwrap();
    assert_eq!(c.library(favs).len(), 3);
    assert_eq!(on(&c).len(), 1);
    // Somebody else's playlist is not theirs to change.
    assert_eq!(c.mutate("bob", "add_to_playlist", add(&first)), Err("not your playlist".into()));
    assert_eq!(c.mutate("", "add_to_playlist", add(&first)), Err("not your playlist".into()));
}

/// A name a person already has is not refused and not dropped: the new
/// playlist keeps its own id and is numbered, with the smallest free n.
#[test]
fn a_second_playlist_of_one_name_is_numbered() {
    let mut c = Lib::new();
    let names = |c: &Lib, who: &str| texts(&c.query(who, "playlists", args([])).unwrap().as_list(), "name");
    let make = |c: &mut Lib, who: &str, name: &str| c.mutate(who, "create_playlist", args([("name", Value::text(name))]));
    make(&mut c, "alice", "Favorites").unwrap();
    assert_eq!(make(&mut c, "alice", "Favorites"), Ok(1), "a second playlist, not nothing");
    // …and the shape a second client's would arrive in if somebody typed it.
    make(&mut c, "alice", "  Favorites  ").unwrap();
    assert_eq!(names(&c, "alice"), ["Favorites", "Favorites (1)", "Favorites (2)"]);

    // The smallest number free, not one more than the largest.
    make(&mut c, "alice", "Mix (2)").unwrap();
    make(&mut c, "alice", "Mix").unwrap();
    make(&mut c, "alice", "Mix").unwrap();
    make(&mut c, "alice", "Mix").unwrap();
    assert_eq!(
        names(&c, "alice")[3..],
        ["Mix (2)", "Mix", "Mix (1)", "Mix (3)"],
        "(1) was free, then (2) was taken"
    );
    // Case is not folded: a name is what somebody typed.
    make(&mut c, "alice", "favorites").unwrap();
    assert_eq!(names(&c, "alice").last().map(String::as_str), Some("favorites"));

    // The same entry twice is one playlist, not "(3)".
    let autos = c.autos("create_playlist");
    let gym = args([("name", Value::text("Gym"))]);
    assert_eq!(c.mutate_with("alice", "create_playlist", autos.clone(), gym.clone()), Ok(1));
    assert_eq!(c.mutate_with("alice", "create_playlist", autos, gym), Ok(0));
    assert_eq!(names(&c, "alice").iter().filter(|n| n.starts_with("Gym")).count(), 1);

    // Somebody else's names are not in the way; nobody (before signing in)
    // makes playlists like anybody else.
    make(&mut c, "bob", "Favorites").unwrap();
    assert_eq!(names(&c, "bob"), ["Favorites"]);
    make(&mut c, "", "Favorites").unwrap();
    make(&mut c, "", "Favorites").unwrap();
    assert_eq!(names(&c, ""), ["Favorites", "Favorites (1)"]);
    assert_eq!(make(&mut c, "alice", "   "), Err("a playlist needs a name".into()));
}

/// …and the rule is about a person, not about the library: Bob's
/// "Favorites" is not Alice's. Each sees their own (`playlists` is the
/// caller's), and both rows are in the one log.
#[test]
fn two_people_each_get_a_playlist_of_one_name() {
    let mut c = Lib::new();
    let a = c.playlist("alice", "Favorites");
    let b = c.playlist("bob", "Favorites");
    assert_ne!(a, b);
    for who in ["alice", "bob"] {
        let mine = c.query(who, "playlists", args([])).unwrap().as_list();
        assert_eq!(texts(&mine, "user_id"), [who]);
        assert_eq!(texts(&mine, "name"), ["Favorites"]);
    }
    assert_eq!(ark::store::Store::scan(&c.store, "playlist").len(), 2);
}

#[test]
fn albums_and_artists_group_the_library_and_select_it_back() {
    let mut c = Lib::new();
    let favs = c.playlist("alice", "Favorites");
    for (title, artist, album) in [
        ("Für Elise", "Beethoven", "Bagatelles"),
        ("Symphony No. 5 - I", "Beethoven", "Symphony No. 5"),
        ("Symphony No. 5 - III", "Beethoven", "Symphony No. 5"),
        ("Clair de lune", "Debussy", "Suite bergamasque"),
    ] {
        c.add(Song::new(title, artist, album));
    }

    let albums = c.list("albums", args([]));
    let got: Vec<(String, String, i64)> = albums
        .iter()
        .map(|a| {
            (
                a.field("name").as_text().into(),
                a.field("creator").as_text().into(),
                a.field("tracks").as_int(),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            ("Bagatelles".into(), "Beethoven".into(), 1),
            ("Suite bergamasque".into(), "Debussy".into(), 1),
            ("Symphony No. 5".into(), "Beethoven".into(), 2),
        ],
        "one row per album, in name order, with the tracks counted"
    );
    let artists = c.list("artists", args([]));
    assert_eq!(texts(&artists, "name"), ["Beethoven", "Debussy"]);
    assert_eq!(ints(&artists, "tracks"), [3, 1]);

    // Selecting one gives back the library's own rows.
    let album = |c: &Lib, name: &str| c.list("album", args([("playlist_id", Value::Id(favs)), ("name", Value::text(name))]));
    let symphony = album(&c, "Symphony No. 5");
    assert_eq!(texts(&symphony, "title"), ["Symphony No. 5 - I", "Symphony No. 5 - III"]);
    let beethoven = c.list("artist", args([("playlist_id", Value::Id(favs)), ("name", Value::text("Beethoven"))]));
    assert_eq!(beethoven.len(), 3);
    assert!(album(&c, "Nocturnes").is_empty());

    // Read against a playlist, like the library list is.
    c.mutate(
        "alice",
        "add_to_playlist",
        args([("playlist_id", Value::Id(favs)), ("media_id", symphony[0].field("id"))]),
    )
    .unwrap();
    let symphony = album(&c, "Symphony No. 5");
    assert_eq!(symphony[0].field("playlist_pos"), Value::int(1), "the membership survives the album view");
    assert!(symphony[1].field("playlist_pos").is_null());

    // An album whose last track has gone is not an album any more.
    let clair = c
        .library(favs)
        .into_iter()
        .find(|r| r.field("title") == Value::text("Clair de lune"))
        .unwrap();
    c.mutate("alice", "remove_media", args([("id", clair.field("id"))])).unwrap();
    assert_eq!(texts(&c.list("albums", args([])), "name"), ["Bagatelles", "Symphony No. 5"]);
    assert_eq!(texts(&c.list("artists", args([])), "name"), ["Beethoven"]);
}

/// An album comes back in the work's order, which is not the library's.
#[test]
fn an_album_is_in_the_works_order_and_not_the_librarys() {
    let mut c = Lib::new();
    let favs = c.playlist("alice", "Favorites");
    // Second suite before the first, and neither in track order.
    for (title, part, t) in [
        ("Gigue", "Suite No. 3 in G major", 21),
        ("Alla Hornpipe", "Suite No. 2 in D major", 12),
        ("Bourree", "Suite No. 3 in G major", 19),
        ("Overture", "Suite No. 2 in D major", 11),
    ] {
        c.add(Song {
            track: t,
            part,
            ..Song::new(title, "Handel", "Water Music")
        });
    }
    // A movement nobody numbered: after the numbered ones in its part.
    c.add(Song {
        part: "Suite No. 2 in D major",
        ..Song::new("Air", "Handel", "Water Music")
    });

    let water = c.list("album", args([("playlist_id", Value::Id(favs)), ("name", Value::text("Water Music"))]));
    assert_eq!(
        texts(&water, "title"),
        ["Overture", "Alla Hornpipe", "Air", "Bourree", "Gigue"],
        "part first, then track, and an untagged track last in its part"
    );
    assert_eq!(
        texts(&c.library(favs), "title"),
        ["Gigue", "Alla Hornpipe", "Bourree", "Overture", "Air"],
        "the library is still the order the songs arrived in"
    );
}

#[test]
fn a_track_knows_which_playlists_it_is_on() {
    let mut c = Lib::new();
    let favs = c.playlist("alice", "Favorites");
    let evening = c.playlist("alice", "Evening");
    for t in ["Air", "Gigue"] {
        c.add(Song::new(t, "Handel", ""));
    }
    let all = c.library(favs);
    let (air, gigue) = (all[0].field("id"), all[1].field("id"));
    let names = |c: &Lib, id: &Value| texts(&c.list("playlists_of", args([("media_id", id.clone())])), "name");
    assert!(names(&c, &air).is_empty(), "on nothing to begin with");

    let on = |p, m: &Value| args([("playlist_id", Value::Id(p)), ("media_id", m.clone())]);
    c.mutate("alice", "add_to_playlist", on(evening, &air)).unwrap();
    c.mutate("alice", "add_to_playlist", on(favs, &air)).unwrap();
    assert_eq!(names(&c, &air), ["Favorites", "Evening"], "in the order the playlists were made");
    assert!(names(&c, &gigue).is_empty(), "and only this track's");
    c.mutate("alice", "remove_from_playlist", on(favs, &air)).unwrap();
    assert_eq!(names(&c, &air), ["Evening"], "taking it off shows");

    // Removing the track from the library takes it off every playlist.
    c.mutate("alice", "remove_media", args([("id", air.clone())])).unwrap();
    assert!(names(&c, &air).is_empty());
    assert!(c.list("playlist", args([("playlist_id", Value::Id(evening))])).is_empty());
    assert_eq!(texts(&c.library(favs), "title"), ["Gigue"]);
    // …and something no longer in the library is a no-op to add.
    assert_eq!(c.mutate("alice", "add_to_playlist", on(evening, &air)), Ok(0));
}

/// A rescan is a no-op, decided in `apply` rather than by the scanner.
#[test]
fn the_same_file_twice_is_one_song() {
    let mut c = Lib::new();
    let favs = c.playlist("library", "Favorites");
    let add = |c: &mut Lib, title: &'static str, file: &str| c.add(with(file, Song::new(title, "Beethoven", "Bagatelles")));
    add(&mut c, "Für Elise", "beethoven/fur-elise.mp3");
    add(&mut c, "Für Elise", "beethoven/fur-elise.mp3");
    add(&mut c, "Fur Elise (again)", " beethoven/fur-elise.mp3 ");
    assert_eq!(c.library(favs).len(), 1, "one file is one song, however many times it is offered");
    add(&mut c, "Für Elise", "beethoven/fur-elise-live.mp3");
    assert_eq!(c.library(favs).len(), 2);
    add(&mut c, "Untitled", "");
    add(&mut c, "Untitled", "");
    assert_eq!(c.library(favs).len(), 4, "no file means no collision");
    assert_eq!(
        c.mutate("alice", "add_song", Song::new("  ", "X", "").args()),
        Err("a song needs a title".into())
    );
}

/// `art_to_write`'s three cases: no row makes one; a picture replaces a
/// picture; nothing replaces nothing (the rescan case).
#[test]
fn artwork_reaches_the_lists_it_is_drawn_on() {
    let mut c = Lib::new();
    let handel = |file: &str, album_art: &'static str, artist_art: &'static str| Song {
        file: file.into(),
        track: 1,
        album_art,
        artist_art,
        ..Song::new("Alla Hornpipe", "George Frideric Handel", "Water Music")
    };
    c.add(handel("music/a.mp3", "https://example.com/thames.jpg", "https://example.com/denner.jpg"));
    let albums = c.list("albums", args([]));
    assert_eq!(texts(&albums, "art"), ["https://example.com/thames.jpg"]);
    assert_eq!(texts(&c.list("artists", args([])), "art"), ["https://example.com/denner.jpg"]);

    c.add(handel("music/b.mp3", "", ""));
    assert_eq!(
        texts(&c.list("albums", args([])), "art"),
        ["https://example.com/thames.jpg"],
        "an entry with no picture must not clear one"
    );
    assert_eq!(texts(&c.list("artists", args([])), "art"), ["https://example.com/denner.jpg"]);

    // The same picture again writes nothing at all: no album or person row
    // moves (media, song and the rest do).
    let before = (ark::store::Store::scan(&c.store, "album"), ark::store::Store::scan(&c.store, "person"));
    c.add(handel("music/b2.mp3", "https://example.com/thames.jpg", "https://example.com/denner.jpg"));
    assert_eq!(
        (ark::store::Store::scan(&c.store, "album"), ark::store::Store::scan(&c.store, "person")),
        before,
        "the same picture is no write"
    );

    c.add(handel("music/c.mp3", "https://example.com/better.jpg", ""));
    assert_eq!(texts(&c.list("albums", args([])), "art"), ["https://example.com/better.jpg"]);

    c.add(Song {
        file: "music/d.mp3".into(),
        track: 3,
        ..Song::new("Clair de lune", "Claude Debussy", "Suite bergamasque")
    });
    let albums = c.list("albums", args([]));
    assert_eq!(texts(&albums, "name"), ["Suite bergamasque", "Water Music"]);
    assert_eq!(
        texts(&albums, "art"),
        ["", "https://example.com/better.jpg"],
        "a record with no cover is still a record"
    );
}

/// One work, two performances of it: `works` says 1, `recordings` says 2.
#[test]
fn one_work_holds_every_recording_of_it() {
    let mut c = Lib::new();
    let favs = c.playlist("alice", "Favorites");
    for (performer, file) in [("Kimiko Ishizaka", "a"), ("Glenn Gould", "b")] {
        for no in 1..=3 {
            let title: &'static str = ["Variatio 1", "Variatio 2", "Variatio 3"][no as usize - 1];
            c.add(track(
                title,
                "Johann Sebastian Bach",
                "Goldberg Variations",
                "BWV 988",
                performer,
                &format!("music/{file}{no}.mp3"),
                no,
                "Goldberg Variations",
                no,
            ));
        }
    }
    let composers = c.list("composers", args([]));
    assert_eq!(texts(&composers, "name"), ["Johann Sebastian Bach"]);
    assert_eq!(ints(&composers, "works"), [1], "one work, however many recordings");
    assert_eq!(ints(&composers, "tracks"), [6]);
    assert_eq!(
        texts(&composers, "sort_name"),
        ["Johann Sebastian Bach"],
        "the name, when nobody said how to sort it"
    );

    let works = c.list("works", args([("composer", Value::text("Johann Sebastian Bach"))]));
    assert_eq!(works.len(), 1, "two performances are not two works");
    assert_eq!(works[0].field("catalogue"), Value::text("BWV 988"));
    assert_eq!(
        works[0].field("id"),
        Value::text(harken_domain::keys::work_key("Johann Sebastian Bach", "BWV 988", ""))
    );
    assert_eq!(ints(&works, "recordings"), [2]);
    assert_eq!(ints(&works, "tracks"), [6]);
    assert_eq!(
        c.list("work", args([("id", works[0].field("id"))])),
        works,
        "one work by its key is the same summary"
    );
    assert!(c.list("work", args([("id", Value::text("nobody/nothing"))])).is_empty());

    let takes = c.list("recordings", args([("work_id", works[0].field("id"))]));
    assert_eq!(takes.len(), 2, "…and they are two recordings");
    let mut who = texts(&takes, "performers");
    who.sort();
    assert_eq!(who, ["Glenn Gould", "Kimiko Ishizaka"]);
    let one = c.list("recording", args([("playlist_id", Value::Id(favs)), ("id", takes[0].field("id"))]));
    assert_eq!(texts(&one, "title"), ["Variatio 1", "Variatio 2", "Variatio 3"]);
}

/// A pop track is a song with no work, and it still has everywhere to hang.
#[test]
fn a_pop_track_has_a_recording_and_no_work() {
    let mut c = Lib::new();
    let favs = c.playlist("alice", "Favorites");
    c.add(track(
        "Low Tide",
        "The Quiet Hours",
        "Northerly",
        "",
        "",
        "music/northerly/07.flac",
        7,
        "",
        0,
    ));
    assert!(
        c.list("composers", args([])).is_empty(),
        "nobody wrote a work, so there is no composers page"
    );
    assert_eq!(texts(&c.list("artists", args([])), "name"), ["The Quiet Hours"]);
    assert_eq!(c.list("albums", args([])).len(), 1);
    assert_eq!(
        c.list("album", args([("playlist_id", Value::Id(favs)), ("name", Value::text("Northerly"))]))
            .len(),
        1
    );

    let details = c.list("track_details", args([]));
    assert_eq!(
        details[0].field("performer"),
        Value::text("The Quiet Hours"),
        "the artist is a credit on a recording"
    );
    assert_eq!(details[0].field("catalogue"), Value::text(""), "no work, so no catalogue number");
    assert_eq!(details[0].field("album"), Value::text("Northerly"));
    assert_eq!(details[0].field("track"), Value::int(7));
    let rec = ark::store::Store::scan(&c.store, "recording");
    assert_eq!(rec.len(), 1);
    assert_eq!(rec[0]["work_id"], Value::Null, "a recording of no work");
}

/// A track number is the release's and a movement number is the work's.
#[test]
fn a_track_number_is_not_a_movement_number() {
    let mut c = Lib::new();
    let favs = c.playlist("alice", "Favorites");
    for (title, movement, t) in [("III. Presto", 3, 1), ("I. Adagio", 1, 9)] {
        c.add(track(
            title,
            "Ludwig van Beethoven",
            "Piano Favourites",
            "Op. 27 No. 2",
            "Wilhelm Kempff",
            &format!("music/{movement}.mp3"),
            t,
            "Moonlight Sonata",
            movement,
        ));
    }
    let record = c.list(
        "album",
        args([("playlist_id", Value::Id(favs)), ("name", Value::text("Piano Favourites"))]),
    );
    assert_eq!(
        texts(&record, "title"),
        ["III. Presto", "I. Adagio"],
        "an album page is in the release's order"
    );
    let works = c.list("works", args([("composer", Value::text("Ludwig van Beethoven"))]));
    let takes = c.list("recordings", args([("work_id", works[0].field("id"))]));
    let in_the_work = c.list("recording", args([("playlist_id", Value::Id(favs)), ("id", takes[0].field("id"))]));
    assert_eq!(
        texts(&in_the_work, "title"),
        ["I. Adagio", "III. Presto"],
        "a work page is in the work's order"
    );
}

/// The two readings that let an entry that names no work still say it has
/// one: a catalogue number, and a part.
#[test]
fn an_entry_without_a_work_title_still_names_one() {
    let mut c = Lib::new();
    c.add(Song {
        file: "music/a.mp3".into(),
        track: 13,
        catalogue: "BWV 988",
        performer: "Kimiko Ishizaka",
        ..Song::new("Variatio 12", "Johann Sebastian Bach", "Goldberg Variations")
    });
    let works = c.list("works", args([("composer", Value::text("Johann Sebastian Bach"))]));
    assert_eq!(
        texts(&works, "title"),
        ["Goldberg Variations"],
        "a catalogue number means there is a work"
    );
    assert_eq!(texts(&c.list("track_details", args([])), "catalogue"), ["BWV 988"]);
    // The movement number is the track number when the entry did not say.
    let movements = ark::store::Store::scan(&c.store, "movement");
    assert_eq!(movements[0]["no"], Value::int(13));

    let mut c = Lib::new();
    c.add(Song {
        file: "music/b.mp3".into(),
        track: 12,
        part: "Suite No. 2 in D major",
        ..Song::new("Alla Hornpipe", "George Frideric Handel", "Water Music")
    });
    assert_eq!(
        texts(&c.list("track_details", args([])), "part"),
        ["Suite No. 2 in D major"],
        "a part is evidence of a work, and it is what an album page groups by"
    );
    assert_eq!(texts(&c.list("composers", args([])), "name"), ["George Frideric Handel"]);
}

/// `credit_recording` turns one lumped string into people with roles.
#[test]
fn a_lumped_performer_becomes_people_with_roles() {
    let mut c = Lib::new();
    c.add(track(
        "Hallelujah",
        "George Frideric Handel",
        "Messiah",
        "HWV 56",
        "London Symphony Orchestra, Hermann Scherchen",
        "music/m.mp3",
        44,
        "Messiah",
        44,
    ));
    let works = c.list("works", args([("composer", Value::text("George Frideric Handel"))]));
    let id = c.list("recordings", args([("work_id", works[0].field("id"))]))[0].field("id");
    assert_eq!(
        texts(&c.list("track_details", args([])), "performer"),
        ["London Symphony Orchestra, Hermann Scherchen"]
    );
    let lumped = c.list("credits", args([("recording_id", id.clone())]));
    assert_eq!(texts(&lumped, "role"), ["performer"]);
    assert_eq!(ints(&lumped, "pos"), [0], "the fallback is at 0");

    let credit = |c: &mut Lib, name: &str, role: &str, pos: i64| {
        c.mutate(
            "alice",
            "credit_recording",
            args([
                ("recording_id", id.clone()),
                ("person_name", Value::text(name)),
                ("role", Value::text(role)),
                ("instrument", Value::text("")),
                ("pos", Value::int(pos)),
            ]),
        )
    };
    credit(&mut c, "Hermann Scherchen", "conductor", 2).unwrap();
    credit(&mut c, "London Symphony Orchestra", "orchestra", 1).unwrap();
    let takes = c.list("recordings", args([("work_id", works[0].field("id"))]));
    assert_eq!(
        texts(&takes, "performers"),
        ["London Symphony Orchestra, Hermann Scherchen"],
        "billing order and not alphabetical, and the lumped string displaced"
    );
    assert_eq!(
        texts(&c.list("track_details", args([])), "performer"),
        ["London Symphony Orchestra, Hermann Scherchen"],
        "the table column reads the same credits the work page does"
    );
    let credits = c.list("credits", args([("recording_id", id.clone())]));
    assert_eq!(texts(&credits, "name"), ["London Symphony Orchestra", "Hermann Scherchen"]);
    assert_eq!(texts(&credits, "role"), ["orchestra", "conductor"]);

    // A second entry for the same person in the same role is a correction;
    // a role left empty is "artist"; 0 is never a real credit's place.
    credit(&mut c, "Hermann Scherchen", "conductor", 0).unwrap();
    credit(&mut c, "Somebody Else", "", 0).unwrap();
    let credits = c.list("credits", args([("recording_id", id.clone())]));
    assert_eq!(
        credits
            .iter()
            .map(|r| (
                r.field("name").as_text().to_string(),
                r.field("role").as_text().to_string(),
                r.field("pos").as_int()
            ))
            .collect::<Vec<_>>(),
        [
            ("London Symphony Orchestra".into(), "orchestra".into(), 1),
            ("Somebody Else".into(), "artist".into(), 1),
            ("Hermann Scherchen".into(), "conductor".into(), 2),
        ]
    );
    assert_eq!(
        c.mutate(
            "alice",
            "credit_recording",
            args([
                ("recording_id", Value::text("nobody/nothing@x")),
                ("person_name", Value::text("Somebody")),
                ("role", Value::text("conductor")),
                ("instrument", Value::text("")),
                ("pos", Value::int(1)),
            ]),
        ),
        Err("no recording nobody/nothing@x to credit anybody on".into()),
        "a credit needs a recording to be on"
    );
}

/// `describe_work` fills in what a track could not carry, never erases,
/// and refuses a work no song has named; the other two describe verbs follow.
#[test]
fn describing_fills_in_and_never_erases() {
    let mut c = Lib::new();
    c.add(track(
        "I. Allegro con brio",
        "Ludwig van Beethoven",
        "Symphony No. 5",
        "Op. 67",
        "Carlos Kleiber",
        "music/5.mp3",
        1,
        "Symphony No. 5",
        1,
    ));
    let id = c.list("works", args([("composer", Value::text("Ludwig van Beethoven"))]))[0].field("id");
    let describe = |c: &mut Lib, id: &Value, key: &str, form: &str, period: &str, composed: i64| {
        c.mutate(
            "alice",
            "describe_work",
            args([
                ("id", id.clone()),
                ("opus", Value::text("")),
                ("key_sig", Value::text(key)),
                ("form", Value::text(form)),
                ("period", Value::text(period)),
                ("composed", Value::int(composed)),
                ("art", Value::text("")),
            ]),
        )
    };
    describe(&mut c, &id, "C Minor", "Symphony", "", 1808).unwrap();
    describe(&mut c, &id, "", "", "Classical", 0).unwrap();
    let work = &c.list("works", args([("composer", Value::text("Ludwig van Beethoven"))]))[0];
    assert_eq!(work.field("period"), Value::text("Classical"), "the second pass said this");
    assert_eq!(work.field("form"), Value::text("Symphony"), "…and must not have erased this");
    let row = ark::store::Store::scan(&c.store, "work").remove(0);
    assert_eq!(
        (row["key_sig"].clone(), row["composed"].clone()),
        (Value::text("C Minor"), Value::int(1808))
    );
    assert_eq!(
        describe(&mut c, &Value::text("nobody/nothing"), "", "", "", 0),
        Err("no work nobody/nothing; a work exists because a song named it".into())
    );

    // A recording, the same way.
    let rid = c.list("recordings", args([("work_id", id.clone())]))[0].field("id");
    let recorded = |c: &mut Lib, id: &Value, year: i64, licence: &str| {
        c.mutate(
            "alice",
            "describe_recording",
            args([
                ("id", id.clone()),
                ("recorded", Value::int(year)),
                ("venue", Value::text("")),
                ("label", Value::text("DG")),
                ("licence", Value::text(licence)),
                ("art", Value::text("")),
            ]),
        )
    };
    recorded(&mut c, &rid, 1975, "CC BY 4.0").unwrap();
    recorded(&mut c, &rid, 0, "").unwrap();
    let take = &c.list("recordings", args([("work_id", id.clone())]))[0];
    assert_eq!(
        (take.field("recorded"), take.field("licence")),
        (Value::int(1975), Value::text("CC BY 4.0"))
    );
    assert_eq!(
        texts(&c.list("track_details", args([])), "licence"),
        ["CC BY 4.0"],
        "the licence reaches the table"
    );
    assert_eq!(
        recorded(&mut c, &Value::text("x@y"), 1, ""),
        Err("no recording x@y; one exists because a song is part of it".into())
    );

    // A person may be described before any of their music arrives.
    let person = |c: &mut Lib, name: &str, sort: &str, born: i64| {
        c.mutate(
            "alice",
            "describe_person",
            args([
                ("name", Value::text(name)),
                ("sort_name", Value::text(sort)),
                ("born", Value::int(born)),
                ("died", Value::int(0)),
                ("art", Value::text("")),
            ]),
        )
    };
    person(&mut c, "Ludwig van Beethoven", "Beethoven, Ludwig van", 1770).unwrap();
    person(&mut c, "Ludwig van Beethoven", "", 0).unwrap();
    let composers = c.list("composers", args([]));
    assert_eq!(texts(&composers, "sort_name"), ["Beethoven, Ludwig van"]);
    assert_eq!(ints(&composers, "born"), [1770]);
    person(&mut c, "Clara Schumann", "", 1819).unwrap();
    assert_eq!(
        texts(&c.list("composers", args([])), "name"),
        ["Ludwig van Beethoven"],
        "a person is not a composer until a work is theirs"
    );
    assert_eq!(person(&mut c, "  ", "", 0), Err("a person needs a name".into()));
}

/// The fields only a song has come through `add_song`'s bounds.
#[test]
fn a_song_keeps_its_own_facts_in_bounds() {
    let mut c = Lib::new();
    c.add(Song {
        file: "a".into(),
        track: 1200,
        bpm: 400,
        disc: 0,
        ..Song::new("Too Much", "X", "Y")
    });
    c.add(Song {
        file: "b".into(),
        track: -3,
        bpm: 120,
        disc: 250,
        ..Song::new("Fine", "X", "")
    });
    let songs = ark::store::Store::scan(&c.store, "song");
    let mut got: Vec<(Value, i64, i64, i64)> = songs
        .iter()
        .map(|s| (s["album_name"].clone(), s["track"].as_int(), s["bpm"].as_int(), s["disc"].as_int()))
        .collect();
    got.sort_by_key(|g| g.1);
    assert_eq!(got, [(Value::Null, 0, 120, 99), (Value::text("Y"), 999, 0, 1)]);
}

/// Everything in the library onto a playlist in one entry, in library
/// order, skipping what is already there.
#[test]
fn the_whole_library_onto_a_playlist() {
    let mut c = Lib::new();
    let favs = c.playlist("alice", "Favorites");
    for (t, f) in [("A", "a"), ("B", "b"), ("C", "c")] {
        c.add(with(f, Song::new(t, "X", "")));
    }
    let b = c.library(favs)[1].field("id");
    c.mutate("alice", "add_to_playlist", args([("playlist_id", Value::Id(favs)), ("media_id", b)]))
        .unwrap();
    c.mutate("alice", "add_all_to_playlist", args([("playlist_id", Value::Id(favs))]))
        .unwrap();
    let on = c.list("playlist", args([("playlist_id", Value::Id(favs))]));
    assert_eq!(texts(&on, "title"), ["B", "A", "C"]);
    assert_eq!(ints(&on, "playlist_pos"), [1, 2, 3]);
    assert_eq!(c.mutate("alice", "add_all_to_playlist", args([("playlist_id", Value::Id(favs))])), Ok(0));
}
