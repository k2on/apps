//! What harken's own mutations and reads cost on ArkDB v5 as the library
//! grows, printed rather than asserted: `harken/domain/tests/perf.rs`'s
//! shapes (`docs/plan-v5.md` Part 3), the same sizes, columns and labels,
//! so the two outputs diff. Over the machine alone — a replica and the
//! authority it pushes to, frames as bytes, and beside them the one peer
//! in the authority's role that a v5 peer alone is — so that what is
//! measured is the domain's functions, the interpreter that runs them and
//! the store under them, not a storage or a socket.
//!
//! Counted beside the timings: the rows each mutation's reads examine (the
//! counting store of `rust/ark/tests/support/counting.rs`; v5's store
//! examines only what it returns) and the allocations a read makes.
//!
//! Ignored by default; run with
//! `cargo test -p harken-domain --release --test perf -- --ignored --nocapture --test-threads=1`.

#[path = "../../../rust/ark/tests/support/counting.rs"]
mod counting;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use ark::author::{Domain, Id, Node};
use ark::hash::{EntryHash, FnHash};
use ark::log::{Durable, EntryId, Snapshot};
use ark::peer::{Ctx, Input, Journal, Output, Peer, Role, Who};
use ark::store::{Read, Scan, Store};
use counting::Counting;
use harken_domain::library::{AddSong, Library};
use harken_domain::playlists::{AddToPlaylist, CreatePlaylist};
use harken_domain::schema::{Media, Playlist, PlaylistItem, LIBRARY};

// Counting allocations ---------------------------------------------------------

struct Tally;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Tally {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        System.realloc(p, l, n)
    }
}

#[global_allocator]
static GLOBAL: Tally = Tally;

fn allocs() -> usize {
    ALLOCS.load(Ordering::Relaxed)
}

// The library, as a peer alone holds it -----------------------------------------

fn domain() -> &'static Domain {
    static D: OnceLock<Domain> = OnceLock::new();
    D.get_or_init(harken_domain::domain)
}

/// A function's hash by its binding, looked up once.
fn f(name: &str) -> FnHash {
    static F: OnceLock<std::collections::BTreeMap<String, FnHash>> = OnceLock::new();
    F.get_or_init(|| domain().module.bindings.iter().cloned().collect())[name]
}

fn idv(tag: u8, n: u64) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[0] = tag;
    b[8..].copy_from_slice(&n.to_be_bytes());
    b
}

/// A media row, as `add_song` writes one.
fn media(i: u64) -> Media {
    Media {
        id: Id::new(idv(1, i)),
        kind: "song".into(),
        title: format!("Track {i}"),
        creator: format!("Artist {}", i % 50),
        duration_ms: 180_000,
        file: format!("music/a{}/t{i}.flac", i % 50),
        pos: i as i64 + 1,
        added_ms: 1_000 + i as i64,
        user_id: "library".into(),
    }
}

fn me() -> Who {
    Who {
        actor: "alice".into(),
        session: "local".into(),
        roles: vec![LIBRARY.into()],
    }
}

/// A library of `n` media rows, and nothing else.
fn library(n: u64) -> Store {
    let mut st = Store::new(domain().schema.clone());
    for i in 0..n {
        st.put("media", media(i).to_bytes()).unwrap();
    }
    st
}

fn playlist_row(id: [u8; 16], name: &str, pos: i64, user: &str) -> Vec<u8> {
    Playlist {
        id: Id::new(id),
        name: name.into(),
        pos,
        created_ms: 1,
        user_id: user.into(),
    }
    .to_bytes()
}

/// A library of `n` media rows and one playlist of alice's holding the
/// first `items` of them, written as rows.
fn with_playlist(n: u64, items: u64) -> (Store, [u8; 16]) {
    let mut st = library(n);
    let pl = idv(2, 1);
    st.put("playlist", playlist_row(pl, "Mine", 1, "alice")).unwrap();
    for i in 0..items {
        let row = PlaylistItem {
            playlist_id: Id::new(pl),
            media_id: Id::new(idv(1, i)),
            pos: i as i64 + 1,
            added_ms: 1,
            user_id: "alice".into(),
        };
        st.put("playlist_item", row.to_bytes()).unwrap();
    }
    (st, pl)
}

