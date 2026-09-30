//! What a peer costs with its storage and a server in the loop, as the
//! data grows, printed rather than asserted: the half of the performance
//! pass `rust/ark/tests/perf.rs` cannot reach, because it needs a `Peer`
//! and a hub. Over the spec's demo (`ark_client::demo`), at sizes four
//! times apart, each reported per operation — the mean, the first hundred
//! and the last hundred — so growth is a ratio on one line.
//!
//! The server is the real hub, in this process, reached through
//! `HubHandle::dial` (no socket): what is measured is the protocol, the
//! hub's thread and its persistence, not a network.
//!
//! Ignored by default; run with
//! `cargo test -p ark-server --release --test perf -- --ignored --nocapture --test-threads=1`.

use std::time::{Duration, Instant};

use ark_client::ark::store::Store;
use ark_client::{args, demo, Args, Options, Peer, Timing, Value};

fn us(d: Duration) -> f64 {
    d.as_secs_f64() * 1e6
}

fn mean(xs: &[Duration]) -> Duration {
    if xs.is_empty() {
        return Duration::ZERO;
    }
    xs.iter().sum::<Duration>() / xs.len() as u32
}

fn header(title: &str) {
    eprintln!("\n== {title}");
    eprintln!(
        "{:<48} {:>7} {:>10} {:>10} {:>10} {:>10} {:>7}",
        "path", "n", "total", "mean µs", "first100", "last100", "l/f"
    );
}

fn line(label: &str, per: &[Duration]) {
    let n = per.len();
    let k = n.min(100);
    let (f, l) = (mean(&per[..k]), mean(&per[n - k..]));
    eprintln!(
        "{:<48} {:>7} {:>9.1}ms {:>10.2} {:>10.2} {:>10.2} {:>7.2}",
        label,
        n,
        per.iter().sum::<Duration>().as_secs_f64() * 1e3,
        us(mean(per)),
        us(f),
        us(l),
        us(l) / us(f).max(1e-9)
    );
}

fn quick() -> Timing {
    Timing {
        first_backoff_ms: 5,
        max_backoff_ms: 50,
        ping_every_ms: 60_000,
        connect_timeout_ms: 2_000,
    }
}

fn playlists(p: &Peer) -> Vec<Value> {
    let mut rows = p.store().scan("playlist");
    rows.sort_by_key(|r| r["name"].clone());
    rows.into_iter().map(|r| r["id"].clone()).collect()
}

fn item(pl: &Value, i: u64) -> Args {
    args([("playlist_id", pl.clone()), ("track_id", Value::text(format!("t{i}")))])
}

/// `lists` playlists, named so that their order is their number.
fn make_lists(p: &mut Peer, lists: u64) -> Vec<Value> {
    for j in 0..lists {
        p.mutate("create_playlist", args([("name", Value::text(format!("P{j:06}")))])).unwrap();
    }
    playlists(p)
}

// (b) Against a server ---------------------------------------------------------------

/// A hub in this process, with its log kept in `data` or nowhere.
fn hub(data: Option<&std::path::Path>) -> ark_server::App {
    let mut b = ark_server::builder(demo::domain()).name("perf").trusting();
    if let Some(d) = data {
        b = b.data(d);
    }
    b.build().expect("the server builds")
}

fn pump_until(p: &mut Peer, done: impl Fn(&Peer) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !done(p) {
        p.pump();
        assert!(Instant::now() < deadline, "timed out: {:?}", p.status());
        std::hint::spin_loop();
    }
}

/// One mutation at a time, each pumped until the server has confirmed it:
/// authored, pushed, sequenced, acknowledged and confirmed here. With the
/// hub keeping its log on disk, and without.
#[test]
#[ignore]
fn perf_b_round_trip() {
    header("(b) Peer::mutate + pump until confirmed, against the in-process hub");
    for (n, keep) in [(500u64, false), (2000, false), (500, true), (2000, true)] {
        for (shape, lists) in [("one playlist", 1u64), ("playlists of 10", n / 10)] {
            // A hub of its own each time: a second peer making the same
            // playlists before it has caught up would be refused them.
            let dir = tempfile::tempdir().unwrap();
            let app = hub(keep.then(|| dir.path()));
            let mut p = Peer::open_memory(demo::domain(), Options::dev("alice").with_timing(quick())).unwrap();
            p.connect_with("local", app.hub.dial());
            pump_until(&mut p, |p| p.linked());
            let ls = make_lists(&mut p, lists);
            pump_until(&mut p, |p| p.pending_len() == 0);
            let (mut whole, mut authoring) = (vec![], vec![]);
            for i in 0..n {
                let t = Instant::now();
                p.mutate("add_to_playlist", item(&ls[(i % lists) as usize], i)).unwrap();
                let t1 = Instant::now();
                pump_until(&mut p, |p| p.pending_len() == 0);
                authoring.push(t1 - t);
                whole.push(t.elapsed());
            }
            let log = if keep { "log on disk" } else { "no log file" };
            line(&format!("{shape}, {log}: round trip"), &whole);
            line("  … Peer::mutate alone", &authoring);
            drop(p);
        }
    }
}

