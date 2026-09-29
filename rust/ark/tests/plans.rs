//! Spec v4 (`docs/plan-v4.md`): every query is a plan. A small classical
//! library authored in the vocabulary — people, their works, the works'
//! movements, songs that are movements — and every part of a plan held to
//! what it means: the builder (§1.9), the wire form and its numbering
//! (§1.8), the one evaluator and the dependencies it records (§1.3, §1.5),
//! and the verifier's rules (§1.10), one complaint per rule.

use ark::authoring::*;
use ark::eval::{self, Args};
use ark::ir::{self, CmpOp, Expr, FnKind, Key, Pred, Source, Stmt};
use ark::schema::{Dir, Ty};
use ark::store::{Change, MemoryStore, Store};
use ark::value::Value;
use ark::verify::{verify, Complaint, VerifyError};
use ark::view::{self, nodes, Node};

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
    pub const work: Rel<Self, Work> = rel("work");
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

/// A song as `listing` draws it: its work's title, through a chain of two
/// lookups, and its part.
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

pub struct Tally {
    pub n: Int,
    pub name: Text,
}
impl Record for Tally {
    fn fields() -> Fields<Self> {
        fields().field("n", int()).field("name", text())
    }
}

pub struct Nothing {}
impl Input for Nothing {
    fn schema() -> Object<Self> {
        object()
    }
}

pub struct AddSong {
    pub id: Text,
    pub title: Text,
}
impl Input for AddSong {
    fn schema() -> Object<Self> {
        object().field("id", text()).field("title", text())
    }
}

fn sum(xs: List<Int>) -> Int {
    helper("sum", ("xs", xs), |xs: List<Int>| xs.fold(0, |acc: Int, x| acc.add(x)))
}

fn lib() -> Router<Lib> {
    let r = router::<Lib>("lib");
    r.routes((
        // A mutator that reads, v3-shaped: its bytes are v3's.
        r.input::<AddSong>().mutation("add_song", |_ctx, db, input| {
            let last = db.song.order_by(Song::pos.desc()).first();
            db.song.insert(Song {
                id: input.id,
                title: input.title,
                creator: "".into(),
                movement_id: none(),
                pos: last.map_or(0, |row| row.pos).add(1),
            })
        }),
        // Two chained lookups, the second through an option.
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
        // Three related plans deep, a having, and a projection over the
        // tree: a person's tracks are the songs that are movements of their
        // works.
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
                .map(|person, (works,)| Tally {
                    name: person.name,
                    n: sum(works),
                })
        }),
        // A group source.
        r.query("creators", |_ctx, db, _input: ()| {
            db.song.group_by(Song::creator).map(|creator, (songs,)| Tally {
                name: creator,
                n: songs.len(),
            })
        }),
        // Expression order keys, the first call primary, under a limit; a
        // child with its own order and a limit per parent.
        r.query("biggest_works", |_ctx, db, _input: ()| {
            db.work
                .each(|work, ()| db.movement.order_by(Movement::no.desc()).limit(1).on(Movement::work_id.eq(work.id)))
                .each(|work, _| db.movement.rows().on(Movement::work_id.eq(work.id)))
                .sort_by(|_work, (_, all)| all.len().neg())
                .sort_by(|work, _| work.title)
                .limit(2)
                .map(|work, (last, all)| Tally {
                    name: concat(list([work.title, "/".into(), last.first().map_or("", |row| row.part)])),
                    n: all.len(),
                })
        }),
        // Two related plans over one table and no projection: two fields.
        r.query("works_twice", |_ctx, db, _input: ()| {
            db.work
                .each(|work, ()| db.movement.rows().on(Movement::work_id.eq(work.id)))
                .each(|work, _| db.movement.filter(Movement::no.eq(1)).on(Movement::work_id.eq(work.id)))
        }),
        // A reference read with `.with`, and no projection: the node is the
        // row and the list beneath it.
        r.input::<Nothing>()
            .query("works_with_movements", |_ctx, db, _input| db.work.with(Work::movement)),
    ))
}

