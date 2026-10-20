//! v4's `eval/` vectors re-derived over the v5 demo: a function, a store
//! and inputs applied in turn — each step's changes, the rows after them
//! and the state hash, or the refusal — under `spec/vectors/eval/`. The
//! input checks are refinements of the input's type, so `checks` and
//! `form-check` are [`parse`]'s verdicts and words; a playlist's existence
//! is the store's, so it is the body's refusal and never the form's.
//!
//! An entry's ids are its draws (§1.6): the context names the entry by
//! `(session, local)`, and a created playlist's id follows from it.

#[path = "demo.rs"]
mod demo;
mod support {
    pub mod json;
    pub mod vectors;
}

use std::sync::Arc;

use ark::check::Types;
use ark::codec::{bytes_enc, int_enc};
use ark::interp::{check_form, decode, Interp};
use ark::ir::{CtxField, Effect, Expr, Function, Node, Op, Std};
use ark::log::EntryId;
use ark::module::Lib;
use ark::peer::Ctx;
use ark::schema::{Schema, Table};
use ark::store::Store;
use ark::ty::Ty;
use demo::*;
use support::json::{array, change, hexed, int as jint, object, quoted, store as jstore, val};
use support::vectors::{falsify, vector};

fn entry(user: &str, roles: &[&str], local: i64) -> Ctx {
    Ctx {
        id: EntryId::of("session-1", local),
        actor: user.into(),
        session: "session-1".into(),
        roles: roles.iter().map(|r| r.to_string()).collect(),
        now: 1_700_000_000_000,
    }
}

fn ctx_json(c: &Ctx) -> String {
    object(&[
        ("id", hexed(&c.id.0)),
        ("now", jint(c.now)),
        ("roles", array(c.roles.iter().map(|r| quoted(r)))),
        ("session", quoted(&c.session)),
        ("user", quoted(&c.actor)),
    ])
}

/// The input as the function's input type prints it.
fn input_json(f: &Function, input: &[u8]) -> String {
    let (cx, t) = (Types::new(&f.types), Interp::input_ty(f));
    match decode(&cx, &t, input) {
        Ok(v) => val(&cx, &t, &v),
        Err(_) => format!("{{\"$bytes\":\"{}\"}}", ark::hash::hex(input)),
    }
}

/// Runs `f` once over `store`, applying what it changes: the step's parts.
fn step(lib: &Lib, sch: &Schema, store: &mut Store, f: &Function, c: &Ctx, input: Vec<u8>) -> Vec<(&'static str, String)> {
    let h = lib.install(sch, f.clone()).unwrap();
    let ran = lib.procedure(&h).unwrap().run(store, c, &input);
    let verdict = ran.and_then(|cs| store.apply_all(&cs).map(|_| cs));
    let mut parts = vec![];
    match verdict {
        Ok(cs) => parts.push(("changes", array(cs.iter().map(|c| change(sch, c))))),
        Err(r) => parts.push(("refused", quoted(&r.0))),
    }
    parts.extend([
        ("hash_after", quoted(&store.state_hash().hex())),
        ("input", input_json(f, &input)),
        ("input_bytes", hexed(&input)),
    ]);
    parts.sort_by_key(|p| p.0);
    parts.push(("store_after", jstore(sch, store)));
    parts
}

fn pid(n: u8) -> Vec<u8> {
    let mut i = vec![0; 16];
    i[15] = n;
    i
}

/// The demo's store with Favorites and its items at these places.
fn favorites(items: &[(&str, i64)]) -> Store {
    let rows = vec![
        ("playlist".into(), vec![playlist_row(&pid(1), "Favorites", "alice")]),
        ("item".into(), items.iter().map(|(t, n)| item_row(&pid(1), t, *n)).collect()),
    ];
    Store::from_rows(shared(), &rows).unwrap()
}

