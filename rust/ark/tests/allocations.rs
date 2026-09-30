//! What the interpreter allocates, counted rather than timed, because a
//! count does not move with the machine (`docs/plan-perf.md` R5). Two
//! guards: a `library` entry hydrated through its plan — every query's plan
//! is interpreted, natively run or not — and a `map`/`filter` over a local
//! list, which must cost allocations linear in the list.
//!
//! The allocator counts per thread, so the suite's other tests running
//! beside these on other threads are not in the count.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::collections::BTreeMap;

use ark::authoring::*;
use ark::eval::{self, Args};
use ark::ir::{self, CmpOp, Expr, FnKind, Stmt};
use ark::schema::{Schema, Ty};
use ark::store::{Change, MemoryStore, Row as StoreRow, Store as _};
use ark::value::Value;

// Counting allocations, on this thread --------------------------------------

struct Tally;

thread_local! {
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}

fn count() {
    // `try_with`: an allocation while a thread's locals are torn down is
    // not counted rather than a panic inside the allocator.
    let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
}

unsafe impl GlobalAlloc for Tally {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        count();
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        count();
        System.realloc(p, l, n)
    }
}

#[global_allocator]
static GLOBAL: Tally = Tally;

/// The allocations `f` makes on this thread, and what it returned.
fn counted<T>(f: impl FnOnce() -> T) -> (usize, T) {
    let a0 = ALLOCS.with(Cell::get);
    let out = f();
    (ALLOCS.with(Cell::get) - a0, out)
}

// harken's library, as its domain authors it -----------------------------------
//
// The tables and the query `harken/domain` declares, copied rather than
// depended on: the engine knows nothing of any domain, and the shape — a
// nine-column row bound in a plan, one related plan under it, a helper
// reading ten fields of the row into the node — is what is held here.

pub struct Lib {
    pub media: Table<Media>,
    pub playlist: Table<Playlist>,
    pub playlist_item: Table<PlaylistItem>,
}
impl Tables for Lib {
    fn open() -> Self {
        Lib {
            media: table(),
            playlist: table(),
            playlist_item: table(),
        }
    }
}

pub struct Media {
    pub id: Id<Media>,
    pub kind: Text,
    pub title: Text,
    pub creator: Text,
    pub duration_ms: Int,
    pub file: Text,
    pub pos: Int,
    pub added_ms: Int,
    pub user_id: Text,
}
impl Row for Media {
    const NAME: &str = "media";
    type Key = (Id<Media>,);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::id)
            .text(Self::kind)
            .text(Self::title)
            .text(Self::creator)
            .int(Self::duration_ms)
            .text(Self::file)
            .int(Self::pos)
            .int(Self::added_ms)
            .text(Self::user_id)
            .key((Self::id,))
            .index((Self::file,))
            .index((Self::pos,))
    }
}
#[allow(non_upper_case_globals)]
impl Media {
    pub const id: Col<Self, Id<Self>> = col("id");
    pub const kind: Col<Self, Text> = col("kind");
    pub const title: Col<Self, Text> = col("title");
    pub const creator: Col<Self, Text> = col("creator");
    pub const duration_ms: Col<Self, Int> = col("duration_ms");
    pub const file: Col<Self, Text> = col("file");
    pub const pos: Col<Self, Int> = col("pos");
    pub const added_ms: Col<Self, Int> = col("added_ms");
    pub const user_id: Col<Self, Text> = col("user_id");
}

pub struct Playlist {
    pub id: Id<Playlist>,
    pub name: Text,
    pub pos: Int,
    pub created_ms: Int,
    pub user_id: Text,
}
impl Row for Playlist {
    const NAME: &str = "playlist";
    type Key = (Id<Playlist>,);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::id)
            .text(Self::name)
            .int(Self::pos)
            .int(Self::created_ms)
            .text(Self::user_id)
            .key((Self::id,))
            .unique((Self::user_id, Self::name))
    }
}
#[allow(non_upper_case_globals)]
impl Playlist {
    pub const id: Col<Self, Id<Self>> = col("id");
    pub const name: Col<Self, Text> = col("name");
    pub const pos: Col<Self, Int> = col("pos");
    pub const created_ms: Col<Self, Int> = col("created_ms");
    pub const user_id: Col<Self, Text> = col("user_id");
}

