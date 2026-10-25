//! The read model, against rows the domain's own functions wrote:
//! `harken/domain/tests/read_model.rs`, assertion for assertion. Each test
//! was falsified once by breaking the function it names; the doc comment
//! says how.

mod common;

use common::{each, song, track, with, Lib};
use harken_domain::Id;
use harken_domain::*;

fn titles(rows: &[LibraryEntry]) -> Vec<String> {
    each(rows, |r| r.title.clone())
}

fn on(p: &Id<Playlist>, m: &Id<Media>) -> AddToPlaylist {
    AddToPlaylist {
        playlist_id: *p,
        media_id: *m,
    }
}

fn names(c: &Lib, who: &str) -> Vec<String> {
    each(&c.query::<Vec<Playlist>>(who, "playlists", &()).unwrap(), |p| p.name.clone())
}

/// Falsified by `playlist_pos` reading the item's `added_ms`: the
/// positions came back as clock readings.
#[test]
fn library_and_a_playlist_agree_with_what_apply_wrote() {
    let mut c = Lib::new();
    let favs = c.playlist("alice", "Favorites");
    for (t, a) in [("Glue", "Bicep"), ("Opal", "Bicep"), ("Gosh", "Jamie xx")] {
        c.add(song(t, a, ""));
    }
    let all = c.library(&favs);
    assert_eq!(
        titles(&all),
        ["Glue", "Opal", "Gosh"],
        "library order is pos, and pos counts up from MAX(pos)"
    );
    assert_eq!(all[0].creator, "Bicep");
    assert_eq!(all[0].user_id, "alice");
    assert_eq!(all[0].kind, "song");
    assert!(all.iter().all(|s| s.playlist_pos.is_none()), "nothing on the playlist yet");
    let on_it = |c: &Lib| c.list::<LibraryEntry>("playlist", &PlaylistInput { playlist_id: favs });
    assert!(on_it(&c).is_empty());

    // The third, then the first: the playlist is in the order they joined it.
    let (third, first) = (all[2].id, all[0].id);
    c.mutate("alice", "add_to_playlist", &on(&favs, &third)).unwrap();
    c.mutate("alice", "add_to_playlist", &on(&favs, &first)).unwrap();
    let playlist = on_it(&c);
    assert_eq!(titles(&playlist), ["Gosh", "Glue"], "playlist order is the item's pos");
    assert_eq!(each(&playlist, |r| r.playlist_pos), [Some(1), Some(2)]);

    // The library keeps its own order and carries the position on the rows
    // that have one.
    let all = c.library(&favs);
    assert_eq!(each(&all, |r| r.playlist_pos), [Some(2), None, Some(1)]);
    assert_eq!(all[0].id, playlist[1].id);

    // Taking it off leaves the song; adding it twice keeps its first place.
    c.mutate("alice", "add_to_playlist", &on(&favs, &third)).unwrap();
    assert_eq!(each(&on_it(&c), |r| r.playlist_pos), [Some(1), Some(2)], "a playlist holds an item once");
    c.mutate(
        "alice",
        "remove_from_playlist",
        &RemoveFromPlaylist {
            playlist_id: favs,
            media_id: first,
        },
    )
    .unwrap();
    assert_eq!(c.library(&favs).len(), 3);
    assert_eq!(on_it(&c).len(), 1);
    // Somebody else's playlist is not theirs to change.
    assert_eq!(c.mutate("bob", "add_to_playlist", &on(&favs, &first)), Err("not your playlist".into()));
    assert_eq!(c.mutate("", "add_to_playlist", &on(&favs, &first)), Err("not your playlist".into()));
}

/// A name a person already has is numbered with the smallest free n.
/// Falsified by `free_number` answering `len + 1` always: "Mix" came
/// back "Mix (4)" where "(1)" was free.
#[test]
fn a_second_playlist_of_one_name_is_numbered() {
    let mut c = Lib::new();
    let make = |c: &mut Lib, who: &str, name: &str| c.mutate(who, "create_playlist", &CreatePlaylist { name: name.into() });
    make(&mut c, "alice", "Favorites").unwrap();
    assert_eq!(make(&mut c, "alice", "Favorites"), Ok(1), "a second playlist, not nothing");
    make(&mut c, "alice", "  Favorites  ").unwrap();
    assert_eq!(names(&c, "alice"), ["Favorites", "Favorites (1)", "Favorites (2)"]);

    make(&mut c, "alice", "Mix (2)").unwrap();
    make(&mut c, "alice", "Mix").unwrap();
    make(&mut c, "alice", "Mix").unwrap();
    make(&mut c, "alice", "Mix").unwrap();
    assert_eq!(
        names(&c, "alice")[3..],
        ["Mix (2)", "Mix", "Mix (1)", "Mix (3)"],
        "(1) was free, then (2) was taken"
    );
    make(&mut c, "alice", "favorites").unwrap();
    assert_eq!(names(&c, "alice").last().map(String::as_str), Some("favorites"), "case is not folded");

    // The same entry twice is one playlist, not "(3)".
    let ctx = c.ctx("alice", &[]);
    let gym = CreatePlaylist { name: "Gym".into() };
    assert_eq!(c.mutate_as(&ctx, "create_playlist", &gym), Ok(1));
    assert_eq!(c.mutate_as(&ctx, "create_playlist", &gym), Ok(0));
    assert_eq!(names(&c, "alice").iter().filter(|n| n.starts_with("Gym")).count(), 1);

    make(&mut c, "bob", "Favorites").unwrap();
    assert_eq!(names(&c, "bob"), ["Favorites"]);
    make(&mut c, "", "Favorites").unwrap();
    make(&mut c, "", "Favorites").unwrap();
    assert_eq!(names(&c, ""), ["Favorites", "Favorites (1)"]);
    assert_eq!(make(&mut c, "alice", "   "), Err("a playlist needs a name".into()));
}

