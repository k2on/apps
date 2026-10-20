//! §1.10 maintained views and §1.8 a scoped feed. v4's `views/` vectors
//! are re-derived under `spec5/vectors/views/`: the same scenarios — the
//! library of `spec/vectors/views` (playlists, people, works, movements,
//! songs), the tracks of `kept-max` and `has-search`, the two fuzz
//! findings on a maintained sum — their stores and batches as they were,
//! each query written in the IR. After every batch the view's answer is a
//! fresh evaluation's and its patches splice the answer before into it
//! (the contract); a change costs reads by what it hits, not by the size
//! of what is viewed (the work contract); a view told a replica's
//! `Output::Changes` holds through a rebase; and a replica fed by a scope
//! holds exactly its union after every delivery.
//!
//! The library's schema declares the indexes its reads go through
//! (`work.composer`, `movement.work_id`, `song.creator`,
//! `song.movement_id`); v4's had none and read by value. `kept-max`'s text
//! index is gone with v4's `has` (here `split_once` of the lowered title);
//! the fuzz schema's unique index over an optional column is not unique,
//! since v5's index holds `None` as a value.

#[path = "demo.rs"]
mod demo;
mod support {
    pub mod counting;
    pub mod json;
    pub mod vectors;
}

use std::time::Instant;

use ark5::check::Types;
use ark5::codec::{bytes_enc, put_bytes, put_int, put_option};
use ark5::hash::FnHash;
use ark5::interp::{Interp, V};
use ark5::ir::{CtxField, Decl, Effect, Expr, Function, Node, Op, Std};
use ark5::log::{EntryId, Origin};
use ark5::module::Lib;
use ark5::peer::{Ctx, Feed, Input, Journal, Output, Peer, Role, Who};
use ark5::schema::{Schema, Table};
use ark5::store::{Change, Dir, Read, Store};
use ark5::ty::Ty;
use ark5::view::{query, splice, Patch, View};
use demo::*;
use support::counting::Counting;
use support::json::{array, change as jchange, hexed, int as jint, object, quoted, schema as jschema, store as jstore, val};
use support::vectors::{falsify, vector};

// * Rows and changes

fn texts(ts: &[&str]) -> Vec<u8> {
    let mut b = vec![];
    ts.iter().for_each(|t| put_bytes(&mut b, t.as_bytes()));
    b
}

