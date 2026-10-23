//! What a peer with its disk and its server costs, held as bounds rather
//! than printed: what an offline device writes per intent as its pending
//! grow, what the hub allocates per connection it fans an entry out to,
//! and the live bytes a store holds per row of harken's `media` table
//! (key `id`, indexes `file` and `pos`, written out here as a schema,
//! since this crate does not depend on harken). Over the demo of
//! `arkdb`; allocations and bytes counted on the test's own thread.
//!
//! [`timings`] is the benchmark Part 3's measurement protocol reads
//! (`docs/plan-v5.md`): ignored by default; run with
//! `cargo test -p ark-peer --release --test costs -- --ignored --nocapture`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ark::format::Encode;
use ark::frames::Frame;
use ark::log::{EntryId, Origin};
use ark::row::{Id, Type};
use ark::schema::{Schema, Space, Table};
use ark::store::Store;
use arkdb::demo::{demo as domain, AddToPlaylist};
use ark_peer::link::wait;
use ark_peer::{dev, now_ms, Config, Disk, Memory, Module, Peer, Server};

mod support;

use support::{device, named};

// Allocations and live bytes, counted on this thread ---------------------------------

struct Tally;

thread_local!(static COUNTS: Cell<(usize, isize)> = const { Cell::new((0, 0)) });

fn tally(allocs: usize, bytes: isize) {
    let _ = COUNTS.try_with(|c| {
        let (a, b) = c.get();
        c.set((a + allocs, b + bytes));
    });
}

unsafe impl GlobalAlloc for Tally {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        tally(1, l.size() as isize);
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        tally(0, -(l.size() as isize));
        System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        tally(1, n as isize - l.size() as isize);
        System.realloc(p, l, n)
    }
}

#[global_allocator]
static GLOBAL: Tally = Tally;

/// What `f` returns, and the allocations it made and the bytes it left
/// live on this thread.
fn counted<T>(f: impl FnOnce() -> T) -> (T, usize, isize) {
    let (a, b) = COUNTS.with(Cell::get);
    let t = f();
    let (a2, b2) = COUNTS.with(Cell::get);
    (t, a2 - a, b2 - b)
}

// The demo, driven --------------------------------------------------------------------

/// The playlist the session's `local`-th entry made.
fn pid(session: &str, local: u64) -> [u8; 16] {
    EntryId::of(Origin::named(session), local as i64).draw(0)
}

fn add_input(pl: &[u8; 16], i: u64) -> Vec<u8> {
    AddToPlaylist {
        playlist_id: Id::new(*pl),
        track_id: format!("t{i}"),
    }
    .to_bytes()
}

/// A disk in memory that counts the bytes written to it.
#[derive(Clone, Default)]
struct Counting(Arc<Mutex<(Memory, usize)>>);

impl Counting {
    fn written(&self) -> usize {
        self.0.lock().unwrap().1
    }
}

impl Disk for Counting {
    fn get(&mut self, n: &str) -> io::Result<Option<Vec<u8>>> {
        self.0.lock().unwrap().0.get(n)
    }
    fn put(&mut self, n: &str, b: &[u8]) -> io::Result<()> {
        let mut d = self.0.lock().unwrap();
        d.1 += b.len();
        d.0.put(n, b)
    }
    fn append(&mut self, n: &str, b: &[u8]) -> io::Result<()> {
        let mut d = self.0.lock().unwrap();
        d.1 += b.len();
        d.0.append(n, b)
    }
}

/// An offline device with ten playlists and `n` intents pending on them:
/// the bytes its next 256 intents write to its disk, all told.
fn offline(n: u64) -> usize {
    let disk = Counting::default();
    let token = "alice@perf";
    let mut c = device(token, Box::new(disk.clone()));
    for j in 0..10 {
        c.author(Space::Durable, "demo.create_playlist", named(&format!("P{j}")), 0).unwrap();
    }
    let add = |c: &mut Peer, i: u64| {
        c.author(Space::Durable, "demo.add_to_playlist", add_input(&pid(token, i % 10 + 1), i), 0)
            .unwrap();
    };
    (0..n).for_each(|i| add(&mut c, i));
    let before = disk.written();
    (n..n + 256).for_each(|i| add(&mut c, i));
    assert_eq!(c.log.pending.len() as u64, n + 266, "all pending");
    disk.written() - before
}

/// An offline device's intent writes what it wrote at 100 pending at
/// 4,000, within a half, across 256 intents, folds of its journal
/// included (73,984 and 74,240 bytes measured): each intent appends its
/// own record, and nothing rewrites every pending one per intent.
/// Falsified by a local instance writing its rows whole after every write
/// (`Instance::step` folding whenever `Fold::rows`): 6,285,807 B at 100,
/// 108,924,032 at 4,000.
#[test]
fn an_offline_intent_writes_the_same_as_pending_grows() {
    let (small, big) = (offline(100), offline(4000));
    assert!(big * 2 <= small * 3, "256 intents wrote {small} B at 100 pending, {big} B at 4,000");
}