/// Bob's "Favorites" is not Alice's. Falsified by `playlists` dropping its
/// filter: each saw both.
#[test]
fn two_people_each_get_a_playlist_of_one_name() {
    let mut c = Lib::new();
    let a = c.playlist("alice", "Favorites");
    let b = c.playlist("bob", "Favorites");
    assert_ne!(a, b);
    for who in ["alice", "bob"] {
        let mine: Vec<Playlist> = c.query(who, "playlists", &()).unwrap();
        assert_eq!(each(&mine, |p| p.user_id.clone()), [who]);
        assert_eq!(each(&mine, |p| p.name.clone()), ["Favorites"]);
    }
    assert_eq!(c.rows::<Playlist>("playlist").len(), 2);
}

/// Falsified by `albums` keeping an album with no songs: "Suite
/// bergamasque" stayed after its last track went.
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
        c.add(song(title, artist, album));
    }
    let albums: Vec<AlbumsEntry> = c.list("albums", &());
    let got = each(&albums, |a| (a.name.clone(), a.creator.clone(), a.tracks));
    assert_eq!(
        got,
        [
            ("Bagatelles".into(), "Beethoven".into(), 1),
            ("Suite bergamasque".into(), "Debussy".into(), 1),
            ("Symphony No. 5".into(), "Beethoven".into(), 2),
        ],
        "one row per album, in name order, with the tracks counted"
    );
    let artists: Vec<ArtistsEntry> = c.list("artists", &());
    assert_eq!(each(&artists, |a| a.name.clone()), ["Beethoven", "Debussy"]);
    assert_eq!(each(&artists, |a| a.tracks), [3, 1]);

    let album = |c: &Lib, name: &str| {
        c.list::<LibraryEntry>(
            "album",
            &AlbumInput {
                playlist_id: favs,
                name: name.into(),
            },
        )
    };
    let symphony = album(&c, "Symphony No. 5");
    assert_eq!(titles(&symphony), ["Symphony No. 5 - I", "Symphony No. 5 - III"]);
    let beethoven: Vec<LibraryEntry> = c.list(
        "artist",
        &Artist {
            playlist_id: favs,
            name: "Beethoven".into(),
        },
    );
    assert_eq!(beethoven.len(), 3);
    assert!(album(&c, "Nocturnes").is_empty());

    c.mutate("alice", "add_to_playlist", &on(&favs, &symphony[0].id)).unwrap();
    let symphony = album(&c, "Symphony No. 5");
    assert_eq!(symphony[0].playlist_pos, Some(1), "the membership survives the album view");
    assert!(symphony[1].playlist_pos.is_none());

    let clair = c.library(&favs).into_iter().find(|r| r.title == "Clair de lune").unwrap();
    c.mutate("alice", "remove_media", &RemoveMedia { id: clair.id }).unwrap();
    assert_eq!(
        each(&c.list::<AlbumsEntry>("albums", &()), |a| a.name.clone()),
        ["Bagatelles", "Symphony No. 5"]
    );
    assert_eq!(each(&c.list::<ArtistsEntry>("artists", &()), |a| a.name.clone()), ["Beethoven"]);
}

