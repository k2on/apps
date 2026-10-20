//! `docs/plan-v5.md` Part 3's views shape over v4, printed rather than
//! asserted, beside `rust/ark5/tests/perf.rs`'s `perf_views` so the two
//! outputs diff: one change against a maintained view of
//! `playlist-with-items`, `top-two-by-pos` and `group-source` (the plans of
//! `spec/vectors/views`, as `src/bin/vectors/views.rs` writes them) at 1k,
//! 4k and 16k rows — the time `view::push_all` takes, the store already
//! moved, and what the push read through a counting store
//! (`tests/support/counting.rs`: gets, and rows a scan returned or a
//! filter examined).
//!
//! Beside it, one line Part 3's report needs that `tests/perf.rs` stops
//! short of: the rebase (e) at a depth of 400 pending, the same harness
//! as `perf_e_rebase` with K = 400.
//!
//! Ignored by default; run with
//! `cargo test -p ark --release --test perf_views -- --ignored --nocapture --test-threads=1`.

#[path = "support/counting.rs"]
mod counting;

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use ark::authoring::*;
use ark::eval::{self, Args, Ctx};
use ark::hash::{closure, closures, Closure, FnHash};
use ark::log::{Entry, Facts, Seq};
use ark::peer::{Authority, Procedure, Replica, Sequenced};
use ark::schema::Schema;
use ark::store::{Change, MemoryStore, Row as StoreRow, Store};
use ark::value::Value;
use ark::view::{hydrate, push_all, Env};
use counting::Counting;

// The tables ------------------------------------------------------------------------

pub struct Demo {
    pub playlist: Table<Playlist>,
    pub item: Table<Item>,
}
impl Tables for Demo {
    fn open() -> Self {
        Demo {
            playlist: table(),
            item: table(),
        }
    }
}

pub struct Lib {
    pub playlist: Table<Playlist>,
    pub item: Table<Item>,
    pub song: Table<Song>,
}
impl Tables for Lib {
    fn open() -> Self {
        Lib {
            playlist: table(),
            item: table(),
            song: table(),
        }
    }
}

pub struct Playlist {
    pub id: Id<Playlist>,
    pub name: Text,
    pub user_id: Text,
}
impl Row for Playlist {
    const NAME: &str = "playlist";
    type Key = (Id<Playlist>,);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::id)
            .text(Self::name)
            .text(Self::user_id)
            .key((Self::id,))
            .unique((Self::user_id, Self::name))
    }
}
#[allow(non_upper_case_globals)]
impl Playlist {
    pub const id: Col<Self, Id<Self>> = col("id");
    pub const name: Col<Self, Text> = col("name");
    pub const user_id: Col<Self, Text> = col("user_id");
}

pub struct Item {
    pub playlist_id: Id<Playlist>,
    pub track_id: Text,
    pub pos: Int,
}
impl Row for Item {
    const NAME: &str = "item";
    type Key = (Id<Playlist>, Text);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::playlist_id)
            .refs::<Playlist>()
            .text(Self::track_id)
            .int(Self::pos)
            .key((Self::playlist_id, Self::track_id))
            .unique((Self::playlist_id, Self::pos))
    }
}
#[allow(non_upper_case_globals)]
impl Item {
    pub const playlist_id: Col<Self, Id<Playlist>> = col("playlist_id");
    pub const track_id: Col<Self, Text> = col("track_id");
    pub const pos: Col<Self, Int> = col("pos");
}

/// The vectors' song, without the reference to a movement (no movement
/// table here; nothing below reads one).
pub struct Song {
    pub id: Text,
    pub title: Text,
    pub creator: Text,
    pub movement_id: Opt<Text>,
    pub pos: Int,
}
impl Row for Song {
    const NAME: &str = "song";
    type Key = (Text,);
    fn columns() -> Columns<Self> {
        columns()
            .text(Self::id)
            .text(Self::title)
            .text(Self::creator)
            .text(Self::movement_id)
            .nullable()
            .int(Self::pos)
            .key((Self::id,))
    }
}
#[allow(non_upper_case_globals)]
impl Song {
    pub const id: Col<Self, Text> = col("id");
    pub const title: Col<Self, Text> = col("title");
    pub const creator: Col<Self, Text> = col("creator");
    pub const movement_id: Col<Self, Opt<Text>> = col("movement_id");
    pub const pos: Col<Self, Int> = col("pos");
}

pub struct Tally {
    pub name: Text,
    pub n: Int,
}
impl Record for Tally {
    fn fields() -> Fields<Self> {
        fields().field("name", text()).field("n", int())
    }
}

pub struct PlaylistId {
    pub playlist_id: Id<Playlist>,
}
impl Input for PlaylistId {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>())
    }
}

