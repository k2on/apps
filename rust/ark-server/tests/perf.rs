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

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};

use ark_client::ark::store::Store;
use ark_client::{args, demo, Args, Options, Peer, Timing, Value};

// Live bytes, counted (`docs/plan-db.md` D7.1) ------------------------------
//
// The allocator `rust/ark/tests/allocations.rs` counts allocations with,
// counting *bytes* instead, and both ways: what is allocated and not yet
// freed is what the program holds, which `/proc`'s resident figure is not —
// glibc keeps what was freed in its arenas, so resident carries every
// high-water mark for the life of the process. Process-wide rather than
// per thread, because a store built on one thread is dropped on another in
// a hub; the measurements that read it run alone in a process of their own
// (`perf_d_bytes_child`, `perf_d_open_child`), so nothing else is in the
// count. A relaxed add per allocation is all the other tests here pay.

struct Live;

static LIVE: AtomicI64 = AtomicI64::new(0);

/// The most [`LIVE`] has been since [`reset_peak`]: the high-water mark a
/// transient structure leaves behind it (`docs/plan-db.md` D7.4), read
/// once the structure is gone.
static PEAK: AtomicI64 = AtomicI64::new(0);

// Raises `PEAK` to a live figure that has just grown: a load, and a
// read-modify-write only when it is a new high.
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

/// The bytes allocated and not yet freed, process-wide. What a request
/// asked for, not what the allocator rounded it to: glibc's chunk header
/// (eight bytes) and its rounding to sixteen are not in it.
fn live_bytes() -> i64 {
    LIVE.load(Ordering::Relaxed)
}

/// The high-water mark of [`live_bytes`] since [`reset_peak`].
fn peak_bytes() -> i64 {
    PEAK.load(Ordering::Relaxed)
}

/// Start [`peak_bytes`] again from what is live now.
fn reset_peak() {
    PEAK.store(live_bytes(), Ordering::Relaxed);
}

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

// (d) Open time (`docs/plan-db.md` D5) ---------------------------------------------------

/// The sizes D5 asks for, in rows.
const OPEN_SIZES: [u64; 3] = [10_000, 100_000, 400_000];

/// harken's module, as the server would host it from `HARKEN_MODULE`: the
/// schema is harken's own (the `media` table, its indexes and the tables
/// beside it), with nothing native — opening runs no function.
///
/// `ARK_PERF_MODULE` names another `.ark` to open with instead — an older
/// revision's harken, say, to set a schema change's cost beside it.
fn harken() -> ark_client::Domain {
    match std::env::var_os("ARK_PERF_MODULE") {
        Some(p) => ark_client::Domain::from_bytes(&std::fs::read(&p).expect("ARK_PERF_MODULE"), vec![]).expect("a module"),
        None => ark_client::Domain::from_bytes(include_bytes!("../../../harken/domain/harken.ark"), vec![]).expect("harken.ark"),
    }
}

fn open_id(tag: u8, n: u64) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[0] = tag;
    b[8..].copy_from_slice(&n.to_be_bytes());
    b
}

/// A media row as `add_song` writes one: nine columns, the shape that
/// dominates a library.
fn open_media(i: u64) -> ark::store::Row {
    open_media_pairs(i).into_iter().collect()
}