/// An album comes back in the work's order. Falsified by dropping `s.track
/// == 0` from `album`'s keys: "Air", untagged, came first in its part.
#[test]
fn an_album_is_in_the_works_order_and_not_the_librarys() {
    let mut c = Lib::new();
    let favs = c.playlist("alice", "Favorites");
    for (title, part, t) in [
        ("Gigue", "Suite No. 3 in G major", 21),
        ("Alla Hornpipe", "Suite No. 2 in D major", 12),
        ("Bourree", "Suite No. 3 in G major", 19),
        ("Overture", "Suite No. 2 in D major", 11),
    ] {
        c.add(AddSong {
            track: t,
            part: part.into(),
            ..song(title, "Handel", "Water Music")
        });
    }
    c.add(AddSong {
        part: "Suite No. 2 in D major".into(),
        ..song("Air", "Handel", "Water Music")
    });
    let water: Vec<LibraryEntry> = c.list(
        "album",
        &AlbumInput {
            playlist_id: favs,
            name: "Water Music".into(),
        },
    );
    assert_eq!(
        titles(&water),
        ["Overture", "Alla Hornpipe", "Air", "Bourree", "Gigue"],
        "part first, then track, and an untagged track last in its part"
    );
    assert_eq!(titles(&c.library(&favs)), ["Gigue", "Alla Hornpipe", "Bourree", "Overture", "Air"]);
}

/// Falsified by `playlists_of` not looking the item up at all: every
/// track was on every playlist.
#[test]
fn a_track_knows_which_playlists_it_is_on() {
    let mut c = Lib::new();
    let favs = c.playlist("alice", "Favorites");
    let evening = c.playlist("alice", "Evening");
    for t in ["Air", "Gigue"] {
        c.add(song(t, "Handel", ""));
    }
    let all = c.library(&favs);
    let (air, gigue) = (all[0].id, all[1].id);
    let names = |c: &Lib, id: &Id<Media>| each(&c.list::<Playlist>("playlists_of", &PlaylistsOf { media_id: *id }), |p| p.name.clone());
    assert!(names(&c, &air).is_empty(), "on nothing to begin with");
    c.mutate("alice", "add_to_playlist", &on(&evening, &air)).unwrap();
    c.mutate("alice", "add_to_playlist", &on(&favs, &air)).unwrap();
    assert_eq!(names(&c, &air), ["Favorites", "Evening"], "in the order the playlists were made");
    assert!(names(&c, &gigue).is_empty(), "and only this track's");
    c.mutate(
        "alice",
        "remove_from_playlist",
        &RemoveFromPlaylist {
            playlist_id: favs,
            media_id: air,
        },
    )
    .unwrap();
    assert_eq!(names(&c, &air), ["Evening"], "taking it off shows");

    c.mutate("alice", "remove_media", &RemoveMedia { id: air }).unwrap();
    assert!(names(&c, &air).is_empty());
    assert!(c.list::<LibraryEntry>("playlist", &PlaylistInput { playlist_id: evening }).is_empty());
    assert_eq!(titles(&c.library(&favs)), ["Gigue"]);
    assert_eq!(
        c.mutate("alice", "add_to_playlist", &on(&evening, &air)),
        Ok(0),
        "no longer in the library is a no-op"
    );
}

/// `add_song` is an upsert by `file`. Falsified by matching on the file
/// untrimmed: " beethoven/fur-elise.mp3 " was a second song.
#[test]
fn the_same_file_twice_is_one_song() {
    let mut c = Lib::new();
    let favs = c.playlist("library", "Favorites");
    let elise = |title: &str, file: &str| with(file, song(title, "Beethoven", "Bagatelles"));
    c.add(elise("Für Elise", "beethoven/fur-elise.mp3"));
    assert_eq!(
        c.mutate("alice", "add_song", &elise("Für Elise", "beethoven/fur-elise.mp3")),
        Ok(0),
        "the same entry again writes no row"
    );
    c.add(elise("Fur Elise (again)", " beethoven/fur-elise.mp3 "));
    assert_eq!(c.library(&favs).len(), 1, "one file is one song, however many times it is offered");
    c.add(elise("Für Elise", "beethoven/fur-elise-live.mp3"));
    assert_eq!(c.library(&favs).len(), 2);
    for n in 3..=5 {
        assert!(c.mutate("alice", "add_song", &song("Untitled", "", "")).unwrap() > 0);
        assert_eq!(c.library(&favs).len(), n, "no file means no collision");
    }
    assert_eq!(c.mutate("alice", "add_song", &song("  ", "X", "")), Err("a song needs a title".into()));
}

