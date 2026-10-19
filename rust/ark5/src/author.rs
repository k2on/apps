//! §1.12 Authoring: the run-time half of `#[ark::mutator]` and its kin.
//!
//! The macros (`ark5-macros`) read a function's restricted Rust and write
//! Rust that builds its [`Expr`] — this module is what that Rust calls. A
//! proc-macro crate can export nothing else, and a field is read **by
//! name** only where its struct is known, which rustc knows and a macro
//! does not: so every expression is built as an [`Ex<T>`], `T` the Rust
//! type it is, and `x.f` is `view(x).f` on the struct the derive wrote for
//! `T` ([`Record`]). rustc resolves the names and the arities; the checker
//! (`crate::check`) judges the types, refinements included: `T` is a
//! type's [`Shape::Base`], its refinements dropped, so `Name` and `Text`
//! are one Rust type and only the checker tells them apart.
//!
//! A variable is a De Bruijn index the macro computes from the binders it
//! sees; an `Ex` bound to a Rust variable is a *witness* of its type, and
//! each use is `w.at(i)`. A function is built once ([`build`]) in a
//! context that gathers the declarations it names — its parameters', its
//! result's, its literals', its fresh ids', its callees' — which are its
//! `types` (§1.5); a call names the callee by hash and keeps it, so a
//! [`Domain`] installs the DAG callees first.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::sync::{Arc, OnceLock};

pub use ark5_macros::{guard, helper, live, module, mutator, query, scope, Input, Node, Row, Schema, Sum};

pub use crate::ir::Node;

use crate::check::Types;
use crate::codec::{bytes_enc, int_enc, Reader};
use crate::hash::FnHash;
use crate::ir::{CtxField, Decl, Effect, Expr, Function, Op, Refine, Rule, Std, Un};
use crate::module::{Lib, Module, SPEC};
use crate::peer::Cas;
use crate::schema::{Schema, Space};
use crate::store::Dir;
use crate::ty::Ty;
use crate::Refusal;

pub type Decls = BTreeMap<String, Decl>;

/// A Rust type as a type of §1.1: its `Ty`, the declarations that names,
/// what a column of it says it is, and the Rust type an expression of it
/// is built as (refinements dropped).
pub trait Shape: 'static {
    type Base: Shape;
    fn ty() -> Ty;
    fn decls(_: &mut Decls) {}
    fn name() -> String;
}

/// A struct whose fields are read by name: [`view`] of an expression of it.
pub trait Record: Shape {
    type View;
    fn view(e: Expr) -> Self::View;
}

/// A table's row struct (`#[derive(Row)]`).
pub trait Row: Record {
    const TABLE: &'static str;
    fn table() -> crate::schema::Table;
}

/// `Text = Bytes where utf8`; a Rust `String`.
pub type Text = String;
/// `Int`; a Rust `i64`.
pub type Int = i64;

/// `Id<t> = Bytes where len 16`: a row id of table `R`.
pub struct Id<R>(pub [u8; 16], PhantomData<fn() -> R>);