/// A library of `n` media rows, and a thousand playlists each for alice
/// and bob: alice's "Favorites", a hundred "Favorites (k)" and 899 others,
/// bob's a thousand "Favorites (k)".
fn with_playlists(n: u64) -> Store {
    let mut st = library(n);
    let alice = std::iter::once("Favorites".to_string())
        .chain((1..=100).map(|k| format!("Favorites ({k})")))
        .chain((0..899).map(|k| format!("List {k}")))
        .map(|name| ("alice", name));
    let bob = (1..=1000).map(|k| ("bob", format!("Favorites ({k})")));
    for (i, (user, name)) in alice.chain(bob).enumerate() {
        st.put("playlist", playlist_row(idv(3, i as u64), &name, i as i64 + 1, user)).unwrap();
    }
    st
}

fn opened(role: Role, st: &Store) -> Peer {
    let mut p = Peer::new(domain().schema.clone(), domain().cas(), role, me());
    let snapshot = Snapshot {
        seq: 0,
        hash: EntryHash::default(),
        state: st.state_hash(),
        rows: st.snapshot_rows(),
    };
    p.step(Input::Open(Durable {
        snapshot,
        chain: vec![],
        pending: vec![],
        local: 0,
    }));
    assert_eq!(p.store, *st, "opened on the library");
    p
}

/// A peer alone over a confirmed store, both ways: a replica and the
/// authority it pushes to (v4's shape), and one peer in the authority's
/// role (v5's).
struct Alone {
    a: Peer,
    r: Peer,
    solo: Peer,
    now: i64,
}

impl Alone {
    fn new(st: &Store) -> Alone {
        let mut solo = opened(Role::Replica, st);
        solo.step(Input::Alone);
        Alone {
            a: opened(Role::Authority { journal: Journal::default() }, st),
            r: opened(Role::Replica, st),
            solo,
            now: 1_000,
        }
    }

    /// Author on the replica and confirm through the authority — the
    /// optimistic run, and the push, sequence and confirm — timed apart;
    /// then the same intent on the peer alone, timed. Refusals panic.
    fn mutate(&mut self, name: &str, input: Vec<u8>) -> (Duration, Duration, Duration) {
        self.now += 1;
        let (func, now) = (f(name), self.now);
        let t = Instant::now();
        let o = self.r.step(Input::Mutate {
            func,
            input: input.clone(),
            now,
        });
        let t1 = Instant::now();
        for x in o {
            match x {
                Output::Send(b) => {
                    for y in self.a.step(Input::Frame { from: Some(me()), bytes: b }) {
                        if let Output::Send(b) | Output::Fanout(b) = y {
                            self.r.step(Input::Frame { from: None, bytes: b });
                        }
                    }
                }
                Output::Refused { why, .. } => panic!("{name}: {why}"),
                _ => {}
            }
        }
        let t2 = Instant::now();
        assert!(self.r.pending.is_empty(), "{name}: confirmed");
        let t3 = Instant::now();
        let o = self.solo.step(Input::Mutate { func, input, now });
        let solo = t3.elapsed();
        assert!(matches!(o.last(), Some(Output::Confirmed(_))), "{name}: alone {o:?}");
        (t1 - t, t2 - t1, solo)
    }

    /// The playlist the replica's last entry made.
    fn newest_playlist(&self) -> [u8; 16] {
        EntryId::of(&me().session, self.r.local).draw(0)
    }
}

fn song(i: u64) -> Vec<u8> {
    AddSong {
        title: format!("Track {i}"),
        artist: format!("Artist {}", i % 50),
        album: format!("Album {}", i % 200),
        duration_ms: 180_000,
        file: format!("music/new/{i}.flac"),
        track: (i % 12) as i64 + 1,
        ..AddSong::default()
    }
    .to_bytes()
}

fn on(pl: [u8; 16], m: [u8; 16]) -> Vec<u8> {
    AddToPlaylist {
        playlist_id: Id::new(pl),
        media_id: Id::new(m),
    }
    .to_bytes()
}

fn named(name: &str) -> Vec<u8> {
    CreatePlaylist { name: name.into() }.to_bytes()
}

// Reporting ------------------------------------------------------------------------

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

const ALONE: &str = "whole = replica step(Mutate) + authority step(Push) + replica step(Batch, Ack); `alone` = one Peer in the authority's role";

// The mutations -----------------------------------------------------------------------