fn steps_vector(f: &Function, name: &str, store: Store, inputs: Vec<Vec<u8>>) -> Vec<(&'static str, String)> {
    let (lib, sch, mut store) = (Lib::default(), schema(), store);
    let before = jstore(&sch, &store);
    let steps = inputs
        .into_iter()
        .enumerate()
        .map(|(i, input)| object(&step(&lib, &sch, &mut store, f, &entry("alice", &[], i as i64 + 1), input)));
    let steps = format!("[{}]", steps.collect::<Vec<_>>().join("\n,"));
    vec![
        ("function", quoted(name)),
        ("function_hash", quoted(&f.hash().hex())),
        ("function_bytes", hexed(&f.to_bytes())),
        ("store_before", before),
        ("ctx", ctx_json(&entry("alice", &[], 1))),
        ("steps", steps),
    ]
}

fn add_steps(f: &Function) -> Vec<(&'static str, String)> {
    let inputs = ["t7", "t9", "t7", "t11"].map(|t| pair_input(&pid(1), t)).to_vec();
    steps_vector(f, ADD, favorites(&[]), inputs)
}

/// `eval/add-to-playlist`: t7, t9, t7 again (on it already: nothing), t11
/// — each one past the last `pos`. Falsified by `Sort` ignoring `Desc`:
/// the vector moved.
#[test]
fn add_to_playlist_steps() {
    let right = add_steps(&add_to_playlist());
    vector("eval", "add-to-playlist.json", &right);
    let mut wrong = add_steps(&add_to_playlist());
    let asc = add_steps(&ascending_add());
    wrong[5] = asc[5].clone();
    falsify("eval", "add-reads-pos-ascending.json", &wrong, &right);
}

/// `add_to_playlist` reading the first `pos` rather than the last: wrong.
fn ascending_add() -> Function {
    let mut f = add_to_playlist();
    fn flip(e: &mut Expr) {
        match e {
            Expr::Sort(_, _, dirs) => dirs[0] = ark::store::Dir::Asc,
            Expr::If(_, a, b) | Expr::Let(a, b) => {
                flip(a);
                flip(b);
            }
            Expr::Std(_, es) | Expr::Tuple(es) | Expr::Seq(es) => es.iter_mut().for_each(flip),
            Expr::Take(l, _) => flip(l),
            _ => {}
        }
    }
    flip(&mut f.body);
    f
}

/// A named case: a function, its input, and the verdict.
fn case(name: &str, f: &Function, store: &Store, c: &Ctx, input: Vec<u8>) -> String {
    let (lib, sch, mut s) = (Lib::default(), schema(), store.clone());
    let mut parts = vec![("function", quoted(&f.hash().hex())), ("name", quoted(name))];
    parts.extend(step(&lib, &sch, &mut s, f, c, input).into_iter().filter(|p| p.0 != "store_after"));
    parts.sort_by_key(|p| p.0);
    object(&parts)
}

/// `eval/checks`: the refinements' words at the boundary, and the body's.
/// Falsified by `Trim` not normalising: "   " passed `non-empty` and the
/// vector moved.
#[test]
fn checks() {
    let s = favorites(&[("t11", 3), ("t7", 1), ("t9", 2)]);
    let c = entry("alice", &[], 1);
    let (create, add) = (create_playlist(), add_to_playlist());
    let cases = [
        case("trim-then-non-empty", &create, &s, &c, create_input("   ")),
        case("trimmed-name-lands", &create, &s, &c, create_input("  Road  ")),
        case("same-name-again-is-nothing", &create, &s, &c, create_input("Favorites")),
        case("no-such-playlist-is-the-body's", &add, &s, &c, pair_input(&pid(2), "t1")),
        case("len-default-message", &add, &s, &c, pair_input(&pid(1), "")),
        case("an-id-of-15-bytes", &add, &s, &c, pair_input(&[1; 15], "t1")),
        case("not-utf8", &add, &s, &c, [bytes_enc(&pid(1)), bytes_enc(&[0xFF])].concat()),
    ];
    vector(
        "eval",
        "checks.json",
        &[
            (
                "functions",
                object(&[
                    ("add_to_playlist", quoted(&add.hash().hex())),
                    ("create_playlist", quoted(&create.hash().hex())),
                ]),
            ),
            ("store_before", jstore(&schema(), &s)),
            ("ctx", ctx_json(&c)),
            ("cases", format!("[{}]", cases.join("\n,"))),
        ],
    );
}

