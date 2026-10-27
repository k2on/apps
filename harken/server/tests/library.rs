//! The scanner, against a real directory of real files, through the whole
//! server.
//!
//! What this asserts is the property the feature exists for, not that the
//! code runs: a directory of audio becomes songs in the log, a file that
//! appears later becomes one too, a new album directory is walked, a file
//! that is not audio — or is audio outside `music/` — is not a song, and a
//! rescan on a fresh scanner over the same log authors nothing at all. And
//! the covers: beside a record or inside its tracks, served where the log
//! says; a file retagged, or given a cover, is the same song said again;
//! and a rescan of nothing new reads nothing.
//!
//! The files are written here rather than checked in. A WAV is a header and
//! some samples, so a test can make one lofty will really parse — which is
//! the part a fixture of zero bytes would not exercise.

mod common;

use common::*;
use harken_server::domain::LibraryEntry;
use harken_server::domain::{AddToPlaylist, CreatePlaylist, PlaylistInput};
use harken_server::domain::{Album, Media, Person, Playlist};
use harken_server::Server;
use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::tag::{Accessor, ItemKey, Tag, TagExt, TagType};
use sha2::{Digest, Sha256};

/// `(title, file)` for every song the server's log holds.
fn songs(server: &Server) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = hub_rows::<Media>(server, "media")
        .into_iter()
        .map(|m| (m.title, m.file))
        .collect();
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

/// The entries in the hub's log but the scanner's looks (asked, claimed,
/// finished: `docs/plan-v5.md` S3a), which a rescan is.
fn head(server: &Server) -> i64 {
    let entries = server.running.hub.with(|h| h.log.log().entries()).unwrap();
    let look = |f: &str| f == "library.look" || f.starts_with("look.");
    entries.iter().filter(|l| !look(&l.entry.func)).count() as i64
}

/// Wait until there are `want` songs, then a little longer, so that a
/// scanner writing duplicates is seen doing it rather than stopped short.
fn settle(server: &Server, want: usize) -> Vec<(String, String)> {
    eventually(20, || (songs(server).len() >= want).then_some(()));
    std::thread::sleep(std::time::Duration::from_millis(600));
    songs(server)
}

fn files(found: &[(String, String)]) -> Vec<&str> {
    found.iter().map(|(_, f)| f.as_str()).collect()
}

#[test]
fn a_directory_of_audio_becomes_songs_and_a_rescan_adds_none() {
    let rt = runtime();
    let root = tempfile::tempdir().unwrap();
    // The media root is a directory *inside* the temp one, so the server's
    // own state can sit beside it rather than in it: a replica in the
    // watched tree is a feedback loop.
    let (media, data) = (root.path().join("media"), root.path().join("data"));
    let music = media.join("music");
    wav(&music.join("Bach/air.wav"), 2_000);
    wav(&music.join("Beethoven/Bagatelles/fur-elise.wav"), 3_000);
    // Not audio, and not songs. A library folder is full of these.
    std::fs::write(music.join("cover.jpg"), b"not audio").unwrap();
    std::fs::write(music.join("Bach/notes.txt"), b"nor this").unwrap();
    // Audio, and not a track: it is not under `music/`. At boot only
    // `music/` is walked, so this one is never even offered — the one that
    // appears later, below, is what reaches the refusal.
    wav(&media.join("podcasts/episode-0.wav"), 1_000);

    let server = serve(&rt, &data, |c| c.media = Some(media.clone()));
    let found = settle(&server, 2);
    assert_eq!(
        files(&found),
        [
            "music/Bach/air.wav",
            "music/Beethoven/Bagatelles/fur-elise.wav"
        ],
        "every audio file under `music/`, by its path relative to the *media* root — the \
         same string `/media/` serves back — and nothing else"
    );
    assert_eq!(
        found[0].0, "air",
        "an untagged file is its name, and nothing more"
    );
    let rows = hub_rows::<Media>(&server, "media");
    let air = rows
        .iter()
        .find(|r| r.file == "music/Bach/air.wav")
        .unwrap();
    assert_eq!(air.duration_ms, 2_000, "the duration is the file's, read");
    assert_eq!(air.creator, "", "no artist guessed from the folder");
    assert_eq!(air.user_id, "library", "authored as the library, signed in");

    // A file appearing after the scan, without anyone asking for a rescan.
    wav(&music.join("Debussy/clair-de-lune.wav"), 1_000);
    assert_eq!(
        settle(&server, 3).len(),
        3,
        "the watch picked up a new file"
    );

    // A whole album directory dropped in at once: the directory is what is
    // announced, and it is walked.
    let staging = root.path().join("staging/Satie");
    wav(&staging.join("gymnopedie-1.wav"), 1_000);
    wav(&staging.join("gymnopedie-2.wav"), 1_000);
    std::fs::rename(&staging, music.join("Satie")).unwrap();
    assert_eq!(
        settle(&server, 5).len(),
        5,
        "a new directory's tracks are found"
    );

    // And audio appearing outside `music/`, which the watch reports — it
    // covers the whole root — and which is not a track.
    wav(&media.join("podcasts/episode-1.wav"), 1_000);
    std::thread::sleep(std::time::Duration::from_millis(1_200));
    assert_eq!(
        songs(&server).len(),
        5,
        "audio outside `music/` is not a song"
    );

    // A file removed is not a song removed: the log is permanent, and an
    // unplugged disk looks exactly the same.
    std::fs::remove_file(music.join("Bach/air.wav")).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(800));
    assert_eq!(songs(&server).len(), 5, "removals are ignored");
    wav(&music.join("Bach/air.wav"), 2_000);

    // Now the point: a whole new scanner over the same log, with a replica
    // of its own that has never seen any of it. It catches up first, so the
    // rescan authors nothing at all — not even entries the procedure would
    // make no-ops of.
    let scanned = server.scanner.as_ref().unwrap().authored();
    assert_eq!(scanned, 5, "one entry per song, never a second");
    let dir = root.path().join("fresh-scanner");
    let login = server
        .auth
        .issue(
            &ark_auth::Account {
                id: "library".into(),
                ..Default::default()
            },
            "a test",
        )
        .unwrap();
    // After its sign-in, itself an entry.
    let before = head(&server);
    let again = harken_server::library::Scanner::start(
        media.clone(),
        hosted().clone(),
        server.running.hub.clone(),
        dir,
        login,
        None,
        None,
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1_200));
    again.rescan();
    std::thread::sleep(std::time::Duration::from_millis(800));
    assert_eq!(
        again.authored(),
        0,
        "a rescan authors nothing the library already says"
    );
    assert_eq!(
        again.read(),
        5,
        "a scanner that never kept a print reads every file, once"
    );
    assert_eq!(head(&server), before, "and the log did not move");
    assert_eq!(songs(&server).len(), 5);
    drop(again);
    rt.block_on(server.stop());
}

