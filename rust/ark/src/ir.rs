//! §1.4 The language as a value, and every Rust type that is one.
//!
//! [`Node`] is a Rust type read as a §1.1 type: its [`Ty`], its encoder
//! and its decoder, by the codec. `#[derive(Node)]` writes it for a struct
//! (a `Tuple` of its fields) and an enum (a `Sum` of its variants' tuples);
//! the impls here are the leaves. So the IR, [`Ty`], the schema and every
//! frame are encoded by one codec, with no walk written by hand.
//!
//! [`Expr`] is the one expression language: every mutator, query, helper,
//! guard, scope and live transition. Variables are **De Bruijn indices** —
//! `Var(0)` is the innermost binder — so a function's bytes, and its
//! [`FnHash`], do not depend on the names it was written with. A binder is
//! a `Let` (one), a `Match` arm (the variant's fields, in order, the last
//! innermost), a [`Function`]'s parameters (likewise) and a `Lambda` (its
//! arity), which stands only as an operator's argument. Every expression
//! has a type and an [`Effect`], `Pure < Read < Write` ([`crate::check`]).
//!
//! A [`Function`] carries the declarations of the types it names, so its
//! hash covers what its input's refinements mean (§1.5). A [`Decl`] is a
//! structural type and the refinements on it (§1.1): `Text = Bytes where
//! utf8`, `Name = Text where trim, len 1..=80`.

use std::cell::RefCell;

use ark_macros::Node;

use crate::codec::{put_bytes, put_int, put_list, Reader};
use crate::hash::{EntryHash, FnHash, ModuleHash, SchemaHash, StateHash, TypeHash};
use crate::log::{EntryId, Origin};
use crate::store::Dir;
use crate::ty::Ty;
use crate::Refusal;

/// A Rust type as a value of §1.1: how to read and write its bytes.
pub trait Node: Sized {
    fn ty() -> Ty;
    fn put(&self, out: &mut Vec<u8>);
    fn take(r: &mut Reader) -> Result<Self, Refusal>;

    /// `List Self`; `u8` makes it `Bytes`, so `Vec<u8>` is bytes.
    fn list_ty() -> Ty {
        Ty::list(Self::ty())
    }
    fn put_list(xs: &[Self], out: &mut Vec<u8>) {
        put_list(out, xs, |o, x| x.put(o));
    }
    fn take_list(r: &mut Reader) -> Result<Vec<Self>, Refusal> {
        r.list(Self::take)
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut out = vec![];
        self.put(&mut out);
        out
    }

    /// The value, from canonical bytes of it and nothing after them.
    fn from_bytes(b: &[u8]) -> Result<Self, Refusal> {
        let mut r = Reader::new(b);
        let x = Self::take(&mut r)?;
        r.end()?;
        Ok(x)
    }
}