/// `eval/form-check`: the form validator is `parse`, every failing field
/// by its path, the normalised value when none fails. Falsified by the
/// walk stopping at the first failing field: `two-fields` showed one.
#[test]
fn form_check() {
    let form = |name: &str, f: &Function, input: Vec<u8>| {
        let (cx, t) = (Types::new(&f.types), Interp::input_ty(f));
        let (messages, normalised) = match check_form(&cx, &t, &input) {
            Ok(v) => (vec![], val(&cx, &t, &v)),
            Err(fails) => (fails, "null".into()),
        };
        let messages = messages
            .iter()
            .map(|(at, m)| object(&[("at", array(at.iter().map(|i| jint(*i as i64)))), ("message", quoted(m))]));
        object(&[
            ("function", quoted(&f.hash().hex())),
            ("input_bytes", hexed(&input)),
            ("messages", array(messages)),
            ("name", quoted(name)),
            ("normalised", normalised),
        ])
    };
    let (create, add) = (create_playlist(), add_to_playlist());
    let mut truncated = create_input("Mix");
    truncated.pop();
    let cases = [
        form("empty-name", &create, create_input(" ")),
        form("trimmed", &create, create_input(" Mix ")),
        form("two-fields", &add, pair_input(&[7; 3], "")),
        form("unknown-playlist-passes", &add, pair_input(&pid(2), "t")),
        form("not-utf8", &create, bytes_enc(&[0xFF])),
        form("not-canonical", &create, truncated),
    ];
    vector("eval", "form-check.json", &[("cases", format!("[{}]", cases.join("\n,")))]);
}

/// `feature(name)`: a curator's playlist, owned by "editors" when the
/// author is an editor too, else by "curators".
fn feature() -> Function {
    let types = vec![id_decl(), name_decl()];
    func(types, &[("name", named("Name"))], Ty::unit(), Effect::Write, |b| {
        let has = |r: &str| std(Std::Contains, vec![ctx(CtxField::Roles), text(r)]);
        let owner = if_(has("editor"), text("editors"), text("curators"));
        let row = vec![ctx(CtxField::NewId("playlist".into())), b.v("name"), owner];
        if_(has("curator"), insert("playlist", row), refuse("only a curator features a playlist"))
    })
}

/// `eval/has-role`: a role is read off the entry's roles. Falsified by
/// `Contains` true of any list: `not-held` landed.
#[test]
fn has_role() {
    let (f, s) = (feature(), Store::new(shared()));
    let cases = [
        ("held", &["curator"][..]),
        ("held-and-an-editor", &["curator", "editor"]),
        ("not-held", &[]),
        ("another-role-only", &["editor"]),
    ]
    .map(|(n, roles)| {
        let c = entry("alice", roles, 1);
        let mut o = case(n, &f, &s, &c, create_input("Picks"));
        o.insert_str(o.len() - 1, &format!(",\"roles\":{}", array(roles.iter().map(|r| quoted(r)))));
        o
    });
    vector(
        "eval",
        "has-role.json",
        &[
            ("function_bytes", hexed(&f.to_bytes())),
            ("store_before", jstore(&schema(), &s)),
            ("cases", format!("[{}]", cases.join("\n,"))),
        ],
    );
}

fn row_ty(t: &Table) -> Ty {
    Ty::Tuple(
        t.columns
            .iter()
            .map(|c| if c.decl == "Int" { c.ty.clone() } else { named(&c.decl) })
            .collect(),
    )
}

