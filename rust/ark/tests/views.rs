//! §1.10 maintained views and §1.8 a scoped feed. The `views/` vectors:
//! v4's scenarios — the library (playlists, people, works, movements,
//! songs), the tracks of `kept-max` and `has-search`, the two fuzz findings
//! on a maintained sum — their stores and batches as they were, each query
//! a plan as JSON (`ark::dynamic`, `support/plans.rs`) over rows as JSON.
//! After every batch the view's answer is the plan evaluated whole and a
//! view made afresh, and its patches splice the answer before into it (the
//! contract); a change costs reads by what it hits, not by the size of what
//! is viewed (the work contract); a view told a replica's `Output::Changes`
//! holds through a rebase; and a replica fed by a scope holds exactly its
//! union after every delivery.
//!
//! The patches are the ones the language's engine gave: before it was
//! deleted, every vector's patches and rows were checked against it, step
//! by step (N1). The library's schema declares the indexes its reads go
//! through (`work.composer`, `movement.work_id`, `song.creator`,
//! `song.movement_id`); the fuzz schema's unique index over an optional
//! column is not unique, since v5's index holds `None` as a value.

#[path = "demo.rs"]
mod demo;
mod support {
    pub mod json;
    pub mod plans;
    pub mod vectors;
}

use ark::dynamic::{evaluate, make, parse, Dynamic};
use ark::hash::hex;
use ark::log::{EntryId, Origin};
use ark::peer::{Ctx, Feed, Input, Journal, Output, Peer, Role, Who};
use ark::plan::{splice, Patch};
use ark::row::Type;
use ark::schema::{Schema, Table};
use ark::store::{Change, Store};
use demo::*;
use serde_json::{json, Value as Json};
use support::json::schema as jschema;
use support::plans;
use support::vectors::{falsify, vector};

type Dv = ark::plan::View<Dynamic, Json>;

// * Rows and changes

