//! How long the demo's reads take, printed rather than asserted: the numbers
//! behind the choices in `harken_domain`'s queries and the client's views.
//! Ignored by default; run with
//! `cargo test -p harken-iced --features demo --release -- --ignored --nocapture --test-threads=1 bench`,
//! and with `BENCH_16=1` in the environment for the views at sixteen times
//! the demo as well (half a minute of seeding).

use std::time::{Duration, Instant};

use ark_client::ark::store::Store;
use ark_client::{args, Args, Domain, Id, Value};
use harken_domain::view::Item;

use crate::rows::{self, Recording, Work};

#[test]
#[ignore]
fn bench_queries() {
    let t0 = Instant::now();
    let app = super::demo_app();
    eprintln!("demo seeded and first read in {:.1?}", t0.elapsed());
    let client = &app.peer.client;
    let t = |s: &str| Value::text(s);
    let reads: Vec<(&str, ark_client::Args)> = vec![
        ("library", args([("playlist_id", Value::Id([0; 16]))])),
        ("albums", args([])),
        ("artists", args([])),
        ("composers", args([])),
        ("track_details", args([])),
        ("works", args([("composer", t("Johann Sebastian Bach"))])),
        ("album", args([("playlist_id", Value::Id([0; 16])), ("name", t("Water Music"))])),
        ("playlists", args([])),
    ];
    for (name, a) in &reads {
        let started = Instant::now();
        let v = client.query(name, a).unwrap_or_else(|e| panic!("{name}: {e}"));
        let n = match &v {
            Value::List(xs) => xs.len(),
            _ => 0,
        };
        eprintln!("{name:>14}: {:>10.1?}  ({n} rows)", started.elapsed());
    }
}

/// How many times each change is made, alternating, in each of the two
/// phases below: the cost of one is the median, because a round now and then
/// pays for an allocator growing and the mean is then about that round.
const ROUNDS: u32 = 21;

/// One maintained view, and what it has cost.
struct Measured {
    name: &'static str,
    view: ark_client::View,
    hydrate: Duration,
    /// Per round: the playlist toggle and the person described, with the
    /// store warm and as it comes out of a mutation.
    toggle: Vec<Duration>,
    describe: Vec<Duration>,
    toggle_cold: Vec<Duration>,
    describe_cold: Vec<Duration>,
}

/// The middle of a set of samples.
fn median(xs: &[Duration]) -> Duration {
    let mut xs = xs.to_vec();
    xs.sort();
    xs.get(xs.len() / 2).copied().unwrap_or_default()
}

