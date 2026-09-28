//! The scanner, against a real directory of real files, through the whole
//! server.
//!
//! What this asserts is the property the feature exists for, not that the
//! code runs: a directory of audio becomes songs in the log, a file that
//! appears later becomes one too, a new album directory is walked, a file
//! that is not audio — or is audio outside `music/` — is not a song, and a
//! rescan on a fresh scanner over the same log authors nothing at all.
//!
//! The files are written here rather than checked in. A WAV is a header and
//! some samples, so a test can make one lofty will really parse — which is
//! the part a fixture of zero bytes would not exercise.

mod common;

use ark::value::Value;
use common::*;
use harken_server::Server;
use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::tag::{Accessor, ItemKey, Tag, TagType};

/// `(title, file)` for every song the server's log holds.
fn songs(server: &Server) -> Vec<(String, String)> {
    let rows = server
        .running
        .hub
        .read_blocking(|h| h.rows("media"))
        .unwrap();
    let mut out: Vec<(String, String)> = rows
        .iter()
        .map(|r| {
            let text = |k: &str| match &r[k] {
                Value::Text(t) => t.clone(),
                other => panic!("{k}: {other:?}"),
            };
            (text("title"), text("file"))
        })
        .collect();
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

fn head(server: &Server) -> i64 {
    server
        .running
        .hub
        .read_blocking(|h| h.authority().log.head_seq())
        .unwrap()
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
    let rows = server
        .running
        .hub
        .read_blocking(|h| h.rows("media"))
        .unwrap();
    let air = rows
        .iter()
        .find(|r| r["file"] == Value::text("music/Bach/air.wav"))
        .unwrap();
    assert_eq!(
        air["duration_ms"],
        Value::int(2_000),
        "the duration is the file's, read"
    );
    assert_eq!(
        air["creator"],
        Value::text(""),
        "no artist guessed from the folder"
    );
    assert_eq!(
        air["user_id"],
        Value::text("library"),
        "authored as the library, signed in"
    );

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
    let before = head(&server);
    let scanned = server.scanner.as_ref().unwrap().authored();
    assert_eq!(scanned, 5, "one entry per song, never a second");
    let dir = root.path().join("fresh-scanner");
    let login = server
        .auth
        .issue(&ark_auth::Account {
            id: "library".into(),
            ..Default::default()
        })
        .unwrap();
    let again = harken_server::library::Scanner::start(
        media.clone(),
        domain(),
        server.running.hub.clone(),
        dir,
        login,
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1_200));
    again.rescan();
    std::thread::sleep(std::time::Duration::from_millis(800));
    assert_eq!(
        again.authored(),
        0,
        "a rescan offers nothing already a song"
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
    let url = harken_domain::listening::url(&server.url(), &found[0].1);
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