/// The three plans, as `src/bin/vectors/views.rs` writes them.
fn views() -> Module {
    let r = router::<Lib>("views");
    Module::new((r.routes((
        r.input::<PlaylistId>().query("top-two-by-pos", |_ctx, db, input| {
            db.item.filter(Item::playlist_id.eq(input.playlist_id)).order_by(Item::pos.asc()).limit(2)
        }),
        r.query("playlist-with-items", |_ctx, db, _input: ()| {
            db.playlist
                .order_by(Playlist::name.asc())
                .each(|p, ()| db.item.order_by(Item::pos.desc()).limit(3).on(Item::playlist_id.eq(p.id)))
        }),
        r.query("group-source", |_ctx, db, _input: ()| {
            db.song.group_by(Song::creator).map(|creator, (songs,)| Tally {
                name: creator,
                n: songs.len(),
            })
        }),
    )),))
}

// Rows -------------------------------------------------------------------------------

fn idv(n: u64) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[8..].copy_from_slice(&n.to_be_bytes());
    b
}

fn row(pairs: Vec<(&str, Value)>) -> StoreRow {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn pl(n: u64, name: &str) -> Change {
    Change::Add(
        "playlist".into(),
        row(vec![
            ("id", Value::Id(idv(n))),
            ("name", Value::text(name)),
            ("user_id", Value::text("alice")),
        ]),
    )
}

fn item(n: u64, track: &str, pos: i64) -> StoreRow {
    row(vec![
        ("playlist_id", Value::Id(idv(n))),
        ("track_id", Value::text(track)),
        ("pos", Value::Int(pos)),
    ])
}

fn song(id: &str, creator: &str, pos: i64) -> StoreRow {
    row(vec![
        ("id", Value::text(id)),
        ("title", Value::text("x")),
        ("creator", Value::text(creator)),
        ("movement_id", Value::Null),
        ("pos", Value::Int(pos)),
    ])
}

fn us(d: Duration) -> f64 {
    d.as_secs_f64() * 1e6
}

/// One view of the plan `name` over `st`: hydrated, then `ops` applied to
/// the store and pushed one at a time, `rounds` times over — the mean per
/// push; and the reads one push of the first op makes, counted.
fn time_view(m: &Module, name: &str, args: Args, mut st: MemoryStore, ops: &[Change], rounds: usize) -> (Duration, f64, counting::Reads) {
    let built = m.build();
    let sch = &built.schema;
    let f = built.functions.iter().find(|f| f.name == name).unwrap_or_else(|| panic!("no plan {name}"));
    let plan = f.plan.clone().expect("a query is a plan");
    let ctx = Ctx::new("alice", "dev");
    let c = closure(built, f);
    let (args_in, provided) = eval::middleware(sch, &c, &ctx, &args, &st).expect("the middleware");
    let env = Env {
        helpers: c.helpers.clone(),
        ctx,
        args: args_in,
        provided,
    };
    let t = Instant::now();
    let mut v = hydrate(sch, &plan, env.clone(), &st).expect("hydrate");
    let hydrated = t.elapsed();
    st.apply_changes(&ops[..1]);
    let counting = Counting::new(&st);
    push_all(sch, &counting, &ops[..1], &mut v).expect("push");
    let reads = counting.reads();
    for c in &ops[1..] {
        st.apply_changes(std::slice::from_ref(c));
        push_all(sch, &st, std::slice::from_ref(c), &mut v).expect("push");
    }
    let mut spent = Duration::ZERO;
    for _ in 0..rounds {
        for c in ops {
            st.apply_changes(std::slice::from_ref(c));
            let t = Instant::now();
            push_all(sch, &st, std::slice::from_ref(c), &mut v).expect("push");
            spent += t.elapsed();
        }
    }
    assert_eq!(v.rows(), hydrate(sch, &plan, env, &st).unwrap().rows(), "the view is a fresh hydrate");
    (hydrated, us(spent) / (rounds * ops.len()) as f64, reads)
}

fn store(m: &Module, rows: &[Change]) -> MemoryStore {
    let mut st = MemoryStore::empty(m.build().schema.clone());
    st.apply_changes(rows);
    st
}

/// One change against each of three maintained views at 1k, 4k and 16k
/// rows, as `rust/ark5/tests/perf.rs`'s `perf_views`: an item added to
/// (and taken off) one of n/4 playlists of four under
/// `playlist-with-items`; the first item of a playlist of n leaving (and
/// coming back) under `top-two-by-pos`; a song changing creator (and back)
/// among n songs of eight creators under `group-source`.
#[test]
#[ignore]
fn perf_views() {
    let m = views();
    let rounds = 2000;
    eprintln!("\n== views: one change against a maintained view (View::push), the store already moved");
    eprintln!(
        "{:<24} {:>7} {:>12} {:>12} {:>8} {:>8}",
        "query", "rows", "hydrate ms", "push µs", "gets", "rows"
    );
    let print = |name: &str, n: u64, (hy, push, r): (Duration, f64, counting::Reads)| {
        eprintln!(
            "{:<24} {:>7} {:>12.2} {:>12.2} {:>8} {:>8}",
            name,
            n,
            hy.as_secs_f64() * 1e3,
            push,
            r.gets,
            r.rows
        );
    };
    for n in [1000u64, 4000, 16000] {
        let mut rows = vec![];
        for p in 0..n / 4 {
            rows.push(pl(p, &format!("p{p}")));
            rows.extend((1..=4).map(|k| Change::Add("item".into(), item(p, &format!("t{k}"), k))));
        }
        let fresh = item(n / 8, "t5", 5);
        let ops = [Change::Add("item".into(), fresh.clone()), Change::Remove("item".into(), fresh)];
        print(
            "playlist-with-items",
            n,
            time_view(&m, "playlist-with-items", Args::new(), store(&m, &rows), &ops, rounds),
        );
        let mut rows = vec![pl(1, "Viewed")];
        rows.extend((0..n).map(|i| Change::Add("item".into(), item(1, &format!("t{i}"), i as i64 + 1))));
        let first = item(1, "t0", 1);
        let ops = [Change::Remove("item".into(), first.clone()), Change::Add("item".into(), first)];
        let args = Args::from([("playlist_id".to_string(), Value::Id(idv(1)))]);
        print("top-two-by-pos", n, time_view(&m, "top-two-by-pos", args, store(&m, &rows), &ops, rounds));
        let rows: Vec<Change> = (0..n)
            .map(|i| Change::Add("song".into(), song(&format!("s{i}"), &format!("c{}", i % 8), i as i64)))
            .collect();
        let (c0, c1) = (song("s0", "c0", 0), song("s0", "c1", 0));
        let ops = [
            Change::Edit("song".into(), c0.clone(), c1.clone()),
            Change::Edit("song".into(), c1, c0),
        ];
        print("group-source", n, time_view(&m, "group-source", Args::new(), store(&m, &rows), &ops, rounds));
    }
}

// (e) at depth 400 ----------------------------------------------------------------------

pub struct CreatePlaylist {
    pub name: Text,
}
impl Input for CreatePlaylist {
    fn schema() -> Object<Self> {
        object().field("name", text().trim().min(1).why("a playlist needs a name"))
    }
}

pub struct AddToPlaylist {
    pub playlist_id: Id<Playlist>,
    pub track_id: Text,
}
impl Input for AddToPlaylist {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>().exists()).field("track_id", text().min(1))
    }
}