/// `add_song` into a library that grows from nothing, as the scanner
/// authors one per file.
#[test]
#[ignore]
fn perf_add_song() {
    header("add_song, one per file, into a library growing from 0 (peer alone, engine)");
    note(ALONE);
    for n in [500u64, 2000, 8000] {
        let mut p = Alone::new(&Store::new(domain().schema.clone()));
        let (mut whole, mut opt, mut commit, mut solo) = (vec![], vec![], vec![], vec![]);
        for i in 0..n {
            let (o, c, s) = p.mutate("library.add_song", song(i));
            opt.push(o);
            commit.push(c);
            whole.push(o + c);
            solo.push(s);
        }
        assert_eq!(p.r.store.rows("media").count(), n as usize);
        line("add_song: whole", &whole);
        line("  … Replica::mutate (optimistic)", &opt);
        line("  … local_commit (authority + confirm)", &commit);
        line("  … alone: Peer (authority) step(Mutate)", &solo);
    }
}

/// `add_to_playlist` onto one playlist that grows to N, and onto N/10
/// playlists of ten, over a library of N media; and `create_playlist` for
/// one person, with names that differ and with one name every time.
#[test]
#[ignore]
fn perf_playlists() {
    header("add_to_playlist over a library of N (peer alone, engine)");
    note(ALONE);
    for n in [500u64, 2000, 8000] {
        for (shape, lists) in [("one playlist", 1u64), ("playlists of 10", n / 10)] {
            let mut p = Alone::new(&library(n));
            let mut ids = vec![];
            for j in 0..lists {
                p.mutate("playlists.create_playlist", named(&format!("P{j}")));
                ids.push(p.newest_playlist());
            }
            let (mut whole, mut opt, mut commit, mut solo) = (vec![], vec![], vec![], vec![]);
            for i in 0..n {
                let (o, c, s) = p.mutate("playlists.add_to_playlist", on(ids[(i % lists) as usize], idv(1, i)));
                opt.push(o);
                commit.push(c);
                whole.push(o + c);
                solo.push(s);
            }
            assert_eq!(p.r.store.rows("playlist_item").count(), n as usize);
            line(&format!("add_to_playlist, {shape}: whole"), &whole);
            line("  … Replica::mutate (optimistic)", &opt);
            line("  … local_commit (authority + confirm)", &commit);
            line("  … alone: Peer (authority) step(Mutate)", &solo);
        }
    }
    header("create_playlist, one person's P playlists (peer alone, engine)");
    note("whole, as above");
    for n in [50u64, 200, 800] {
        for (shape, same) in [("distinct names", false), ("all \"Favorites\"", true)] {
            let mut p = Alone::new(&Store::new(domain().schema.clone()));
            let per: Vec<Duration> = (0..n)
                .map(|j| {
                    let name = if same { "Favorites".to_string() } else { format!("List {j}") };
                    let (o, c, _) = p.mutate("playlists.create_playlist", named(&name));
                    o + c
                })
                .collect();
            assert_eq!(p.r.store.rows("playlist").count(), n as usize);
            line(&format!("create_playlist, {shape}"), &per);
        }
    }
}

/// `playlist_name` alone, interpreted (v5 has no native path), among one
/// person's P playlists: a name they do not have, and one they do
/// (`free_number`: each name's rank among them, then the least rank free).
#[test]
#[ignore]
fn perf_playlist_name() {
    eprintln!("\n== playlist_name among P names (native, harken_domain::playlists)");
    note("interpreted: Domain::ask(\"playlists.playlist_name\"), the one path");
    eprintln!("{:<40} {:>7} {:>12}", "name", "P", "µs per call");
    let st = Store::new(domain().schema.clone());
    let ctx = Ctx {
        id: EntryId::of("q", 0),
        actor: "alice".into(),
        session: "q".into(),
        roles: vec![],
        now: 0,
    };
    for p in [10usize, 100, 1000] {
        let names: Vec<String> = std::iter::once("Favorites".to_string())
            .chain((1..p).map(|i| format!("List {i}")))
            .collect();
        for (label, name) in [("a free name", "Night"), ("a taken name", "Favorites")] {
            let input = (names.clone(), name.to_string()).to_bytes();
            let reps = if p >= 1000 { 5 } else { 50 };
            let t = Instant::now();
            let mut out = vec![];
            for _ in 0..reps {
                out = std::hint::black_box(domain().ask(&st, &ctx, "playlists.playlist_name", &input).unwrap());
            }
            let named = String::from_bytes(&out).unwrap();
            assert_eq!(named, if name == "Night" { "Night" } else { "Favorites (1)" });
            eprintln!("{:<40} {:>7} {:>12.1}", label, p, us(t.elapsed()) / reps as f64);
        }
    }
}

