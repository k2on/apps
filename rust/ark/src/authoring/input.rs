//! §1.3, §2.5 A procedure's input: a struct, its fields said once in
//! [`Input::schema`] with the checks each runs before anything else.

use std::marker::PhantomData;
use std::sync::Arc;

use crate::ir::Check;
use crate::schema::Ty;

use super::cx::H;
use super::schema::Row;
use super::values::{Bool, Bytes, Data, Id, Int, List, ListOf, Opt, Text};

/// An input: a struct of the vocabulary's values, its fields in order.
pub trait Input: Sized + 'static {
    fn schema() -> Object<Self>;
}

/// No input at all: `_input: ()`.
impl Input for () {
    fn schema() -> Object<()> {
        object()
    }
}

pub(crate) type RefineFn = Arc<dyn Fn(H) -> H + Send + Sync>;

#[derive(Clone)]
pub(crate) enum CheckSpec {
    Plain(Check),
    Refine(RefineFn, Option<String>),
}

pub(crate) type ObjectRefine<I> = Arc<dyn Fn(&I) -> Bool + Send + Sync>;

/// An input's fields and the refinements over the whole of it.
pub struct Object<I> {
    pub(crate) fields: Vec<(String, Ty, Vec<CheckSpec>)>,
    pub(crate) refine: Vec<(ObjectRefine<I>, Option<String>)>,
}

/// An input with no fields yet.
pub fn object<I>() -> Object<I> {
    Object {
        fields: vec![],
        refine: vec![],
    }
}

impl<I> Object<I> {
    /// The next field, in the struct's field order.
    pub fn field<V: Data>(mut self, name: &str, f: FieldB<V>) -> Self {
        self.fields.push((name.into(), f.ty, f.checks));
        self
    }
    /// A check over the whole input, after every field's.
    pub fn refine(mut self, f: impl Fn(&I) -> Bool + Send + Sync + 'static) -> Self {
        self.refine.push((Arc::new(f), None));
        self
    }
    /// The message of the refinement just added (instead of `invalid`).
    pub fn why(mut self, msg: &str) -> Self {
        let r = self.refine.last_mut().expect(".why() after .refine()");
        r.1 = Some(msg.into());
        self
    }
}

/// A field of an input: its type and its checks, in order.
pub struct FieldB<V> {
    pub(crate) ty: Ty,
    pub(crate) checks: Vec<CheckSpec>,
    _v: PhantomData<fn() -> V>,
}

fn field<V: Data>(ty: Ty) -> FieldB<V> {
    FieldB {
        ty,
        checks: vec![],
        _v: PhantomData,
    }
}

/// A text field.
pub fn text() -> FieldB<Text> {
    field(Ty::Text)
}

/// An int field.
pub fn int() -> FieldB<Int> {
    field(Ty::Int)
}

/// A bool field.
pub fn bool_() -> FieldB<Bool> {
    field(Ty::Bool)
}

/// A bytes field.
pub fn bytes() -> FieldB<Bytes> {
    field(Ty::Bytes)
}

/// An id of a row of `T`.
pub fn id<T: Row>() -> FieldB<Id<T>> {
    field(<Id<T> as Data>::ty())
}

/// The variants of an enum.
pub trait Variants {
    const VARIANTS: &'static [&'static str];
}

/// A text restricted to `E`'s variants.
pub fn enum_<E: Variants>() -> FieldB<Text> {
    field(Ty::Enum(E::VARIANTS.iter().map(|v| v.to_string()).collect()))
}

/// An optional field; its checks apply when it is `Some`.
pub fn opt<V: Data>(f: FieldB<V>) -> FieldB<Opt<V>> {
    FieldB {
        ty: Ty::Option(Box::new(f.ty)),
        checks: f.checks,
        _v: PhantomData,
    }
}

impl<V: Data> ListOf for FieldB<V> {
    type Out = FieldB<List<V>>;
    /// A list field. A list's checks are its own (`non_empty`, `refine`);
    /// an element's are not expressible.
    fn list(self) -> FieldB<List<V>> {
        assert!(self.checks.is_empty(), "a list field's element may carry no checks");
        field(Ty::List(Box::new(self.ty)))
    }
}

impl<V: Data> FieldB<V> {
    fn check(mut self, c: Check) -> Self {
        self.checks.push(CheckSpec::Plain(c));
        self
    }
    /// A check of the value by any expression; `field: invalid` unless
    /// `.why(..)` says otherwise.
    pub fn refine(mut self, f: impl Fn(V) -> Bool + Send + Sync + 'static) -> Self {
        self.checks.push(CheckSpec::Refine(Arc::new(move |h| f(V::from_h(h)).to_h()), None));
        self
    }
    /// The message of the check just added, in place of its default.
    pub fn why(mut self, msg: &str) -> Self {
        let m = Some(msg.to_string());
        match self.checks.last_mut().expect(".why() after a check") {
            CheckSpec::Plain(Check::Trim) => panic!(".why() after .trim(): a trim does not refuse"),
            CheckSpec::Plain(Check::MinLen(_, w))
            | CheckSpec::Plain(Check::MaxLen(_, w))
            | CheckSpec::Plain(Check::Range(_, _, w))
            | CheckSpec::Plain(Check::NonEmpty(w))
            | CheckSpec::Plain(Check::Exists(w))
            | CheckSpec::Plain(Check::Refine(_, w))
            | CheckSpec::Refine(_, w) => *w = m,
        }
        self
    }
}

impl FieldB<Text> {
    /// Strip white space from both ends, before every later check and the
    /// body.
    pub fn trim(self) -> Self {
        self.check(Check::Trim)
    }
    /// At least `n` code points.
    pub fn min(self, n: i64) -> Self {
        self.check(Check::MinLen(n, None))
    }
    /// At most `n` code points.
    pub fn max(self, n: i64) -> Self {
        self.check(Check::MaxLen(n, None))
    }
}

impl FieldB<Int> {
    /// `lo <= v <= hi`.
    pub fn range(self, lo: i64, hi: i64) -> Self {
        self.check(Check::Range(Some(lo), Some(hi), None))
    }
    /// `lo <= v`.
    pub fn at_least(self, lo: i64) -> Self {
        self.check(Check::Range(Some(lo), None, None))
    }
    /// `v <= hi`.
    pub fn at_most(self, hi: i64) -> Self {
        self.check(Check::Range(None, Some(hi), None))
    }
}

impl<X: Data> FieldB<List<X>> {
    /// At least one element.
    pub fn non_empty(self) -> Self {
        self.check(Check::NonEmpty(None))
    }
}

impl<T: Row> FieldB<Id<T>> {
    /// A row with this key exists.
    pub fn exists(self) -> Self {
        self.check(Check::Exists(None))
    }
}