fn module() -> Module {
    Module::new((lib(),))
}

fn row(pairs: Vec<(&str, Value)>) -> ark::store::Row {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn add(st: &mut MemoryStore, t: &str, pairs: Vec<(&str, Value)>) {
    st.apply_change(&Change::Add(t.into(), row(pairs)));
}

fn t(s: &str) -> Value {
    Value::text(s)
}

/// Bach with two works, Handel with one and no songs, Hildegard with none;
/// three songs of Bach's, one of no movement at all.
fn library(sch: &ark::schema::Schema) -> MemoryStore {
    let mut st = MemoryStore::empty(sch.clone());
    for (n, b) in [("Bach", 1685), ("Handel", 1685), ("Hildegard", 1098)] {
        add(&mut st, "person", vec![("name", t(n)), ("born", Value::int(b))]);
    }
    for (id, c, title) in [
        ("bwv988", "Bach", "Goldberg"),
        ("bwv1046", "Bach", "Brandenburg 1"),
        ("hwv349", "Handel", "Water Music"),
    ] {
        add(&mut st, "work", vec![("id", t(id)), ("composer", t(c)), ("title", t(title))]);
    }
    for (id, w, no, part) in [
        ("bwv988#1", "bwv988", 1, "Aria"),
        ("bwv988#2", "bwv988", 2, "Var. 1"),
        ("bwv1046#1", "bwv1046", 1, "Allegro"),
        ("hwv349#1", "hwv349", 1, "Overture"),
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

fn ask(m: &ir::Module, name: &str, st: &MemoryStore) -> Vec<Value> {
    eval::query(m, name, &Args::new(), st)
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .as_list()
}

fn tallies(rows: &[Value]) -> Vec<(String, i64)> {
    rows.iter()
        .map(|r| (r.field("name").as_text().to_string(), r.field("n").as_int()))
        .collect()
}

/// §1.4 A query is its plan: an empty body, a plan, and a result that is a
/// list of the plan's node type; a mutator's bare read binds no row.
/// Falsified by keeping the row binder on a bare plan (`Query::finish`).
#[test]
fn a_query_is_a_plan_and_nothing_else() {
    let m = module();
    let built = m.build();
    for f in built.functions.iter().filter(|f| f.kind == FnKind::Query) {
        assert!(f.body.is_empty(), "{}: {:?}", f.name, f.body);
        assert!(f.plan.is_some(), "{}", f.name);
        assert!(matches!(f.ret, Some(Ty::List(_))), "{}", f.name);
    }
    let add = built.lookup_function("add_song").unwrap();
    assert!(add.plan.is_none());
    assert!(matches!(&add.body[0], Stmt::Let(_, Expr::Select(p)) if p.row.is_none() && p.is_bare()));
}

/// §1.3 Lookups chain, and a `None` key part looks up nothing: the song of
/// no movement has no work. Falsified by binding every lookup to `None`.
#[test]
fn lookups_chain_through_options() {
    let m = module();
    let st = library(&m.build().schema);
    let rows = ask(m.build(), "listing", &st);
    let got: Vec<(String, String, String)> = rows
        .iter()
        .map(|r| {
            (
                r.field("title").as_text().into(),
                r.field("work").as_text().into(),
                r.field("part").as_text().into(),
            )
        })
        .collect();
    let want = [
        ("Variation 1", "Goldberg", "Var. 1"),
        ("Allegro", "Brandenburg 1", "Allegro"),
        ("Aria", "Goldberg", "Aria"),
        ("Hum", "", ""),
    ];
    assert_eq!(got, want.map(|(a, b, c)| (a.to_string(), b.to_string(), c.to_string())));
}

/// §1.5 A lookup with a `Null` key reads nothing and records nothing; one
/// with a key records exactly that key against its node. Falsified by
/// recording the dependency before the `Null` test.
#[test]
fn a_lookup_of_nothing_records_nothing() {
    let m = module();
    let built = m.build();
    let st = library(&built.schema);
    let plan = built.lookup_function("listing").unwrap().plan.clone().unwrap();
    let (ctx, none) = (eval::Ctx::default(), Args::new());
    let scope = eval::Scope::new(&built.schema, &[], &ctx, &none, &none);
    let entries = view::pull(&built.schema, &plan, &scope, &st).unwrap();
    let hum = entries.iter().find(|e| e.key == [t("s4")]).unwrap();
    assert!(hum.deps.is_empty(), "{:?}", hum.deps);
    let aria = entries.iter().find(|e| e.key == [t("s1")]).unwrap();
    assert_eq!(
        aria.deps,
        vec![(0, Value::List(vec![t("bwv988#1")])), (1, Value::List(vec![t("bwv988")]))]
    );
}

/// §1.3 Related plans at depth three, a having over them and a projection
/// folding the tree. Hildegard has no works and is not in the answer;
/// Handel has a work and no songs, and is (with none). Falsified by
/// evaluating the child plans without their `on` pins (every person then
/// counts every song).
#[test]
fn related_plans_nest_and_a_having_keeps_what_has_children() {
    let m = module();
    let st = library(&m.build().schema);
    assert_eq!(tallies(&ask(m.build(), "composers", &st)), [("Bach".into(), 3), ("Handel".into(), 0)]);
}

/// §1.5 A node a having refuses is still an entry, so a child arriving is
/// what admits it. Falsified by dropping refused candidates from `pull`.
#[test]
fn a_having_admits_a_node_when_a_child_arrives() {
    let m = module();
    let built = m.build();
    let mut st = library(&built.schema);
    let plan = built.lookup_function("composers").unwrap().plan.clone().unwrap();
    let (ctx, none) = (eval::Ctx::default(), Args::new());
    let scope = eval::Scope::new(&built.schema, &built.functions, &ctx, &none, &none);
    let before = view::pull(&built.schema, &plan, &scope, &st).unwrap();
    let hildegard = before.iter().find(|e| e.key == [t("Hildegard")]).expect("an entry, though refused");
    assert!(!hildegard.admitted && hildegard.node.is_null());
    add(
        &mut st,
        "work",
        vec![("id", t("o-virtus")), ("composer", t("Hildegard")), ("title", t("O virtus"))],
    );
    let after = view::pull(&built.schema, &plan, &scope, &st).unwrap();
    assert!(after.iter().find(|e| e.key == [t("Hildegard")]).unwrap().admitted);
    assert_eq!(view::answer(&plan, &after).len(), 3);
}

/// §1.3 A group source: one node per distinct key, `members` the group's
/// rows, ordered by the key. Falsified by grouping on the whole row.
#[test]
fn a_group_is_one_node_per_key() {
    let m = module();
    let st = library(&m.build().schema);
    assert_eq!(tallies(&ask(m.build(), "creators", &st)), [("Gould".into(), 3), ("Pinnock".into(), 1)]);
    let plan = m.build().lookup_function("creators").unwrap().plan.clone().unwrap();
    assert!(matches!(&plan.source, Source::Group { by, .. } if by == &["creator"]));
    assert_eq!(plan.order, [(Key::Column("creator".into()), Dir::Asc)], "completed with the by columns");
}

/// §1.9 `sort_by` appends a key and the first call is primary (not v3's
/// list `sort_by`, where the last was); a child's order and limit are per
/// parent. Brandenburg and Goldberg: Goldberg has two movements so it is
/// first; the limit keeps two of three; each carries its last movement.
/// Falsified by taking the expression keys last call first, as v3's list
/// `sort_by` did (the titles then lead, and Brandenburg comes first).
#[test]
fn sort_by_appends_and_a_child_limit_is_per_parent() {
    let m = module();
    let st = library(&m.build().schema);
    assert_eq!(
        tallies(&ask(m.build(), "biggest_works", &st)),
        [("Goldberg/Var. 1".into(), 2), ("Brandenburg 1/Allegro".into(), 1)]
    );
    let plan = m.build().lookup_function("biggest_works").unwrap().plan.clone().unwrap();
    let keys: Vec<bool> = plan.order.iter().map(|(k, _)| matches!(k, Key::Expr(_))).collect();
    assert_eq!(keys, [true, true, false], "two expressions, then the key column");
}

/// `.with` is a related plan on the declared reference, and a plan with no
/// projection is the row and a list per related plan beneath it. Falsified
/// by pinning the child to the parent's second column (`composer`) rather
/// than its key (every list is then empty).
#[test]
fn with_reads_a_reference_beneath() {
    let m = module();
    let st = library(&m.build().schema);
    let rows = ask(m.build(), "works_with_movements", &st);
    let got: Vec<(String, usize)> = rows
        .iter()
        .map(|r| (r.field("id").as_text().to_string(), r.field("movement").as_list().len()))
        .collect();
    assert_eq!(got, [("bwv1046".into(), 1), ("bwv988".into(), 2), ("hwv349".into(), 1)]);
}

/// A second related plan over a table already related is numbered, so a
/// node with no projection carries both lists. Falsified by naming every
/// related plan after its table alone: both are `movement`, one field of
/// the node.
#[test]
fn two_related_plans_on_one_table_are_two_fields() {
    let m = module();
    let st = library(&m.build().schema);
    let plan = m.build().lookup_function("works_twice").unwrap().plan.clone().unwrap();
    let names: Vec<&str> = plan.related.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["movement", "movement_2"]);
    let got: Vec<(String, usize, usize)> = ask(m.build(), "works_twice", &st)
        .iter()
        .map(|r| {
            (
                r.field("id").as_text().to_string(),
                r.field("movement").as_list().len(),
                r.field("movement_2").as_list().len(),
            )
        })
        .collect();
    assert_eq!(got, [("bwv1046".into(), 1, 1), ("bwv988".into(), 2, 1), ("hwv349".into(), 1, 1)]);
}

/// §1.5 Soundness of the recorded dependencies: for every entry and every
/// row of every table a node of the plan reads, if taking the row away
/// changes the entry, the entry recorded that row's dependency against a
/// node of its table. What makes a change nothing depends on free.
/// Falsified by not carrying a related plan's entries' dependencies up to
/// their parent (the depth-three `composers` then misses the songs).
#[test]
fn every_row_an_entry_depends_on_is_recorded() {
    let m = module();
    let built = m.build();
    let st = library(&built.schema);
    let (ctx, none) = (eval::Ctx::default(), Args::new());
    let scope = eval::Scope::new(&built.schema, &built.functions, &ctx, &none, &none);
    let mut checked = 0;
    for name in ["listing", "composers", "biggest_works", "works_with_movements"] {
        let plan = built.lookup_function(name).unwrap().plan.clone().unwrap();
        let ns = nodes(&plan);
        let before = view::pull(&built.schema, &plan, &scope, &st).unwrap();
        let mut tables: Vec<&str> = ns.iter().map(|(_, n)| n.table().as_str()).collect();
        tables.dedup();
        for tn in tables {
            for r in st.scan(tn) {
                let mut less = st.clone();
                less.apply_change(&Change::Remove(tn.into(), r.clone()));
                let after = view::pull(&built.schema, &plan, &scope, &less).unwrap();
                for e in &before {
                    let moved = after.iter().find(|a| a.key == e.key) != Some(e);
                    if moved {
                        let recorded = ns
                            .iter()
                            .filter(|(_, n)| n.table() == tn)
                            .any(|(id, n)| e.deps.contains(&(*id, n.dependency(&built.schema, &r))));
                        assert!(recorded, "{name}: {:?} depends on {tn} {r:?} and did not say so: {:?}", e.key, e.deps);
                        checked += 1;
                    }
                }
            }
        }
    }
    assert!(checked > 10, "the property was exercised ({checked})");
}

/// §1.8 The node ids are a pre-order walk: a plan's lookups, then each
/// related plan followed by its own. Falsified by numbering the related
/// plans before the lookups.
#[test]
fn node_ids_are_a_pre_order_walk() {
    let field = |x: i64, c: &str| Expr::Field(Box::new(var(x)), c.into());
    let both = ir::Plan::from("song")
        .row(0)
        .lookup("movement", 1, "movement", vec![field(0, "movement_id")])
        .related(
            "same",
            3,
            vec![("id", field(0, "movement_id"))],
            ir::Plan::from("movement").row(2).lookup("work", 4, "work", vec![field(2, "work_id")]),
        );
    let ns = nodes(&both);
    let order: Vec<(usize, &str, bool)> = ns.iter().map(|(i, n)| (*i, n.table().as_str(), matches!(n, Node::Lookup(_)))).collect();
    assert_eq!(order, [(0, "movement", true), (1, "movement", false), (2, "work", true)]);
    let m = module();
    let built = m.build();
    let listing = built.lookup_function("listing").unwrap().plan.clone().unwrap();
    let kinds: Vec<(usize, String)> = nodes(&listing).iter().map(|(i, n)| (*i, n.table().clone())).collect();
    assert_eq!(kinds, [(0, "movement".into()), (1, "work".into())]);
    let composers = built.lookup_function("composers").unwrap().plan.clone().unwrap();
    let tables: Vec<String> = nodes(&composers).iter().map(|(_, n)| n.table().clone()).collect();
    assert_eq!(tables, ["work", "movement", "song"]);
    assert!(nodes(&composers).iter().all(|(_, n)| matches!(n, Node::Related(_))));
    assert_eq!(view::node_count(&composers), 3);
}

/// §1.8 The wire form: a v4 plan decodes to itself and its bytes are
/// stable; a v3-shaped plan carries exactly v3's keys. Falsified by always
/// writing `lookups` (the v3 plan then has a key it did not have).
#[test]
fn the_wire_form_round_trips_and_keeps_v3_keys() {
    let m = module();
    let built = m.build();
    let bytes = m.emit();
    let back = ir::module_from_value(&ark::canon::decode(&bytes).unwrap()).unwrap();
    assert_eq!(ir::module_value(&back), ir::module_value(built));
    assert_eq!(ark::canon::encode(&ir::module_value(&back)), bytes);
    assert_eq!(back.spec, 4);
    let add = ir::function_value(&Default::default(), built.lookup_function("add_song").unwrap());
    let body = add.field("body").as_list();
    let plan = body[0].field("e").field("plan");
    let keys: Vec<&String> = plan.as_struct().keys().collect();
    assert_eq!(keys, ["filter", "limit", "order", "related", "t", "table"]);
    assert!(add.as_struct().get("plan").is_none(), "a mutator writes no plan key");
    let listing = ir::function_value(&Default::default(), built.lookup_function("listing").unwrap());
    let plan = listing.field("plan");
    let keys: Vec<&String> = plan.as_struct().keys().collect();
    assert_eq!(keys, ["filter", "limit", "lookups", "order", "project", "related", "row", "t", "table"]);
}

/// §1.8 Numbering: the plan is walked in evaluation order — row, each
/// lookup's key then its symbol, each related plan's `on`, its child, then
/// its symbol — and normalising twice changes nothing. Falsified by
/// numbering a related plan's symbol before its child plan's binders.
#[test]
fn a_plan_numbers_its_binders_in_evaluation_order() {
    let m = module();
    let built = m.build();
    let listing = built.lookup_function("listing").unwrap();
    let p = listing.plan.as_ref().unwrap();
    assert_eq!(p.row, Some(0));
    assert_eq!(
        p.lookups.iter().map(|l| l.sym).collect::<Vec<_>>(),
        [1, 3],
        "the second key's `map` binds 2"
    );
    let composers = built.lookup_function("composers").unwrap().plan.clone().unwrap();
    // person 0; work plan: row 1, movement plan: row 2, song plan: row 3
    // (bare, so none) … then the related symbols after their children.
    assert_eq!(composers.row, Some(0));
    let works = &composers.related[0];
    assert_eq!(works.plan.row, Some(1));
    assert!(works.sym > works.plan.related[0].sym, "a related plan's symbol comes after its child's");
    for f in &built.functions {
        let again = ir::normalize(f);
        assert_eq!(
            (&again.body, &again.plan),
            (&f.body, &f.plan),
            "{}: normalised twice is normalised once",
            f.name
        );
    }
}

// The verifier, one complaint per rule ------------------------------------

fn refused(m: &ir::Module) -> Vec<Complaint> {
    match verify(m) {
        Ok(_) => vec![],
        Err(es) => es
            .into_iter()
            .filter_map(|e| match e {
                VerifyError::In(_, c) => Some(c),
                _ => None,
            })
            .collect(),
    }
}

fn built() -> ir::Module {
    module().build().clone()
}

fn plan_of<'a>(m: &'a mut ir::Module, name: &str) -> &'a mut ir::Plan {
    m.functions.iter_mut().find(|f| f.name == name).unwrap().plan.as_mut().unwrap()
}