/// A hub with `c` connections fed whole, each at the head, and one of
/// them pushing an add: what one push allocates.
fn fanout(c: usize) -> usize {
    let mut hub = Peer::serve(Config::new(Module::of(&domain())), Box::new(Memory::default()), 0);
    let author = dev("alice@dev").unwrap();
    let a = hub.accept(author.clone());
    let mut local = 0;
    let mut push = |hub: &mut Peer, conn, who: &ark::peer::Who, func: &str, input: Vec<u8>| {
        local += 1;
        hub.frame(conn, support::push(who, local, func, input), 0)
    };
    for j in 0..10 {
        push(&mut hub, a, &author, "demo.create_playlist", named(&format!("P{j}")));
    }
    let (head, hash) = hub.log.log().head();
    let hello = Frame::Hello {
        space: Space::Durable,
        cursor: head,
        hash: Some(hash),
        device: None,
        schema: Default::default(),
        version: String::new(),
    }
    .enc();
    let conns: Vec<u64> = (0..c).map(|k| hub.accept(dev(&format!("u{k}@x")).unwrap())).collect();
    conns.iter().for_each(|k| drop(hub.frame(*k, hello.clone(), 0)));
    let (out, made, _) = counted(|| push(&mut hub, a, &author, "demo.add_to_playlist", add_input(&pid("alice@dev", 1), 0)));
    assert!(out.len() > c, "a batch to each, and the author's ack");
    made
}

/// The hub fans one entry out to 160 connections in at most one
/// allocation more per connection than to 10 (164 and 318 measured): the
/// `Batch` is encoded once and each connection is handed a copy of it.
/// Falsified by the hub decoding and encoding the `Batch` again for each
/// connection: 351 and 3,055.
#[test]
fn a_fanout_encodes_once() {
    let (ten, many) = (fanout(10), fanout(160));
    assert!(many <= ten + 150 + 8, "one push allocated {ten} times to 10 connections, {many} to 160");
}

/// harken's `media` table, as `harken/domain/src/schema.rs` declares it.
fn media_schema() -> Schema {
    let t = Table::new("media")
        .col("id", Type::Id("media".into()))
        .col("kind", Type::Text)
        .col("title", Type::Text)
        .col("creator", Type::Text)
        .col("duration_ms", Type::Int)
        .col("file", Type::Text)
        .col("pos", Type::Int)
        .col("added_ms", Type::Int)
        .col("user_id", Type::Text)
        .index(&["file"], false)
        .index(&["pos"], false);
    Schema { tables: vec![t] }
}

/// A media row as `add_song` writes one.
fn media(schema: &Schema, i: u64) -> Vec<u8> {
    let mut id = [0u8; 16];
    id[8..].copy_from_slice(&i.to_be_bytes());
    let row = serde_json::json!({
        "id": ark::hash::hex(&id),
        "kind": "song",
        "title": format!("Track {i}"),
        "creator": format!("Artist {}", i % 50),
        "duration_ms": 180_000,
        "file": format!("music/a{}/t{i}.flac", i % 50),
        "pos": i as i64 + 1,
        "added_ms": 1_000 + i as i64,
        "user_id": "library",
    });
    ark::dynamic::row_bytes(&schema.tables[0], &row).unwrap()
}

/// A store of 20,000 media rows with harken's two indexes holds at most
/// 400 live bytes a row (323 measured), the rows' own bytes included.
/// Falsified by an index entry holding the row in place of its key
/// (`Store::write`): 447 bytes a row.
#[test]
fn a_media_row_is_held_in_bounded_bytes() {
    let (schema, n) = (media_schema(), 20_000u64);
    let rows: Vec<Vec<u8>> = (0..n).map(|i| media(&schema, i)).collect();
    let (store, _, live) = counted(|| {
        let mut st = Store::new(Arc::new(schema.clone()), Space::Durable);
        rows.into_iter().for_each(|r| drop(st.put("media", r).unwrap()));
        st
    });
    assert_eq!(store.rows("media").count(), n as usize);
    let per = live as f64 / n as f64;
    assert!(per <= 400.0, "{per:.0} live bytes a media row");
}

/// The numbers Part 3 reads: a linked round trip through the hub in this
/// process (a playlist created and pumped until confirmed), and an
/// offline device's intent at 4,000 pending.
#[test]
#[ignore]
fn timings() {
    let mut s = Server::new(Config::new(Module::of(&domain())));
    s.data = None;
    let hub = s.build().unwrap().0;
    let mut c = support::linked("alice@perf", "", hub.dialer(Arc::new(dev)));
    support::until(&mut c, "linked", |c, _| c.linked);
    let n = 2000;
    let t = Instant::now();
    for j in 0..n {
        c.0.author(Space::Durable, "demo.create_playlist", named(&format!("P{j}")), now_ms())
            .unwrap();
        c.0.pump();
        while !c.0.log.pending.is_empty() {
            wait(&c.0, &c.1, Duration::from_millis(1));
            c.0.pump();
        }
    }
    eprintln!("b: a linked round trip, {n} playlists: {:.1} µs", us(t.elapsed() / n));
    let disk = Counting::default();
    let mut d = device("bob@perf", Box::new(disk));
    d.author(Space::Durable, "demo.create_playlist", named("P"), 0).unwrap();
    (0..4000).for_each(|i| {
        d.author(Space::Durable, "demo.add_to_playlist", add_input(&pid("bob@perf", 1), i), 0)
            .unwrap()
    });
    let t = Instant::now();
    (0..1000).for_each(|i| {
        d.author(Space::Durable, "demo.add_to_playlist", add_input(&pid("bob@perf", 1), 5000 + i), 0)
            .unwrap()
    });
    eprintln!("c: an offline intent at 4,000 pending: {:.1} µs", us(t.elapsed() / 1000));
}

fn us(d: Duration) -> f64 {
    d.as_secs_f64() * 1e6
}