type Make = Box<dyn Fn(u64) -> (Store, Vec<u8>)>;

/// The mutations whose reads are about, each with the store and the input
/// it is applied to at a library of `n`.
fn read_cases() -> Vec<(&'static str, &'static str, Make)> {
    let onto = |filled: bool| {
        move |n: u64| {
            let (st, pl) = with_playlist(n, if filled { n - 1 } else { 0 });
            (st, on(pl, idv(1, n - 1)))
        }
    };
    vec![
        ("add_song (a new file)", "library.add_song", Box::new(|n| (library(n), song(99_999_999)))),
        ("add_to_playlist (an empty playlist)", "playlists.add_to_playlist", Box::new(onto(false))),
        ("add_to_playlist (a playlist of N-1)", "playlists.add_to_playlist", Box::new(onto(true))),
        (
            "create_playlist (alice's second)",
            "playlists.create_playlist",
            Box::new(|n| (with_playlist(n, 0).0, named("Other"))),
        ),
        (
            "create_playlist (a 102nd Favorites, of 1,000)",
            "playlists.create_playlist",
            Box::new(|n| (with_playlists(n), named("Favorites"))),
        ),
    ]
}

fn ctx_of(n: i64) -> Ctx {
    Ctx {
        id: EntryId::of("local", n),
        actor: "alice".into(),
        session: "local".into(),
        roles: vec![LIBRARY.into()],
        now: 5,
    }
}

fn reads_of(n: u64, name: &str, make: &dyn Fn(u64) -> (Store, Vec<u8>)) -> counting::Reads {
    let (st, input) = make(n);
    let c = Counting::new(&st);
    let out = domain().run(&c, &ctx_of(1_000_000), name, &input);
    assert!(out.as_ref().is_ok_and(|cs| !cs.is_empty()), "{name}: {out:?}");
    c.reads()
}

/// The rows each mutation's reads examine, counted, at two library sizes
/// sixteen times apart.
#[test]
#[ignore]
fn perf_rows_read_per_mutation() {
    eprintln!("\n== rows a mutation's reads examine (Counting store), by library size");
    note("g = Read::get calls, r = rows a scan returned (v5's store examines only what it returns)");
    eprintln!("{:<52} {:>12} {:>12}", "mutation", "N=500", "N=8000");
    for (label, name, make) in &read_cases() {
        let (small, big) = (reads_of(500, name, &**make), reads_of(8000, name, &**make));
        eprintln!(
            "{:<52} {:>12} {:>12}",
            label,
            format!("{}g {}r", small.gets, small.rows),
            format!("{}g {}r", big.gets, big.rows)
        );
    }
}

/// What the mutations read is bounded by what they need, as v4's was, at
/// any library size: `add_song` at most v4's 15 gets and one row (its
/// `first` of the file's rows and of the media by `pos` descending each one
/// bounded read), `add_to_playlist` onto a playlist of N-1 one row, the
/// 102nd "Favorites" the 101 names from "Favorites" up to "Favorites )"
/// and the last playlist by `pos` — not every playlist. Not ignored: it
/// pins the routing. Falsified by routing no `first` (add_song read 500
/// rows at 500) and by reading no bounds on the column past the lead (the
/// 102nd "Favorites" read 1,001 rows: alice's 1,000 and the last).
#[test]
fn reads_are_bounded_by_what_they_need() {
    for n in [500u64, 8000] {
        let reads: Vec<_> = read_cases().iter().map(|(_, name, make)| reads_of(n, name, &**make)).collect();
        let (song, onto, favorites) = (reads[0], reads[2], reads[4]);
        assert!(song.gets <= 15 && song.rows <= 1, "add_song at {n}: {song:?}");
        assert!(onto.rows <= 1, "add_to_playlist onto N-1 at {n}: {onto:?}");
        assert!(favorites.rows <= 102, "the 102nd Favorites at {n}: {favorites:?}");
    }
}

// Reads, and what a row costs --------------------------------------------------------