/// What the tags say is what the song is: title, artist, album, number,
/// the album artist as the performer, and the tempo.
#[test]
fn the_tags_are_what_a_song_is() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("tagged.wav");
    wav(&path, 1_500);
    let mut file = lofty::read_from_path(&path).unwrap();
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Clair de lune".into());
    tag.set_artist("Claude Debussy".into());
    tag.set_album("Suite bergamasque".into());
    tag.set_track(3);
    tag.set_track_total(4);
    tag.insert_text(ItemKey::AlbumArtist, "  Ivan Ilić ".into());
    tag.insert_text(ItemKey::Bpm, "not a number".into());
    file.insert_tag(tag);
    file.save_to_path(&path, WriteOptions::default()).unwrap();

    let track = harken_server::library::read(&path).expect("a tagged file is read");
    assert_eq!(track.title, "Clair de lune");
    assert_eq!(track.artist, "Claude Debussy");
    assert_eq!(track.album, "Suite bergamasque");
    assert_eq!(track.number, 3, "the track, not the total");
    assert_eq!(track.performer, "Ivan Ilić");
    assert_eq!(track.bpm, 0, "a number nobody can read is not a number");
    assert!(
        (1_400..=1_600).contains(&track.duration_ms),
        "{}",
        track.duration_ms
    );

    assert!(harken_server::library::read(&root.path().join("missing.wav")).is_none());
    let junk = root.path().join("junk.mp3");
    std::fs::write(&junk, b"this is not an mp3").unwrap();
    assert!(
        harken_server::library::read(&junk).is_none(),
        "not audio lofty can read"
    );
}

/// A classical file's standard tags are read: the composer is who it is
/// by and the artist tag who played it, the work and its catalogue number,
/// the movement numbered as the title, and the disc. Falsified by reading
/// the artist tag as the artist whatever the composer: it said L'Astrée.
#[test]
fn a_classical_files_tags_name_its_work() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("rv68.wav");
    wav(&path, 1_500);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Sonata in F major, RV 68: II. Andante".into());
    tag.set_artist("L'Astrée".into());
    tag.set_album("Sonate da Camera".into());
    tag.set_track(2);
    tag.set_disk(1);
    tag.insert_text(ItemKey::Composer, "Antonio Vivaldi".into());
    tag.insert_text(ItemKey::Movement, "Andante".into());
    tag.insert_text(ItemKey::MovementNumber, "2".into());
    // The work is `TXXX:WORK`, which lofty's generic tag cannot write
    // (it names the frame `WORK`), so it goes in as the user text it is.
    let mut id3 = lofty::id3::v2::Id3v2Tag::from(tag);
    id3.insert_user_text("WORK".into(), "Sonata in F major, RV 68".into());
    id3.save_to_path(&path, WriteOptions::default()).unwrap();

    let track = harken_server::library::read(&path).expect("a tagged file is read");
    assert_eq!(track.artist, "Antonio Vivaldi", "who it is by");
    assert_eq!(track.performer, "L'Astrée", "who played it");
    assert_eq!(track.title, "II. Andante", "the movement, numbered");
    assert_eq!(
        (track.work.as_str(), track.catalogue.as_str()),
        ("Sonata in F major, RV 68", "RV 68")
    );
    assert_eq!((track.movement_no, track.number, track.disc), (2, 2, 1));
    assert_eq!(
        harken_server::library::catalogue("Tafelmusik I: Quartet in G major, TWV 43:G2"),
        "TWV 43:G2"
    );
    assert_eq!(
        harken_server::library::catalogue("Spring, Summer"),
        "",
        "no index, no number"
    );
}

