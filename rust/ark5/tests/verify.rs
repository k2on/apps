//! v4's `verify/` vectors re-derived: a module's functions, and whether
//! each checks over its schema (§1.4) — the demo, the list editors, a
//! scope, refinements kept by signature and by call — and, beside them,
//! one vector per thing the checker refuses, with its words. Under
//! `spec5/vectors/verify/`; `falsify/` claims a scope that reads the store
//! verifies.

#[path = "demo.rs"]
mod demo;
mod support {
    pub mod json;
    pub mod vectors;
}

use ark5::check::{check, Env};
use ark5::codec::bytes_enc;
use ark5::ir::{CtxField, Decl, Effect, Expr, Function, Node, Op, Rule, Std};
use ark5::module::{verify, Lib, Module, SPEC};
use ark5::schema::{Schema, Table};
use ark5::ty::Ty;
use demo::*;
use support::json::{array, hexed, object, quoted, schema as jschema};
use support::vectors::{falsify, vector};

/// Checks each function in turn, each callable by the ones after it, then
/// the module they make: whether it verifies, and if not why.
fn verdict(sch: &Schema, fs: &[(&str, Function)]) -> Result<(), String> {
    let lib = Lib::default();
    let mut bindings = vec![];
    for (name, f) in fs {
        let callee = |h: &_| lib.get(h);
        check(f, &Env { schema: sch, callee: &callee }).map_err(|r| format!("{name}: {r}"))?;
        bindings.push((name.to_string(), lib.install(sch, f.clone()).unwrap()));
    }
    bindings.sort();
    let mut types: Vec<(String, Decl)> = fs.iter().flat_map(|(_, f)| f.types.clone()).collect();
    types.sort_by(|a, b| a.0.cmp(&b.0));
    types.dedup();
    let m = Module {
        spec: SPEC,
        schema: sch.clone(),
        types,
        bindings,
    };
    verify(&m, &lib).map_err(|r| r.0)
}

fn parts(sch: &Schema, fs: &[(&str, Function)], claim: Option<bool>) -> Vec<(&'static str, String)> {
    let v = verdict(sch, fs);
    let functions = fs.iter().map(|(n, f)| object(&[("bytes", hexed(&f.to_bytes())), ("hash", quoted(&f.hash().hex())), ("name", quoted(n))]));
    vec![
        ("schema", jschema(sch)),
        ("functions", array(functions)),
        ("verifies", claim.unwrap_or(v.is_ok()).to_string()),
        ("why", v.err().filter(|_| claim.is_none()).map_or("null".into(), |w| quoted(&w))),
    ]
}

fn demo_fns() -> Vec<(&'static str, Function)> {
    vec![
        (CREATE, create_playlist()),
        (ADD, add_to_playlist()),
        (RENAME, rename_playlist()),
        (REMOVE, remove_from_playlist()),
    ]
}

fn ok(name: &str, sch: &Schema, fs: &[(&str, Function)]) {
    assert_eq!(verdict(sch, fs), Ok(()), "{name}");
    vector("verify", name, &parts(sch, fs, None));
}

fn refused(name: &str, sch: &Schema, fs: &[(&str, Function)], words: &str) {
    let v = verdict(sch, fs).expect_err(name);
    assert!(v.contains(words), "{name}: {v}");
    vector("verify/refused", name, &parts(sch, fs, None));
}

fn texts() -> Ty {
    Ty::list(text_ty())
}

fn non_empty() -> (String, Decl) {
    ("Texts".into(), decl(texts(), &[(Rule::NonEmpty, None)]))
}

/// A scope: a playlist is held by its owner. `Pure`, of the row and ctx.
fn scope(body: impl FnOnce(&mut B) -> Expr) -> Function {
    let row = Ty::Tuple(vec![named("Id<playlist>"), text_ty(), text_ty()]);
    func(vec![id_decl()], &[("p", row)], Ty::bool(), Effect::Pure, body)
}

fn owner() -> Function {
    scope(|b| op(Op::Eq, field(b.v("p"), 2), ctx(CtxField::User)))
}

/// A `Name` helper, and a caller handing it one.
fn helper() -> Function {
    func(vec![name_decl()], &[("n", named("Name"))], Ty::bool(), Effect::Pure, |b| {
        std(Std::StartsWith, vec![b.v("n"), text("A")])
    })
}

fn caller(arg: Ty) -> Function {
    func(vec![name_decl()], &[("n", arg)], Ty::bool(), Effect::Pure, |b| Expr::Call(helper().hash(), vec![b.v("n")]))
}

