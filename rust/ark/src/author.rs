//! §1.15 Authoring in Rust: the builders, and what a Rust type is as a
//! type of §1.1.
//!
//! [`Shape`] is a Rust type read as a module's type: its `Ty`, the
//! declarations it names, its name, and its [`Shape::Base`] — the Rust
//! type an expression of it is built as, its refinements dropped, so
//! `Name` and `Text` are one Rust type and only the checker tells them
//! apart. The derives say the rest by name: `Row` a table's row, `Input` a
//! record or a refinement of one type, `Sum` a sum, `Schema` the tables.
//!
//! An expression is an [`Ex<T>`]. Its methods take `&self` and copy what
//! they keep, so an expression is used as often as it is named. A record's
//! fields are read and given by name through the struct the derive writes
//! for it (`PlaylistEx`, [`Fields`]); a sum is made and matched through its
//! unit struct (`ClockEx::paused(at)`, `ClockEx::match_(&c, |at| …, |at,
//! since| …)`).
//!
//! **Binders are closures** — `let_(v, |x| …)`, [`block`]'s `let_`,
//! `xs.filter(|x| …)`, a match arm — and a variable is its level,
//! `Var(MARK + level)`, until the function is built and
//! [`crate::text::close`] turns levels into De Bruijn indices; an arm is
//! built as a `Lambda` of its fields. So a body written as the attribute
//! macros' Rust was is their IR, byte for byte.
//!
//! A function is a `Def0`…`Def5` in a `static`: its binding's name, its
//! kind and its body, a plain `fn` of its parameters typed by their
//! declared types. It is built once, in a context gathering the
//! declarations it names — its parameters', its result's, its literals',
//! its fresh ids', its callees' — which are its `types` (§1.5); calling it
//! names it by hash. [`text`] embeds Ark text where Rust would be awkward.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::ops;
use std::sync::Arc;

pub use ark_macros::{Input, Node, Row, Schema, Sum};

pub use crate::ir::Node;

use crate::check::Types;
use crate::codec::{bytes_enc, int_enc, Reader};
use crate::hash::FnHash;
use crate::ir::{CtxField, Decl, Effect, Expr, Function, Op, Refine, Rule, Std, Un};
use crate::module::{Lib, Module, SPEC};
use crate::peer::Cas;
use crate::schema::{Schema, Space};
use crate::store::Dir;
use crate::text::{close, MARK};
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

/// A table's row struct (`#[derive(Row)]`).
pub trait Row: Shape {
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
        let len = Refine {
            rule: Rule::Len(16),
            why: None,
        };
        d.insert(
            Self::name(),
            Decl {
                ty: Ty::Bytes,
                refine: vec![len],
                names: vec![],
            },
        );
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
        d.insert(
            Self::name(),
            Decl {
                ty: Ty::option(T::ty()),
                refine: vec![],
                names: vec![],
            },
        );
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

macro_rules! shapes {
    ($(($($t:ident),*)),*) => {$(
        impl<$($t: Shape),*> Shape for ($($t,)*) {
            type Base = ($($t::Base,)*);
            fn ty() -> Ty { Ty::Tuple(vec![$($t::ty()),*]) }
            fn decls(d: &mut Decls) { $($t::decls(d);)* }
            fn name() -> String { format!("({})", [$($t::name()),*].join(", ")) }
        }
    )*};
}
shapes!((A, B), (A, B, C), (A, B, C, D));

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

/// A declaration of a record's fields or a sum's variants, by their
/// names ([`Decl::names`]), or of a refinement of one type.
pub fn decl(name: &str, ty: Ty, refine: Vec<(Rule, Option<&str>)>, names: &[&str], d: &mut Decls) {
    let refine = refine
        .into_iter()
        .map(|(rule, why)| Refine {
            rule,
            why: why.map(Into::into),
        })
        .collect();
    let names = names.iter().map(|n| n.to_string()).collect();
    d.insert(name.into(), Decl { ty, refine, names });
}

/// A refinement of an `Int`: from `lo` to `hi`, both included.
pub fn within(lo: i64, hi: i64) -> Rule {
    let (Rule::Refine(a), Rule::Refine(b)) = (bound(Op::Ge, lo), bound(Op::Le, hi)) else {
        unreachable!()
    };
    Rule::Refine(Expr::Op(Op::And, Box::new(a), Box::new(b)))
}

/// A refinement of an `Int`: the value `o` the bound.
pub fn bound(o: Op, n: i64) -> Rule {
    Rule::Refine(Expr::Op(o, Box::new(Expr::Var(0)), Box::new(int(n).e)))
}

// * Expressions

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

