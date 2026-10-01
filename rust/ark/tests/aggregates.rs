//! `docs/plan-perf.md` R9, `docs/plan-v4.md` §1.13: a list a node uses
//! only as a count or a sum is kept as numbers, and a change moves the
//! numbers rather than recounting the list.
//!
//! Three things are here. What is recognised ([`view::shape`]): harken's
//! `composers` — a sum over works of a sum over movements of a count of
//! songs — kept at all three depths, a group's `members` counted and
//! summed, and the shapes that are not an aggregate (a list mapped, a
//! list's first) left as lists, the whole tree beneath with them. The
//! contract under churn (`support/churn.rs`) for each, the kept and the
//! broken alike: after every batch the view is a fresh hydrate, its answer
//! is what the plan reads — through the lists, which is the check that the
//! numbers are the lists' — and the patches splice. And the guard: a song
//! added under a composer costs `composers` the rows on its path, counted
//! through the counting store, the same at 500 songs and at 2,000.
//!
//! The library is `views.rs`'s (people, works, movements, songs), written
//! in this crate's vocabulary; `composers` and `total` are harken's, node
//! for node.

#[path = "support/churn.rs"]
mod churn;
#[path = "support/counting.rs"]
mod counting;

use ark::authoring::*;
use ark::eval::{self, Args};
use ark::ir;
use ark::schema::Schema;
use ark::store::{Change, MemoryStore, Store};
use ark::value::Value;
use ark::view::{self, Agg, Env, Patch};
use churn::{drive, Tally};
use counting::{Counting, Reads};

pub struct Lib {
    pub person: Table<Person>,
    pub work: Table<Work>,
    pub movement: Table<Movement>,
    pub song: Table<Song>,
}
impl Tables for Lib {
    fn open() -> Self {
        Lib {
            person: table(),
            work: table(),
            movement: table(),
            song: table(),
        }
    }
}

pub struct Person {
    pub name: Text,
    pub born: Int,
}
impl Row for Person {
    const NAME: &str = "person";
    type Key = (Text,);
    fn columns() -> Columns<Self> {
        columns().text(Self::name).int(Self::born).key((Self::name,))
    }
}
#[allow(non_upper_case_globals)]
impl Person {
    pub const name: Col<Self, Text> = col("name");
    pub const born: Col<Self, Int> = col("born");
}

pub struct Work {
    pub id: Text,
    pub composer: Text,
    pub title: Text,
}
impl Row for Work {
    const NAME: &str = "work";
    type Key = (Text,);
    fn columns() -> Columns<Self> {
        columns()
            .text(Self::id)
            .text(Self::composer)
            .refs::<Person>()
            .text(Self::title)
            .key((Self::id,))
    }
}
#[allow(non_upper_case_globals)]
impl Work {
    pub const id: Col<Self, Text> = col("id");
    pub const composer: Col<Self, Text> = col("composer");
    pub const title: Col<Self, Text> = col("title");
}

pub struct Movement {
    pub id: Text,
    pub work_id: Text,
    pub no: Int,
    pub part: Text,
}
impl Row for Movement {
    const NAME: &str = "movement";
    type Key = (Text,);
    fn columns() -> Columns<Self> {
        columns()
            .text(Self::id)
            .text(Self::work_id)
            .refs::<Work>()
            .int(Self::no)
            .text(Self::part)
            .key((Self::id,))
    }
}
#[allow(non_upper_case_globals)]
impl Movement {
    pub const id: Col<Self, Text> = col("id");
    pub const work_id: Col<Self, Text> = col("work_id");
    pub const no: Col<Self, Int> = col("no");
    pub const part: Col<Self, Text> = col("part");
}

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
            .refs::<Movement>()
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

pub struct Tally2 {
    pub n: Int,
    pub name: Text,
}
impl Record for Tally2 {
    fn fields() -> Fields<Self> {
        fields().field("n", int()).field("name", text())
    }
}

/// `ComposersEntry`, to the columns this library has.
pub struct Composer {
    pub name: Text,
    pub tracks: Int,
    pub works: Int,
}
impl Record for Composer {
    fn fields() -> Fields<Self> {
        fields().field("name", text()).field("tracks", int()).field("works", int())
    }
}

