//! `module/` (§7): the module as a value, its canonical bytes and its hash;
//! decode of encode is the identity. And `module/rules`, `verify/rules-ok`
//! (`docs/plan-auth.md`): the demo with a rule of every form declared — a
//! column that is `Me`, a role, the one lookup — written where a table has
//! one and nowhere else, so `module/demo.json`'s bytes are unmoved.

use ark::canon::{decode, encode};
use ark::hash::module_hash;
use ark::ir::{module_from_value, module_value, CmpOp, Expr, Module, Pred};
use ark::value::{hex, Value};
use ark::verify::verify;

use super::demo;
use super::json::{json, obj, quoted};
use super::Out;

pub fn module(out: &Out) {
    out.dir("module/");
    let m = demo::module();
    let v = module_value(&m);
    let bytes = encode(&v);
    // Symbol names are not in the wire form, so they cannot come back.
    let unnamed = Module {
        functions: m
            .functions
            .iter()
            .map(|f| ark::ir::Function {
                names: Default::default(),
                ..f.clone()
            })
            .collect(),
        ..m.clone()
    };
    match decode(&bytes).map(|v| module_from_value(&v)) {
        Ok(Ok(back)) if back == unnamed => {}
        Ok(Ok(_)) => panic!("module: decode . encode is not the identity"),
        other => panic!("module: {other:?}"),
    }
    // The hash of the same module with the query's plan left out: a runtime
    // that does not hash the plan would claim it.
    let planless = Module {
        functions: m.functions.iter().map(|f| ark::ir::Function { plan: None, ..f.clone() }).collect(),
        ..m.clone()
    };
    out.write(
        "module/falsify/hash-without-the-plan.json",
        &obj(&[
            ("module", json(&v)),
            ("bytes", quoted(&hex(&bytes))),
            ("hash", quoted(&hex(&module_hash(&planless)))),
            ("expect", quoted("fail")),
        ]),
    );
    out.write(
        "module/demo.json",
        &obj(&[
            ("module", json(&v)),
            ("bytes", quoted(&hex(&bytes))),
            ("hash", quoted(&hex(&module_hash(&m)))),
        ]),
    );
}

/// The demo with rules: a playlist is seen by its owner and by anyone when
/// one of its items is the track `"public"`, and written by its owner; an
/// item is written by an `editor`. Built on the module rather than in the
/// vocabulary because the demo's rows are the spec's Appendix B, which
/// declares none; `rust/ark/tests/rules.rs` holds the vocabulary to this
/// IR.
pub fn ruled() -> Module {
    let mut m = demo::module();
    let mine = Pred::Cmp("user_id".into(), CmpOp::Eq, Expr::CtxUser);
    let public = Pred::Exists(
        "item".into(),
        "playlist_id".into(),
        Box::new(Pred::Cmp("track_id".into(), CmpOp::Eq, Expr::Lit(Value::text("public")))),
    );
    for t in m.schema.tables.iter_mut() {
        let rules = match t.name.as_str() {
            "playlist" => (Some(Pred::Any(vec![mine.clone(), public.clone()])), Some(mine.clone())),
            _ => (None, Some(Pred::Role("editor".into()))),
        };
        *t = t.clone().with_rules(rules.0, rules.1);
    }
    m
}

pub fn rules(out: &Out) {
    out.dir("module/ verify/ (rules)");
    let m = ruled();
    assert!(verify(&m).is_ok(), "the demo with rules verifies");
    let v = module_value(&m);
    let bytes = encode(&v);
    match decode(&bytes).map(|v| module_from_value(&v)) {
        Ok(Ok(back)) if back.schema == m.schema => {}
        other => panic!("module/rules: decode . encode is not the identity: {other:?}"),
    }
    assert_ne!(module_hash(&m), module_hash(&demo::module()), "a rule is in the module's hash");
    out.write(
        "module/rules.json",
        &obj(&[
            ("module", json(&v)),
            ("bytes", quoted(&hex(&bytes))),
            ("hash", quoted(&hex(&module_hash(&m)))),
        ]),
    );
    out.write("verify/rules-ok.json", &obj(&[("module", json(&v)), ("verifies", "true".into())]));
    // A rule naming a column its table does not have, claimed to verify.
    let mut bad = m.clone();
    let t = bad.schema.tables[0].clone();
    bad.schema.tables[0] = t.with_rules(Some(Pred::Cmp("owner".into(), CmpOp::Eq, Expr::CtxUser)), None);
    assert!(verify(&bad).is_err(), "a rule naming no column is refused");
    out.write(
        "verify/falsify/a-rule-naming-no-column.json",
        &obj(&[
            ("module", json(&module_value(&bad))),
            ("verifies", "true".into()),
            ("expect", quoted("fail")),
        ]),
    );
}
