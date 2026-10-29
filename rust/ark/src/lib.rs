//! ArkDB v5's kernel, as `docs/plan-v5.md` Part 1 defines it: a value is
//! its bytes in the value format and a key its `memcomparable` bytes
//! (§1.1, [`format`]), JSON its human form ([`json`]); hash
//! spaces are distinct types ([`hash`]); the schema is data and the only
//! place that knows what a row is (§1.2, [`schema`]); one ordered store
//! whose every write is a [`store::Change`] that inverts exactly (§1.3,
//! [`store`]); the log, a chain of entries each carrying its facts and the
//! state after it (§1.6, [`log`]); one sans-io peer machine whose role is
//! replica or authority (§1.7, [`peer`]); and the frames (§1.11,
//! [`frames`]).
//!
//! The kernel runs a function as a [`peer::Procedure`], found by its name
//! in a peer's [`peer::Functions`]. A domain is plain Rust (§1.16): rows,
//! inputs and the schema derived ([`row`]), functions registered by name
//! in a [`Domain`] ([`domain`]), reads and writes through a typed store
//! ([`db`]) and views as plans maintained per operator ([`plan`]); rows
//! without a Rust type are JSON under the schema ([`dynamic`]); effects
//! are rows, jobs a handler claims and finishes ([`effect`]).

use ::std::fmt;

extern crate self as ark;

pub use domain::{effect, Domain};

pub mod db;
pub mod domain;
pub mod dynamic;
pub mod format;
pub mod frames;
pub mod hash;
pub mod json;
pub mod log;
pub mod migrate;
pub mod peer;
pub mod plan;
pub mod row;
pub mod schema;
pub mod store;

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
    pub use crate::db::{Db, Tables, Volatile};
    pub use crate::domain::{Domain, Scope};
    pub use crate::effect::{Effect, Job};
    pub use crate::format::Encode;
    pub use crate::peer::Ctx;
    pub use crate::plan::{scan, scan_from, Desc, Plan};
    pub use crate::row::{Describe, Id, Index, Input, Row};
    pub use crate::Refusal;
    pub use ark_macros::{live, mutator, query, scope, view, Describe, Effect, Input, Row, Schema};
    pub use serde::{Deserialize, Serialize};
}
