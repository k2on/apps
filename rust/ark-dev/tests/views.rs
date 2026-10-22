//! `spec/vectors/views/` (§1.10), written by the vector writer and run as
//! `spec/RUNNERS.md` § views says: each vector is a view named in a
//! reference domain (`ark_dev::library`), its input, a store and batches of
//! row changes, and the rows and patches after each batch. v4's scenarios —
//! the library (playlists, people, works, movements, songs), the tracks of
//! `kept-max` and `has-search`, the two fuzz findings on a maintained sum —
//! their stores and batches as they were. After every batch the view is its
//! answer afresh and a view made again, and its patches splice the rows
//! before into it (the contract); a change costs reads by what it hits, not
//! by the size of what is viewed (the work contract).
//!
//! The patches are the ones v4's language engine gave: before it was
//! deleted, every vector's patches and rows were checked against it, step by
//! step (N1), and again when the plans written as JSON became these named
//! views (S4): every step the same.

use std::collections::BTreeSet;

use ark::hash::hex;
use ark::row::Value as _;
use ark::row::{Bytes, Id};
use ark::store::{Change, Store};
use ark_dev::library::colors::{self, Color};
use ark_dev::library::{pairs, Item, Movement, Person, Playlist, Song, Track, Work};
use ark_dev::vectors::{on_disk, run_view, vector, view_ctx, view_vector};
use serde_json::{json, Value};

// * Rows and changes