type Row = (&'static str, Vec<u8>);

/// A row of `table` (in `schema`) from its columns' values, in any order:
/// its one encoding.
fn row_of(schema: &Schema, table: &'static str, cols: Json) -> Row {
    let t = schema.table(table).unwrap().1;
    (table, ark::dynamic::row_bytes(t, &cols).unwrap_or_else(|e| panic!("{table}: {e}")))
}

fn pl(n: u32, name: &str, user: &str) -> Row {
    ("playlist", playlist_row(&pid(n), name, user))
}

fn it(n: u32, track: &str, pos: i64) -> Row {
    ("item", item_row(&pid(n), track, pos))
}

fn person(name: &str, born: i64) -> Row {
    row_of(&library(), "person", json!({ "name": name, "born": born }))
}

fn work(id: &str, composer: &str, title: &str) -> Row {
    row_of(&library(), "work", json!({ "id": id, "composer": composer, "title": title }))
}

fn mv(id: &str, work: &str, no: i64, part: &str) -> Row {
    row_of(&library(), "movement", json!({ "id": id, "work_id": work, "no": no, "part": part }))
}

fn song(id: &str, title: &str, creator: &str, m: Option<&str>, pos: i64) -> Row {
    let cols = json!({ "id": id, "title": title, "creator": creator, "movement_id": m, "pos": pos });
    row_of(&library(), "song", cols)
}

fn track(id: &str, title: &str, creator: &str, pos: i64) -> Row {
    row_of(&tracks(), "track", json!({ "id": id, "title": title, "creator": creator, "pos": pos }))
}

/// A field of a fuzz schema's row, read under its column's type: an `E`
/// a variant by its place (a `Bool`'s `false`, `true`).
enum F<'a> {
    I(i64),
    T(&'a str),
    Bs(&'a [u8]),
    Id(&'a str),
    E(u8),
    Opt(Option<&'a F<'a>>),
}
use F::*;

fn f_json(ty: &Type, f: &F) -> Json {
    match (ty, f) {
        (_, I(n)) => json!(n),
        (_, T(t)) => json!(t),
        (_, Bs(b)) => json!(b),
        (_, Id(h)) => json!(h),
        (_, Opt(None)) => Json::Null,
        (Type::Option(t), Opt(Some(x))) => f_json(t, x),
        (Type::Bool, E(i)) => json!(*i == 1),
        (Type::Sum(_, vs), E(i)) => json!(vs[*i as usize].0),
        _ => panic!("no field of that type"),
    }
}

/// A row of a fuzz schema's table, its fields in order: `fuzz_colors`'s
/// rows have four, `fuzz_pairs`'s `t0` three and `t1` six.
fn fz(table: &'static str, fs: &[F]) -> Row {
    let schema = if fs.len() == 4 { fuzz_colors() } else { fuzz_pairs() };
    let t = schema.table(table).unwrap().1;
    let cols = t.columns.iter().zip(fs).map(|(c, f)| (c.name.clone(), f_json(&c.ty, f))).collect();
    row_of(&schema, table, Json::Object(cols))
}

fn add((t, row): Row) -> Change {
    Change::Add { table: t.into(), row }
}

fn remove((t, row): Row) -> Change {
    Change::Remove { table: t.into(), row }
}

fn edit((t, old): Row, (_, new): Row) -> Change {
    Change::Edit { table: t.into(), old, new }
}

fn pid(n: u32) -> Vec<u8> {
    [&[0; 12][..], &n.to_be_bytes()].concat()
}

// * The scenarios, as v4's vectors have them

fn library_before() -> Vec<Change> {
    vec![
        add(mv("bwv1046#1", "bwv1046", 1, "Allegro")),
        add(mv("bwv988#1", "bwv988", 1, "Aria")),
        add(mv("bwv988#2", "bwv988", 2, "Var. 1")),
        add(mv("hwv349#1", "hwv349", 1, "Overture")),
        add(person("Bach", 1685)),
        add(person("Gould", 1932)),
        add(person("Handel", 1685)),
        add(song("s1", "Aria", "Gould", Some("bwv988#1"), 3)),
        add(song("s2", "Variation 1", "Gould", Some("bwv988#2"), 1)),
        add(song("s3", "Allegro", "Pinnock", Some("bwv1046#1"), 2)),
        add(song("s4", "Hum", "Gould", None, 4)),
        add(song("s5", "Overture", "Pinnock", Some("hwv349#1"), 5)),
        add(work("bwv1046", "Bach", "Brandenburg 1")),
        add(work("bwv988", "Bach", "Goldberg")),
        add(work("hwv349", "Handel", "Water Music")),
    ]
}
fn library_batches() -> Vec<Vec<Change>> {
    vec![
        vec![add(pl(1, "Viewed", "alice"))],
        vec![add(it(1, "t1", 1))],
        vec![add(it(1, "t2", 2))],
        vec![add(it(1, "t3", 3)), add(it(1, "t4", 4))],
        vec![remove(it(1, "t1", 1))],
        vec![add(it(1, "t5", 5))],
        vec![add(work("hwv56", "Handel", "Messiah"))],
        vec![add(mv("hwv56#1", "hwv56", 1, "Sinfony")), add(mv("hwv56#2", "hwv56", 2, "Comfort ye"))],
        vec![
            add(song("s9", "Sinfony", "Gould", Some("hwv56#1"), 8)),
            edit(
                song("s2", "Variation 1", "Gould", Some("bwv988#2"), 1),
                song("s2", "Variation 1", "Gould", Some("bwv988#2"), 9),
            ),
        ],
        vec![edit(work("bwv988", "Bach", "Goldberg"), work("bwv988", "Bach", "Goldberg Variations"))],
        vec![
            edit(
                song("s3", "Allegro", "Pinnock", Some("bwv1046#1"), 2),
                song("s3", "Allegro", "Gould", Some("bwv1046#1"), 2),
            ),
            add(person("Pinnock", 1946)),
        ],
        vec![remove(song("s1", "Aria", "Gould", Some("bwv988#1"), 3))],
        vec![
            add(song("s10", "Passing", "Gould", None, 10)),
            edit(song("s10", "Passing", "Gould", None, 10), song("s10", "Passing", "Gould", None, 11)),
            remove(song("s10", "Passing", "Gould", None, 11)),
        ],
        vec![
            remove(song("s9", "Sinfony", "Gould", Some("hwv56#1"), 8)),
            remove(mv("hwv56#1", "hwv56", 1, "Sinfony")),
            remove(mv("hwv56#2", "hwv56", 2, "Comfort ye")),
        ],
    ]
}
fn tracks_before() -> Vec<Change> {
    vec![
        add(person("Bach", 1685)),
        add(person("Gould", 1932)),
        add(person("Handel", 1685)),
        add(track("t1", "Aria", "Gould", 3)),
        add(track("t2", "Variation 1", "Gould", 7)),
        add(track("t3", "Variation 2", "Gould", 7)),
        add(track("t4", "Air", "Bach", 1)),
        add(track("t5", "Ariadne", "Handel", 2)),
    ]
}
fn tracks_batches() -> Vec<Vec<Change>> {
    vec![
        vec![add(track("t6", "ARIA da capo", "Gould", 9))],
        vec![remove(track("t6", "ARIA da capo", "Gould", 9))],
        vec![remove(track("t2", "Variation 1", "Gould", 7))],
        vec![edit(track("t3", "Variation 2", "Gould", 7), track("t3", "Sarabande", "Gould", 2))],
        vec![edit(track("t4", "Air", "Bach", 1), track("t4", "Fair aria", "Bach", 12))],
        vec![add(person("Nobody", 2000))],
        vec![
            add(track("t9", "Passing aria", "Gould", 20)),
            remove(track("t9", "Passing aria", "Gould", 20)),
        ],
    ]
}
fn fold_overflows_before() -> Vec<Change> {
    vec![
        add(fz("t0", &[Id("f0000000000000000000000000000003"), T(""), Bs(&[]), Opt(None)])),
        add(fz("t0", &[Id("f0000000000000000000000000000008"), T(""), Bs(&[]), Opt(None)])),
        add(fz("t0", &[Id("f000000000000000000000000000000c"), T("a"), Bs(&[0]), Opt(None)])),
        add(fz("t0", &[Id("f0000000000000000000000000000015"), T("A"), Bs(&[0]), Opt(None)])),
        add(fz("t0", &[Id("f000000000000000000000000000001b"), T("x y"), Bs(&[]), Opt(None)])),
        add(fz("t0", &[Id("f000000000000000000000000000001e"), T("ab"), Bs(&[]), Opt(None)])),
        add(fz("t0", &[Id("f0000000000000000000000000000023"), T("a"), Bs(&[0]), Opt(None)])),
        add(fz("t1", &[I(-9223372036854775808), T("a"), T("b"), Opt(None)])),
        add(fz("t1", &[I(0), T("zz"), T("b"), Opt(Some(&E(1)))])),
        add(fz("t1", &[I(2), T("b"), T("b"), Opt(Some(&E(2)))])),
        add(fz("t1", &[I(1099511627776), T("b"), T("x y"), Opt(None)])),
    ]
}
fn fold_overflows_batches() -> Vec<Vec<Change>> {
    vec![
        vec![
            add(fz("t1", &[I(-2), T("b"), T("x y"), Opt(Some(&E(2)))])),
            add(fz("t1", &[I(9), T("x2"), T("b"), Opt(Some(&E(2)))])),
        ],
        vec![edit(
            fz("t0", &[Id("f0000000000000000000000000000023"), T("a"), Bs(&[0]), Opt(None)]),
            fz("t0", &[Id("f0000000000000000000000000000023"), T("a"), Bs(&[0]), Opt(Some(&T("x5")))]),
        )],
        vec![
            edit(
                fz("t1", &[I(-2), T("b"), T("x y"), Opt(Some(&E(2)))]),
                fz("t1", &[I(-2), T("b"), T("b"), Opt(Some(&E(2)))]),
            ),
            add(fz("t1", &[I(3), T("b"), T("x7"), Opt(None)])),
            add(fz("t1", &[I(8), T("x2"), T("x7"), Opt(Some(&E(2)))])),
        ],
        vec![
            remove(fz("t1", &[I(2), T("b"), T("b"), Opt(Some(&E(2)))])),
            remove(fz("t1", &[I(9), T("x2"), T("b"), Opt(Some(&E(2)))])),
        ],
        vec![
            edit(
                fz("t1", &[I(3), T("b"), T("x7"), Opt(None)]),
                fz("t1", &[I(3), T("b"), T("x7"), Opt(Some(&E(2)))]),
            ),
            edit(
                fz("t0", &[Id("f0000000000000000000000000000023"), T("a"), Bs(&[0]), Opt(Some(&T("x5")))]),
                fz("t0", &[Id("f0000000000000000000000000000023"), T("x y"), Bs(&[0]), Opt(Some(&T("x5")))]),
            ),
            remove(fz("t1", &[I(0), T("zz"), T("b"), Opt(Some(&E(1)))])),
            add(fz("t1", &[I(6), T("a"), T("x y"), Opt(Some(&E(2)))])),
        ],
        vec![
            add(fz("t1", &[I(2), T("a"), T("b"), Opt(Some(&E(2)))])),
            remove(fz("t1", &[I(2), T("a"), T("b"), Opt(Some(&E(2)))])),
            add(fz(
                "t0",
                &[Id("000000000000feed0000000000000014"), T("x19"), Bs(&[0]), Opt(Some(&T("x5")))],
            )),
        ],
        vec![
            add(fz("t1", &[I(4), T("x2"), T("x21"), Opt(Some(&E(1)))])),
            remove(fz("t1", &[I(4), T("x2"), T("x21"), Opt(Some(&E(1)))])),
        ],
        vec![
            edit(
                fz("t1", &[I(1099511627776), T("b"), T("x y"), Opt(None)]),
                fz("t1", &[I(1099511627776), T("a"), T("x y"), Opt(None)]),
            ),
            add(fz("t1", &[I(4), T("a"), T("x24"), Opt(Some(&E(2)))])),
            remove(fz("t1", &[I(1099511627776), T("a"), T("x y"), Opt(None)])),
            edit(
                fz("t1", &[I(-9223372036854775808), T("a"), T("b"), Opt(None)]),
                fz("t1", &[I(-9223372036854775808), T("b"), T("b"), Opt(Some(&E(2)))]),
            ),
        ],
    ]
}
fn kept_overflows_before() -> Vec<Change> {
    vec![
        add(fz("t0", &[T("ab"), I(-2), I(4)])),
        add(fz("t0", &[T("ab"), I(8), I(-2)])),
        add(fz("t0", &[T("ab"), I(1099511627776), I(2)])),
        add(fz("t0", &[T("ü"), I(4), I(5)])),
        add(fz("t0", &[T("ü"), I(6), I(-1)])),
        add(fz(
            "t1",
            &[T(""), I(-9223372036854775808), Opt(Some(&E(0))), E(0), I(1), Opt(Some(&T("b")))],
        )),
        add(fz("t1", &[T("a"), I(-2), Opt(Some(&E(1))), E(0), I(1), Opt(None)])),
        add(fz("t1", &[T("ü"), I(-1), Opt(Some(&E(1))), E(0), I(1), Opt(Some(&T("x y")))])),
    ]
}
fn kept_overflows_batches() -> Vec<Vec<Change>> {
    vec![vec![
        edit(fz("t0", &[T("ab"), I(8), I(-2)]), fz("t0", &[T("ab"), I(8), I(-3)])),
        edit(fz("t0", &[T("ab"), I(8), I(-3)]), fz("t0", &[T("ab"), I(8), I(-2)])),
        add(fz("t0", &[T("zz"), I(8), I(-3)])),
        add(fz("t1", &[T(""), I(4), Opt(None), E(1), I(0), Opt(Some(&T("zz")))])),
        add(fz("t0", &[T("ü"), I(9223372036854775807), I(2)])),
        remove(fz("t0", &[T("ab"), I(-2), I(4)])),
        remove(fz("t0", &[T("ü"), I(4), I(5)])),
        remove(fz("t0", &[T("ü"), I(6), I(-1)])),
        remove(fz("t0", &[T("ab"), I(8), I(-2)])),
        remove(fz("t0", &[T("ab"), I(1099511627776), I(2)])),
        remove(fz("t0", &[T("zz"), I(8), I(-3)])),
    ]]
}

// * Schemas

fn library() -> Schema {
    let mut s = demo::schema();
    let text = || Type::Text;
    s.tables.extend([
        Table::new("person").col("name", text()).col("born", Type::Int),
        Table::new("work")
            .col("id", text())
            .col("composer", text())
            .col("title", text())
            .index(&["composer"], false)
            .refs("composer", "person"),
        Table::new("movement")
            .col("id", text())
            .col("work_id", text())
            .col("no", Type::Int)
            .col("part", text())
            .index(&["work_id"], false)
            .refs("work_id", "work"),
        Table::new("song")
            .col("id", text())
            .col("title", text())
            .col("creator", text())
            .col("movement_id", Type::Option(Box::new(text())))
            .col("pos", Type::Int)
            .index(&["creator"], false)
            .index(&["movement_id"], false)
            .refs("movement_id", "movement"),
    ]);
    s
}

fn tracks() -> Schema {
    Schema {
        tables: vec![
            Table::new("person").col("name", Type::Text).col("born", Type::Int),
            Table::new("track")
                .col("id", Type::Text)
                .col("title", Type::Text)
                .col("creator", Type::Text)
                .col("pos", Type::Int)
                .index(&["creator", "pos"], false),
        ],
    }
}

/// `Option<Color>`, three colours named in their order.
fn color() -> Type {
    let v = |n: &str| (n.to_string(), vec![]);
    Type::Option(Box::new(Type::Sum("Color".into(), vec![v("c0"), v("c1"), v("c2")])))
}

/// `a-maintained-sum-answers-where-the-fold-overflows`'s schema.
fn fuzz_colors() -> Schema {
    Schema {
        tables: vec![
            Table::new("t0")
                .col("id", Type::Id("t0".into()))
                .col("c00", Type::Text)
                .col("c01", Type::Bytes)
                .col("c02", Type::Option(Box::new(Type::Text)))
                .index(&["c02"], false),
            Table::new("t1")
                .col("k1", Type::Int)
                .col("c10", Type::Text)
                .col("c11", Type::Text)
                .col("c12", color()),
        ],
    }
}

/// `a-maintained-sum-overflows-where-the-fold-answers`'s schema.
fn fuzz_pairs() -> Schema {
    Schema {
        tables: vec![
            Table::new("t0")
                .col("p0", Type::Text)
                .col("n0", Type::Int)
                .col("c00", Type::Int)
                .keyed(2)
                .index(&["c00"], false),
            Table::new("t1")
                .col("p1", Type::Text)
                .col("n1", Type::Int)
                .col("c10", Type::Option(Box::new(Type::Bool)))
                .col("c11", Type::Bool)
                .col("c12", Type::Int)
                .col("c13", Type::Option(Box::new(Type::Text)))
                .keyed(2),
        ],
    }
}

// * The runner

fn ctx_of(user: &str) -> Ctx {
    Ctx {
        draws: Default::default(),
        id: EntryId::default(),
        actor: user.into(),
        session: "dev".into(),
        roles: vec![],
        now: 0,
    }
}

fn ctx() -> Ctx {
    ctx_of("alice")
}

/// A scenario: a plan over a schema, its input, who reads it, a store and
/// the batches that move it.
struct Case {
    schema: Schema,
    arg: Json,
    ctx: Ctx,
    before: Vec<Change>,
    batches: Vec<Vec<Change>>,
}

fn store_of(schema: &Schema, rows: &[Change]) -> Store {
    let mut by: Vec<(String, Vec<Vec<u8>>)> = schema.tables.iter().map(|t| (t.name.clone(), vec![])).collect();
    for c in rows {
        let Change::Add { table, row } = c else { panic!("a store is rows") };
        by.iter_mut().find(|(t, _)| t == table).unwrap().1.push(row.clone());
    }
    Store::from_rows(schema.clone().into(), &by).unwrap()
}

fn row_json(schema: &Schema, table: &str, row: &[u8]) -> Json {
    ark::dynamic::row(schema.table(table).unwrap().1, row).unwrap()
}

/// A change as the protocol's facts read: its kind, its table, its rows.
fn change_json(schema: &Schema, c: &Change) -> Json {
    let r = |row: &[u8]| row_json(schema, c.table(), row);
    match c {
        Change::Add { table, row } => json!({ "t": "add", "table": table, "row": r(row) }),
        Change::Remove { table, row } => json!({ "t": "remove", "table": table, "row": r(row) }),
        Change::Edit { table, old, new } => json!({ "t": "edit", "table": table, "old": r(old), "new": r(new) }),
    }
}

fn patch_json(p: &Patch<Json>) -> Json {
    match p {
        Patch::Insert { at, node } => json!({ "t": "insert", "at": at, "node": node }),
        Patch::Remove { at } => json!({ "t": "remove", "at": at }),
        Patch::Update { at, node } => json!({ "t": "update", "at": at, "node": node }),
    }
}

/// The store, every table by name, rows in key order.
fn store_json(schema: &Schema, s: &Store) -> Json {
    let mut m = serde_json::Map::new();
    for t in &schema.tables {
        m.insert(t.name.clone(), s.rows(&t.name).map(|r| row_json(schema, &t.name, r)).collect());
    }
    Json::Object(m)
}

/// The contract after one push: the view's answer is the plan evaluated
/// whole and a view made afresh's, and the patches splice the answer
/// before into it.
fn holds(plan: &Json, arg: &Json, v: &Dv, store: &Store, before: &[Json], patches: &[Patch<Json>]) {
    let fresh = match evaluate(&parse(plan).unwrap(), arg, store).unwrap() {
        Json::Array(xs) => xs,
        x => panic!("a plan answers a list: {x}"),
    };
    assert_eq!(v.rows(), fresh, "the view is the plan evaluated whole");
    assert_eq!(
        Dv::new(make(plan, arg.clone()).unwrap(), v_ctx(v), store).unwrap().rows(),
        fresh,
        "and a view made again"
    );
    assert_eq!(splice(patches, before), fresh, "the patches splice");
}

fn v_ctx(_: &Dv) -> Ctx {
    ctx()
}

fn compact(v: &Json) -> String {
    serde_json::to_string(v).unwrap()
}

/// The case run: hydrated over its store, then each batch applied whole
/// and pushed as one, the contract held after each; the vector's parts.
fn run(name: &str, c: &Case) -> Vec<(&'static str, String)> {
    let plan = plans::plan(name);
    let mut store = store_of(&c.schema, &c.before);
    let before = store_json(&c.schema, &store);
    let mut v = Dv::new(make(&plan, c.arg.clone()).unwrap(), c.ctx.clone(), &store).unwrap();
    holds(&plan, &c.arg, &v, &store, &v.rows(), &[]);
    let rows_before = Json::Array(v.rows());
    let mut steps = vec![];
    for (i, batch) in c.batches.iter().enumerate() {
        let was = v.rows();
        store.apply_all(batch).unwrap_or_else(|e| panic!("{name} batch {i}: {e}"));
        let patches = v.push(batch, &store).unwrap_or_else(|e| panic!("{name} batch {i}: {e}"));
        holds(&plan, &c.arg, &v, &store, &was, &patches);
        steps.push(json!({ "patches": patches.iter().map(patch_json).collect::<Vec<_>>(), "rows": v.rows() }));
    }
    let batches: Vec<Json> = c.batches.iter().map(|b| b.iter().map(|x| change_json(&c.schema, x)).collect()).collect();
    vec![
        ("batches", compact(&Json::Array(batches))),
        ("ctx", compact(&json!({ "session": c.ctx.session, "user": c.ctx.actor }))),
        ("input", compact(&c.arg)),
        ("plan", compact(&plan)),
        ("query", compact(&json!(name))),
        ("rows_before", compact(&rows_before)),
        ("schema", jschema(&c.schema)),
        ("steps", format!("[{}]", steps.iter().map(compact).collect::<Vec<_>>().join("\n,"))),
        ("store_before", compact(&before)),
    ]
}

fn library_case(arg: Json) -> Case {
    Case {
        schema: library(),
        arg,
        ctx: ctx(),
        before: library_before(),
        batches: library_batches(),
    }
}

fn tracks_case(arg: Json) -> Case {
    Case {
        schema: tracks(),
        arg,
        ctx: ctx(),
        before: tracks_before(),
        batches: tracks_batches(),
    }
}

/// Every library and tracks vector: the plans over v4's scenarios, the
/// contract held after every batch, the vector as on disk. Falsified by
/// the dynamic sort not complementing a descending key's bytes: both the
/// view and the plan evaluated whole kept `playlist-with-items`'s first
/// three items, and the vector disagreed at its third batch.
#[test]
fn vectors() {
    let none = || json!({});
    let cases = [
        ("top-two-by-pos", library_case(json!({ "playlist_id": hex(&pid(1)) }))),
        ("playlist-with-items", library_case(none())),
        ("projection", library_case(none())),
        ("having-admits-on-a-child", library_case(none())),
        ("group-source", library_case(none())),
        ("lookup-chain", library_case(none())),
        ("related-on-a-non-key-column", library_case(none())),
        ("expression-order-under-a-limit", library_case(none())),
        ("related-three-deep", library_case(none())),
        ("playlist-join-item", library_case(none())),
        ("kept-max", tracks_case(none())),
        ("has-search", tracks_case(json!({ "needle": "ari" }))),
    ];
    for (name, c) in &cases {
        vector("views", &format!("{name}.json"), &run(name, c));
    }
}

/// `falsify/top-two-by-pos-no-refill`: the window not refilled when t1
/// leaves it — the step's insert of t3 dropped, its rows one short. The
/// runner disagrees with it (`falsify` holds the two apart). Falsified by
/// a `Take` keeping only what was in its window or in its input's delta:
/// at the batch removing t1 the view held one row where a fresh
/// evaluation holds two.
#[test]
fn a_window_not_refilled_is_wrong() {
    let right = run("top-two-by-pos", &library_case(json!({ "playlist_id": hex(&pid(1)) })));
    let mut wrong = right.clone();
    let steps = &mut wrong.iter_mut().find(|p| p.0 == "steps").unwrap().1;
    let mut parsed: Vec<Json> = serde_json::from_str(steps).unwrap();
    let refill = parsed
        .iter()
        .position(|s| {
            s["patches"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["t"] == "insert" && p["node"]["track_id"] == "t3")
        })
        .expect("the refill is in the vector");
    let s = &mut parsed[refill];
    s["patches"]
        .as_array_mut()
        .unwrap()
        .retain(|p| !(p["t"] == "insert" && p["node"]["track_id"] == "t3"));
    s["rows"].as_array_mut().unwrap().retain(|r| r["track_id"] != "t3");
    *steps = format!("[{}]", parsed.iter().map(compact).collect::<Vec<_>>().join("\n,"));
    falsify("views", "top-two-by-pos-no-refill.json", &wrong, &right);
}

/// v4's §6.5 rule, both ways, on the two fuzz findings: a group's sum is
/// taken in an `i128` and judged once, after the whole batch, as a fresh
/// sum judges its total. `a-maintained-sum-answers-where-the-fold-
/// overflows` sums `i64::MIN` and then `-2` on the way to a total that
/// fits; `…-overflows-where-the-fold-answers` passes through a group whose
/// sum is past `i64::MAX` inside one batch, which a view takes net, as one.
/// Falsified twice: by `sum` adding in a checked `i64`, the first vector's
/// fresh evaluation refused (`integer overflow`); by the runner pushing the
/// second's batch one change at a time, the push refused at its fifth
/// change.
#[test]
fn kept_sums() {
    let colours = Case {
        schema: fuzz_colors(),
        arg: json!({}),
        ctx: ctx_of("peer-0"),
        before: fold_overflows_before(),
        batches: fold_overflows_batches(),
    };
    let name = "a-maintained-sum-answers-where-the-fold-overflows";
    vector("views", &format!("{name}.json"), &run(name, &colours));
    let pairs = Case {
        schema: fuzz_pairs(),
        arg: json!({ "f_p0": "a" }),
        ctx: ctx_of("peer-1"),
        before: kept_overflows_before(),
        batches: kept_overflows_batches(),
    };
    let name = "a-maintained-sum-overflows-where-the-fold-answers";
    vector("views", &format!("{name}.json"), &run(name, &pairs));
}

// * The work contract

/// What one push of `c` costs a view of `name`: the reads it made, the
/// contract held after it.
fn cost(name: &str, arg: Json, mut store: Store, c: Change) -> ark::db::Reads {
    let plan = plans::plan(name);
    let mut v = Dv::new(make(&plan, arg.clone()).unwrap(), ctx(), &store).unwrap();
    let was = v.rows();
    store.apply(&c).unwrap();
    let (patches, reads) = ark::db::count(|| v.push(std::slice::from_ref(&c), &store).unwrap());
    holds(&plan, &arg, &v, &store, &was, &patches);
    reads
}

/// The work contract (§1.10): a change costs reads by the entries it hits,
/// not by the rows viewed — the same at 1k, 4k and 16k rows. An item added
/// to one of `n / 4` playlists of `playlist-with-items` re-reads that
/// playlist's items through the `(playlist_id, pos)` index and nothing
/// else; the first item of `top-two-by-pos` leaving is refilled from the
/// view's own order, and a song changing creator under `group-source`
/// moves two groups — no read at all. Falsified by recording a related
/// read as the whole table (the prefix dropped): every playlist was hit,
/// and the item cost 1,250 rows at 1k.
#[test]
fn a_change_costs_what_it_hits() {
    let mut seen = vec![];
    for n in [1000u32, 4000, 16000] {
        let mut rows = vec![];
        for p in 0..n / 4 {
            rows.push(add(pl(p, &format!("p{p}"), "alice")));
            rows.extend((1..=4).map(|k| add(it(p, &format!("t{k}"), k as i64))));
        }
        let store = store_of(&library(), &rows);
        let items = cost("playlist-with-items", json!({}), store, add(it(n / 8, "t5", 5)));
        let top = cost(
            "top-two-by-pos",
            json!({ "playlist_id": hex(&pid(1)) }),
            scaled_items(n as usize),
            remove(it(1, "t0", 1)),
        );
        let (c0, c1) = (song("s0", "x", "c0", None, 0), song("s0", "x", "c1", None, 0));
        let groups = cost("group-source", json!({}), scaled_songs(n as usize), edit(c0, c1));
        seen.push((items, top, groups));
    }
    let r = |gets, rows| ark::db::Reads { gets, rows };
    assert_eq!(seen[0], (r(0, 5), r(0, 0), r(0, 0)), "what one change reads");
    assert!(seen.iter().all(|s| *s == seen[0]), "at every size: {seen:?}");
}

fn scaled_items(n: usize) -> Store {
    let mut rows = vec![add(pl(1, "Viewed", "alice"))];
    rows.extend((0..n).map(|i| add(it(1, &format!("t{i}"), i as i64 + 1))));
    store_of(&library(), &rows)
}

fn scaled_songs(n: usize) -> Store {
    store_of(
        &library(),
        &(0..n)
            .map(|i| add(song(&format!("s{i}"), "x", &format!("c{}", i % 8), None, i as i64)))
            .collect::<Vec<_>>(),
    )
}

// * A view told a replica's changes

/// Views over a replica's store, told nothing but its `Output::Changes`,
/// hold through a rebase: alice previews tracks offline, bob's land under
/// them — hers undone, his applied, hers redone — and after every step
/// each view is a fresh evaluation and its patches splice. One view reads
/// the items in a lambda (`playlist-with-items`), one scans them on its
/// spine (`top-two-by-pos`). Falsified by the view taking each key's first
/// change as its net: alice's track, undone and redone at a new place,
/// left `top-two-by-pos`.
#[test]
fn a_view_follows_a_replica_through_a_rebase() {
    let mut server = Peer::new(
        demo::shared(),
        demo::cas(),
        Role::Authority { journal: Journal::default() },
        who_of("server"),
    );
    let (mut alice, mut bob) = (replica_of("alice"), replica_of("bob"));
    let pid = EntryId::of(Origin::named("alice-session"), 1).draw(0);
    let queries = [
        ("playlist-with-items", json!({})),
        ("top-two-by-pos", json!({ "playlist_id": hex(&pid) })),
    ];
    let mut views: Vec<(Json, Json, Dv)> = queries
        .into_iter()
        .map(|(name, arg)| {
            let p = plans::plan(name);
            let v = Dv::new(make(&p, arg.clone()).unwrap(), ctx(), &alice.store).unwrap();
            (p, arg, v)
        })
        .collect();
    let told = std::cell::Cell::new(0);
    let mut tell = |alice: &mut Peer, input: Input| -> Vec<Output> {
        let o = alice.step(input);
        let cs: Vec<Change> = o
            .iter()
            .flat_map(|x| if let Output::Changes(cs) = x { cs.clone() } else { vec![] })
            .collect();
        told.set(told.get() + cs.len());
        for (p, arg, v) in views.iter_mut() {
            let was = v.rows();
            let patches = v.push(&cs, &alice.store).unwrap();
            holds(p, arg, v, &alice.store, &was, &patches);
        }
        o
    };
    let up = |server: &mut Peer, w: &str, o: &[Output]| -> Vec<Vec<u8>> {
        let mut down = vec![];
        for b in o.iter().filter_map(|x| if let Output::Send(b) = x { Some(b.clone()) } else { None }) {
            for r in server.step(Input::Frame {
                from: Some(who_of(w)),
                bytes: b,
            }) {
                if let Output::Fanout(b) = r {
                    down.push(b);
                }
            }
        }
        down
    };
    let o = tell(&mut alice, mutate_input(CREATE, create_input("Mix")));
    for b in up(&mut server, "alice", &o) {
        tell(
            &mut alice,
            Input::Frame {
                from: None,
                bytes: b.clone(),
            },
        );
        bob.step(Input::Frame { from: None, bytes: b });
    }
    tell(&mut alice, mutate_input(ADD, pair_input(&pid, "a1")));
    tell(&mut alice, mutate_input(ADD, pair_input(&pid, "a2")));
    let mut landed = vec![];
    for t in ["b1", "b2", "b3"] {
        let o = bob.step(mutate_input(ADD, pair_input(&pid, t)));
        landed.extend(up(&mut server, "bob", &o));
    }
    for b in landed {
        tell(&mut alice, Input::Frame { from: None, bytes: b });
    }
    assert_eq!(alice.pending.len(), 2, "alice's two tracks still pending");
    assert!(told.get() > 12, "the rebases were told as changes: {}", told.get());
    let o = tell(&mut alice, Input::Connected);
    for b in up(&mut server, "alice", &o) {
        tell(&mut alice, Input::Frame { from: None, bytes: b });
    }
    assert!(alice.pending.is_empty() && alice.store == server.store);
}

fn who_of(name: &str) -> Who {
    Who {
        actor: name.into(),
        session: format!("{name}-session"),
        roles: vec![],
    }
}

fn replica_of(name: &str) -> Peer {
    Peer::new(demo::shared(), demo::cas(), Role::Replica, who_of(name))
}

fn mutate_input(name: &str, input: Vec<u8>) -> Input {
    Input::Mutate {
        func: h(name),
        input,
        now: 0,
    }
}

// * A scoped feed (§1.8)

/// What `actor` holds of `store`, by the scope's meaning written out.
fn union_of(store: &Store, actor: &str) -> Store {
    let rows: Vec<(String, Vec<Vec<u8>>)> = store
        .snapshot_rows()
        .into_iter()
        .map(|(t, rs)| {
            let held = rs
                .into_iter()
                .filter(|r| t == "playlist" || item_of(r).1.starts_with(&format!("{actor}:")))
                .collect();
            (t, held)
        })
        .collect();
    Store::from_rows(demo::shared(), &rows).unwrap()
}

/// A peer's confirmed state: its store with its pending changes undone.
fn confirmed(p: &Peer) -> Store {
    let mut s = p.store.clone();
    for c in p.pending.iter().rev().flat_map(|x| x.changes.iter().rev()) {
        s.apply(&c.inverse()).unwrap();
    }
    s
}

/// The authority's state at `seq`, its later facts undone.
fn state_at(server: &Peer, seq: i64) -> Store {
    let mut s = server.store.clone();
    for c in server
        .log()
        .after(seq)
        .collect::<Vec<_>>()
        .iter()
        .rev()
        .flat_map(|l| l.facts.iter().rev())
    {
        s.apply(&c.inverse()).unwrap();
    }
    s
}

/// A fleet of an authority, a client fed whole and two fed by
/// `demo.mine` (every playlist, and the items whose track is the
/// reader's — `actor:…`), on
/// v4's scheduler (a random frame from a random direction, one in eight
/// twice, one in sixteen lost).
struct Fleet {
    server: Peer,
    clients: Vec<Peer>,
    linked: Vec<bool>,
    up: Vec<Vec<Vec<u8>>>,
    down: Vec<Vec<Vec<u8>>>,
    seed: u64,
}

impl Fleet {
    fn new(seed: u64) -> Fleet {
        let cas = demo::cas();
        let h = "demo.mine";
        let auth = Role::Authority { journal: Journal::default() };
        let mut server = Peer::new(demo::shared(), cas.clone(), auth, who_of("server"));
        let mut clients = vec![];
        for i in 0..3 {
            let mut c = Peer::new(demo::shared(), cas.clone(), Role::Replica, who_of(&format!("peer-{i}")));
            if i > 0 {
                c.feed = Feed::Scoped(h.into());
                server.step(Input::Feed {
                    who: c.me.clone(),
                    feed: c.feed.clone(),
                });
            }
            clients.push(c);
        }
        let mut f = Fleet {
            server,
            clients,
            linked: vec![false; 3],
            up: vec![vec![]; 3],
            down: vec![vec![]; 3],
            seed,
        };
        (0..3).for_each(|i| f.heal(i));
        f
    }

    fn heard(&mut self, i: usize, o: Vec<Output>) {
        for x in o {
            if let (Output::Send(b), true) = (x, self.linked[i]) {
                self.up[i].push(b);
            }
        }
    }

    fn deliver_up(&mut self, i: usize, b: Vec<u8>) {
        let o = self.server.step(Input::Frame {
            from: Some(self.clients[i].me.clone()),
            bytes: b,
        });
        for x in o {
            match x {
                Output::Send(b) => self.down[i].push(b),
                Output::Fanout(b) => (0..3)
                    .filter(|j| self.linked[*j] && self.clients[*j].feed == Feed::Whole)
                    .for_each(|j| self.down[j].push(b.clone())),
                Output::To { session, bytes } => {
                    let j = self.clients.iter().position(|c| c.me.session == session).unwrap();
                    if self.linked[j] {
                        self.down[j].push(bytes);
                    }
                }
                _ => {}
            }
        }
    }

    fn deliver_down(&mut self, i: usize, b: Vec<u8>) {
        let o = self.clients[i].step(Input::Frame { from: None, bytes: b });
        self.heard(i, o);
    }

    fn mutate(&mut self, i: usize, track: &str) {
        let pid = EntryId::of(Origin::named("peer-0-session"), 1).draw(0);
        let o = self.clients[i].step(mutate_input(ADD, pair_input(&pid, track)));
        self.heard(i, o);
    }

    fn partition(&mut self, i: usize) {
        self.linked[i] = false;
        self.up[i].clear();
        self.down[i].clear();
    }

    fn heal(&mut self, i: usize) {
        if !self.linked[i] {
            self.linked[i] = true;
            let o = self.clients[i].step(Input::Connected);
            self.heard(i, o);
        }
    }

    fn roll(&mut self) -> u64 {
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.seed >> 11
    }

    fn step(&mut self) {
        let r = self.roll();
        let mut ready: Vec<(usize, bool)> = (0..3).filter(|i| !self.up[*i].is_empty()).map(|i| (i, true)).collect();
        ready.extend((0..3).filter(|i| !self.down[*i].is_empty()).map(|i| (i, false)));
        if ready.is_empty() {
            return;
        }
        let (i, upward) = ready[(r % ready.len() as u64) as usize];
        let times = match self.roll() % 16 {
            0 => 0,
            1 | 2 => 2,
            _ => 1,
        };
        let b = if upward { self.up[i].remove(0) } else { self.down[i].remove(0) };
        for _ in 0..times {
            if upward {
                self.deliver_up(i, b.clone())
            } else {
                self.deliver_down(i, b.clone())
            }
        }
    }

    /// Every client fed by the scope holds exactly its union of the
    /// authority's state at its cursor: no row outside it, nothing in it
    /// missing.
    fn unions_hold(&self) -> Result<(), String> {
        for c in self.clients.iter().filter(|c| c.feed != Feed::Whole) {
            let at = c.log().head().0;
            if confirmed(c) != union_of(&state_at(&self.server, at), &c.me.actor) {
                return Err(format!("{} at {at} does not hold its union", c.me.actor));
            }
        }
        Ok(())
    }

    fn settle(&mut self) {
        for i in 0..3 {
            self.partition(i);
            self.heal(i);
        }
        for _ in 0..1000 {
            if self.up.iter().chain(&self.down).all(|q| q.is_empty()) {
                return;
            }
            for i in 0..3 {
                for b in std::mem::take(&mut self.up[i]) {
                    self.deliver_up(i, b);
                }
                for b in std::mem::take(&mut self.down[i]) {
                    self.deliver_down(i, b);
                }
            }
        }
        panic!("the fleet did not settle");
    }
}

/// A replica fed by a scope (§1.8) in a fleet holds exactly its union
/// after every delivery, mutation, partition and heal — v4's `sim.rs`
/// partial-client assertion — and at the end every client is settled: the
/// whole one the authority's store, each scoped one its union of it,
/// nothing pending. Each client adds tracks of its own and of the others'
/// names, so facts cross every union's line. Falsified by the authority
/// sending a scoped connection the entry's facts whole: at seed 7's second
/// op peer-1 held tracks outside its union.
#[test]
fn a_scoped_replica_holds_its_union() {
    for seed in [7u64, 11, 23] {
        let mut f = Fleet::new(seed);
        let o = f.clients[0].step(mutate_input(CREATE, create_input("Fleet")));
        f.heard(0, o);
        f.settle();
        f.unions_hold().unwrap();
        for k in 0..30u64 {
            match (f.roll() % 10, k) {
                (0, _) => f.partition((k % 3) as usize),
                (1, _) => f.heal((k % 3) as usize),
                (r, _) if r < 6 => {
                    let (i, j) = ((k % 3) as usize, (r % 3) as usize);
                    f.mutate(i, &format!("peer-{j}:{k}"));
                }
                _ => (0..4).for_each(|_| f.step()),
            }
            f.unions_hold().unwrap_or_else(|e| panic!("seed {seed}, op {k}: {e}"));
        }
        f.settle();
        f.unions_hold().unwrap();
        for c in &f.clients {
            assert!(c.pending.is_empty(), "{} settled", c.me.actor);
            let want = if c.feed == Feed::Whole {
                f.server.store.clone()
            } else {
                union_of(&f.server.store, &c.me.actor)
            };
            assert!(c.store == want, "{} holds what it is fed", c.me.actor);
            let (held, all) = (c.store.rows("item").count(), f.server.store.rows("item").count());
            assert!(
                c.feed == Feed::Whole || (0 < held && held < all),
                "{} holds some items, not all",
                c.me.actor
            );
        }
        assert!(f.server.store.rows("item").count() > 5, "tracks were added");
    }
}