/// The authority's raw write of a row: put it — an edit when its key has a
/// row, an add when not.
fn put_row(table: &str) -> Function {
    let t = schema().table(table).unwrap().1.clone();
    func(vec![id_decl()], &[("row", row_ty(&t))], Ty::unit(), Effect::Write, |b| {
        let key = Expr::Tuple((0..t.key as u32).map(|i| field(b.v("row"), i)).collect());
        let same = b.lam(&["old"], |b| b.v("row"));
        let edit = Expr::Update(table.into(), Box::new(key.clone()), Box::new(same));
        let add = Expr::Insert(table.into(), Box::new(b.v("row")));
        if_(std(Std::IsSome, vec![Expr::Lookup(table.into(), Box::new(key))]), edit, add)
    })
}

fn delete_row(table: &str) -> Function {
    let t = schema().table(table).unwrap().1.clone();
    let key = Ty::Tuple(row_ty(&t).as_tuple()[..t.key].to_vec());
    func(vec![id_decl()], &[("key", key)], Ty::unit(), Effect::Write, |b| {
        Expr::Delete(table.into(), Box::new(b.v("key")))
    })
}

trait AsTuple {
    fn as_tuple(&self) -> &[Ty];
}
impl AsTuple for Ty {
    fn as_tuple(&self) -> &[Ty] {
        match self {
            Ty::Tuple(ts) => ts,
            _ => panic!(),
        }
    }
}

/// `eval/raw-put-row` and `raw-delete-row`: the authority's raw writes are
/// functions like any other, judged by the store as every write is: an
/// add, an edit, nothing for the same row, a reference refused; a playlist
/// an item still names refused (v4's `StillReferenced`), the item removed,
/// then nothing, then the playlist. Falsified by `Update` reporting an
/// edit to the same row: the fourth put showed one; and by `Store::apply`
/// not judging a remove's referrers: the first delete took the playlist.
#[test]
fn raw_writes() {
    let (lib, sch) = (Lib::default(), schema());
    let mut s = Store::new(shared());
    let puts = [
        (put_row("playlist"), playlist_row(&pid(1), "Favorites", "alice")),
        (put_row("item"), item_row(&pid(1), "t7", 1)),
        (put_row("playlist"), playlist_row(&pid(1), "Kept", "alice")),
        (put_row("playlist"), playlist_row(&pid(1), "Kept", "alice")),
        (put_row("item"), item_row(&pid(2), "t9", 1)),
    ];
    let mut steps = vec![];
    for (i, (f, row)) in puts.iter().enumerate() {
        let mut p = step(&lib, &sch, &mut s, f, &entry("authority", &[], i as i64 + 1), row.clone());
        p.push(("function", quoted(&f.hash().hex())));
        p.sort_by_key(|x| x.0);
        steps.push(object(&p));
    }
    vector("eval", "raw-put-row.json", &[("steps", format!("[{}]", steps.join("\n,")))]);
    let before = jstore(&sch, &s);
    let mut key = bytes_enc(&pid(1));
    key.extend(bytes_enc(b"t7"));
    let deletes = [
        (delete_row("playlist"), bytes_enc(&pid(1))),
        (delete_row("item"), key.clone()),
        (delete_row("item"), key),
        (delete_row("playlist"), bytes_enc(&pid(1))),
    ];
    let mut steps = vec![];
    for (i, (f, k)) in deletes.iter().enumerate() {
        let mut p = step(&lib, &sch, &mut s, f, &entry("authority", &[], i as i64 + 1), k.clone());
        p.push(("function", quoted(&f.hash().hex())));
        p.sort_by_key(|x| x.0);
        steps.push(object(&p));
    }
    vector(
        "eval",
        "raw-delete-row.json",
        &[("store_before", before), ("steps", format!("[{}]", steps.join("\n,")))],
    );
}

// * The list editors, over a module of their own.

fn joined() -> Schema {
    Schema {
        tables: vec![Table::new("joined").col("id", "Text", Ty::Bytes).col("text", "Text", Ty::Bytes)],
    }
}