/// The server serves what the scanner wrote at the path it wrote: the
/// `file` column joined to `/media/` is the file, and a range request is
/// answered as one.
#[test]
fn a_songs_file_is_served_at_the_path_the_log_carries() {
    let rt = runtime();
    let root = tempfile::tempdir().unwrap();
    let media = root.path().join("media");
    wav(&media.join("music/Bach/air.wav"), 500);
    let server = serve(&rt, &root.path().join("data"), |c| {
        c.media = Some(media.clone())
    });
    let found = settle(&server, 1);
    let url = harken_server::domain::host::url(&server.url(), &found[0].1);
    let whole = ureq::get(&url).call().unwrap();
    assert_eq!(whole.status(), 200);
    let res = ureq::get(&url).set("Range", "bytes=0-3").call().unwrap();
    assert_eq!(res.status(), 206, "seeking is a range request");
    let mut body = vec![];
    std::io::Read::read_to_end(&mut res.into_reader(), &mut body).unwrap();
    assert_eq!(body, b"RIFF");
    // …with no sign-in: which is what lets a speaker fetch it.
    rt.block_on(server.stop());
}

/// A JPEG and a PNG as far as anything here asks: the bytes a picture
/// starts with, and some more. Nothing decodes them.
const JPEG: &[u8] = b"\xFF\xD8\xFF\xE0 a cover beside the tracks";
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n a picture inside the tracks";

/// A tagged WAV at `path`, made beside and renamed in so a watch sees it
/// whole: a title, an artist, an album, and a front cover when one is
/// given.
fn put(
    root: &std::path::Path,
    path: &std::path::Path,
    title: &str,
    artist: &str,
    album: &str,
    picture: Option<&[u8]>,
) {
    let staging = root.join("staging").join(path.file_name().unwrap());
    wav(&staging, 300);
    let mut file = lofty::read_from_path(&staging).unwrap();
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(title.into());
    tag.set_artist(artist.into());
    tag.set_album(album.into());
    if let Some(p) = picture {
        tag.push_picture(
            Picture::unchecked(p.to_vec())
                .pic_type(PictureType::CoverFront)
                .mime_type(MimeType::Png)
                .build(),
        );
    }
    file.insert_tag(tag);
    file.save_to_path(&staging, WriteOptions::default())
        .unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::rename(&staging, path).unwrap();
}

/// Beside and renamed in, for a picture.
fn picture(path: &std::path::Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let tmp = path.with_extension("partial");
    std::fs::write(&tmp, bytes).unwrap();
    std::fs::rename(&tmp, path).unwrap();
}

/// A row's `art`, by its table and its name.
fn art(server: &Server, table: &str, name: &str) -> Option<String> {
    match table {
        "album" => hub_rows::<Album>(server, "album")
            .into_iter()
            .find(|r| r.name == name)
            .map(|r| r.art),
        "person" => hub_rows::<Person>(server, "person")
            .into_iter()
            .find(|r| r.name == name)
            .map(|r| r.art),
        other => panic!("no art in {other}"),
    }
}

/// The status line's code for a GET of `path` exactly as written: no
/// client between, to tidy a `..` away before the server sees it.
fn raw_status(server: &Server, path: &str) -> u16 {
    use std::io::{Read, Write};
    let addr = server.url().trim_start_matches("http://").to_string();
    let mut s = std::net::TcpStream::connect(&addr).unwrap();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut out = vec![];
    let _ = s.read_to_end(&mut out);
    String::from_utf8_lossy(&out)
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0)
}

fn fetch(server: &Server, art: &str) -> (String, Vec<u8>) {
    let url = harken_server::domain::host::url(&server.url(), art);
    let res = ureq::get(&url)
        .call()
        .unwrap_or_else(|e| panic!("{url}: {e}"));
    let kind = res.content_type().to_string();
    let mut body = vec![];
    std::io::Read::read_to_end(&mut res.into_reader(), &mut body).unwrap();
    (kind, body)
}