/// A second peer, connected throughout, while the first authors: what
/// receiving another peer's entries one at a time costs it — a `Batch` of
/// one per push, applied by intent. (With an intent of its own pending each
/// would be a rebase; `rust/ark/tests/perf.rs`'s (e) measures that.)
#[test]
#[ignore]
fn perf_b_watcher() {
    header("(b) a second peer receiving the first's entries as they land");
    for n in [500u64, 2000] {
        let app = hub(None);
        let mut author = Peer::open_memory(demo::domain(), Options::dev("alice").with_timing(quick())).unwrap();
        let mut watcher = Peer::open_memory(demo::domain(), Options::dev("bob").with_timing(quick())).unwrap();
        author.connect_with("local", app.hub.dial());
        watcher.connect_with("local", app.hub.dial());
        pump_until(&mut author, |p| p.linked());
        pump_until(&mut watcher, |p| p.linked());
        let ls = make_lists(&mut author, 10);
        pump_until(&mut author, |p| p.pending_len() == 0);
        let head = author.cursor();
        pump_until(&mut watcher, |p| p.cursor() >= head);
        let mut per = vec![];
        for i in 0..n {
            author.mutate("add_to_playlist", item(&ls[(i % 10) as usize], i)).unwrap();
            pump_until(&mut author, |p| p.pending_len() == 0);
            let want = author.cursor();
            let t = Instant::now();
            pump_until(&mut watcher, |p| p.cursor() >= want);
            let _ = watcher.take_changes();
            per.push(t.elapsed());
        }
        line("watcher, playlists of 10, nothing pending", &per);
    }
}

// (c) Persistence ---------------------------------------------------------------------

/// A peer alone authoring and then pumping, which writes what the
/// confirmed store moved by: `mutate` (the intent, and the pending record)
/// and `pump` (the journal page, or a snapshot), timed apart, over memory
/// and over a directory.
#[test]
#[ignore]
fn perf_c_pump_alone() {
    header("(c) a peer alone: mutate, then pump (persist), per mutation");
    for n in [500u64, 2000, 8000] {
        for dir in [false, true] {
            let tmp = tempfile::tempdir().unwrap();
            let mut p = if dir {
                Peer::open_path(demo::domain(), tmp.path(), Options::alone("me")).unwrap()
            } else {
                Peer::open_memory(demo::domain(), Options::alone("me")).unwrap()
            };
            let ls = make_lists(&mut p, n / 10);
            p.pump();
            let (mut m, mut pu) = (vec![], vec![]);
            for i in 0..n {
                let t = Instant::now();
                p.mutate("add_to_playlist", item(&ls[(i % (n / 10)) as usize], i)).unwrap();
                let t1 = Instant::now();
                p.pump();
                m.push(t1 - t);
                pu.push(t1.elapsed());
            }
            let st = if dir { "Dir" } else { "Memory" };
            line(&format!("playlists of 10, {st}: mutate"), &m);
            line(&format!("playlists of 10, {st}: pump"), &pu);
        }
    }
}

/// A peer with a server it cannot reach — offline, or not yet caught up,
/// or the scanner authoring a whole directory before its first pump — so
/// every intent stays pending: what `mutate` costs as the pending list
/// grows. It wrote the whole `pending` record each time; since
/// `docs/plan-perf.md` §R3 it writes one `pending.<n>` page, and the bytes
/// on the directory are the snapshot and every page.
#[test]
#[ignore]
fn perf_c_offline_pending() {
    header("(c) offline: mutate with N pending (one pending page per mutate)");
    for n in [500u64, 2000, 8000] {
        for dir in [false, true] {
            let tmp = tempfile::tempdir().unwrap();
            let mut p = if dir {
                Peer::open_path(demo::domain(), tmp.path(), Options::dev("alice")).unwrap()
            } else {
                Peer::open_memory(demo::domain(), Options::dev("alice")).unwrap()
            };
            let ls = make_lists(&mut p, n / 10);
            let per: Vec<Duration> = (0..n)
                .map(|i| {
                    let t = Instant::now();
                    p.mutate("add_to_playlist", item(&ls[(i % (n / 10)) as usize], i)).unwrap();
                    t.elapsed()
                })
                .collect();
            let bytes: u64 = if dir {
                std::fs::read_dir(tmp.path())
                    .unwrap()
                    .filter_map(|e| e.ok())
                    .filter(|e| e.file_name().to_string_lossy().starts_with("pending"))
                    .map(|e| e.metadata().map_or(0, |m| m.len()))
                    .sum()
            } else {
                0
            };
            line(
                &format!(
                    "playlists of 10, {}: mutate{}",
                    if dir { "Dir" } else { "Memory" },
                    if dir { format!(" (pending {bytes} B)") } else { String::new() }
                ),
                &per,
            );
        }
    }
}
