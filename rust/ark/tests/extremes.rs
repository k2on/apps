//! `docs/plan-db.md` D4: a least or a greatest value kept, as R9 keeps a
//! count or a sum (`aggregates.rs`).
//!
//! An extreme is recognised from a `fold` whose step is `min` or `max` of
//! one column of the element, and from `first`/`last` of a list ordered by
//! one column (no limit) whose element is read for that column alone. It is
//! kept only where an index serves the column under the list's pins — a
//! departure of the extreme is then one indexed read (`Store::scan_ordered`,
//! limit 1) — and the list stays a list elsewhere. An arrival compares; an
//! edit is a departure and an arrival.
//!
//! Three things are here, as in `aggregates.rs`: what is recognised, the
//! contract under churn (`support/churn.rs`) — whose small integers tie
//! constantly, so the extreme leaving while a twin stays is the common case
//! — and the guard: the greatest of 2,000 leaving costs one row read
//! through the index, counted.

#[path = "support/churn.rs"]
mod churn;
#[path = "support/counting.rs"]
mod counting;

use ark::authoring::*;
use ark::eval::{self, Args};
use ark::ir;
use ark::schema::Schema;
use ark::store::{Change, MemoryStore, Row as StoreRow, Store};
use ark::value::Value;
use ark::view::{self, Agg, Env, Patch};
use churn::{drive, Tally};
use counting::{Counting, Reads};