fn var(x: i64) -> Expr {
    Expr::Var(x)
}

/// §1.10 Each rule the verifier holds a plan to, broken once on a module
/// that verifies. Falsified rule by rule — no plan on a query, a plan on a
/// mutator, a lookup outside a query, a read in a plan, no row binder, a
/// having's type, a lookup key's type, an `on`'s type, flat scope, the
/// result type — by removing that check from `verify.rs`, which lets the
/// broken module through or on to a different complaint.
#[test]
fn the_verifier_holds_plans_to_v4() {
    assert!(verify(&built()).is_ok());

    // A query without a plan, or with a body.
    let mut m = built();
    m.functions.iter_mut().find(|f| f.name == "creators").unwrap().plan = None;
    assert_eq!(refused(&m), [Complaint::QueryWithoutPlan]);
    let mut m = built();
    m.functions.iter_mut().find(|f| f.name == "creators").unwrap().body = vec![Stmt::Return(None)];
    assert_eq!(refused(&m), [Complaint::QueryWithBody]);

    // A plan on a mutator.
    let mut m = built();
    let p = m.lookup_function("creators").unwrap().plan.clone();
    m.functions[0].plan = p;
    assert_eq!(refused(&m), [Complaint::PlanOutsideQuery]);

    // A mutator's read with a query's features.
    let mut m = built();
    let lookup = m.lookup_function("listing").unwrap().plan.clone().unwrap();
    if let Stmt::Let(_, Expr::Select(p)) = &mut m.functions[0].body[0] {
        **p = lookup;
    }
    assert_eq!(refused(&m), [Complaint::PlanFeatureOutsideQuery("a lookup".into())]);
    let mut m = built();
    if let Stmt::Let(_, Expr::Select(p)) = &mut m.functions[0].body[0] {
        p.row = Some(90);
        p.related.push(ir::Related {
            name: "song".into(),
            sym: 91,
            on: vec![("title".into(), Expr::Field(Box::new(var(90)), "id".into()))],
            plan: ir::Plan::from("song"),
        });
    }
    assert_eq!(
        refused(&m),
        [Complaint::PlanFeatureOutsideQuery("a related plan that is not a reference".into())]
    );

    // A read, or an auto, inside a plan's expression.
    let mut m = built();
    plan_of(&mut m, "creators").having = Some(Expr::Exists("person".into(), vec![Expr::Lit(t("Bach"))]));
    assert_eq!(refused(&m), [Complaint::ReadInPlan]);
    let mut m = built();
    plan_of(&mut m, "creators").having = Some(Expr::Cmp(
        CmpOp::Eq,
        Box::new(Expr::Auto("now".into())),
        Box::new(Expr::Lit(Value::int(0))),
    ));
    assert_eq!(refused(&m), [Complaint::AutoInPlan]);

    // A plan that binds with no row binder; members without a group.
    let mut m = built();
    plan_of(&mut m, "listing").row = None;
    assert_eq!(refused(&m), [Complaint::NoRowBinder]);
    let mut m = built();
    plan_of(&mut m, "listing").members = Some(77);
    assert_eq!(refused(&m), [Complaint::MembersWithoutGroup]);

    // A having that is not a Bool.
    let mut m = built();
    plan_of(&mut m, "composers").having = Some(Expr::Lit(Value::int(1)));
    assert_eq!(refused(&m), [Complaint::TypeMismatch("having".into(), Ty::Bool, Ty::Int)]);

    // A lookup's key: its arity, and its type (an option of the key's type
    // is allowed, anything else is not).
    let mut m = built();
    plan_of(&mut m, "listing").lookups[0].key.push(Expr::Lit(t("x")));
    assert_eq!(refused(&m), [Complaint::KeyArity("movement".into(), 1, 2)]);
    let mut m = built();
    plan_of(&mut m, "listing").lookups[0].key = vec![Expr::Field(Box::new(var(0)), "pos".into())];
    assert_eq!(refused(&m), [Complaint::TypeMismatch("key of movement".into(), Ty::Text, Ty::Int)]);

    // An `on`: a column the child has, at the parent value's type.
    let mut m = built();
    plan_of(&mut m, "composers").related[0].on[0].0 = "nope".into();
    assert_eq!(refused(&m), [Complaint::UnknownColumn("work".into(), "nope".into())]);
    let mut m = built();
    plan_of(&mut m, "composers").related[0].on[0].1 = Expr::Field(Box::new(var(0)), "born".into());
    assert_eq!(refused(&m), [Complaint::TypeMismatch("on work.composer".into(), Ty::Text, Ty::Int)]);

    // An order column the source has; a group's `by` columns exist.
    let mut m = built();
    plan_of(&mut m, "creators").order.push((Key::Column("title".into()), Dir::Asc));
    assert_eq!(refused(&m), [Complaint::UnknownColumn("song".into(), "title".into())]);
    let mut m = built();
    if let Source::Group { by, .. } = &mut plan_of(&mut m, "creators").source {
        by[0] = "genre".into();
    }
    assert_eq!(refused(&m), [Complaint::UnknownColumn("song".into(), "genre".into())]);

    // Scope is flat: a child plan cannot see its parent's binders.
    let mut m = built();
    let person_row = plan_of(&mut m, "composers").row.unwrap();
    plan_of(&mut m, "composers").related[0].plan.having = Some(Expr::Cmp(
        CmpOp::Eq,
        Box::new(Expr::Field(Box::new(var(person_row)), "name".into())),
        Box::new(Expr::Lit(t("Bach"))),
    ));
    assert_eq!(refused(&m), [Complaint::UnboundSymbol(person_row)]);

    // The result is a list of the node type.
    let mut m = built();
    m.functions.iter_mut().find(|f| f.name == "creators").unwrap().ret = Some(Ty::List(Box::new(Ty::Int)));
    assert!(matches!(&refused(&m)[..], [Complaint::TypeMismatch(site, _, _)] if site == "query result"));
}

