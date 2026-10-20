//! §1.1 Types: the five constructors every value is read under.
//!
//! There is no run-time value. A value is bytes, and a [`Ty`] says how to
//! read them ([`crate::codec`] is how). Everything else a module calls a
//! type — `Bool`, `Option<T>`, an enum, `Text`, `Id<t>`, a row — is a
//! declaration over these five, by name; the ones the kernel itself uses
//! are the constructors below. A refinement (`where utf8`, `where len 16`)
//! is a predicate on a structural type's values and is the language's
//! (§1.4), checked at the boundary by `parse`; the kernel parses the
//! structure only. `Named` is a declaration's name, which only the
//! language resolves (`crate::check`); a recursive type recurs through one.

use ark_macros::Node;

use crate::hash::TypeHash;
use crate::ir::Node;

/// A type (§1.1): how to read a value's bytes.
///
/// `Sum`'s variants are each a tuple of fields, so a variant with no
/// payload is an empty one. A `Sum` has at most 256 variants: its index is
/// one byte.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Node)]
pub enum Ty {
    Int,
    Bytes,
    List(Box<Ty>),
    Tuple(Vec<Ty>),
    Sum(Vec<Vec<Ty>>),
    Named(String),
}

impl Ty {
    pub fn list(t: Ty) -> Ty {
        Ty::List(Box::new(t))
    }

    /// `()`, the empty tuple: no bytes at all.
    pub fn unit() -> Ty {
        Ty::Tuple(vec![])
    }

    /// `Bool = Sum [(), ()]`: `false < true`.
    pub fn bool() -> Ty {
        Ty::Sum(vec![vec![], vec![]])
    }

    /// `Option<T> = Sum [(), (T)]`: `None < Some`.
    pub fn option(t: Ty) -> Ty {
        Ty::Sum(vec![vec![], vec![t]])
    }

    /// An enum of `n` variants without payloads, ordered by declaration.
    pub fn enumeration(n: usize) -> Ty {
        Ty::Sum(vec![vec![]; n])
    }

    /// Whether this is `Option<T>`'s shape, which is what a column appended
    /// to a table may be without a default (§1.2).
    pub fn is_option(&self) -> bool {
        matches!(self, Ty::Sum(vs) if vs.len() == 2 && vs[0].is_empty() && vs[1].len() == 1)
    }

    /// The type as a value of its own: `Sum [(), (), (Ty), (List Ty),
    /// (List (List Ty)), (Bytes)]`, `Ty` recurring by name (derived).
    pub fn enc(&self, out: &mut Vec<u8>) {
        self.put(out);
    }

    /// `sha256(enc(ty))`.
    pub fn hash(&self) -> TypeHash {
        let mut b = vec![];
        self.enc(&mut b);
        TypeHash::of(&b)
    }
}
