//! What a v5 client costs with its storage and the hub in the loop, as
//! the data grows, printed rather than asserted: `rust/ark-server/tests/
//! perf.rs`'s shapes (`docs/plan-v5.md` Part 3), the same sizes, columns
//! and labels, so the two outputs diff. Over the demo of `ark5-dev`
//! (`demo.create_playlist`, `demo.add_to_playlist`).
//!
//! The hub is the real one on its thread, reached in this process through
//! `Handle::dial` (no socket), and the client is `ark5_client::Client`
//! driven by its `Link`, as an app drives it — so the link thread's
//! polling is in the round trip; a second line drives the in-process
//! connection directly, without the link, to say how much. Then the
//! storage shapes over harken's `media` table as `harken/domain5`
//! declares it (key `id`, indexes `file` and `pos`; written out here as a
//! schema, since this crate does not depend on harken): live bytes per
//! row by component, open time and resident memory, and a snapshot frame
//! adopted — each in a process of its own.
//!
//! Ignored by default; run with
//! `cargo test -p ark5-server --release --test perf -- --ignored --nocapture --test-threads=1`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ark5::codec::{bytes_enc, put_bytes, put_int};
use ark5::frames::Frame;
use ark5::hash::{EntryHash, FnHash};
use ark5::ir::Node;
use ark5::log::{Entry, EntryId, Logged, Record, Seq, Snapshot};
use ark5::module::Lib;
use ark5::peer::{Cas, Who};
use ark5::schema::{Schema, Space, Table};
use ark5::store::{Change, Store};
use ark5::ty::Ty;
use ark5_client::link::{Backoff, Conn, Dial, Link, Recv};
use ark5_client::{Client, Config, Disk, Files, Memory, Module};
use ark5_dev::demo::{domain, module};
use ark5_server::handle::now;
use ark5_server::{builder, dev, Hub};

// Live bytes, counted, process-wide (v4's allocator, as it is) -----------------

struct Live;

static LIVE: AtomicI64 = AtomicI64::new(0);
static PEAK: AtomicI64 = AtomicI64::new(0);

fn grew(by: i64) {
    let now = LIVE.fetch_add(by, Ordering::Relaxed) + by;
    if now > PEAK.load(Ordering::Relaxed) {
        PEAK.fetch_max(now, Ordering::Relaxed);
    }
}

unsafe impl GlobalAlloc for Live {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        grew(l.size() as i64);
        System.alloc(l)
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        grew(l.size() as i64);
        System.alloc_zeroed(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size() as i64, Ordering::Relaxed);
        System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        grew(n as i64 - l.size() as i64);
        System.realloc(p, l, n)
    }
}

#[global_allocator]
static GLOBAL: Live = Live;

fn live_bytes() -> i64 {
    LIVE.load(Ordering::Relaxed)
}

fn peak_bytes() -> i64 {
    PEAK.load(Ordering::Relaxed)
}

fn reset_peak() {
    PEAK.store(live_bytes(), Ordering::Relaxed);
}

// Reporting ----------------------------------------------------------------------