impl<R> Id<R> {
    pub fn new(b: [u8; 16]) -> Id<R> {
        Id(b, PhantomData)
    }
}
impl<R> Clone for Id<R> {
    fn clone(&self) -> Self {
        Id::new(self.0)
    }
}
impl<R> PartialEq for Id<R> {
    fn eq(&self, o: &Self) -> bool {
        self.0 == o.0
    }
}
impl<R> Eq for Id<R> {}
impl<R> PartialOrd for Id<R> {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl<R> Ord for Id<R> {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        self.0.cmp(&o.0)
    }
}
impl<R> std::fmt::Debug for Id<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&crate::hash::hex(&self.0))
    }
}
impl<R> Node for Id<R> {
    fn ty() -> Ty {
        Ty::Bytes
    }
    fn put(&self, out: &mut Vec<u8>) {
        crate::codec::put_bytes(out, &self.0);
    }
    fn take(r: &mut Reader) -> Result<Self, Refusal> {
        r.fixed::<16>().map(Id::new)
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

/// Plain `Bytes`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bytes(pub Vec<u8>);

/// The empty sum: what an element of no list is.
pub enum Never {}

macro_rules! leaf {
    ($($t:ty => $ty:expr, $n:literal);*) => {$(
        impl Shape for $t {
            type Base = $t;
            fn ty() -> Ty { $ty }
            fn name() -> String { $n.into() }
        }
    )*};
}
leaf!(i64 => Ty::Int, "Int"; String => Types::text(), "Text"; bool => Ty::bool(), "Bool";
      () => Ty::unit(), "()"; Bytes => Ty::Bytes, "Bytes"; Never => Ty::Sum(vec![]), "Never");

impl<R: Row> Shape for Id<R> {
    type Base = Id<R>;
    fn ty() -> Ty {
        Ty::Named(Self::name())
    }
    fn decls(d: &mut Decls) {
        let len = Refine { rule: Rule::Len(16), why: None };
        d.insert(Self::name(), Decl { ty: Ty::Bytes, refine: vec![len] });
    }
    fn name() -> String {
        format!("Id<{}>", R::TABLE)
    }
}
/// `Option<T>` is declared by its name, a name for its structure, so a
/// column of it reads as one wherever a function names it.
impl<T: Shape> Shape for Option<T> {
    type Base = Option<T::Base>;
    fn ty() -> Ty {
        Ty::Named(Self::name())
    }
    fn decls(d: &mut Decls) {
        d.insert(Self::name(), Decl { ty: Ty::option(T::ty()), refine: vec![] });
        T::decls(d)
    }
    fn name() -> String {
        format!("Option<{}>", T::name())
    }
}
impl<T: Shape> Shape for Vec<T> {
    type Base = Vec<T::Base>;
    fn ty() -> Ty {
        Ty::list(T::ty())
    }
    fn decls(d: &mut Decls) {
        T::decls(d)
    }
    fn name() -> String {
        format!("List<{}>", T::name())
    }
}

macro_rules! tuples {
    ($(($($t:ident $i:tt),*)),*) => {$(
        impl<$($t: Shape),*> Shape for ($($t,)*) {
            type Base = ($($t::Base,)*);
            fn ty() -> Ty { Ty::Tuple(vec![$($t::ty()),*]) }
            fn decls(d: &mut Decls) { $($t::decls(d);)* }
            fn name() -> String { format!("({})", [$($t::name()),*].join(", ")) }
        }
        impl<$($t: Shape),*> Record for ($($t,)*) {
            type View = ($(Ex<$t>,)*);
            fn view(e: Expr) -> Self::View { ($(Ex::of(Expr::Field(Box::new(e.clone()), $i)),)*) }
        }
        impl<$($t: Shape),*> Tup for ($(Ex<$t>,)*) {
            type Out = ($($t,)*);
            fn tup(self) -> Ex<Self::Out> { Ex::of(Expr::Tuple(vec![$(self.$i.e),*])) }
        }
    )*};
}
tuples!((A 0, B 1), (A 0, B 1, C 2), (A 0, B 1, C 2, D 3));

/// A tuple of expressions as an expression of the tuple.
pub trait Tup {
    type Out: Shape;
    fn tup(self) -> Ex<Self::Out>;
}

pub fn tup<T: Tup>(t: T) -> Ex<T::Out> {
    t.tup()
}

/// An expression built as Rust type `T`.
pub struct Ex<T> {
    pub e: Expr,
    t: PhantomData<fn() -> T>,
}

impl<T> Clone for Ex<T> {
    fn clone(&self) -> Self {
        Ex::of(self.e.clone())
    }
}

impl<T> Ex<T> {
    pub fn of(e: Expr) -> Ex<T> {
        Ex { e, t: PhantomData }
    }

    /// The variable `i`, of this witness's type.
    pub fn at(&self, i: u32) -> Ex<T> {
        Ex::of(Expr::Var(i))
    }
}

/// A witness of `x`'s type, for a variable bound to it.
pub fn w<T>(_: &Ex<T>) -> Ex<T> {
    Ex::of(Expr::Var(u32::MAX))
}

/// The witness of an option's payload.
pub fn some_w<T>(_: &Ex<Option<T>>) -> Ex<T> {
    Ex::of(Expr::Var(u32::MAX))
}

pub fn view<T: Record>(x: Ex<T>) -> T::View {
    T::view(x.e)
}

fn b(e: Ex<impl Sized>) -> Box<Expr> {
    Box::new(e.e)
}

// * The context a function is built in

struct Cx {
    decls: Decls,
    calls: Vec<&'static Built>,
    params: Vec<Ty>,
}

thread_local!(static CX: RefCell<Vec<Cx>> = const { RefCell::new(vec![]) });

/// `T`'s declarations, in the function being built.
pub fn note<T: Shape>() {
    CX.with(|c| c.borrow_mut().last_mut().map(|cx| T::decls(&mut cx.decls)));
}

/// What a function is: an entry point, a query, a scope, a live
/// transition (a mutator over volatile tables, §1.9), or a function the
/// others call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Mutator,
    Query,
    Scope,
    Live,
    Helper,
}

/// A function as built: its binding's name, the function, its hash, and
/// the functions it calls.
#[derive(Debug)]
pub struct Built {
    pub name: &'static str,
    pub kind: Kind,
    pub f: Function,
    pub hash: FnHash,
    pub calls: Vec<&'static Built>,
}

/// A parameter of the function being built: its type, and a witness.
pub fn param<T: Shape>() -> Ex<T::Base> {
    note::<T>();
    CX.with(|c| c.borrow_mut().last_mut().expect("in a build").params.push(T::ty()));
    Ex::of(Expr::Var(u32::MAX))
}

/// The function `cell` holds, built on first use.
pub fn once(cell: &'static OnceLock<Built>, f: impl FnOnce() -> Built) -> &'static Built {
    match cell.get() {
        Some(b) => b,
        None => {
            let b = f();
            cell.get_or_init(|| b)
        }
    }
}

/// Builds a function whose result is `R`. A function that reaches itself
/// through its calls is a cycle, which a hash cannot name: refused here.
pub fn build<R: Shape>(name: &'static str, kind: Kind, body: impl FnOnce() -> Ex<R::Base>) -> Built {
    thread_local!(static BUILDING: RefCell<Vec<&'static str>> = const { RefCell::new(vec![]) });
    BUILDING.with(|s| {
        assert!(!s.borrow().contains(&name), "{name} calls itself, which a function named by its hash cannot");
        s.borrow_mut().push(name)
    });
    CX.with(|c| c.borrow_mut().push(Cx { decls: Decls::new(), calls: vec![], params: vec![] }));
    note::<R>();
    let body = body().e;
    let cx = CX.with(|c| c.borrow_mut().pop().unwrap());
    BUILDING.with(|s| s.borrow_mut().pop());
    let effect = match kind {
        Kind::Mutator | Kind::Live => Effect::Write,
        Kind::Query => Effect::Read,
        Kind::Scope => Effect::Pure,
        Kind::Helper => effect(&body, &cx.calls),
    };
    let types = cx.decls.into_iter().filter(|(n, _)| n != "Text").collect();
    let f = Function { types, params: cx.params, ret: R::ty(), effect, body };
    Built { name, kind, hash: f.hash(), f, calls: cx.calls }
}

/// The least effect `e` has: a read, a write, a fresh id, or a callee's.
fn effect(e: &Expr, calls: &[&Built]) -> Effect {
    let own = match e {
        Expr::Scan(_) | Expr::Lookup(..) => Effect::Read,
        Expr::Insert(..) | Expr::Update(..) | Expr::Delete(..) | Expr::Ctx(CtxField::NewId(_)) => Effect::Write,
        Expr::Call(h, _) => calls.iter().find(|c| c.hash == *h).map_or(Effect::Pure, |c| c.f.effect),
        _ => Effect::Pure,
    };
    children(e).into_iter().map(|c| effect(c, calls)).fold(own, Effect::max)
}

/// The expressions directly under `e`.
pub fn children(e: &Expr) -> Vec<&Expr> {
    use Expr::*;
    match e {
        Lit(..) | Var(_) | Ctx(_) | Scan(_) => vec![],
        Let(a, b) | Op(_, a, b) | Filter(a, b) | Map(a, b) | Take(a, b) | Sort(a, b, _) | Update(_, a, b) => vec![a, b],
        If(a, b, c) | Join(a, b, c) | Group(a, b, c) => vec![a, b, c],
        Match(a, arms) => std::iter::once(&**a).chain(arms).collect(),
        Field(a, _) | Un(_, a) | Lambda(_, a) | Lookup(_, a) | Insert(_, a) | Delete(_, a) | Refuse(a) => vec![a],
        Tuple(es) | Variant(_, _, es) | List(_, es) | Std(_, es) | Call(_, es) | Seq(es) => es.iter().collect(),
    }
}

/// A call of a built function: its declarations become the caller's.
pub fn call<R>(f: &'static Built, args: Vec<Expr>) -> Ex<R> {
    CX.with(|c| {
        let mut c = c.borrow_mut();
        let cx = c.last_mut().expect("a call inside a build");
        cx.decls.extend(f.f.types.iter().cloned());
        cx.calls.push(f);
    });
    Ex::of(Expr::Call(f.hash, args))
}

// * Values

pub fn int(n: i64) -> Ex<i64> {
    Ex::of(Expr::Lit(Ty::Int, int_enc(n).to_vec()))
}

pub fn text(s: &str) -> Ex<String> {
    Ex::of(Expr::Lit(Types::text(), bytes_enc(s.as_bytes())))
}

pub fn bool(v: bool) -> Ex<bool> {
    Ex::of(Expr::Lit(Ty::bool(), vec![v as u8]))
}

pub fn unit() -> Ex<()> {
    Ex::of(Expr::Seq(vec![]))
}

pub fn some<T: Shape>(x: Ex<T>) -> Ex<Option<T>> {
    note::<Option<T>>();
    Ex::of(Expr::Variant(Option::<T>::ty(), 1, vec![x.e]))
}

pub fn none<T: Shape>() -> Ex<Option<T>> {
    note::<Option<T>>();
    Ex::of(Expr::Variant(Option::<T>::ty(), 0, vec![]))
}

pub fn list<T: Shape>(xs: Vec<Ex<T>>) -> Ex<Vec<T>> {
    note::<T>();
    Ex::of(Expr::List(T::ty(), xs.into_iter().map(|x| x.e).collect()))
}

/// Fields given by position, in any order, the rest read from `base`.
fn ordered(n: u32, mut given: Vec<(u32, Expr)>, base: Option<Expr>) -> Vec<Expr> {
    given.sort_by_key(|g| g.0);
    let mut given = given.into_iter().peekable();
    (0..n)
        .map(|i| match given.next_if(|g| g.0 == i) {
            Some((_, e)) => e,
            None => Expr::Field(Box::new(base.clone().expect("every field given, or a base")), i),
        })
        .collect()
}

/// A struct literal: its fields by position, the rest from `..base`.
pub fn record<S: Shape>(n: u32, given: Vec<(u32, Expr)>, base: Option<Ex<S>>) -> Ex<S> {
    Ex::of(Expr::Tuple(ordered(n, given, base.map(|b| b.e))))
}

/// Variant `i` of `S`, its fields by position.
pub fn variant<S: Shape>(i: u8, n: u32, given: Vec<(u32, Expr)>) -> Ex<S> {
    note::<S>();
    Ex::of(Expr::Variant(S::ty(), i, ordered(n, given, None)))
}

/// A field's setter's result, for [`record`] and [`variant`].
pub fn set<T>(i: u32, e: Ex<T>) -> (u32, Expr) {
    (i, e.e)
}

pub fn if_<T>(c: Ex<bool>, a: Ex<T>, b: Ex<T>) -> Ex<T> {
    Ex::of(Expr::If(self::b(c), self::b(a), self::b(b)))
}

pub fn let_<A, B>(v: Ex<A>, body: Ex<B>) -> Ex<B> {
    Ex::of(Expr::Let(b(v), b(body)))
}

/// Statements, then the last one, whose value the block's is.
pub fn seq<T>(first: Vec<Expr>, last: Ex<T>) -> Ex<T> {
    match first.is_empty() {
        true => last,
        false => Ex::of(Expr::Seq(first.into_iter().chain([last.e]).collect())),
    }
}

/// A statement's expression, its value dropped: `()` as it is, anything
/// else followed by `()`.
pub fn stmt<T: Shape>(e: Ex<T>) -> Ex<()> {
    match T::ty() == Ty::unit() {
        true => Ex::of(e.e),
        false => Ex::of(Expr::Seq(vec![e.e, Expr::Seq(vec![])])),
    }
}

/// A refinement of an `Int`: from `lo` to `hi`, both included.
pub fn within(lo: i64, hi: i64) -> Rule {
    let (Rule::Refine(a), Rule::Refine(b)) = (bound(Op::Ge, lo), bound(Op::Le, hi)) else { unreachable!() };
    Rule::Refine(Expr::Op(Op::And, Box::new(a), Box::new(b)))
}

/// A refinement of an `Int`: the value `o` the bound.
pub fn bound(o: Op, n: i64) -> Rule {
    Rule::Refine(Expr::Op(o, Box::new(Expr::Var(0)), Box::new(int(n).e)))
}

/// One arm per variant in order: those written, by index, and `wild` for
/// the rest, given how many fields each binds.
pub fn match_<S, R>(s: Ex<S>, arities: &[u32], arms: Vec<(u8, Ex<R>)>, wild: Option<&dyn Fn(u32) -> Ex<R>>) -> Ex<R> {
    let mut arms: BTreeMap<u8, Expr> = arms.into_iter().map(|(i, a)| (i, a.e)).collect();
    let each = arities.iter().enumerate().map(|(i, n)| {
        arms.remove(&(i as u8))
            .unwrap_or_else(|| wild.expect("every variant matched")(*n).e)
    });
    Ex::of(Expr::Match(b(s), each.collect()))
}

pub fn refuse<T>(why: Ex<String>) -> Ex<T> {
    Ex::of(Expr::Refuse(b(why)))
}

pub fn arith(o: Op, a: Ex<i64>, c: Ex<i64>) -> Ex<i64> {
    Ex::of(Expr::Op(o, b(a), b(c)))
}

pub fn cmp<A, B>(o: Op, a: Ex<A>, c: Ex<B>) -> Ex<bool> {
    Ex::of(Expr::Op(o, b(a), b(c)))
}

pub fn logic(o: Op, a: Ex<bool>, c: Ex<bool>) -> Ex<bool> {
    Ex::of(Expr::Op(o, b(a), b(c)))
}

pub fn not(a: Ex<bool>) -> Ex<bool> {
    Ex::of(Expr::Un(Un::Not, b(a)))
}

pub fn neg(a: Ex<i64>) -> Ex<i64> {
    Ex::of(Expr::Un(Un::Neg, b(a)))
}

fn std<T>(f: Std, args: Vec<Expr>) -> Ex<T> {
    Ex::of(Expr::Std(f, args))
}

fn lam<R>(n: u32, body: Ex<R>) -> Box<Expr> {
    Box::new(Expr::Lambda(n, b(body)))
}

fn wit<T>() -> Ex<T> {
    Ex::of(Expr::Var(u32::MAX))
}

// * The context

/// `ctx`: what a function knows of its entry (§1.6).
#[derive(Clone, Copy)]
pub struct Ctx;

impl Ctx {
    pub fn user(self) -> Ex<String> {
        Ex::of(Expr::Ctx(CtxField::User))
    }
    pub fn session(self) -> Ex<String> {
        Ex::of(Expr::Ctx(CtxField::Session))
    }
    pub fn roles(self) -> Ex<Vec<String>> {
        Ex::of(Expr::Ctx(CtxField::Roles))
    }
    pub fn now(self) -> Ex<i64> {
        Ex::of(Expr::Ctx(CtxField::Now))
    }
    /// The entry's next draw, an id of table `R`.
    pub fn new_id<R: Row>(self) -> Ex<Id<R>> {
        note::<Id<R>>();
        Ex::of(Expr::Ctx(CtxField::NewId(R::TABLE.into())))
    }
    pub fn has_role(self, r: Ex<String>) -> Ex<bool> {
        self.roles().contains(r)
    }
}

// * Tables

/// `db.<table>`: a table's rows, and its writes.
pub struct Table<R>(PhantomData<fn() -> R>);

impl<R> Default for Table<R> {
    fn default() -> Self {
        Table(PhantomData)
    }
}

impl<R: Row> Table<R> {
    pub fn all(self) -> Ex<Vec<R>> {
        Ex::of(Expr::Scan(R::TABLE.into()))
    }
    /// The row under a key: the tuple of the key columns' values.
    pub fn get(self, key: Vec<Expr>) -> Ex<Option<R>> {
        Ex::of(Expr::Lookup(R::TABLE.into(), Box::new(Expr::Tuple(key))))
    }
    pub fn insert(self, row: Ex<R>) -> Ex<()> {
        Ex::of(Expr::Insert(R::TABLE.into(), b(row)))
    }
    pub fn update(self, key: Vec<Expr>, f: impl FnOnce(Ex<R>) -> Ex<R>) -> Ex<()> {
        Ex::of(Expr::Update(R::TABLE.into(), Box::new(Expr::Tuple(key)), lam(1, f(wit()))))
    }
    pub fn delete(self, key: Vec<Expr>) -> Ex<()> {
        Ex::of(Expr::Delete(R::TABLE.into(), Box::new(Expr::Tuple(key))))
    }
}

// * The standard library and the relational operators, as methods

impl Ex<String> {
    pub fn trim(self) -> Ex<String> {
        std(Std::Trim, vec![self.e])
    }
    pub fn is_empty(self) -> Ex<bool> {
        std(Std::IsEmpty, vec![self.e])
    }
    pub fn lower(self) -> Ex<String> {
        std(Std::Lower, vec![self.e])
    }
    pub fn is_alnum(self) -> Ex<bool> {
        std(Std::IsAlnum, vec![self.e])
    }
    pub fn chars(self) -> Ex<Vec<String>> {
        std(Std::Chars, vec![self.e])
    }
    pub fn len(self) -> Ex<i64> {
        std(Std::TextLen, vec![self.e])
    }
    pub fn starts_with(self, p: Ex<String>) -> Ex<bool> {
        std(Std::StartsWith, vec![self.e, p.e])
    }
    pub fn split_once(self, p: Ex<String>) -> Ex<Option<(String, String)>> {
        std(Std::SplitOnce, vec![self.e, p.e])
    }
    pub fn fnv1a64(self) -> Ex<i64> {
        std(Std::Fnv1a64, vec![self.e])
    }
    pub fn utf8(self) -> Ex<Bytes> {
        std(Std::Utf8, vec![self.e])
    }
    pub fn id_of_text<R>(self) -> Ex<Option<Id<R>>> {
        std(Std::IdOfText, vec![self.e])
    }
}

impl Ex<i64> {
    pub fn to_text(self) -> Ex<String> {
        std(Std::TextOfInt, vec![self.e])
    }
    pub fn min(self, o: Ex<i64>) -> Ex<i64> {
        std(Std::Min, vec![self.e, o.e])
    }
    pub fn max(self, o: Ex<i64>) -> Ex<i64> {
        std(Std::Max, vec![self.e, o.e])
    }
    pub fn clamp(self, lo: Ex<i64>, hi: Ex<i64>) -> Ex<i64> {
        std(Std::Clamp, vec![self.e, lo.e, hi.e])
    }
    pub fn abs(self) -> Ex<i64> {
        std(Std::Abs, vec![self.e])
    }
}

impl Ex<Bytes> {
    pub fn hex(self) -> Ex<String> {
        std(Std::Hex, vec![self.e])
    }
    pub fn sha256(self) -> Ex<Bytes> {
        std(Std::Sha256, vec![self.e])
    }
}

impl<R> Ex<Id<R>> {
    pub fn text_of_id(self) -> Ex<String> {
        std(Std::TextOfId, vec![self.e])
    }
    pub fn hex(self) -> Ex<String> {
        std(Std::Hex, vec![self.e])
    }
}

/// `concat` of texts.
pub fn concat(xs: Ex<Vec<String>>) -> Ex<String> {
    std(Std::Concat, vec![xs.e])
}

pub fn nil_id<R>() -> Ex<Id<R>> {
    std(Std::NilId, vec![])
}

impl<T: Shape> Ex<Vec<T>> {
    pub fn first(self) -> Ex<Option<T>> {
        std(Std::First, vec![self.e])
    }
    pub fn last(self) -> Ex<Option<T>> {
        std(Std::Last, vec![self.e])
    }
    pub fn len(self) -> Ex<i64> {
        std(Std::Len, vec![self.e])
    }
    pub fn is_empty(self) -> Ex<bool> {
        cmp(Op::Eq, self.len(), int(0))
    }
    pub fn contains(self, x: Ex<T>) -> Ex<bool> {
        std(Std::Contains, vec![self.e, x.e])
    }
    pub fn reverse(self) -> Ex<Vec<T>> {
        std(Std::Reverse, vec![self.e])
    }
    pub fn concat(self, o: Ex<Vec<T>>) -> Ex<Vec<T>> {
        std(Std::ListConcat, vec![self.e, o.e])
    }
    pub fn insert(self, i: Ex<i64>, x: Ex<T>) -> Ex<Vec<T>> {
        std(Std::Insert, vec![self.e, i.e, x.e])
    }
    pub fn remove_at(self, i: Ex<i64>) -> Ex<Vec<T>> {
        std(Std::RemoveAt, vec![self.e, i.e])
    }
    pub fn move_at(self, from: Ex<i64>, to: Ex<i64>) -> Ex<Vec<T>> {
        std(Std::Move, vec![self.e, from.e, to.e])
    }
    pub fn slice(self, i: Ex<i64>, j: Ex<i64>) -> Ex<Vec<T>> {
        std(Std::Slice, vec![self.e, i.e, j.e])
    }
    pub fn take(self, n: Ex<i64>) -> Ex<Vec<T>> {
        Ex::of(Expr::Take(b(self), b(n)))
    }
    pub fn filter(self, p: impl FnOnce(Ex<T>) -> Ex<bool>) -> Ex<Vec<T>> {
        Ex::of(Expr::Filter(b(self), lam(1, p(wit()))))
    }
    pub fn map<U>(self, f: impl FnOnce(Ex<T>) -> Ex<U>) -> Ex<Vec<U>> {
        Ex::of(Expr::Map(b(self), lam(1, f(wit()))))
    }
    /// The pairs the condition admits, in this list's order.
    pub fn join<U>(self, o: Ex<Vec<U>>, on: impl FnOnce(Ex<T>, Ex<U>) -> Ex<bool>) -> Ex<Vec<(T, U)>> {
        let on = Box::new(Expr::Lambda(2, b(on(wit(), wit()))));
        Ex::of(Expr::Join(b(self), b(o), on))
    }
    /// `(key, aggregate)` per key, in key order.
    pub fn group<K, A>(self, by: impl FnOnce(Ex<T>) -> Ex<K>, agg: impl FnOnce(Ex<K>, Ex<Vec<T>>) -> Ex<A>) -> Ex<Vec<(K, A)>> {
        let by = lam(1, by(wit()));
        let agg = Box::new(Expr::Lambda(2, b(agg(wit(), wit()))));
        Ex::of(Expr::Group(b(self), by, agg))
    }
    /// Stable, by the key (each field of a tuple of keys in its direction).
    pub fn sort<K>(self, dirs: Vec<Dir>, keys: impl FnOnce(Ex<T>) -> Ex<K>) -> Ex<Vec<T>> {
        Ex::of(Expr::Sort(b(self), lam(1, keys(wit())), dirs))
    }
}

impl Ex<Vec<i64>> {
    /// The total, judged once (`Std::Sum`).
    pub fn sum(self) -> Ex<i64> {
        std(Std::Sum, vec![self.e])
    }
}

impl<T: Shape> Ex<Option<T>> {
    pub fn is_some(self) -> Ex<bool> {
        std(Std::IsSome, vec![self.e])
    }
    pub fn is_none(self) -> Ex<bool> {
        not(self.is_some())
    }
    pub fn unwrap(self) -> Ex<T> {
        std(Std::Unwrap, vec![self.e])
    }
    pub fn unwrap_or(self, d: Ex<T>) -> Ex<T> {
        std(Std::UnwrapOr, vec![self.e, d.e])
    }
    /// `match self { None => d, Some(x) => f(x) }`.
    pub fn map_or<U>(self, d: Ex<U>, f: impl FnOnce(Ex<T>) -> Ex<U>) -> Ex<U> {
        Ex::of(Expr::Match(b(self), vec![d.e, f(wit()).e]))
    }
    pub fn map<U: Shape>(self, f: impl FnOnce(Ex<T>) -> Ex<U>) -> Ex<Option<U>> {
        let some = some(f(wit())).e;
        self.map_or(none::<U>(), |_| Ex::of(some))
    }
    /// The value when the predicate holds of it.
    pub fn filter(self, p: impl FnOnce(Ex<T>) -> Ex<bool>) -> Ex<Option<T>> {
        let keep = if_(p(wit()), some(Ex::<T>::of(Expr::Var(0))), none());
        self.map_or(none(), |_| keep)
    }
}

// * A module

/// A domain assembled: the schema, the module binding names to hashes,
/// every function checked into a library, and their bodies.
pub struct Domain {
    pub schema: Arc<Schema>,
    pub module: Module,
    pub lib: Lib,
    pub bodies: BTreeMap<FnHash, Vec<u8>>,
}

impl Domain {
    pub fn get(&self, name: &str) -> FnHash {
        self.module.get(name).unwrap_or_else(|| panic!("no function {name}"))
    }

    /// A content store holding every function, loading what it is sent.
    pub fn cas(&self) -> Cas {
        let mut c = Cas::default().with_loader(self.lib.loader(self.schema.clone()));
        for b in self.bodies.values() {
            c.add_body(b.clone());
        }
        c
    }

    /// A function's answer over a store: its input parsed under its
    /// parameters, its body's value — what a query is asked.
    pub fn ask(&self, store: &dyn crate::store::Read, ctx: &crate::peer::Ctx, name: &str, input: &[u8]) -> Result<Vec<u8>, Refusal> {
        let f = self.lib.get(&self.get(name)).expect("an installed function");
        let ty = crate::interp::Interp::input_ty(&f.f);
        let v = crate::interp::decode(&f.cx, &ty, crate::interp::parse(&f.cx, &ty, input)?.bytes())?;
        let callee = |h: &FnHash| self.lib.get(h);
        let mut m = crate::interp::Machine::new(store, ctx, &callee, &f.cx);
        m.stack = match (v, f.f.params.len()) {
            (crate::interp::V::Tup(vs), n) if n != 1 => vs,
            (v, _) => vec![v],
        };
        Ok(m.eval(&f.f.body)?.to_bytes())
    }

    /// A mutator's changes over a store, as the kernel runs it.
    pub fn run(&self, store: &dyn crate::store::Read, ctx: &crate::peer::Ctx, name: &str, input: &[u8]) -> Result<Vec<crate::store::Change>, Refusal> {
        self.lib.procedure(&self.get(name)).expect("an installed function").run(store, ctx, input)
    }

    fn install(&mut self, f: &Built) -> Result<FnHash, Refusal> {
        for c in &f.calls {
            self.install(c)?;
        }
        if f.kind == Kind::Live {
            let mut todo = vec![&f.f.body];
            let mut fns: Vec<&Built> = f.calls.clone();
            while let Some(c) = fns.pop() {
                todo.push(&c.f.body);
                fns.extend(c.calls.iter().copied());
            }
            while let Some(e) = todo.pop() {
                if let Expr::Scan(t) | Expr::Lookup(t, _) | Expr::Insert(t, _) | Expr::Update(t, ..) | Expr::Delete(t, _) = e {
                    if self.schema.table(t).is_some_and(|(_, t)| t.space != Space::Volatile) {
                        return Err(Refusal(format!("{}: a live transition touches {t}, which is durable", f.name)));
                    }
                }
                todo.extend(children(e));
            }
        }
        self.bodies.insert(f.hash, f.f.to_bytes());
        self.lib.install(&self.schema, f.f.clone()).map_err(|r| Refusal(format!("{}: {r}", f.name)))
    }
}

/// §1.5 The module of a schema and functions: each checked, its callees
/// first, bound by its name; the types every one of them declares.
pub fn assemble(schema: Schema, fns: &[&'static Built]) -> Result<Domain, Refusal> {
    let mut d = Domain {
        schema: Arc::new(schema.clone()),
        module: Module { spec: SPEC, schema, types: vec![], bindings: vec![] },
        lib: Lib::default(),
        bodies: BTreeMap::new(),
    };
    let (mut bindings, mut types) = (BTreeMap::new(), BTreeMap::new());
    for f in fns {
        if bindings.insert(f.name.to_string(), d.install(f)?).is_some() {
            return Err(Refusal(format!("{} is bound twice", f.name)));
        }
        types.extend(f.f.types.iter().cloned());
    }
    d.module.bindings = bindings.into_iter().collect();
    d.module.types = types.into_iter().collect();
    Ok(d)
}

/// A column's type: its declaration's name when it is one, else its
/// structure — as the checker types a row's columns.
pub fn column_ty<T: Shape>() -> (String, Ty, Ty) {
    let mut d = Decls::new();
    T::decls(&mut d);
    let cx = Types::new(&d.into_iter().collect::<Vec<_>>());
    let t = T::ty();
    let structure = cx.deep(&t);
    let row = if matches!(t, Ty::Named(_)) { t } else { structure.clone() };
    (T::name(), structure, row)
}

/// A declaration of a record's fields, or of a refinement of one type.
pub fn decl(name: &str, ty: Ty, refine: Vec<(Rule, Option<&str>)>, d: &mut Decls) {
    let refine = refine.into_iter().map(|(rule, why)| Refine { rule, why: why.map(Into::into) }).collect();
    d.insert(name.into(), Decl { ty, refine });
}
