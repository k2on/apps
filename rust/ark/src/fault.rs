//! A fault, as GENERATED.md names it: what a generated body stops on.
//!
//! Two constructors that are never conflated: [`Fault::Refuse`] is a
//! verdict — a deterministic fact about the entry that every replica
//! reaches (an explicit `refuse`, a constraint, an overflow) — and
//! [`Fault::Bug`] is a bug: a module the verifier would have refused, or a
//! generator that disagrees with `Ark.Eval`. In Rust a fault is
//! `Err(Fault)`.

use std::fmt;

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

impl Fault {
    /// `Fault.refuse(text)`.
    pub fn refuse<S: Into<String>>(text: S) -> Fault {
        Fault::Refuse(text.into())
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
