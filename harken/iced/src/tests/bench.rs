//! How long the demo's reads take, printed rather than asserted: the numbers
//! behind the choices in the domain's queries and the client's views.
//! Ignored by default; run with
//! `cargo test -p harken-iced --features demo --release -- --ignored --nocapture --test-threads=1 bench`,
//! and with `BENCH_16=1` in the environment for the views at sixteen times
//! the demo as well (half a minute of seeding).

use std::time::{Duration, Instant};

use crate::domain::{AddToPlaylist, PlaylistInput, PlaylistsOf, RemoveFromPlaylist};
use crate::domain::{AlbumInput, Artist, Credits, DescribePerson, Library, LibraryEntry, RecordingInput, Recordings, WorkInput, WorkSummary, Works};
use ark::ir::Node;

use crate::peer::key;
use crate::rows::Id;

#[test]
#[ignore]
fn bench_queries() {
    let t0 = Instant::now();
    let app = super::demo_app();
    eprintln!("demo seeded and first read in {:.1?}", t0.elapsed());
    let client = &app.peer.client;
    let nobody = Library { playlist_id: key([0; 16]) };
    let album = AlbumInput {
        playlist_id: key([0; 16]),
        name: "Water Music".into(),
    };
    let bach = Works {
        composer: "Johann Sebastian Bach".into(),
    };
    let reads: Vec<(&str, Vec<u8>)> = vec![
        ("library", nobody.to_bytes()),
        ("albums", vec![]),
        ("artists", vec![]),
        ("composers", vec![]),
        ("track_details", vec![]),
        ("works", bach.to_bytes()),
        ("album", album.to_bytes()),
        ("playlists", vec![]),
    ];
    for (name, input) in &reads {
        let started = Instant::now();
        let b = crate::client::binding(name).unwrap();
        let v = crate::client::domain()
            .ask(&client.client.log.store, &client.ctx(), &b, input)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        eprintln!("{name:>14}: {:>10.1?}  ({} bytes)", started.elapsed(), v.len());
    }
}

/// How many times each change is made, alternating: the cost of one is the
/// median, because a round now and then pays for an allocator growing.
const ROUNDS: u32 = 21;

/// The middle of a set of samples.
fn median(xs: &[Duration]) -> Duration {
    let mut xs = xs.to_vec();
    xs.sort();
    xs.get(xs.len() / 2).copied().unwrap_or_default()
}

/// Every query the window holds open, over the demo library `copies` times
/// over: its hydrate, and then the cost of one change — a playlist toggle,
/// and a `describe_person` (what the composer and artist views read beyond
/// the songs) — each a mutation whose changes the client pushes through
/// every view held (§1.10), timed together, since the client pushes them in
/// the one step. Flat in the library size is the claim: a change costs the
/// entries it touches.
fn views_at(copies: usize) {
    let t0 = Instant::now();
    let mut client = crate::seed::seeded_times(copies);
    eprintln!("\n{copies}x the demo, seeded in {:.1?}", t0.elapsed());

    let favorites: Id = client.query::<Vec<crate::domain::Playlist>>("playlists", &()).unwrap()[0].id.0;
    let handel = "George Frideric Handel";
    let album = AlbumInput {
        playlist_id: key(favorites),
        name: "Water Music".into(),
    };
    let track = client
        .query::<Vec<LibraryEntry>>("album", &album)
        .unwrap()
        .into_iter()
        .find(|i| i.playlist_pos.is_none())
        .expect("a track not on Favorites")
        .id
        .0;
    let works = Works { composer: handel.into() };
    let work = |cat: &str| {
        client
            .query::<Vec<WorkSummary>>("works", &works)
            .unwrap()
            .into_iter()
            .find(|w| w.catalogue == cat)
            .unwrap_or_else(|| panic!("{cat} is a work"))
            .id
    };
    let (water, messiah) = (work("HWV 348"), work("HWV 56"));
    let first_take = |w: &str| {
        client
            .query::<Vec<crate::domain::RecordingsEntry>>("recordings", &Recordings { work_id: w.into() })
            .unwrap()[0]
            .id
            .clone()
    };
    let (recording, credited) = (first_take(&water), first_take(&messiah));
    let queries: Vec<(&'static str, Vec<u8>)> = vec![
        ("library", Library { playlist_id: key(favorites) }.to_bytes()),
        ("playlists", vec![]),
        ("albums", vec![]),
        ("artists", vec![]),
        ("composers", vec![]),
        ("track_details", vec![]),
        ("album", album.to_bytes()),
        (
            "artist",
            Artist {
                playlist_id: key(favorites),
                name: handel.into(),
            }
            .to_bytes(),
        ),
        ("works", works.to_bytes()),
        ("work", WorkInput { id: water.clone() }.to_bytes()),
        ("recordings", Recordings { work_id: water }.to_bytes()),
        ("credits", Credits { recording_id: credited }.to_bytes()),
        (
            "recording",
            RecordingInput {
                playlist_id: key(favorites),
                id: recording,
            }
            .to_bytes(),
        ),
        ("playlist", PlaylistInput { playlist_id: key(favorites) }.to_bytes()),
        ("playlists_of", PlaylistsOf { media_id: key(track) }.to_bytes()),
    ];
    eprintln!("{:>14} {:>6} {:>10}", "query", "rows", "hydrate");
    for (name, input) in &queries {
        let started = Instant::now();
        let v = client.watch(name, input).unwrap_or_else(|e| panic!("{name}: {e}"));
        let took = started.elapsed();
        eprintln!("{name:>14} {:>6} {took:>10.1?}", client.rows(v).len());
    }
    let (mut toggle, mut describe) = (vec![], vec![]);
    for round in 0..ROUNDS * 2 {
        let started = Instant::now();
        match round % 2 {
            0 => client.mutate(
                "add_to_playlist",
                &AddToPlaylist {
                    playlist_id: key(favorites),
                    media_id: key(track),
                },
            ),
            _ => client.mutate(
                "remove_from_playlist",
                &RemoveFromPlaylist {
                    playlist_id: key(favorites),
                    media_id: key(track),
                },
            ),
        }
        .unwrap();
        toggle.push(started.elapsed());
        let person = DescribePerson {
            name: handel.into(),
            art: format!("handel-{round}.jpg"),
            ..DescribePerson::default()
        };
        let started = Instant::now();
        client.mutate("describe_person", &person).unwrap();
        describe.push(started.elapsed());
    }
    eprintln!(
        "a toggle, through {} views: {:.1?}; a person described: {:.1?}",
        queries.len(),
        median(&toggle),
        median(&describe)
    );
}

#[test]
#[ignore]
fn bench_views() {
    views_at(1);
    views_at(4);
    if std::env::var_os("BENCH_16").is_some() {
        views_at(16);
    }
}