fn us(d: Duration) -> f64 {
    d.as_secs_f64() * 1e6
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
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

fn note(s: &str) {
    eprintln!("   v5: {s}");
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

fn na(label: &str, n: usize, why: &str) {
    eprintln!("{:<48} {:>7} {:>10} n/a: {why}", label, n, "");
}

// The demo, from a client --------------------------------------------------------

fn config() -> Config {
    Config {
        log: module(&domain()),
        live: None,
    }
}

fn client(token: &str, disk: Box<dyn Disk>) -> Client {
    Client::open(config(), dev(token).unwrap(), disk, false)
}

/// `demo.create_playlist` and `demo.add_to_playlist`, by hash: the domain
/// assembled once, not per mutation.
fn fns() -> (FnHash, FnHash) {
    static F: std::sync::OnceLock<(FnHash, FnHash)> = std::sync::OnceLock::new();
    *F.get_or_init(|| {
        let d = domain();
        (d.get("demo.create_playlist"), d.get("demo.add_to_playlist"))
    })
}

fn create(c: &mut Client, j: u64) {
    c.mutate(fns().0, bytes_enc(format!("P{j:06}").as_bytes()), now());
}

/// The playlist the session's `local`-th entry made.
fn pid(session: &str, local: u64) -> [u8; 16] {
    EntryId::of(session, local as i64).draw(0)
}

fn add_input(pl: &[u8; 16], i: u64) -> Vec<u8> {
    let mut b = bytes_enc(pl);
    put_bytes(&mut b, format!("t{i}").as_bytes());
    b
}

fn add(c: &mut Client, pl: &[u8; 16], i: u64) {
    c.mutate(fns().1, add_input(pl, i), now());
}

fn fast() -> Backoff {
    Backoff {
        first: Duration::from_millis(5),
        most: Duration::from_millis(50),
    }
}

/// Pumps until `done`, or panics after a minute.
fn pump_until(c: &mut Client, link: &Link, done: impl Fn(&Client) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !done(c) {
        let ev = link.wait(Duration::from_millis(1));
        c.pump(link, ev, now());
        assert!(Instant::now() < deadline, "timed out");
    }
}

/// The same, over a connection with no link thread: the frames the client
/// has are sent, and what comes is received, here.
fn drive_until(c: &mut Client, conn: &mut Box<dyn Conn>, done: impl Fn(&Client) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        for f in c.take_frames() {
            conn.send(f);
        }
        if done(c) {
            return;
        }
        if let Recv::Frame(b) = conn.recv(Duration::from_millis(1)) {
            c.receive(b);
        }
        assert!(Instant::now() < deadline, "timed out");
    }
}

fn hub_app(data: Option<&Path>) -> ark5_server::App {
    let mut b = builder(ark5_server::Config::new(module(&domain())));
    if let Some(d) = data {
        b = b.data(d);
    }
    b.build().expect("the server builds")
}

// (b) Against a server --------------------------------------------------------------

/// One mutation at a time, each pumped until confirmed: authored, pushed,
/// sequenced, written (with the log on disk, synced), acknowledged and
/// confirmed here; with the hub keeping its log on disk, and without.
#[test]
#[ignore]
fn perf_b_round_trip() {
    header("(b) Peer::mutate + pump until confirmed, against the in-process hub");
    note("Client::mutate + Client::pump through its Link (frames sent from this thread; the link thread receives);");
    note("`no link thread` is the same over the in-process Conn, sent and received here");
    for (n, keep) in [(500u64, false), (2000, false), (500, true), (2000, true)] {
        for (shape, lists) in [("one playlist", 1u64), ("playlists of 10", n / 10)] {
            let dir = tempfile::tempdir().unwrap();
            let app = hub_app(keep.then(|| dir.path()));
            let token = "alice@perf";
            let mut c = client(token, Box::new(Memory::default()));
            let link = Link::start(Box::new(app.hub.dial(Arc::new(dev))), token.into(), fast());
            pump_until(&mut c, &link, |c| c.linked);
            for j in 0..lists {
                create(&mut c, j);
            }
            c.pump(&link, vec![], now());
            pump_until(&mut c, &link, |c| c.log.pending.is_empty() && c.log.log.head().0 >= lists as Seq);
            let ls: Vec<[u8; 16]> = (1..=lists).map(|j| pid(token, j)).collect();
            let (mut whole, mut authoring) = (vec![], vec![]);
            for i in 0..n {
                let t = Instant::now();
                add(&mut c, &ls[(i % lists) as usize], i);
                c.pump(&link, vec![], now());
                let t1 = Instant::now();
                pump_until(&mut c, &link, |c| c.log.pending.is_empty());
                authoring.push(t1 - t);
                whole.push(t.elapsed());
            }
            drop(link);
            // The same again, a second session over a Conn of its own.
            let token2 = "alice@direct";
            let mut d = client(token2, Box::new(Memory::default()));
            let mut conn = app.hub.dial(Arc::new(dev)).dial(token2).unwrap();
            d.connected();
            drive_until(&mut d, &mut conn, |d| d.log.log.head() == c.log.log.head());
            let direct: Vec<Duration> = (0..n)
                .map(|i| {
                    let t = Instant::now();
                    add(&mut d, &ls[(i % lists) as usize], 1_000_000 + i);
                    drive_until(&mut d, &mut conn, |d| d.log.pending.is_empty());
                    t.elapsed()
                })
                .collect();
            let log = if keep { "log on disk" } else { "no log file" };
            line(&format!("{shape}, {log}: round trip"), &whole);
            line("  … Peer::mutate alone", &authoring);
            line("  … round trip, no link thread", &direct);
        }
    }
}

/// A second client, linked throughout, while the first authors: what
/// receiving another's entries one at a time costs it — a `Batch` of one
/// per push, applied by its facts.
#[test]
#[ignore]
fn perf_b_watcher() {
    header("(b) a second peer receiving the first's entries as they land");
    for n in [500u64, 2000] {
        let app = hub_app(None);
        let (ta, tb) = ("alice@perf", "bob@perf");
        let mut author = client(ta, Box::new(Memory::default()));
        let mut watcher = client(tb, Box::new(Memory::default()));
        let la = Link::start(Box::new(app.hub.dial(Arc::new(dev))), ta.into(), fast());
        let lb = Link::start(Box::new(app.hub.dial(Arc::new(dev))), tb.into(), fast());
        pump_until(&mut author, &la, |c| c.linked);
        pump_until(&mut watcher, &lb, |c| c.linked);
        for j in 0..10 {
            create(&mut author, j);
        }
        author.pump(&la, vec![], now());
        pump_until(&mut author, &la, |c| c.log.pending.is_empty() && c.log.log.head().0 >= 10);
        let head = author.log.log.head().0;
        pump_until(&mut watcher, &lb, |c| c.log.log.head().0 >= head);
        let ls: Vec<[u8; 16]> = (1..=10).map(|j| pid(ta, j)).collect();
        let mut per = vec![];
        for i in 0..n {
            add(&mut author, &ls[(i % 10) as usize], i);
            author.pump(&la, vec![], now());
            pump_until(&mut author, &la, |c| c.log.pending.is_empty());
            let want = author.log.log.head().0;
            let t = Instant::now();
            pump_until(&mut watcher, &lb, |c| c.log.log.head().0 >= want);
            let _ = watcher.take_events();
            per.push(t.elapsed());
        }
        line("watcher, playlists of 10, nothing pending", &per);
    }
}

// (c) Persistence ---------------------------------------------------------------

fn disk_of(dir: Option<&Path>) -> Box<dyn Disk> {
    match dir {
        Some(d) => Box::new(Files::new(d).unwrap()),
        None => Box::new(Memory::default()),
    }
}

/// A client alone authoring: `mutate` is the authority's step, and what it
/// makes durable — the logged entry appended to `chain`, and the `pending`
/// blob (its last `local`) rewritten — is written inside it. There is no
/// pump to persist with.
#[test]
#[ignore]
fn perf_c_pump_alone() {
    header("(c) a peer alone: mutate, then pump (persist), per mutation");
    note("Client::alone, then Client::mutate: the record and the `pending` blob are written in the step itself");
    for n in [500u64, 2000, 8000] {
        for dir in [false, true] {
            let tmp = tempfile::tempdir().unwrap();
            let token = "me@local";
            let mut c = client(token, disk_of(dir.then(|| tmp.path())));
            c.alone();
            let lists = n / 10;
            for j in 0..lists {
                create(&mut c, j);
            }
            let ls: Vec<[u8; 16]> = (1..=lists).map(|j| pid(token, j)).collect();
            let per: Vec<Duration> = (0..n)
                .map(|i| {
                    let t = Instant::now();
                    add(&mut c, &ls[(i % lists) as usize], i);
                    t.elapsed()
                })
                .collect();
            assert_eq!(c.log.log.head().0 as u64, n + lists);
            let st = if dir { "Dir" } else { "Memory" };
            line(&format!("playlists of 10, {st}: mutate"), &per);
            na(&format!("playlists of 10, {st}: pump"), n as usize, "persisted inside mutate's step");
        }
    }
}

/// A client with a server it cannot reach: every intent stays pending, and
/// each `mutate` rewrites the whole `pending` blob (its intents not yet
/// confirmed) — through a temporary file synced and renamed, on `Files`.
/// The bytes are that blob's at the end.
#[test]
#[ignore]
fn perf_c_offline_pending() {
    header("(c) offline: mutate with N pending (one pending page per mutate)");
    note("the `pending` journal: a record appended per mutate (its one entry), written whole when it holds twice what is pending");
    for n in [500u64, 2000, 8000] {
        for dir in [false, true] {
            let tmp = tempfile::tempdir().unwrap();
            let token = "alice@perf";
            let mut c = client(token, disk_of(dir.then(|| tmp.path())));
            let lists = n / 10;
            for j in 0..lists {
                create(&mut c, j);
            }
            let ls: Vec<[u8; 16]> = (1..=lists).map(|j| pid(token, j)).collect();
            let per: Vec<Duration> = (0..n)
                .map(|i| {
                    let t = Instant::now();
                    add(&mut c, &ls[(i % lists) as usize], i);
                    t.elapsed()
                })
                .collect();
            assert_eq!(c.log.pending.len() as u64, n + lists);
            let bytes = if dir {
                std::fs::metadata(tmp.path().join("pending")).map_or(0, |m| m.len())
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

// (g) through the hub: the fan-out as the hub routes it ------------------------------

/// `rust/ark5/tests/perf.rs`'s fan-out, through the real `Hub` (sans-io, on
/// this thread): C connections that said `Hello` at the head, one pushing
/// a single entry. `Hub::frame` is the authority's step and the routing —
/// a copy of the one `Batch` per connection fed whole.
#[test]
#[ignore]
fn perf_g_hub_fanout() {
    eprintln!("\n== (g) fan-out of one pushed entry to C connections, through Hub::frame (v5 only)");
    eprintln!("{:<48} {:>7} {:>10} {:>10}", "shape", "C", "frame µs", "per conn");
    let d = domain();
    let (cr, ad) = (d.get("demo.create_playlist"), d.get("demo.add_to_playlist"));
    for c in [10usize, 40, 160] {
        for s in [500u64, 8000] {
            let mut hub = Hub::open(ark5_server::Config::new(module(&d)), Box::new(Memory::default()), 0);
            let author = dev("alice@dev").unwrap();
            let a = hub.connect(author.clone());
            let mut local = 0;
            for j in 0..10u64 {
                push(&mut hub, a, &author, &mut local, cr, bytes_enc(format!("P{j}").as_bytes()));
            }
            let ls: Vec<[u8; 16]> = (1..=10).map(|j| pid("alice@dev", j)).collect();
            for i in 0..s {
                push(&mut hub, a, &author, &mut local, ad, add_input(&ls[(i % 10) as usize], i));
            }
            let (head, hash) = hub.log.log.head();
            let hello = Frame::Hello {
                space: Space::Durable,
                cursor: head,
                hash: Some(hash),
                device: None,
            }
            .enc();
            let conns: Vec<u64> = (0..c).map(|k| hub.connect(dev(&format!("u{k}@x")).unwrap())).collect();
            for k in &conns {
                hub.frame(*k, hello.clone(), 0);
            }
            let w = dev("u0@x").unwrap();
            let mut l0 = 0;
            let reps = 20u64;
            let mut spent = Duration::ZERO;
            let mut bytes = 0;
            for i in 0..reps {
                let t = Instant::now();
                let out = push(&mut hub, conns[0], &w, &mut l0, ad, add_input(&ls[(i % 10) as usize], 600_000 + i));
                spent += t.elapsed();
                bytes += out.iter().map(|(_, b)| b.len()).sum::<usize>();
                // the author's connection is fed whole too, and said no Hello: it is sent the batch as well
                assert!(out.len() > c, "an Ack and a Batch to each");
            }
            let per = spent / reps as u32;
            eprintln!(
                "{:<48} {:>7} {:>10.1} {:>10.2}",
                format!("log of {s}; {} B per push", bytes / reps as usize),
                c,
                us(per),
                us(per) / c as f64
            );
        }
    }
}

/// A `Push` of the session's next entry, through the hub.
fn push(hub: &mut Hub, conn: u64, w: &Who, local: &mut i64, func: FnHash, input: Vec<u8>) -> Vec<(u64, Vec<u8>)> {
    *local += 1;
    let entry = Entry {
        id: EntryId::of(&w.session, *local),
        actor: w.actor.clone(),
        session: w.session.clone(),
        local: *local,
        roles: vec![],
        func,
        input,
        now: *local,
    };
    let f = Frame::Push {
        space: Space::Durable,
        entry,
        body: None,
    };
    hub.frame(conn, f.enc(), 0)
}

// (d) Storage over harken's media rows -----------------------------------------------

/// harken's `media` table as `harken/domain5/src/schema.rs` declares it,
/// with its two indexes or without.
fn media_schema(indexes: bool) -> Schema {
    let mut t = Table::new("media")
        .col("id", "Id<Media>", Ty::Bytes)
        .col("kind", "Text", Ty::Bytes)
        .col("title", "Text", Ty::Bytes)
        .col("creator", "Text", Ty::Bytes)
        .col("duration_ms", "Int", Ty::Int)
        .col("file", "Text", Ty::Bytes)
        .col("pos", "Int", Ty::Int)
        .col("added_ms", "Int", Ty::Int)
        .col("user_id", "Text", Ty::Bytes);
    if indexes {
        t = t.index(&["file"], false).index(&["pos"], false);
    }
    Schema { tables: vec![t] }
}

fn open_id(tag: u8, n: u64) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[0] = tag;
    b[8..].copy_from_slice(&n.to_be_bytes());
    b
}

/// A media row as `add_song` writes one: the nine columns of v4's.
fn media(i: u64) -> Vec<u8> {
    let mut b = bytes_enc(&open_id(1, i));
    put_bytes(&mut b, b"song");
    put_bytes(&mut b, format!("Track {i}").as_bytes());
    put_bytes(&mut b, format!("Artist {}", i % 50).as_bytes());
    put_int(&mut b, 180_000);
    put_bytes(&mut b, format!("music/a{}/t{i}.flac", i % 50).as_bytes());
    put_int(&mut b, i as i64 + 1);
    put_int(&mut b, 1_000 + i as i64);
    put_bytes(&mut b, b"library");
    b
}

fn media_store(schema: &Schema, rows: std::ops::Range<u64>) -> Store {
    Store::from_rows(Arc::new(schema.clone()), &[("media".into(), rows.map(media).collect())]).unwrap()
}

fn media_module() -> Module {
    Module {
        schema: Arc::new(media_schema(true)),
        cas: Cas::default(),
        lib: Lib::default(),
    }
}

fn snapshot(store: &Store, seq: Seq, hash: EntryHash) -> Snapshot {
    Snapshot {
        seq,
        hash,
        state: store.state_hash(),
        rows: store.snapshot_rows(),
    }
}

/// The chain that adds rows `from..to` to `store`, one `add_song` each,
/// linked from `(seq, hash)`.
fn chain(store: &mut Store, (mut seq, mut prev): (Seq, EntryHash), rows: std::ops::Range<u64>, session: &str) -> Vec<Logged> {
    let func = FnHash::of(b"library.add_song");
    rows.map(|i| {
        let facts = vec![Change::Add {
            table: "media".into(),
            row: media(i),
        }];
        store.apply_all(&facts).unwrap();
        seq += 1;
        let local = seq;
        let entry = Entry {
            id: EntryId::of(session, local),
            actor: "library".into(),
            session: session.into(),
            local,
            roles: vec!["library".into()],
            func,
            input: vec![],
            now: 1_000 + i as i64,
        };
        let state = store.state_hash();
        let hash = Logged::chain(&prev, seq, &entry, &facts, &state);
        prev = hash;
        Logged {
            seq,
            entry,
            facts,
            state,
            hash,
        }
    })
    .collect()
}

fn record(disk: &mut Files, r: Record) {
    disk.record(&r.enc()).unwrap();
}

/// A client of a server, caught up: a snapshot of `n` rows at seq `n`,
/// no chain, nothing pending.
fn write_client(dir: &Path, n: u64) {
    let st = media_store(&media_schema(true), 0..n);
    let mut f = Files::new(dir).unwrap();
    record(&mut f, Record::Snapshot(snapshot(&st, n as Seq, EntryHash([0x5a; 32]))));
}

/// A hub's log: a snapshot at the horizon `n - n/10` and a chain of the
/// last tenth, each entry an `add_song` adding one row.
fn write_server(dir: &Path, n: u64) {
    let horizon = n - n / 10;
    let mut st = media_store(&media_schema(true), 0..horizon);
    let base = EntryHash([0x5a; 32]);
    let mut f = Files::new(dir.join("log")).unwrap();
    record(&mut f, Record::Snapshot(snapshot(&st, horizon as Seq, base)));
    for l in chain(&mut st, (horizon as Seq, base), horizon..n, "scan") {
        record(&mut f, Record::Logged(l));
    }
}

/// A client alone that has authored `n` songs: its fork (the empty log)
/// and the chain of `n` from it.
fn write_alone(dir: &Path, n: u64) {
    let mut st = Store::new(Arc::new(media_schema(true)));
    let mut f = Files::new(dir).unwrap();
    f.put("fork", &(0i64, EntryHash::default()).to_bytes()).unwrap();
    for l in chain(&mut st, (0, EntryHash::default()), 0..n, "alice") {
        record(&mut f, Record::Logged(l));
    }
}

fn vm_rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("VmRSS:").and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok()))
        })
        .unwrap_or(0)
}

