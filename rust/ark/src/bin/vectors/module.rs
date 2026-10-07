//! `module/` (§7): the module as a value, its canonical bytes and its hash;
//! decode of encode is the identity. The demo's, and the demo with a guard
//! testing a role beside it (`docs/plan-guards.md` D1).

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
    out.write(
        "module/demo.json",
        &obj(&[
            ("module", json(&v)),
            ("bytes", quoted(&hex(&bytes))),
            ("hash", quoted(&hex(&module_hash(&m)))),
        ]),
    );
}
