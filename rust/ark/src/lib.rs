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

/// The attribute macros and what a module is assembled into, at the
/// crate's root, so a domain writes `#[ark::mutator]` and `ark::module!`
/// (§1.12).
pub use author::{guard, helper, live, module, mutator, query, scope, Domain};

pub mod author;
pub mod build;
pub mod check;
pub mod codec;
pub mod frames;
pub mod hash;
pub mod interp;
pub mod ir;
pub mod log;
pub mod module;
pub mod peer;
pub mod schema;
pub mod std;
pub mod store;
pub mod text;
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