type Row = (&'static str, Vec<u8>);

fn pid(n: u32) -> Id<Playlist> {
    Id::new([&[0; 12][..], &n.to_be_bytes()].concat().try_into().unwrap())
}

fn pl(n: u32, name: &str, user: &str) -> Row {
    let p = Playlist {
        id: pid(n),
        name: name.into(),
        user_id: user.into(),
    };
    ("playlist", p.encode())
}

fn it(n: u32, track: &str, pos: i64) -> Row {
    let i = Item {
        playlist_id: pid(n),
        track_id: track.into(),
        pos,
    };
    ("item", i.encode())
}

fn person(name: &str, born: i64) -> Row {
    ("person", Person { name: name.into(), born }.encode())
}

fn work(id: &str, composer: &str, title: &str) -> Row {
    let w = Work {
        id: id.into(),
        composer: composer.into(),
        title: title.into(),
    };
    ("work", w.encode())
}

fn mv(id: &str, work: &str, no: i64, part: &str) -> Row {
    let m = Movement {
        id: id.into(),
        work_id: work.into(),
        no,
        part: part.into(),
    };
    ("movement", m.encode())
}

fn song(id: &str, title: &str, creator: &str, m: Option<&str>, pos: i64) -> Row {
    let s = Song {
        id: id.into(),
        title: title.into(),
        creator: creator.into(),
        movement_id: m.map(String::from),
        pos,
    };
    ("song", s.encode())
}

fn track(id: &str, title: &str, creator: &str, pos: i64) -> Row {
    let t = Track {
        id: id.into(),
        title: title.into(),
        creator: creator.into(),
        pos,
    };
    ("track", t.encode())
}

/// `colors`' `t0`: an id by its hex, two texts and bytes.
fn c0(id: &str, c00: &str, c01: &[u8], c02: Option<&str>) -> Row {
    let r = colors::T0 {
        id: Id::new(ark::hash::unhex(id).unwrap().try_into().unwrap()),
        c00: c00.into(),
        c01: Bytes(c01.to_vec()),
        c02: c02.map(String::from),
    };
    ("t0", r.encode())
}

/// `colors`' `t1`: a colour by its place.
fn c1(k1: i64, c10: &str, c11: &str, c12: Option<u8>) -> Row {
    let r = colors::T1 {
        k1,
        c10: c10.into(),
        c11: c11.into(),
        c12: c12.map(|i| [Color::c0, Color::c1, Color::c2][i as usize].clone()),
    };
    ("t1", r.encode())
}

fn p0(p0: &str, n0: i64, c00: i64) -> Row {
    ("t0", pairs::T0 { p0: p0.into(), n0, c00 }.encode())
}

/// `pairs`' `t1`: an optional boolean by its place (`false`, `true`).
fn p1(p1: &str, n1: i64, c10: Option<u8>, c11: u8, c12: i64, c13: Option<&str>) -> Row {
    let r = pairs::T1 {
        p1: p1.into(),
        n1,
        c10: c10.map(|b| b == 1),
        c11: c11 == 1,
        c12,
        c13: c13.map(String::from),
    };
    ("t1", r.encode())
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

const F: &str = "f0000000000000000000000000000";

fn colors_before() -> Vec<Change> {
    vec![
        add(c0(&format!("{F}003"), "", &[], None)),
        add(c0(&format!("{F}008"), "", &[], None)),
        add(c0(&format!("{F}00c"), "a", &[0], None)),
        add(c0(&format!("{F}015"), "A", &[0], None)),
        add(c0(&format!("{F}01b"), "x y", &[], None)),
        add(c0(&format!("{F}01e"), "ab", &[], None)),
        add(c0(&format!("{F}023"), "a", &[0], None)),
        add(c1(i64::MIN, "a", "b", None)),
        add(c1(0, "zz", "b", Some(1))),
        add(c1(2, "b", "b", Some(2))),
        add(c1(1099511627776, "b", "x y", None)),
    ]
}

fn colors_batches() -> Vec<Vec<Change>> {
    let f23 = format!("{F}023");
    vec![
        vec![add(c1(-2, "b", "x y", Some(2))), add(c1(9, "x2", "b", Some(2)))],
        vec![edit(c0(&f23, "a", &[0], None), c0(&f23, "a", &[0], Some("x5")))],
        vec![
            edit(c1(-2, "b", "x y", Some(2)), c1(-2, "b", "b", Some(2))),
            add(c1(3, "b", "x7", None)),
            add(c1(8, "x2", "x7", Some(2))),
        ],
        vec![remove(c1(2, "b", "b", Some(2))), remove(c1(9, "x2", "b", Some(2)))],
        vec![
            edit(c1(3, "b", "x7", None), c1(3, "b", "x7", Some(2))),
            edit(c0(&f23, "a", &[0], Some("x5")), c0(&f23, "x y", &[0], Some("x5"))),
            remove(c1(0, "zz", "b", Some(1))),
            add(c1(6, "a", "x y", Some(2))),
        ],
        vec![
            add(c1(2, "a", "b", Some(2))),
            remove(c1(2, "a", "b", Some(2))),
            add(c0("000000000000feed0000000000000014", "x19", &[0], Some("x5"))),
        ],
        vec![add(c1(4, "x2", "x21", Some(1))), remove(c1(4, "x2", "x21", Some(1)))],
        vec![
            edit(c1(1099511627776, "b", "x y", None), c1(1099511627776, "a", "x y", None)),
            add(c1(4, "a", "x24", Some(2))),
            remove(c1(1099511627776, "a", "x y", None)),
            edit(c1(i64::MIN, "a", "b", None), c1(i64::MIN, "b", "b", Some(2))),
        ],
    ]
}

fn pairs_before() -> Vec<Change> {
    vec![
        add(p0("ab", -2, 4)),
        add(p0("ab", 8, -2)),
        add(p0("ab", 1099511627776, 2)),
        add(p0("ü", 4, 5)),
        add(p0("ü", 6, -1)),
        add(p1("", i64::MIN, Some(0), 0, 1, Some("b"))),
        add(p1("a", -2, Some(1), 0, 1, None)),
        add(p1("ü", -1, Some(1), 0, 1, Some("x y"))),
    ]
}

fn pairs_batches() -> Vec<Vec<Change>> {
    vec![vec![
        edit(p0("ab", 8, -2), p0("ab", 8, -3)),
        edit(p0("ab", 8, -3), p0("ab", 8, -2)),
        add(p0("zz", 8, -3)),
        add(p1("", 4, None, 1, 0, Some("zz"))),
        add(p0("ü", i64::MAX, 2)),
        remove(p0("ab", -2, 4)),
        remove(p0("ü", 4, 5)),
        remove(p0("ü", 6, -1)),
        remove(p0("ab", 8, -2)),
        remove(p0("ab", 1099511627776, 2)),
        remove(p0("zz", 8, -3)),
    ]]
}

// * The runner

/// A vector: a view of a domain, read as a user, its input, a store and the
/// batches that move it.
struct Case {
    domain: &'static str,
    view: &'static str,
    user: &'static str,
    input: Value,
    before: Vec<Change>,
    batches: Vec<Vec<Change>>,
}

fn store_of(domain: &str, rows: &[Change]) -> Store {
    let schema = ark_dev::library::named(domain).unwrap().schema;
    let mut by: Vec<(String, Vec<Vec<u8>>)> = schema.tables.iter().map(|t| (t.name.clone(), vec![])).collect();
    for c in rows {
        let Change::Add { table, row } = c else { panic!("a store is rows") };
        by.iter_mut().find(|(t, _)| t == table).unwrap().1.push(row.clone());
    }
    Store::from_rows(schema, ark::schema::Space::Durable, &by).unwrap().0
}

fn library(view: &'static str, input: Value) -> Case {
    Case {
        domain: "library",
        view,
        user: "alice",
        input,
        before: library_before(),
        batches: library_batches(),
    }
}

fn tracks(view: &'static str, input: Value) -> Case {
    Case {
        before: tracks_before(),
        batches: tracks_batches(),
        ..library(view, input)
    }
}

/// Every vector, by its file's name.
fn cases() -> Vec<(&'static str, Case)> {
    let none = || json!(null);
    vec![
        (
            "top-two-by-pos",
            library("library.top_two_by_pos", json!({ "playlist_id": hex(&pid(1).0) })),
        ),
        ("playlist-with-items", library("library.playlist_with_items", none())),
        ("projection", library("library.projection", none())),
        ("having-admits-on-a-child", library("library.having_admits_on_a_child", none())),
        ("group-source", library("library.group_source", none())),
        ("lookup-chain", library("library.lookup_chain", none())),
        ("related-on-a-non-key-column", library("library.related_on_a_non_key_column", none())),
        (
            "expression-order-under-a-limit",
            library("library.expression_order_under_a_limit", none()),
        ),
        ("related-three-deep", library("library.related_three_deep", none())),
        ("playlist-join-item", library("library.playlist_join_item", none())),
        ("kept-max", tracks("library.kept_max", none())),
        ("has-search", tracks("library.has_search", json!({ "needle": "ari" }))),
        (
            "a-maintained-sum-answers-where-the-fold-overflows",
            Case {
                domain: "colors",
                view: "colors.kept_sums",
                user: "peer-0",
                input: none(),
                before: colors_before(),
                batches: colors_batches(),
            },
        ),
        (
            "a-maintained-sum-overflows-where-the-fold-answers",
            Case {
                domain: "pairs",
                view: "pairs.kept_sums",
                user: "peer-1",
                input: json!({ "f_p0": "a" }),
                before: pairs_before(),
                batches: pairs_batches(),
            },
        ),
    ]
}

fn text(c: &Case) -> String {
    let store = store_of(c.domain, &c.before);
    view_vector(c.domain, c.view, &view_ctx(c.user, "dev"), &c.input, store, &c.batches).unwrap_or_else(|e| panic!("{}: {e}", c.view))
}

/// Every vector: written (or held to what is on disk), nothing else beside
/// them, each passing the runner, which reads the file alone; and the
/// falsified one, `top-two-by-pos` with the window not refilled when t1
/// leaves it (the step's insert of t3 dropped, its rows one short), failing
/// it. The kept sums hold v4's §6.5 rule both ways: a group's sum is taken
/// in an `i128` and judged once, after the whole batch, as a fresh sum
/// judges its total — `…-answers-where-the-fold-overflows` sums `i64::MIN`
/// and then `-2` on the way to a total that fits, and
/// `…-overflows-where-the-fold-answers` passes through a group whose sum is
/// past `i64::MAX` inside one batch, which a view takes net, as one.
/// Falsified three times: by the runner comparing no steps, the falsified
/// vector passed; by `playlist_with_items` sorting its items first first,
/// `playlist-with-items` moved; by `total` adding in an `i64`, the first
/// sum's view overflowed.
#[test]
fn vectors() {
    let mut written = BTreeSet::new();
    for (name, c) in cases() {
        vector("views", &format!("{name}.json"), &text(&c)).unwrap();
        written.insert(format!("{name}.json"));
    }
    let there: BTreeSet<String> = on_disk("views").into_iter().map(|(n, _)| n).collect();
    assert_eq!(there, written, "views/ holds what the writer did not write");
    for (n, text) in on_disk("views") {
        run_view(&text).unwrap_or_else(|e| panic!("{n}: {e}"));
    }
    let right = text(&cases().swap_remove(0).1);
    let mut v: Value = serde_json::from_str(&right).unwrap();
    let steps = v["steps"].as_array_mut().unwrap();
    let refill = steps
        .iter()
        .position(|s| {
            s["patches"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["t"] == "insert" && p["node"]["track_id"] == "t3")
        })
        .expect("the refill is in the vector");
    let s = &mut steps[refill];
    s["patches"]
        .as_array_mut()
        .unwrap()
        .retain(|p| !(p["t"] == "insert" && p["node"]["track_id"] == "t3"));
    s["rows"].as_array_mut().unwrap().retain(|r| r["track_id"] != "t3");
    v["expect"] = json!("fail");
    vector("views/falsify", "top-two-by-pos-no-refill.json", &format!("{v}\n")).unwrap();
    for (n, text) in on_disk("views/falsify") {
        assert!(run_view(&text).is_err(), "{n} passed");
    }
}

// * The work contract

/// What one push of `c` costs a view: the reads it made.
fn cost(view: &str, input: Value, mut store: Store, c: Change) -> ark::db::Reads {
    let d = ark_dev::library::library();
    let input = d.input_json(view, &input.to_string()).unwrap();
    let mut v = d.watch(&store, &view_ctx("alice", "dev"), view, &input).unwrap();
    store.apply(&c).unwrap();
    let (patches, reads) = ark::db::count(|| v.push(std::slice::from_ref(&c), &store).unwrap());
    assert!(!patches.is_empty(), "{view} moved");
    reads
}

/// The work contract (§1.10): a change costs reads by the entries it hits,
/// not by the rows viewed — the same at 1k, 4k and 16k rows. An item added
/// to one of `n / 4` playlists of `playlist_with_items` re-reads that
/// playlist's items through the key and nothing else; the first item of
/// `top_two_by_pos` leaving is refilled from the view's own order, and a
/// song changing creator under `group_source` moves two groups — no read at
/// all. (The contract itself is held by every vector.) Falsified by
/// `playlist_with_items` scanning every item and keeping its playlist's:
/// every playlist's run covered the whole table, and the item cost
/// 250,250 rows at 1k.
#[test]
fn a_change_costs_what_it_hits() {
    let mut seen = vec![];
    for n in [1000u32, 4000, 16000] {
        let mut rows = vec![];
        for p in 0..n / 4 {
            rows.push(add(pl(p, &format!("p{p}"), "alice")));
            rows.extend((1..=4).map(|k| add(it(p, &format!("t{k}"), k as i64))));
        }
        let items = cost(
            "library.playlist_with_items",
            json!(null),
            store_of("library", &rows),
            add(it(n / 8, "t5", 5)),
        );
        let input = json!({ "playlist_id": hex(&pid(1).0) });
        let top = cost("library.top_two_by_pos", input, scaled_items(n as usize), remove(it(1, "t0", 1)));
        let (c0, c1) = (song("s0", "x", "c0", None, 0), song("s0", "x", "c1", None, 0));
        let groups = cost("library.group_source", json!(null), scaled_songs(n as usize), edit(c0, c1));
        seen.push((items, top, groups));
    }
    let r = |gets, rows| ark::db::Reads { gets, rows };
    assert_eq!(seen[0], (r(0, 5), r(0, 0), r(0, 0)), "what one change reads");
    assert!(seen.iter().all(|s| *s == seen[0]), "at every size: {seen:?}");
}

fn scaled_items(n: usize) -> Store {
    let mut rows = vec![add(pl(1, "Viewed", "alice"))];
    rows.extend((0..n).map(|i| add(it(1, &format!("t{i}"), i as i64 + 1))));
    store_of("library", &rows)
}

fn scaled_songs(n: usize) -> Store {
    store_of(
        "library",
        &(0..n)
            .map(|i| add(song(&format!("s{i}"), "x", &format!("c{}", i % 8), None, i as i64)))
            .collect::<Vec<_>>(),
    )
}