/// harken's `total`.
fn total(counts: List<Int>) -> Int {
    helper("total", ("counts", counts), |counts: List<Int>| counts.fold(0, |acc: Int, x| acc.add(x)))
}

fn lib() -> Router<Lib> {
    let r = router::<Lib>("lib");
    r.routes((
        // harken's `composers`: a sum over works of a sum over movements of
        // a count of songs, a count of works, and a having over it.
        r.query("composers", |_ctx, db, _input: ()| {
            db.person
                .each(|person, ()| {
                    db.work
                        .each(|work, ()| {
                            db.movement
                                .each(|movement, ()| db.song.rows().on(Song::movement_id.eq(some(movement.id))))
                                .on(Movement::work_id.eq(work.id))
                                .map(|_movement, (songs,)| songs.len())
                        })
                        .on(Work::composer.eq(person.name))
                        .map(|_work, (movements,)| total(movements))
                })
                .having(|_person, (works,)| works.len().gt(0))
                .map(|person, (works,)| Composer {
                    name: person.name,
                    tracks: total(works),
                    works: works.len(),
                })
        }),
        // A sum in an expression order key, under a limit: a work crosses
        // the window's edge as its songs come and go.
        r.query("biggest_works", |_ctx, db, _input: ()| {
            db.work
                .each(|work, ()| {
                    db.movement
                        .each(|movement, ()| db.song.rows().on(Song::movement_id.eq(some(movement.id))))
                        .on(Movement::work_id.eq(work.id))
                        .map(|_movement, (songs,)| songs.len())
                })
                .sort_by_desc(|_work, (movements,)| total(movements))
                .limit(3)
                .map(|work, (movements,)| Tally2 {
                    name: work.title,
                    n: total(movements),
                })
        }),
        // A group's members summed by a fold written in place, a having
        // over their count, and a lookup from the key.
        r.query("creator_pos", |_ctx, db, _input: ()| {
            db.song
                .group_by(Song::creator)
                .get(|creator, (_songs,)| db.person.by((creator,)))
                .having(|_creator, (songs, _)| songs.len().gt(1))
                .map(|creator, (songs, person)| Tally2 {
                    name: creator,
                    n: songs.fold(0, |acc: Int, s| acc.add(s.pos)).add(person.map_or(0, |row| row.born)),
                })
        }),
        // The aggregate broken at the root: the works' list is mapped, so
        // it is a list, and so is everything beneath it.
        r.query("composers_mapped", |_ctx, db, _input: ()| {
            db.person
                .each(|person, ()| {
                    db.work
                        .each(|work, ()| {
                            db.movement
                                .each(|movement, ()| db.song.rows().on(Song::movement_id.eq(some(movement.id))))
                                .on(Movement::work_id.eq(work.id))
                                .map(|_movement, (songs,)| songs.len())
                        })
                        .on(Work::composer.eq(person.name))
                        .map(|_work, (movements,)| total(movements))
                })
                .having(|_person, (works,)| works.len().gt(0))
                .map(|person, (works,)| Composer {
                    name: person.name,
                    tracks: total(works.map(|n| n.add(1))),
                    works: works.len(),
                })
        }),
        // The aggregate broken two deep: a movement reads its songs'
        // first, so no work above it is all numbers and no composer's
        // works are kept.
        r.query("composers_first", |_ctx, db, _input: ()| {
            db.person
                .each(|person, ()| {
                    db.work
                        .each(|work, ()| {
                            db.movement
                                .each(|movement, ()| db.song.rows().on(Song::movement_id.eq(some(movement.id))))
                                .on(Movement::work_id.eq(work.id))
                                .map(|_movement, (songs,)| songs.len().add(songs.first().map_or(0, |row| row.pos)))
                        })
                        .on(Work::composer.eq(person.name))
                        .map(|_work, (movements,)| total(movements))
                })
                .having(|_person, (works,)| works.len().gt(0))
                .map(|person, (works,)| Composer {
                    name: person.name,
                    tracks: total(works),
                    works: works.len(),
                })
        }),
        // One list kept and one not, beside each other: the works counted,
        // the person's own songs read for their first.
        r.query("mixed", |_ctx, db, _input: ()| {
            db.person
                .each(|person, ()| {
                    db.work
                        .each(|work, ()| db.movement.rows().on(Movement::work_id.eq(work.id)))
                        .on(Work::composer.eq(person.name))
                        .map(|_work, (movements,)| movements.len())
                })
                .each(|person, _| db.song.order_by(Song::pos.asc()).on(Song::creator.eq(person.name)))
                .map(|person, (works, songs)| Tally2 {
                    name: person.name,
                    n: total(works).add(songs.first().map_or(0, |row| row.pos)),
                })
        }),
    ))
}