/// The demo of `tests/perf.rs`, as it is there.
fn demo() -> Module {
    let demo = router::<Demo>("demo");
    Module::new((demo.routes((
        demo.input::<CreatePlaylist>().mutation("create_playlist", |ctx, db, input| {
            db.playlist
                .insert(Playlist {
                    id: ctx.new_id("id"),
                    name: input.name,
                    user_id: ctx.user,
                })
                .on((Playlist::user_id, Playlist::name))
        }),
        demo.input::<AddToPlaylist>().mutation("add_to_playlist", |_ctx, db, input| {
            let last = db.item.filter(Item::playlist_id.eq(input.playlist_id)).order_by(Item::pos.desc()).first();
            db.item.insert(Item {
                playlist_id: input.playlist_id,
                track_id: input.track_id,
                pos: last.map_or(0, |row| row.pos).add(1),
            })
        }),
    )),))
}

struct Fixture {
    schema: Schema,
    bodies: BTreeMap<FnHash, Closure>,
    procs: Vec<(FnHash, Procedure)>,
    create: FnHash,
    add: FnHash,
}

fn fixture() -> Fixture {
    let m = demo();
    let procs = m.procedures();
    let hash = |n: &str| procs.iter().find(|(_, p)| p.name() == n).unwrap().0.clone();
    Fixture {
        schema: m.build().schema.clone(),
        bodies: closures(m.build()),
        create: hash("create_playlist"),
        add: hash("add_to_playlist"),
        procs,
    }
}

