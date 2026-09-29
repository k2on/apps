//! §1.5 Every plan feature, maintained: the contract held under a seeded
//! generator of change sequences (`support/churn.rs`), one test per query,
//! each saying which cases it reached — joins and non-joins, having flips,
//! group moves, the limit's edge, batches — and what broke it when the
//! engine was broken on purpose. The engine's own unit-sized cases (the
//! window's patches, the empty batch, a stale store) follow.
//!
//! A small classical library, as `plans.rs` has it: people, their works,
//! the works' movements, and songs that may be movements.

#[path = "support/churn.rs"]
mod churn;

use ark::authoring::*;
use ark::eval::{self, Args};
use ark::ir;
use ark::store::{Change, MemoryStore, Store};
use ark::value::Value;
use ark::view::{self, Env, Patch};
use churn::{drive, Tally};

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
    pub const movement: Rel<Self, Movement> = rel("movement");
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

pub struct Listed {
    pub part: Text,
    pub title: Text,
    pub work: Text,
}
impl Record for Listed {
    fn fields() -> Fields<Self> {
        fields().field("part", text()).field("title", text()).field("work", text())
    }
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

pub struct Since {
    pub born: Int,
}
impl Input for Since {
    fn schema() -> Object<Self> {
        object().field("born", int())
    }
}

fn sum(xs: List<Int>) -> Int {
    helper("sum", ("xs", xs), |xs: List<Int>| xs.fold(0, |acc: Int, x| acc.add(x)))
}

fn lib() -> Router<Lib> {
    let r = router::<Lib>("lib");
    r.routes((
        // A lookup chain, the second through an option.
        r.query("listing", |_ctx, db, _input: ()| {
            db.song
                .order_by(Song::pos.asc())
                .get(|song, ()| db.movement.by_opt(song.movement_id))
                .get(|_song, (movement,)| db.work.by_opt(movement.map(|row| row.work_id)))
                .map(|song, (movement, work)| Listed {
                    part: movement.map_or("", |row| row.part),
                    title: song.title,
                    work: work.map_or("", |row| row.title),
                })
        }),
        // Related three deep, a having over them, a helper in the projection.
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
                        .map(|_work, (movements,)| sum(movements))
                })
                .having(|_person, (works,)| works.len().gt(0))
                .map(|person, (works,)| Tally2 {
                    name: person.name,
                    n: sum(works),
                })
        }),
        // A group source, and a lookup from its key.
        r.query("creators", |_ctx, db, _input: ()| {
            db.song
                .group_by(Song::creator)
                .get(|creator, (_songs,)| db.person.by((creator,)))
                .map(|creator, (songs, person)| Tally2 {
                    name: creator,
                    n: songs.len().add(person.map_or(0, |row| row.born)),
                })
        }),
        // A group under an expression order and a limit.
        r.query("busiest", |_ctx, db, _input: ()| {
            db.song
                .group_by(Song::creator)
                .sort_by_desc(|_creator, (songs,)| songs.len())
                .limit(2)
                .map(|creator, (songs,)| Tally2 {
                    name: creator,
                    n: songs.len(),
                })
        }),
        // A bare plan under a limit: the window, refilled from the entries.
        r.query("top_songs", |_ctx, db, _input: ()| db.song.order_by(Song::pos.desc()).limit(3)),
        // Expression keys under a limit; a child's order and limit per parent.
        r.query("biggest_works", |_ctx, db, _input: ()| {
            db.work
                .each(|work, ()| db.movement.order_by(Movement::no.desc()).limit(1).on(Movement::work_id.eq(work.id)))
                .each(|work, _| db.movement.rows().on(Movement::work_id.eq(work.id)))
                .sort_by(|_work, (_, all)| all.len().neg())
                .sort_by(|work, _| work.title)
                .limit(2)
                .map(|work, (last, all)| Tally2 {
                    name: concat(list([work.title, "/".into(), last.first().map_or("", |row| row.part)])),
                    n: all.len(),
                })
        }),
        // A related plan on a column that is no key and no reference, with
        // a filter from the input on the source.
        r.input::<Since>().query("by_creator", |_ctx, db, input| {
            db.person
                .filter(Person::born.ge(input.born))
                .each(|person, ()| db.song.order_by(Song::pos.asc()).on(Song::creator.eq(person.name)))
                .map(|person, (songs,)| Tally2 {
                    name: person.name,
                    n: songs.len(),
                })
        }),
        // A reference read with `.with`, and no projection.
        r.query("works_with_movements", |_ctx, db, _input: ()| db.work.with(Work::movement)),
        // A filter, a having over a lookup, an expression order, a limit:
        // an entry can leave the answer by its having and let the next in.
        r.query("placed", |_ctx, db, _input: ()| {
            db.song
                .filter(Song::pos.gt(0))
                .get(|song, ()| db.movement.by_opt(song.movement_id))
                .having(|_song, (movement,)| movement.is_some())
                .sort_by(|_song, (movement,)| movement.map_or(0, |row| row.no))
                .limit(3)
                .map(|song, (movement,)| Tally2 {
                    name: song.title,
                    n: movement.map_or(0, |row| row.no),
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

fn add(st: &mut MemoryStore, table: &str, pairs: Vec<(&str, Value)>) {
    st.apply_change(&Change::Add(table.into(), pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()));
}

/// Four people, four works, six movements, eight songs: some movements of
/// nothing, one song of no movement, one creator with no person row.
fn library(sch: &ark::schema::Schema) -> MemoryStore {
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
    for (id, w, no, part) in [
        ("bwv988#1", "bwv988", 1, "Aria"),
        ("bwv988#2", "bwv988", 2, "Var. 1"),
        ("bwv988#3", "bwv988", 3, "Var. 2"),
        ("bwv1046#1", "bwv1046", 1, "Allegro"),
        ("hwv349#1", "hwv349", 1, "Overture"),
        ("hwv56#1", "hwv56", 1, "Sinfony"),
    ] {
        add(
            &mut st,
            "movement",
            vec![("id", t(id)), ("work_id", t(w)), ("no", Value::int(no)), ("part", t(part))],
        );
    }
    for (id, title, creator, mv, pos) in [
        ("s1", "Aria", "Gould", Some("bwv988#1"), 3),
        ("s2", "Variation 1", "Gould", Some("bwv988#2"), 1),
        ("s3", "Allegro", "Pinnock", Some("bwv1046#1"), 2),
        ("s4", "Hum", "Gould", None, 4),
        ("s5", "Overture", "Pinnock", Some("hwv349#1"), 5),
        ("s6", "Variation 2", "Gould", Some("bwv988#3"), 6),
        ("s7", "Sinfony", "Bach", Some("hwv56#1"), 0),
        ("s8", "Aria da capo", "Hildegard", Some("bwv988#1"), 7),
    ] {
        add(
            &mut st,
            "song",
            vec![
                ("id", t(id)),
                ("title", t(title)),
                ("creator", t(creator)),
                ("movement_id", mv.map(t).unwrap_or(Value::Null)),
                ("pos", Value::int(pos)),
            ],
        );
    }
    st
}

const SEEDS: u64 = 12;
const STEPS: usize = 60;

// Every seed of one query, summed.
fn hold(name: &str, args: Args) -> Tally {
    let m = module();
    let built = m.build();
    let f = built.lookup_function(name).unwrap_or_else(|| panic!("no query {name}"));
    let c = ark::hash::closure(built, f);
    let st = library(&built.schema);
    let (args, provided) = eval::middleware(&built.schema, &c, &eval::Ctx::default(), &args, &st).unwrap();
    let env = Env {
        helpers: c.helpers.clone(),
        ctx: eval::Ctx::default(),
        args,
        provided,
    };
    let plan = f.plan.clone().unwrap();
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

fn none() -> Args {
    Args::new()
}

/// A lookup chain: a change to a movement or a work reaches the songs
/// whose lookups recorded its key. Falsified by recording no dependency
/// for a `Lookup` node in `entry` (a work retitled leaves the listing's
/// `work` stale: "the view is not a fresh hydrate").
#[test]
fn listing_is_maintained() {
    let t = hold("listing", none());
    assert!(t.joined > 0 && t.unjoined > 0 && t.updates > 0, "{t:?}");
}

/// Related three deep under a having: a song arriving under a movement of
/// a work admits the composer; the last work leaving refuses them.
/// Falsified by carrying no child entry's dependencies up to its parent
/// (a song under a movement moves nothing: "the answer").
#[test]
fn composers_is_maintained() {
    let t = hold("composers", none());
    assert!(t.joined > 0 && t.unjoined > 0 && t.flips > 0 && t.updates > 0, "{t:?}");
}

/// A group source with a lookup from the key: a song's creator edited is
/// a move out of one group and into another, both rebuilt from the kept
/// members. Falsified by not removing the old row from its group's
/// members in `touched` (the group keeps a song that left it).
#[test]
fn creators_is_maintained() {
    let t = hold("creators", none());
    assert!(t.moves > 0 && t.joined > 0 && t.updates > 0, "{t:?}");
}

/// A group under an expression order and a limit: groups cross the edge
/// as they grow and shrink. Falsified by dropping the refill in `settle`
/// (a group leaving the window lets nothing in: "the patches").
#[test]
fn busiest_is_maintained() {
    let t = hold("busiest", none());
    assert!(t.moves > 0 && t.window > 0, "{t:?}");
}

/// A bare plan under a limit. Falsified by pushing `Remove { at: lim }` at
/// `lim - 1` when an insert overfills the window (the splice keeps the
/// wrong row).
#[test]
fn top_songs_is_maintained() {
    let t = hold("top_songs", none());
    assert!(t.window > 0 && t.updates > 0, "{t:?}");
}

/// Expression order keys and a limit over nodes with two related lists,
/// one with its own limit per parent. Falsified by comparing entries by
/// key alone in `View::position` (an entry reordered by a movement
/// arriving lands at the wrong place).
#[test]
fn biggest_works_is_maintained() {
    let t = hold("biggest_works", none());
    assert!(t.joined > 0 && t.window > 0, "{t:?}");
}

/// A related plan on a plain column, under a filter from the input.
/// Falsified by keying `by_dep` hits on the new row alone in `touched` (a
/// song whose creator moved away stays under the old person).
#[test]
fn by_creator_is_maintained() {
    let t = hold("by_creator", eval::Args::from([("born".to_string(), Value::int(1500))]));
    assert!(t.joined > 0 && t.unjoined > 0 && t.updates > 0, "{t:?}");
}

/// `.with` a reference, no projection: the node is the row and the list.
/// Falsified as `listing` is, for `Related` nodes (a movement added under
/// a work leaves its list short).
#[test]
fn works_with_movements_is_maintained() {
    let t = hold("works_with_movements", none());
    assert!(t.joined > 0 && t.updates > 0, "{t:?}");
}

/// A having over a lookup, under a limit: an entry refused leaves the
/// window and the next comes in from the entries. Falsified by settling a
/// refused entry as though admitted (the answer keeps it).
#[test]
fn placed_is_maintained() {
    let t = hold("placed", none());
    assert!(t.flips > 0 && t.window > 0 && t.joined > 0, "{t:?}");
}

// The engine's own cases ------------------------------------------------------

fn setup(name: &str) -> (ark::schema::Schema, ir::Plan, MemoryStore, view::View) {
    let m = module();
    let built = m.build();
    let f = built.lookup_function(name).unwrap();
    let c = ark::hash::closure(built, f);
    let st = library(&built.schema);
    let env = Env {
        helpers: c.helpers.clone(),
        ..Env::default()
    };
    let plan = f.plan.clone().unwrap();
    let v = view::hydrate(&built.schema, &plan, env, &st).unwrap();
    (built.schema.clone(), plan, st, v)
}

fn song(id: &str, pos: i64) -> ark::store::Row {
    [
        ("id", t(id)),
        ("title", t(id)),
        ("creator", t("Gould")),
        ("movement_id", Value::Null),
        ("pos", Value::int(pos)),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// §1.5, 4 The window's patches, exactly: a row entering the top three
/// pushes the third out; the top one removed lets the fourth back in, read
/// from the entries. Falsified by refilling from `entries[lim]` rather than
/// `entries[lim - 1]` after the removal (the refill is the fifth row).
#[test]
fn a_window_refills_from_the_entries() {
    let (sch, _, mut st, mut v) = setup("top_songs");
    let titles = |v: &view::View| v.rows().iter().map(|r| r.field("id").as_text().to_string()).collect::<Vec<_>>();
    assert_eq!(titles(&v), ["s8", "s6", "s5"]);
    let ch = Change::Add("song".into(), song("s9", 10));
    st.apply_change(&ch);
    let ps = view::push_all(&sch, &st, &[ch], &mut v).unwrap();
    assert_eq!(
        ps,
        [
            Patch::Insert {
                at: 0,
                node: Value::Struct(song("s9", 10))
            },
            Patch::Remove { at: 3 }
        ]
    );
    let gone = st.get("song", &[t("s9")]).unwrap();
    let ch = Change::Remove("song".into(), gone);
    st.apply_change(&ch);
    let ps = view::push_all(&sch, &st, &[ch], &mut v).unwrap();
    assert_eq!(ps[0], Patch::Remove { at: 0 });
    assert!(matches!(&ps[1], Patch::Insert { at: 2, node } if node.field("id") == t("s5")), "{ps:?}");
    assert_eq!(titles(&v), ["s8", "s6", "s5"]);
    assert!(view::contract(&sch, &st, &v));
}

/// §1.5 One settle is reconciled against the final store, so a row added,
/// edited and removed inside one batch costs nothing, and an empty batch
/// is nothing. Falsified by building entries from the changes' rows
/// rather than the store (the removed row is inserted).
#[test]
fn a_batch_is_reconciled_against_the_final_store() {
    let (sch, _, mut st, mut v) = setup("listing");
    assert!(view::push_all(&sch, &st, &[], &mut v).unwrap().is_empty());
    let a = song("s9", 9);
    let mut b = a.clone();
    b.insert("title".into(), t("renamed"));
    let chs = vec![
        Change::Add("song".into(), a.clone()),
        Change::Edit("song".into(), a, b.clone()),
        Change::Remove("song".into(), b),
    ];
    for c in &chs {
        st.apply_change(c);
    }
    let before = v.clone();
    assert!(view::push_all(&sch, &st, &chs, &mut v).unwrap().is_empty());
    assert_eq!(v, before);
}

/// The contract is a real check: a view pushed against the store *before*
/// the change is not a fresh hydrate over the store after it. What makes
/// the generator's assertions mean something. Falsified by making
/// `contract` compare the answers alone (a view whose indexes are stale but
/// whose answer happens to agree then passes).
#[test]
fn a_view_pushed_against_a_stale_store_breaks_the_contract() {
    let (sch, _, st, mut v) = setup("composers");
    let mut after = st.clone();
    let ch = Change::Add(
        "work".into(),
        [("id", t("o-virtus")), ("composer", t("Hildegard")), ("title", t("O virtus"))]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    );
    after.apply_change(&ch);
    view::push_all(&sch, &st, std::slice::from_ref(&ch), &mut v).unwrap();
    assert!(!view::contract(&sch, &after, &v));
}