fn dir_bytes(dir: &Path) -> u64 {
    walk(dir)
}

fn walk(p: &Path) -> u64 {
    std::fs::read_dir(p)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| {
                    let m = e.metadata().unwrap();
                    if m.is_dir() {
                        walk(&e.path())
                    } else {
                        m.len()
                    }
                })
                .sum()
        })
        .unwrap_or(0)
}

fn child(test: &str, var: &str, val: String) -> String {
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([test, "--exact", "--ignored", "--nocapture", "--test-threads=1"])
        .env(var, val)
        .output()
        .expect("the child runs");
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

const OPEN_SIZES: [u64; 3] = [10_000, 100_000, 400_000];

/// What opening costs, in time and resident memory, at 10,000, 100,000 and
/// 400,000 rows: a store alone (the snapshot file read, decoded and built),
/// a client of a server from its snapshot (`Client::open` over `Files`), a
/// hub from its log's snapshot at the horizon and the chain above it
/// (`Hub::open`: every entry's facts applied and its state hash checked),
/// and a client alone from its fork and its chain of `n`. Each open in a
/// process of its own ([`perf_d_open_child`]). The split is the snapshot
/// file read and decoded, and the chain read; "the rest" is building and
/// checking.
#[test]
#[ignore]
fn perf_d_open() {
    let root = std::env::var_os("ARK_PERF_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    eprintln!("\n== (d) open time and resident memory (docs/plan-db.md D5), harken's schema, media rows");
    note("harken's media table (key id; indexes file, pos) as a schema of its own; v4's opened harken.ark whole");
    eprintln!(
        "{:<8} {:>8} {:>9} {:>10} {:>9} {:>9} {:>9}   split (ms)",
        "what", "rows", "on disk", "open ms", "rss MB", "base MB", "live MB"
    );
    for n in OPEN_SIZES {
        for kind in ["store", "client", "server", "alone"] {
            let dir = tempfile::Builder::new().prefix("ark5-open-").tempdir_in(&root).unwrap();
            match kind {
                "client" | "store" => write_client(dir.path(), n),
                "server" => write_server(dir.path(), n),
                _ => write_alone(dir.path(), n),
            }
            let bytes = dir_bytes(dir.path());
            let text = child("perf_d_open_child", "ARK5_OPEN", format!("{kind} {n} {}", dir.path().display()));
            let line = text
                .lines()
                .find_map(|l| l.split_once("OPEN ").map(|(_, m)| m))
                .unwrap_or_else(|| panic!("{kind} {n}: no measurement:\n{text}"));
            let f: Vec<&str> = line.splitn(5, ' ').collect();
            eprintln!(
                "{kind:<8} {n:>8} {:>7.1}MB {:>10} {:>9} {:>9} {:>9}   {}",
                bytes as f64 / 1e6,
                f[0],
                f[1],
                f[2],
                f[3],
                f.get(4).unwrap_or(&"")
            );
        }
    }
}

/// One open, in a process of its own. Prints `OPEN <ms> <rss MB>
/// <baseline MB> <live MB> <split…>`. Does nothing unless `ARK5_OPEN`
/// says what to open.
#[test]
#[ignore]
fn perf_d_open_child() {
    let Ok(spec) = std::env::var("ARK5_OPEN") else { return };
    let mut it = spec.splitn(3, ' ');
    let (kind, n, dir) = (
        it.next().unwrap(),
        it.next().unwrap().parse::<u64>().unwrap(),
        std::path::PathBuf::from(it.next().unwrap()),
    );
    let mb = |kb: u64| format!("{:.1}", kb as f64 / 1024.0);
    let base = vm_rss_kb();
    let live0 = live_bytes();
    let who = Who {
        actor: "alice".into(),
        session: "alice".into(),
        roles: vec![],
    };
    let t = Instant::now();
    let rows = match kind {
        "store" => {
            let b = std::fs::read(dir.join("snapshot")).unwrap();
            let Record::Snapshot(s) = Record::dec(&b).unwrap() else { panic!("a snapshot") };
            let st = Store::from_rows(Arc::new(media_schema(true)), &s.rows).unwrap();
            let r = st.rows("media").count();
            std::mem::forget(st);
            r
        }
        "server" => {
            let disk = Box::new(Files::new(dir.join("log")).unwrap());
            let hub = Hub::open(ark5_server::Config::new(media_module()), disk, 0);
            let r = hub.log.store.rows("media").count();
            std::mem::forget(hub);
            r
        }
        _ => {
            let c = Client::open(
                Config {
                    log: media_module(),
                    live: None,
                },
                who,
                Box::new(Files::new(&dir).unwrap()),
                false,
            );
            let r = c.log.store.rows("media").count();
            std::mem::forget(c);
            r
        }
    };
    let whole = t.elapsed();
    let (rss, live) = (vm_rss_kb(), live_bytes() - live0);
    assert_eq!(rows as u64, n, "every row opened");
    // Where the time goes: the files read and decoded, timed again.
    let time = |f: &mut dyn FnMut()| {
        let t = Instant::now();
        f();
        t.elapsed()
    };
    let mut split = String::new();
    let snap = if kind == "server" { dir.join("log/snapshot") } else { dir.join("snapshot") };
    if snap.exists() {
        let read = time(&mut || {
            let b = std::fs::read(&snap).unwrap();
            std::hint::black_box(Record::dec(&b).unwrap());
        });
        split.push_str(&format!("snapshot read {:.0}", ms(read)));
    }
    if kind == "server" || kind == "alone" {
        let path = if kind == "server" { dir.join("log") } else { dir.clone() };
        let mut records = 0;
        let read = time(&mut || {
            records = Files::new(&path).unwrap().load().unwrap().chain.len();
        });
        if !split.is_empty() {
            split.push_str(", ");
        }
        split.push_str(&format!("chain {records} records read {:.0}", ms(read)));
    }
    println!(
        "OPEN {:.0} {} {} {:.1} {split}",
        ms(whole),
        mb(rss),
        mb(base),
        live as f64 / 1048576.0
    );
}

const BYTES_SIZES: [u64; 2] = [10_000, 100_000];

/// What a media row costs by component, in live bytes, each variant in a
/// process of its own ([`perf_d_bytes_child`]): the rows alone (a
/// `Vec<Vec<u8>>`), a store without indexes (the primary map), and with
/// harken's two (the secondaries). v5 has no text indexes.
#[test]
#[ignore]
fn perf_d_bytes() {
    eprintln!("\n== (d) bytes by component (docs/plan-db.md D7.1), harken's schema, media rows");
    eprintln!(
        "{:<12} {:>8} {:>10} {:>10} {:>12} {:>12} {:>12}",
        "variant", "rows", "live MB", "rss MB", "live B/row", "rss B/row", "component"
    );
    for n in BYTES_SIZES {
        let mut prev: Option<f64> = None;
        for variant in ["rows", "bare", "secondaries", "text"] {
            if variant == "text" {
                eprintln!("{variant:<12} {n:>8} {:>10} n/a: v5 has no text indexes (search scans)", "");
                continue;
            }
            let text = child("perf_d_bytes_child", "ARK5_BYTES", format!("{variant} {n}"));
            let line = text
                .lines()
                .find_map(|l| l.split_once("BYTES ").map(|(_, m)| m))
                .unwrap_or_else(|| panic!("{variant} {n}: no measurement:\n{text}"));
            let f: Vec<f64> = line.split(' ').map(|x| x.parse().unwrap()).collect();
            let (live, rss) = (f[0], f[1] * 1024.0);
            let per = live / n as f64;
            let what = match variant {
                "rows" => "the rows",
                "bare" => "primary map",
                _ => "secondaries",
            };
            eprintln!(
                "{variant:<12} {n:>8} {:>10.1} {:>10.1} {:>12.0} {:>12.0} {:>7.0} {what}",
                live / 1048576.0,
                rss / 1048576.0,
                per,
                rss / n as f64,
                per - prev.unwrap_or(0.0)
            );
            prev = Some(per);
        }
    }
    // An open as `perf_d_open` makes it, live against resident.
    let root = std::env::var_os("ARK_PERF_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    eprintln!(
        "\n{:<8} {:>8} {:>10} {:>10} {:>12} {:>12}",
        "open", "rows", "live MB", "rss MB", "live B/row", "rss B/row"
    );
    for n in BYTES_SIZES {
        for kind in ["store", "client"] {
            let dir = tempfile::Builder::new().prefix("ark5-bytes-").tempdir_in(&root).unwrap();
            write_client(dir.path(), n);
            let text = child("perf_d_open_child", "ARK5_OPEN", format!("{kind} {n} {}", dir.path().display()));
            let line = text
                .lines()
                .find_map(|l| l.split_once("OPEN ").map(|(_, m)| m))
                .unwrap_or_else(|| panic!("{kind} {n}: no measurement:\n{text}"));
            let f: Vec<&str> = line.splitn(5, ' ').collect();
            let (rss, base, live): (f64, f64, f64) = (f[1].parse().unwrap(), f[2].parse().unwrap(), f[3].parse().unwrap());
            eprintln!(
                "{kind:<8} {n:>8} {live:>10.1} {:>10.1} {:>12.0} {:>12.0}",
                rss - base,
                live * 1048576.0 / n as f64,
                (rss - base) * 1048576.0 / n as f64
            );
        }
    }
}

/// One variant of [`perf_d_bytes`]. Prints `BYTES <live bytes> <rss KiB>`.
#[test]
#[ignore]
fn perf_d_bytes_child() {
    let Ok(spec) = std::env::var("ARK5_BYTES") else { return };
    let (variant, n) = spec.split_once(' ').unwrap();
    let n: u64 = n.parse().unwrap();
    let (rss0, live0) = (vm_rss_kb(), live_bytes());
    let held: Box<dyn std::any::Any> = match variant {
        "rows" => Box::new((0..n).map(media).collect::<Vec<_>>()),
        "bare" => Box::new(build(&media_schema(false), n)),
        _ => Box::new(build(&media_schema(true), n)),
    };
    let (rss, live) = (vm_rss_kb().saturating_sub(rss0), live_bytes() - live0);
    println!("BYTES {live} {rss}");
    drop(held);
}

/// A store of `n` media rows, put one at a time.
fn build(schema: &Schema, n: u64) -> Store {
    let mut st = Store::new(Arc::new(schema.clone()));
    for i in 0..n {
        st.put("media", media(i)).unwrap();
    }
    st
}

/// A disk that keeps nothing: the frame's measurement is the store it
/// builds, not a copy of the snapshot kept for a reopen.
struct Nowhere;

impl Disk for Nowhere {
    fn load(&mut self) -> std::io::Result<ark5_client::storage::Stored> {
        Ok(Default::default())
    }
    fn record(&mut self, _: &[u8]) -> std::io::Result<()> {
        Ok(())
    }
    fn put(&mut self, _: &str, _: &[u8]) -> std::io::Result<()> {
        Ok(())
    }
    fn get(&mut self, _: &str) -> std::io::Result<Option<Vec<u8>>> {
        Ok(None)
    }
}

const FRAME_SIZES: [u64; 2] = [10_000, 100_000];

/// A `Snapshot` frame of `n` media rows — what a peer below the horizon is
/// sent — received by a fresh client (`Client::receive`: the frame decoded
/// whole, its rows built into a store, the state hash checked), in a
/// process of its own ([`perf_d_frame_child`]): the high-water mark of the
/// live bytes, what is held after, resident memory. v5 has one way; v4's
/// "tree" and "read" were two.
#[test]
#[ignore]
fn perf_d_frame() {
    let root = std::env::var_os("ARK_PERF_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    eprintln!("\n== (d) a snapshot frame decoded and adopted (docs/plan-db.md D7.4), harken's schema, media rows");
    eprintln!(
        "{:<8} {:>8} {:>9} {:>9} {:>11} {:>10} {:>9} {:>14}",
        "way", "rows", "frame MB", "ms", "high MB", "held MB", "rss MB", "high B/row"
    );
    for n in FRAME_SIZES {
        let dir = tempfile::Builder::new().prefix("ark5-frame-").tempdir_in(&root).unwrap();
        let path = dir.path().join("frame");
        let st = media_store(&media_schema(true), 0..n);
        let f = Frame::Snapshot {
            space: Space::Durable,
            snapshot: snapshot(&st, n as Seq, EntryHash([0x5a; 32])),
        };
        std::fs::write(&path, f.enc()).unwrap();
        let size = std::fs::metadata(&path).unwrap().len();
        eprintln!("{:<8} {n:>8} {:>9} n/a: v5 decodes a frame one way", "tree", "");
        let text = child("perf_d_frame_child", "ARK5_FRAME", format!("{n} {}", path.display()));
        let line = text
            .lines()
            .find_map(|l| l.split_once("FRAME ").map(|(_, m)| m))
            .unwrap_or_else(|| panic!("{n}: no measurement:\n{text}"));
        let f: Vec<f64> = line.split(' ').map(|x| x.parse().unwrap()).collect();
        let (ms, high, held, rss) = (f[0], f[1], f[2], f[3] * 1024.0);
        eprintln!(
            "{:<8} {n:>8} {:>9.1} {ms:>9.0} {:>11.1} {:>10.1} {:>9.1} {:>14.0}",
            "read",
            size as f64 / 1e6,
            high / 1048576.0,
            held / 1048576.0,
            rss / 1048576.0,
            high / n as f64
        );
    }
}

/// One frame received, in a process of its own. Prints `FRAME <ms>
/// <high-water bytes> <held bytes> <rss KiB>`.
#[test]
#[ignore]
fn perf_d_frame_child() {
    let Ok(spec) = std::env::var("ARK5_FRAME") else { return };
    let (n, path) = spec.split_once(' ').unwrap();
    let n: u64 = n.parse().unwrap();
    let mut c = Client::open(
        Config {
            log: media_module(),
            live: None,
        },
        Who {
            actor: "alice".into(),
            session: "alice".into(),
            roles: vec![],
        },
        Box::new(Nowhere),
        false,
    );
    let bytes = std::fs::read(path).unwrap();
    let (rss0, live0) = (vm_rss_kb(), live_bytes());
    reset_peak();
    let t = Instant::now();
    c.receive(bytes);
    let took = t.elapsed();
    let (high, held, rss) = (peak_bytes() - live0, live_bytes() - live0, vm_rss_kb().saturating_sub(rss0));
    assert_eq!(c.log.store.rows("media").count() as u64, n, "every row adopted");
    println!("FRAME {} {high} {held} {rss}", ms(took));
}