fn args<const N: usize>(pairs: [(&str, Value); N]) -> Args {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn plv(j: u64) -> Value {
    Value::Id(idv(1_000_000 + j))
}

impl Fixture {
    fn replica(&self) -> Replica {
        let mut r = Replica::open(
            self.schema.clone(),
            self.bodies.clone(),
            MemoryStore::empty(self.schema.clone()),
            0,
            vec![],
        );
        r.hold(self.procs.clone());
        r
    }

    fn authority(&self) -> Authority {
        let mut a = Authority::new(self.schema.clone(), self.bodies.clone());
        a.hold(self.procs.clone());
        a
    }

    fn create(&self, r: &mut Replica, ctx: &Ctx, n: u64, j: u64) -> Entry {
        r.mutate(
            idv(n),
            ctx,
            &self.create,
            &args([("id", plv(j))]),
            &args([("name", Value::text(format!("P{j}")))]),
        )
        .unwrap()
    }

    fn add(&self, r: &mut Replica, ctx: &Ctx, n: u64, j: u64, track: u64) -> Entry {
        let a = args([("playlist_id", plv(j)), ("track_id", Value::text(format!("t{track}")))]);
        r.mutate(idv(n), ctx, &self.add, &Args::new(), &a).unwrap()
    }

    fn log(&self, playlists: u64, n: u64) -> (Authority, Vec<(Seq, Entry, Facts)>) {
        let ctx = Ctx::new("alice", "dev");
        let mut author = self.replica();
        let mut a = self.authority();
        let mut out = vec![];
        let mut push = |a: &mut Authority, e: Entry| match a.sequence_entry(&e) {
            Sequenced::Appended(s, f) => out.push((s, e, f)),
            o => panic!("{o:?}"),
        };
        for j in 0..playlists {
            let e = self.create(&mut author, &ctx, j, j);
            push(&mut a, e);
            author.pending.clear();
        }
        for i in 0..n {
            let e = self.add(&mut author, &ctx, 1_000 + i, i % playlists, i);
            push(&mut a, e);
            author.pending.clear();
        }
        (a, out)
    }
}

/// `tests/perf.rs`'s `perf_e_rebase` at K = 400: K intents of this peer's
/// pending while M entries of another's land over S confirmed rows — one
/// page, one entry at a time with a pump each, and with one pump.
#[test]
#[ignore]
fn perf_e_rebase_depth() {
    let d = fixture();
    let me = Ctx::new("me", "s");
    eprintln!("\n== (e) rebase at depth 400: K pending, M landing, over S confirmed rows (tests/perf.rs perf_e_rebase, K = 400)");
    eprintln!("{:<44} {:>7} {:>10} {:>10}", "shape", "S", "per entry", "per page");
    for s in [500u64, 2000, 8000] {
        let (a0, base) = d.log(10, s);
        let k = 400u64;
        let m = 256u64;
        let mut a = a0.clone();
        let mut other = d.replica();
        for (n, e, f) in &base {
            other.receive_with(*n, e.clone(), f.clone());
        }
        other.settle();
        let them = Ctx::new("alice", "dev");
        let landing: Vec<(Seq, Entry, Facts)> = (0..m)
            .map(|i| {
                let e = d.add(&mut other, &them, 5_000_000 + i, i % 10, 900_000 + i);
                match a.sequence_entry(&e) {
                    Sequenced::Appended(n, f) => (n, e, f),
                    o => panic!("{o:?}"),
                }
            })
            .collect();
        let mine = |r: &mut Replica| {
            for i in 0..k {
                d.add(r, &me, 7_000_000 + i, 3, 800_000 + i);
            }
        };
        let caught_up = || {
            let mut r = d.replica();
            r.receive_batch(base.iter().map(|(n, e, f)| (*n, e.clone(), Some(f.clone()))));
            r
        };
        let mut r = caught_up();
        mine(&mut r);
        let t = Instant::now();
        r.receive_batch(landing.iter().map(|(n, e, f)| (*n, e.clone(), Some(f.clone()))));
        let page = t.elapsed();
        assert_eq!(r.pending.len(), k as usize);
        let mut r = caught_up();
        mine(&mut r);
        let t = Instant::now();
        for (n, e, f) in &landing {
            r.receive_with(*n, e.clone(), f.clone());
            r.settle();
        }
        let each = t.elapsed();
        assert_eq!(r.pending.len(), k as usize);
        let mut r = caught_up();
        mine(&mut r);
        let t = Instant::now();
        for (n, e, f) in &landing {
            r.receive_with(*n, e.clone(), f.clone());
        }
        r.settle();
        let pumped = t.elapsed();
        assert_eq!(r.pending.len(), k as usize);
        for (label, d) in [
            ("one entry at a time, a pump each", each),
            ("one entry at a time, one pump", pumped),
            ("one page", page),
        ] {
            eprintln!(
                "{:<44} {:>7} {:>8.1}µs {:>8.1}ms",
                format!("K={k} M={m}: {label}"),
                s,
                us(d) / m as f64,
                d.as_secs_f64() * 1e3
            );
        }
    }
}