pub struct Lib {
    pub person: Table<Person>,
    pub song: Table<Song>,
    pub plain: Table<Plain>,
}
impl Tables for Lib {
    fn open() -> Self {
        Lib {
            person: table(),
            song: table(),
            plain: table(),
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

/// Songs, with an index for each order a person's songs are read in: by
/// position and by title, each under the creator.
pub struct Song {
    pub id: Text,
    pub creator: Text,
    pub title: Text,
    pub pos: Int,
}
impl Row for Song {
    const NAME: &str = "song";
    type Key = (Text,);
    fn columns() -> Columns<Self> {
        columns()
            .text(Self::id)
            .text(Self::creator)
            .text(Self::title)
            .int(Self::pos)
            .key((Self::id,))
            .index((Self::creator, Self::pos))
            .index((Self::creator, Self::title))
    }
}
#[allow(non_upper_case_globals)]
impl Song {
    pub const id: Col<Self, Text> = col("id");
    pub const creator: Col<Self, Text> = col("creator");
    pub const title: Col<Self, Text> = col("title");
    pub const pos: Col<Self, Int> = col("pos");
}

/// The same rows with no index at all: an extreme of it is a list.
pub struct Plain {
    pub id: Text,
    pub creator: Text,
    pub pos: Int,
}
impl Row for Plain {
    const NAME: &str = "plain";
    type Key = (Text,);
    fn columns() -> Columns<Self> {
        columns().text(Self::id).text(Self::creator).int(Self::pos).key((Self::id,))
    }
}
#[allow(non_upper_case_globals)]
impl Plain {
    pub const id: Col<Self, Text> = col("id");
    pub const creator: Col<Self, Text> = col("creator");
    pub const pos: Col<Self, Int> = col("pos");
}

pub struct Tally2 {
    pub name: Text,
    pub n: Int,
}
impl Record for Tally2 {
    fn fields() -> Fields<Self> {
        fields().field("name", text()).field("n", int())
    }
}

pub struct Titled {
    pub name: Text,
    pub title: Text,
}
impl Record for Titled {
    fn fields() -> Fields<Self> {
        fields().field("name", text()).field("title", text())
    }
}

fn lib() -> Router<Lib> {
    let r = router::<Lib>("lib");
    r.routes((
        // The greatest position of a person's songs, by a fold of `max`.
        r.query("latest", |_ctx, db, _input: ()| {
            db.person
                .each(|person, ()| db.song.rows().on(Song::creator.eq(person.name)))
                .map(|person, (songs,)| Tally2 {
                    name: person.name,
                    n: songs.fold(-1, |acc: Int, s| acc.max(s.pos)),
                })
        }),
        // The least, as the first of the list in position order, behind a
        // having on it.
        r.query("earliest", |_ctx, db, _input: ()| {
            db.person
                .each(|person, ()| db.song.order_by(Song::pos.asc()).on(Song::creator.eq(person.name)))
                .having(|_person, (songs,)| songs.first().map_or(-1, |s| s.pos).ge(0))
                .map(|person, (songs,)| Tally2 {
                    name: person.name,
                    n: songs.first().map_or(-1, |s| s.pos),
                })
        }),
        // The greatest title, as the last of the list in title order: an
        // extreme of text, ordered by it.
        r.query("last_title", |_ctx, db, _input: ()| {
            db.person
                .each(|person, ()| db.song.order_by(Song::title.asc()).on(Song::creator.eq(person.name)))
                .sort_by(|_person, (songs,)| songs.last().map_or("", |s| s.title))
                .map(|person, (songs,)| Titled {
                    name: person.name,
                    title: songs.last().map_or("", |s| s.title),
                })
        }),
        // Both ends and the count of one list, beside each other, in an
        // order key under a limit.
        r.query("span", |_ctx, db, _input: ()| {
            db.person
                .each(|person, ()| db.song.rows().on(Song::creator.eq(person.name)))
                .sort_by_desc(|_person, (songs,)| {
                    songs
                        .fold(0, |acc: Int, s| acc.max(s.pos))
                        .sub(songs.fold(100, |acc: Int, s| acc.min(s.pos)))
                })
                .limit(2)
                .map(|person, (songs,)| Tally2 {
                    name: person.name,
                    n: songs
                        .fold(0, |acc: Int, s| acc.max(s.pos))
                        .sub(songs.fold(100, |acc: Int, s| acc.min(s.pos)))
                        .add(songs.len()),
                })
        }),
        // A group's members: the greatest position of each creator's songs.
        r.query("latest_by_creator", |_ctx, db, _input: ()| {
            db.song.group_by(Song::creator).map(|creator, (songs,)| Tally2 {
                name: creator,
                n: songs.fold(-1, |acc: Int, s| acc.max(s.pos)),
            })
        }),
        // `latest` over a table no index serves: a list.
        r.query("latest_plain", |_ctx, db, _input: ()| {
            db.person
                .each(|person, ()| db.plain.rows().on(Plain::creator.eq(person.name)))
                .map(|person, (songs,)| Tally2 {
                    name: person.name,
                    n: songs.fold(-1, |acc: Int, s| acc.max(s.pos)),
                })
        }),
        // The first in position order, read for its title: that is a row,
        // not a value, and ties could tell — a list.
        r.query("first_title", |_ctx, db, _input: ()| {
            db.person
                .each(|person, ()| db.song.order_by(Song::pos.asc()).on(Song::creator.eq(person.name)))
                .map(|person, (songs,)| Titled {
                    name: person.name,
                    title: songs.first().map_or("", |s| s.title),
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

fn song(id: &str, creator: &str, title: &str, pos: i64) -> StoreRow {
    [("id", t(id)), ("creator", t(creator)), ("title", t(title)), ("pos", Value::int(pos))]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
}

fn add(st: &mut MemoryStore, table: &str, pairs: Vec<(&str, Value)>) {
    st.apply_change(&Change::Add(table.into(), pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()));
}

/// Four people, ten songs with tied positions and titles, a creator with
/// no person, a person with no songs; the same rows again in `plain`.
fn small(sch: &Schema) -> MemoryStore {
    let mut st = MemoryStore::empty(sch.clone());
    for (n, b) in [("Bach", 1685), ("Handel", 1685), ("Gould", 1932), ("Nobody", 2000)] {
        add(&mut st, "person", vec![("name", t(n)), ("born", Value::int(b))]);
    }
    for (id, creator, title, pos) in [
        ("s1", "Gould", "Aria", 3),
        ("s2", "Gould", "Variation 1", 7),
        ("s3", "Gould", "Variation 2", 7),
        ("s4", "Bach", "Air", 1),
        ("s5", "Bach", "Air", 4),
        ("s6", "Handel", "Hornpipe", 0),
        ("s7", "Handel", "Sinfony", 2),
        ("s8", "Pinnock", "Allegro", 5),
        ("s9", "Gould", "Aria", 3),
        ("s10", "Bach", "Sarabande", 4),
    ] {
        st.apply_change(&Change::Add("song".into(), song(id, creator, title, pos)));
        add(&mut st, "plain", vec![("id", t(id)), ("creator", t(creator)), ("pos", Value::int(pos))]);
    }
    st
}

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

/// D4 A fold of `max` is `Max`, the first of an ascending list `Min`, the
/// last of an ascending list `Max` — of text as of numbers — and two
/// extremes and a count of one list are three numbers kept side by side; a
/// group's members fold to one. No index on the column, or the first read
/// for another column than its order's, and the list is a list.
/// Falsified by dropping the index check (`served` answering `true`):
/// `latest_plain` is kept.
#[test]
fn an_extreme_is_kept_where_an_index_serves_it() {
    let m = module();
    let built = m.build();
    let shape = |name: &str| {
        let (plan, env) = plan_env(built, name);
        view::shape(&built.schema, &plan, &env.helpers)
    };
    let aggs = |name: &str| {
        shape(name)
            .kept
            .get(&0)
            .map(|k| k.aggs.iter().map(|(a, _)| a.clone()).collect::<Vec<_>>())
    };
    assert_eq!(aggs("latest"), Some(vec![Agg::Max("pos".into())]));
    assert_eq!(aggs("earliest"), Some(vec![Agg::Min("pos".into())]));
    assert_eq!(aggs("last_title"), Some(vec![Agg::Max("title".into())]));
    assert_eq!(
        aggs("span"),
        Some(vec![Agg::Max("pos".into()), Agg::Min("pos".into()), Agg::Count]),
        "both ends and the count, each once"
    );
    let members = shape("latest_by_creator").root.members.expect("the members kept");
    assert_eq!(members.iter().map(|(a, _)| a.clone()).collect::<Vec<_>>(), vec![Agg::Max("pos".into())]);
    assert_eq!(aggs("latest_plain"), None, "no index serves plain.pos under its creator");
    assert_eq!(aggs("first_title"), None, "the first by position read for its title is a row");
    // The expressions no longer name the list.
    let (plan, env) = plan_env(built, "earliest");
    let s = view::shape(&built.schema, &plan, &env.helpers);
    let songs = plan.related[0].sym;
    for e in [s.root.project.as_ref().unwrap(), s.root.having.as_ref().unwrap()] {
        assert!(!format!("{e:?}").contains(&format!("Var({songs})")), "{e:?}");
    }
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

/// The greatest of each person's songs, by a fold, under churn: songs
/// arrive above it, tie it, leave it with a twin still there and without
/// one, and change creator. Falsified by never reading an extreme again
/// (`take_out` answering `false`): the greatest stays at a song that has
/// left — "the view is not a fresh hydrate" within the first steps.
#[test]
fn a_kept_max_is_maintained() {
    let t = hold("latest");
    assert!(t.joined > 0 && t.updates > 0, "{t:?}");
}

/// The least as a first, behind a having that flips when the least goes
/// negative. Falsified by comparing an arrival the wrong way round
/// (`put_in` keeping the greater for a `Min`).
#[test]
fn a_kept_min_is_maintained() {
    let t = hold("earliest");
    assert!(t.joined > 0 && t.updates > 0 && t.flips > 0, "{t:?}");
}

/// The greatest title as a last, in an expression order key.
#[test]
fn a_kept_max_of_text_is_maintained() {
    let t = hold("last_title");
    assert!(t.joined > 0 && t.updates > 0, "{t:?}");
}

/// Both ends and a count of one list, in an order key under a limit.
#[test]
fn both_ends_and_a_count_are_maintained() {
    let t = hold("span");
    assert!(t.joined > 0 && t.window > 0, "{t:?}");
}

/// A group's greatest, moved by the rows that leave and arrive, read again
/// when it leaves. Falsified by not reading a group's extreme again
/// (`Track::stale` ignored in `root_of`).
#[test]
fn a_group_s_kept_max_is_maintained() {
    let t = hold("latest_by_creator");
    assert!(t.moves > 0 && t.updates > 0, "{t:?}");
}

/// The shapes left as lists still hold.
#[test]
fn an_extreme_left_a_list_is_maintained() {
    for name in ["latest_plain", "first_title"] {
        let t = hold(name);
        assert!(t.joined > 0 && t.updates > 0, "{name}: {t:?}");
    }
}

// The guard ------------------------------------------------------------------

/// Gould with `n` songs at positions 0..n, and a hundred other people with
/// a song each.
fn seeded(sch: &Schema, n: usize) -> MemoryStore {
    let mut st = MemoryStore::empty(sch.clone());
    add(&mut st, "person", vec![("name", t("Gould")), ("born", Value::int(1932))]);
    for i in 0..n {
        st.apply_change(&Change::Add("song".into(), song(&format!("g{i}"), "Gould", &format!("t{i}"), i as i64)));
    }
    for p in 0..100 {
        let who = format!("p{p}");
        add(&mut st, "person", vec![("name", t(&who)), ("born", Value::int(1900 + p))]);
        st.apply_change(&Change::Add("song".into(), song(&format!("{who}-s"), &who, "x", p)));
    }
    st
}

fn push_counted(sch: &Schema, st: &mut MemoryStore, batch: &[Change], v: &mut view::View) -> (Reads, Vec<Patch>) {
    st.apply_changes(batch);
    let counted = Counting::new(st);
    let ps = view::push_all(sch, &counted, batch, v).expect("push");
    let reads = counted.reads();
    assert!(view::contract(sch, st, v), "the view is a fresh hydrate");
    (reads, ps)
}

/// The guard: Gould's greatest position over 2,000 songs (and 500). The
/// song holding it leaving costs one row examined — the next greatest,
/// through the `(creator, pos)` index walked backwards — and Gould's own
/// row (`get`), which his node is evaluated over; a song arriving above it
/// costs the `get` and no row; one arriving below it, the same. Through a
/// group source (`latest_by_creator`), the departure is the one row and no
/// `get`. Each is one `Update`, the same at both sizes.
///
/// Falsified by reading the extreme without the index (`extremes` taking
/// its `scan_where_eq` arm whatever the store answers): the departure
/// examines 1,999 rows at 2,000 songs and 499 at 500 — every one of
/// Gould's songs left — and through the group 2,000 and 500.
#[test]
fn the_max_leaving_costs_one_indexed_read() {
    let m = module();
    let built = m.build();
    let sch = &built.schema;
    let mut seen = vec![];
    for n in [500usize, 2000] {
        let (plan, env) = plan_env(built, "latest");
        let mut st = seeded(sch, n);
        let mut v = view::hydrate(sch, &plan, env.clone(), &st).expect("hydrate");
        let gould = |v: &view::View| v.rows().into_iter().find(|r| r.field("name") == t("Gould")).expect("Gould").field("n");
        assert_eq!(gould(&v), Value::int(n as i64 - 1));
        let top = song(&format!("g{}", n - 1), "Gould", &format!("t{}", n - 1), n as i64 - 1);
        let (gone, ps) = push_counted(sch, &mut st, &[Change::Remove("song".into(), top)], &mut v);
        assert!(matches!(ps.as_slice(), [Patch::Update { .. }]), "{ps:?}");
        assert_eq!(gould(&v), Value::int(n as i64 - 2));
        let (above, _) = push_counted(sch, &mut st, &[Change::Add("song".into(), song("hi", "Gould", "hi", 9999))], &mut v);
        assert_eq!(gould(&v), Value::int(9999));
        let (below, ps) = push_counted(sch, &mut st, &[Change::Add("song".into(), song("lo", "Gould", "lo", 5))], &mut v);
        assert!(ps.is_empty(), "nothing moved: {ps:?}");

        let (plan, env) = plan_env(built, "latest_by_creator");
        let mut g = view::hydrate(sch, &plan, env, &st).expect("hydrate");
        let (grouped, ps) = push_counted(sch, &mut st, &[Change::Remove("song".into(), song("hi", "Gould", "hi", 9999))], &mut g);
        assert!(matches!(ps.as_slice(), [Patch::Update { .. }]), "{ps:?}");
        eprintln!("latest at {n} songs: the max leaving {gone:?}; one above {above:?}; one below {below:?}; through a group {grouped:?}");
        seen.push((gone, above, below, grouped));
    }
    assert_eq!(seen[0], seen[1], "the same at 500 songs and at 2,000");
    let (gone, above, below, grouped) = seen[0];
    assert_eq!(gone, Reads { gets: 1, rows: 1 });
    assert_eq!(above, Reads { gets: 1, rows: 0 });
    assert_eq!(below, Reads { gets: 1, rows: 0 });
    assert_eq!(grouped, Reads { gets: 0, rows: 1 });
}