// The builder's refusals ---------------------------------------------------

fn errors(m: Module) -> Vec<String> {
    m.try_build().err().unwrap_or_default()
}

/// §1.9 What the builder refuses at `build()`: a query's feature in a
/// mutator's read, and a read written inside a plan. Falsified by removing
/// the `is_v3_shaped` test in `Query::pull` (the first then reaches the
/// verifier, whose message names the rule differently) and the `planning`
/// test in `Table::get` (the second then writes a statement, refused as
/// one inside an expression closure).
#[test]
fn the_builder_says_where_a_plan_feature_may_not_go() {
    let r = router::<Lib>("lib");
    let m = Module::new((r.routes((r.input::<AddSong>().mutation("m", |_ctx, db, input| {
        let last = db.song.rows().get(|song, ()| db.movement.by_opt(song.movement_id)).first();
        db.song.insert(Song {
            id: input.id,
            title: input.title,
            creator: "".into(),
            movement_id: none(),
            pos: last.map_or(0, |row| row.pos),
        })
    }),)),));
    let es = errors(m);
    assert!(es.iter().any(|e| e.contains("only a query's plan may have one")), "{es:?}");

    let r = router::<Lib>("lib");
    let m = Module::new((r.routes((r.query("q", |_ctx, db, _input: ()| {
        db.song.rows().having(|song, ()| db.person.get((song.creator,)).is_some())
    }),)),));
    let es = errors(m);
    assert!(es.iter().any(|e| e.contains("db.person.get(..) inside a query's plan")), "{es:?}");

    let r = router::<Lib>("lib");
    let m = Module::new((r.routes((r.query("q", |_ctx, db, _input: ()| db.song.rows().map(|_song, ()| db.person.all().len())),)),));
    let es = errors(m);
    assert!(es.iter().any(|e| e.contains("a read of person inside a query's plan")), "{es:?}");

    let r = router::<Lib>("lib");
    let m = Module::new((r.routes((r.query("q", |_ctx, db, _input: ()| {
        db.work.each(|work, ()| db.movement.on(Movement::work_id.ne(work.id)))
    }),)),));
    let es = errors(m);
    assert!(es.iter().any(|e| e.contains(".on(..) takes column equalities")), "{es:?}");
}