thread_local!(static OPEN: RefCell<Vec<&'static str>> = const { RefCell::new(vec![]) });

/// The type a derived `Node` is: `f()`, or `Named(name)` where the type
/// recurs inside itself.
pub fn named(name: &'static str, f: impl FnOnce() -> Ty) -> Ty {
    if OPEN.with(|o| o.borrow().contains(&name)) {
        return Ty::Named(name.into());
    }
    OPEN.with(|o| o.borrow_mut().push(name));
    let t = f();
    OPEN.with(|o| o.borrow_mut().pop());
    t
}

impl Node for i64 {
    fn ty() -> Ty {
        Ty::Int
    }
    fn put(&self, out: &mut Vec<u8>) {
        put_int(out, *self);
    }
    fn take(r: &mut Reader) -> Result<i64, Refusal> {
        r.int()
    }
}

macro_rules! narrow {
    ($($t:ty),*) => {$(
        impl Node for $t {
            fn ty() -> Ty {
                Ty::Int
            }
            fn put(&self, out: &mut Vec<u8>) {
                put_int(out, *self as i64);
            }
            fn take(r: &mut Reader) -> Result<$t, Refusal> {
                let at = r.at;
                <$t>::try_from(r.int()?).map_err(|_| Refusal(format!("an int out of range at byte {at}")))
            }
        }
    )*};
}
narrow!(usize, u32);

impl Node for u8 {
    fn ty() -> Ty {
        Ty::Int
    }
    fn put(&self, out: &mut Vec<u8>) {
        put_int(out, *self as i64);
    }
    fn take(r: &mut Reader) -> Result<u8, Refusal> {
        u8::try_from(r.int()?).map_err(|_| Refusal::new("a byte out of range"))
    }
    fn list_ty() -> Ty {
        Ty::Bytes
    }
    fn put_list(xs: &[u8], out: &mut Vec<u8>) {
        put_bytes(out, xs);
    }
    fn take_list(r: &mut Reader) -> Result<Vec<u8>, Refusal> {
        r.bytes()
    }
}

impl Node for bool {
    fn ty() -> Ty {
        Ty::bool()
    }
    fn put(&self, out: &mut Vec<u8>) {
        out.push(*self as u8);
    }
    fn take(r: &mut Reader) -> Result<bool, Refusal> {
        Ok(r.variant(2)? == 1)
    }
}

impl Node for String {
    fn ty() -> Ty {
        Ty::Bytes
    }
    fn put(&self, out: &mut Vec<u8>) {
        put_bytes(out, self.as_bytes());
    }
    fn take(r: &mut Reader) -> Result<String, Refusal> {
        r.text()
    }
}

impl Node for () {
    fn ty() -> Ty {
        Ty::unit()
    }
    fn put(&self, _: &mut Vec<u8>) {}
    fn take(_: &mut Reader) -> Result<(), Refusal> {
        Ok(())
    }
}

impl<T: Node> Node for Vec<T> {
    fn ty() -> Ty {
        T::list_ty()
    }
    fn put(&self, out: &mut Vec<u8>) {
        T::put_list(self, out);
    }
    fn take(r: &mut Reader) -> Result<Vec<T>, Refusal> {
        T::take_list(r)
    }
}

impl<T: Node> Node for Option<T> {
    fn ty() -> Ty {
        Ty::option(T::ty())
    }
    fn put(&self, out: &mut Vec<u8>) {
        crate::codec::put_option(out, self.as_ref(), |o, x| x.put(o));
    }
    fn take(r: &mut Reader) -> Result<Option<T>, Refusal> {
        r.option(T::take)
    }
}

impl<T: Node> Node for Box<T> {
    fn ty() -> Ty {
        T::ty()
    }
    fn put(&self, out: &mut Vec<u8>) {
        (**self).put(out);
    }
    fn take(r: &mut Reader) -> Result<Box<T>, Refusal> {
        T::take(r).map(Box::new)
    }
}

macro_rules! tuples {
    ($(($($t:ident $i:tt),+))*) => {$(
        impl<$($t: Node),+> Node for ($($t,)+) {
            fn ty() -> Ty {
                Ty::Tuple(vec![$($t::ty()),+])
            }
            fn put(&self, out: &mut Vec<u8>) {
                $(self.$i.put(out);)+
            }
            fn take(r: &mut Reader) -> Result<Self, Refusal> {
                Ok(($($t::take(r)?,)+))
            }
        }
    )*};
}
tuples!((A 0) (A 0, B 1) (A 0, B 1, C 2) (A 0, B 1, C 2, D 3));

/// A fixed number of bytes — a hash, an id — is `Bytes` of that length.
macro_rules! fixed {
    ($($t:ty: $n:literal),*) => {$(
        impl Node for $t {
            fn ty() -> Ty {
                Ty::Bytes
            }
            fn put(&self, out: &mut Vec<u8>) {
                put_bytes(out, &self.0);
            }
            fn take(r: &mut Reader) -> Result<$t, Refusal> {
                r.fixed::<$n>().map(Self)
            }
        }
    )*};
}
fixed!(FnHash: 32, StateHash: 32, ModuleHash: 32, TypeHash: 32, EntryHash: 32, SchemaHash: 32, EntryId: 16, Origin: 16);

// * The language

/// What an expression may do: compute, read the store, write it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Node)]
pub enum Effect {
    Pure,
    Read,
    Write,
}

/// Checked arithmetic (an overflow, a division by zero, is a refusal),
/// comparison by the values' bytes, and the booleans, short-circuiting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Node)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Node)]
pub enum Un {
    Neg,
    Not,
}

/// What a function knows of the entry it runs for (§1.6): all frozen in
/// it. `NewId(t)` is the entry's next draw, typed `Id<t>`.
#[derive(Clone, Debug, PartialEq, Eq, Node)]
pub enum CtxField {
    User,
    Session,
    Roles,
    Now,
    NewId(String),
}

/// The standard library, by name, in v4's order ([`crate::std`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Node)]
pub enum Std {
    Trim,
    IsEmpty,
    Concat,
    Lower,
    IsAlnum,
    Chars,
    TextLen,
    StartsWith,
    SplitOnce,
    TextOfInt,
    Hex,
    Min,
    Max,
    Clamp,
    Abs,
    Fnv1a64,
    Sha256,
    IdOfText,
    TextOfId,
    NilId,
    Utf8,
    First,
    Last,
    Len,
    Contains,
    Reverse,
    ListConcat,
    Insert,
    RemoveAt,
    Move,
    Slice,
    IsSome,
    UnwrapOr,
    Unwrap,
    /// Appended by V1: a list's integers added in an `i128`, the total
    /// judged once (v4's §6.5 rule, [`crate::std`]).
    Sum,
}

#[derive(Clone, Debug, PartialEq, Eq, Node)]
pub enum Expr {
    /// A value's bytes under its type, checked once by the verifier.
    Lit(Ty, Vec<u8>),
    Var(u32),
    Let(Box<Expr>, Box<Expr>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    /// One arm per variant, in order, binding its fields.
    Match(Box<Expr>, Vec<Expr>),
    Field(Box<Expr>, u32),
    Tuple(Vec<Expr>),
    /// Variant `i` of a sum type, from its fields.
    Variant(Ty, u8, Vec<Expr>),
    /// A list of the element type.
    List(Ty, Vec<Expr>),
    Op(Op, Box<Expr>, Box<Expr>),
    Un(Un, Box<Expr>),
    Std(Std, Vec<Expr>),
    /// A function by hash: the call graph is a DAG, so every function ends.
    Call(FnHash, Vec<Expr>),
    Ctx(CtxField),
    /// Binds its arity; only as an operator's argument.
    Lambda(u32, Box<Expr>),
    Scan(String),
    /// A table's row by its key, the tuple of the key columns.
    Lookup(String, Box<Expr>),
    Filter(Box<Expr>, Box<Expr>),
    Map(Box<Expr>, Box<Expr>),
    /// The pairs of the two lists the lambda admits, in the order of the first.
    Join(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `(key, aggregate of (key, members))` per key, in key order.
    Group(Box<Expr>, Box<Expr>, Box<Expr>),
    /// Stable, by the lambda's key, each of its fields in its direction.
    Sort(Box<Expr>, Box<Expr>, Vec<Dir>),
    Take(Box<Expr>, Box<Expr>),
    Insert(String, Box<Expr>),
    /// The row under the key, through the lambda.
    Update(String, Box<Expr>, Box<Expr>),
    Delete(String, Box<Expr>),
    Refuse(Box<Expr>),
    Seq(Vec<Expr>),
}

/// A predicate a refinement holds its values to (§1.1). `Len` counts code
/// points of text, bytes of other bytes and elements of a list; `Trim`
/// takes white space off both ends of text before any later rule.
#[derive(Clone, Debug, PartialEq, Eq, Node)]
pub enum Rule {
    Utf8,
    Len(i64),
    LenRange(i64, i64),
    Trim,
    NonEmpty,
    /// A `Pure` predicate of the value, `Var(0)`.
    Refine(Expr),
}

/// A rule, and what a form shows when a value breaks it (`None`: the
/// default words).
#[derive(Clone, Debug, PartialEq, Eq, Node)]
pub struct Refine {
    pub rule: Rule,
    pub why: Option<String>,
}

/// A declared type: a structure, the refinements on it, and its names —
/// a record's fields in order, or a sum's variants each followed by its
/// fields' (`[Paused, at_ms, Playing, at_ms, since]`) — so a printer
/// names what the bytes otherwise hold by position (§1.15). A tuple or a
/// sum without names, or any other structure, has none.
#[derive(Clone, Debug, PartialEq, Eq, Node)]
pub struct Decl {
    pub ty: Ty,
    pub refine: Vec<Refine>,
    pub names: Vec<String>,
}

/// A function: the declarations it names, sorted by name; its parameters,
/// bound in order (the last is `Var(0)`); what it returns; the most it may
/// do; its body; and the names its binders were written with, as a
/// declaration keeps its fields' (§1.15) — its parameters', then each
/// `Let`'s, arm's and lambda's in the order [`crate::interp::walk`] meets
/// them, `""` for one the text gave no name — so a printer prints the
/// author's. None, or too few, and the printer names the rest. A mutator
/// is `(Input) -> ()` at `Write`, a scope `(Row) -> Bool` at `Pure`, a
/// query `(Input) -> List<T>` at `Read`.
#[derive(Clone, Debug, PartialEq, Eq, Node)]
pub struct Function {
    pub types: Vec<(String, Decl)>,
    pub params: Vec<Ty>,
    pub ret: Ty,
    pub effect: Effect,
    pub body: Expr,
    pub names: Vec<String>,
}

impl Function {
    /// `sha256(enc(function))`.
    pub fn hash(&self) -> FnHash {
        FnHash::of(&self.to_bytes())
    }

    /// The function with its binders' names set aside.
    pub fn unnamed(&self) -> Function {
        Function {
            names: vec![],
            ..self.clone()
        }
    }
}

/// `Text = Bytes where utf8`, which every module has without declaring it.
pub fn text_decl() -> Decl {
    Decl {
        ty: Ty::Bytes,
        refine: vec![Refine { rule: Rule::Utf8, why: None }],
        names: vec![],
    }
}