/// A retagged file is the song it was. Falsified by minting the id afresh
/// for a known file: the playlist went on naming the old row.
#[test]
fn a_retagged_file_is_the_same_song_saying_the_new_tags() {
    let mut c = Lib::new();
    let favs = c.playlist("alice", "Favorites");
    c.add(with("music/a.flac", song("Glue", "Bicep", "Isles")));
    c.add(with("music/b.flac", song("Opal", "Bicep", "Isles")));
    let before = c.library(&favs);
    let glue = before[0].id;
    c.mutate("alice", "add_to_playlist", &on(&favs, &glue)).unwrap();
    c.add(AddSong {
        duration_ms: 242_000,
        track: 3,
        ..with("music/a.flac", song("Glue (Remastered)", "Bicep", "Isles"))
    });
    let after = c.library(&favs);
    assert_eq!(titles(&after), ["Glue (Remastered)", "Opal"], "in its place, retitled");
    assert_eq!(after[0].id, glue, "the same song");
    assert_eq!(after[0].added_ms, before[0].added_ms, "added when it was added");
    assert_eq!(after[0].duration_ms, 242_000);
    let playlist: Vec<LibraryEntry> = c.list("playlist", &PlaylistInput { playlist_id: favs });
    assert_eq!(titles(&playlist), ["Glue (Remastered)"], "the playlist holds it still");
    let details: Vec<TrackDetailsEntry> = c.list("track_details", &());
    assert_eq!(details.iter().find(|d| d.media_id == glue).map(|d| d.track), Some(3));
    assert_eq!(
        c.rows::<Recording>("recording").len(),
        2,
        "the old title's recording went with nothing left on it"
    );
}

/// What a moved file was part of goes when nothing else is. Falsified by
/// never deleting a work: four were left where three are.
#[test]
fn a_file_moved_to_another_album_takes_nothing_with_it_that_is_still_used() {
    let mut c = Lib::new();
    let favs = c.playlist("alice", "Favorites");
    let bach = |file: &str, album: &str, catalogue: &str, title: &str, no: i64| {
        track(title, "Johann Sebastian Bach", album, catalogue, "Kimiko Ishizaka", file, no, "", no)
    };
    c.add(bach("music/g1.flac", "Goldberg Variations", "BWV 988", "Aria", 1));
    c.add(bach("music/g2.flac", "Goldberg Variations", "BWV 988", "Variatio 1", 2));
    c.add(bach("music/i1.flac", "Inventions", "BWV 772", "Invention 1", 1));
    let tables = |c: &Lib| ["album", "work", "movement", "recording", "credit"].map(|t| c.store.rows(t).count());
    assert_eq!(tables(&c), [2, 2, 3, 2, 2]);

    c.add(bach("music/g2.flac", "Inventions", "BWV 773", "Invention 2", 2));
    assert_eq!(tables(&c), [2, 3, 3, 3, 3]);
    let album = |c: &Lib, name: &str| {
        titles(&c.list(
            "album",
            &AlbumInput {
                playlist_id: favs,
                name: name.into(),
            },
        ))
    };
    assert_eq!(album(&c, "Goldberg Variations"), ["Aria"]);
    assert_eq!(album(&c, "Inventions"), ["Invention 1", "Invention 2"]);

    c.add(bach("music/g1.flac", "Inventions", "BWV 774", "Invention 3", 3));
    assert_eq!(tables(&c), [2, 3, 3, 3, 3]);
    let works: Vec<WorkSummary> = c.list(
        "works",
        &Works {
            composer: "Johann Sebastian Bach".into(),
        },
    );
    assert_eq!(each(&works, |w| w.catalogue.clone()), ["BWV 772", "BWV 773", "BWV 774"]);
    assert!(works.iter().all(|w| w.tracks == 1));
    assert_eq!(each(&c.list::<AlbumsEntry>("albums", &()), |a| a.name.clone()), ["Inventions"]);
    let composers: Vec<ComposersEntry> = c.list("composers", &());
    assert_eq!(each(&composers, |p| p.works), [3]);
    assert_eq!(c.library(&favs).len(), 3, "three files, three songs");
}

/// No row makes one; a picture replaces a picture; nothing replaces
/// nothing. Falsified by writing the album whenever the entry names one:
/// the same picture again moved the album row.
#[test]
fn artwork_reaches_the_lists_it_is_drawn_on() {
    let mut c = Lib::new();
    let handel = |file: &str, album_art: &str, artist_art: &str| AddSong {
        file: file.into(),
        track: 1,
        album_art: album_art.into(),
        artist_art: artist_art.into(),
        ..song("Alla Hornpipe", "George Frideric Handel", "Water Music")
    };
    let arts = |c: &Lib| {
        (
            each(&c.list::<AlbumsEntry>("albums", &()), |a| a.art.clone()),
            each(&c.list::<ArtistsEntry>("artists", &()), |a| a.art.clone()),
        )
    };
    c.add(handel("music/a.mp3", "https://example.com/thames.jpg", "https://example.com/denner.jpg"));
    assert_eq!(
        arts(&c),
        (
            vec!["https://example.com/thames.jpg".to_string()],
            vec!["https://example.com/denner.jpg".to_string()]
        )
    );
    c.add(handel("music/b.mp3", "", ""));
    assert_eq!(
        arts(&c),
        (
            vec!["https://example.com/thames.jpg".to_string()],
            vec!["https://example.com/denner.jpg".to_string()]
        ),
        "no picture must not clear one"
    );

    let before = (c.rows::<Album>("album"), c.rows::<Person>("person"));
    c.add(handel("music/b2.mp3", "https://example.com/thames.jpg", "https://example.com/denner.jpg"));
    assert_eq!(
        (c.rows::<Album>("album"), c.rows::<Person>("person")),
        before,
        "the same picture is no write"
    );

    c.add(handel("music/c.mp3", "https://example.com/better.jpg", ""));
    assert_eq!(arts(&c).0, ["https://example.com/better.jpg"]);
    c.add(AddSong {
        file: "music/d.mp3".into(),
        track: 3,
        ..song("Clair de lune", "Claude Debussy", "Suite bergamasque")
    });
    let albums: Vec<AlbumsEntry> = c.list("albums", &());
    assert_eq!(each(&albums, |a| a.name.clone()), ["Suite bergamasque", "Water Music"]);
    assert_eq!(
        each(&albums, |a| a.art.clone()),
        ["", "https://example.com/better.jpg"],
        "a record with no cover is still a record"
    );
}

