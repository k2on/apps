//! `module/` (§7): the module as a value, its canonical bytes and its hash;
//! decode of encode is the identity.

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
    out.write(
        "module/demo.json",
        &obj(&[
            ("module", json(&v)),
            ("bytes", quoted(&hex(&bytes))),
            ("hash", quoted(&hex(&module_hash(&m)))),
        ]),
    );
}