/// A cover beside a record is its album's, and an `artist.jpg` above it the
/// artist's; a record with none takes the picture inside its tracks, kept
/// once in the data directory by its hash and served under `/media/.art/`
/// with its type. Each is fetched from `<server>/media/<art>` — the join
/// every client already makes for a track. Nothing reaches outside either
/// directory by `..`, written or encoded.
///
/// Falsified by offering the picture inside a track before the cover
/// beside it: Goldberg's art was `.art/…`, not its `cover.jpg`; by naming
/// an extracted picture without its extension, which is all that tells
/// `/media` its type: the value had none; and by keeping the pictures in
/// the data directory itself rather than in its `art`:
/// `/media/.art/library-files.json` answered 200 (the prints were a
/// file of the data directory then).
#[test]
fn a_record_takes_its_cover_from_beside_it_or_from_inside_its_tracks() {
    let rt = runtime();
    let root = tempfile::tempdir().unwrap();
    let (media, data) = (root.path().join("media"), root.path().join("data"));
    let music = media.join("music");
    let goldberg = music.join("Bach/Goldberg");
    put(
        root.path(),
        &goldberg.join("01.wav"),
        "Aria",
        "Bach",
        "Goldberg Variations",
        Some(PNG),
    );
    put(
        root.path(),
        &goldberg.join("02.wav"),
        "Variatio 1",
        "Bach",
        "Goldberg Variations",
        Some(PNG),
    );
    picture(&goldberg.join("cover.jpg"), JPEG);
    picture(
        &music.join("Bach/artist.jpg"),
        b"\xFF\xD8\xFF\xE0 Bach himself",
    );
    let satie = music.join("Satie/Gymnopedies");
    put(
        root.path(),
        &satie.join("1.wav"),
        "Gymnopedie No. 1",
        "Satie",
        "Gymnopedies",
        Some(PNG),
    );
    put(
        root.path(),
        &satie.join("2.wav"),
        "Gymnopedie No. 2",
        "Satie",
        "Gymnopedies",
        Some(PNG),
    );

    let server = serve(&rt, &data, |c| c.media = Some(media.clone()));
    settle(&server, 4);
    let ready = || art(&server, "album", "Gymnopedies").filter(|a| !a.is_empty());
    let extracted = eventually(10, ready).expect("the picture inside the tracks is the album's");

    let beside = art(&server, "album", "Goldberg Variations").unwrap();
    assert!(beside.starts_with(".cover/"), "{beside}");
    assert!(
        beside.ends_with("/music/Bach/Goldberg/cover.jpg"),
        "the cover beside, before the picture inside: {beside}"
    );
    assert_eq!(
        fetch(&server, &beside),
        ("image/jpeg".to_string(), JPEG.to_vec())
    );
    let bach = art(&server, "person", "Bach").unwrap();
    assert!(bach.ends_with("/music/Bach/artist.jpg"), "{bach}");
    assert_eq!(
        art(&server, "person", "Satie").as_deref(),
        Some(""),
        "an artist's picture is never invented"
    );

    let sha: String = Sha256::digest(PNG)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(extracted, format!(".art/{sha}.png"));
    assert_eq!(
        fetch(&server, &extracted),
        ("image/png".to_string(), PNG.to_vec())
    );

    // Nothing above either directory, however it is spelt.
    assert_eq!(
        raw_status(&server, &format!("/media/.art/{sha}.png")),
        200,
        "the probe reaches the server"
    );
    for path in [
        "/media/.art/library-files/log",
        "/media/.art/../library-files/log",
        "/media/.art/%2e%2e/library-files/log",
        "/media/.art/..%2flibrary-files/log",
        "/media/.cover/x/../../data/library-files/log",
        "/media/.cover/x/%2e%2e/data/library-files/log",
        "/media/%2e%2e/data/library-files/log",
    ] {
        assert_ne!(raw_status(&server, path), 200, "{path}");
    }
    let kept: Vec<_> = std::fs::read_dir(data.join("art"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(
        kept,
        [std::ffi::OsString::from(format!("{sha}.png"))],
        "tracks carrying one picture are one file"
    );
    rt.block_on(server.stop());
}

/// The library over its own media from the media's own events: a retagged
/// file is the same song — the same id, still on the playlist that held it
/// — saying its new title; a cover put beside a record later is its album's,
/// and a cover replaced under the same name is a new value, so no client
/// goes on drawing the one it fetched first.
///
/// Falsified by authoring a known file only when it is not yet a song (the
/// scanner's old rule): the title never moved; and by leaving the covers'
/// prints out of a track's: the cover put beside it later never reached
/// the album.
#[test]
fn a_retagged_file_or_a_new_cover_is_the_same_song_said_again() {
    let rt = runtime();
    let root = tempfile::tempdir().unwrap();
    let (media, data) = (root.path().join("media"), root.path().join("data"));
    let isles = media.join("music/Bicep/Isles");
    put(
        root.path(),
        &isles.join("01.wav"),
        "Glue",
        "Bicep",
        "Isles",
        None,
    );
    put(
        root.path(),
        &isles.join("02.wav"),
        "Opal",
        "Bicep",
        "Isles",
        None,
    );
    let server = serve(&rt, &data, |c| c.media = Some(media.clone()));
    settle(&server, 2);
    let id_of = |title: &str| {
        hub_rows::<Media>(&server, "media")
            .into_iter()
            .find(|r| r.title == title)
            .map(|r| r.id)
    };
    let glue = id_of("Glue").unwrap();

    let mut alice = signed_in(&server, "alice");
    pump_until(&mut [&mut alice], 10, |p| {
        p[0].rows::<Media>("media").len() == 2
    });
    alice.mutate(
        "playlists.create_playlist",
        &CreatePlaylist {
            name: "Mine".into(),
        },
    );
    let list: Vec<Playlist> = alice.query("playlists.playlists", &()).unwrap();
    let mine = list[0].id;
    alice.mutate(
        "playlists.add_to_playlist",
        &AddToPlaylist {
            playlist_id: mine,
            media_id: glue,
        },
    );
    let titles = |p: &Peer| {
        let rows: Vec<LibraryEntry> = p
            .query("playlists.playlist", &PlaylistInput { playlist_id: mine })
            .unwrap();
        rows.into_iter().map(|r| r.title).collect::<Vec<_>>()
    };
    pump_until(&mut [&mut alice], 10, |p| p[0].pending() == 0);

    put(
        root.path(),
        &isles.join("01.wav"),
        "Glue (Remastered)",
        "Bicep",
        "Isles",
        None,
    );
    assert_eq!(
        eventually(10, || id_of("Glue (Remastered)")),
        Some(glue),
        "retagged, and the same song"
    );
    assert_eq!(songs(&server).len(), 2, "and not a second one");
    pump_until(&mut [&mut alice], 10, |p| {
        titles(&*p[0]) == ["Glue (Remastered)"]
    });

    assert_eq!(art(&server, "album", "Isles").as_deref(), Some(""));
    picture(&isles.join("cover.jpg"), JPEG);
    let first = eventually(10, || {
        art(&server, "album", "Isles").filter(|a| !a.is_empty())
    })
    .expect("a cover put beside the record later is its album's");
    assert!(first.ends_with("/music/Bicep/Isles/cover.jpg"), "{first}");
    let better = b"\xFF\xD8\xFF\xE0 a better cover, and a longer one";
    picture(&isles.join("cover.jpg"), better);
    let second = eventually(10, || {
        art(&server, "album", "Isles").filter(|a| *a != first)
    })
    .expect("a cover replaced under its name replaces the album's");
    assert!(second.ends_with("/music/Bicep/Isles/cover.jpg"), "{second}");
    assert_eq!(fetch(&server, &second).1, better.to_vec());
    assert_eq!(songs(&server).len(), 2);
    drop(alice);
    rt.block_on(server.stop());
}

/// What the scanner has been through survives it, and costs nothing when
/// nothing moved: a rescan reads no tags and authors nothing, and neither
/// does a restart over the same data. A library scanned before any cover
/// was found — no prints at all, as on the first start of a server that
/// never kept them — is read once, and a cover beside a record is one entry
/// for the whole record. A file of prints that cannot be read is a full
/// read, and nothing worse.
///
/// Falsified by skipping no file however its print stood: the rescan read
/// three; by keeping the prints only in memory: the restart read three;
/// and by refusing to start over an unreadable file of them: the last
/// start failed. On v5: by `known_file` never finding the file in the
/// replica's `media (file)` index: the rescan read six; by `would_change`
/// answering yes whatever the run changed: the cover was three entries,
/// not one; and by opening the scanner's replica without signing it in as
/// the start's own login: the cover's entry was held at the hub behind a
/// gap of its new session's `local`s, and the album never had it.
#[test]
fn a_rescan_of_nothing_new_reads_nothing() {
    let rt = runtime();
    let root = tempfile::tempdir().unwrap();
    let (media, data) = (root.path().join("media"), root.path().join("data"));
    let album = media.join("music/Satie/Gymnopedies");
    for n in 1..=3 {
        put(
            root.path(),
            &album.join(format!("{n}.wav")),
            &format!("Gymnopedie No. {n}"),
            "Satie",
            "Gymnopedies",
            None,
        );
    }
    let server = serve(&rt, &data, |c| c.media = Some(media.clone()));
    settle(&server, 3);
    let scanner = server.scanner.as_ref().unwrap();
    assert_eq!((scanner.read(), scanner.authored()), (3, 3));
    let before = head(&server);
    scanner.rescan();
    std::thread::sleep(std::time::Duration::from_millis(1_000));
    assert_eq!(
        (scanner.read(), scanner.authored()),
        (3, 3),
        "a rescan of nothing new reads nothing"
    );
    assert_eq!(head(&server), before);
    rt.block_on(server.stop());

    let server = serve(&rt, &data, |c| c.media = Some(media.clone()));
    std::thread::sleep(std::time::Duration::from_millis(1_500));
    let scanner = server.scanner.as_ref().unwrap();
    assert_eq!(
        (scanner.read(), scanner.authored()),
        (0, 0),
        "nor does a restart"
    );
    assert_eq!(head(&server), before);
    rt.block_on(server.stop());

    // A library from before covers were found: the cover is there now, and
    // no file of prints says what was offered.
    picture(&album.join("folder.jpg"), JPEG);
    std::fs::remove_dir_all(data.join("library-files")).unwrap();
    let server = serve(&rt, &data, |c| c.media = Some(media.clone()));
    let got = eventually(10, || {
        art(&server, "album", "Gymnopedies").filter(|a| !a.is_empty())
    });
    assert!(
        got.is_some_and(|a| a.ends_with("/folder.jpg")),
        "the first start that keeps prints gives the record its cover"
    );
    std::thread::sleep(std::time::Duration::from_millis(600));
    let scanner = server.scanner.as_ref().unwrap();
    assert_eq!(
        (scanner.read(), scanner.authored()),
        (3, 1),
        "every file read once, and one entry for one cover"
    );
    assert_eq!(songs(&server).len(), 3, "and no song twice");
    let after = head(&server);
    rt.block_on(server.stop());

    std::fs::write(data.join("library-files/log"), b"{ not a record").unwrap();
    let _ = std::fs::remove_file(data.join("library-files/kv"));
    let server = serve(&rt, &data, |c| c.media = Some(media.clone()));
    let scanner = server.scanner.as_ref().unwrap();
    eventually(10, || (scanner.read() == 3).then_some(()));
    std::thread::sleep(std::time::Duration::from_millis(600));
    assert_eq!(
        (scanner.read(), scanner.authored()),
        (3, 0),
        "an unreadable file of prints is one full read"
    );
    assert_eq!(head(&server), after);
    rt.block_on(server.stop());
}

/// A look is a job (`docs/plan-v5.md` S3a): the scanner asks for one at
/// boot and on a rescan, its handler claims and does it, and the row is
/// done with what it read and authored. Falsified by the scanner
/// registering no handler: the looks stayed pending and no song came.
#[test]
fn a_look_is_a_job_the_scanner_does() {
    use ark::effect::Job;
    use harken_server::domain::{Look, Looked};
    let rt = runtime();
    let root = tempfile::tempdir().unwrap();
    let (media, data) = (root.path().join("media"), root.path().join("data"));
    for n in 1..=2 {
        let file = media.join(format!("music/Satie/{n}.wav"));
        put(
            root.path(),
            &file,
            &format!("Gnossienne {n}"),
            "Satie",
            "Gnossiennes",
            None,
        );
    }
    let server = serve(&rt, &data, |c| c.media = Some(media.clone()));
    settle(&server, 2);
    server.scanner.as_ref().unwrap().rescan();
    let done = |n: usize| {
        eventually(20, || {
            let looks = hub_rows::<Look>(&server, "look");
            let done: Vec<Job<Looked>> = looks
                .into_iter()
                .map(|l| l.state)
                .filter(|s| matches!(s, Job::Done(_)))
                .collect();
            (done.len() >= n).then_some(done)
        })
        .expect("the looks done")
    };
    let read = |l: &Job<Looked>| match l {
        Job::Done(l) => (l.read, l.authored),
        _ => (0, 0),
    };
    let looks = done(2);
    let mut looked: Vec<(i64, i64)> = looks.iter().map(read).collect();
    looked.sort();
    assert_eq!(looked, [(0, 0), (2, 2)], "the rescan's, and the boot's");
    rt.block_on(server.stop());
}

/// An artist's page opens with the wide picture beside their own: a
/// `backdrop.jpg` where `artist.jpg` is, in the directory named for them
/// above their records, served where the log says.
#[test]
fn an_artist_takes_a_backdrop_from_beside_their_picture() {
    let rt = runtime();
    let root = tempfile::tempdir().unwrap();
    let (media, data) = (root.path().join("media"), root.path().join("data"));
    let music = media.join("music");
    picture(&music.join("Vivaldi/artist.jpg"), b"\xFF\xD8\xFF vivaldi");
    picture(
        &music.join("Vivaldi/backdrop.jpg"),
        b"\xFF\xD8\xFF the red cloak",
    );
    put(
        root.path(),
        &music.join("Vivaldi/Seasons/spring.wav"),
        "Spring",
        "Antonio Vivaldi",
        "The Four Seasons",
        None,
    );
    let server = serve(&rt, &data, |c| c.media = Some(media.clone()));
    settle(&server, 1);
    let vivaldi = eventually(20, || {
        hub_rows::<Person>(&server, "person")
            .into_iter()
            .find(|p| p.name == "Antonio Vivaldi" && !p.banner.is_empty())
    })
    .expect("Vivaldi's backdrop");
    assert_eq!(fetch(&server, &vivaldi.art).1, b"\xFF\xD8\xFF vivaldi");
    assert_eq!(
        fetch(&server, &vivaldi.banner),
        (
            "image/jpeg".to_string(),
            b"\xFF\xD8\xFF the red cloak".to_vec()
        )
    );
    rt.block_on(server.stop());
}

/// A folder under `podcasts/` or `sermons/` is a show, named for it and
/// pictured by the cover in it, and each audio file in it an episode of
/// it: a media row of its kind, which the music leaves out. A file loose in
/// `podcasts/` is nobody's show.
#[test]
fn a_folder_of_episodes_is_a_show() {
    use harken_server::domain::{Episode, Show};
    let rt = runtime();
    let root = tempfile::tempdir().unwrap();
    let (media, data) = (root.path().join("media"), root.path().join("data"));
    wav(&media.join("music/Bach/air.wav"), 2_000);
    wav(
        &media.join("podcasts/Full Time Nix/nix-freaks-51.wav"),
        1_000,
    );
    wav(
        &media.join("podcasts/Full Time Nix/nix-freaks-50.wav"),
        1_000,
    );
    std::fs::write(media.join("podcasts/Full Time Nix/cover.jpg"), b"a picture").unwrap();
    wav(
        &media.join("sermons/Gospel in Life/the-dream-of-the-kingdom.wav"),
        3_000,
    );
    wav(&media.join("podcasts/loose.wav"), 1_000);

    let server = serve(&rt, &data, |c| c.media = Some(media.clone()));
    let found = settle(&server, 4);
    assert_eq!(found.len(), 4, "{found:?}");
    let mut shows = hub_rows::<Show>(&server, "show");
    shows.sort_by(|a, b| a.id.cmp(&b.id));
    let said: Vec<(&str, &str, &str, &str)> = shows
        .iter()
        .map(|s| {
            (
                s.id.as_str(),
                s.kind.as_str(),
                s.title.as_str(),
                // A cover is spelt versioned (`.cover/<print>/…`), as a
                // record's is.
                s.art.split_once("/podcasts/").map_or("", |(_, a)| a),
            )
        })
        .collect();
    assert_eq!(
        said,
        [
            (
                "podcast/full-time-nix",
                "podcast",
                "Full Time Nix",
                "Full Time Nix/cover.jpg"
            ),
            ("sermon/gospel-in-life", "sermon", "Gospel in Life", ""),
        ]
    );
    let media_rows = hub_rows::<Media>(&server, "media");
    let kind_of = |file: &str| {
        media_rows
            .iter()
            .find(|m| m.file == file)
            .map(|m| (m.kind.clone(), m.creator.clone()))
    };
    assert_eq!(
        kind_of("sermons/Gospel in Life/the-dream-of-the-kingdom.wav"),
        Some(("sermon".into(), "Gospel in Life".into()))
    );
    assert_eq!(
        kind_of("music/Bach/air.wav").map(|k| k.0),
        Some("song".into())
    );
    let episodes = hub_rows::<Episode>(&server, "episode");
    assert_eq!(episodes.len(), 3);
    assert_eq!(
        episodes
            .iter()
            .filter(|e| e.show_id == "podcast/full-time-nix")
            .count(),
        2
    );
    let rescan = head(&server);
    server.scanner.as_ref().unwrap().rescan();
    std::thread::sleep(std::time::Duration::from_millis(1_200));
    assert_eq!(
        head(&server),
        rescan,
        "a rescan of nothing new authors nothing"
    );
}

/// A stand-in host on loopback answering every GET with what `answer`
/// says for its path: the body, or 404.
fn host(answer: impl Fn(&str) -> Option<Vec<u8>> + Send + 'static) -> String {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            let _ = reader.read_line(&mut first);
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let path = first.split_whitespace().nth(1).unwrap_or("/").to_string();
            let mut stream = stream;
            let _ = match answer(&path) {
                Some(body) => stream
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        )
                        .as_bytes(),
                    )
                    .and_then(|_| stream.write_all(&body)),
                None => stream.write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                ),
            };
        }
    });
    format!("http://{addr}")
}

