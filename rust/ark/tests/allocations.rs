//! What the interpreter allocates, counted rather than timed, because a
//! count does not move with the machine (`docs/plan-perf.md` R5, Round 4).
//! Three guards: a `library` entry hydrated through its plan — every
//! query's plan is interpreted, natively run or not — a `map`/`filter` over
//! a local list, which must cost allocations linear in the list, and a
//! standard function over a local list, which must cost what it answers.
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

pub struct AddToPlaylist {
    pub playlist_id: Id<Playlist>,
    pub media_id: Id<Media>,
}
impl Input for AddToPlaylist {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>().exists()).field("media_id", id::<Media>())
    }
}

fn module() -> Module {
    let library = router::<Lib>("library");
    Module::new((library.routes((
        library.input::<Library>().query("library", |_ctx, db, input| {
            db.media
                .order_by(Media::pos.asc())
                .each(|media, ()| {
                    db.playlist_item
                        .filter(PlaylistItem::playlist_id.eq(input.playlist_id))
                        .on(PlaylistItem::media_id.eq(media.id))
                })
                .map(|media, (items,)| library_entry(media, items.first().map(|row| row.pos)))
        }),
        // harken's, less its `owned` middleware (which reads the playlist
        // once more): the next position on one playlist.
        library.input::<AddToPlaylist>().mutation("add_to_playlist", |ctx, db, input| {
            let media = db.media.exists((input.media_id,));
            when(media, || {
                let last = db
                    .playlist_item
                    .filter(PlaylistItem::playlist_id.eq(input.playlist_id))
                    .order_by(PlaylistItem::pos.desc())
                    .first();
                db.playlist_item.insert(PlaylistItem {
                    playlist_id: input.playlist_id,
                    media_id: input.media_id,
                    pos: last.map_or(0, |row| row.pos).add(1),
                    added_ms: ctx.now("added_ms"),
                    user_id: ctx.user,
                })
            })
        }),
    )),))
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

/// `docs/plan-perf.md` R5 and R11: a `library` entry hydrated through its
/// plan allocates 37.2 times (285 entries, a third of them on the
/// playlist). It was 52.2 before the row was positional (R11), 55.8 when
/// R5 landed, and 287 before the interpreter borrowed. The media row the
/// store hands out was 15 of those — its keys, its map nodes and its
/// values copied — and is none now: a reference count, bound as the store
/// holds it, each field the helper reads taken by position. The node the
/// helper builds is about 20; the rest is the related read of the entry's
/// items (a third of the entries have one, built into the struct a bare
/// node is) and the entry itself. The bound is 44, which binding the row
/// as the struct it is — what every read cost before R11 — crosses.
/// Falsified twice: binding the plan's row as `to_value()` in `view.rs`
/// (`Cand::bind`), 52.2 an entry — the number before R11, exactly; and
/// making `Field` copy what it reads before taking the field
/// (`cow().into_owned()` in `eval_val`), 194.8.
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
    assert!(per <= 44.0, "{per:.1} allocations a library entry (37.2 with R11, 52.2 before it)");
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

// A standard function over a local ---------------------------------------------

/// A helper over a list of text: `let xs = input; return f(xs)`, for a
/// standard function of one list.
fn std_over_a_local(f: ir::StdFn, ret: Ty) -> ir::Module {
    let text = || Ty::List(Box::new(Ty::Text));
    let g = ir::Function {
        name: "call".into(),
        kind: FnKind::Helper,
        router: None,
        uses: vec![],
        autos: vec![],
        input: vec![("input".into(), ir::Field::plain(text()))],
        refine: vec![],
        ret: Some(ret),
        body: vec![
            Stmt::Let(1, Expr::Arg("input".into())),
            Stmt::Return(Some(Expr::Std(f, vec![Expr::Var(1)]))),
        ],
        plan: None,
        names: BTreeMap::new(),
    };
    ir::Module {
        spec: ir::SPEC_VERSION,
        schema: Schema::empty(),
        functions: vec![g],
        routers: vec![],
        live: vec![],
    }
}

fn called(m: &ir::Module, n: usize, want: impl Fn(&[Value]) -> Value) -> usize {
    let xs: Vec<Value> = (0..n).map(|i| Value::text(format!("element {i}"))).collect();
    let input = Value::List(xs.clone());
    let (allocs, out) = counted(|| eval::eval_helper(m, "call", vec![input]).unwrap());
    assert_eq!(out, want(&xs));
    allocs
}

/// `docs/plan-perf.md` Round 4: a standard function reads its arguments
/// where they are bound and copies only what it answers, so `len` of a
/// local list is the same allocations at a hundred elements as at four
/// hundred — 17, which is the helper's call — and `first` of one is those
/// and the element it answers, 19. Falsified by copying the arguments
/// before the call, as `std` taking them owned did: 118 and 418 for `len`,
/// 120 and 420 for `first` — a copy of the list each time.
#[test]
fn a_standard_function_copies_only_what_it_answers() {
    let len = std_over_a_local(ir::StdFn::Len, Ty::Int);
    let first = std_over_a_local(ir::StdFn::First, Ty::Option(Box::new(Ty::Text)));
    let len_of = |xs: &[Value]| Value::int(xs.len() as i64);
    let first_of = |xs: &[Value]| xs[0].clone();
    let (l100, l400) = (called(&len, 100, len_of), called(&len, 400, len_of));
    let (f100, f400) = (called(&first, 100, first_of), called(&first, 400, first_of));
    eprintln!("len of a local: {l100} allocations at 100, {l400} at 400; first: {f100} and {f400}");
    assert_eq!((l100, f100), (l400, f400), "the same at any length");
    assert!(f100 <= l100 + 2, "first {f100} against len {l100}: more than the element it answers");
}

// What is left, by what it is ----------------------------------------------------

/// A store that counts the rows it hands out and takes in, by table: every
/// row a read returns is a copy of the row the store holds, and every row
/// a change carries is copied into it. What those copies cost in keys and
/// map nodes is [`keys_and_map`]'s.
struct Crossing {
    inner: MemoryStore,
    out: std::cell::RefCell<BTreeMap<String, usize>>,
    written: BTreeMap<String, usize>,
}

impl Crossing {
    // Counted without allocating: a name is copied the first time only.
    fn out(&self, table: &str, n: usize) {
        let mut out = self.out.borrow_mut();
        match out.get_mut(table) {
            Some(k) => *k += n,
            None => {
                out.insert(table.into(), n);
            }
        }
    }
}

impl ark::store::Store for Crossing {
    fn schema(&self) -> &Schema {
        self.inner.schema()
    }
    fn get(&self, table: &str, key: &[Value]) -> Option<StoreRow> {
        let r = self.inner.get(table, key);
        self.out(table, r.iter().len());
        r
    }
    fn exists(&self, table: &str, key: &[Value]) -> bool {
        self.inner.exists(table, key)
    }
    fn scan(&self, table: &str) -> Vec<StoreRow> {
        let rs = self.inner.scan(table);
        self.out(table, rs.len());
        rs
    }
    fn scan_where(&self, table: &str, keep: &dyn Fn(&StoreRow) -> bool) -> Vec<StoreRow> {
        let rs = self.inner.scan_where(table, keep);
        self.out(table, rs.len());
        rs
    }
    fn scan_where_eq(&self, table: &str, eq: &[(&str, &Value)], spans: &[ark::store::Span], keep: &dyn Fn(&StoreRow) -> bool) -> Vec<StoreRow> {
        let rs = self.inner.scan_where_eq(table, eq, spans, keep);
        self.out(table, rs.len());
        rs
    }
    fn scan_ordered(
        &self,
        table: &str,
        eq: &[(&str, &Value)],
        spans: &[ark::store::Span],
        order: &[(&str, ark::schema::Dir)],
        keep: &dyn Fn(&StoreRow) -> bool,
        limit: usize,
    ) -> Option<Vec<StoreRow>> {
        let rs = self.inner.scan_ordered(table, eq, spans, order, keep, limit)?;
        self.out(table, rs.len());
        Some(rs)
    }
    fn apply_change(&mut self, change: &Change) {
        *self.written.entry(change.table().into()).or_default() += 1;
        self.inner.apply_change(change);
    }
    fn as_store(&self) -> &dyn ark::store::Store {
        self
    }
}

/// What a row costs where one still costs anything: handed out (a clone,
/// two reference counts since the row is positional — `docs/plan-perf.md`
/// R11) and built into the struct it is (§4), which the evaluator does
/// only where a row is used whole — a bare plan's node, a row returned.
fn row_costs(row: &StoreRow) -> (usize, usize) {
    let (clone, _) = counted(|| std::hint::black_box(row.clone()));
    let (as_struct, _) = counted(|| std::hint::black_box(row.to_value()));
    (clone, as_struct)
}

/// `docs/plan-perf.md` R5, then R11: how much of a `library` hydrate and
/// of one `add_to_playlist` apply is rows — what the row representation
/// was decided by. Before R11 a row handed out was its whole map copied
/// (media 15 allocations, 10 of them keys and map nodes; `playlist_item`
/// 7 and 6), 31% of a hydrate and 14% of an apply. Now a row handed out
/// costs nothing, and what is left is the rows built into structs: in the
/// hydrate each item found (the related plan's bare node), in the apply
/// the item `MAX(pos)` read and the row written (its values, once).
/// Printed, not asserted.
#[test]
#[ignore]
fn perf_row_share() {
    let m = module();
    let built = m.build();
    let procs: BTreeMap<String, Procedure> = m.procedures().into_iter().map(|(_, p)| (p.name().to_string(), p)).collect();
    eprintln!("\n== what is left: the share that is rows");
    for n in [DEMO, 2000, 8000] {
        let (st, pl) = library(&built.schema, n);
        let per: BTreeMap<&str, (usize, usize)> = ["media", "playlist", "playlist_item"]
            .into_iter()
            .map(|t| (t, row_costs(&st.rows(t).into_values().next().unwrap())))
            .collect();
        let crossing = Crossing {
            inner: st,
            out: Default::default(),
            written: BTreeMap::new(),
        };
        let a: Args = [("playlist_id".to_string(), pl.clone())].into_iter().collect();
        let _ = eval::query(built, "library", &a, &crossing).unwrap();
        crossing.out.borrow_mut().values_mut().for_each(|k| *k = 0);
        let (total, _) = counted(|| eval::query(built, "library", &a, &crossing).unwrap());
        let out = crossing.out.borrow().clone();
        let handed: usize = out.iter().map(|(t, k)| k * per[t.as_str()].0).sum();
        // Each item found is the related plan's node: its struct, built.
        let items = out.get("playlist_item").copied().unwrap_or(0);
        let built_rows = items * per["playlist_item"].1;
        // The node: a struct of ten fields, built by the helper — its ten
        // names and its one map node.
        let node = 11 * n as usize;
        eprintln!(
            "library at {n}: {total} allocations ({:.1} an entry); rows handed out {out:?} at {handed} allocations; items built into structs {built_rows} ({:.0}%); the nodes' own names and maps {node} ({:.0}%)",
            total as f64 / n as f64,
            100.0 * built_rows as f64 / total as f64,
            100.0 * node as f64 / total as f64,
        );

        crossing.out.borrow_mut().clear();
        let a: Args = [("playlist_id".to_string(), pl.clone()), ("media_id".to_string(), idv(1, n - 1))]
            .into_iter()
            .collect();
        let autos: Args = [("added_ms".to_string(), Value::int(7))].into_iter().collect();
        let ctx = eval::Ctx::new("alice", "s");
        let p = &procs["add_to_playlist"];
        let (total, out) = counted(|| {
            let mut ov = ark::store::Overlay::new(&crossing);
            p.apply(&ctx, &autos, &a, &mut ov)
        });
        let changes = out.unwrap().unwrap();
        assert_eq!(changes.len(), 1);
        let out = crossing.out.borrow().clone();
        // The item read, built into a struct; the row written, its values
        // laid out once and then shared by the procedure's overlay and the
        // caller's.
        let handed: usize = out.iter().map(|(t, k)| k * per[t.as_str()].0).sum();
        let rows = out.get("playlist_item").copied().unwrap_or(0) * per["playlist_item"].1 + 1;
        eprintln!(
            "add_to_playlist at {n}: {total} allocations; rows handed out {out:?} at {handed} allocations; rows built {rows} ({:.0}%)",
            100.0 * rows as f64 / total as f64,
        );
    }
}
