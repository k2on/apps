//! `module/` (§7): the module as a value, its canonical bytes and its hash;
//! decode of encode is the identity. The demo's, the demo with a guard
//! testing a role beside it (`docs/plan-guards.md` D1), and a router of
//! scopes over its tables (D2).

use ark::canon::{decode, encode};
use ark::hash::module_hash;
use ark::ir::{module_from_value, module_value, Module};
use ark::value::hex;

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
    // A module with a guard testing a role (`docs/plan-guards.md` D1):
    // `has_role` on the wire, and the hash of a module that has one.
    let g = demo::guarded();
    let gv = module_value(&g);
    super::claim(
        "the guarded module holds has_role",
        ark::canon::encode(&gv).windows(8).any(|w| w == b"has_role"),
    );
    match decode(&encode(&gv)).map(|v| module_from_value(&v)) {
        Ok(Ok(back)) if module_value(&back) == gv => {}
        other => panic!("module guarded: {other:?}"),
    }
    out.write(
        "module/guarded.json",
        &obj(&[
            ("module", json(&gv)),
            ("bytes", quoted(&hex(&encode(&gv)))),
            ("hash", quoted(&hex(&module_hash(&g)))),
        ]),
    );
    // A module with scopes (`docs/plan-guards.md` D2): one on the router,
    // one on a chain, the `exists` form and a projection; `holds`,
    // `pwhen`, `pexists` and `exclude` on the wire.
    let sc = demo::scoped();
    let sv = module_value(&sc);
    let sbytes = encode(&sv);
    for word in [&b"holds"[..], b"pwhen", b"pexists", b"exclude", b"scope"] {
        super::claim(
            &format!("the scoped module carries {}", String::from_utf8_lossy(word)),
            sbytes.windows(word.len()).any(|w| w == word),
        );
    }
    match decode(&sbytes).map(|v| module_from_value(&v)) {
        Ok(Ok(back)) if module_value(&back) == sv => {}
        other => panic!("module scoped: {other:?}"),
    }
    out.write(
        "module/scoped.json",
        &obj(&[
            ("module", json(&sv)),
            ("bytes", quoted(&hex(&sbytes))),
            ("hash", quoted(&hex(&module_hash(&sc)))),
        ]),
    );
    // And for the verifier: the scoped module verifies, and the same with
    // `mine`'s filter reading an argument — a scope is a function of who
    // the person is, never of what they asked — claimed to verify.
    out.write("verify/scoped-ok.json", &obj(&[("module", json(&sv)), ("verifies", "true".into())]));
    let mut reads_input = sc.clone();
    for f in reads_input.functions.iter_mut().filter(|f| f.name == "mine") {
        f.holds[0].filter = Some(ark::ir::Pred::Cmp("user_id".into(), ark::ir::CmpOp::Eq, ark::ir::Expr::Arg("who".into())));
    }
    match ark::verify::verify(&reads_input) {
        Err(es)
            if es
                .iter()
                .any(|e| matches!(e, ark::verify::VerifyError::In(_, ark::verify::Complaint::ScopeReadsBeyondCtx(_)))) => {}
        other => panic!("verify: a scope reading input was not refused: {other:?}"),
    }
    out.write(
        "verify/falsify/a-scope-reading-input.json",
        &obj(&[
            ("module", json(&module_value(&reads_input))),
            ("verifies", "true".into()),
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