/// One work, two performances. Falsified by keying a recording without its
/// performer: one recording held both.
#[test]
fn one_work_holds_every_recording_of_it() {
    let mut c = Lib::new();
    let favs = c.playlist("alice", "Favorites");
    for (performer, file) in [("Kimiko Ishizaka", "a"), ("Glenn Gould", "b")] {
        for no in 1..=3 {
            let title = ["Variatio 1", "Variatio 2", "Variatio 3"][no as usize - 1];
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
    let composers: Vec<ComposersEntry> = c.list("composers", &());
    assert_eq!(each(&composers, |p| p.name.clone()), ["Johann Sebastian Bach"]);
    assert_eq!(each(&composers, |p| p.works), [1], "one work, however many recordings");
    assert_eq!(each(&composers, |p| p.tracks), [6]);
    assert_eq!(
        each(&composers, |p| p.sort_name.clone()),
        ["Johann Sebastian Bach"],
        "the name, when nobody said how to sort it"
    );

    let works: Vec<WorkSummary> = c.list(
        "works",
        &Works {
            composer: "Johann Sebastian Bach".into(),
        },
    );
    assert_eq!(works.len(), 1, "two performances are not two works");
    assert_eq!(works[0].catalogue, "BWV 988");
    assert_eq!(works[0].id, harken_domain::host::work_key("Johann Sebastian Bach", "BWV 988", ""));
    assert_eq!(each(&works, |w| w.recordings), [2]);
    assert_eq!(each(&works, |w| w.tracks), [6]);
    assert_eq!(
        c.list::<WorkSummary>("work", &WorkInput { id: works[0].id.clone() }),
        works,
        "one work by its key is the same summary"
    );
    assert!(c.list::<WorkSummary>("work", &WorkInput { id: "nobody/nothing".into() }).is_empty());

    let takes: Vec<RecordingsEntry> = c.list(
        "recordings",
        &Recordings {
            work_id: works[0].id.clone(),
        },
    );
    assert_eq!(takes.len(), 2, "…and they are two recordings");
    let mut who = each(&takes, |t| t.performers.clone());
    who.sort();
    assert_eq!(who, ["Glenn Gould", "Kimiko Ishizaka"]);
    let one: Vec<LibraryEntry> = c.list(
        "recording",
        &RecordingInput {
            playlist_id: favs,
            id: takes[0].id.clone(),
        },
    );
    assert_eq!(titles(&one), ["Variatio 1", "Variatio 2", "Variatio 3"]);
}

/// A pop track is a song with no work. Falsified by `credited_as` naming
/// nobody for a track of no work: the performer column was empty.
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
    assert!(c.list::<ComposersEntry>("composers", &()).is_empty(), "nobody wrote a work");
    assert_eq!(each(&c.list::<ArtistsEntry>("artists", &()), |a| a.name.clone()), ["The Quiet Hours"]);
    assert_eq!(c.list::<AlbumsEntry>("albums", &()).len(), 1);
    assert_eq!(
        c.list::<LibraryEntry>(
            "album",
            &AlbumInput {
                playlist_id: favs,
                name: "Northerly".into()
            }
        )
        .len(),
        1
    );
    let details: Vec<TrackDetailsEntry> = c.list("track_details", &());
    assert_eq!(details[0].performer, "The Quiet Hours", "the artist is a credit on a recording");
    assert_eq!(details[0].catalogue, "", "no work, so no catalogue number");
    assert_eq!(
        (details[0].work.as_str(), details[0].composer.as_str()),
        ("", ""),
        "no work, so nobody composed it"
    );
    assert_eq!(details[0].album, "Northerly");
    assert_eq!(details[0].track, 7);
    let rec = c.rows::<Recording>("recording");
    assert_eq!(rec.len(), 1);
    assert_eq!(rec[0].work_id, None, "a recording of no work");
}

/// A track number is the release's and a movement number is the work's.
/// Falsified by `recording` sorting by track: "III. Presto" came first.
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
    let record: Vec<LibraryEntry> = c.list(
        "album",
        &AlbumInput {
            playlist_id: favs,
            name: "Piano Favourites".into(),
        },
    );
    assert_eq!(titles(&record), ["III. Presto", "I. Adagio"], "an album page is in the release's order");
    let works: Vec<WorkSummary> = c.list(
        "works",
        &Works {
            composer: "Ludwig van Beethoven".into(),
        },
    );
    let takes: Vec<RecordingsEntry> = c.list(
        "recordings",
        &Recordings {
            work_id: works[0].id.clone(),
        },
    );
    let in_the_work: Vec<LibraryEntry> = c.list(
        "recording",
        &RecordingInput {
            playlist_id: favs,
            id: takes[0].id.clone(),
        },
    );
    assert_eq!(titles(&in_the_work), ["I. Adagio", "III. Presto"], "a work page is in the work's order");
}