pub struct PlaylistItem {
    pub playlist_id: Id<Playlist>,
    pub media_id: Id<Media>,
    pub pos: Int,
    pub added_ms: Int,
    pub user_id: Text,
}
impl Row for PlaylistItem {
    const NAME: &str = "playlist_item";
    type Key = (Id<Playlist>, Id<Media>);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::playlist_id)
            .refs::<Playlist>()
            .id(Self::media_id)
            .refs::<Media>()
            .int(Self::pos)
            .int(Self::added_ms)
            .text(Self::user_id)
            .key((Self::playlist_id, Self::media_id))
            .index((Self::playlist_id, Self::pos))
    }
}
#[allow(non_upper_case_globals)]
impl PlaylistItem {
    pub const playlist_id: Col<Self, Id<Playlist>> = col("playlist_id");
    pub const media_id: Col<Self, Id<Media>> = col("media_id");
    pub const pos: Col<Self, Int> = col("pos");
    pub const added_ms: Col<Self, Int> = col("added_ms");
    pub const user_id: Col<Self, Text> = col("user_id");
}

pub struct LibraryEntry {
    pub added_ms: Int,
    pub creator: Text,
    pub duration_ms: Int,
    pub file: Text,
    pub id: Id<Media>,
    pub kind: Text,
    pub playlist_pos: Opt<Int>,
    pub pos: Int,
    pub title: Text,
    pub user_id: Text,
}
impl Record for LibraryEntry {
    fn fields() -> Fields<Self> {
        fields()
            .field("added_ms", int())
            .field("creator", text())
            .field("duration_ms", int())
            .field("file", text())
            .field("id", id::<Media>())
            .field("kind", text())
            .field("playlist_pos", opt(int()))
            .field("pos", int())
            .field("title", text())
            .field("user_id", text())
    }
}

pub fn library_entry(media: Media, playlist_pos: Opt<Int>) -> LibraryEntry {
    helper(
        "library_entry",
        (("media", media), ("playlist_pos", playlist_pos)),
        |media: Media, playlist_pos: Opt<Int>| LibraryEntry {
            added_ms: media.added_ms,
            creator: media.creator,
            duration_ms: media.duration_ms,
            file: media.file,
            id: media.id,
            kind: media.kind,
            playlist_pos,
            pos: media.pos,
            title: media.title,
            user_id: media.user_id,
        },
    )
}

pub struct Library {
    pub playlist_id: Id<Playlist>,
}
impl Input for Library {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>())
    }
}

fn module() -> Module {
    let library = router::<Lib>("library");
    Module::new((library.routes((library.input::<Library>().query("library", |_ctx, db, input| {
        db.media
            .order_by(Media::pos.asc())
            .each(|media, ()| {
                db.playlist_item
                    .filter(PlaylistItem::playlist_id.eq(input.playlist_id))
                    .on(PlaylistItem::media_id.eq(media.id))
            })
            .map(|media, (items,)| library_entry(media, items.first().map(|row| row.pos)))
    }),)),))
}

fn idv(tag: u8, n: u64) -> Value {
    let mut b = [0u8; 16];
    b[0] = tag;
    b[8..].copy_from_slice(&n.to_be_bytes());
    Value::Id(b)
}

