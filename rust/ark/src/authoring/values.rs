//! §2.1–2.2 The values of the vocabulary and the operations on them. Each
//! is one [`H`]; under `Emit` it stands for an expression, under `Native`
//! for a value, and no method says which.

use std::marker::PhantomData;

use crate::eval::{EvalError, EvalFault};
use crate::ir::{Auto, CmpOp, Expr, Op, StdFn};
use crate::schema::Ty;
use crate::value::Value;

use super::cx::{self, H};
use super::schema::Row;

/// A type of the vocabulary: what it is in the IR, and how it is carried.
/// Every value type, and every row (through [`Row`]).
pub trait Data: Sized + 'static {
    /// Its type in the IR.
    fn ty() -> Ty;
    #[doc(hidden)]
    fn from_h(h: H) -> Self;
    #[doc(hidden)]
    fn to_h(&self) -> H;
}

macro_rules! scalar {
    ($(#[$doc:meta])* $name:ident, $ty:expr) => {
        $(#[$doc])*
        #[derive(Clone, Copy)]
        #[repr(transparent)]
        pub struct $name(pub(crate) H);

        impl Data for $name {
            fn ty() -> Ty {
                $ty
            }
            fn from_h(h: H) -> Self {
                $name(h)
            }
            fn to_h(&self) -> H {
                self.0
            }
        }

        impl $name {
            /// `ECmp Eq`.
            pub fn eq(self, b: impl Into<$name>) -> Bool {
                Bool(cx::cmp_op(CmpOp::Eq, self.0, b.into().0))
            }
            /// `ECmp Ne`.
            pub fn ne(self, b: impl Into<$name>) -> Bool {
                Bool(cx::cmp_op(CmpOp::Ne, self.0, b.into().0))
            }
            /// `ECmp Lt`, under the one total order of values.
            pub fn lt(self, b: impl Into<$name>) -> Bool {
                Bool(cx::cmp_op(CmpOp::Lt, self.0, b.into().0))
            }
            /// `ECmp Le`.
            pub fn le(self, b: impl Into<$name>) -> Bool {
                Bool(cx::cmp_op(CmpOp::Le, self.0, b.into().0))
            }
            /// `ECmp Gt`.
            pub fn gt(self, b: impl Into<$name>) -> Bool {
                Bool(cx::cmp_op(CmpOp::Gt, self.0, b.into().0))
            }
            /// `ECmp Ge`.
            pub fn ge(self, b: impl Into<$name>) -> Bool {
                Bool(cx::cmp_op(CmpOp::Ge, self.0, b.into().0))
            }
        }
    };
}

scalar!(
    /// `TBool`.
    Bool,
    Ty::Bool
);
scalar!(
    /// `TInt`: a 64-bit integer; arithmetic is checked, and an overflow is
    /// a refusal.
    Int,
    Ty::Int
);
scalar!(
    /// `TText`: Unicode text.
    Text,
    Ty::Text
);
scalar!(
    /// `TBytes`.
    Bytes,
    Ty::Bytes
);

impl From<bool> for Bool {
    fn from(b: bool) -> Bool {
        Bool(cx::lit(Value::Bool(b)))
    }
}

impl From<i64> for Int {
    fn from(n: i64) -> Int {
        Int(cx::lit(Value::Int(n)))
    }
}

impl From<i32> for Int {
    fn from(n: i32) -> Int {
        Int(cx::lit(Value::Int(n as i64)))
    }
}

impl From<&str> for Text {
    fn from(s: &str) -> Text {
        Text(cx::lit(Value::text(s)))
    }
}

impl From<String> for Text {
    fn from(s: String) -> Text {
        Text(cx::lit(Value::from(s)))
    }
}

impl From<&[u8]> for Bytes {
    fn from(b: &[u8]) -> Bytes {
        Bytes(cx::lit(Value::Bytes(b.into())))
    }
}

impl From<Vec<u8>> for Bytes {
    fn from(b: Vec<u8>) -> Bytes {
        Bytes(cx::lit(Value::bytes(b)))
    }
}

fn std1<T: Data>(f: StdFn, a: H) -> T {
    T::from_h(cx::std_op(f, &[a]))
}

fn std2<T: Data>(f: StdFn, a: H, b: H) -> T {
    T::from_h(cx::std_op(f, &[a, b]))
}

impl Bool {
    /// `EOp And`.
    pub fn and(self, b: impl Into<Bool>) -> Bool {
        Bool(cx::arith_op(Op::And, &[self.0, b.into().0]))
    }
    /// `EOp Or`.
    pub fn or(self, b: impl Into<Bool>) -> Bool {
        Bool(cx::arith_op(Op::Or, &[self.0, b.into().0]))
    }
    /// `EOp Not`.
    #[allow(clippy::should_implement_trait)]
    pub fn not(self) -> Bool {
        Bool(cx::arith_op(Op::Not, &[self.0]))
    }
}

impl Int {
    /// `EOp Add`: checked.
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, b: impl Into<Int>) -> Int {
        Int(cx::arith_op(Op::Add, &[self.0, b.into().0]))
    }
    /// `EOp Sub`.
    #[allow(clippy::should_implement_trait)]
    pub fn sub(self, b: impl Into<Int>) -> Int {
        Int(cx::arith_op(Op::Sub, &[self.0, b.into().0]))
    }
    /// `EOp Mul`.
    #[allow(clippy::should_implement_trait)]
    pub fn mul(self, b: impl Into<Int>) -> Int {
        Int(cx::arith_op(Op::Mul, &[self.0, b.into().0]))
    }
    /// `EOp Div`: truncating; by zero is a refusal.
    #[allow(clippy::should_implement_trait)]
    pub fn div(self, b: impl Into<Int>) -> Int {
        Int(cx::arith_op(Op::Div, &[self.0, b.into().0]))
    }
    /// `EOp Mod`: the dividend's sign.
    #[allow(clippy::should_implement_trait)]
    pub fn rem(self, b: impl Into<Int>) -> Int {
        Int(cx::arith_op(Op::Mod, &[self.0, b.into().0]))
    }
    /// `EOp Neg`.
    #[allow(clippy::should_implement_trait)]
    pub fn neg(self) -> Int {
        Int(cx::arith_op(Op::Neg, &[self.0]))
    }
    /// `EStd Min`.
    pub fn min(self, b: impl Into<Int>) -> Int {
        std2(StdFn::Min, self.0, b.into().0)
    }
    /// `EStd Max`.
    pub fn max(self, b: impl Into<Int>) -> Int {
        std2(StdFn::Max, self.0, b.into().0)
    }
    /// `EStd Clamp`.
    pub fn clamp(self, lo: impl Into<Int>, hi: impl Into<Int>) -> Int {
        Int(cx::std_op(StdFn::Clamp, &[self.0, lo.into().0, hi.into().0]))
    }
    /// `EStd Abs`.
    pub fn abs(self) -> Int {
        std1(StdFn::Abs, self.0)
    }
    /// `EStd TextOfInt`.
    pub fn to_text(self) -> Text {
        std1(StdFn::TextOfInt, self.0)
    }
}

impl Text {
    /// `EStd Trim`.
    pub fn trim(self) -> Text {
        std1(StdFn::Trim, self.0)
    }
    /// `EStd IsEmpty`.
    pub fn is_empty(self) -> Bool {
        std1(StdFn::IsEmpty, self.0)
    }
    /// `EStd Lower`.
    pub fn lower(self) -> Text {
        std1(StdFn::Lower, self.0)
    }
    /// `EStd TextLen`: in code points.
    pub fn len(self) -> Int {
        std1(StdFn::TextLen, self.0)
    }
    /// `EStd StartsWith`.
    pub fn starts_with(self, p: impl Into<Text>) -> Bool {
        std2(StdFn::StartsWith, self.0, p.into().0)
    }
    /// `EStd Chars`.
    pub fn chars(self) -> List<Text> {
        std1(StdFn::Chars, self.0)
    }
    /// `EStd IsAlnum`.
    pub fn is_alnum(self) -> Bool {
        std1(StdFn::IsAlnum, self.0)
    }
    /// `EStd Utf8`.
    pub fn utf8(self) -> Bytes {
        std1(StdFn::Utf8, self.0)
    }
    /// `EStd Fnv1a64`.
    pub fn fnv1a64(self) -> Int {
        std1(StdFn::Fnv1a64, self.0)
    }
}

impl Bytes {
    /// `EStd Hex`.
    pub fn hex(self) -> Text {
        std1(StdFn::Hex, self.0)
    }
    /// `EStd Sha256`.
    pub fn sha256(self) -> Bytes {
        std1(StdFn::Sha256, self.0)
    }
}

/// `EStd Concat` of a list of texts.
pub fn concat(xs: List<Text>) -> Text {
    std1(StdFn::Concat, xs.0)
}

// Ids ---------------------------------------------------------------------

/// `TId t`: the id of a row of `T`.
#[repr(transparent)]
pub struct Id<T>(pub(crate) H, PhantomData<fn() -> T>);

impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Id<T> {}

impl<T: Row> Data for Id<T> {
    fn ty() -> Ty {
        Ty::Id(T::NAME.into())
    }
    fn from_h(h: H) -> Self {
        Id(h, PhantomData)
    }
    fn to_h(&self) -> H {
        self.0
    }
}

impl<T: Row> Id<T> {
    /// `ECmp Eq`.
    pub fn eq(self, b: impl Into<Id<T>>) -> Bool {
        Bool(cx::cmp_op(CmpOp::Eq, self.0, b.into().0))
    }
    /// `ECmp Ne`.
    pub fn ne(self, b: impl Into<Id<T>>) -> Bool {
        Bool(cx::cmp_op(CmpOp::Ne, self.0, b.into().0))
    }
    /// `EStd TextOfId`.
    pub fn to_text(self) -> Text {
        std1(StdFn::TextOfId, self.0)
    }
}

/// `EStd IdOfText`: `None` if the text is not an id.
pub fn id_of_text<T: Row>(t: impl Into<Text>) -> Opt<Id<T>> {
    std1(StdFn::IdOfText, t.into().0)
}

/// `EStd NilId`.
pub fn nil_id<T: Row>() -> Id<T> {
    Id::from_h(cx::std_op(StdFn::NilId, &[]))
}

// Options -----------------------------------------------------------------

/// `TOption t`.
#[repr(transparent)]
pub struct Opt<T>(pub(crate) H, PhantomData<fn() -> T>);

impl<T> Clone for Opt<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Opt<T> {}

impl<T: Data> Data for Opt<T> {
    fn ty() -> Ty {
        Ty::Option(Box::new(T::ty()))
    }
    fn from_h(h: H) -> Self {
        Opt(h, PhantomData)
    }
    fn to_h(&self) -> H {
        self.0
    }
}

/// `ESome x`.
pub fn some<T: Data>(x: impl Into<T>) -> Opt<T> {
    let h = x.into().to_h();
    Opt::from_h(cx::op(
        &[h],
        |mut es| Expr::Some(Box::new(es.pop().expect("one"))),
        |vs| Ok(vs[0].clone()),
    ))
}

/// `ENone`, at `T`.
pub fn none<T: Data>() -> Opt<T> {
    if cx::emitting() {
        Opt::from_h(cx::e(Expr::None(T::ty())))
    } else {
        Opt::from_h(cx::lit(Value::Null))
    }
}

// A closure over an element: run once with a fresh symbol under Emit, as
// often as there are elements under Native.
fn bound<T: Data>(sym: crate::ir::Sym) -> T {
    T::from_h(cx::e(Expr::Var(sym)))
}

impl<T: Data> Opt<T> {
    /// `EStd IsSome`.
    pub fn is_some(self) -> Bool {
        std1(StdFn::IsSome, self.0)
    }
    /// `EOp Not [EStd IsSome]`.
    pub fn is_none(self) -> Bool {
        self.is_some().not()
    }
    /// `EStd Unwrap`: the value, or the refusal `unwrapped none`. What a
    /// plan's projection takes a lookup's row with, behind a `having` that
    /// holds `is_some()` — a projection is evaluated only for an admitted
    /// node; everywhere else `or_refuse` says why.
    pub fn unwrap(self) -> T {
        std1(StdFn::Unwrap, self.0)
    }
    /// `EStd UnwrapOr`.
    pub fn unwrap_or(self, d: impl Into<T>) -> T {
        std2(StdFn::UnwrapOr, self.0, d.into().to_h())
    }
    /// `EMatch opt x e d`: `f` of the value, or the default.
    pub fn map_or<U: Data>(self, d: impl Into<U>, f: impl FnOnce(T) -> U) -> U {
        let d = d.into();
        if cx::emitting() {
            let o = cx::expr(self.0);
            let x = cx::fresh();
            let body = cx::in_expr(|| cx::expr(f(bound::<T>(x)).to_h()));
            return U::from_h(cx::e(Expr::Match(Box::new(o), x, Box::new(body), Box::new(cx::expr(d.to_h())))));
        }
        match cx::value(self.0) {
            Value::Null => d,
            v => f(T::from_h(cx::lit(v))),
        }
    }
    /// `EMatch opt x (ESome e) (ENone U)`.
    pub fn map<U: Data>(self, f: impl FnOnce(T) -> U) -> Opt<U> {
        if cx::emitting() {
            let o = cx::expr(self.0);
            let x = cx::fresh();
            let body = cx::in_expr(|| cx::expr(f(bound::<T>(x)).to_h()));
            let none = Expr::None(U::ty());
            return Opt::from_h(cx::e(Expr::Match(Box::new(o), x, Box::new(Expr::Some(Box::new(body))), Box::new(none))));
        }
        match cx::value(self.0) {
            Value::Null => none(),
            v => {
                let u = f(T::from_h(cx::lit(v)));
                Opt::from_h(u.to_h())
            }
        }
    }
    /// `EMatch opt x (EIf p (ESome (EVar x)) (ENone T)) (ENone T)`.
    pub fn filter(self, f: impl FnOnce(T) -> Bool) -> Opt<T> {
        if cx::emitting() {
            let o = cx::expr(self.0);
            let x = cx::fresh();
            let p = cx::in_expr(|| cx::expr(f(bound::<T>(x)).0));
            let none = || Box::new(Expr::None(T::ty()));
            let keep = Expr::If(Box::new(p), Box::new(Expr::Some(Box::new(Expr::Var(x)))), none());
            return Opt::from_h(cx::e(Expr::Match(Box::new(o), x, Box::new(keep), none())));
        }
        match cx::value(self.0) {
            Value::Null => self,
            v => {
                let keep = f(T::from_h(cx::lit(v)));
                match cx::value(keep.0) {
                    Value::Bool(true) => self,
                    _ => none(),
                }
            }
        }
    }
    /// The value, or the refusal `msg`: `SLet s opt`,
    /// `SIf (EStd IsSome [EVar s]) [] [SRefuse msg]`, and `EStd Unwrap [EVar s]`.
    pub fn or_refuse(self, msg: &str) -> T {
        if cx::emitting() {
            let s = cx::bind(cx::expr(self.0));
            let c = Expr::Std(StdFn::IsSome, vec![cx::expr(s)]);
            cx::stmt(crate::ir::Stmt::If(c, vec![], vec![crate::ir::Stmt::Refuse(Expr::Lit(Value::text(msg)))]));
            return T::from_h(cx::e(Expr::Std(StdFn::Unwrap, vec![cx::expr(s)])));
        }
        if !cx::halted() && cx::value(self.0).is_null() {
            cx::refused(msg.into());
        }
        T::from_h(self.0)
    }
}

// Lists -------------------------------------------------------------------

/// `TList t`.
#[repr(transparent)]
pub struct List<T>(pub(crate) H, PhantomData<fn() -> T>);

impl<T> Clone for List<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for List<T> {}

impl<T: Data> Data for List<T> {
    fn ty() -> Ty {
        Ty::List(Box::new(T::ty()))
    }
    fn from_h(h: H) -> Self {
        List(h, PhantomData)
    }
    fn to_h(&self) -> H {
        self.0
    }
}

/// `list([a, b])` is `EList`; `list(field)` is a list input field
/// ([`super::input::list`]): one name, two meanings, told apart by the
/// argument.
pub trait ListOf {
    type Out;
    fn list(self) -> Self::Out;
}

impl<T: Data, const N: usize> ListOf for [T; N] {
    type Out = List<T>;
    fn list(self) -> List<T> {
        let hs: Vec<H> = self.iter().map(|x| x.to_h()).collect();
        if cx::emitting() && hs.is_empty() {
            return List::from_h(cx::e(Expr::List(vec![])));
        }
        List::from_h(cx::op(&hs, Expr::List, |vs| Ok(Value::List(vs.iter().map(|v| (*v).clone()).collect()))))
    }
}

impl<T: Data> ListOf for Vec<T> {
    type Out = List<T>;
    fn list(self) -> List<T> {
        let hs: Vec<H> = self.iter().map(|x| x.to_h()).collect();
        List::from_h(cx::op(&hs, Expr::List, |vs| Ok(Value::List(vs.iter().map(|v| (*v).clone()).collect()))))
    }
}

/// `EList`, or a list input field; see [`ListOf`].
pub fn list<A: ListOf>(a: A) -> A::Out {
    a.list()
}

/// Native: the list at a handle, shared rather than copied, and the node a
/// loop reads its elements through in place. A list a closure captured is
/// iterated as often as the closure runs and copied never.
fn elems(h: H) -> (std::rc::Rc<Value>, H, usize) {
    let list = cx::shared(h);
    let n = match &*list {
        Value::List(xs) => xs.len(),
        Value::Null => 0,
        other => {
            cx::halt(EvalFault::Bug(EvalError::TypeError(format!("expected a list, got {other:?}"))));
            0
        }
    };
    let base = cx::lit_rc(list.clone());
    (list, base, n)
}

fn items(list: &Value) -> &[Value] {
    match list {
        Value::List(xs) => xs,
        _ => &[],
    }
}

impl<T: Data> List<T> {
    /// `EStd Len`.
    pub fn len(self) -> Int {
        std1(StdFn::Len, self.0)
    }
    /// `EStd Len` is zero.
    pub fn is_empty(self) -> Bool {
        self.len().eq(0)
    }
    /// `EStd First`.
    pub fn first(self) -> Opt<T> {
        std1(StdFn::First, self.0)
    }
    /// `EStd Last`.
    pub fn last(self) -> Opt<T> {
        std1(StdFn::Last, self.0)
    }
    /// `EStd Contains`.
    pub fn contains(self, x: impl Into<T>) -> Bool {
        std2(StdFn::Contains, self.0, x.into().to_h())
    }
    /// `EStd Reverse`.
    pub fn reverse(self) -> List<T> {
        std1(StdFn::Reverse, self.0)
    }

    // Native: `f` over every element, read in place, each iteration's
    // scratch forgotten once its result is copied out; then `native` of
    // the elements and the results. Every element is visited, as the
    // interpreter visits every one (a fault on the last is still a fault).
    fn each<U: Data>(
        self,
        f: &mut dyn FnMut(T) -> H,
        mk: impl FnOnce(Box<Expr>, crate::ir::Sym, Box<Expr>) -> Expr,
        native: impl FnOnce(&[Value], Vec<Value>) -> Value,
    ) -> U {
        if cx::emitting() {
            let xs = cx::expr(self.0);
            let x = cx::fresh();
            let body = cx::in_expr(|| cx::expr(f(bound::<T>(x))));
            return U::from_h(cx::e(mk(Box::new(xs), x, Box::new(body))));
        }
        if cx::halted() {
            return U::from_h(cx::lit(Value::Null));
        }
        let (list, base, n) = elems(self.0);
        let mut results = Vec::with_capacity(n);
        for i in 0..n {
            if cx::halted() {
                break;
            }
            let mark = cx::mark();
            let r = cx::value(f(T::from_h(cx::elem(base, i))));
            cx::truncate(mark);
            results.push(r);
        }
        U::from_h(cx::lit(native(items(&list), results)))
    }

    /// `EMap`.
    pub fn map<U: Data>(self, mut f: impl FnMut(T) -> U) -> List<U> {
        self.each(&mut |x| f(x).to_h(), Expr::Map, |_, rs| Value::from(rs))
    }
    /// `EFilter`.
    pub fn filter(self, mut f: impl FnMut(T) -> Bool) -> List<T> {
        self.each(&mut |x| f(x).0, Expr::Filter, |xs, rs| {
            Value::List(
                xs.iter()
                    .zip(&rs)
                    .filter(|(_, r)| **r == Value::Bool(true))
                    .map(|(x, _)| x.clone())
                    .collect(),
            )
        })
    }
    /// `EAny`.
    pub fn any(self, mut f: impl FnMut(T) -> Bool) -> Bool {
        self.each(&mut |x| f(x).0, Expr::Any, |_, rs| Value::Bool(rs.contains(&Value::Bool(true))))
    }
    /// `EAll`.
    pub fn all(self, mut f: impl FnMut(T) -> Bool) -> Bool {
        self.each(&mut |x| f(x).0, Expr::All, |_, rs| {
            Value::Bool(rs.iter().all(|r| *r == Value::Bool(true)))
        })
    }
    /// `ESortBy`: stable, by the key under the one order of values.
    pub fn sort_by<K: Data>(self, mut f: impl FnMut(T) -> K) -> List<T> {
        self.each(&mut |x| f(x).to_h(), Expr::SortBy, |xs, keys| {
            let mut order: Vec<usize> = (0..xs.len()).collect();
            order.sort_by(|a, b| keys[*a].cmp(&keys[*b]));
            Value::List(order.into_iter().map(|i| xs[i].clone()).collect())
        })
    }
    /// `EFold`.
    pub fn fold<A: Data>(self, init: impl Into<A>, mut f: impl FnMut(A, T) -> A) -> A {
        let init = init.into();
        if cx::emitting() {
            let xs = cx::expr(self.0);
            let z = cx::expr(init.to_h());
            let acc = cx::fresh();
            let x = cx::fresh();
            let body = cx::in_expr(|| cx::expr(f(bound::<A>(acc), bound::<T>(x)).to_h()));
            return A::from_h(cx::e(Expr::Fold(Box::new(xs), Box::new(z), acc, x, Box::new(body))));
        }
        let (_list, base, n) = elems(self.0);
        let mut a = init;
        for i in 0..n {
            if cx::halted() {
                break;
            }
            // The accumulator is copied out before the iteration's scratch
            // goes, and carried into the next as a value of its own.
            let mark = cx::mark();
            let av = cx::value(f(a, T::from_h(cx::elem(base, i))).to_h());
            cx::truncate(mark);
            a = A::from_h(cx::lit(av));
        }
        a
    }
}

// Choosing ------------------------------------------------------------------

/// `EIf c a b`: an expression, both arms of which are values. (Natively
/// both arms are already computed when this is called; the IR evaluates
/// only the one taken, which differs only when the other would fault.)
pub fn pick<T: Data>(c: Bool, a: impl Into<T>, b: impl Into<T>) -> T {
    let (a, b) = (a.into().to_h(), b.into().to_h());
    T::from_h(cx::op(
        &[c.0, a, b],
        |mut es| {
            let b = es.pop().expect("three");
            let a = es.pop().expect("three");
            let c = es.pop().expect("three");
            Expr::If(Box::new(c), Box::new(a), Box::new(b))
        },
        |vs| match vs[0] {
            Value::Bool(true) => Ok(vs[1].clone()),
            Value::Bool(false) => Ok(vs[2].clone()),
            other => Err(EvalFault::Bug(EvalError::TypeError(format!("pick on {other:?}")))),
        },
    ))
}

// The context ----------------------------------------------------------------

/// Who authored the entry, and the non-determinism it was given.
#[non_exhaustive]
pub struct Ctx {
    /// `ECtxUser`: the user the authority verified.
    pub user: Text,
    /// `ECtxSession`: the login the entry was authored under.
    pub session: Text,
}

/// The context, where a closure is not handed it: an input's check
/// (`.refine(|input| ctx().has_role("editor").or(..))`). The same `Ctx` a
/// guard, a provide or a body is given.
pub fn ctx() -> Ctx {
    Ctx::current()
}

impl Ctx {
    pub(crate) fn current() -> Ctx {
        if cx::emitting() {
            Ctx {
                user: Text(cx::e(Expr::CtxUser)),
                session: Text(cx::e(Expr::CtxSession)),
            }
        } else {
            let (u, s) = cx::native(|n| (n.ctx.user.clone(), n.ctx.session.clone()));
            Ctx {
                user: Text(cx::lit(Value::from(u))),
                session: Text(cx::lit(Value::from(s))),
            }
        }
    }

    fn auto(&self, name: &str, a: Auto) -> H {
        if cx::emitting() {
            cx::auto(name, a);
            return cx::e(Expr::Auto(name.into()));
        }
        match cx::native(|n| n.autos.get(name).cloned()) {
            Some(v) => cx::lit(v),
            None => {
                cx::halt(EvalFault::Bug(EvalError::MissingAuto(name.into())));
                cx::lit(Value::Null)
            }
        }
    }

    /// `EHasRole name`: whether the entry's author holds the role — a
    /// `Bool`, usable in a guard, a provide, a body or a check. Natively,
    /// the roles of the `Ctx` the run was given: the device's own belief
    /// when it authors, the entry's — which the authority stamped with the
    /// connection's — when it is run again (`docs/plan-guards.md` D1). So a
    /// guard written with it refuses on the device, and refuses at the
    /// server a device that believed wrongly.
    pub fn has_role(&self, name: &str) -> Bool {
        if cx::emitting() {
            return Bool(cx::e(Expr::HasRole(name.into())));
        }
        let held = cx::native(|n| n.ctx.roles.contains(name));
        Bool(cx::lit(Value::Bool(held)))
    }

    /// `EAuto name` of `Now`: milliseconds since the Unix epoch, drawn once
    /// at the origin and frozen in the entry.
    pub fn now(&self, name: &str) -> Int {
        Int(self.auto(name, Auto::Now))
    }

    /// `EAuto name` of `NewId t`: a fresh id for a row of `T`, drawn once
    /// at the origin and frozen in the entry.
    pub fn new_id<T: Row>(&self, name: &str) -> Id<T> {
        Id::from_h(self.auto(name, Auto::NewId(T::NAME.into())))
    }
}

impl<T: Data> Opt<T> {
    /// `ECmp Eq`: an option is flat, so `none` equals only `none` and
    /// `some(x)` equals `some(x)`.
    pub fn eq(self, b: impl Into<Opt<T>>) -> Bool {
        Bool(cx::cmp_op(CmpOp::Eq, self.0, b.into().0))
    }
    /// `ECmp Ne`.
    pub fn ne(self, b: impl Into<Opt<T>>) -> Bool {
        Bool(cx::cmp_op(CmpOp::Ne, self.0, b.into().0))
    }
}