/// A catalogue number or a part names a work. Falsified by `work_title`
/// reading only the catalogue: the part-only entry named no work.
#[test]
fn an_entry_without_a_work_title_still_names_one() {
    let mut c = Lib::new();
    c.add(AddSong {
        file: "music/a.mp3".into(),
        track: 13,
        catalogue: "BWV 988".into(),
        performer: "Kimiko Ishizaka".into(),
        ..song("Variatio 12", "Johann Sebastian Bach", "Goldberg Variations")
    });
    let works: Vec<WorkSummary> = c.list(
        "works",
        &Works {
            composer: "Johann Sebastian Bach".into(),
        },
    );
    assert_eq!(
        each(&works, |w| w.title.clone()),
        ["Goldberg Variations"],
        "a catalogue number means there is a work"
    );
    assert_eq!(
        each(&c.list::<TrackDetailsEntry>("track_details", &()), |d| d.catalogue.clone()),
        ["BWV 988"]
    );
    assert_eq!(
        each(&c.list::<TrackDetailsEntry>("track_details", &()), |d| (
            d.work.clone(),
            d.composer.clone()
        )),
        [("Goldberg Variations".to_string(), "Johann Sebastian Bach".to_string())],
        "a track names the work it is a movement of, and who wrote it"
    );
    assert_eq!(
        c.rows::<Movement>("movement")[0].no,
        13,
        "the movement number is the track number when the entry did not say"
    );

    let mut c = Lib::new();
    c.add(AddSong {
        file: "music/b.mp3".into(),
        track: 12,
        part: "Suite No. 2 in D major".into(),
        ..song("Alla Hornpipe", "George Frideric Handel", "Water Music")
    });
    assert_eq!(
        each(&c.list::<TrackDetailsEntry>("track_details", &()), |d| d.part.clone()),
        ["Suite No. 2 in D major"]
    );
    assert_eq!(
        each(&c.list::<ComposersEntry>("composers", &()), |p| p.name.clone()),
        ["George Frideric Handel"]
    );
}

