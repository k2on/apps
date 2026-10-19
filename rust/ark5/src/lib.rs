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
//! Functions are opaque here: a [`peer::Procedure`] stands for one, found
//! by its [`hash::FnHash`] in a [`peer::Cas`]. The language that writes
//! them (§1.4) is another layer's.

use std::fmt;

pub mod codec;
pub mod hash;
pub mod ty;

/// Why something was not done, as a sentence: a parse that failed, a write
/// a constraint refused, an intent its function refused. What a form shows
/// and what a `Reject` frame carries.
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