    fn b(&self) -> Box<Expr> {
        Box::new(self.e.clone())
    }
}

impl<T: Shape> Ex<T> {
    /// This expression as a statement: `()` as it is, a value dropped.
    pub fn s(&self) -> Ex<()> {
        match T::ty() == Ty::unit() {
            true => Ex::of(self.e.clone()),
            false => Ex::of(Expr::Seq(vec![self.e.clone(), Expr::Seq(vec![])])),
        }
    }
}

impl<T> From<&Ex<T>> for Ex<T> {
    fn from(x: &Ex<T>) -> Ex<T> {
        x.clone()
    }
}
impl From<i64> for Ex<i64> {
    fn from(n: i64) -> Ex<i64> {
        int(n)
    }
}
impl From<&str> for Ex<String> {
    fn from(s: &str) -> Ex<String> {
        txt(s)
    }
}
impl From<bool> for Ex<bool> {
    fn from(b: bool) -> Ex<bool> {
        Ex::of(Expr::Lit(Ty::bool(), vec![b as u8]))
    }
}

/// A record, its fields read and given by name: `PlaylistEx`.
pub trait Fields: Shape {
    type View;
    fn view(e: Expr) -> Self::View;
}

impl<T: Fields> Ex<T> {
    /// The fields, each an expression.
    pub fn f(&self) -> T::View {
        T::view(self.e.clone())
    }
}

macro_rules! tuples {
    ($(($($t:ident $i:tt),*)),*) => {$(
        impl<$($t: Shape),*> Fields for ($($t,)*) {
            type View = ($(Ex<$t>,)*);
            fn view(e: Expr) -> Self::View { ($(Ex::of(Expr::Field(Box::new(e.clone()), $i)),)*) }
        }
        impl<$($t: Shape),*> From<($(Ex<$t>,)*)> for Ex<($($t,)*)> {
            fn from(x: ($(Ex<$t>,)*)) -> Self { Ex::of(Expr::Tuple(vec![$(x.$i.e),*])) }
        }
    )*};
}
tuples!((A 0, B 1), (A 0, B 1, C 2), (A 0, B 1, C 2, D 3));

// * The context a function is built in

struct Cx {
    decls: Decls,
    calls: Vec<&'static Built>,
    depth: u32,
}

thread_local! {
    static CX: RefCell<Vec<Cx>> = const { RefCell::new(vec![]) };
    static SCHEMA: RefCell<Option<Schema>> = const { RefCell::new(None) };
}

fn with<X>(f: impl FnOnce(&mut Cx) -> X) -> X {
    CX.with(|c| f(c.borrow_mut().last_mut().expect("an expression is built inside a function's build")))
}

/// `T`'s declarations, in the function being built.
pub fn note<T: Shape>() {
    with(|cx| T::decls(&mut cx.decls))
}

/// `n` variables at the next levels, for `f` to build a body with.
fn bind<R>(n: u32, f: impl FnOnce(Vec<Expr>) -> Ex<R>) -> Expr {
    let d = with(|cx| (cx.depth, cx.depth += n).0);
    let body = f((d..d + n).map(|l| Expr::Var(MARK + l)).collect()).e;
    with(|cx| cx.depth -= n);
    body
}

fn var<T>(v: &[Expr], i: usize) -> Ex<T> {
    Ex::of(v[i].clone())
}

fn lam<R>(n: u32, f: impl FnOnce(Vec<Expr>) -> Ex<R>) -> Box<Expr> {
    Box::new(Expr::Lambda(n, Box::new(bind(n, f))))
}

/// A match arm binding `n` fields: built as a `Lambda`, closed to its body.
pub fn arm<R>(n: u32, f: impl FnOnce(Vec<Expr>) -> Ex<R>) -> Expr {
    *lam(n, f)
}

/// A match: one arm per variant, in order ([`arm`]).
pub fn match_<S, R>(s: &Ex<S>, arms: Vec<Expr>) -> Ex<R> {
    Ex::of(Expr::Match(s.b(), arms))
}

/// Variant `i` of `S`, its fields in order.
pub fn variant<S: Shape>(i: u8, fields: Vec<Expr>) -> Ex<S> {
    note::<S>();
    Ex::of(Expr::Variant(S::ty(), i, fields))
}

/// The field `i` of `e`, for a record's view.
pub fn field<T>(e: &Expr, i: u32) -> Ex<T> {
    Ex::of(Expr::Field(Box::new(e.clone()), i))
}

// * Functions

/// What a function is: an entry point, a query, a scope, a live
/// transition (a mutator over volatile tables, §1.9), or a function the
/// others call at the least effect its body has.
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