/// `credit_recording` turns one lumped string into people with roles.
/// Falsified by `performers` sorting by name alone: "Hermann Scherchen"
/// was billed first.
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
    let works: Vec<WorkSummary> = c.list(
        "works",
        &Works {
            composer: "George Frideric Handel".into(),
        },
    );
    let id = c.list::<RecordingsEntry>(
        "recordings",
        &Recordings {
            work_id: works[0].id.clone(),
        },
    )[0]
    .id
    .clone();
    let performer = |c: &Lib| each(&c.list::<TrackDetailsEntry>("track_details", &()), |d| d.performer.clone());
    assert_eq!(performer(&c), ["London Symphony Orchestra, Hermann Scherchen"]);
    let lumped: Vec<CreditsEntry> = c.list("credits", &Credits { recording_id: id.clone() });
    assert_eq!(each(&lumped, |r| r.role.clone()), ["performer"]);
    assert_eq!(each(&lumped, |r| r.pos), [0], "the fallback is at 0");

    let credit = |c: &mut Lib, name: &str, role: &str, pos: i64| {
        c.mutate(
            "alice",
            "credit_recording",
            &CreditRecording {
                recording_id: id.clone(),
                person_name: name.into(),
                role: role.into(),
                instrument: String::new(),
                pos,
            },
        )
    };
    credit(&mut c, "Hermann Scherchen", "conductor", 2).unwrap();
    credit(&mut c, "London Symphony Orchestra", "orchestra", 1).unwrap();
    let takes: Vec<RecordingsEntry> = c.list(
        "recordings",
        &Recordings {
            work_id: works[0].id.clone(),
        },
    );
    assert_eq!(
        each(&takes, |t| t.performers.clone()),
        ["London Symphony Orchestra, Hermann Scherchen"],
        "billing order, the lumped string displaced"
    );
    assert_eq!(performer(&c), ["London Symphony Orchestra, Hermann Scherchen"]);
    let credits: Vec<CreditsEntry> = c.list("credits", &Credits { recording_id: id.clone() });
    assert_eq!(each(&credits, |r| r.name.clone()), ["London Symphony Orchestra", "Hermann Scherchen"]);
    assert_eq!(each(&credits, |r| r.role.clone()), ["orchestra", "conductor"]);

    credit(&mut c, "Hermann Scherchen", "conductor", 0).unwrap();
    credit(&mut c, "Somebody Else", "", 0).unwrap();
    let credits: Vec<CreditsEntry> = c.list("credits", &Credits { recording_id: id.clone() });
    assert_eq!(
        each(&credits, |r| (r.name.clone(), r.role.clone(), r.pos)),
        [
            ("London Symphony Orchestra".into(), "orchestra".into(), 1),
            ("Somebody Else".into(), "artist".into(), 1),
            ("Hermann Scherchen".into(), "conductor".into(), 2),
        ]
    );
    let none = CreditRecording {
        recording_id: "nobody/nothing@x".into(),
        person_name: "Somebody".into(),
        role: "conductor".into(),
        instrument: String::new(),
        pos: 1,
    };
    assert_eq!(
        c.mutate("alice", "credit_recording", &none),
        Err("no recording nobody/nothing@x to credit anybody on".into())
    );
}

/// Describing fills in and never erases. Falsified by `describe_work`
/// writing `form` unconditionally: the second pass erased "Symphony".
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
    let works = |c: &Lib| {
        c.list::<WorkSummary>(
            "works",
            &Works {
                composer: "Ludwig van Beethoven".into(),
            },
        )
    };
    let id = works(&c)[0].id.clone();
    let describe = |c: &mut Lib, id: &str, key: &str, form: &str, period: &str, composed: i64| {
        let input = DescribeWork {
            id: id.into(),
            key_sig: key.into(),
            form: form.into(),
            period: period.into(),
            composed,
            ..DescribeWork::default()
        };
        c.mutate("alice", "describe_work", &input)
    };
    describe(&mut c, &id, "C Minor", "Symphony", "", 1808).unwrap();
    describe(&mut c, &id, "", "", "Classical", 0).unwrap();
    let work = &works(&c)[0];
    assert_eq!(work.period, "Classical", "the second pass said this");
    assert_eq!(work.form, "Symphony", "…and must not have erased this");
    let row = c.rows::<Work>("work").remove(0);
    assert_eq!((row.key_sig, row.composed), ("C Minor".to_string(), 1808));
    assert_eq!(
        describe(&mut c, "nobody/nothing", "", "", "", 0),
        Err("no work nobody/nothing; a work exists because a song named it".into())
    );

    let rid = c.list::<RecordingsEntry>("recordings", &Recordings { work_id: id.clone() })[0].id.clone();
    let recorded = |c: &mut Lib, id: &str, year: i64, licence: &str| {
        let input = DescribeRecording {
            id: id.into(),
            recorded: year,
            label: "DG".into(),
            licence: licence.into(),
            ..DescribeRecording::default()
        };
        c.mutate("alice", "describe_recording", &input)
    };
    recorded(&mut c, &rid, 1975, "CC BY 4.0").unwrap();
    recorded(&mut c, &rid, 0, "").unwrap();
    let take = &c.list::<RecordingsEntry>("recordings", &Recordings { work_id: id.clone() })[0];
    assert_eq!((take.recorded, take.licence.as_str()), (1975, "CC BY 4.0"));
    assert_eq!(
        each(&c.list::<TrackDetailsEntry>("track_details", &()), |d| d.licence.clone()),
        ["CC BY 4.0"],
        "the licence reaches the table"
    );
    assert_eq!(
        recorded(&mut c, "x@y", 1, ""),
        Err("no recording x@y; one exists because a song is part of it".into())
    );

    let person = |c: &mut Lib, name: &str, sort: &str, born: i64| {
        c.mutate(
            "alice",
            "describe_person",
            &DescribePerson {
                name: name.into(),
                sort_name: sort.into(),
                born,
                ..DescribePerson::default()
            },
        )
    };
    person(&mut c, "Ludwig van Beethoven", "Beethoven, Ludwig van", 1770).unwrap();
    person(&mut c, "Ludwig van Beethoven", "", 0).unwrap();
    let composers: Vec<ComposersEntry> = c.list("composers", &());
    assert_eq!(each(&composers, |p| p.sort_name.clone()), ["Beethoven, Ludwig van"]);
    assert_eq!(each(&composers, |p| p.born), [1770]);
    let artists: Vec<ArtistsEntry> = c.list("artists", &());
    let beethoven = artists.iter().find(|a| a.name == "Ludwig van Beethoven").map(|a| (a.born, a.died));
    assert_eq!(beethoven, Some((1770, 0)), "an artist's years are their person's");
    person(&mut c, "Clara Schumann", "", 1819).unwrap();
    assert_eq!(
        each(&c.list::<ComposersEntry>("composers", &()), |p| p.name.clone()),
        ["Ludwig van Beethoven"],
        "not a composer until a work is theirs"
    );
    assert_eq!(person(&mut c, "  ", "", 0), Err("a person needs a name".into()));
}