/// `name(xs, args…)`: the row `(name, concat(edit(xs, args…)))`.
fn list_op(name: &str, f: Std, args: &[&str]) -> Function {
    let mut params = vec![("xs", Ty::list(text_ty()))];
    params.extend(args.iter().map(|a| (*a, if *a == "x" { text_ty() } else { Ty::Int })));
    let name = name.to_string();
    func(vec![], &params, Ty::unit(), Effect::Write, |b| {
        let mut es = vec![b.v("xs")];
        es.extend(args.iter().map(|a| match (*a, name.as_str()) {
            ("at", "insert_after") => op(Op::Add, b.v(a), int(1)),
            (a, _) => b.v(a),
        }));
        let edited = match f {
            Std::ListConcat => std(f, vec![b.v("xs"), b.v("ys")]),
            _ => std(f, es),
        };
        insert("joined", vec![text(&name), std(Std::Concat, vec![edited])])
    })
}

fn list_input(xs: &[&str], ints: &[i64], x: Option<&str>) -> Vec<u8> {
    let mut b = vec![];
    ark::codec::put_list(&mut b, xs, |o, s| ark::codec::put_bytes(o, s.as_bytes()));
    for (i, n) in ints.iter().enumerate() {
        if i == 1 && x.is_some() {
            break;
        }
        b.extend(int_enc(*n));
    }
    if let Some(x) = x {
        b.extend(bytes_enc(x.as_bytes()));
    }
    b
}

fn list_cases(clamp: bool) -> Vec<(&'static str, String)> {
    let sch = Arc::new(joined());
    let abc = ["a", "b", "c"];
    let abcd = ["a", "b", "c", "d"];
    let concat = |xs: &[&str], ys: &[&str]| {
        let mut b = list_input(xs, &[], None);
        b.extend(list_input(ys, &[], None));
        b
    };
    let ins = |xs: &[&str], at: i64| list_input(xs, &[at], Some("x"));
    let two = |xs: &[&str], i: i64, j: i64| list_input(xs, &[i, j], None);
    let (cat, insert_, after, remove, mv, slice) = (
        list_op("concat", Std::ListConcat, &["ys"]),
        list_op("insert", Std::Insert, &["at", "x"]),
        list_op("insert_after", Std::Insert, &["at", "x"]),
        list_op("remove_at", Std::RemoveAt, &["at"]),
        list_op("move", Std::Move, &["from", "to"]),
        list_op("slice", Std::Slice, &["start", "end"]),
    );
    let cat = Function {
        params: vec![Ty::list(text_ty()), Ty::list(text_ty())],
        ..cat
    };
    let cases: Vec<(&str, &Function, Vec<u8>)> = vec![
        ("concat-two", &cat, concat(&["a", "b"], &["c"])),
        ("concat-onto-empty", &cat, concat(&[], &abc)),
        ("concat-empty", &cat, concat(&[], &[])),
        ("insert-at-the-front", &insert_, ins(&abc, 0)),
        ("insert-in-the-middle", &insert_, ins(&abc, 2)),
        ("insert-at-the-end", &insert_, ins(&abc, 3)),
        ("insert-into-empty", &insert_, ins(&[], 0)),
        ("insert-past-the-end", &insert_, ins(&abc, if clamp { 3 } else { 4 })),
        ("insert-below-zero", &insert_, ins(&abc, -1)),
        ("insert-far-past-the-end", &insert_, ins(&abc, i64::MAX)),
        ("insert-after", &after, ins(&abc, 0)),
        ("insert-after-overflows-first", &after, ins(&abc, i64::MAX)),
        ("remove-the-middle", &remove, list_input(&abc, &[1], None)),
        ("remove-the-last", &remove, list_input(&abc, &[2], None)),
        ("remove-the-only", &remove, list_input(&["a"], &[0], None)),
        ("remove-past-the-end", &remove, list_input(&abc, &[3], None)),
        ("remove-from-empty", &remove, list_input(&[], &[0], None)),
        ("remove-below-zero", &remove, list_input(&abc, &[-1], None)),
        ("move-forward", &mv, two(&abcd, 0, 2)),
        ("move-to-the-end", &mv, two(&abcd, 0, 3)),
        ("move-backward", &mv, two(&abcd, 3, 1)),
        ("move-in-place", &mv, two(&abcd, 1, 1)),
        ("move-from-out-of-range", &mv, two(&abcd, 4, 0)),
        ("move-to-out-of-range", &mv, two(&abcd, 0, -1)),
        ("move-both-out-of-range", &mv, two(&abcd, 9, -1)),
        ("slice-the-middle", &slice, two(&abcd, 1, 3)),
        ("slice-all", &slice, two(&abcd, 0, 4)),
        ("slice-nothing", &slice, two(&abcd, 2, 2)),
        ("slice-nothing-at-the-end", &slice, two(&abcd, 4, 4)),
        ("slice-backwards", &slice, two(&abcd, 3, 1)),
        ("slice-past-the-end", &slice, two(&abcd, 0, 5)),
        ("slice-below-zero", &slice, two(&abcd, -1, 2)),
    ];
    let lib = Lib::default();
    let out = cases.into_iter().map(|(name, f, input)| {
        let mut s = Store::new(sch.clone());
        let mut p = step(&lib, &sch, &mut s, f, &entry("alice", &[], 1), input);
        if clamp && name == "insert-past-the-end" {
            p.iter_mut().filter(|x| x.0 == "input_bytes").for_each(|x| x.1 = hexed(&ins(&abc, 4)));
        }
        p.retain(|x| x.0 != "store_after" && x.0 != "hash_after" && x.0 != "input");
        p.extend([("function", quoted(&f.hash().hex())), ("name", quoted(name))]);
        p.sort_by_key(|x| x.0);
        object(&p)
    });
    vec![
        ("schema", support::json::schema(&sch)),
        ("cases", format!("[{}]", out.collect::<Vec<_>>().join("\n,"))),
    ]
}