type Row = (&'static str, Vec<u8>);

fn pl(n: u32, name: &str, user: &str) -> Row {
    ("playlist", playlist_row(&pid(n), name, user))
}

fn it(n: u32, track: &str, pos: i64) -> Row {
    ("item", item_row(&pid(n), track, pos))
}

fn person(name: &str, born: i64) -> Row {
    let mut b = texts(&[name]);
    put_int(&mut b, born);
    ("person", b)
}

fn work(id: &str, composer: &str, title: &str) -> Row {
    ("work", texts(&[id, composer, title]))
}

fn mv(id: &str, work: &str, no: i64, part: &str) -> Row {
    let mut b = texts(&[id, work]);
    put_int(&mut b, no);
    put_bytes(&mut b, part.as_bytes());
    ("movement", b)
}

fn song(id: &str, title: &str, creator: &str, m: Option<&str>, pos: i64) -> Row {
    let mut b = texts(&[id, title, creator]);
    put_option(&mut b, m, |o, m| put_bytes(o, m.as_bytes()));
    put_int(&mut b, pos);
    ("song", b)
}

fn track(id: &str, title: &str, creator: &str, pos: i64) -> Row {
    let mut b = texts(&[id, title, creator]);
    put_int(&mut b, pos);
    ("track", b)
}

/// A field of a fuzz schema's row.
enum F<'a> {
    I(i64),
    T(&'a str),
    Bs(&'a [u8]),
    Id(&'a str),
    E(u8),
    Opt(Option<&'a F<'a>>),
}
use F::*;

fn put_f(b: &mut Vec<u8>, f: &F) {
    match f {
        I(n) => put_int(b, *n),
        T(t) => put_bytes(b, t.as_bytes()),
        Bs(x) => put_bytes(b, x),
        Id(h) => put_bytes(b, &ark5::hash::unhex(h).unwrap()),
        E(i) => b.push(*i),
        Opt(x) => put_option(b, *x, put_f),
    }
}

fn fz(table: &'static str, fs: &[F]) -> Row {
    let mut b = vec![];
    fs.iter().for_each(|f| put_f(&mut b, f));
    (table, b)
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
    let text = || Ty::Bytes;
    s.tables.extend([
        Table::new("person").col("name", "Text", text()).col("born", "Int", Ty::Int),
        Table::new("work")
            .col("id", "Text", text())
            .col("composer", "Text", text())
            .col("title", "Text", text())
            .index(&["composer"], false)
            .refs("composer", "person"),
        Table::new("movement")
            .col("id", "Text", text())
            .col("work_id", "Text", text())
            .col("no", "Int", Ty::Int)
            .col("part", "Text", text())
            .index(&["work_id"], false)
            .refs("work_id", "work"),
        Table::new("song")
            .col("id", "Text", text())
            .col("title", "Text", text())
            .col("creator", "Text", text())
            .col("movement_id", "Option<Text>", Ty::option(text()))
            .col("pos", "Int", Ty::Int)
            .index(&["creator"], false)
            .index(&["movement_id"], false)
            .refs("movement_id", "movement"),
    ]);
    s
}

fn tracks() -> Schema {
    Schema {
        tables: vec![
            Table::new("person").col("name", "Text", Ty::Bytes).col("born", "Int", Ty::Int),
            Table::new("track")
                .col("id", "Text", Ty::Bytes)
                .col("title", "Text", Ty::Bytes)
                .col("creator", "Text", Ty::Bytes)
                .col("pos", "Int", Ty::Int)
                .index(&["creator", "pos"], false),
        ],
    }
}

fn color() -> Ty {
    Ty::option(Ty::enumeration(3))
}

/// `a-maintained-sum-answers-where-the-fold-overflows`'s schema.
fn fuzz_colors() -> Schema {
    Schema {
        tables: vec![
            Table::new("t0")
                .col("id", "Id<t0>", Ty::Bytes)
                .col("c00", "Text", Ty::Bytes)
                .col("c01", "Bytes", Ty::Bytes)
                .col("c02", "Option<Text>", Ty::option(Ty::Bytes))
                .index(&["c02"], false),
            Table::new("t1")
                .col("k1", "Int", Ty::Int)
                .col("c10", "Text", Ty::Bytes)
                .col("c11", "Text", Ty::Bytes)
                .col("c12", "Option<Color>", color()),
        ],
    }
}

/// `a-maintained-sum-overflows-where-the-fold-answers`'s schema.
fn fuzz_pairs() -> Schema {
    Schema {
        tables: vec![
            Table::new("t0")
                .col("p0", "Text", Ty::Bytes)
                .col("n0", "Int", Ty::Int)
                .col("c00", "Int", Ty::Int)
                .keyed(2)
                .index(&["c00"], false),
            Table::new("t1")
                .col("p1", "Text", Ty::Bytes)
                .col("n1", "Int", Ty::Int)
                .col("c10", "Option<Bool>", Ty::option(Ty::bool()))
                .col("c11", "Bool", Ty::bool())
                .col("c12", "Int", Ty::Int)
                .col("c13", "Option<Text>", Ty::option(Ty::Bytes))
                .keyed(2),
        ],
    }
}

// * The queries, in the IR

fn query_fn(types: Vec<(String, Decl)>, params: &[(&str, Ty)], ret: Ty, body: impl FnOnce(&mut B) -> Expr) -> Function {
    func(types, params, Ty::list(ret), Effect::Read, body)
}

fn sort(l: Expr, keys: Expr, dirs: &[Dir]) -> Expr {
    Expr::Sort(Box::new(l), Box::new(keys), dirs.to_vec())
}

fn take(l: Expr, n: i64) -> Expr {
    Expr::Take(Box::new(l), Box::new(int(n)))
}

fn map(l: Expr, f: Expr) -> Expr {
    Expr::Map(Box::new(l), Box::new(f))
}

fn group(l: Expr, by: Expr, agg: Expr) -> Expr {
    Expr::Group(Box::new(l), Box::new(by), Box::new(agg))
}

fn tuple(es: Vec<Expr>) -> Expr {
    Expr::Tuple(es)
}

fn fields(b: &B, v: &str, ns: &[u32]) -> Expr {
    tuple(ns.iter().map(|n| field(b.v(v), *n)).collect())
}

fn len(e: Expr) -> Expr {
    std(Std::Len, vec![e])
}

fn match_(e: Expr, arms: Vec<Expr>) -> Expr {
    Expr::Match(Box::new(e), arms)
}

fn item_ty() -> Ty {
    Ty::Tuple(vec![named("Id<playlist>"), text_ty(), Ty::Int])
}

fn playlist_ty() -> Ty {
    Ty::Tuple(vec![named("Id<playlist>"), text_ty(), text_ty()])
}

fn n_name() -> Ty {
    Ty::Tuple(vec![Ty::Int, text_ty()])
}

/// `Option<Text>`, which a song's `movement_id` is declared as.
fn opt_text() -> (String, Decl) {
    ("Option<Text>".into(), decl(Ty::option(text_ty()), &[]))
}

fn song_ty() -> Ty {
    Ty::Tuple(vec![text_ty(), text_ty(), text_ty(), named("Option<Text>"), Ty::Int])
}

/// The rows of `table` whose column `c` is `e`, `e` of the scope outside.
fn related(b: &mut B, table: &str, c: u32, e: impl Fn(&B) -> Expr) -> Expr {
    let on = b.lam(&["r"], |b| op(Op::Eq, field(b.v("r"), c), e(b)));
    filter(scan(table), on)
}

/// `top-two-by-pos(playlist_id)`: the playlist's items by `pos`, the first two.
fn top_two_by_pos() -> Function {
    query_fn(vec![id_decl()], &[("playlist_id", named("Id<playlist>"))], item_ty(), |b| {
        let mine = related(b, "item", 0, |b| b.v("playlist_id"));
        let by = b.lam(&["i"], |b| fields(b, "i", &[2, 0, 1]));
        take(sort(mine, by, &[Dir::Asc; 3]), 2)
    })
}

/// `playlist-with-items`: playlists by name, each with its last three items.
fn playlist_with_items() -> Function {
    let ret = Ty::Tuple(vec![named("Id<playlist>"), text_ty(), text_ty(), Ty::list(item_ty())]);
    query_fn(vec![id_decl()], &[], ret, |b| {
        let by = b.lam(&["p"], |b| fields(b, "p", &[1, 0]));
        let each = b.lam(&["p"], |b| {
            let items = related(b, "item", 0, |b| field(b.v("p"), 0));
            let by = b.lam(&["i"], |b| fields(b, "i", &[2, 0, 1]));
            let last = take(sort(items, by, &[Dir::Desc, Dir::Asc, Dir::Asc]), 3);
            tuple(vec![field(b.v("p"), 0), field(b.v("p"), 1), field(b.v("p"), 2), last])
        });
        map(sort(scan("playlist"), by, &[Dir::Asc; 2]), each)
    })
}

/// `projection`: songs by position, as `(n, name)`.
fn projection() -> Function {
    query_fn(vec![], &[], n_name(), |b| {
        let by = b.lam(&["s"], |b| fields(b, "s", &[4, 0]));
        let each = b.lam(&["s"], |b| fields(b, "s", &[4, 1]));
        map(sort(scan("song"), by, &[Dir::Asc; 2]), each)
    })
}

/// `(how many movements, title)` of each work by title, then id.
fn works_with_counts(b: &mut B, with_id: bool) -> Expr {
    let by = b.lam(&["w"], |b| fields(b, "w", &[2, 0]));
    let each = b.lam(&["w"], |b| {
        let n = len(related(b, "movement", 1, |b| field(b.v("w"), 0)));
        let mut es = vec![n, field(b.v("w"), 2)];
        es.extend(with_id.then(|| field(b.v("w"), 0)));
        tuple(es)
    });
    map(sort(scan("work"), by, &[Dir::Asc; 2]), each)
}

/// `having-admits-on-a-child`: works with a movement, `(n, name)`.
fn having_admits_on_a_child() -> Function {
    query_fn(vec![], &[], n_name(), |b| {
        let some = b.lam(&["x"], |b| op(Op::Gt, field(b.v("x"), 0), int(0)));
        filter(works_with_counts(b, false), some)
    })
}

/// `group-source`: songs grouped by creator, each with how many, in the
/// creator's order.
fn group_source() -> Function {
    query_fn(vec![], &[], Ty::Tuple(vec![text_ty(), Ty::Int]), |b| {
        let by = b.lam(&["s"], |b| field(b.v("s"), 2));
        let n = b.lam(&["k", "ms"], |b| len(b.v("ms")));
        let key = b.lam(&["g"], |b| field(b.v("g"), 0));
        sort(group(scan("song"), by, n), key, &[Dir::Asc])
    })
}

/// `lookup-chain`: songs by position, each with its movement's part and its
/// work's title, looked up, `""` for none.
fn lookup_chain() -> Function {
    query_fn(vec![opt_text()], &[], Ty::Tuple(vec![text_ty(), text_ty(), text_ty()]), |b| {
        let by = b.lam(&["s"], |b| fields(b, "s", &[4, 0]));
        let each = b.lam(&["s"], |b| {
            let none = tuple(vec![text(""), field(b.v("s"), 1), text("")]);
            let some = b.with(&["mid"], |b| {
                let none = tuple(vec![text(""), field(b.v("s"), 1), text("")]);
                let found = b.with(&["m"], |b| {
                    let title = b.with(&["w"], |b| field(b.v("w"), 2));
                    let work = match_(lookup("work", vec![field(b.v("m"), 1)]), vec![text(""), title]);
                    tuple(vec![field(b.v("m"), 3), field(b.v("s"), 1), work])
                });
                match_(lookup("movement", vec![b.v("mid")]), vec![none, found])
            });
            match_(field(b.v("s"), 3), vec![none, some])
        });
        map(sort(scan("song"), by, &[Dir::Asc; 2]), each)
    })
}

/// `related-on-a-non-key-column`: each person, by name, with how many
/// songs they made.
fn related_on_a_non_key_column() -> Function {
    query_fn(vec![], &[], n_name(), |b| {
        let each = b.lam(&["p"], |b| {
            tuple(vec![len(related(b, "song", 2, |b| field(b.v("p"), 0))), field(b.v("p"), 0)])
        });
        map(scan("person"), each)
    })
}

/// `expression-order-under-a-limit`: the two works with the most
/// movements, then by id.
fn expression_order_under_a_limit() -> Function {
    query_fn(vec![], &[], n_name(), |b| {
        let by = b.lam(&["x"], |b| fields(b, "x", &[0, 2]));
        let top = take(sort(works_with_counts(b, true), by, &[Dir::Desc, Dir::Asc]), 2);
        let each = b.lam(&["x"], |b| fields(b, "x", &[0, 1]));
        map(top, each)
    })
}

/// `related-three-deep`: each person with their works, each with its
/// movements, each with its songs.
fn related_three_deep() -> Function {
    let movement = Ty::Tuple(vec![text_ty(), text_ty(), Ty::Int, text_ty(), Ty::list(song_ty())]);
    let work = Ty::Tuple(vec![text_ty(), text_ty(), text_ty(), Ty::list(movement)]);
    query_fn(vec![opt_text()], &[], Ty::Tuple(vec![text_ty(), Ty::Int, Ty::list(work)]), |b| {
        let each = b.lam(&["p"], |b| {
            let works = related(b, "work", 1, |b| field(b.v("p"), 0));
            let each = b.lam(&["w"], |b| {
                let movements = related(b, "movement", 1, |b| field(b.v("w"), 0));
                let each = b.lam(&["m"], |b| {
                    let songs = related(b, "song", 3, |b| Expr::Variant(named("Option<Text>"), 1, vec![field(b.v("m"), 0)]));
                    tuple(vec![
                        field(b.v("m"), 0),
                        field(b.v("m"), 1),
                        field(b.v("m"), 2),
                        field(b.v("m"), 3),
                        songs,
                    ])
                });
                tuple(vec![field(b.v("w"), 0), field(b.v("w"), 1), field(b.v("w"), 2), map(movements, each)])
            });
            tuple(vec![field(b.v("p"), 0), field(b.v("p"), 1), map(works, each)])
        });
        map(scan("person"), each)
    })
}

/// Beside v4's: playlists joined with their items on the playlist's id.
fn playlist_join_item() -> Function {
    query_fn(vec![id_decl()], &[], Ty::Tuple(vec![playlist_ty(), item_ty()]), |b| {
        let on = b.lam(&["p", "i"], |b| op(Op::Eq, field(b.v("p"), 0), field(b.v("i"), 0)));
        Expr::Join(Box::new(scan("playlist")), Box::new(scan("item")), Box::new(on))
    })
}

/// `kept-max`: each person with the greatest `pos` of their tracks, `-1`
/// for none.
fn kept_max() -> Function {
    query_fn(vec![], &[], n_name(), |b| {
        let each = b.lam(&["p"], |b| {
            let theirs = related(b, "track", 2, |b| field(b.v("p"), 0));
            let by = b.lam(&["t"], |b| field(b.v("t"), 3));
            let last = std(Std::First, vec![take(sort(theirs, by, &[Dir::Desc]), 1)]);
            let pos = b.with(&["t"], |b| field(b.v("t"), 3));
            tuple(vec![match_(last, vec![int(-1), pos]), field(b.v("p"), 0)])
        });
        map(scan("person"), each)
    })
}

/// `has-search(needle)`: tracks whose title has the needle, either case,
/// by position.
fn has_search() -> Function {
    query_fn(vec![], &[("needle", text_ty())], n_name(), |b| {
        let has = b.lam(&["t"], |b| {
            let lower = |e: Expr| std(Std::Lower, vec![e]);
            let split = std(Std::SplitOnce, vec![lower(field(b.v("t"), 1)), lower(b.v("needle"))]);
            std(Std::IsSome, vec![split])
        });
        let by = b.lam(&["t"], |b| fields(b, "t", &[3, 0]));
        let each = b.lam(&["t"], |b| fields(b, "t", &[3, 1]));
        map(sort(filter(scan("track"), has), by, &[Dir::Asc; 2]), each)
    })
}

/// `(key, how many, the sum of column c)` per group.
fn counted(b: &mut B, c: u32) -> Expr {
    b.lam(&["k", "ms"], |b| {
        let col = b.lam(&["m"], |b| field(b.v("m"), c));
        tuple(vec![b.v("k"), len(b.v("ms")), std(Std::Sum, vec![map(b.v("ms"), col)])])
    })
}

/// `groups_t1_1`: rows grouped by colour, each with how many and the sum
/// of their keys, by colour descending.
fn groups_by_colour() -> Function {
    query_fn(vec![], &[], Ty::Tuple(vec![color(), Ty::Int, Ty::Int]), |b| {
        let by = b.lam(&["r"], |b| field(b.v("r"), 3));
        let agg = counted(b, 0);
        let key = b.lam(&["g"], |b| field(b.v("g"), 0));
        let each = b.lam(&["g"], |b| field(b.v("g"), 1));
        map(sort(group(scan("t1"), by, agg), key, &[Dir::Desc]), each)
    })
}

/// `groups_t0_2(f_p0)`: rows not of `f_p0` grouped by `c00`, the groups
/// of two or more, each with how many and the sum of `n0`.
fn groups_of_two() -> Function {
    query_fn(vec![], &[("f_p0", text_ty())], Ty::Tuple(vec![Ty::Int, Ty::Int, Ty::Int]), |b| {
        let other = b.lam(&["r"], |b| op(Op::Ne, field(b.v("r"), 0), b.v("f_p0")));
        let by = b.lam(&["r"], |b| field(b.v("r"), 2));
        let agg = counted(b, 1);
        let two = b.lam(&["g"], |b| op(Op::Ge, field(field(b.v("g"), 1), 1), int(2)));
        let each = b.lam(&["g"], |b| field(b.v("g"), 1));
        map(filter(group(filter(scan("t0"), other), by, agg), two), each)
    })
}

// * The runner

fn ctx_of(user: &str) -> Ctx {
    Ctx {
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

/// A scenario: a query of a schema, its input, who reads it, a store and
/// the batches that move it.
struct Case {
    schema: Schema,
    f: Function,
    input: Vec<u8>,
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

fn patch_json(cx: &Types, t: &Ty, p: &Patch) -> String {
    let tag = |s: &str| quoted(s);
    match p {
        Patch::Insert { at, node } => object(&[("at", jint(*at as i64)), ("node", val(cx, t, node)), ("t", tag("insert"))]),
        Patch::Remove { at } => object(&[("at", jint(*at as i64)), ("t", tag("remove"))]),
        Patch::Update { at, node } => object(&[("at", jint(*at as i64)), ("node", val(cx, t, node)), ("t", tag("update"))]),
    }
}

/// The contract after one push: the answer is a fresh evaluation's, and
/// the patches splice the answer before into it.
fn holds((lib, h, input, ctx): (&Lib, &FnHash, &[u8], &Ctx), v: &View, store: &dyn Read, before: &[V], patches: &[Patch]) {
    let fresh = query(lib, h, input, ctx, store).unwrap();
    assert_eq!(v.rows(), fresh, "the view is a fresh evaluation");
    assert_eq!(splice(patches, before), fresh, "the patches splice");
}

/// The case run: hydrated over its store, then each batch applied whole
/// and pushed as one, the contract held after each; the vector's parts.
fn run(name: &str, c: &Case) -> Vec<(&'static str, String)> {
    let lib = Lib::default();
    let h = lib.install(&c.schema, c.f.clone()).unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut store = store_of(&c.schema, &c.before);
    let before = jstore(&c.schema, &store);
    let mut v = View::new(&lib, &h, &c.input, c.ctx.clone(), &store).unwrap();
    assert_eq!(v.rows(), query(&lib, &h, &c.input, &c.ctx, &store).unwrap(), "{name} hydrated");
    let (cx, t) = (
        Types::new(&c.f.types),
        match &c.f.ret {
            Ty::List(e) => (**e).clone(),
            _ => unreachable!(),
        },
    );
    let rows = |v: &View| array(v.rows().iter().map(|x| val(&cx, &t, x)));
    let rows_before = rows(&v);
    let mut steps = vec![];
    for (i, batch) in c.batches.iter().enumerate() {
        let was = v.rows();
        store.apply_all(batch).unwrap_or_else(|e| panic!("{name} batch {i}: {e}"));
        let patches = v.push(batch, &store).unwrap_or_else(|e| panic!("{name} batch {i}: {e}"));
        holds((&lib, &h, &c.input, &c.ctx), &v, &store, &was, &patches);
        let ps = array(patches.iter().map(|p| patch_json(&cx, &t, p)));
        steps.push(object(&[("patches", ps), ("rows", rows(&v))]));
    }
    let input_ty = Interp::input_ty(&c.f);
    let input = ark5::interp::decode(&cx, &input_ty, &c.input).unwrap();
    vec![
        ("query", quoted(name)),
        ("function_hash", quoted(&h.hex())),
        ("function_bytes", hexed(&c.f.to_bytes())),
        ("schema", jschema(&c.schema)),
        ("input", val(&cx, &input_ty, &input)),
        ("ctx", object(&[("session", quoted(&c.ctx.session)), ("user", quoted(&c.ctx.actor))])),
        ("store_before", before),
        ("rows_before", rows_before),
        ("batches", array(c.batches.iter().map(|b| array(b.iter().map(|x| jchange(&c.schema, x)))))),
        ("steps", format!("[{}]", steps.join("\n,"))),
    ]
}

fn library_case(f: Function, input: Vec<u8>) -> Case {
    Case {
        schema: library(),
        f,
        input,
        ctx: ctx(),
        before: library_before(),
        batches: library_batches(),
    }
}

fn tracks_case(f: Function, input: Vec<u8>) -> Case {
    Case {
        schema: tracks(),
        f,
        input,
        ctx: ctx(),
        before: tracks_before(),
        batches: tracks_batches(),
    }
}

/// Every library and tracks vector: the queries over v4's scenarios, the
/// contract held after every batch, the vector as on disk. Falsified by
/// the view's `Sort` not complementing a descending field's bytes:
/// `expression-order-under-a-limit` kept its two works fewest movements
/// first, and the fresh evaluation disagreed after its first batch.
#[test]
fn vectors() {
    let pid1 = bytes_enc(&pid(1));
    let cases = [
        ("top-two-by-pos", library_case(top_two_by_pos(), pid1)),
        ("playlist-with-items", library_case(playlist_with_items(), vec![])),
        ("projection", library_case(projection(), vec![])),
        ("having-admits-on-a-child", library_case(having_admits_on_a_child(), vec![])),
        ("group-source", library_case(group_source(), vec![])),
        ("lookup-chain", library_case(lookup_chain(), vec![])),
        ("related-on-a-non-key-column", library_case(related_on_a_non_key_column(), vec![])),
        ("expression-order-under-a-limit", library_case(expression_order_under_a_limit(), vec![])),
        ("related-three-deep", library_case(related_three_deep(), vec![])),
        ("playlist-join-item", library_case(playlist_join_item(), vec![])),
        ("kept-max", tracks_case(kept_max(), vec![])),
        ("has-search", tracks_case(has_search(), bytes_enc(b"ari"))),
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
    let right = run("top-two-by-pos", &library_case(top_two_by_pos(), bytes_enc(&pid(1))));
    let mut wrong = right.clone();
    let steps = &mut wrong.iter_mut().find(|p| p.0 == "steps").unwrap().1;
    let t3 = r#"{"at":{"$int":"1"},"node":[{"$id":"00000000-0000-0000-0000-000000000001"},"t3",{"$int":"3"}],"t":"insert"}"#;
    let t3_row = r#",[{"$id":"00000000-0000-0000-0000-000000000001"},"t3",{"$int":"3"}]]"#;
    let at = steps.find(t3).expect("the refill is in the vector");
    let next = steps[at..].find(t3_row).unwrap() + at;
    *steps = format!("{}{}{}", &steps[..at - 1], "]", &steps[at + t3.len() + 1..next]) + "]" + &steps[next + t3_row.len()..];
    falsify("views", "top-two-by-pos-no-refill.json", &wrong, &right);
}

/// v4's §6.5 rule, both ways, on the two fuzz findings: a kept sum is
/// moved in an `i128` and judged once, after the whole batch, as a fresh
/// `Std::Sum` judges its total. `a-maintained-sum-answers-where-the-fold-
/// overflows` sums `i64::MIN` and then `-2` on the way to a total that
/// fits; `…-overflows-where-the-fold-answers` passes through a group whose
/// sum is past `i64::MAX` inside one batch, which a view takes net, as one.
/// Falsified twice: by `Std::Sum` adding in a checked `i64`, the first
/// vector's fresh evaluation refused (`integer overflow`); by the runner
/// pushing the second's batch one change at a time, the push refused at
/// its fifth change.
#[test]
fn kept_sums() {
    let colours = Case {
        schema: fuzz_colors(),
        f: groups_by_colour(),
        input: vec![],
        ctx: ctx_of("peer-0"),
        before: fold_overflows_before(),
        batches: fold_overflows_batches(),
    };
    let name = "a-maintained-sum-answers-where-the-fold-overflows";
    vector("views", &format!("{name}.json"), &run(name, &colours));
    let pairs = Case {
        schema: fuzz_pairs(),
        f: groups_of_two(),
        input: bytes_enc(b"a"),
        ctx: ctx_of("peer-1"),
        before: kept_overflows_before(),
        batches: kept_overflows_batches(),
    };
    let name = "a-maintained-sum-overflows-where-the-fold-answers";
    vector("views", &format!("{name}.json"), &run(name, &pairs));
}

// * The work contract

/// What one push of `c` costs the view, read through a counting store;
/// the contract held after it.
fn cost(f: Function, input: &[u8], mut store: Store, c: Change) -> support::counting::Reads {
    let lib = Lib::default();
    let h = lib.install(&library(), f).unwrap();
    let mut v = View::new(&lib, &h, input, ctx(), &store).unwrap();
    let was = v.rows();
    store.apply(&c).unwrap();
    let counting = Counting::new(&store);
    let patches = v.push(std::slice::from_ref(&c), &counting).unwrap();
    let reads = counting.reads();
    holds((&lib, &h, input, &ctx()), &v, &store, &was, &patches);
    reads
}

/// The work contract (§1.10): a change costs reads by the entries it hits,
/// not by the rows viewed — the same at 1k, 4k and 16k rows. An item added
/// to one of `n / 4` playlists of `playlist-with-items` re-reads that
/// playlist's last three items through the `(playlist_id, pos)` index and
/// nothing else; the first item of `top-two-by-pos` leaving is refilled
/// from the view's own order, and a song changing creator under
/// `group-source` moves two kept counts — no read at all. Falsified by
/// recording a scan's reads as the whole table (the prefix dropped): every
/// playlist was hit, and the item cost 750 rows at 1k.
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
        let items = cost(playlist_with_items(), &[], store, add(it(n / 8, "t5", 5)));
        let ops = [remove(it(1, "t0", 1))];
        let top = cost(top_two_by_pos(), &bytes_enc(&pid(1)), scaled_items(n as usize), ops[0].clone());
        let (c0, c1) = (song("s0", "x", "c0", None, 0), song("s0", "x", "c1", None, 0));
        let groups = cost(group_source(), &[], scaled_songs(n as usize), edit(c0, c1));
        seen.push((items, top, groups));
    }
    let r = |gets, rows| support::counting::Reads { gets, rows };
    assert_eq!(seen[0], (r(0, 3), r(0, 0), r(0, 0)), "what one change reads");
    assert!(seen.iter().all(|s| *s == seen[0]), "at every size: {seen:?}");
}

/// The interpreter reads a filter's equalities and a sort's order through
/// the store's indexes (as v4's `scan_where_eq` and `scan_ordered`):
/// `add_to_playlist`'s last `pos` is one row of `(playlist_id, pos)` read
/// backwards, its two lookups and its insert's check three gets, and `create_playlist`'s name check
/// the rows of `(user_id, name)` under the person's name — at any size.
/// Falsified by routing nothing: `add_to_playlist` read 301 rows at 100.
#[test]
fn the_interpreter_reads_through_indexes() {
    for n in [100u32, 1600] {
        let mut rows = vec![add(pl(1, "Favorites", "alice"))];
        rows.extend((0..n).map(|k| add(it(1, &format!("t{k}"), k as i64 + 1))));
        rows.extend((2..n).map(|p| add(pl(p, &format!("p{p}"), "bob"))));
        let store = store_of(&demo::schema(), &rows);
        let counting = Counting::new(&store);
        let ctx = Ctx {
            id: EntryId::of("s", 1),
            ..ctx()
        };
        let run = |name: &str, input: Vec<u8>| demo::cas().get(&h(name)).unwrap().run(&counting, &ctx, &input).unwrap();
        let before = counting.reads();
        let changes = run(ADD, pair_input(&pid(1), "new"));
        assert!(matches!(&changes[..], [Change::Add { row, .. }] if item_of(row).2 == n as i64 + 1));
        let added = counting.reads();
        assert_eq!((added.gets - before.gets, added.rows - before.rows), (3, 1), "add_to_playlist at {n}");
        run(CREATE, create_input("Favorites"));
        let created = counting.reads();
        assert_eq!((created.gets - added.gets, created.rows - added.rows), (0, 1), "create_playlist at {n}");
    }
}

/// A track: `(id, title, creator, pos)`.
fn track_ty() -> Ty {
    Ty::Tuple(vec![text_ty(), text_ty(), text_ty(), Ty::Int])
}

/// A route answers as the interpreter does with no route: each read of
/// `track` (indexed `(creator, pos)`, not unique, positions tied) asked
/// once over the table and once over the same rows behind a `Map` no route
/// reads through — a sort by `pos` descending under a creator and by both
/// columns descending, cut to every count from 0 to 7, `first` of one,
/// and filters bounding `pos` and `creator` — the same answer for every
/// creator; and `first` of a creator's tracks by `pos` descending reads
/// one row when the greatest is alone, two when it is tied. Falsified
/// three times: by dropping the read of the rest of a run of ties, and by
/// not putting a descending run's ties back in key order (each: `take` of
/// 1 under `c0` gave `t21` where the stable sort gives `t09`); by reading
/// `1 < pos` as `pos < 1` (the bound not flipped), which read nothing.
#[test]
fn a_route_answers_as_the_unrouted_read() {
    let mut rows: Vec<Change> = (0..30).map(|i| add(track(&format!("t{i:02}"), "x", &format!("c{}", i % 3), (i / 3) % 4))).collect();
    rows.push(add(track("t99", "x", "c1", 9)));
    let store = store_of(&tracks(), &rows);
    let plain = Expr::Lambda(1, Box::new(Expr::Var(0)));
    fn mine(b: &mut B, src: Expr, more: Option<&dyn Fn(&B) -> Expr>) -> Expr {
        let on = b.lam(&["t"], |b| {
            let eq = op(Op::Eq, field(b.v("t"), 2), b.v("c"));
            more.map_or(eq.clone(), |m| op(Op::And, eq, m(b)))
        });
        filter(src, on)
    }
    type Body = Box<dyn Fn(&mut B, Expr) -> Expr>;
    let mut cases: Vec<(String, Ty, Body)> = vec![];
    for k in 0..=7 {
        cases.push((
            format!("a creator's by pos descending, {k}"),
            track_ty(),
            Box::new(move |b, src| {
                let by = b.lam(&["t"], |b| field(b.v("t"), 3));
                take(sort(mine(b, src, None), by, &[Dir::Desc]), k)
            }),
        ));
        cases.push((
            format!("all by creator and pos descending, {k}"),
            track_ty(),
            Box::new(move |b, src| {
                let by = b.lam(&["t"], |b| fields(b, "t", &[2, 3]));
                take(sort(src, by, &[Dir::Desc; 2]), k)
            }),
        ));
    }
    cases.push((
        "first of a creator's by pos descending".into(),
        Ty::option(track_ty()),
        Box::new(|b, src| {
            let by = b.lam(&["t"], |b| field(b.v("t"), 3));
            Expr::List(Ty::option(track_ty()), vec![std(Std::First, vec![sort(mine(b, src, None), by, &[Dir::Desc])])])
        }),
    ));
    cases.push((
        "first of a creator's".into(),
        Ty::option(track_ty()),
        Box::new(|b, src| Expr::List(Ty::option(track_ty()), vec![std(Std::First, vec![mine(b, src, None)])])),
    ));
    cases.push((
        "a creator's with 1 < pos <= 2, by pos descending".into(),
        track_ty(),
        Box::new(|b, src| {
            let within = |b: &B| op(Op::And, op(Op::Lt, int(1), field(b.v("t"), 3)), op(Op::Le, field(b.v("t"), 3), int(2)));
            let by = b.lam(&["t"], |b| field(b.v("t"), 3));
            sort(mine(b, src, Some(&within)), by, &[Dir::Desc])
        }),
    ));
    cases.push((
        "creators after c and up to c2".into(),
        track_ty(),
        Box::new(|b, src| {
            let on = b.lam(&["t"], |b| {
                let lo = op(Op::Gt, field(b.v("t"), 2), b.v("c"));
                op(Op::And, lo, op(Op::Ge, text("c2"), field(b.v("t"), 2)))
            });
            filter(src, on)
        }),
    ));
    let lib = Lib::default();
    for (name, ret, body) in &cases {
        let [routed, unrouted] = [scan("track"), map(scan("track"), plain.clone())]
            .map(|src| lib.install(&tracks(), query_fn(vec![], &[("c", text_ty())], ret.clone(), |b| body(b, src))).unwrap());
        for c in ["c", "c0", "c1", "c2", "c9"] {
            let input = bytes_enc(c.as_bytes());
            let want = query(&lib, &unrouted, &input, &ctx(), &store).unwrap();
            assert_eq!(query(&lib, &routed, &input, &ctx(), &store).unwrap(), want, "{name}, {c}");
        }
    }
    let (_, _, first) = &cases[16];
    let h = lib.install(&tracks(), query_fn(vec![], &[("c", text_ty())], Ty::option(track_ty()), |b| first(b, scan("track")))).unwrap();
    for (c, rows, id) in [("c1", 1, "t99"), ("c0", 2, "t09")] {
        let counting = Counting::new(&store);
        let got = query(&lib, &h, &bytes_enc(c.as_bytes()), &ctx(), &counting).unwrap();
        assert_eq!(counting.reads().rows, rows, "first of {c}'s");
        let (1, [V::Tup(t)]) = got[0].variant() else { panic!("{c}: {got:?}") };
        assert_eq!(t[0], V::Bytes(id.into()), "{c}");
    }
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
    let lib = Lib::default();
    let pid = EntryId::of(Origin::named("alice-session"), 1).draw(0);
    let queries = [(playlist_with_items(), vec![]), (top_two_by_pos(), bytes_enc(&pid))];
    let mut views: Vec<(FnHash, Vec<u8>, View)> = queries
        .into_iter()
        .map(|(f, input)| {
            let h = lib.install(&demo::schema(), f).unwrap();
            let v = View::new(&lib, &h, &input, ctx(), &alice.store).unwrap();
            (h, input, v)
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
        for (h, input, v) in views.iter_mut() {
            let was = v.rows();
            let patches = v.push(&cs, &alice.store).unwrap();
            holds((&lib, h, input, &ctx()), v, &alice.store, &was, &patches);
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

/// The scope of the fleet: every playlist, and the items whose track is
/// the reader's — `actor:…`. One `Pure` function of a row, `Sum` over the
/// demo's tables.
fn scope() -> Function {
    let row = Ty::Sum(vec![vec![playlist_ty()], vec![item_ty()]]);
    func(vec![id_decl()], &[("row", row)], Ty::bool(), Effect::Pure, |b| {
        let all = b.with(&["p"], |_| Expr::Variant(Ty::bool(), 1, vec![]));
        let mine = b.with(&["i"], |b| {
            let prefix = std(Std::Concat, vec![Expr::List(text_ty(), vec![ctx_user(), text(":")])]);
            std(Std::StartsWith, vec![field(b.v("i"), 1), prefix])
        });
        match_(b.v("row"), vec![all, mine])
    })
}

fn ctx_user() -> Expr {
    Expr::Ctx(CtxField::User)
}

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
    for c in server.log.after(seq).iter().rev().flat_map(|l| l.facts.iter().rev()) {
        s.apply(&c.inverse()).unwrap();
    }
    s
}

/// A fleet of an authority, a client fed whole and two fed by `scope`, on
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
        let mut cas = demo::cas();
        let h = cas.add_body(scope().to_bytes());
        assert!(cas.get(&h).is_some(), "the scope loads");
        let auth = Role::Authority { journal: Journal::default() };
        let mut server = Peer::new(demo::shared(), cas.clone(), auth, who_of("server"));
        let mut clients = vec![];
        for i in 0..3 {
            let mut c = Peer::new(demo::shared(), cas.clone(), Role::Replica, who_of(&format!("peer-{i}")));
            if i > 0 {
                c.feed = Feed::Scoped(h);
                server.step(Input::Feed {
                    who: c.me.clone(),
                    feed: c.feed,
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
            let at = c.log.head().0;
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

// * The prototype (plan-v5 §1.14): Sort+Take with refill, and a Group under
//   a Sort, timed at 1k/4k/16k rows against v4's on the same operations.
//   Run in release, `--ignored`; `ARK5_N`, `ARK5_ROUNDS` narrow it.

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

/// µs per push over `rounds` of `ops`, each applied then pushed; the view
/// held to a fresh evaluation at the end.
fn time(f: Function, input: &[u8], mut store: Store, ops: &[Change], rounds: usize) -> f64 {
    let lib = Lib::default();
    let h = lib.install(&library(), f).unwrap();
    let mut v = View::new(&lib, &h, input, ctx(), &store).unwrap();
    let mut spent = 0.0;
    for _ in 0..rounds {
        for c in ops {
            store.apply(c).unwrap();
            let t = Instant::now();
            v.push(std::slice::from_ref(c), &store).unwrap();
            spent += t.elapsed().as_secs_f64();
        }
    }
    assert_eq!(v.rows(), query(&lib, &h, input, &ctx(), &store).unwrap());
    spent * 1e6 / (rounds * ops.len()) as f64
}

#[test]
#[ignore]
fn prototype() {
    let env = |k: &str| std::env::var(k).ok().and_then(|n| n.parse::<usize>().ok());
    let rounds = env("ARK5_ROUNDS").unwrap_or(2000);
    for n in [1000, 4000, 16000].into_iter().filter(|n| env("ARK5_N").is_none_or(|o| o == *n)) {
        let ops = [remove(it(1, "t0", 1)), add(it(1, "t0", 1))];
        let tt = time(top_two_by_pos(), &bytes_enc(&pid(1)), scaled_items(n), &ops, rounds);
        let (c0, c1) = (song("s0", "x", "c0", None, 0), song("s0", "x", "c1", None, 0));
        let ops = [edit(c0.clone(), c1.clone()), edit(c1, c0)];
        let gs = time(group_source(), &[], scaled_songs(n), &ops, rounds);
        println!("v5 n={n}: top-two-by-pos {tt:.2} µs/push, group-source {gs:.2} µs/push");
    }
}