/// A feed followed from a client is a show: its newest episodes in the
/// library before their files are, each queued, then downloaded into the
/// data directory and served at `/media/.feeds/`, where the log says they
/// are, its download gone; one whose file cannot be had stays, its
/// download failed and saying why, and is downloaded at a later read once
/// it can be.
#[test]
fn a_feed_followed_is_downloaded_and_a_show() {
    use harken_server::domain::{
        Download, Episode, FetchOlder, Show, Subscribe, Subscription, LIBRARY,
    };
    let rt = runtime();
    let root = tempfile::tempdir().unwrap();
    let (media, data) = (root.path().join("media"), root.path().join("data"));
    std::fs::create_dir_all(&media).unwrap();
    let audio = root.path().join("episode.wav");
    wav(&audio, 1_000);
    let bytes = std::fs::read(&audio).unwrap();
    let base = std::sync::Arc::new(std::sync::OnceLock::<String>::new());
    let served = std::sync::Arc::new(std::sync::Mutex::new(vec![
        "/1.wav".to_string(),
        "/2.wav".to_string(),
        "/3.wav".to_string(),
    ]));
    let (at, served_bytes, files) = (base.clone(), bytes.clone(), served.clone());
    let url = host(move |path| {
        let base = at.get().cloned().unwrap_or_default();
        match path {
            "/feed.xml" => Some(
                format!(
                    r#"<rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd" xmlns:psc="http://podlove.org/simple-chapters" xmlns:podcast="https://podcastindex.org/namespace/1.0"><channel>
                    <title>Gospel in Life</title><itunes:author>Tim Keller</itunes:author>
                    <item><title>Living Grace</title><guid>2</guid><pubDate>Wed, 21 Oct 2026 10:00:00 +0000</pubDate>
                      <itunes:duration>47:04</itunes:duration><enclosure url="{base}/2.wav" type="audio/wav"/>
                      <psc:chapters><psc:chapter start="0" title="Reading"/><psc:chapter start="05:00" title="Sermon"/></psc:chapters>
                      <podcast:person role="guest" href="https://redeemer.com">Tim Keller</podcast:person></item>
                    <item><title>The Dream of the Kingdom</title><guid>3</guid><pubDate>Mon, 26 Oct 2026 10:00:00 +0000</pubDate>
                      <enclosure url="{base}/3.wav" type="audio/wav"/></item>
                    <item><title>Too Old</title><guid>1</guid><pubDate>Mon, 19 Oct 2026 10:00:00 +0000</pubDate>
                      <enclosure url="{base}/1.wav" type="audio/wav"/></item>
                    <item><title>Gone</title><guid>0</guid><pubDate>Sun, 18 Oct 2026 10:00:00 +0000</pubDate>
                      <enclosure url="{base}/0.wav" type="audio/wav"/></item>
                    </channel></rss>"#
                )
                .into_bytes(),
            ),
            p if files.lock().unwrap().iter().any(|f| f == p) => Some(served_bytes.clone()),
            _ => None,
        }
    });
    base.set(url.clone()).unwrap();
    // What hears each episode: whisper.cpp on a server, and here a script
    // that says the same two lines of every file it is given.
    let transcriber = root.path().join("transcriber");
    std::fs::write(
        &transcriber,
        "#!/bin/sh\ntest -f \"$1\" || exit 1\nprintf 'WEBVTT\\n\\n00:00:01.000 --> 00:00:04.000\\n Grace and peace.\\n\\n00:00:05.000 --> 00:00:07.500\\n Let us pray.\\n' > \"$2\"\n",
    )
    .unwrap();
    std::fs::set_permissions(
        &transcriber,
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
    )
    .unwrap();
    let server = serve(&rt, &data, |c| {
        c.media = Some(media.clone());
        c.transcriber = Some(transcriber.clone());
        c.roles.insert(LIBRARY.into(), vec!["admin".into()]);
    });
    let feed = format!("{url}/feed.xml");
    let mut admin = signed_in(&server, "admin");
    admin.client.set_roles([LIBRARY]);
    admin.mutate(
        "library.subscribe",
        &Subscribe {
            kind: "sermon".into(),
            url: feed.clone(),
        },
    );
    // Downloaded: in the library, and no download of it left but the one
    // that cannot be. Its episodes are listed before their downloads are
    // queued, and a failure is said after every one is: until a failure is
    // here, an empty list is one not queued yet.
    let here = |server: &Server, admin: &mut Peer, n: usize| {
        eventually(20, || {
            admin.pump();
            let downloads = hub_rows::<Download>(server, "download");
            let songs = songs(server);
            let done = !downloads.is_empty() && downloads.iter().all(|d| d.state == "failed");
            (songs.len() >= n && done).then_some(songs)
        })
    };
    let found = here(&server, &mut admin, 4).expect("every episode listed");
    let titles: Vec<&str> = found.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(titles.len(), 4, "{found:?}");
    for t in [
        "The Dream of the Kingdom",
        "Living Grace",
        "Too Old",
        "Gone",
    ] {
        assert!(titles.contains(&t), "{t}: {found:?}");
    }
    assert!(found
        .iter()
        .all(|(_, f)| f.starts_with(".feeds/") && f.ends_with(".wav")));
    let shows = hub_rows::<Show>(&server, "show");
    assert_eq!(shows.len(), 1);
    assert_eq!(
        (
            shows[0].id.as_str(),
            shows[0].author.as_str(),
            shows[0].feed.as_str()
        ),
        ("sermon/gospel-in-life", "Tim Keller", feed.as_str())
    );
    let subscription = hub_rows::<Subscription>(&server, "subscription");
    assert_eq!(
        subscription
            .iter()
            .map(|s| (s.title.as_str(), s.items, s.keep))
            .collect::<Vec<_>>(),
        [("Gospel in Life", 4, 20)],
        "the read said on the subscription"
    );
    let episodes = hub_rows::<Episode>(&server, "episode");
    let grace = episodes.iter().find(|e| e.guid == "2").unwrap();
    assert_eq!(grace.source, format!("{url}/2.wav"));
    let chapters = hub_rows::<harken_server::domain::Chapter>(&server, "chapter");
    assert_eq!(
        chapters
            .iter()
            .map(|c| (c.media_id == grace.media_id, c.start_ms, c.title.as_str()))
            .collect::<Vec<_>>(),
        [(true, 0, "Reading"), (true, 300_000, "Sermon")],
        "the feed's chapters, on its episode"
    );
    let people = hub_rows::<harken_server::domain::Appearance>(&server, "appearance");
    assert_eq!(
        people
            .iter()
            .map(|a| (
                a.media_id == grace.media_id,
                a.person_name.as_str(),
                a.role.as_str()
            ))
            .collect::<Vec<_>>(),
        [(true, "Tim Keller", "Guest")],
        "the feed's people, on its episode"
    );
    // Heard once the file is here, from the server's own copy.
    let cues = eventually(30, || {
        let cues = hub_rows::<harken_server::domain::Cue>(&server, "cue");
        cues.iter()
            .any(|c| c.media_id == grace.media_id)
            .then_some(cues)
    })
    .expect("the episode heard");
    assert_eq!(
        cues.iter()
            .filter(|c| c.media_id == grace.media_id)
            .map(|c| (
                c.media_id == grace.media_id,
                c.start_ms,
                c.end_ms,
                c.text.as_str()
            ))
            .collect::<Vec<_>>(),
        [
            (true, 1_000, 4_000, "Grace and peace."),
            (true, 5_000, 7_500, "Let us pray.")
        ],
        "what was heard in it, on its episode"
    );
    let file = &found.iter().find(|(t, _)| t == "Living Grace").unwrap().1;
    let media_rows = hub_rows::<Media>(&server, "media");
    assert_eq!(
        media_rows
            .iter()
            .find(|m| &m.file == file)
            .unwrap()
            .duration_ms,
        2_824_000
    );
    let response = ureq::get(&format!("{}/media/{file}", server.url()))
        .call()
        .unwrap();
    let mut got = vec![];
    std::io::Read::read_to_end(&mut response.into_reader(), &mut got).unwrap();
    assert_eq!(got, bytes, "the episode is served from the server's copy");

    let failed = hub_rows::<Download>(&server, "download");
    assert_eq!(failed.len(), 1, "{failed:?}");
    let gone = media_rows_of(&server, "Gone");
    assert_eq!(
        (
            failed[0].media_id,
            failed[0].state.as_str(),
            failed[0].source.clone()
        ),
        (gone, "failed", format!("{url}/0.wav"))
    );
    assert!(failed[0].error.contains("404"), "{}", failed[0].error);

    // Its file there now, and the library read again: downloaded.
    served.lock().unwrap().push("/0.wav".into());
    server.scanner.as_ref().unwrap().rescan();
    eventually(20, || {
        admin.pump();
        hub_rows::<Download>(&server, "download")
            .is_empty()
            .then_some(())
    })
    .expect("the failed one downloaded at the next read");
    // Asked for more of a feed it has every one of: nothing to read.
    admin.mutate(
        "library.fetch_older",
        &FetchOlder {
            url: feed.clone(),
            more: 0,
        },
    );
    assert!(admin.refused.is_empty(), "{:?}", admin.refused);
    rt.block_on(server.stop());
}

/// The id of the media row titled `title`.
fn media_rows_of(server: &Server, title: &str) -> harken_server::domain::Id<Media> {
    hub_rows::<Media>(server, "media")
        .into_iter()
        .find(|m| m.title == title)
        .map(|m| m.id)
        .unwrap()
}
