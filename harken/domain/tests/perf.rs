//! What harken's own mutations and reads cost as the library grows, printed
//! rather than asserted — the harken-shaped half of the performance pass
//! whose engine half is `rust/ark/tests/perf.rs`. Over the engine alone (a
//! replica and the authority it commits to, as a peer alone is), so that
//! what is measured is the domain's procedures and the store under them,
//! not a storage or a socket.
//!
//! Beside the timings, two things are counted rather than timed, because a
//! count does not move with the machine: the rows each mutation's reads
//! examine (the `Counting` store `rust/ark/tests/toggle.rs` counts with),
//! and the allocations a read makes — with a row's clone split into its
//! values and its keys, which is the measurement the question "how much of
//! a read is `Row`'s owned `String` keys" needs before anything is proposed
//! about it.
//!
//! Ignored by default; run with
//! `cargo test -p harken-domain --release --test perf -- --ignored --nocapture --test-threads=1`.

#[path = "../../../rust/ark/tests/support/counting.rs"]
mod counting;

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use ark::authoring::Procedure;
use ark::eval::{Args, Ctx};
use ark::hash::{closures, FnHash};
use ark::ir::Auto;
use ark::log::{snapshot_of, Log};
use ark::peer::{local_commit, Authority, Replica};
use ark::schema::Schema;
use ark::store::{Change, MemoryStore, Overlay, Row, Store};
use ark::value::Value;
use counting::Counting;

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

// The library, as a peer alone holds it ---------------------------------------

fn args<const N: usize>(pairs: [(&str, Value); N]) -> Args {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn idv(tag: u8, n: u64) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[0] = tag;
    b[8..].copy_from_slice(&n.to_be_bytes());
    b
}

struct Harken {
    schema: Schema,
    procs: BTreeMap<String, (FnHash, Procedure)>,
    bodies: BTreeMap<FnHash, ark::hash::Closure>,
}

fn harken() -> Harken {
    let m = harken_domain::module();
    Harken {
        schema: m.build().schema.clone(),
        bodies: closures(m.build()),
        procs: m.procedures().into_iter().map(|(h, p)| (p.name().to_string(), (h, p))).collect(),
    }
}