fn joined() -> Schema {
    Schema {
        tables: vec![Table::new("joined").col("id", "Text", Ty::Bytes).col("text", "Text", Ty::Bytes)],
    }
}

/// Every module that verifies. Falsified by `Types::sub` refusing a
/// refined type where what it refines is asked (`Name ≤ Text` false):
/// the demo did not verify.
#[test]
fn modules_that_verify() {
    ok("demo-ok.json", &schema(), &demo_fns());
    let mut scoped = demo_fns();
    scoped.push(("scope.playlist", owner()));
    ok("scoped-ok.json", &schema(), &scoped);
    let editors = [Std::ListConcat, Std::Insert, Std::RemoveAt, Std::Move, Std::Slice].map(|f| {
        let args: &[(&str, Ty)] = match f {
            Std::ListConcat => &[("xs", texts()), ("ys", texts())],
            Std::Insert => &[("xs", texts()), ("i", Ty::Int), ("x", text_ty())],
            Std::RemoveAt => &[("xs", texts()), ("i", Ty::Int)],
            _ => &[("xs", texts()), ("i", Ty::Int), ("j", Ty::Int)],
        };
        let names: Vec<&str> = args.iter().map(|a| a.0).collect();
        func(vec![], args, Ty::unit(), Effect::Write, |b| {
            let edited = std(f, names.iter().map(|n| b.v(n)).collect());
            insert("joined", vec![text(&format!("{f:?}")), std(Std::Concat, vec![edited])])
        })
    });
    let named_editors: Vec<(&str, Function)> = ["concat", "insert", "remove_at", "move", "slice"].into_iter().zip(editors).collect();
    ok("list-ops.json", &joined(), &named_editors);
    // NonEmpty kept by Reverse, Move, Insert and ListConcat's first; a
    // Name handed to a function asking for one, by hash.
    let kept = [Std::Reverse, Std::Insert, Std::ListConcat, Std::Move].map(|f| {
        let args: &[(&str, Ty)] = match f {
            Std::Reverse => &[("xs", named("Texts"))],
            Std::Insert => &[("xs", named("Texts")), ("i", Ty::Int), ("x", text_ty())],
            Std::ListConcat => &[("xs", named("Texts")), ("ys", texts())],
            _ => &[("xs", named("Texts")), ("i", Ty::Int), ("j", Ty::Int)],
        };
        let names: Vec<&str> = args.iter().map(|a| a.0).collect();
        func(vec![non_empty()], args, named("Texts"), Effect::Pure, |b| std(f, names.iter().map(|n| b.v(n)).collect()))
    });
    let mut fs: Vec<(&str, Function)> = ["reverse", "insert", "concat", "move"].into_iter().zip(kept).collect();
    fs.extend([("helper", helper()), ("caller", caller(named("Name")))]);
    ok("refinements-kept.json", &schema(), &fs);
}

/// One function the checker refuses, with the words it refuses it in.
fn one(types: Vec<(String, Decl)>, params: &[(&str, Ty)], ret: Ty, effect: Effect, body: impl FnOnce(&mut B) -> Expr) -> Vec<(&'static str, Function)> {
    vec![("f", func(types, params, ret, effect, body))]
}