/// [`open_media`]'s columns, by name.
fn open_media_pairs(i: u64) -> Vec<(String, Value)> {
    [
        ("id", Value::Id(open_id(1, i))),
        ("kind", Value::text("song")),
        ("title", Value::text(format!("Track {i}"))),
        ("creator", Value::text(format!("Artist {}", i % 50))),
        ("duration_ms", Value::int(180_000)),
        ("file", Value::text(format!("music/a{}/t{i}.flac", i % 50))),
        ("pos", Value::int(i as i64 + 1)),
        ("added_ms", Value::int(1_000 + i as i64)),
        ("user_id", Value::text("library")),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

fn open_store(d: &ark_client::Domain, rows: std::ops::Range<u64>) -> ark::store::MemoryStore {
    let mut st = ark::store::MemoryStore::empty(d.module().schema.clone());
    for i in rows {
        st.apply_change(&ark::store::Change::Add("media".into(), open_media(i)));
    }
    st
}

/// The entry that put row `i` there, as the scanner authors it: `add_song`
/// with its arguments and autos, and the row as its one fact.
fn open_record(d: &ark_client::Domain, i: u64) -> (ark::log::Entry, ark::log::Facts) {
    let (fh, _) = d.mutator("add_song").expect("add_song");
    let a: Args = [
        ("title", Value::text(format!("Track {i}"))),
        ("artist", Value::text(format!("Artist {}", i % 50))),
        ("album", Value::text(format!("Album {}", i % 200))),
        ("duration_ms", Value::int(180_000)),
        ("file", Value::text(format!("music/a{}/t{i}.flac", i % 50))),
        ("track", Value::int((i % 12) as i64 + 1)),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    let autos: Args = [
        ("id".to_string(), Value::Id(open_id(1, i))),
        ("now".to_string(), Value::int(1_000 + i as i64)),
    ]
    .into();
    let e = ark::log::Entry {
        id: open_id(7, i),
        actor: "library".into(),
        session: "scan".into(),
        roles: Default::default(),
        fn_hash: fh.clone(),
        args: a,
        autos,
    };
    (e, vec![ark::store::Change::Add("media".into(), open_media(i))])
}

const OPEN_LOG: [u8; 16] = [0x5a; 16];

/// A client of a server, caught up: a `replica` record of `n` rows at
/// cursor `n` of a named log, nothing pending, no pages.
fn write_client(dir: &std::path::Path, d: &ark_client::Domain, n: u64) {
    let st = open_store(d, 0..n);
    let fork = ark_client::Fork::default();
    let bytes = ark_client::storage::encode_replica_of(n as i64, Some(OPEN_LOG), fork, &st, "alice", "dev");
    std::fs::write(dir.join("replica"), bytes).unwrap();
}

/// A server's data directory: a snapshot at the horizon `n - n/10` with
/// its rows and the id of every entry below it, and a journal of the last
/// tenth — each record an `add_song` adding one row — which is as long as
/// a journal gets before it is compacted (the journal is folded once it
/// outgrows the snapshot; a tenth is a typical middle).
fn write_server(dir: &std::path::Path, d: &ark_client::Domain, n: u64) {
    let horizon = n - n / 10;
    let st = open_store(d, 0..horizon);
    let mut log = ark::log::Log {
        base: ark::log::snapshot_of(horizon as i64, st).of_log(Some(OPEN_LOG)),
        ..ark::log::Log::empty(d.module().schema.clone())
    };
    for i in 0..horizon {
        log.ids.insert(open_id(7, i), i as i64 + 1);
    }
    ark_server::persist::save(dir, &log).unwrap();
    let mut journal = vec![];
    for i in horizon..n {
        let (e, f) = open_record(d, i);
        journal.extend(ark::journal::encode_record(i as i64 + 1, &e, &f));
    }
    std::fs::write(ark_server::persist::journal_path_of(dir), journal).unwrap();
}

/// A peer alone that has authored `n` songs: the `replica` record at its
/// head, and its local history — the fork (the empty store at 0) and one
/// page of `n` records — which `open` reads whole for the ids.
fn write_alone(dir: &std::path::Path, d: &ark_client::Domain, n: u64) {
    let st = open_store(d, 0..n);
    let replica = ark_client::storage::encode_replica_of(n as i64, None, ark_client::Fork::default(), &st, "alice", "local");
    std::fs::write(dir.join("replica"), replica).unwrap();
    let fork = ark::log::Log::empty(d.module().schema.clone());
    std::fs::write(dir.join("log"), ark::canon::encode(&ark::journal::log_to_value(&fork))).unwrap();
    let mut page = vec![];
    for i in 0..n {
        let (e, f) = open_record(d, i);
        page.extend(ark::journal::encode_record(i as i64 + 1, &e, &f));
    }
    std::fs::write(dir.join("log.1"), page).unwrap();
}

/// Resident memory, in KiB, from `/proc/self/status`.
fn vm_rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("VmRSS:").and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok()))
        })
        .unwrap_or(0)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

