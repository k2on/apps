//! §8 Hashes, as `Ark.Hash` defines them: two quantities every runtime must
//! reproduce byte for byte, both SHA-256 over canonical CBOR.

use std::collections::BTreeMap;

use crate::canon::encode;
use crate::ir::encode::{calls, function_value, module_value};
use crate::ir::normalize::{normalize, normalize_module};
use crate::ir::{Function, Module};
use crate::sha256::sha256;
use crate::store::Store;
use crate::value::Value;

/// The 32-byte hash an entry names its function by.
pub type FnHash = Vec<u8>;

/// §8.3 A function with the helpers it was verified against: every helper
/// the function reaches, in the version current when it was hashed, in
/// declaration order — a complete program `apply_closure` runs without
/// consulting any module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Closure {
    pub function: Function,
    pub helpers: Vec<Function>,
}

/// The closure of a function within a module: the function normalised,
/// with the helpers it reaches, normalised, in the module's order
/// (`Ark.Hash.closure`).
pub fn closure(m: &Module, f: &Function) -> Closure {
    let mut seen: Vec<String> = Vec::new();
    let mut todo: Vec<String> = calls(f);
    todo.reverse();
    while let Some(n) = todo.pop() {
        if seen.contains(&n) {
            continue;
        }
        if let Some(h) = m.lookup_function(&n) {
            seen.push(n);
            let mut more = calls(h);
            more.reverse();
            todo.extend(more);
        }
    }
    Closure {
        function: normalize(f),
        helpers: m.functions.iter().filter(|h| seen.contains(&h.name)).map(normalize).collect(),
    }
}

/// Every function of a module, by the hash of its closure
/// (`Ark.Hash.closures`).
pub fn closures(m: &Module) -> BTreeMap<FnHash, Closure> {
    m.functions
        .iter()
        .map(|f| {
            let c = closure(m, f);
            (function_hash(&c), c)
        })
        .collect()
}

/// §8.1 The state hash of a store: the hash of the canonical encoding of a
/// list with, for every table of the schema in schema order, the table's
/// name and its rows in key order. Tables with no rows contribute their name
/// and an empty list.
pub fn state_hash(st: &dyn Store) -> Vec<u8> {
    let tables: Vec<Value> = st
        .schema()
        .tables()
        .map(|t| {
            Value::List(vec![
                Value::text(&t.name),
                Value::List(st.scan(&t.name).into_iter().map(Value::Struct).collect()),
            ])
        })
        .collect();
    sha256(&encode(&Value::List(tables)))
}

/// §8.2 The hash of a function: over its normalised canonical form, names
/// excluded, together with the hashes of the helpers it calls directly —
/// which cover theirs in turn.
pub fn function_hash(c: &Closure) -> FnHash {
    let mut deps = BTreeMap::new();
    for n in calls(&c.function) {
        if let Some(h) = c.helpers.iter().find(|h| h.name == n) {
            deps.insert(
                n,
                Value::Bytes(function_hash(&Closure {
                    function: h.clone(),
                    helpers: c.helpers.clone(),
                })),
            );
        }
    }
    sha256(&encode(&function_value(&deps, &c.function)))
}

/// The hash of a whole module, normalised.
pub fn module_hash(m: &Module) -> Vec<u8> {
    sha256(&encode(&module_value(&normalize_module(m))))
}
