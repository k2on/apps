//! ArkDB v5's kernel, as `docs/plan-v5.md` Part 1 defines it: a value is
//! bytes and a type says how to read them (§1.1, [`ty`], [`codec`]); hash
//! spaces are distinct types ([`hash`]); the schema is data and the only
//! place that knows what a row is (§1.2, [`schema`]); one ordered store
//! whose every write is a [`store::Change`] that inverts exactly (§1.3,
//! [`store`]); the log, a chain of entries each carrying its facts and the
//! state after it (§1.6, [`log`]); one sans-io peer machine whose role is
//! replica or authority (§1.7, [`peer`]); and the frames (§1.11,
//! [`frames`]).
//!
//! The kernel runs a function as a [`peer::Procedure`], found by its
//! [`hash::FnHash`] in a [`peer::Cas`]. The language is the rest (§1.4,
//! §1.5): the IR and every type as a value ([`ir`], `#[derive(Node)]`),
//! one checker ([`check`]), one interpreter, which is a `Procedure`, and
//! the boundary's `parse` ([`interp`]), the standard library ([`std`]) and
//! modules as binding maps over the content store ([`module`]).

use ::std::fmt;

extern crate self as ark;

/// What a module is assembled into (§1.15).
pub use module::Domain;

pub mod check;
pub mod codec;
pub mod db;
pub mod domain;
pub mod frames;
pub mod gen;
pub mod hash;
pub mod interp;
pub mod ir;
pub mod log;
pub mod module;
pub mod peer;
pub mod plan;
pub mod row;
pub mod schema;
pub mod sexp;
pub mod std;
pub mod store;
pub mod ty;
pub mod unicode_tables;
pub mod view;

/// Why something was not done, as a sentence: a parse that failed, a write
/// a constraint refused, an intent its function refused. What a form shows
/// and what a refused `Verdict` carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal(pub String);

impl Refusal {
    pub fn new(why: impl Into<String>) -> Refusal {
        Refusal(why.into())
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a domain written in plain Rust uses (§1.16): the derives and the
/// attributes that register a function by name, the context, the typed
/// store, plans, ids and indexes.
pub mod prelude {
    pub use crate::db::{Db, Tables};
    pub use crate::domain::{Domain, Scope};
    pub use crate::ir::Node;
    pub use crate::peer::Ctx;
    pub use crate::plan::{scan, Desc, Plan};
    pub use crate::row::{Describe, Id, Index, Input, Row};
    pub use crate::Refusal;
    pub use ark_macros::{live, mutator, query, scope, view, Describe, Input, Node, Row, Schema};
}