/// A media row, as `add_song` writes one.
fn media(i: u64) -> Row {
    [
        ("id", Value::Id(idv(1, i))),
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

/// A peer alone over a confirmed store, and its authority: a replica, and
/// an authority whose log stands on that store as its snapshot.
struct Alone {
    a: Authority,
    r: Replica,
    ctx: Ctx,
    next: u64,
}

impl Harken {
    fn alone(&self, confirmed: MemoryStore) -> Alone {
        let mut a = Authority::new(self.schema.clone(), self.bodies.clone());
        a.log = Log {
            base: snapshot_of(0, confirmed.clone()),
            entries: BTreeMap::new(),
            ids: BTreeMap::new(),
        };
        a.store = confirmed.clone();
        let natives: Vec<_> = self.procs.values().cloned().collect();
        a.hold(natives.clone());
        let mut r = Replica::open(self.schema.clone(), self.bodies.clone(), confirmed, 0, vec![]);
        r.hold(natives);
        Alone {
            a,
            r,
            ctx: Ctx::new("alice", "local"),
            next: 0,
        }
    }

    /// A library of `n` media rows, and nothing else.
    fn library(&self, n: u64) -> MemoryStore {
        let mut st = MemoryStore::empty(self.schema.clone());
        for i in 0..n {
            st.apply_change(&Change::Add("media".into(), media(i)));
        }
        st
    }

    /// A library of `n` media rows, and one playlist of alice's holding the
    /// first `items` of them — written as rows, so that a size is not paid
    /// for by the quadratic append it measures elsewhere.
    fn with_playlist(&self, n: u64, items: u64) -> (MemoryStore, Value) {
        let mut st = self.library(n);
        let pl = Value::Id(idv(2, 1));
        let row = |pairs: Vec<(&str, Value)>| -> Row { pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect() };
        st.apply_change(&Change::Add(
            "playlist".into(),
            row(vec![
                ("id", pl.clone()),
                ("name", Value::text("Mine")),
                ("pos", Value::int(1)),
                ("created_ms", Value::int(1)),
                ("user_id", Value::text("alice")),
            ]),
        ));
        for i in 0..items {
            st.apply_change(&Change::Add(
                "playlist_item".into(),
                row(vec![
                    ("playlist_id", pl.clone()),
                    ("media_id", Value::Id(idv(1, i))),
                    ("pos", Value::int(i as i64 + 1)),
                    ("added_ms", Value::int(1)),
                    ("user_id", Value::text("alice")),
                ]),
            ));
        }
        (st, pl)
    }

    /// A library of `n` media rows, and a thousand playlists each for
    /// alice and bob: alice's are "Favorites", a hundred "Favorites (k)"
    /// and 899 others, bob's a thousand "Favorites (k)" — what naming a
    /// person's next "Favorites" reads a range of (`docs/plan-perf.md` R6).
    fn with_playlists(&self, n: u64) -> MemoryStore {
        let mut st = self.library(n);
        let alice = std::iter::once("Favorites".to_string())
            .chain((1..=100).map(|k| format!("Favorites ({k})")))
            .chain((0..899).map(|k| format!("List {k}")))
            .map(|name| ("alice", name));
        let bob = (1..=1000).map(|k| ("bob", format!("Favorites ({k})")));
        for (i, (user, name)) in alice.chain(bob).enumerate() {
            let row: Row = [
                ("id", Value::Id(idv(3, i as u64))),
                ("name", Value::text(name)),
                ("pos", Value::int(i as i64 + 1)),
                ("created_ms", Value::int(1)),
                ("user_id", Value::text(user)),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
            st.apply_change(&Change::Add("playlist".into(), row));
        }
        st
    }

    fn autos(&self, name: &str, next: &mut u64) -> Args {
        self.procs[name]
            .1
            .function()
            .autos
            .iter()
            .map(|(n, a)| {
                *next += 1;
                let v = match a {
                    Auto::NewId(_) => Value::Id(idv(9, *next)),
                    Auto::Now => Value::int(*next as i64),
                };
                (n.clone(), v)
            })
            .collect()
    }
}

impl Alone {
    /// Author and commit, as `ark_client::Peer::mutate` does alone: the
    /// optimistic apply, and the commit (authority + confirmed), timed
    /// apart.
    fn mutate(&mut self, h: &Harken, name: &str, a: Args) -> (Duration, Duration) {
        let autos = h.autos(name, &mut self.next);
        self.next += 1;
        let id = idv(7, self.next);
        let t = Instant::now();
        self.r
            .mutate(id, &self.ctx, &h.procs[name].0, &autos, &a)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let t1 = Instant::now();
        local_commit(&mut self.a, &mut self.r);
        let t2 = Instant::now();
        assert!(self.r.pending.is_empty(), "{name}: {:?}", self.r.rejections);
        (t1 - t, t2 - t1)
    }
}

fn song(i: u64) -> Args {
    args([
        ("title", Value::text(format!("Track {i}"))),
        ("artist", Value::text(format!("Artist {}", i % 50))),
        ("album", Value::text(format!("Album {}", i % 200))),
        ("duration_ms", Value::int(180_000)),
        ("file", Value::text(format!("music/new/{i}.flac"))),
        ("track", Value::int((i % 12) as i64 + 1)),
        ("part", Value::text("")),
        ("catalogue", Value::text("")),
        ("performer", Value::text("")),
        ("bpm", Value::int(0)),
        ("album_art", Value::text("")),
        ("artist_art", Value::text("")),
        ("disc", Value::int(0)),
        ("work_title", Value::text("")),
        ("movement_no", Value::int(0)),
    ])
}

// Reporting ----------------------------------------------------------------------

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

// The mutations ----------------------------------------------------------------------

/// `add_song` into a library that grows from nothing, as the scanner
/// authors one per file.
#[test]
#[ignore]
fn perf_add_song() {
    let h = harken();
    header("add_song, one per file, into a library growing from 0 (peer alone, engine)");
    for n in [500u64, 2000, 8000] {
        let mut p = h.alone(MemoryStore::empty(h.schema.clone()));
        let (mut whole, mut opt, mut commit) = (vec![], vec![], vec![]);
        for i in 0..n {
            let (o, c) = p.mutate(&h, "add_song", song(i));
            opt.push(o);
            commit.push(c);
            whole.push(o + c);
        }
        assert_eq!(p.r.view.scan("media").len(), n as usize);
        line("add_song: whole", &whole);
        line("  … Replica::mutate (optimistic)", &opt);
        line("  … local_commit (authority + confirm)", &commit);
    }
}

/// `add_to_playlist` onto one playlist that grows to N, and onto N/10
/// playlists of ten, over a library of N media; and `create_playlist` for
/// one person, with names that differ and with one name every time (the
/// default every device makes, which `free_number` numbers).
#[test]
#[ignore]
fn perf_playlists() {
    let h = harken();
    header("add_to_playlist over a library of N (peer alone, engine)");
    for n in [500u64, 2000, 8000] {
        for (shape, lists) in [("one playlist", 1u64), ("playlists of 10", n / 10)] {
            let mut p = h.alone(h.library(n));
            let mut ids = vec![];
            for j in 0..lists {
                p.mutate(&h, "create_playlist", args([("name", Value::text(format!("P{j}")))]));
                ids.push(newest_playlist(&p.r.view));
            }
            let (mut whole, mut opt, mut commit) = (vec![], vec![], vec![]);
            for i in 0..n {
                let a = args([("playlist_id", ids[(i % lists) as usize].clone()), ("media_id", Value::Id(idv(1, i)))]);
                let (o, c) = p.mutate(&h, "add_to_playlist", a);
                opt.push(o);
                commit.push(c);
                whole.push(o + c);
            }
            line(&format!("add_to_playlist, {shape}: whole"), &whole);
            line("  … Replica::mutate (optimistic)", &opt);
            line("  … local_commit (authority + confirm)", &commit);
        }
    }
    header("create_playlist, one person's P playlists (peer alone, engine)");
    for n in [50u64, 200, 800] {
        for (shape, same) in [("distinct names", false), ("all \"Favorites\"", true)] {
            let mut p = h.alone(MemoryStore::empty(h.schema.clone()));
            let per: Vec<Duration> = (0..n)
                .map(|j| {
                    let name = if same { "Favorites".to_string() } else { format!("List {j}") };
                    let (o, c) = p.mutate(&h, "create_playlist", args([("name", Value::text(name))]));
                    o + c
                })
                .collect();
            line(&format!("create_playlist, {shape}"), &per);
        }
    }
}

/// `playlist_name` alone, natively, among one person's P playlists: a
/// name they do not have (one `contains`, since R1 made the branch lazy)
/// and one they do (`free_number`, quadratic in P). The number
/// `free_number`'s doc comment quotes at a thousand.
#[test]
#[ignore]
fn perf_playlist_name() {
    use ark::authoring::{evaluate, list, Text};
    eprintln!("\n== playlist_name among P names (native, harken_domain::playlists)");
    eprintln!("{:<40} {:>7} {:>12}", "name", "P", "µs per call");
    for p in [10usize, 100, 1000] {
        let names: Vec<String> = std::iter::once("Favorites".to_string())
            .chain((1..p).map(|i| format!("List {i}")))
            .collect();
        for (label, name) in [("a free name", "Night"), ("a taken name", "Favorites")] {
            let reps = if p >= 1000 { 5 } else { 50 };
            let t = Instant::now();
            for _ in 0..reps {
                let v = evaluate(|| {
                    let of = list(names.iter().map(|n| Text::from(n.as_str())).collect::<Vec<_>>());
                    harken_domain::playlists::playlist_name(of, Text::from(name))
                })
                .unwrap();
                std::hint::black_box(v);
            }
            eprintln!("{:<40} {:>7} {:>12.1}", label, p, us(t.elapsed()) / reps as f64);
        }
    }
}

fn newest_playlist(st: &MemoryStore) -> Value {
    st.scan("playlist")
        .into_iter()
        .max_by_key(|r| r["pos"].as_int())
        .map(|r| r["id"].clone())
        .expect("a playlist")
}

type Make<'a> = Box<dyn Fn(u64) -> (MemoryStore, Args) + 'a>;

/// The mutations whose reads R1 is about, each with the store and the
/// arguments it is applied to at a library of `n`.
fn read_cases(h: &Harken) -> Vec<(&'static str, &'static str, Make<'_>)> {
    let onto = |filled: bool| {
        move |n: u64| {
            let (st, pl) = h.with_playlist(n, if filled { n - 1 } else { 0 });
            (st, args([("playlist_id", pl), ("media_id", Value::Id(idv(1, n - 1)))]))
        }
    };
    vec![
        ("add_song (a new file)", "add_song", Box::new(|n| (h.library(n), song(99_999_999)))),
        ("add_to_playlist (an empty playlist)", "add_to_playlist", Box::new(onto(false))),
        ("add_to_playlist (a playlist of N-1)", "add_to_playlist", Box::new(onto(true))),
        (
            "create_playlist (alice's second)",
            "create_playlist",
            Box::new(|n| (h.with_playlist(n, 0).0, args([("name", Value::text("Other"))]))),
        ),
        (
            "create_playlist (a 102nd Favorites, of 1,000)",
            "create_playlist",
            Box::new(|n| (h.with_playlists(n), args([("name", Value::text("Favorites"))]))),
        ),
    ]
}

/// What one mutation's reads examine, applied once through a counting
/// store over a peer alone at a library of `n`.
fn reads_of(h: &Harken, n: u64, name: &str, make: &dyn Fn(u64) -> (MemoryStore, Args)) -> counting::Reads {
    let (st, a) = make(n);
    let p = h.alone(st);
    let autos = h.autos(name, &mut 1_000_000);
    let c = Counting::new(&p.r.view);
    let mut ov = Overlay::new(&c);
    let out = h.procs[name].1.apply(&p.ctx, &autos, &a, &mut ov);
    assert!(matches!(out, Ok(Ok(_))), "{name}: {out:?}");
    c.reads()
}

/// The rows each mutation's reads examine, counted, at two library sizes
/// sixteen times apart: a mutation whose reads go through an index reads
/// the same number at both; one that scans reads sixteen times as many.
#[test]
#[ignore]
fn perf_rows_read_per_mutation() {
    let h = harken();
    eprintln!("\n== rows a mutation's reads examine (Counting store), by library size");
    eprintln!("{:<52} {:>12} {:>12}", "mutation", "N=500", "N=8000");
    for (label, name, make) in &read_cases(&h) {
        let (small, big) = (reads_of(&h, 500, name, &**make), reads_of(&h, 8000, name, &**make));
        eprintln!(
            "{:<52} {:>12} {:>12}",
            label,
            format!("{}g {}r", small.gets, small.rows),
            format!("{}g {}r", big.gets, big.rows)
        );
    }
}

/// The guard for `docs/plan-perf.md` R1, run with the suite rather than
/// printed: every read these mutations make is served by an index, so the
/// rows they examine are a handful at a library of 8,000 and the same at
/// 500. At 8,000: `add_song` examines one row (the last by `pos`; the file
/// is new, so its lookup finds none) and fourteen gets; `add_to_playlist`
/// onto a playlist of 7,999 examines one (its last item) and seven gets,
/// onto an empty one none; `create_playlist` two (the last playlist, and
/// alice's one other) and two gets. Before the indexes these were 16,000,
/// 7,999 and 2 rows (the harness's round-1 rows, `docs/plan-perf.md`).
/// Falsified by taking `.index((Self::playlist_id, Self::pos))` off
/// `playlist_item`: the playlist of 7,999 is 7,999 rows again; and by
/// taking `.index((Self::file,))` off `media`: `add_song` is 8,001.
///
/// R6: `create_playlist` now reads only the names from its own up to its
/// numbered siblings, so alice's second is one row (the last playlist;
/// "Mine" is not near "Other"), and her next "Favorites" among a thousand
/// of hers — a hundred of them "Favorites (k)" — and a thousand of bob's
/// "Favorites (k)" is 102: the last playlist, "Favorites" and its hundred
/// siblings, where reading all of hers was 1,001. It is named
/// "Favorites (101)". Falsified by dropping the span from `view::read`
/// (`&[]` for `&spans`): alice's second is two rows again and the 102nd
/// Favorites 1,001; and by ending the range at `name (` in
/// `create_playlist`: the siblings go unread and it names the playlist
/// "Favorites (1)", which she has — a unique violation.
#[test]
fn a_mutation_examines_the_rows_it_needs_not_the_library() {
    let h = harken();
    let want = [(14, 1), (7, 0), (7, 1), (2, 1), (2, 102)];
    for ((label, name, make), (gets, rows)) in read_cases(&h).iter().zip(want) {
        let (small, big) = (reads_of(&h, 500, name, &**make), reads_of(&h, 8000, name, &**make));
        assert_eq!((big.gets, big.rows), (gets, rows), "{label} at 8,000");
        assert_eq!(small, big, "{label}: the same at 500 as at 8,000");
    }
    // And the range named it as reading everything would have.
    let p = h.alone(h.with_playlists(10));
    let mut ov = Overlay::new(&p.r.view);
    let autos = h.autos("create_playlist", &mut 1_000_000);
    let out = h.procs["create_playlist"]
        .1
        .apply(&p.ctx, &autos, &args([("name", Value::text("Favorites"))]), &mut ov);
    assert!(matches!(out, Ok(Ok(_))), "{out:?}");
    let made: Vec<Value> = ov
        .scan("playlist")
        .into_iter()
        .filter(|r| p.r.view.get("playlist", &[r["id"].clone()]).is_none())
        .map(|r| r["name"].clone())
        .collect();
    assert_eq!(made, [Value::text("Favorites (101)")]);
}

// Reads, and what a row's keys cost ---------------------------------------------------

/// `library` read whole (what a hydrate pulls), with the allocations it
/// makes, beside what a `Row` costs to copy: the whole row, its values
/// alone, and so its keys and its map. The rows the store hands out are a
/// floor on the rows copied (the pull copies each again into its node and
/// its fields), so the key share printed is a lower bound.
#[test]
#[ignore]
fn perf_row_keys() {
    let h = harken();
    eprintln!("\n== what a Row's owned keys cost");
    let (st, _) = h.with_playlist(2, 1);
    let media_row = st.scan("media")[0].clone();
    let item_row = st.scan("playlist_item")[0].clone();
    let km = keys_of("media", &media_row);
    let ki = keys_of("playlist_item", &item_row);

    // One field of a row read by a plan's expression: `media.title` with
    // `media` bound, as `library_entry` reads each of its ten. The
    // interpreter evaluates the variable — a copy of the whole row, keys
    // and all — and then takes the one field out of the copy.
    let module = harken_domain::module();
    let m = module.build();
    let (ctx, none) = (Ctx::default(), Args::new());
    let scope = ark::eval::Scope::new(&m.schema, &[], &ctx, &none, &none);
    let mut node = scope.node();
    node.bind_row_owned(0, media_row.clone());
    let read = ark::ir::Expr::Field(Box::new(ark::ir::Expr::Var(0)), "title".into());
    let reps = 100_000;
    let a0 = allocs();
    let t = Instant::now();
    for _ in 0..reps {
        std::hint::black_box(node.eval(&read).unwrap());
    }
    eprintln!(
        "`media.title` in a plan's expression: {:.0} ns, {:.1} allocs (the row's clone is 15, the title's 1)",
        us(t.elapsed()) * 1e3 / reps as f64,
        (allocs() - a0) as f64 / reps as f64
    );

    eprintln!(
        "{:<36} {:>7} {:>10} {:>12} {:>12} {:>14}",
        "read", "N", "time", "allocs", "rows out", "key share ≥"
    );
    for n in [500u64, 2000, 8000] {
        let (st, pl) = h.with_playlist(n, n / 3);
        let p = h.alone(st);
        let q = &h.procs["library"].1;
        let a = args([("playlist_id", pl.clone())]);
        let c = Counting::new(&p.r.view);
        let _ = q.query(&p.ctx, &a, &c).unwrap();
        let rows_out = c.reads().rows + c.reads().gets;
        let a0 = allocs();
        let t = Instant::now();
        let v = q.query(&p.ctx, &a, &p.r.view).unwrap();
        let (dt, al) = (t.elapsed(), allocs() - a0);
        assert_eq!(v.as_list().len(), n as usize);
        // The store hands out each media row once and each item it finds
        // once; each is one clone of its keys and its map. A floor: the
        // pull copies a row again into its node and its fields.
        let floor = n as f64 * km + (rows_out as f64 - n as f64) * ki;
        eprintln!(
            "{:<36} {:>7} {:>8.2}ms {:>12} {:>12} {:>13.0}%",
            "library (read whole, as hydrate)",
            n,
            dt.as_secs_f64() * 1e3,
            al,
            rows_out,
            100.0 * floor / al as f64
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
        std::hint::black_box(p.r.view.scan("media"));
        eprintln!(
            "{:<36} {:>7} {:>8.2}ms {:>12} {:>12}",
            "  … scan(\"media\") alone",
            n,
            t.elapsed().as_secs_f64() * 1e3,
            "",
            n
        );
        // A replay's apply: the next item of that playlist, as a rebase
        // applies each pending intent.
        let autos = h.autos("add_to_playlist", &mut 5_000_000);
        let ap = args([("playlist_id", pl.clone()), ("media_id", Value::Id(idv(1, n - 1)))]);
        let a0 = allocs();
        let t = Instant::now();
        let mut ov = Overlay::new(&p.r.view);
        let out = h.procs["add_to_playlist"].1.apply(&p.ctx, &autos, &ap, &mut ov);
        let (dt, al) = (t.elapsed(), allocs() - a0);
        assert!(matches!(out, Ok(Ok(_))));
        let c = Counting::new(&p.r.view);
        let mut ov = Overlay::new(&c);
        let _ = h.procs["add_to_playlist"].1.apply(&p.ctx, &autos, &ap, &mut ov);
        let rows_out = c.reads().rows + c.reads().gets;
        eprintln!(
            "{:<36} {:>7} {:>8.2}ms {:>12} {:>12} {:>13.0}%",
            "add_to_playlist applied (N/3 items)",
            n,
            dt.as_secs_f64() * 1e3,
            al,
            rows_out,
            100.0 * rows_out as f64 * ki / al as f64
        );
    }
}

/// A row's clone, split: the allocations and time of the whole row, of its
/// values alone, and so of its keys and its map.
fn keys_of(table: &str, row: &Row) -> f64 {
    let reps = 100_000usize;
    let a0 = allocs();
    let t = Instant::now();
    for _ in 0..reps {
        std::hint::black_box(row.clone());
    }
    let (whole_t, whole_a) = (t.elapsed(), allocs() - a0);
    let a0 = allocs();
    let t = Instant::now();
    for _ in 0..reps {
        std::hint::black_box(row.values().cloned().collect::<Vec<Value>>());
    }
    let (vals_t, vals_a) = (t.elapsed(), allocs() - a0);
    let per = |d: Duration| d.as_secs_f64() * 1e9 / reps as f64;
    let (row_allocs, val_allocs) = (whole_a as f64 / reps as f64, vals_a as f64 / reps as f64 - 1.0);
    let keys = row_allocs - val_allocs;
    eprintln!(
        "a {table} row ({} columns): clone {:.0} ns / {:.1} allocs; its values alone {:.0} ns / {:.1} allocs; so its keys and map {:.1} allocs, ~{:.0} ns ({:.0}% of the clone's time)",
        row.len(),
        per(whole_t),
        row_allocs,
        per(vals_t),
        val_allocs,
        keys,
        per(whole_t) - per(vals_t),
        100.0 * (per(whole_t) - per(vals_t)) / per(whole_t)
    );
    keys
}

/// What the scanner does per look to decide which files it already has
/// (`harken-server`'s `known_file`): since R4 one probe of `media (file)`
/// per file the look names, where it was the whole media table read into a
/// set once per look. Timed for a look at one new file (what the watch
/// sends) and for a full rescan of every file the library has; the set,
/// as it was, beside them.
#[test]
#[ignore]
fn perf_scanner_known_files() {
    let h = harken();
    eprintln!("\n== the scanner's known_file: one probe per file the look names");
    eprintln!(
        "{:>7} {:>22} {:>22} {:>22}",
        "files", "one new file, ms", "full rescan, ms", "the set (before), ms"
    );
    for n in [500u64, 2000, 8000] {
        let st = h.library(n);
        let known = |file: &str| {
            let f = Value::text(file);
            !st.scan_where_eq("media", &[("file", &f)], &[], &|r| r.get("file") == Some(&f)).is_empty()
        };
        let t = Instant::now();
        assert!(!known("music/new/0.flac"));
        let one = t.elapsed();
        let files: Vec<String> = (0..n).map(|i| format!("music/a{}/t{i}.flac", i % 50)).collect();
        let t = Instant::now();
        assert!(files.iter().all(|f| known(f)));
        let all = t.elapsed();
        let t = Instant::now();
        let set: std::collections::BTreeSet<String> = st
            .scan("media")
            .into_iter()
            .filter_map(|r| match r.get("file") {
                Some(Value::Text(f)) if !f.is_empty() => Some(f.clone()),
                _ => None,
            })
            .collect();
        let before = t.elapsed();
        assert_eq!(set.len(), n as usize);
        eprintln!(
            "{:>7} {:>22.4} {:>22.2} {:>22.2}",
            n,
            one.as_secs_f64() * 1e3,
            all.as_secs_f64() * 1e3,
            before.as_secs_f64() * 1e3
        );
    }
}

// (f) The views, engine-side --------------------------------------------------------

/// A library of `n` songs written by `add_song` itself, straight onto a
/// store: a quarter of them Bach's, catalogued, in fifty works — so one
/// artist, one composer and their works are groups that grow with the
/// library — and the rest spread over fifty artists and two hundred albums.
fn seeded(h: &Harken, n: u64) -> MemoryStore {
    let mut st = MemoryStore::empty(h.schema.clone());
    let ctx = Ctx::new("library", "scan");
    let mut next = 0;
    for i in 0..n {
        let a = seed_song(i);
        let autos = h.autos("add_song", &mut next);
        let out = h.procs["add_song"].1.apply(&ctx, &autos, &a, &mut st);
        assert!(matches!(out, Ok(Ok(_))), "{out:?}");
    }
    st
}

fn seed_song(i: u64) -> Args {
    let bach = i.is_multiple_of(4);
    let mut a = song(i);
    if bach {
        let w = (i / 4) % 50;
        for (k, v) in [
            ("artist", Value::text("Johann Sebastian Bach")),
            ("album", Value::text(format!("Bach {}", w % 10))),
            ("catalogue", Value::text(format!("BWV {w}"))),
            ("work_title", Value::text(format!("Work {w}"))),
            ("movement_no", Value::int((i / 200) as i64 + 1)),
            ("performer", Value::text(format!("Ensemble {}", i % 3))),
        ] {
            a.insert(k.into(), v);
        }
    }
    a
}

/// Every query the desktop holds open, hydrated over the library `n` songs
/// long, and then two changes pushed through each: a playlist toggle and a
/// new song by the library's largest artist (Bach, a quarter of it). The
/// medians of five of each. What `harken-iced`'s `bench_views` measures
/// through `ark_client::View`, here through `ark::view` directly and over
/// a library whose groups grow with it.
#[test]
#[ignore]
fn perf_f_views() {
    let h = harken();
    for n in [500u64, 2000] {
        let t0 = Instant::now();
        let mut st = seeded(&h, n);
        let seed = t0.elapsed();
        let me = Ctx::new("alice", "s");
        let mut next = 50_000_000;
        let mut run = |st: &mut MemoryStore, name: &str, a: Args| -> Vec<Change> {
            let autos = h.autos(name, &mut next);
            match h.procs[name].1.apply(&me, &autos, &a, st) {
                Ok(Ok(chs)) => chs,
                o => panic!("{name}: {o:?}"),
            }
        };
        run(&mut st, "create_playlist", args([("name", Value::text("Favorites"))]));
        let pl = newest_playlist(&st);
        let bach: Vec<Value> = st
            .scan("media")
            .into_iter()
            .filter(|r| r["creator"] == Value::text("Johann Sebastian Bach"))
            .map(|r| r["id"].clone())
            .collect();
        for m in bach.iter().step_by(3) {
            run(&mut st, "add_to_playlist", args([("playlist_id", pl.clone()), ("media_id", m.clone())]));
        }
        let query = |name: &str, a: Args| h.procs[name].1.query(&me, &a, &st).unwrap().as_list();
        let t = Value::text;
        let work = query("works", args([("composer", t("Johann Sebastian Bach"))]))[0].field("id");
        let recording = query("recordings", args([("work_id", work.clone())]))[0].field("id");
        let on = |k: &str, v: Value| args([("playlist_id", pl.clone()), (k, v)]);
        let queries: Vec<(&str, Args)> = vec![
            ("library", args([("playlist_id", pl.clone())])),
            ("playlists", args([])),
            ("albums", args([])),
            ("artists", args([])),
            ("composers", args([])),
            ("track_details", args([])),
            ("album", on("name", t("Bach 0"))),
            ("artist", on("name", t("Johann Sebastian Bach"))),
            ("works", args([("composer", t("Johann Sebastian Bach"))])),
            ("work", args([("id", work.clone())])),
            ("recordings", args([("work_id", work)])),
            ("credits", args([("recording_id", recording.clone())])),
            ("recording", on("id", recording)),
            ("playlist", args([("playlist_id", pl.clone())])),
            ("playlists_of", args([("media_id", bach[0].clone())])),
        ];
        let mut views: Vec<(&str, ark::view::View, Duration)> = queries
            .into_iter()
            .map(|(name, a)| {
                let c = &h.bodies[&h.procs[name].0];
                let t = Instant::now();
                let (args, provided) = ark::eval::middleware(&h.schema, c, &me, &a, &st).unwrap();
                let env = ark::view::Env {
                    helpers: c.helpers.clone(),
                    ctx: me.clone(),
                    args,
                    provided,
                };
                let v = ark::view::hydrate(&h.schema, c.function.plan.as_ref().unwrap(), env, &st).unwrap();
                (name, v, t.elapsed())
            })
            .collect();
        let mut toggle: Vec<Vec<Duration>> = vec![vec![]; views.len()];
        let mut added: Vec<Vec<Duration>> = vec![vec![]; views.len()];
        for round in 0..5u64 {
            let m = bach[1 + round as usize * 3].clone();
            let chs = run(&mut st, "add_to_playlist", args([("playlist_id", pl.clone()), ("media_id", m)]));
            for (i, (_, v, _)) in views.iter_mut().enumerate() {
                let t = Instant::now();
                ark::view::push_all(&h.schema, &st, &chs, v).unwrap();
                toggle[i].push(t.elapsed());
            }
            let chs = run(&mut st, "add_song", seed_song(4 * (1_000_000 + round)));
            for (i, (_, v, _)) in views.iter_mut().enumerate() {
                let t = Instant::now();
                ark::view::push_all(&h.schema, &st, &chs, v).unwrap();
                added[i].push(t.elapsed());
            }
        }
        let med = |xs: &mut Vec<Duration>| {
            xs.sort();
            xs[xs.len() / 2]
        };
        eprintln!(
            "\n== (f) harken's views over {n} songs (seeded in {:.1?}): hydrate, then one change each",
            seed
        );
        eprintln!("{:>14} {:>7} {:>10} {:>12} {:>14}", "query", "rows", "hydrate", "toggle", "add Bach song");
        for (i, (name, v, hy)) in views.iter().enumerate() {
            eprintln!(
                "{:>14} {:>7} {:>8.2}ms {:>10.1}µs {:>12.1}µs",
                name,
                v.rows().len(),
                hy.as_secs_f64() * 1e3,
                us(med(&mut toggle[i])),
                us(med(&mut added[i]))
            );
        }
    }
}

/// The same entry applied natively and through the interpreter, over the
/// same store: `create_playlist` for somebody who has P playlists already,
/// and `add_song` into an empty library with short names and with long
/// ones. The two must agree on what they write (the agreement tests hold
/// that); what they cost need not be the same, and where native is the
/// slower it is doing work the interpreter skips.
#[test]
#[ignore]
fn perf_native_vs_interpreted() {
    let h = harken();
    let me = Ctx::new("alice", "s");
    eprintln!("\n== one apply, native against interpreted");
    eprintln!("{:<52} {:>12} {:>14}", "entry", "native", "interpreted");
    let both = |label: &str, st: &MemoryStore, name: &str, a: &Args| {
        let autos = h.autos(name, &mut 77_000_000);
        let (p, c) = (&h.procs[name].1, &h.bodies[&h.procs[name].0]);
        let reps = 5;
        let t = Instant::now();
        for _ in 0..reps {
            let mut ov = Overlay::new(st);
            assert!(matches!(p.apply(&me, &autos, a, &mut ov), Ok(Ok(_))));
        }
        let native = t.elapsed() / reps;
        let t = Instant::now();
        for _ in 0..reps {
            let mut ov = Overlay::new(st);
            assert!(matches!(ark::eval::apply_closure(&h.schema, c, &me, &autos, a, &mut ov), Ok(Ok(_))));
        }
        let interp = t.elapsed() / reps;
        eprintln!("{:<52} {:>10.1}µs {:>12.1}µs", label, us(native), us(interp));
    };
    for p in [50u64, 200, 800] {
        let mut st = MemoryStore::empty(h.schema.clone());
        for j in 0..p {
            let row: Row = [
                ("id", Value::Id(idv(3, j))),
                ("name", Value::text(format!("List {j}"))),
                ("pos", Value::int(j as i64 + 1)),
                ("created_ms", Value::int(1)),
                ("user_id", Value::text("alice")),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
            st.apply_change(&Change::Add("playlist".into(), row));
        }
        both(
            &format!("create_playlist, a new name, P={p}"),
            &st,
            "create_playlist",
            &args([("name", Value::text("Fresh"))]),
        );
        both(
            &format!("create_playlist, a taken name, P={p}"),
            &st,
            "create_playlist",
            &args([("name", Value::text("List 7"))]),
        );
    }
    let st = MemoryStore::empty(h.schema.clone());
    both("add_song, empty library, short names", &st, "add_song", &song(1));
    let mut long = song(2);
    for k in ["title", "artist", "album", "performer"] {
        long.insert(
            k.into(),
            Value::text(format!("{k} {}", "Lorem ipsum dolor sit amet consectetur ".repeat(2))),
        );
    }
    both("add_song, empty library, 80-character names", &st, "add_song", &long);
    let mut classical = seed_song(0);
    classical.insert("file".into(), Value::text("music/x.flac"));
    both("add_song, empty library, a catalogued work", &st, "add_song", &classical);
}