fn row(pairs: Vec<(&str, Value)>) -> StoreRow {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// `n` media rows as `add_song` writes them, and one playlist holding the
/// first third — the harness's shape (`harken/domain/tests/perf.rs`).
fn library(sch: &Schema, n: u64) -> (MemoryStore, Value) {
    let mut st = MemoryStore::empty(sch.clone());
    for i in 0..n {
        let media = row(vec![
            ("id", idv(1, i)),
            ("kind", Value::text("song")),
            ("title", Value::text(format!("Track {i}"))),
            ("creator", Value::text(format!("Artist {}", i % 50))),
            ("duration_ms", Value::int(180_000)),
            ("file", Value::text(format!("music/a{}/t{i}.flac", i % 50))),
            ("pos", Value::int(i as i64 + 1)),
            ("added_ms", Value::int(1_000 + i as i64)),
            ("user_id", Value::text("library")),
        ]);
        st.apply_change(&Change::Add("media".into(), media));
    }
    let pl = idv(2, 1);
    let playlist = row(vec![
        ("id", pl.clone()),
        ("name", Value::text("Mine")),
        ("pos", Value::int(1)),
        ("created_ms", Value::int(1)),
        ("user_id", Value::text("alice")),
    ]);
    st.apply_change(&Change::Add("playlist".into(), playlist));
    for i in 0..n / 3 {
        let item = row(vec![
            ("playlist_id", pl.clone()),
            ("media_id", idv(1, i)),
            ("pos", Value::int(i as i64 + 1)),
            ("added_ms", Value::int(1)),
            ("user_id", Value::text("alice")),
        ]);
        st.apply_change(&Change::Add("playlist_item".into(), item));
    }
    (st, pl)
}

/// The demo's library: the tracks `harken/iced`'s seed authors.
const DEMO: u64 = 285;

/// `docs/plan-perf.md` R5: a `library` entry hydrated through its plan
/// allocates 56 times (285 entries, a third of them on the playlist; 55
/// in harken's own harness, where it was 287 before the interpreter
/// borrowed). The media row the store hands out is 15 of them and the
/// node the helper builds about 20 more; the rest is the related read of
/// the entry's items and the entry itself. Reading each of the ten fields
/// by copying the row, as it was, is 150 more, and binding the row by a
/// copy 15 more — so the bound is 64, which either would cross. Falsified
/// by making `Field` copy what it reads before taking the field
/// (`eval_ref(..).into_owned()`): 203.2 an entry.
#[test]
fn a_library_entry_hydrates_in_a_bounded_number_of_allocations() {
    let m = module();
    let built = m.build();
    let (st, pl) = library(&built.schema, DEMO);
    let args: Args = [("playlist_id".to_string(), pl)].into_iter().collect();
    // Once to warm what is allocated once per process.
    let _ = eval::query(built, "library", &args, &st).unwrap();
    let (allocs, v) = counted(|| eval::query(built, "library", &args, &st).unwrap());
    let rows = v.as_list();
    assert_eq!(rows.len(), DEMO as usize);
    assert_eq!(rows[0].field("playlist_pos"), Value::int(1));
    assert_eq!(rows[DEMO as usize - 1].field("playlist_pos"), Value::Null);
    assert_eq!(rows[5].field("title"), Value::text("Track 5"));
    let per = allocs as f64 / DEMO as f64;
    eprintln!("library at {DEMO}: {allocs} allocations, {per:.1} an entry");
    assert!(per <= 64.0, "{per:.1} allocations a library entry (55.8 when R5 landed)");
}

// A local list, mapped and filtered ---------------------------------------------

/// A helper over a list of text: `let xs = input; return filter(map(xs,
/// x -> x), y -> y != "")` — every element kept, so the work is the list's.
fn spin() -> ir::Module {
    let text = || Ty::List(Box::new(Ty::Text));
    let f = ir::Function {
        name: "spin".into(),
        kind: FnKind::Helper,
        router: None,
        uses: vec![],
        autos: vec![],
        input: vec![("input".into(), ir::Field::plain(text()))],
        refine: vec![],
        ret: Some(text()),
        body: vec![
            Stmt::Let(1, Expr::Arg("input".into())),
            Stmt::Return(Some(Expr::Filter(
                Box::new(Expr::Map(Box::new(Expr::Var(1)), 2, Box::new(Expr::Var(2)))),
                3,
                Box::new(Expr::Cmp(CmpOp::Ne, Box::new(Expr::Var(3)), Box::new(Expr::Lit(Value::text(""))))),
            ))),
        ],
        plan: None,
        names: BTreeMap::new(),
    };
    ir::Module {
        spec: ir::SPEC_VERSION,
        schema: Schema::empty(),
        functions: vec![f],
        routers: vec![],
        live: vec![],
    }
}

fn spun(m: &ir::Module, n: usize) -> usize {
    let xs: Vec<Value> = (0..n).map(|i| Value::text(format!("element {i}"))).collect();
    let input = Value::List(xs.clone());
    let (allocs, out) = counted(|| eval::eval_helper(m, "spin", vec![input]).unwrap());
    assert_eq!(out, Value::List(xs));
    allocs
}

/// `docs/plan-perf.md` R5: a `map` and a `filter` over a local list of N
/// text elements cost allocations linear in N — one an element, the copy
/// `map` makes into its answer (`filter` owns the list `map` answered, so
/// it moves what it keeps), plus a constant: 142 at 100, 446 at 400.
/// Binding an element used to copy every local, the list among them, which
/// is N copies an element. Falsified by copying every local on each bind,
/// as `Env::bind` did: 20,542 allocations at 100 and 322,046 at 400.
#[test]
fn a_map_and_a_filter_over_a_local_cost_what_the_list_holds() {
    let m = spin();
    let (small, big) = (spun(&m, 100), spun(&m, 400));
    let per = (big - small) as f64 / 300.0;
    eprintln!("map and filter over a local: {small} allocations at 100, {big} at 400; {per:.2} an element");
    assert!(per <= 3.0, "{per:.2} allocations an element: {small} at 100, {big} at 400");
    assert!(big <= 5 * small, "not linear: {small} at 100, {big} at 400");
}