/// The bytes on the directory, as an open reads them.
fn dir_bytes(dir: &std::path::Path) -> u64 {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.metadata().map_or(0, |m| m.len()))
        .sum()
}

/// D5: what opening costs, in time and in resident memory, at 10,000,
/// 100,000 and 400,000 rows — one store alone (the `replica` decoded and
/// built, the baseline a row costs), a client of a server from its
/// `replica` (`Peer::open` over `Dir`), a server from `log.ark-log` and its
/// journal (what `Builder::build` does: `persist::LogFile::open`, then the
/// state at the head), and a peer alone from its replica and local
/// history. The
/// stores are written directly, as rows, so that the setup is not the
/// thing measured; each open runs in a process of its own
/// ([`perf_d_open_child`]) so the resident memory after it is the open's,
/// not this generator's. The time is split coarsely into what it is spent
/// on: reading the rows — decoding the CBOR and building the store in one
/// pass (`docs/plan-db.md` D7.4), rows into tables and their indexes and
/// each row's leaf of the state hash — checking the snapshot's hash,
/// replaying records, and — for a peer — opening the replica, whose
/// optimistic store starts as a clone of the confirmed one, sharing every
/// table (D7.3). "The rest" is the open less what was timed
/// apart. Timings move with the machine's load; the resident memory does
/// not.
///
/// `ARK_PERF_DIR` names where the stores are written (the 400,000-row ones
/// are tens of megabytes); a temporary directory otherwise. Each is removed
/// once measured.
#[test]
#[ignore]
fn perf_d_open() {
    let d = harken();
    let root = std::env::var_os("ARK_PERF_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    eprintln!("\n== (d) open time and resident memory (docs/plan-db.md D5), harken's schema, media rows");
    eprintln!(
        "{:<8} {:>8} {:>9} {:>10} {:>9} {:>9} {:>9}   split (ms)",
        "what", "rows", "on disk", "open ms", "rss MB", "base MB", "live MB"
    );
    for n in OPEN_SIZES {
        for kind in ["store", "client", "server", "alone"] {
            let dir = tempfile::Builder::new().prefix("ark-open-").tempdir_in(&root).unwrap();
            match kind {
                "client" | "store" => write_client(dir.path(), &d, n),
                "server" => write_server(dir.path(), &d, n),
                _ => write_alone(dir.path(), &d, n),
            }
            let bytes = dir_bytes(dir.path());
            let out = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["perf_d_open_child", "--exact", "--ignored", "--nocapture", "--test-threads=1"])
                .env("ARK_OPEN", format!("{kind} {n} {}", dir.path().display()))
                .output()
                .expect("the child runs");
            let text = String::from_utf8_lossy(&out.stdout);
            let line = text
                .lines()
                .find_map(|l| l.split_once("OPEN ").map(|(_, m)| m))
                .unwrap_or_else(|| panic!("{kind} {n}: no measurement:\n{text}\n{}", String::from_utf8_lossy(&out.stderr)));
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

/// One open, in a process of its own: what [`perf_d_open`] runs. Prints
/// `OPEN <ms> <rss MB> <baseline MB> <live MB> <split…>` — the live
/// figure is what the open holds, counted by the allocator, against which
/// resident less the baseline is what the allocator kept besides
/// (`docs/plan-db.md` D7.1). Does nothing unless `ARK_OPEN` says what to
/// open.
#[test]
#[ignore]
fn perf_d_open_child() {
    use ark::journal::{self, Layout};
    use ark::store::MemoryStore;
    let Ok(spec) = std::env::var("ARK_OPEN") else { return };
    let mut it = spec.splitn(3, ' ');
    let (kind, n, dir) = (
        it.next().unwrap(),
        it.next().unwrap().parse::<u64>().unwrap(),
        std::path::PathBuf::from(it.next().unwrap()),
    );
    let d = harken();
    let schema = d.module().schema.clone();
    let mb = |kb: u64| format!("{:.1}", kb as f64 / 1024.0);
    let base = vm_rss_kb();
    let live0 = live_bytes();
    // The open, whole, as the program does it; then the resident memory
    // with what it holds still held.
    let t = Instant::now();
    let held: Box<dyn std::any::Any> = match kind {
        "server" => {
            let (file, log) = ark_server::persist::LogFile::open(&dir, &schema).unwrap();
            let log = log.expect("a log");
            let mut a = ark::peer::Authority::new(schema.clone(), d.closures().clone());
            a.store = log.state_at(log.head_seq()).unwrap();
            a.log = log;
            Box::new((file, a))
        }
        "client" => Box::new(Peer::open_path(d.clone(), &dir, Options::dev("alice")).unwrap()),
        // One store and nothing else — the client's `replica` decoded and
        // built, the decoded tree freed: what a row costs resident, with
        // its indexes, before any program holds two copies of it.
        "store" => {
            let bytes = std::fs::read(dir.join("replica")).unwrap();
            let (file, _) = ark_client::storage::decode_replica(&bytes, &schema).unwrap();
            Box::new(file.confirmed)
        }
        _ => Box::new(Peer::open_path(d.clone(), &dir, Options::alone("alice")).unwrap()),
    };
    let whole = t.elapsed();
    let rss = vm_rss_kb();
    let live = live_bytes() - live0;
    if let Some((_, a)) = held.downcast_ref::<(ark_server::persist::LogFile, ark::peer::Authority)>() {
        assert_eq!(a.store.scan("media").len() as u64, n, "every row opened");
    } else if let Some(p) = held.downcast_ref::<Peer>() {
        assert_eq!(p.store().scan("media").len() as u64, n, "every row opened");
    } else if let Some(st) = held.downcast_ref::<MemoryStore>() {
        assert_eq!(st.scan("media").len() as u64, n, "every row opened");
    }
    drop(held);

    // The same work again, step by step, for where the time goes. Timed
    // after the whole open, so the files are in the page cache both times.
    let time = |f: &mut dyn FnMut()| {
        let t = Instant::now();
        f();
        t.elapsed()
    };
    let mut split = String::new();
    // The client checks no hash on open; the server checks its snapshot's.
    // The rows are built as they are decoded (`docs/plan-db.md` D7.4), so
    // decoding and building are one pass and there is no tree to free:
    // "read" is that pass, as the open makes it.
    let snapshot_split = |bytes: &[u8], rows_key: &str, hashes: bool, split: &mut String| -> MemoryStore {
        let path: &[&str] = if rows_key == "base" { &["base", "rows"] } else { &[rows_key] };
        let mut st = MemoryStore::empty(schema.clone());
        let read = time(&mut || {
            let mut built = MemoryStore::empty(schema.clone());
            let rest = ark::canon::decode_rows(bytes, path, &mut |t, fields| {
                let row = ark::store::Row::from_fields(schema.lookup_table(t), fields);
                built.apply_change(&ark::store::Change::Add(t.into(), row));
            })
            .unwrap();
            std::hint::black_box(rest);
            st = built;
        });
        split.push_str(&format!("read {:.0}", ms(read)));
        if hashes {
            let hash = time(&mut || {
                std::hint::black_box(ark::hash::state_hash(std::hint::black_box(&st)));
            });
            split.push_str(&format!(", hash {:.1}", ms(hash)));
        }
        st
    };
    match kind {
        "store" => {
            let replica = std::fs::read(dir.join("replica")).unwrap();
            let _ = snapshot_split(&replica, "confirmed", false, &mut split);
        }
        "server" => {
            let snap = std::fs::read(ark_server::persist::path_of(&dir)).unwrap();
            let journal_bytes = std::fs::read(ark_server::persist::journal_path_of(&dir)).unwrap();
            let _ = snapshot_split(&snap, "base", true, &mut split);
            let mut log = journal::decode_snapshot(&schema, &snap).unwrap();
            let replay = time(&mut || {
                journal::replay_into(&mut log, &journal_bytes);
            });
            let mut st = MemoryStore::empty(schema.clone());
            let head = time(&mut || st = log.state_at(log.head_seq()).unwrap());
            split.push_str(&format!(
                ", journal {} records {:.0}, state at head {:.0}",
                log.entries.len(),
                ms(replay),
                ms(head)
            ));
        }
        _ => {
            let replica = std::fs::read(dir.join("replica")).unwrap();
            let _ = snapshot_split(&replica, "confirmed", false, &mut split);
            // The replica as `Peer::open` reads it — the bytes, decoded,
            // built — and then opened: the optimistic store is a copy of
            // the confirmed one (`ark::peer::Replica::open`).
            let storage = ark_client::storage::Dir(dir.clone());
            let mut stored = None;
            let load = time(&mut || stored = ark_client::storage::Stored::load(&storage, &schema).unwrap());
            let file = stored.expect("a replica").file;
            let replica = time(&mut || {
                std::hint::black_box(ark::peer::Replica::open(
                    schema.clone(),
                    d.closures().clone(),
                    file.confirmed.clone(),
                    file.cursor,
                    vec![],
                ));
            });
            let mut accounted = load + replica;
            split.push_str(&format!(" (read whole {:.0}), replica open {:.0}", ms(load), ms(replica)));
            if kind == "alone" {
                let mut records = 0usize;
                let pages = time(&mut || {
                    journal::read(&storage, &Layout::alone(), &schema, |_, _, _| records += 1).unwrap();
                });
                accounted += pages;
                split.push_str(&format!(", history {records} records {:.0}", ms(pages)));
            }
            split.push_str(&format!(", the rest {:.0}", ms(whole.saturating_sub(accounted))));
        }
    }
    println!("OPEN {:.0} {} {} {:.1} {split}", ms(whole), mb(rss), mb(base), live as f64 / 1048576.0);
}

/// D7.1 The schema `perf_d_bytes` builds a variant under: harken's, with
/// its indexes, its references and its text indexes kept or taken out.
/// A reference is an index here (`store.rs`, `secondaries`), so it goes
/// with them.
fn bytes_schema(d: &ark_client::Domain, secondaries: bool, texts: bool) -> ark::schema::Schema {
    let mut schema = d.module().schema.clone();
    for t in &mut schema.tables {
        if !secondaries {
            t.indexes.clear();
            t.refs.clear();
        }
        if !texts {
            t.text.clear();
        }
    }
    schema
}

const BYTES_SIZES: [u64; 2] = [10_000, 100_000];

/// `docs/plan-db.md` D7.1: what a media row costs, by component, in live
/// bytes — what the allocator was asked for and has not had back — and
/// beside each the resident figure `perf_d_open` reads. Built by
/// subtraction, each variant in a process of its own
/// ([`perf_d_bytes_child`]) so that one's freed memory is not another's
/// resident:
///
/// - **rows**: the rows alone, laid out as the table's, in a `Vec<Row>`
///   (sixteen bytes a row of that is the vector's slot);
/// - **bare**: a store under harken's schema with every index, reference
///   and text index taken out — the rows and the primary map;
/// - **secondaries**: the declared indexes and the references back;
/// - **text**: D4's text indexes back too — harken as it is.
///
/// Then one open of a `replica` record as `perf_d_open` makes it, a store
/// and a client, each with its live bytes beside its resident memory: the
/// question whether the decoded tree an open builds and frees is still
/// resident after it.
///
/// `cargo test -p ark-server --release --test perf perf_d_bytes -- --ignored --nocapture --test-threads=1`.
#[test]
#[ignore]
fn perf_d_bytes() {
    let run = |test: &str, var: &str, val: String| -> String {
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([test, "--exact", "--ignored", "--nocapture", "--test-threads=1"])
            .env(var, val)
            .output()
            .expect("the child runs");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    eprintln!("\n== (d) bytes by component (docs/plan-db.md D7.1), harken's schema, media rows");
    eprintln!(
        "{:<12} {:>8} {:>10} {:>10} {:>12} {:>12} {:>12}",
        "variant", "rows", "live MB", "rss MB", "live B/row", "rss B/row", "component"
    );
    for n in BYTES_SIZES {
        let mut prev: Option<f64> = None;
        for variant in ["rows", "bare", "secondaries", "text"] {
            let text = run("perf_d_bytes_child", "ARK_BYTES", format!("{variant} {n}"));
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
                "secondaries" => "secondaries",
                _ => "text indexes",
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
    // The decoded tree: an open as `perf_d_open` makes it, live against
    // resident.
    let d = harken();
    let root = std::env::var_os("ARK_PERF_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    eprintln!(
        "\n{:<8} {:>8} {:>10} {:>10} {:>12} {:>12}",
        "open", "rows", "live MB", "rss MB", "live B/row", "rss B/row"
    );
    for n in BYTES_SIZES {
        for kind in ["store", "client"] {
            let dir = tempfile::Builder::new().prefix("ark-bytes-").tempdir_in(&root).unwrap();
            write_client(dir.path(), &d, n);
            let text = run("perf_d_open_child", "ARK_OPEN", format!("{kind} {n} {}", dir.path().display()));
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

/// One variant of [`perf_d_bytes`], in a process of its own. Prints
/// `BYTES <live bytes> <rss KiB>`, both as moved by building it. Does
/// nothing unless `ARK_BYTES` says what to build.
#[test]
#[ignore]
fn perf_d_bytes_child() {
    use ark::store::{Change, MemoryStore, Row};
    let Ok(spec) = std::env::var("ARK_BYTES") else { return };
    let (variant, n) = spec.split_once(' ').unwrap();
    let n: u64 = n.parse().unwrap();
    let d = harken();
    let schema = match variant {
        "rows" | "bare" => bytes_schema(&d, false, false),
        "secondaries" => bytes_schema(&d, true, false),
        _ => bytes_schema(&d, true, true),
    };
    let (rss0, live0) = (vm_rss_kb(), live_bytes());
    let held: Box<dyn std::any::Any> = if variant == "rows" {
        let tbl = schema.lookup_table("media").unwrap();
        let rows: Vec<Row> = (0..n).map(|i| Row::of(tbl, open_media_pairs(i))).collect();
        Box::new(rows)
    } else {
        let mut st = MemoryStore::empty(schema.clone());
        for i in 0..n {
            st.apply_change(&Change::Add("media".into(), open_media(i)));
        }
        Box::new(st)
    };
    let (rss, live) = (vm_rss_kb().saturating_sub(rss0), live_bytes() - live0);
    println!("BYTES {live} {rss}");
    drop(held);
}

const FRAME_SIZES: [u64; 2] = [10_000, 100_000];

/// `docs/plan-db.md` D7.4, the third place a store arrives whole: a
/// `snapshot` frame on the socket — what a peer below the horizon is sent
/// — of `n` media rows under harken's schema, decoded and adopted by a
/// client, each way in a process of its own ([`perf_d_frame_child`]).
/// Printed: the high-water mark of the live bytes above what was held
/// before, what is held after, and resident memory less what it was before
/// — the tree a decode builds and frees is in the first and the third and
/// not in the second.
///
/// - **tree**: `canon::decode` of the whole frame, `ServerMsg::from_value`,
///   `Client::recv` — the snapshot's rows a struct each until adopted, as
///   every client read one before D7.4;
/// - **read**: `ServerMsg::decode_for` and `Client::recv_snapshot` — each
///   row built as its fields are read, as `ark_client::Peer` reads the
///   socket now.
///
/// `cargo test -p ark-server --release --test perf perf_d_frame -- --ignored --nocapture --test-threads=1`.
#[test]
#[ignore]
fn perf_d_frame() {
    let d = harken();
    let root = std::env::var_os("ARK_PERF_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    eprintln!("\n== (d) a snapshot frame decoded and adopted (docs/plan-db.md D7.4), harken's schema, media rows");
    eprintln!(
        "{:<8} {:>8} {:>9} {:>9} {:>11} {:>10} {:>9} {:>14}",
        "way", "rows", "frame MB", "ms", "high MB", "held MB", "rss MB", "high B/row"
    );
    for n in FRAME_SIZES {
        let dir = tempfile::Builder::new().prefix("ark-frame-").tempdir_in(&root).unwrap();
        let path = dir.path().join("frame");
        std::fs::write(&path, snapshot_frame(&d, n)).unwrap();
        let size = std::fs::metadata(&path).unwrap().len();
        for way in FRAME_WAYS {
            let out = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["perf_d_frame_child", "--exact", "--ignored", "--nocapture", "--test-threads=1"])
                .env("ARK_FRAME", format!("{way} {n} {}", path.display()))
                .output()
                .expect("the child runs");
            let text = String::from_utf8_lossy(&out.stdout);
            let line = text
                .lines()
                .find_map(|l| l.split_once("FRAME ").map(|(_, m)| m))
                .unwrap_or_else(|| panic!("{way} {n}: no measurement:\n{text}\n{}", String::from_utf8_lossy(&out.stderr)));
            let f: Vec<f64> = line.split(' ').map(|x| x.parse().unwrap()).collect();
            let (ms, high, held, rss) = (f[0], f[1], f[2], f[3] * 1024.0);
            eprintln!(
                "{way:<8} {n:>8} {:>9.1} {ms:>9.0} {:>11.1} {:>10.1} {:>9.1} {:>14.0}",
                size as f64 / 1e6,
                high / 1048576.0,
                held / 1048576.0,
                rss / 1048576.0,
                high / n as f64
            );
        }
    }
}

const FRAME_WAYS: [&str; 2] = ["tree", "read"];

/// The bytes of the `snapshot` frame a server sends a peer below the
/// horizon, made as `protocol::Server` makes it: every table's rows, each
/// `Row::into_value`, under harken's module hash.
fn snapshot_frame(d: &ark_client::Domain, n: u64) -> Vec<u8> {
    use ark::protocol::ServerMsg;
    let st = open_store(d, 0..n);
    let rows = st
        .table_names()
        .into_iter()
        .map(|t| (t.clone(), st.scan(&t).into_iter().map(ark::store::Row::into_value).collect()))
        .collect();
    let msg = ServerMsg::SnapshotOf {
        seq: n as i64,
        hash: vec![0x5a; 32],
        rows,
        log_id: Some(OPEN_LOG),
        module: Some(vec![0xa5; 32]),
    };
    ark::canon::encode(&msg.to_value())
}

/// One way of [`perf_d_frame`], in a process of its own: the frame read
/// from its file first, then the measurement from there. Prints
/// `FRAME <ms> <high-water bytes> <held bytes> <rss KiB>`. Does nothing
/// unless `ARK_FRAME` says what to decode.
#[test]
#[ignore]
fn perf_d_frame_child() {
    use ark::protocol::{Client, Mode, Received, ServerMsg};
    let Ok(spec) = std::env::var("ARK_FRAME") else { return };
    let mut it = spec.splitn(3, ' ');
    let (way, n, path) = (
        it.next().unwrap(),
        it.next().unwrap().parse::<u64>().unwrap(),
        std::path::PathBuf::from(it.next().unwrap()),
    );
    let d = harken();
    let schema = d.module().schema.clone();
    let replica = ark::peer::Replica::open(schema.clone(), d.closures().clone(), ark::store::MemoryStore::empty(schema), 0, vec![]);
    let mut client = Client::open(replica, Mode::ByFacts, None);
    let bytes = std::fs::read(&path).unwrap();
    let (rss0, live0) = (vm_rss_kb(), live_bytes());
    reset_peak();
    let t = Instant::now();
    match way {
        "tree" => {
            let v = ark::canon::decode(&bytes).unwrap();
            client.recv(ServerMsg::from_value(&v).unwrap());
        }
        "read" => match ServerMsg::decode_for(&bytes, &client.schema).unwrap() {
            Received::Snapshot(s) => client.recv_snapshot(s),
            Received::Msg(m) => panic!("not a snapshot: {m:?}"),
        },
        other => panic!("no way {other}"),
    }
    client.settle();
    let took = t.elapsed();
    let (high, held, rss) = (peak_bytes() - live0, live_bytes() - live0, vm_rss_kb().saturating_sub(rss0));
    assert_eq!(client.replica.confirmed.scan("media").len() as u64, n, "every row adopted");
    println!("FRAME {} {high} {held} {rss}", ms(took));
    drop(client);
}