fn module() -> Module {
    Module::new((lib(),))
}

fn t(s: &str) -> Value {
    Value::text(s)
}

fn song(id: &str, creator: &str, movement: Option<&str>, pos: i64) -> ark::store::Row {
    [
        ("id", t(id)),
        ("title", t(id)),
        ("creator", t(creator)),
        ("movement_id", movement.map(t).unwrap_or(Value::Null)),
        ("pos", Value::int(pos)),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

fn add(st: &mut MemoryStore, table: &str, pairs: Vec<(&str, Value)>) {
    st.apply_change(&Change::Add(table.into(), pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()));
}

/// `views.rs`'s library: four people, four works, six movements, eight
/// songs — movements of nothing, a song of no movement, a creator with no
/// person row.
fn small(sch: &Schema) -> MemoryStore {
    let mut st = MemoryStore::empty(sch.clone());
    for (n, b) in [("Bach", 1685), ("Handel", 1685), ("Hildegard", 1098), ("Gould", 1932)] {
        add(&mut st, "person", vec![("name", t(n)), ("born", Value::int(b))]);
    }
    for (id, c, title) in [
        ("bwv988", "Bach", "Goldberg"),
        ("bwv1046", "Bach", "Brandenburg 1"),
        ("hwv349", "Handel", "Water Music"),
        ("hwv56", "Handel", "Messiah"),
    ] {
        add(&mut st, "work", vec![("id", t(id)), ("composer", t(c)), ("title", t(title))]);
    }
    for (id, w, no) in [
        ("bwv988#1", "bwv988", 1),
        ("bwv988#2", "bwv988", 2),
        ("bwv988#3", "bwv988", 3),
        ("bwv1046#1", "bwv1046", 1),
        ("hwv349#1", "hwv349", 1),
        ("hwv56#1", "hwv56", 1),
    ] {
        add(
            &mut st,
            "movement",
            vec![("id", t(id)), ("work_id", t(w)), ("no", Value::int(no)), ("part", t(""))],
        );
    }
    for (id, creator, mv, pos) in [
        ("s1", "Gould", Some("bwv988#1"), 3),
        ("s2", "Gould", Some("bwv988#2"), 1),
        ("s3", "Pinnock", Some("bwv1046#1"), 2),
        ("s4", "Gould", None, 4),
        ("s5", "Pinnock", Some("hwv349#1"), 5),
        ("s6", "Gould", Some("bwv988#3"), 6),
        ("s7", "Bach", Some("hwv56#1"), 0),
        ("s8", "Hildegard", Some("bwv988#1"), 7),
    ] {
        st.apply_change(&Change::Add("song".into(), song(id, creator, mv, pos)));
    }
    st
}

// The plan of a query and the environment its closure gives it.
fn plan_env(built: &ir::Module, name: &str) -> (ir::Plan, Env) {
    let f = built.lookup_function(name).unwrap_or_else(|| panic!("no query {name}"));
    let c = ark::hash::closure(built, f);
    let env = Env {
        helpers: c.helpers.clone(),
        ctx: eval::Ctx::default(),
        args: Args::new(),
        provided: Args::new(),
    };
    (f.plan.clone().expect("a plan"), env)
}

// What is recognised --------------------------------------------------------

/// R9 `composers` is kept at all three depths: the works counted and
/// summed (the `len` in the having and the projection is one count), each
/// work's movements summed, each movement's songs counted — through the
/// `total` helper, whose whole body is the fold. Falsified by refusing a
/// helper call as an aggregate use (`helper_agg` answering `None`): the
/// works are then a list, and so is everything beneath them — nothing is
/// kept.
#[test]
fn composers_is_kept_three_deep() {
    let m = module();
    let built = m.build();
    let (plan, env) = plan_env(built, "composers");
    let s = view::shape(&built.schema, &plan, &env.helpers);
    assert_eq!(s.root.kept, vec![true]);
    assert_eq!(s.kept.keys().copied().collect::<Vec<_>>(), vec![0, 1, 2]);
    let aggs = |id: usize| s.kept[&id].aggs.iter().map(|(a, _)| a.clone()).collect::<Vec<_>>();
    assert!(matches!(aggs(0).as_slice(), [Agg::Count, Agg::Sum { .. }]), "{:?}", aggs(0));
    assert!(matches!(aggs(1).as_slice(), [Agg::Sum { .. }]), "{:?}", aggs(1));
    assert_eq!(aggs(2), vec![Agg::Count]);
    // The expressions no longer name the lists.
    let names = |e: &ir::Expr, x: ir::Sym| format!("{e:?}").contains(&format!("Var({x})"));
    let works = plan.related[0].sym;
    assert!(!names(s.root.project.as_ref().unwrap(), works));
    assert!(!names(s.root.having.as_ref().unwrap(), works));
}

/// R9 A group's members counted and summed; a lookup beside them is read
/// as before. And what breaks the shape: a list mapped at the root keeps
/// nothing; a list's `first` two deep keeps nothing above it, because a
/// kept child plan is all numbers; one list kept beside one that is not
/// keeps the one. Falsified by keeping a child plan with a list of its own
/// that is not kept (`face` without its `None if !root` arm):
/// `composers_first` keeps its works.
#[test]
fn what_is_kept_and_what_is_not() {
    let m = module();
    let built = m.build();
    let shape = |name: &str| {
        let (plan, env) = plan_env(built, name);
        view::shape(&built.schema, &plan, &env.helpers)
    };
    let pos = shape("creator_pos");
    let members = pos.root.members.as_ref().expect("the members kept");
    assert!(matches!(members.as_slice(), [(Agg::Count, _), (Agg::Sum { .. }, _)]), "{members:?}");
    assert!(shape("composers_mapped").kept.is_empty());
    assert_eq!(shape("composers_mapped").root.kept, vec![false]);
    assert!(shape("composers_first").kept.is_empty());
    let mixed = shape("mixed");
    assert_eq!(mixed.root.kept, vec![true, false]);
    assert_eq!(mixed.kept.keys().copied().collect::<Vec<_>>(), vec![0, 1]);
    let b = shape("biggest_works");
    assert_eq!(b.kept.keys().copied().collect::<Vec<_>>(), vec![0, 1]);
    assert!(b.root.order[0].is_some());
}

/// R9 A fold is a sum only when its step is `acc + f(x)` with `f` free of
/// the accumulator and of every other binder: `acc + acc`, `acc * x` and a
/// step reading the node's row are lists. Built by hand, since the
/// vocabulary would not write most of these. Falsified by dropping the
/// free-variable test (`closed`) from `aggregate_use`: the step over the
/// row is taken for a sum.
#[test]
fn only_a_sum_of_the_element_is_a_sum() {
    use ir::{Expr, Op};
    let (row, list, acc, x) = (0, 1, 2, 3);
    let var = |s| Expr::Var(s);
    let fold = |body: Expr| Expr::Fold(Box::new(var(list)), Box::new(Expr::Lit(Value::int(0))), acc, x, Box::new(body));
    let plan = |project: Expr| ir::Plan {
        source: ir::Source::Table("work".into()),
        filter: None,
        row: Some(row),
        members: None,
        lookups: vec![],
        related: vec![ir::Related {
            name: "movement".into(),
            sym: list,
            on: vec![("work_id".into(), Expr::Field(Box::new(var(row)), "id".into()))],
            plan: ir::Plan::from("movement"),
        }],
        having: None,
        project: Some(project),
        order: vec![],
        limit: None,
    };
    let m = module();
    let sch = &m.build().schema;
    let kept = |e: Expr| !view::shape(sch, &plan(e), &[]).kept.is_empty();
    let field = |s, c: &str| Expr::Field(Box::new(var(s)), c.into());
    assert!(kept(fold(Expr::Op(Op::Add, vec![var(acc), field(x, "no")]))));
    assert!(kept(fold(Expr::Op(Op::Add, vec![field(x, "no"), var(acc)]))));
    assert!(!kept(fold(Expr::Op(Op::Add, vec![var(acc), var(acc)]))));
    assert!(!kept(fold(Expr::Op(Op::Mul, vec![var(acc), field(x, "no")]))));
    assert!(!kept(fold(Expr::Op(Op::Add, vec![var(acc), field(row, "id")]))));
    assert!(kept(Expr::Std(ir::StdFn::Len, vec![var(list)])));
    assert!(!kept(Expr::Std(ir::StdFn::First, vec![var(list)])));
}

// The contract, under churn ---------------------------------------------------

const SEEDS: u64 = 12;
const STEPS: usize = 60;

fn hold(name: &str) -> Tally {
    let m = module();
    let built = m.build();
    let (plan, env) = plan_env(built, name);
    let st = small(&built.schema);
    let mut sum = Tally::default();
    for seed in 0..SEEDS {
        let t = drive(name, &built.schema, &plan, &env, st.clone(), seed, STEPS);
        sum.steps += t.steps;
        sum.batches += t.batches;
        sum.inserts += t.inserts;
        sum.removes += t.removes;
        sum.updates += t.updates;
        sum.joined += t.joined;
        sum.unjoined += t.unjoined;
        sum.flips += t.flips;
        sum.moves += t.moves;
        sum.window += t.window;
    }
    eprintln!("{name}: {sum:?}");
    assert!(sum.batches > 0 && sum.inserts > 0 && sum.removes > 0, "{name}: {sum:?}");
    sum
}

/// The numbers three deep, against the lists: a song arriving under a
/// movement of a work admits its composer, the last work leaving refuses
/// them, a movement moved between works moves its count. Falsified by
/// never letting go of a re-read child node's old dependencies (the
/// sweep's `release` skipped): numbers nobody reads stay held — "the view
/// is not a fresh hydrate" at the first movement moved.
#[test]
fn composers_is_maintained_as_numbers() {
    let t = hold("composers");
    assert!(t.joined > 0 && t.unjoined > 0 && t.flips > 0 && t.updates > 0, "{t:?}");
}

/// A sum in an expression order key, under a limit. Falsified by not
/// re-evaluating a kept child node whose numbers moved beneath it (the
/// sweep's `reeval`): the work's sum is stale — "the view is not a fresh
/// hydrate" at the first step.
#[test]
fn biggest_works_is_maintained_as_numbers() {
    let t = hold("biggest_works");
    assert!(t.joined > 0 && t.window > 0 && t.updates > 0, "{t:?}");
}

/// A group's members summed by the changes' rows, counted by the kept
/// keys, and a having over the count. Falsified by not moving the sum by
/// a row that leaves its group (only arrivals shifted in `touched`).
#[test]
fn creator_pos_is_maintained_as_numbers() {
    let t = hold("creator_pos");
    assert!(t.moves > 0 && t.flips > 0 && t.updates > 0, "{t:?}");
}

/// The rebuild path still holds where the shape breaks: at the root, two
/// deep, and beside a kept list.
#[test]
fn a_broken_shape_is_rebuilt_and_maintained() {
    for name in ["composers_mapped", "composers_first", "mixed"] {
        let t = hold(name);
        assert!(t.joined > 0 && t.updates > 0, "{name}: {t:?}");
    }
}

// The guard ------------------------------------------------------------------

/// A library of `n` songs, a quarter of them under Bach's fifty works of
/// ten movements each, the rest by fifty other creators and no work; and
/// ten other composers of two works of two movements, with a song each.
fn seeded(sch: &Schema, n: usize) -> MemoryStore {
    let mut st = MemoryStore::empty(sch.clone());
    add(&mut st, "person", vec![("name", t("Bach")), ("born", Value::int(1685))]);
    for w in 0..50 {
        add(
            &mut st,
            "work",
            vec![("id", t(&format!("bwv{w}"))), ("composer", t("Bach")), ("title", t(&format!("Work {w}")))],
        );
        for m in 0..10 {
            let id = format!("bwv{w}#{m}");
            add(
                &mut st,
                "movement",
                vec![("id", t(&id)), ("work_id", t(&format!("bwv{w}"))), ("no", Value::int(m)), ("part", t(""))],
            );
        }
    }
    for c in 0..10 {
        let who = format!("c{c}");
        add(&mut st, "person", vec![("name", t(&who)), ("born", Value::int(1700 + c))]);
        for w in 0..2 {
            let work = format!("{who}-w{w}");
            add(&mut st, "work", vec![("id", t(&work)), ("composer", t(&who)), ("title", t(&work))]);
            for m in 0..2 {
                let id = format!("{work}#{m}");
                add(
                    &mut st,
                    "movement",
                    vec![("id", t(&id)), ("work_id", t(&work)), ("no", Value::int(m)), ("part", t(""))],
                );
                st.apply_change(&Change::Add("song".into(), song(&format!("{id}-s"), &who, Some(&id), 0)));
            }
        }
    }
    for i in 0..n {
        let s = if i % 4 == 0 {
            let (w, m) = ((i / 4) % 50, (i / 200) % 10);
            song(&format!("s{i}"), "Gould", Some(&format!("bwv{w}#{m}")), i as i64)
        } else {
            song(&format!("s{i}"), &format!("a{}", i % 50), None, i as i64)
        };
        st.apply_change(&Change::Add("song".into(), s));
    }
    st
}

// One batch pushed through the view, its reads counted; the view is then
// held to the contract.
fn push_counted(sch: &Schema, st: &mut MemoryStore, batch: &[Change], v: &mut view::View) -> (Reads, Vec<Patch>) {
    st.apply_changes(batch);
    let counted = Counting::new(st);
    let ps = view::push_all(sch, &counted, batch, v).expect("push");
    let reads = counted.reads();
    assert!(view::contract(sch, st, v), "the view is a fresh hydrate");
    (reads, ps)
}

/// The guard: at 500 songs and at 2,000, a song added under one of Bach's
/// movements costs `composers` one `get` — Bach's row, which the node is
/// evaluated over — and no row scanned: the song's term is read from the
/// change, the movement's count, the work's sum and Bach's sum move by
/// one, and neither Bach's works (50), their movements (500) nor his songs
/// (125 and 500) are read. A new movement with its first song, in one
/// batch, is two `get`s (the movement, Bach) and the one song under the
/// movement, through the index; taking the song away again is one `get`.
/// Each is one `Update` of Bach's line, the same at both sizes.
///
/// Falsified by keeping nothing (`face` answering `None` for every child
/// plan, so the tree is rebuilt whole as before R9): the first push reads
/// 1 `get` and 676 rows at 500 songs and 1 and 1,051 at 2,000 — Bach's 50
/// works, 500 movements and his 125 or 500 songs, and the new one.
#[test]
fn a_song_costs_composers_its_path() {
    let m = module();
    let built = m.build();
    let sch = &built.schema;
    let (plan, env) = plan_env(built, "composers");
    let mut seen: Vec<(Reads, Reads, Reads)> = vec![];
    for n in [500, 2000] {
        let mut st = seeded(sch, n);
        let mut v = view::hydrate(sch, &plan, env.clone(), &st).expect("hydrate");
        let bach = |v: &view::View| v.rows().into_iter().find(|r| r.field("name") == t("Bach")).expect("Bach");
        let before = bach(&v).field("tracks");

        let one = Change::Add("song".into(), song("new", "Gould", Some("bwv7#3"), 1));
        let (r1, ps) = push_counted(sch, &mut st, std::slice::from_ref(&one), &mut v);
        assert!(matches!(ps.as_slice(), [Patch::Update { .. }]), "{ps:?}");
        assert_eq!(bach(&v).field("tracks"), Value::int(before.as_int() + 1));

        let mv = [("id", t("bwv7#99")), ("work_id", t("bwv7")), ("no", Value::int(99)), ("part", t(""))];
        let mv = Change::Add("movement".into(), mv.into_iter().map(|(k, v)| (k.to_string(), v)).collect());
        let under = Change::Add("song".into(), song("newer", "Gould", Some("bwv7#99"), 2));
        let (r2, ps) = push_counted(sch, &mut st, &[mv, under.clone()], &mut v);
        assert!(matches!(ps.as_slice(), [Patch::Update { .. }]), "{ps:?}");

        let Change::Add(_, row) = under else { unreachable!() };
        let (r3, ps) = push_counted(sch, &mut st, &[Change::Remove("song".into(), row)], &mut v);
        assert!(matches!(ps.as_slice(), [Patch::Update { .. }]), "{ps:?}");
        eprintln!("composers at {n} songs: a song {r1:?}; a movement and its song {r2:?}; the song taken away {r3:?}");
        seen.push((r1, r2, r3));
    }
    assert_eq!(seen[0], seen[1], "the same at 500 songs and at 2,000");
    let (r1, r2, r3) = seen[0];
    assert_eq!(r1, Reads { gets: 1, rows: 0 });
    assert_eq!(r2, Reads { gets: 2, rows: 1 });
    assert_eq!(r3, Reads { gets: 1, rows: 0 });
}

/// The guard's other face: a person's own row edited (what harken's
/// `describe_person` does) re-evaluates their node over the numbers kept
/// and reads nothing beneath it — one `get`, at any size. Falsified as
/// above: 1 `get` and 675 rows at 500 songs.
#[test]
fn a_composer_described_reads_one_row() {
    let m = module();
    let built = m.build();
    let sch = &built.schema;
    let (plan, env) = plan_env(built, "composers");
    for n in [500, 2000] {
        let mut st = seeded(sch, n);
        let mut v = view::hydrate(sch, &plan, env.clone(), &st).expect("hydrate");
        let old = st.get("person", &[t("Bach")]).unwrap();
        let mut new = old.clone();
        new.insert("born".into(), Value::int(1686));
        let (r, ps) = push_counted(sch, &mut st, &[Change::Edit("person".into(), old, new)], &mut v);
        assert_eq!(r, Reads { gets: 1, rows: 0 }, "at {n}");
        assert!(ps.is_empty(), "the node does not show `born`: {ps:?}");
    }
}

/// The group's face of the guard: a song added to the largest creator's
/// group of `creator_pos` — its count and its sum kept — costs one `get`
/// (the person row the key looks up) and no member row, at 500 songs and
/// at 2,000; the group's kept keys are moved, not copied. Falsified by
/// keeping the members as a list (`face` answering no `members`): 127
/// and 502 `get`s at the two sizes — every one of Gould's songs read
/// again, and the person.
#[test]
fn a_song_costs_its_group_one_row() {
    let m = module();
    let built = m.build();
    let sch = &built.schema;
    let (plan, env) = plan_env(built, "creator_pos");
    let mut seen = vec![];
    for n in [500, 2000] {
        let mut st = seeded(sch, n);
        let mut v = view::hydrate(sch, &plan, env.clone(), &st).expect("hydrate");
        let one = Change::Add("song".into(), song("new", "Gould", None, 5));
        let (r, ps) = push_counted(sch, &mut st, &[one], &mut v);
        assert!(matches!(ps.as_slice(), [Patch::Update { .. }]), "{ps:?}");
        eprintln!("creator_pos at {n} songs: a song {r:?}");
        seen.push(r);
    }
    assert_eq!(seen, vec![Reads { gets: 1, rows: 0 }; 2]);
}
