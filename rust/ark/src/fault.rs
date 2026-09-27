//! A fault, as GENERATED.md names it: what a generated body stops on.
//!
//! Two constructors that are never conflated: [`Fault::Refuse`] is a
//! verdict — a deterministic fact about the entry that every replica
//! reaches (an explicit `refuse`, a constraint, an overflow) — and
//! [`Fault::Bug`] is a bug: a module the verifier would have refused, or a
//! generator that disagrees with `Ark.Eval`. In Rust a fault is
//! `Err(Fault)`.

use std::fmt;

use crate::value::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    /// A verdict: the text a mutator's `refuse` gave, or a constraint's or a
    /// checked operation's name (`"integer overflow"`, `"division by
    /// zero"`).
    Refuse(String),
    /// A bug, never a verdict; a conformant runtime never reports one for a
    /// verified module.
    Bug(String),
}

/// What a refusal may be given as its reason: a string, or the `Value`
/// the emitter spells a mutator's `refuse` argument as (§4.1: it is
/// always a text in a verified module; any other value reads as its
/// display form rather than stopping the runtime).
pub trait Reason {
    fn reason(self) -> String;
}

impl Reason for String {
    fn reason(self) -> String {
        self
    }
}

impl Reason for &str {
    fn reason(self) -> String {
        self.to_string()
    }
}

impl Reason for Value {
    fn reason(self) -> String {
        match self {
            Value::Text(t) => t,
            other => other.to_string(),
        }
    }
}

impl Fault {
    /// `Fault.refuse(text)`.
    pub fn refuse<R: Reason>(reason: R) -> Fault {
        Fault::Refuse(reason.reason())
    }

    /// `Fault.bug(text)`.
    pub fn bug<S: Into<String>>(text: S) -> Fault {
        Fault::Bug(text.into())
    }
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fault::Refuse(t) => write!(f, "refused: {t}"),
            Fault::Bug(t) => write!(f, "bug: {t}"),
        }
    }
}

impl std::error::Error for Fault {}