/// `eval/list-ops`: the five list editors, every edge an index has and its
/// refusal's words; `falsify/list-ops-clamping` expects an index past the
/// end clamped. Falsified by `Insert` clamping an index past the end:
/// the insert past the end landed and the vector moved.
#[test]
fn list_ops() {
    let right = list_cases(false);
    vector("eval", "list-ops.json", &right);
    falsify("eval", "list-ops-clamping.json", &list_cases(true), &right);
}

/// Through its own list so far: a body that inserts a row, then reads it
/// back by key and by scan, then updates it and deletes it, reports each
/// change against the row as it then stood. Falsified by `Lookup` reading
/// the store alone: the read-back was none and the body refused.
#[test]
fn a_body_reads_what_it_wrote() {
    let f = func(vec![id_decl()], &[("name", text_ty())], Ty::unit(), Effect::Write, |b| {
        b.let_("id", ctx(CtxField::NewId("playlist".into())), |b| {
            let rename = b.lam(&["p"], |b| Expr::Tuple(vec![field(b.v("p"), 0), text("renamed"), field(b.v("p"), 2)]));
            let id = b.v("id");
            let key = || vec![id.clone()];
            Expr::Seq(vec![
                insert("playlist", vec![b.v("id"), b.v("name"), ctx(CtxField::User)]),
                if_(std(Std::IsSome, vec![lookup("playlist", key())]), nothing(), refuse("not read back")),
                if_(
                    op(Op::Eq, std(Std::Len, vec![scan("playlist")]), int(1)),
                    nothing(),
                    refuse("not scanned"),
                ),
                Expr::Update("playlist".into(), Box::new(Expr::Tuple(key())), Box::new(rename)),
                Expr::Delete("playlist".into(), Box::new(Expr::Tuple(key()))),
            ])
        })
    });
    let (lib, sch) = (Lib::default(), schema());
    let mut s = Store::new(shared());
    let p = step(&lib, &sch, &mut s, &f, &entry("alice", &[], 1), create_input("Mix"));
    let changes = &p.iter().find(|x| x.0 == "changes").expect("no refusal").1;
    assert_eq!(changes.matches("\"t\":").count(), 3, "{changes}");
    assert_eq!(s.rows("playlist").count(), 0);
}

