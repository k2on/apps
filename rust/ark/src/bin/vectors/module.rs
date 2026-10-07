//! `module/` (§7): the module as a value, its canonical bytes and its hash;
//! decode of encode is the identity. The demo's, the demo with a guard
//! testing a role beside it (`docs/plan-guards.md` D1), a router of
//! scopes over its tables (D2), a server's module with a private block and
//! the client's stripped from it (D3), and a table's CRUD in one line beside
//! the same written by hand (D4).

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
    // `docs/plan-guards.md` D3 A server's module with a private block,
    // and the module a client loads from it: the block gone, `private: true`
    // left, and every function named by one hash in both.
    let pm = demo::private();
    let stripped = ark::verify::verify(&ark::ir::strip_module(&pm)).expect("the stripped module verifies");
    let (pv, stv) = (module_value(&pm), module_value(&stripped));
    let (pbytes, stbytes) = (encode(&pv), encode(&stv));
    let kept = b"the server keeps that name";
    for word in [&b"private"[..], kept] {
        super::claim(
            &format!("the server's module carries {}", String::from_utf8_lossy(word)),
            pbytes.windows(word.len()).any(|w| w == word),
        );
    }
    super::claim(
        "the stripped module carries nothing of the block",
        !stbytes.windows(kept.len()).any(|w| w == kept),
    );
    super::claim(
        "the stripped module says private",
        stripped
            .functions
            .iter()
            .any(|f| f.private && f.body.iter().all(|s| !matches!(s, ark::ir::Stmt::Private(_)))),
    );
    let names = |m: &Module| -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = ark::hash::closures(m).into_iter().map(|(h, c)| (c.function.name, hex(&h))).collect();
        v.sort();
        v
    };
    super::claim("every function has one hash in both modules", names(&pm) == names(&stripped));
    super::claim("the two modules are two hashes", module_hash(&pm) != module_hash(&stripped));
    match decode(&pbytes).map(|v| module_from_value(&v)) {
        Ok(Ok(back)) if module_value(&back) == pv => {}
        other => panic!("module private: {other:?}"),
    }
    let fns = |m: &Module| -> String {
        let pairs: Vec<String> = names(m).into_iter().map(|(n, h)| format!("[{},{}]", quoted(&n), quoted(&h))).collect();
        format!("[{}]", pairs.join(","))
    };
    out.write(
        "module/private.json",
        &obj(&[
            ("module", json(&pv)),
            ("bytes", quoted(&hex(&pbytes))),
            ("hash", quoted(&hex(&module_hash(&pm)))),
            ("stripped", json(&stv)),
            ("strippedBytes", quoted(&hex(&stbytes))),
            ("public", quoted(&hex(&module_hash(&stripped)))),
            ("functions", fns(&stripped)),
        ]),
    );
    // And a client's module that lost the flag with the block: its
    // `create_audited` is another function, at another hash, and a runner
    // must say so.
    let unflagged = Module {
        functions: stripped
            .functions
            .iter()
            .map(|f| ark::ir::Function { private: false, ..f.clone() })
            .collect(),
        ..stripped.clone()
    };
    super::claim("the flag is hashed", names(&unflagged) != names(&stripped));
    let uv = module_value(&unflagged);
    out.write(
        "module/falsify/stripped-without-the-flag.json",
        &obj(&[
            ("module", json(&pv)),
            ("bytes", quoted(&hex(&pbytes))),
            ("hash", quoted(&hex(&module_hash(&pm)))),
            ("stripped", json(&uv)),
            ("strippedBytes", quoted(&hex(&encode(&uv)))),
            ("public", quoted(&hex(&module_hash(&unflagged)))),
            ("functions", fns(&stripped)),
            ("expect", quoted("fail")),
        ]),
    );
    out.write("verify/private-ok.json", &obj(&[("module", json(&pv)), ("verifies", "true".into())]));
    // `docs/plan-guards.md` D4 A table's CRUD in one line, and the same
    // four mutations written by hand: one module, byte for byte — so one
    // hash, and every function one hash — written as a pair, each in this
    // directory's form.
    let crud = ark::authoring::Module::new((demo::demo(), demo::crud()));
    let by_hand = ark::authoring::Module::new((demo::demo(), demo::crud_by_hand()));
    let (cm, hm) = (crud.build(), by_hand.build());
    super::claim("crud emits what the same lines by hand emit", crud.emit() == by_hand.emit());
    super::claim(
        "crud emits the four, under the router's guard",
        ["insert_item", "update_item", "delete_item", "put_item"]
            .iter()
            .all(|n| cm.lookup_function(n).is_some_and(|f| f.uses == ["signed_in"])),
    );
    for (name, m) in [("crud", cm), ("crud-by-hand", hm)] {
        let mv = module_value(m);
        out.write(
            &format!("module/{name}.json"),
            &obj(&[
                ("module", json(&mv)),
                ("bytes", quoted(&hex(&encode(&mv)))),
                ("hash", quoted(&hex(&module_hash(m)))),
            ]),
        );
    }
    out.write(
        "module/demo.json",
        &obj(&[
            ("module", json(&v)),
            ("bytes", quoted(&hex(&bytes))),
            ("hash", quoted(&hex(&module_hash(&m)))),
        ]),
    );
}
