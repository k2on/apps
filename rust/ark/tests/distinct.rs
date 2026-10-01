//! `docs/plan-db.md` D4: `.distinct(cols)` is a group with no aggregate —
//! `group_by(cols)` whose node is its key — so it is the explicit group form
//! byte for byte, verified and maintained as any group source is.

#[path = "support/churn.rs"]
mod churn;

use std::collections::BTreeSet;

use ark::authoring::*;
use ark::eval::{self, Args};
use ark::ir;
use ark::schema::Ty;
use ark::store::{Change, MemoryStore, Store};
use ark::value::Value;
use ark::view::{self, Env};
use churn::drive;

pub struct Lib {
    pub song: Table<Song>,
}
impl Tables for Lib {
    fn open() -> Self {
        Lib { song: table() }
    }
}

pub struct Song {
    pub id: Text,
    pub creator: Text,
    pub album: Opt<Text>,
    pub pos: Int,
}
impl Row for Song {
    const NAME: &str = "song";
    type Key = (Text,);
    fn columns() -> Columns<Self> {
        columns()
            .text(Self::id)
            .text(Self::creator)
            .text(Self::album)
            .nullable()
            .int(Self::pos)
            .key((Self::id,))
    }
}
#[allow(non_upper_case_globals)]
impl Song {
    pub const id: Col<Self, Text> = col("id");
    pub const creator: Col<Self, Text> = col("creator");
    pub const album: Col<Self, Opt<Text>> = col("album");
    pub const pos: Col<Self, Int> = col("pos");
}

fn lib() -> Router<Lib> {
    let r = router::<Lib>("lib");
    r.routes((
        r.query("creators", |_ctx, db, _input: ()| db.song.distinct(Song::creator)),
        r.query("creators_grouped", |_ctx, db, _input: ()| {
            db.song.group_by(Song::creator).map(|creator, _| creator)
        }),
        // A nullable column: `None` is one of its values.
        r.query("albums", |_ctx, db, _input: ()| db.song.filter(Song::pos.gt(0)).distinct(Song::album)),
        r.query("albums_grouped", |_ctx, db, _input: ()| {
            db.song.filter(Song::pos.gt(0)).group_by(Song::album).map(|album, _| album)
        }),
        // A tuple: the node is the key struct.
        r.query("pairs", |_ctx, db, _input: ()| db.song.distinct((Song::creator, Song::album))),
    ))
}

fn t(s: &str) -> Value {
    Value::text(s)
}

fn store(sch: &ark::schema::Schema) -> MemoryStore {
    let mut st = MemoryStore::empty(sch.clone());
    for (id, creator, album, pos) in [
        ("s1", "Gould", Some("Goldberg"), 1),
        ("s2", "Gould", Some("Goldberg"), 2),
        ("s3", "Gould", None, 3),
        ("s4", "Pinnock", Some("Brandenburg"), 0),
        ("s5", "Pinnock", Some("Water Music"), 5),
        ("s6", "Bach", Some("Goldberg"), 6),
    ] {
        let row = [
            ("id", t(id)),
            ("creator", t(creator)),
            ("album", album.map(t).unwrap_or(Value::Null)),
            ("pos", Value::int(pos)),
        ];
        st.apply_change(&Change::Add("song".into(), row.into_iter().map(|(k, v)| (k.to_string(), v)).collect()));
    }
    st
}

fn env() -> Env {
    Env {
        helpers: vec![],
        ctx: eval::Ctx::default(),
        args: Args::new(),
        provided: Args::new(),
    }
}

/// D4 `distinct(c)` is `group_by(c).map(|c, _| c)`: the same plan and the
/// same node type, filter and all; over a tuple the node is the key
/// struct. Each answers its distinct values in key order, and the module
/// built — so verified (§1.10) — with no rule added for it. Falsified by
/// projecting the key struct for one column too (`Expr::Var(row)` in
/// `Query::distinct`): `creators` is then not `creators_grouped`.
#[test]
fn distinct_is_the_group_with_its_key_for_a_node() {
    let m = Module::new((lib(),));
    let built = m.build();
    let f = |n: &str| built.lookup_function(n).unwrap_or_else(|| panic!("{n}"));
    for (a, b) in [("creators", "creators_grouped"), ("albums", "albums_grouped")] {
        assert_eq!(f(a).plan, f(b).plan, "{a} is {b}");
        assert_eq!(f(a).ret, f(b).ret, "{a} is {b}");
    }
    assert_eq!(f("creators").ret, Some(Ty::List(Box::new(Ty::Text))));
    let pairs = f("pairs").plan.clone().unwrap();
    assert!(matches!(&pairs.source, ir::Source::Group { by, .. } if by == &["creator", "album"]));
    assert_eq!(pairs.project, pairs.row.map(ir::Expr::Var));

    let sch = &built.schema;
    let st = store(sch);
    let env = env();
    let read = |n: &str| view::read(sch, f(n).plan.as_ref().unwrap(), &env.scope(sch), &st).expect("read");
    assert_eq!(read("creators"), vec![t("Bach"), t("Gould"), t("Pinnock")]);
    assert_eq!(
        read("albums"),
        vec![Value::Null, t("Goldberg"), t("Water Music")],
        "Brandenburg is filtered out"
    );
    let distinct: BTreeSet<(Value, Value)> = st.scan("song").iter().map(|r| (r["creator"].clone(), r["album"].clone())).collect();
    let got: Vec<(Value, Value)> = read("pairs").iter().map(|n| (n.field("creator"), n.field("album"))).collect();
    assert_eq!(got, distinct.into_iter().collect::<Vec<_>>());
}

/// A value appears with its first row and goes with its last, under churn.
#[test]
fn distinct_is_maintained_as_a_group() {
    let m = Module::new((lib(),));
    let built = m.build();
    for name in ["creators", "albums", "pairs"] {
        let plan = built.lookup_function(name).unwrap().plan.clone().unwrap();
        let st = store(&built.schema);
        let mut moves = 0;
        for seed in 0..8 {
            let tally = drive(name, &built.schema, &plan, &env(), st.clone(), seed, 50);
            assert!(tally.inserts > 0 && tally.removes > 0, "{name}: {tally:?}");
            moves += tally.moves;
        }
        assert!(moves > 0, "{name}: rows moved between values");
    }
}