/// Checked arithmetic and the standard library's faults are refusals.
/// Falsified by `Op::Mul` wrapping: `i64::MAX * 2` landed as -2.
#[test]
fn overflow_is_a_refusal() {
    let sch = Arc::new(joined());
    let row = |e: Expr| insert("joined", vec![text("n"), std(Std::TextOfInt, vec![e])]);
    let cases = [
        (op(Op::Mul, int(i64::MAX), int(2)), "integer overflow"),
        (op(Op::Div, int(1), int(0)), "division by zero"),
        (op(Op::Div, int(i64::MIN), int(-1)), "integer overflow"),
        (std(Std::Abs, vec![int(i64::MIN)]), "integer overflow"),
        (std(Std::Clamp, vec![int(1), int(2), int(1)]), "clamp: lower bound above upper bound"),
        (Expr::Un(ark::ir::Un::Neg, Box::new(int(i64::MIN))), "integer overflow"),
    ];
    for (e, why) in cases {
        let f = func(vec![], &[], Ty::unit(), Effect::Write, |_| row(e.clone()));
        let lib = Lib::default();
        let h = lib.install(&sch, f).unwrap();
        let r = lib.procedure(&h).unwrap().run(&Store::new(sch.clone()), &entry("a", &[], 1), &[]);
        assert_eq!(r.unwrap_err().0, why, "{e:?}");
    }
}

/// The standard library through the pinned tables, and the text and list
/// functions' meanings. Falsified by `Trim` trimming only the start.
#[test]
fn the_standard_library() {
    use ark::interp::V;
    use ark::ir::Std::*;
    let t = |s: &str| V::Bytes(s.as_bytes().to_vec());
    let l = |xs: &[&str]| V::List(xs.iter().map(|x| t(x)).collect());
    let ap = |f, a: Vec<V>| ark::std::apply(f, a).map_err(|r| r.0);
    assert_eq!(ap(Trim, vec![t("\u{3000} a \u{85}")]), Ok(t("a")));
    assert_eq!(ap(Lower, vec![t("İ")]), Ok(t("i")));
    assert_eq!(ap(IsAlnum, vec![t("Ⅻ٣")]), Ok(V::bool(true)));
    assert_eq!(ap(IsAlnum, vec![t("")]), Ok(V::bool(false)));
    assert_eq!(ap(TextLen, vec![t("añb")]), Ok(V::Int(3)));
    assert_eq!(ap(Chars, vec![t("añ")]), Ok(l(&["a", "ñ"])));
    assert_eq!(ap(SplitOnce, vec![t("a=b=c"), t("=")]), Ok(V::some(V::Tup(vec![t("a"), t("b=c")]))));
    assert_eq!(ap(SplitOnce, vec![t("abc"), t("")]), Ok(V::none()));
    assert_eq!(ap(Fnv1a64, vec![t("a")]), Ok(V::Int(0xaf63dc4c8601ec8cu64 as i64)));
    let id = ap(IdOfText, vec![t("00000000-0000-0000-0000-00000000000A")]).unwrap();
    assert_eq!(
        ap(TextOfId, vec![id.variant().1[0].clone()]),
        Ok(t("00000000-0000-0000-0000-00000000000a"))
    );
    assert_eq!(ap(IdOfText, vec![t("nope")]), Ok(V::none()));
    assert_eq!(ap(Unwrap, vec![V::none()]), Err("unwrapped none".into()));
    assert_eq!(ap(UnwrapOr, vec![V::none(), V::Int(3)]), Ok(V::Int(3)));
    assert_eq!(ap(Reverse, vec![l(&["a", "b"])]), Ok(l(&["b", "a"])));
    assert_eq!(ap(Contains, vec![l(&["a", "b"]), t("b")]), Ok(V::bool(true)));
}