/// Every refusal of the checker, one vector each, and the falsify: a scope
/// that reads the store claimed to verify. Falsified by dropping the
/// effect bound (`e <= f.effect` always true): a-scope-reading-the-store
/// verified; and, for `a-cycle-under-another-type`, by the cycle search
/// keeping no visited set: it never ended.
#[test]
fn what_the_checker_refuses() {
    let (sch, unit, w) = (schema(), Ty::unit(), Effect::Write);
    let reads = vec![("scope.playlist", scope(|b| std(Std::IsSome, vec![lookup("playlist", vec![field(b.v("p"), 0)])])))];
    refused("a-scope-reading-the-store.json", &sch, &reads, "the body's effect is Read, above Pure");
    let right = parts(&sch, &reads, None);
    falsify("verify", "a-scope-reading-the-store.json", &parts(&sch, &reads, Some(true)), &right);
    let wrong_element = one(vec![], &[("xs", texts())], unit.clone(), w, |b| {
        insert("joined", vec![text("x"), std(Std::Concat, vec![std(Std::Insert, vec![b.v("xs"), int(0), int(1)])])])
    });
    refused("list-ops-wrong-element.json", &joined(), &wrong_element, "Insert: argument 2 is Int");
    let text_into_id = one(vec![id_decl(), name_decl()], &[("name", named("Name"))], unit.clone(), w, |b| {
        insert("playlist", vec![ctx(CtxField::User), b.v("name"), ctx(CtxField::User)])
    });
    refused("text-into-an-id.json", &sch, &text_into_id, "a row is");
    let lit = |s: &str| Expr::Lit(named("Name"), bytes_enc(s.as_bytes()));
    let empty = one(vec![name_decl()], &[], named("Name"), Effect::Pure, |_| lit(""));
    refused("a-literal-breaking-its-refinement.json", &sch, &empty, "a playlist needs a name");
    let untrimmed = one(vec![name_decl()], &[], named("Name"), Effect::Pure, |_| lit(" x "));
    refused("a-literal-not-normal.json", &sch, &untrimmed, "not in normal form");
    let filtered = one(vec![non_empty()], &[("xs", named("Texts"))], named("Texts"), Effect::Pure, |b| {
        let all = b.lam(&["x"], |_| Expr::Variant(Ty::bool(), 1, vec![]));
        filter(b.v("xs"), all)
    });
    refused("filter-drops-non-empty.json", &sch, &filtered, "the body is List");
    let not_a_name = vec![("helper", helper()), ("caller", caller(text_ty()))];
    refused("a-call-holds-its-arguments.json", &sch, &not_a_name, "an argument is Named(\"Text\")");
    let bare = one(vec![], &[], unit.clone(), Effect::Pure, |b| b.lam(&["x"], |_| nothing()));
    refused("a-lambda-outside-an-operator.json", &sch, &bare, "a lambda outside an operator");
    let unbound = one(vec![], &[], Ty::Int, Effect::Pure, |_| Expr::Var(0));
    refused("an-unbound-variable.json", &sch, &unbound, "variable 0 is not bound");
    let unknown = one(vec![], &[], Ty::bool(), Effect::Pure, |_| Expr::Call(ark5::hash::FnHash([9; 32]), vec![]));
    refused("an-unknown-callee.json", &sch, &unknown, "no function 0909");
    let looped = ("Loop".to_string(), decl(Ty::Tuple(vec![Ty::Int, named("Loop")]), &[]));
    let tree = ("Tree".to_string(), decl(Ty::Sum(vec![vec![], vec![Ty::list(named("Tree"))]]), &[]));
    let loops = one(vec![looped, tree], &[], unit.clone(), Effect::Pure, |_| nothing());
    refused("a-type-containing-itself.json", &sch, &loops, "Loop contains itself outside a list or a sum");
    let under = vec![
        ("A".to_string(), decl(named("B"), &[])),
        ("B".to_string(), decl(Ty::Tuple(vec![named("B")]), &[])),
    ];
    let loops = one(under, &[], Ty::unit(), Effect::Pure, |_| nothing());
    refused("a-cycle-under-another-type.json", &sch, &loops, "B contains itself");
    let query_writes = one(vec![], &[("x", text_ty())], unit.clone(), Effect::Read, |b| insert("joined", vec![b.v("x"), b.v("x")]));
    refused("a-write-in-a-query.json", &joined(), &query_writes, "the body's effect is Write, above Read");
    let undeclared = one(vec![], &[("x", named("Nope"))], unit.clone(), Effect::Pure, |_| nothing());
    refused("an-undeclared-type.json", &sch, &undeclared, "Nope is not declared");
    let unsorted = one(vec![name_decl(), id_decl()], &[], unit.clone(), Effect::Pure, |_| nothing());
    refused("unsorted-declarations.json", &sch, &unsorted, "not sorted");
    let mixed = one(vec![], &[], Ty::bool(), Effect::Pure, |_| op(Op::Eq, int(1), text("1")));
    refused("an-int-compared-with-text.json", &sch, &mixed, "compared with");
    let no_table = one(vec![], &[], Ty::Int, Effect::Read, |_| std(Std::Len, vec![scan("song")]));
    refused("no-such-table.json", &sch, &no_table, "no table song");
    let trim_bytes = ("Raw".to_string(), decl(Ty::Bytes, &[(Rule::Trim, None)]));
    let bad_rule = one(vec![trim_bytes], &[], unit, Effect::Pure, |_| nothing());
    refused("trim-on-bytes.json", &sch, &bad_rule, "Raw: Trim does not fit Bytes");
}