/// The fields only a song has come through `add_song`'s bounds. Falsified
/// by clamping `disc` to 9: the second song's disc was 9.
#[test]
fn a_song_keeps_its_own_facts_in_bounds() {
    let mut c = Lib::new();
    c.add(AddSong {
        file: "a".into(),
        track: 1200,
        bpm: 400,
        disc: 0,
        ..song("Too Much", "X", "Y")
    });
    c.add(AddSong {
        file: "b".into(),
        track: -3,
        bpm: 120,
        disc: 250,
        ..song("Fine", "X", "")
    });
    let mut got = each(&c.rows::<Song>("song"), |s| (s.album_name.clone(), s.track, s.bpm, s.disc));
    got.sort_by_key(|g| g.1);
    assert_eq!(got, [(None, 0, 120, 99), (Some("Y".into()), 999, 0, 1)]);
}

/// Everything onto a playlist in one entry, in library order, skipping what
/// is there. Falsified by not skipping: the insert of "B" again was refused.
#[test]
fn the_whole_library_onto_a_playlist() {
    let mut c = Lib::new();
    let favs = c.playlist("alice", "Favorites");
    for (t, f) in [("A", "a"), ("B", "b"), ("C", "c")] {
        c.add(with(f, song(t, "X", "")));
    }
    let b = c.library(&favs)[1].id;
    c.mutate("alice", "add_to_playlist", &on(&favs, &b)).unwrap();
    let all = AddAllToPlaylist { playlist_id: favs };
    c.mutate("alice", "add_all_to_playlist", &all).unwrap();
    let on_it: Vec<LibraryEntry> = c.list("playlist", &PlaylistInput { playlist_id: favs });
    assert_eq!(titles(&on_it), ["B", "A", "C"]);
    assert_eq!(each(&on_it, |r| r.playlist_pos), [Some(1), Some(2), Some(3)]);
    assert_eq!(c.mutate("alice", "add_all_to_playlist", &all), Ok(0));
}

/// A first sign-in's welcome makes the person's Favorites, theirs, once
/// however often it runs; and nobody but the server's authority runs it.
/// Falsified by dropping the authority's check: mallory made alice a
/// playlist.
#[test]
fn the_welcome_is_the_servers_and_makes_one_favorites() {
    let mut c = Lib::new();
    let alice = ark_auth::Welcome {
        user: "alice".into(),
        name: "Alice".into(),
    };
    assert!(c.mutate("mallory", "welcome", &alice).is_err(), "only the server welcomes");
    assert!(names(&c, "alice").is_empty());
    c.mutate("authority", "welcome", &alice).unwrap();
    c.mutate("authority", "welcome", &alice).unwrap();
    assert_eq!(names(&c, "alice"), ["Favorites"]);
    let mine: Vec<Playlist> = c.query("alice", "playlists", &()).unwrap();
    assert_eq!((mine[0].id, mine[0].user_id.as_str()), (playlists::favorites_of("alice"), "alice"));
}

/// A recording with no picture of its own is drawn with the cover of the
/// album it was recorded on. Falsified by answering `recording.art` alone:
/// the take's art was empty.
#[test]
fn a_recording_wears_its_albums_cover() {
    let mut c = Lib::new();
    c.add(AddSong {
        file: "music/a.mp3".into(),
        track: 1,
        catalogue: "BWV 988".into(),
        performer: "Kimiko Ishizaka".into(),
        album_art: "covers/goldberg.jpg".into(),
        ..song("Aria", "Johann Sebastian Bach", "Goldberg Variations")
    });
    let works: Vec<WorkSummary> = c.list(
        "works",
        &Works {
            composer: "Johann Sebastian Bach".into(),
        },
    );
    let takes: Vec<RecordingsEntry> = c.list(
        "recordings",
        &Recordings {
            work_id: works[0].id.clone(),
        },
    );
    assert_eq!(each(&takes, |r| r.art.clone()), ["covers/goldberg.jpg"]);
}
