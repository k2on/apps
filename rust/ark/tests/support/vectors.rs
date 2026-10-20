//! Writing a vector under `spec/vectors/`, or holding it to what is
//! there: `ARK_WRITE_VECTORS` set writes, and otherwise the text must be
//! what is on disk. A `falsify/` vector is a deliberately wrong one, marked
//! `"expect": "fail"`, which a conformant runner must fail: it is written
//! only after this runner has been shown to disagree with it.

#![allow(dead_code)]

use std::path::PathBuf;

use super::json::obj;

fn dir(d: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/vectors").join(d)
}

pub fn vector(d: &str, name: &str, parts: &[(&str, String)]) {
    let text = obj(parts);
    let path = dir(d).join(name);
    if std::env::var_os("ARK_WRITE_VECTORS").is_some() {
        std::fs::create_dir_all(dir(d)).unwrap();
        std::fs::write(&path, &text).unwrap();
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(on_disk, text, "{d}/{name} moved; ARK_WRITE_VECTORS=1 writes it again");
}

/// A wrong vector beside the right one, which this runner must disagree
/// with.
pub fn falsify(d: &str, name: &str, wrong: &[(&str, String)], right: &[(&str, String)]) {
    assert_ne!(obj(wrong), obj(right), "{d}/falsify/{name} is not wrong");
    let mut parts = wrong.to_vec();
    parts.push(("expect", "\"fail\"".into()));
    vector(&format!("{d}/falsify"), name, &parts);
}

/// `fns` over `schema` as ArkLang (§1.15), quoted: a vector's module as
/// text beside its bytes — informative, the bytes normative, and a runner
/// may ignore it — or `null` where the functions do not check.
pub fn ark_text(schema: &ark::schema::Schema, fns: &[(&str, &ark::ir::Function)]) -> String {
    let mut bound: Vec<(String, ark::hash::FnHash)> = fns.iter().map(|(n, f)| (n.to_string(), f.hash())).collect();
    bound.sort();
    bound.dedup_by(|a, b| a.0 == b.0);
    let mut types: Vec<(String, ark::ir::Decl)> = fns.iter().flat_map(|(_, f)| f.types.clone()).collect();
    types.sort_by(|a, b| a.0.cmp(&b.0));
    types.dedup_by(|a, b| a.0 == b.0);
    let module = ark::module::Module {
        spec: ark::module::SPEC,
        schema: schema.clone(),
        types,
        bindings: bound,
    };
    let all: Vec<ark::ir::Function> = fns.iter().map(|(_, f)| (*f).clone()).collect();
    ark::sexp::print(&module, &all).map_or("null".into(), |t| super::json::quoted(&t))
}