/// Every query the window holds open, over the demo library `copies` times
/// over: its hydrate, and the cost of one change pushed through
/// `View::update` — a playlist toggle, and a `describe_person` (which is what
/// the composer and artist views read beyond the songs). Flat in the library
/// size is the claim: a change costs the entries it touches.
///
/// **Measured twice, because a peer alone flushes the cache.** It writes its
/// whole replica after every mutation, which streams the library through the
/// cache, so whichever view first touches the store's rows afterwards pays
/// to fetch them again — a cost of the store's size and of this machine, not
/// of the view: pushing the same changes a second time does the same work
/// in a tenth of it. So each change is made `ROUNDS` times as it comes out
/// of the mutation (the "cold" columns: what the demo pays, in `refresh`'s
/// order, the first view in line paying for the rest), and `ROUNDS` times
/// after an untimed walk of every table (the algorithm's cost).
fn views_at(copies: usize) {
    let t0 = Instant::now();
    let domain = Domain::new(&harken_domain::module());
    let mut client = crate::seed::seeded_times(domain, copies);
    let _ = client.take_changes();
    eprintln!("\n{copies}x the demo, seeded in {:.1?}", t0.elapsed());

    let read = |c: &ark_client::Peer, name: &str, a: Args| c.query(name, &a).unwrap_or_else(|e| panic!("{name}: {e}"));
    let favorites: Id = rows::list(&read(&client, "playlists", args([])), rows::Playlist::from_value)[0].id;
    let t = |s: &str| Value::text(s);
    let handel = "George Frideric Handel";
    let album = args([("playlist_id", Value::Id(favorites)), ("name", t("Water Music"))]);
    let track = rows::list(&read(&client, "album", album.clone()), Item::from_value)
        .into_iter()
        .find(|i| !i.on_playlist())
        .expect("a track not on Favorites")
        .id;
    let work = rows::list(&read(&client, "works", args([("composer", t(handel))])), Work::from_value)
        .into_iter()
        .find(|w| w.catalogue == "HWV 348")
        .expect("Water Music is a work")
        .id;
    let recording = rows::list(&read(&client, "recordings", args([("work_id", t(&work))])), Recording::from_value)[0]
        .id
        .clone();
    // The credits page over a recording that has some: the Messiah's.
    let messiah = rows::list(&read(&client, "works", args([("composer", t(handel))])), Work::from_value)
        .into_iter()
        .find(|w| w.catalogue == "HWV 56")
        .expect("the Messiah is a work")
        .id;
    let credited = rows::list(&read(&client, "recordings", args([("work_id", t(&messiah))])), Recording::from_value)[0]
        .id
        .clone();
    let on = |key: &str, v: Value| args([("playlist_id", Value::Id(favorites)), (key, v)]);
    let queries: Vec<(&'static str, Args)> = vec![
        ("library", args([("playlist_id", Value::Id(favorites))])),
        ("playlists", args([])),
        ("albums", args([])),
        ("artists", args([])),
        ("composers", args([])),
        ("track_details", args([])),
        ("album", album),
        ("artist", on("name", t(handel))),
        ("works", args([("composer", t(handel))])),
        ("work", args([("id", t(&work))])),
        ("recordings", args([("work_id", t(&work))])),
        ("credits", args([("recording_id", t(&credited))])),
        ("recording", on("id", t(&recording))),
        ("playlist", args([("playlist_id", Value::Id(favorites))])),
        ("playlists_of", args([("media_id", Value::Id(track))])),
    ];
    let mut held: Vec<Measured> = queries
        .into_iter()
        .map(|(name, a)| {
            let started = Instant::now();
            let view = client.view(name, a).unwrap_or_else(|e| panic!("{name}: {e}"));
            Measured {
                name,
                view,
                hydrate: started.elapsed(),
                toggle: Vec::new(),
                describe: Vec::new(),
                toggle_cold: Vec::new(),
                describe_cold: Vec::new(),
            }
        })
        .collect();

    // Hand one settle's changes to every view in turn, as `Peer::refresh`
    // does, timing each.
    let push = |client: &mut ark_client::Peer, held: &mut Vec<Measured>, toggle: bool, warm: bool| {
        let changes = client.take_changes();
        if warm {
            for tbl in client.schema().tables() {
                let _ = client.store().scan(&tbl.name);
            }
        }
        for m in held.iter_mut() {
            let started = Instant::now();
            m.view.update(client, &changes).unwrap_or_else(|e| panic!("{}: {e}", m.name));
            let took = started.elapsed();
            match (toggle, warm) {
                (true, true) => m.toggle.push(took),
                (false, true) => m.describe.push(took),
                (true, false) => m.toggle_cold.push(took),
                (false, false) => m.describe_cold.push(took),
            }
        }
    };
    let pair = args([("playlist_id", Value::Id(favorites)), ("media_id", Value::Id(track))]);
    for round in 0..ROUNDS * 2 {
        let warm = round >= ROUNDS;
        let verb = match round % 2 {
            0 => "add_to_playlist",
            _ => "remove_from_playlist",
        };
        client.mutate(verb, pair.clone()).unwrap();
        push(&mut client, &mut held, true, warm);
        let art = format!("handel-{round}.jpg");
        client
            .mutate(
                "describe_person",
                args([
                    ("name", t(handel)),
                    ("sort_name", t("")),
                    ("born", Value::Int(0)),
                    ("died", Value::Int(0)),
                    ("art", t(&art)),
                ]),
            )
            .unwrap();
        push(&mut client, &mut held, false, warm);
    }

    eprintln!(
        "{:>14} {:>6} {:>10} {:>10} {:>10} {:>12} {:>12}",
        "query", "rows", "hydrate", "toggle", "describe", "cold toggle", "cold describe"
    );
    for m in &held {
        eprintln!(
            "{:>14} {:>6} {:>10.1?} {:>10.1?} {:>10.1?} {:>12.1?} {:>12.1?}",
            m.name,
            m.view.rows().len(),
            m.hydrate,
            median(&m.toggle),
            median(&m.describe),
            median(&m.toggle_cold),
            median(&m.describe_cold)
        );
    }
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
