//! How long the demo's reads take, printed rather than asserted: the numbers
//! behind the choices in the domain's queries and the client's views.
//! Ignored by default; run with
//! `cargo test -p harken-iced --features demo --release -- --ignored --nocapture --test-threads=1 bench`,
//! and with `BENCH_16=1` in the environment for the views at sixteen times
//! the demo as well (half a minute of seeding).

use std::time::{Duration, Instant};

use crate::domain::{AddToPlaylist, PlaylistInput, PlaylistsOf, RemoveFromPlaylist};
use crate::domain::{AlbumInput, Artist, Credits, DescribePerson, Library, LibraryEntry, RecordingInput, Recordings, WorkInput, WorkSummary, Works};
use ark::format::Encode;

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
        id: app.peer.album_id("Water Music"),
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
        let b = client.binding(name).unwrap();
        let v = crate::client::domain()
            .ask(&client.log.store, &client.ctx(), &b, input)
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
/// over: its hydrate, and then the cost of one change â a playlist toggle,
/// and a `describe_person` (what the composer and artist views read beyond
/// the songs) â each a mutation whose changes the client pushes through
/// every view held (Â§1.10), timed together, since the client pushes them in
/// the one step. Flat in the library size is the claim: a change costs the
/// entries it touches.
fn views_at(copies: usize) {
    let t0 = Instant::now();
    let mut client = crate::seed::seeded_times(copies);
    eprintln!("\n{copies}x the demo, seeded in {:.1?}", t0.elapsed());

    let favorites: Id = client.query::<Vec<crate::domain::Playlist>>("playlists", &()).unwrap()[0].id.0;
    let handel = "George Frideric Handel";
    let albums = client.query::<Vec<crate::domain::AlbumsEntry>>("albums", &()).unwrap();
    let album = AlbumInput {
        playlist_id: key(favorites),
        id: albums.into_iter().find(|a| a.name == "Water Music").unwrap().id,
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
        let v = client.view(name, input).unwrap_or_else(|e| panic!("{name}: {e}"));
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

/// One message as the runtime takes it: the update, the view built again,
/// the tree diffed, laid out and drawn â what a frame costs.
struct Frame {
    tree: iced::advanced::widget::Tree,
    renderer: iced::Renderer,
    size: iced::Size,
}

impl Frame {
    fn new(app: &crate::App) -> Frame {
        let renderer = iced::Renderer::Secondary(iced_tiny_skia::Renderer::new(iced::Font::DEFAULT, iced::Pixels(16.0)));
        let el = app.view();
        Frame {
            tree: iced::advanced::widget::Tree::new(el.as_widget()),
            renderer,
            size: iced::Size::new(1440.0, 900.0),
        }
    }

    /// `m` taken and a frame made of it: the time each part took.
    fn take(&mut self, app: &mut crate::App, m: Option<crate::Message>) -> [Duration; 4] {
        use iced::advanced::{layout, renderer, Layout};
        let t = Instant::now();
        if let Some(m) = m {
            let _ = app.update(m);
        }
        let updated = t.elapsed();
        let t = Instant::now();
        let mut el = app.view();
        let viewed = t.elapsed();
        let t = Instant::now();
        self.tree.diff(el.as_widget());
        let node = el
            .as_widget_mut()
            .layout(&mut self.tree, &self.renderer, &layout::Limits::new(iced::Size::ZERO, self.size));
        let laid = t.elapsed();
        let t = Instant::now();
        let at = iced::Point::new(600.0, 400.0);
        let viewport = iced::Rectangle::with_size(self.size);
        el.as_widget().draw(
            &self.tree,
            &mut self.renderer,
            &iced::Theme::Dark,
            &renderer::Style::default(),
            Layout::new(&node),
            iced::mouse::Cursor::Available(at),
            &viewport,
        );
        [updated, viewed, laid, t.elapsed()]
    }
}

fn row(name: &str, ts: &[[Duration; 4]]) {
    let part = |i: usize| median(&ts.iter().map(|t| t[i]).collect::<Vec<_>>());
    let total = median(&ts.iter().map(|t| t.iter().sum()).collect::<Vec<_>>());
    eprintln!(
        "{name:>22} {:>9.1?} {:>9.1?} {:>9.1?} {:>9.1?} {:>9.1?}",
        part(0),
        part(1),
        part(2),
        part(3),
        total
    );
}

/// A frame of each thing the person does on a real library â a server's
/// log at `HARKEN_BENCH_LOG`, copied in: the page opened, a row hovered,
/// the cursor moved, the clock ticked; and the pages a click on the bar
/// opens. Run with
/// `HARKEN_BENCH_LOG=<dir> cargo test -p harken-iced --release -- --ignored --nocapture bench_real`.
#[test]
#[ignore]
fn bench_real() {
    let Some(dir) = std::env::var_os("HARKEN_BENCH_LOG") else {
        return eprintln!("HARKEN_BENCH_LOG names no log");
    };
    let mut disk = ark_peer::Memory::default();
    for n in ["kv", "log", "schema.json"] {
        if let Ok(b) = std::fs::read(std::path::Path::new(&dir).join(n)) {
            ark_peer::Disk::put(&mut disk, n, &b).unwrap();
        }
    }
    let t = Instant::now();
    let replica = crate::client::open(Box::new(disk), None, crate::listening::device());
    let mut app = crate::App::with_peer(crate::peer::Peer::open(replica), String::new(), None);
    eprintln!("opened in {:.1?}: {} songs", t.elapsed(), app.peer.rows().len());
    let mut frame = Frame::new(&app);
    eprintln!("{:>22} {:>9} {:>9} {:>9} {:>9} {:>9}", "", "update", "view", "layout", "draw", "frame");
    let n = 15;
    let songs: Vec<_> = (0..n)
        .map(|_| frame.take(&mut app, Some(crate::Message::Select(crate::places::Source::Library))))
        .collect();
    row("songs opened", &songs);
    let hovers: Vec<_> = (0..n).map(|i| frame.take(&mut app, Some(crate::Message::HoverAt(i % 12)))).collect();
    row("a row hovered", &hovers);
    let moves: Vec<_> = (0..n)
        .map(|i| frame.take(&mut app, Some(crate::Message::Hover(iced::Point::new(600.0 + i as f32, 400.0)))))
        .collect();
    row("the cursor moved", &moves);
    let ticks: Vec<_> = (0..n).map(|_| frame.take(&mut app, Some(crate::Message::Tick))).collect();
    row("a tick", &ticks);
    let ids: Vec<_> = app.peer.rows().iter().map(|r| r.id).take(40).collect();
    let plays: Vec<_> = ids.iter().map(|id| frame.take(&mut app, Some(crate::Message::PlayItem(*id)))).collect();
    row("a song played", &plays);
    let playing: Vec<_> = (0..n).map(|i| frame.take(&mut app, Some(crate::Message::HoverAt(i % 12)))).collect();
    row("hovered, playing", &playing);
    let ticking: Vec<_> = (0..n).map(|_| frame.take(&mut app, Some(crate::Message::Tick))).collect();
    row("a tick, playing", &ticking);
    let idle: Vec<_> = (0..n).map(|_| frame.take(&mut app, None)).collect();
    row("a frame, playing", &idle);
    let queue: Vec<_> = (0..n)
        .map(|i| {
            frame.take(
                &mut app,
                Some(if i % 2 == 0 {
                    crate::Message::OpenQueue
                } else {
                    crate::Message::CloseQueue
                }),
            )
        })
        .collect();
    row("the queue toggled", &queue);
    let _ = frame.take(&mut app, Some(crate::Message::OpenQueue));
    let open: Vec<_> = (0..n).map(|i| frame.take(&mut app, Some(crate::Message::QueueAt(i % 10)))).collect();
    row("hovered, queue open", &open);
    let _ = frame.take(&mut app, Some(crate::Message::CloseQueue));
    let album = app.peer.rows()[0].clone();
    let detail = app.peer.detail_of(album.id);
    let to = crate::places::Source::Album(detail.album_id.clone(), detail.album.clone());
    let albums: Vec<_> = (0..n)
        .map(|i| {
            frame.take(
                &mut app,
                Some(crate::Message::Select(if i % 2 == 0 {
                    to.clone()
                } else {
                    crate::places::Source::Library
                })),
            )
        })
        .collect();
    row("songs to album", &albums);
    let longest = ark::db::read::<crate::domain::schema::Harken, _>(&app.peer.client.log.store, |db| {
        let mut n = std::collections::BTreeMap::<_, usize>::new();
        for c in db.cue.scan() {
            *n.entry(c.media_id).or_default() += 1;
        }
        n.into_iter().max_by_key(|(_, n)| *n)
    });
    if let Some((episode, lines)) = longest {
        let played = frame.take(&mut app, Some(crate::Message::PlayItem(episode.0)));
        row("an episode played", &[played]);
        eprintln!("{:>22} {lines} lines", "its transcript");
    }
    let transcript: Vec<_> = (0..n)
        .map(|i| {
            let to = if i % 2 == 0 {
                crate::places::Source::Transcript
            } else {
                crate::places::Source::Library
            };
            frame.take(&mut app, Some(crate::Message::Select(to)))
        })
        .collect();
    row("songs to transcript", &transcript);
}

/// What each wake of a window filling costs on its own thread: a fresh
/// replica, through a pair of queues, filled from a server's log at
/// `HARKEN_BENCH_LOG`, every update timed. Run as [`bench_real`] is, named
/// `bench_fill`.
#[test]
#[ignore]
fn bench_fill() {
    use ark::frames::Frame;
    use ark::peer::{Input, Output, Role};
    let Some(dir) = std::env::var_os("HARKEN_BENCH_LOG") else {
        return eprintln!("HARKEN_BENCH_LOG names no log");
    };
    let m = ark_peer::Module::of(crate::client::domain());
    let mut server = ark_peer::Instance::open_under(m.schema.clone(), Box::new(ark_peer::Files::new(&dir).unwrap()));
    server.machine.role = Role::Authority { journal: Default::default() };
    server.machine.functions = m.functions.clone();
    server.machine.later = m.later().to_vec();
    let me = super::who("max");
    let mut app = crate::App::with_peer(super::signed_in("max"), "http://127.0.0.1:9".into(), None);
    let made: std::sync::Arc<std::sync::Mutex<Vec<super::Queues>>> = Default::default();
    app.peer.client.connect_with("ws://queues", super::Queues::dialer(made.clone()));
    let _ = app.update(crate::Message::Due);
    let q = made.lock().unwrap()[0].clone();
    q.push(ark_peer::Event::Open);
    let (mut worst, mut total, mut wakes) = (Duration::ZERO, Duration::ZERO, 0);
    let t0 = Instant::now();
    for _ in 0..10_000 {
        let t = Instant::now();
        let _ = app.update(crate::Message::Wake);
        let took = t.elapsed();
        (worst, total, wakes) = (worst.max(took), total + took, wakes + 1);
        if took > Duration::from_millis(8) {
            eprintln!("wake {wakes}: {took:.1?}");
        }
        let sent: Vec<Vec<u8>> = std::mem::take(&mut *q.sent.lock().unwrap());
        if sent.is_empty() && q.said.lock().unwrap().is_empty() {
            break;
        }
        for b in sent {
            let Ok(f) = Frame::dec(&b) else { continue };
            if f.space() != ark::schema::Space::Durable || matches!(f, Frame::Telemetry { .. }) {
                continue;
            }
            for o in server.machine.step(Input::Frame {
                from: Some(me.clone()),
                bytes: b,
            }) {
                if let Output::Send(b) | Output::Fanout(b) = o {
                    q.push(ark_peer::Event::Frame(b));
                }
            }
        }
    }
    let filled = app.peer.client.log.fill.is_none() && app.peer.client.log.log().head() == server.log().head();
    eprintln!(
        "{wakes} wakes in {:.1?}: worst {worst:.1?}, busy {total:.1?}; filled {filled}, {} songs",
        t0.elapsed(),
        app.peer.rows().len()
    );
}

/// What a fold of the replica of a real library costs, where it is made:
/// the store written whole, as a device does every few dozen entries.
#[test]
#[ignore]
fn bench_fold() {
    let Some(dir) = std::env::var_os("HARKEN_BENCH_LOG") else {
        return eprintln!("HARKEN_BENCH_LOG names no log");
    };
    let tmp = tempfile_dir();
    for n in ["kv", "log", "schema.json"] {
        let _ = std::fs::copy(std::path::Path::new(&dir).join(n), tmp.join(n));
    }
    let mut replica = crate::client::open(Box::new(ark_peer::Files::new(&tmp).unwrap()), None, crate::listening::device());
    let head = replica.log.log().head().0;
    let t = Instant::now();
    let _ = replica.log.step(ark::peer::Input::Compact(head));
    eprintln!("a fold at {head}: {:.1?}", t.elapsed());
    let t = Instant::now();
    let rows = replica.log.confirmed_rows();
    eprintln!("  its rows: {:.1?}", t.elapsed());
    let t = Instant::now();
    let bytes = ark::log::Record::Rows(rows).enc();
    eprintln!("  encoded, {} bytes: {:.1?}", bytes.len(), t.elapsed());
    let _ = std::fs::remove_dir_all(&tmp);
}

fn tempfile_dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("harken-bench-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}