/// §1.4 `view::read` is `answer` of `pull`: for a bare plan it sorts only
/// what the limit keeps, and must keep exactly that — under each
/// direction, ties broken by the key, every limit from none to past the
/// end, filtered or not; for any other plan it is the pull. Falsified
/// twice in `read`: by breaking ties by the key descending (the three
/// Goulds, tied on `creator`, then come out backwards), and by ignoring a
/// `Desc` (every descending order disagrees).
#[test]
fn a_read_is_the_answer_of_the_pull() {
    let m = module();
    let built = m.build();
    let st = library(&built.schema);
    let (ctx, none) = (eval::Ctx::default(), Args::new());
    let scope = eval::Scope::new(&built.schema, &built.functions, &ctx, &none, &none);
    let mut plans = vec![];
    for filter in [None, Some(Pred::cmp("creator", CmpOp::Eq, t("Gould")))] {
        for order in [
            vec![],
            vec![("pos", Dir::Asc)],
            vec![("creator", Dir::Desc)],
            vec![("creator", Dir::Asc), ("title", Dir::Desc)],
        ] {
            for limit in [None, Some(0), Some(1), Some(2), Some(3), Some(9)] {
                let mut p = ir::Plan::from("song");
                p.filter = filter.clone();
                for (c, d) in &order {
                    p = p.order_by(c, *d);
                }
                p.limit = limit;
                plans.push(p);
            }
        }
    }
    for name in ["listing", "composers", "creators", "biggest_works", "works_twice"] {
        plans.push(built.lookup_function(name).unwrap().plan.clone().unwrap());
    }
    for p in &plans {
        let pulled = view::answer(p, &view::pull(&built.schema, p, &scope, &st).unwrap());
        assert_eq!(view::read(&built.schema, p, &scope, &st).unwrap(), pulled, "{p:?}");
    }
}

/// The query's value is what `pull` answers whoever asks: the interpreter
/// by name, a procedure, `select_plan` over a plan with its right-hand
/// sides as literals, and `Store::select`. Falsified by answering the
/// entries in scan order rather than sorted (the order asserted breaks).
#[test]
fn every_path_to_a_query_is_the_one_evaluator() {
    let m = module();
    let built = m.build();
    let st = library(&built.schema);
    let (_, p) = m.procedure("listing").unwrap();
    let by_proc = p.query(&eval::Ctx::default(), &Args::new(), &st).unwrap().as_list();
    let by_name = ask(built, "listing", &st);
    assert_eq!(by_proc, by_name);
    let bare = ir::Plan::from("song")
        .filter(Pred::cmp("creator", CmpOp::Eq, t("Gould")))
        .order_by("pos", Dir::Desc);
    let rows = eval::select_plan(&built.schema, &bare, &st).unwrap();
    let ids: Vec<Value> = rows.iter().map(|r| r.field("id")).collect();
    assert_eq!(ids, [t("s4"), t("s1"), t("s2")]);
    assert_eq!(st.select(&bare), Value::List(rows));
}