/// A function in a `static`, built on first use.
pub trait Fun: Sync {
    fn built(&'static self) -> &'static Built;
}

fn build<R: Shape>(name: &'static str, kind: Kind, params: Vec<Ty>, body: impl FnOnce(Vec<Expr>) -> Ex<R::Base>) -> Built {
    thread_local!(static BUILDING: RefCell<Vec<&'static str>> = const { RefCell::new(vec![]) });
    BUILDING.with(|s| {
        assert!(
            !s.borrow().contains(&name),
            "{name} calls itself, which a function named by its hash cannot"
        );
        s.borrow_mut().push(name)
    });
    let n = params.len() as u32;
    CX.with(|c| {
        c.borrow_mut().push(Cx {
            decls: Decls::new(),
            calls: vec![],
            depth: n,
        })
    });
    note::<R>();
    let body = body((0..n).map(|l| Expr::Var(MARK + l)).collect());
    let cx = CX.with(|c| c.borrow_mut().pop().unwrap());
    BUILDING.with(|s| s.borrow_mut().pop());
    let body = close(body.e, n);
    let effect = match kind {
        Kind::Mutator | Kind::Live => Effect::Write,
        Kind::Query => Effect::Read,
        Kind::Scope => Effect::Pure,
        Kind::Helper => effect(&body, &cx.calls),
    };
    let types = cx.decls.into_iter().filter(|(n, _)| n != "Text").collect();
    let f = Function {
        types,
        params,
        ret: R::ty(),
        effect,
        body,
    };
    Built {
        name,
        kind,
        hash: f.hash(),
        f,
        calls: cx.calls,
    }
}

/// The least effect `e` has: a read, a write, a fresh id, or a callee's.
fn effect(e: &Expr, calls: &[&Built]) -> Effect {
    let mut out = Effect::Pure;
    crate::interp::walk(e, None, &mut |x, _| {
        out = out.max(match x {
            Expr::Scan(_) | Expr::Lookup(..) => Effect::Read,
            Expr::Insert(..) | Expr::Update(..) | Expr::Delete(..) | Expr::Ctx(CtxField::NewId(_)) => Effect::Write,
            Expr::Call(h, _) => calls.iter().find(|c| c.hash == *h).map_or(Effect::Pure, |c| c.f.effect),
            _ => Effect::Pure,
        })
    });
    out
}

/// Built functions, by the address of their `static`.
static BUILT: std::sync::Mutex<BTreeMap<usize, &'static Built>> = std::sync::Mutex::new(BTreeMap::new());

fn cached(key: usize, f: impl FnOnce() -> Built) -> &'static Built {
    if let Some(b) = BUILT.lock().unwrap().get(&key) {
        return b;
    }
    let b = f();
    BUILT.lock().unwrap().entry(key).or_insert_with(|| Box::leak(Box::new(b)))
}

macro_rules! defs {
    ($($def:ident($($a:ident $x:ident $i:tt),*);)*) => {$(
        /// A function of this many parameters, typed by their declared
        /// types (`Name`, not `Text`), in a `static`: its binding's name,
        /// its kind and its body.
        pub struct $def<$($a: Shape,)* R: Shape> {
            pub name: &'static str,
            pub kind: Kind,
            pub body: fn($(Ex<$a::Base>),*) -> Ex<R::Base>,
        }
        impl<$($a: Shape,)* R: Shape> $def<$($a,)* R> {
            /// A call of it: its declarations become the caller's.
            #[allow(clippy::too_many_arguments)]
            pub fn call(&'static self $(, $x: impl Into<Ex<$a::Base>>)*) -> Ex<R::Base> {
                let f = self.built();
                with(|cx| {
                    cx.decls.extend(f.f.types.iter().cloned());
                    cx.calls.push(f);
                });
                Ex::of(Expr::Call(f.hash, vec![$($x.into().e),*]))
            }
        }
        impl<$($a: Shape,)* R: Shape> Fun for $def<$($a,)* R> {
            fn built(&'static self) -> &'static Built {
                cached(self as *const Self as usize, || build::<R>(self.name, self.kind, vec![$($a::ty()),*], |_v| {
                    $(note::<$a>();)*
                    (self.body)($(var(&_v, $i)),*)
                }))
            }
        }
    )*};
}
defs! {
    Def0();
    Def1(A a 0);
    Def2(A a 0, B b 1);
    Def3(A a 0, B b 1, C c 2);
    Def4(A a 0, B b 1, C c 2, D d 3);
    Def5(A a 0, B b 1, C c 2, D d 3, E e 4);
}

// * Values

pub fn int(n: i64) -> Ex<i64> {
    Ex::of(Expr::Lit(Ty::Int, int_enc(n).to_vec()))
}

pub fn bool(b: bool) -> Ex<bool> {
    b.into()
}

/// A text literal.
pub fn txt(s: &str) -> Ex<String> {
    Ex::of(Expr::Lit(Types::text(), bytes_enc(s.as_bytes())))
}

/// `()`.
pub fn unit() -> Ex<()> {
    Ex::of(Expr::Seq(vec![]))
}

pub fn some<T: Shape>(x: impl Into<Ex<T>>) -> Ex<Option<T>> {
    variant(1, vec![x.into().e])
}

pub fn none<T: Shape>() -> Ex<Option<T>> {
    variant(0, vec![])
}

pub fn list<T: Shape>(xs: Vec<Ex<T>>) -> Ex<Vec<T>> {
    note::<T>();
    Ex::of(Expr::List(T::ty(), xs.into_iter().map(|x| x.e).collect()))
}

pub fn tup<T>(t: T) -> Ex<T::Out>
where
    T: Tup,
{
    t.tup()
}

/// A tuple of expressions as an expression of the tuple.
pub trait Tup {
    type Out;
    fn tup(self) -> Ex<Self::Out>;
}
impl<A: Shape, B: Shape> Tup for (Ex<A>, Ex<B>) {
    type Out = (A, B);
    fn tup(self) -> Ex<(A, B)> {
        self.into()
    }
}
impl<A: Shape, B: Shape, C: Shape> Tup for (Ex<A>, Ex<B>, Ex<C>) {
    type Out = (A, B, C);
    fn tup(self) -> Ex<(A, B, C)> {
        self.into()
    }
}

/// `if c { a } else { b }`; with no `else`, `b` is [`unit`].
pub fn if_<T>(c: impl Into<Ex<bool>>, a: Ex<T>, b: Ex<T>) -> Ex<T> {
    Ex::of(Expr::If(c.into().b(), a.b(), b.b()))
}

/// `let x = v` in `body`.
pub fn let_<A, B>(v: impl Into<Ex<A>>, body: impl FnOnce(Ex<A>) -> Ex<B>) -> Ex<B> {
    let v = v.into();
    Ex::of(Expr::Let(v.b(), Box::new(bind(1, |x| body(var(&x, 0))))))
}

/// Statements, then the last, whose value the block's is.
pub fn seq<T>(first: impl IntoIterator<Item = Ex<()>>, last: Ex<T>) -> Ex<T> {
    let first: Vec<Expr> = first.into_iter().map(|s| s.e).collect();
    if first.is_empty() {
        return last;
    }
    Ex::of(Expr::Seq(first.into_iter().chain([last.e]).collect()))
}

/// A block of statements: `let`s, statements whose values are dropped,
/// and the value of the last, as a body in Rust reads.
pub struct Block {
    items: Vec<(bool, Expr)>,
    lets: u32,
}

impl Block {
    /// `let x = v` for the rest of the block: `x`.
    pub fn let_<A>(&mut self, v: impl Into<Ex<A>>) -> Ex<A> {
        let v = v.into();
        let d = with(|cx| (cx.depth, cx.depth += 1).0);
        self.items.push((true, v.e));
        self.lets += 1;
        Ex::of(Expr::Var(MARK + d))
    }

    /// A statement: its value dropped ([`Ex::s`]).
    pub fn s<T: Shape>(&mut self, x: impl Into<Ex<T>>) {
        self.items.push((false, x.into().s().e));
    }
}

/// `{ let a = …; s; …; last }`: what `f` says in `b`, then its result.
pub fn block<T>(f: impl FnOnce(&mut Block) -> Ex<T>) -> Ex<T> {
    let mut b = Block { items: vec![], lets: 0 };
    let mut out = f(&mut b).e;
    with(|cx| cx.depth -= b.lets);
    let seq = |mut first: Vec<Expr>, last: Expr| match first.is_empty() {
        true => last,
        false => {
            first.reverse();
            first.push(last);
            Expr::Seq(first)
        }
    };
    let mut after = vec![];
    for (is_let, e) in b.items.into_iter().rev() {
        match is_let {
            true => out = Expr::Let(Box::new(e), Box::new(seq(std::mem::take(&mut after), out))),
            false => after.push(e),
        }
    }
    Ex::of(seq(after, out))
}

/// A refusal, of whatever type its place wants.
pub fn refuse<T>(why: impl Into<Ex<String>>) -> Ex<T> {
    Ex::of(Expr::Refuse(why.into().b()))
}

/// [`refuse`] as a statement.
pub fn refuse_(why: impl Into<Ex<String>>) -> Ex<()> {
    Ex::of(Expr::Refuse(why.into().b()))
}

/// `concat` of texts.
pub fn concat(xs: Vec<Ex<String>>) -> Ex<String> {
    concat_of(&list(xs))
}

/// `concat` of a list of texts.
pub fn concat_of(xs: &Ex<Vec<String>>) -> Ex<String> {
    Ex::of(Expr::Std(Std::Concat, vec![xs.e.clone()]))
}

pub fn nil_id<R>() -> Ex<Id<R>> {
    Ex::of(Expr::Std(Std::NilId, vec![]))
}

impl<T> Ex<T> {
    fn op<U>(&self, o: Op, b: Expr) -> Ex<U> {
        Ex::of(Expr::Op(o, self.b(), Box::new(b)))
    }
}

/// Comparisons, by the values' bytes.
#[allow(clippy::should_implement_trait)]
impl<T> Ex<T> {
    pub fn eq(&self, b: impl Into<Ex<T>>) -> Ex<bool> {
        self.op(Op::Eq, b.into().e)
    }
    pub fn ne(&self, b: impl Into<Ex<T>>) -> Ex<bool> {
        self.op(Op::Ne, b.into().e)
    }
    pub fn lt(&self, b: impl Into<Ex<T>>) -> Ex<bool> {
        self.op(Op::Lt, b.into().e)
    }
    pub fn le(&self, b: impl Into<Ex<T>>) -> Ex<bool> {
        self.op(Op::Le, b.into().e)
    }
    pub fn gt(&self, b: impl Into<Ex<T>>) -> Ex<bool> {
        self.op(Op::Gt, b.into().e)
    }
    pub fn ge(&self, b: impl Into<Ex<T>>) -> Ex<bool> {
        self.op(Op::Ge, b.into().e)
    }
}

impl Ex<bool> {
    pub fn and(&self, b: impl Into<Ex<bool>>) -> Ex<bool> {
        self.op(Op::And, b.into().e)
    }
    pub fn or(&self, b: impl Into<Ex<bool>>) -> Ex<bool> {
        self.op(Op::Or, b.into().e)
    }
}

macro_rules! arith {
    ($($tr:ident $f:ident $op:ident),*) => {$(
        arith!(@ $tr $f $op, Ex<i64>, Ex<i64>; Ex<i64>, &Ex<i64>; Ex<i64>, i64; &Ex<i64>, Ex<i64>; &Ex<i64>, &Ex<i64>; &Ex<i64>, i64);
    )*};
    (@ $tr:ident $f:ident $op:ident, $($a:ty, $b:ty);*) => {$(
        impl ops::$tr<$b> for $a {
            type Output = Ex<i64>;
            fn $f(self, b: $b) -> Ex<i64> { self.op(Op::$op, Ex::<i64>::from(b).e) }
        }
    )*};
}
arith!(Add add Add, Sub sub Sub, Mul mul Mul, Div div Div, Rem rem Mod);

impl ops::Not for Ex<bool> {
    type Output = Ex<bool>;
    fn not(self) -> Ex<bool> {
        Ex::of(Expr::Un(Un::Not, self.b()))
    }
}
impl ops::Not for &Ex<bool> {
    type Output = Ex<bool>;
    fn not(self) -> Ex<bool> {
        Ex::of(Expr::Un(Un::Not, self.b()))
    }
}
impl ops::Neg for Ex<i64> {
    type Output = Ex<i64>;
    fn neg(self) -> Ex<i64> {
        Ex::of(Expr::Un(Un::Neg, self.b()))
    }
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
    pub fn has_role(self, r: impl Into<Ex<String>>) -> Ex<bool> {
        self.roles().contains(r)
    }
}

// * Tables

/// A table's key: the tuple of its key columns' values, one or several.
pub trait Key {
    fn key(self) -> Expr;
}
impl<T> Key for Ex<T> {
    fn key(self) -> Expr {
        Expr::Tuple(vec![self.e])
    }
}
impl<T> Key for &Ex<T> {
    fn key(self) -> Expr {
        Expr::Tuple(vec![self.e.clone()])
    }
}
impl<A, B> Key for (Ex<A>, Ex<B>) {
    fn key(self) -> Expr {
        Expr::Tuple(vec![self.0.e, self.1.e])
    }
}
impl<A, B, C> Key for (Ex<A>, Ex<B>, Ex<C>) {
    fn key(self) -> Expr {
        Expr::Tuple(vec![self.0.e, self.1.e, self.2.e])
    }
}

/// A table's rows, and its writes.
pub struct Table<R>(PhantomData<fn() -> R>);

impl<R> Default for Table<R> {
    fn default() -> Self {
        Table(PhantomData)
    }
}
impl<R> Clone for Table<R> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<R> Copy for Table<R> {}

impl<R: Row> Table<R> {
    pub fn all(&self) -> Ex<Vec<R>> {
        Ex::of(Expr::Scan(R::TABLE.into()))
    }
    /// The row under a key.
    pub fn get(&self, key: impl Key) -> Ex<Option<R>> {
        Ex::of(Expr::Lookup(R::TABLE.into(), Box::new(key.key())))
    }
    pub fn insert(&self, row: impl Into<Ex<R>>) -> Ex<()> {
        Ex::of(Expr::Insert(R::TABLE.into(), row.into().b()))
    }
    pub fn update(&self, key: impl Key, f: impl FnOnce(Ex<R>) -> Ex<R>) -> Ex<()> {
        Ex::of(Expr::Update(R::TABLE.into(), Box::new(key.key()), lam(1, |v| f(var(&v, 0)))))
    }
    pub fn delete(&self, key: impl Key) -> Ex<()> {
        Ex::of(Expr::Delete(R::TABLE.into(), Box::new(key.key())))
    }
    pub fn filter(&self, p: impl FnOnce(Ex<R>) -> Ex<bool>) -> Ex<Vec<R>> {
        self.all().filter(p)
    }
    pub fn map<U>(&self, f: impl FnOnce(Ex<R>) -> Ex<U>) -> Ex<Vec<U>> {
        self.all().map(f)
    }
    pub fn sort<K: Keys>(&self, keys: impl FnOnce(Ex<R>) -> K) -> Ex<Vec<R>> {
        self.all().sort(keys)
    }
}

// * The standard library and the relational operators

fn std<T>(f: Std, args: Vec<Expr>) -> Ex<T> {
    Ex::of(Expr::Std(f, args))
}

/// Methods that are a standard function of the receiver and the arguments.
macro_rules! std_methods {
    ($(impl[$($g:tt)*] $recv:ty { $($name:ident($($a:ident: $at:ty),*) -> $r:ty = $f:ident;)* })*) => {$(
        impl<$($g)*> Ex<$recv> {
            $(pub fn $name(&self $(, $a: impl Into<Ex<$at>>)*) -> Ex<$r> {
                std(Std::$f, vec![self.e.clone() $(, $a.into().e)*])
            })*
        }
    )*};
}

std_methods! {
    impl[] String {
        trim() -> String = Trim;
        is_empty() -> bool = IsEmpty;
        lower() -> String = Lower;
        is_alnum() -> bool = IsAlnum;
        chars() -> Vec<String> = Chars;
        len() -> i64 = TextLen;
        starts_with(p: String) -> bool = StartsWith;
        split_once(p: String) -> Option<(String, String)> = SplitOnce;
        fnv1a64() -> i64 = Fnv1a64;
        utf8() -> Bytes = Utf8;
        id_of_text() -> Option<Bytes> = IdOfText;
    }
    impl[] i64 {
        to_text() -> String = TextOfInt;
        min(o: i64) -> i64 = Min;
        max(o: i64) -> i64 = Max;
        clamp(lo: i64, hi: i64) -> i64 = Clamp;
        abs() -> i64 = Abs;
    }
    impl[] Bytes {
        hex() -> String = Hex;
        sha256() -> Bytes = Sha256;
    }
    impl[R] Id<R> {
        text_of_id() -> String = TextOfId;
        hex() -> String = Hex;
    }
    impl[T: Shape] Vec<T> {
        first() -> Option<T> = First;
        last() -> Option<T> = Last;
        len() -> i64 = Len;
        contains(x: T) -> bool = Contains;
        reverse() -> Vec<T> = Reverse;
        concat(o: Vec<T>) -> Vec<T> = ListConcat;
        insert(i: i64, x: T) -> Vec<T> = Insert;
        remove_at(i: i64) -> Vec<T> = RemoveAt;
        move_at(from: i64, to: i64) -> Vec<T> = Move;
        slice(i: i64, j: i64) -> Vec<T> = Slice;
    }
    impl[] Vec<i64> {
        sum() -> i64 = Sum;
    }
    impl[T: Shape] Option<T> {
        is_some() -> bool = IsSome;
        unwrap() -> T = Unwrap;
        unwrap_or(d: T) -> T = UnwrapOr;
    }
}

/// A sort key and its direction.
pub struct Dirn(Expr, Dir);

pub fn desc<T>(k: impl Into<Ex<T>>) -> Dirn {
    Dirn(k.into().e, Dir::Desc)
}

pub fn asc<T>(k: impl Into<Ex<T>>) -> Dirn {
    Dirn(k.into().e, Dir::Asc)
}

/// A sort's keys: one, ascending or [`desc`]; or a tuple of them.
pub trait Keys {
    fn keys(self) -> (Expr, Vec<Dir>);
}
/// One sort key.
pub trait OneKey {
    fn one(self) -> (Expr, Dir);
}
impl OneKey for Dirn {
    fn one(self) -> (Expr, Dir) {
        (self.0, self.1)
    }
}
impl<T> OneKey for Ex<T> {
    fn one(self) -> (Expr, Dir) {
        (self.e, Dir::Asc)
    }
}
impl<K: OneKey> Keys for K {
    fn keys(self) -> (Expr, Vec<Dir>) {
        let (e, d) = self.one();
        (e, vec![d])
    }
}
macro_rules! keys {
    ($(($($k:ident $i:tt),*)),*) => {$(
        impl<$($k: OneKey),*> Keys for ($($k,)*) {
            fn keys(self) -> (Expr, Vec<Dir>) {
                let ks = [$(self.$i.one()),*];
                (Expr::Tuple(ks.iter().map(|k| k.0.clone()).collect()), ks.iter().map(|k| k.1).collect())
            }
        }
    )*};
}
keys!((A 0, B 1), (A 0, B 1, C 2), (A 0, B 1, C 2, D 3));

impl<T: Shape> Ex<Vec<T>> {
    /// `len() == 0`.
    pub fn is_empty(&self) -> Ex<bool> {
        self.len().eq(int(0))
    }
    pub fn take(&self, n: impl Into<Ex<i64>>) -> Ex<Vec<T>> {
        Ex::of(Expr::Take(self.b(), n.into().b()))
    }
    pub fn filter(&self, p: impl FnOnce(Ex<T>) -> Ex<bool>) -> Ex<Vec<T>> {
        Ex::of(Expr::Filter(self.b(), lam(1, |v| p(var(&v, 0)))))
    }
    pub fn map<U>(&self, f: impl FnOnce(Ex<T>) -> Ex<U>) -> Ex<Vec<U>> {
        Ex::of(Expr::Map(self.b(), lam(1, |v| f(var(&v, 0)))))
    }
    /// The pairs the condition admits, in this list's order.
    pub fn join<U>(&self, o: &Ex<Vec<U>>, on: impl FnOnce(Ex<T>, Ex<U>) -> Ex<bool>) -> Ex<Vec<(T, U)>> {
        Ex::of(Expr::Join(self.b(), o.b(), lam(2, |v| on(var(&v, 0), var(&v, 1)))))
    }
    /// `(key, aggregate)` per key, in key order.
    pub fn group<K, A>(&self, by: impl FnOnce(Ex<T>) -> Ex<K>, agg: impl FnOnce(Ex<K>, Ex<Vec<T>>) -> Ex<A>) -> Ex<Vec<(K, A)>> {
        let by = lam(1, |v| by(var(&v, 0)));
        Ex::of(Expr::Group(self.b(), by, lam(2, |v| agg(var(&v, 0), var(&v, 1)))))
    }
    /// Stable, by the keys, each in its direction.
    pub fn sort<K: Keys>(&self, keys: impl FnOnce(Ex<T>) -> K) -> Ex<Vec<T>> {
        let mut dirs = vec![];
        let l = lam(1, |v| {
            let (e, d) = keys(var(&v, 0)).keys();
            dirs = d;
            Ex::<()>::of(e)
        });
        Ex::of(Expr::Sort(self.b(), l, dirs))
    }
}

impl<T: Shape> Ex<Option<T>> {
    pub fn is_none(&self) -> Ex<bool> {
        !self.is_some()
    }
    /// `match self { None => d, Some(x) => f(x) }`.
    pub fn map_or<U>(&self, d: impl Into<Ex<U>>, f: impl FnOnce(Ex<T>) -> Ex<U>) -> Ex<U> {
        let some = arm(1, |v| f(var(&v, 0)));
        Ex::of(Expr::Match(self.b(), vec![Expr::Lambda(0, d.into().b()), some]))
    }
    pub fn map<U: Shape>(&self, f: impl FnOnce(Ex<T>) -> Ex<U>) -> Ex<Option<U>> {
        self.map_or(none::<U>(), |x| some(f(x)))
    }
    /// The value when the predicate holds of it.
    pub fn filter(&self, p: impl FnOnce(Ex<T>) -> Ex<bool>) -> Ex<Option<T>> {
        self.map_or(none(), |x| if_(p(x.clone()), some(x), none()))
    }
}

// * Text inside a builder

/// A value usable in a [`text`] fragment: its expression and type.
pub trait Var {
    fn var(&self) -> (Expr, Ty);
}
impl<T: Shape> Var for Ex<T> {
    fn var(&self) -> (Expr, Ty) {
        (self.e.clone(), T::ty())
    }
}

/// An Ark text expression (§1.15) inside a builder: `vars` its free
/// names, the tables those of the module being assembled. Type-checked
/// here, as `T`; refused with the text's words otherwise.
pub fn text<T: Shape>(src: &str, vars: &[(&str, &dyn Var)]) -> Ex<T> {
    let schema = SCHEMA.with(|s| s.borrow().clone()).unwrap_or_default();
    let decls: Vec<_> = with(|cx| cx.decls.clone().into_iter().collect());
    let vs: Vec<(&str, Expr, Ty)> = vars
        .iter()
        .map(|(n, v)| {
            let (e, t) = v.var();
            (*n, e, t)
        })
        .collect();
    let depth = with(|cx| cx.depth);
    let (e, t, more) = crate::text::fragment(src, &schema, &decls, &vs, depth).unwrap_or_else(|r| panic!("text({src:?}): {r}"));
    with(|cx| cx.decls.extend(more));
    let types = Types::new(&with(|cx| cx.decls.clone().into_iter().collect::<Vec<_>>()));
    assert!(
        types.sub(&t, &T::ty()) || types.sub(&T::ty(), &t),
        "text({src:?}) is {t:?}, not {:?}",
        T::ty()
    );
    Ex::of(e)
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
    pub fn run(
        &self,
        store: &dyn crate::store::Read,
        ctx: &crate::peer::Ctx,
        name: &str,
        input: &[u8],
    ) -> Result<Vec<crate::store::Change>, Refusal> {
        self.lib.procedure(&self.get(name)).expect("an installed function").run(store, ctx, input)
    }
}

/// §1.5 The module of a schema and functions: each checked, its callees
/// first, every one bound by its name; the types every one declares. A
/// live function touching a durable table is refused.
pub fn assemble(schema: Schema, fns: &[&'static dyn Fun]) -> Result<Domain, Refusal> {
    SCHEMA.with(|s| *s.borrow_mut() = Some(schema.clone()));
    let mut all: BTreeMap<&str, &'static Built> = BTreeMap::new();
    let mut todo: Vec<&'static Built> = fns.iter().map(|f| f.built()).collect();
    while let Some(b) = todo.pop() {
        if let Some(o) = all.insert(b.name, b) {
            if o.hash != b.hash {
                return Err(Refusal(format!("{} is bound twice", b.name)));
            }
        }
        todo.extend(b.calls.iter().copied());
    }
    let lib = Lib::default();
    let mut bodies = BTreeMap::new();
    fn install(lib: &Lib, schema: &Schema, bodies: &mut BTreeMap<FnHash, Vec<u8>>, b: &Built) -> Result<(), Refusal> {
        for c in &b.calls {
            install(lib, schema, bodies, c)?;
        }
        if b.kind == Kind::Live {
            let mut touched = vec![];
            let mut fns = vec![b];
            while let Some(f) = fns.pop() {
                crate::interp::walk(&f.f.body, None, &mut |e, _| {
                    if let Expr::Scan(t) | Expr::Lookup(t, _) | Expr::Insert(t, _) | Expr::Update(t, ..) | Expr::Delete(t, _) = e {
                        touched.push(t.clone())
                    }
                });
                fns.extend(f.calls.iter().copied());
            }
            if let Some(t) = touched.iter().find(|t| schema.table(t).is_some_and(|(_, t)| t.space != Space::Volatile)) {
                return Err(Refusal(format!("{}: a live transition touches {t}, which is durable", b.name)));
            }
        }
        bodies.insert(b.hash, b.f.to_bytes());
        lib.install(schema, b.f.clone())
            .map(|_| ())
            .map_err(|r| Refusal(format!("{}: {r}", b.name)))
    }
    let mut types = BTreeMap::new();
    for b in all.values() {
        install(&lib, &schema, &mut bodies, b)?;
        types.extend(b.f.types.iter().cloned());
    }
    let bindings = all.iter().map(|(n, b)| (n.to_string(), b.hash)).collect();
    let module = Module {
        spec: SPEC,
        schema: schema.clone(),
        types: types.into_iter().collect(),
        bindings,
    };
    Ok(Domain {
        schema: Arc::new(schema),
        module,
        lib,
        bodies,
    })
}