/// `library` read whole (what a hydrate pulls), with the allocations it
/// makes; what a row costs — handed out (its bytes copied) and built into
/// its struct (`Node::from_bytes`); and an `add_to_playlist` run, as a
/// rebase runs each pending intent.
#[test]
#[ignore]
fn perf_row_keys() {
    eprintln!("\n== what a Row costs");
    let (st, _) = with_playlist(2, 1);
    let media_row = st.scan("media", &Scan::all())[0].clone();
    let item_row = st.scan("playlist_item", &Scan::all())[0].clone();
    row_cost::<Media>("media", 9, &media_row);
    row_cost::<PlaylistItem>("playlist_item", 5, &item_row);
    eprintln!("`media.title` in a plan's expression: n/a: the interpreter reads a field by its span in the row's bytes; not timed alone");
    eprintln!(
        "{:<36} {:>7} {:>10} {:>12} {:>12} {:>14}",
        "read", "N", "time", "allocs", "rows out", "rows built"
    );
    for n in [500u64, 2000, 8000] {
        let (st, pl) = with_playlist(n, n / 3);
        let ctx = ctx_of(1);
        let input = Library { playlist_id: Id::new(pl) }.to_bytes();
        let c = Counting::new(&st);
        domain().ask(&c, &ctx, "library.library", &input).unwrap();
        let rows_out = c.reads().rows + c.reads().gets;
        let a0 = allocs();
        let t = Instant::now();
        let v = domain().ask(&st, &ctx, "library.library", &input).unwrap();
        let (dt, al) = (t.elapsed(), allocs() - a0);
        assert_eq!(harken_domain::view::Item::list(&v).unwrap().len(), n as usize);
        eprintln!(
            "{:<36} {:>7} {:>8.2}ms {:>12} {:>12} {:>14}",
            "library (read whole, as hydrate)",
            n,
            dt.as_secs_f64() * 1e3,
            al,
            rows_out,
            "n/a"
        );
        eprintln!(
            "{:<36} {:>7} {:>8.2}ms {:>12.0} {:>12} {:>14}",
            "  … per entry",
            "",
            dt.as_secs_f64() * 1e3 / n as f64,
            al as f64 / n as f64,
            "",
            ""
        );
        let t = Instant::now();
        std::hint::black_box(st.scan("media", &Scan::all()));
        eprintln!(
            "{:<36} {:>7} {:>8.2}ms {:>12} {:>12}",
            "  … scan(\"media\") alone",
            n,
            t.elapsed().as_secs_f64() * 1e3,
            "",
            n
        );
        let ap = on(pl, idv(1, n - 1));
        let a0 = allocs();
        let t = Instant::now();
        let out = domain().run(&st, &ctx, "playlists.add_to_playlist", &ap);
        let (dt, al) = (t.elapsed(), allocs() - a0);
        assert!(out.is_ok_and(|cs| cs.len() == 1));
        let c = Counting::new(&st);
        domain().run(&c, &ctx, "playlists.add_to_playlist", &ap).unwrap();
        let rows_out = c.reads().rows + c.reads().gets;
        eprintln!(
            "{:<36} {:>7} {:>8.2}ms {:>12} {:>12} {:>14}",
            "add_to_playlist applied (N/3 items)",
            n,
            dt.as_secs_f64() * 1e3,
            al,
            rows_out,
            "n/a"
        );
    }
}

/// A row's costs: its bytes copied (what a read hands out) and the struct
/// it is built into, in allocations and time.
fn row_cost<T: Node>(table: &str, columns: usize, row: &[u8]) {
    let reps = 100_000usize;
    let a0 = allocs();
    let t = Instant::now();
    for _ in 0..reps {
        std::hint::black_box(row.to_vec());
    }
    let (clone_t, clone_a) = (t.elapsed(), allocs() - a0);
    let a0 = allocs();
    let t = Instant::now();
    for _ in 0..reps {
        std::hint::black_box(T::from_bytes(row).unwrap());
    }
    let (built_t, built_a) = (t.elapsed(), allocs() - a0);
    let per = |d: Duration| d.as_secs_f64() * 1e9 / reps as f64;
    eprintln!(
        "a {table} row ({columns} columns): clone {:.0} ns / {:.1} allocs; built into its struct {:.0} ns / {:.1} allocs",
        per(clone_t),
        clone_a as f64 / reps as f64,
        per(built_t),
        built_a as f64 / reps as f64,
    );
}

/// v4's other harken shapes, and why they have no line here.
#[test]
#[ignore]
fn perf_not_ported() {
    eprintln!("\n== one apply, native against interpreted");
    eprintln!("n/a: v5 has one execution path, the interpreter (§1.12); the interpreted figures are add_song's and create_playlist's above");
    eprintln!("\n== (f) harken's views, the scanner's known_file, (g) search");
    eprintln!("not measured by M1: Part 3 asks for add_song, playlists, playlist_name, rows examined and allocations; the views shape is rust/ark/tests/perf.rs's");
}
