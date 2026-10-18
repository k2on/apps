//! §3.12 Live state in the vocabulary (`docs/plan-live.md`,
//! `spec/AUTHORING.md` §2.6): a typed state machine per scope, declared on
//! a router beside its mutations and written in the same words.
//!
//! ```ignore
//! r.live::<Listening>("listening")
//!     .scope(|ctx| ctx.user)
//!     .init(|| Listening { session: Sum::of("idle", Idle {}), devices: PerDevice::empty(), pointer: PerDevice::empty() })
//!     .transition::<()>("pause", |s, ctx, ()| ..)                                   // ordered
//!     .transition::<Point>("point", |s, ctx, p| Listening { pointer: s.pointer.put(ctx.device, p.at), ..s })  // ephemeral
//!     .on_join(|s, ctx, device| Listening { devices: s.devices.put(device.id, device), ..s })
//!     .on_leave(|s, ctx, device| Listening { devices: s.devices.clear(device.id), ..s })
//! ```
//!
//! The types a state is made of are the vocabulary's: a record, a sum of
//! records ([`Sum`] over a [`SumType`]), [`Opt`](super::Opt),
//! [`List`](super::List), [`NonEmpty`] and [`PerDevice`]. A sum and a record
//! [`Fields::named`](super::Fields::named) are declared in the module's
//! type section by name ([`declare`]); everything else is written where it
//! is used. A transition is `|state, ctx, input| new_state`, run once under
//! `Emit` to be the IR and on every call under `Native`, as a mutator's
//! body is — and held to the interpreter the same way
//! ([`NativeLive::agrees`]).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;
use std::sync::Arc;

use crate::eval::{self, Args, EvalError, EvalFault};
use crate::hash::{live_closure, live_hash, LiveClosure};
use crate::ir::{self, Check, Expr, Field, Stmt, Sym, Transition};
use crate::schema::Ty;
use crate::store::Refusal;
use crate::value::Value;

use super::cx::{self, Cx, H};
use super::input::{CheckSpec, Input};
use super::raw;
use super::schema::Record;
use super::values::{Bool, Ctx, Data, Int, List, Opt, Text};

// The type section ---------------------------------------------------------

/// The named types a build has declared, the names being worked out, and
/// the clashes met.
type Declared = (BTreeMap<String, Ty>, BTreeSet<String>, Vec<String>);

thread_local! {
    /// The named types a module build has met, by name, and the names whose
    /// declaration is being worked out — so a type that refers to itself
    /// is a name rather than a loop.
    static DECLARED: RefCell<Option<Declared>> = const { RefCell::new(None) };
}

/// `docs/plan-live.md` The named type `name`, declared as `def` says, in the
/// type section of the module being built: what [`SumType`]'s and a named
/// record's `ty()` are. Outside a build it is only the name. Two
/// declarations of one name that differ are an authoring error the build
/// reports.
pub(crate) fn declare(name: &str, def: impl FnOnce() -> Ty) -> Ty {
    let busy = DECLARED.with(|d| match &mut *d.borrow_mut() {
        Some((done, working, _)) => done.contains_key(name) || !working.insert(name.to_string()),
        None => true,
    });
    if !busy {
        let t = def();
        DECLARED.with(|d| {
            if let Some((done, working, clashes)) = &mut *d.borrow_mut() {
                working.remove(name);
                match done.get(name) {
                    Some(had) if *had != t => clashes.push(format!("the type {name} is declared twice, as {had:?} and as {t:?}")),
                    _ => {
                        done.insert(name.to_string(), t);
                    }
                }
            }
        });
    }
    Ty::Named(name.into())
}

/// Start recording the named types a build meets.
pub(crate) fn begin_types() {
    DECLARED.with(|d| *d.borrow_mut() = Some((BTreeMap::new(), BTreeSet::new(), vec![])));
}

/// What was recorded, by name, and any clash; recording stops.
pub(crate) fn end_types() -> (BTreeMap<String, Ty>, Vec<String>) {
    DECLARED.with(|d| match d.borrow_mut().take() {
        Some((done, _, clashes)) => (done, clashes),
        None => (BTreeMap::new(), vec![]),
    })
}

// Sums -----------------------------------------------------------------------

/// `docs/plan-live.md` A sum of records, declared: its name in the type
/// section, and its variants, each a record type under a name.
///
/// ```ignore
/// pub struct Clock;
/// impl SumType for Clock {
///     const NAME: &str = "clock";
///     fn variants() -> Cases<Self> {
///         cases().of::<Paused>("paused").of::<Playing>("playing")
///     }
/// }
/// // a value of it is a `Sum<Clock>`
/// ```
pub trait SumType: 'static {
    const NAME: &'static str;
    fn variants() -> Cases<Self>;
}

/// A sum's variants, by name, each a record's fields.
pub struct Cases<V: ?Sized> {
    pub(crate) variants: BTreeMap<String, BTreeMap<String, Ty>>,
    _v: PhantomData<fn() -> Box<V>>,
}

/// A sum with no variants yet.
pub fn cases<V: ?Sized>() -> Cases<V> {
    Cases {
        variants: BTreeMap::new(),
        _v: PhantomData,
    }
}

impl<V: ?Sized> Cases<V> {
    /// The variant `name`, a record of `R`'s fields (an empty record for a
    /// variant that carries nothing).
    pub fn of<R: Record>(mut self, name: &str) -> Self {
        self.variants.insert(name.into(), R::fields().fields.into_iter().collect());
        self
    }
}

/// `docs/plan-live.md` A value of the sum `V`: one handle, as every value of
/// the vocabulary is.
#[repr(transparent)]
pub struct Sum<V>(pub(crate) H, PhantomData<fn() -> V>);

impl<V> Clone for Sum<V> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<V> Copy for Sum<V> {}

impl<V: SumType> Data for Sum<V> {
    fn ty() -> Ty {
        declare(V::NAME, || Ty::Sum(V::variants().variants))
    }
    fn from_h(h: H) -> Self {
        Sum(h, PhantomData)
    }
    fn to_h(&self) -> H {
        self.0
    }
}

// A record's fields as expressions (Emit) or its struct (Native).
fn record_parts<R: Record>(r: &R) -> H {
    r.to_h()
}

impl<V: SumType> Sum<V> {
    /// `EVariant`: the variant `name`, carrying the record `r`.
    pub fn of<R: Record>(name: &str, r: R) -> Sum<V> {
        let h = record_parts(&r);
        if cx::emitting() {
            let names: Vec<String> = R::fields().fields.into_iter().map(|(n, _)| n).collect();
            let fields: BTreeMap<String, Expr> = match cx::expr(h) {
                Expr::Struct(fs) => fs,
                Expr::Lit(Value::Struct(m)) => m.into_iter().map(|(k, v)| (k, Expr::Lit(v))).collect(),
                other => names.into_iter().map(|n| (n.clone(), Expr::Field(Box::new(other.clone()), n))).collect(),
            };
            return Sum::from_h(cx::e(Expr::Variant(Self::ty(), name.into(), fields)));
        }
        if cx::halted() {
            return Sum::from_h(cx::lit(Value::Null));
        }
        let mut m = match cx::value(h) {
            Value::Struct(m) => *m,
            _ => BTreeMap::new(),
        };
        m.insert(crate::types::TAG.into(), Value::text(name));
        Sum::from_h(cx::lit(Value::Struct(Box::new(m))))
    }

    /// `ECase`: the sum taken apart, one arm per variant
    /// ([`Case::on`]), ended with [`Case::end`].
    pub fn case<T: Data>(self) -> Case<V, T> {
        let native_tag = if cx::emitting() || cx::halted() {
            None
        } else {
            match cx::value(self.0) {
                Value::Struct(m) => match m.get(crate::types::TAG) {
                    Some(Value::Text(t)) => Some(t.to_string()),
                    _ => None,
                },
                _ => None,
            }
        };
        Case {
            of: self.0,
            arms: vec![],
            taken: None,
            tag: native_tag,
            _t: PhantomData,
        }
    }

    /// Whether the value is the variant `name`: `ECase` with a `true` arm
    /// for it and a `false` arm for every other.
    pub fn is(self, name: &str) -> Bool {
        if cx::emitting() {
            let arms = V::variants()
                .variants
                .keys()
                .map(|v| (v.clone(), cx::fresh(), Expr::Lit(Value::Bool(v == name))))
                .collect();
            return Bool(cx::e(Expr::Case(Box::new(cx::expr(self.0)), arms)));
        }
        if cx::halted() {
            return Bool(cx::lit(Value::Bool(false)));
        }
        let yes = matches!(cx::value(self.0), Value::Struct(m) if m.get(crate::types::TAG) == Some(&Value::text(name)));
        Bool(cx::lit(Value::Bool(yes)))
    }

    /// `ECmp Eq`.
    pub fn eq(self, b: Sum<V>) -> Bool {
        Bool(cx::cmp_op(crate::ir::CmpOp::Eq, self.0, b.0))
    }
}

/// A sum being taken apart: the arms written so far.
pub struct Case<V, T> {
    of: H,
    arms: Vec<(String, Sym, Expr)>,
    /// Native: the arm the value's tag chose, once run.
    taken: Option<H>,
    tag: Option<String>,
    _t: PhantomData<fn() -> (V, T)>,
}

impl<V: SumType, T: Data> Case<V, T> {
    /// The arm for the variant `name`, over its record `R`. Under `Emit`
    /// every arm is recorded; natively only the one the value is runs.
    pub fn on<R: Record>(mut self, name: &str, f: impl FnOnce(R) -> T) -> Self {
        if cx::emitting() {
            let x = cx::fresh();
            let body = cx::in_expr(|| cx::expr(f(R::from_h(cx::e(Expr::Var(x)))).to_h()));
            self.arms.push((name.into(), x, body));
            return self;
        }
        if self.tag.as_deref() == Some(name) && self.taken.is_none() {
            let rec = match cx::value(self.of) {
                Value::Struct(mut m) => {
                    m.remove(crate::types::TAG);
                    Value::Struct(m)
                }
                other => other,
            };
            self.taken = Some(f(R::from_h(cx::lit(rec))).to_h());
        }
        self
    }

    /// The case's value: under `Emit` the `ECase` of its arms in the sum's
    /// variant order; natively the taken arm's value.
    pub fn end(mut self) -> T {
        if cx::emitting() {
            self.arms.sort_by(|a, b| a.0.cmp(&b.0));
            return T::from_h(cx::e(Expr::Case(Box::new(cx::expr(self.of)), self.arms)));
        }
        match self.taken {
            Some(h) => T::from_h(h),
            None => {
                if !cx::halted() {
                    cx::halt(EvalFault::Bug(EvalError::TypeError(format!(
                        "{}: no arm for the variant {:?}",
                        V::NAME,
                        self.tag
                    ))));
                }
                T::from_h(cx::lit(Value::Null))
            }
        }
    }
}

// Non-empty lists ----------------------------------------------------------------

/// `TNonEmpty t`: a list of at least one, held to it where it crosses a
/// boundary; a [`List`] to everything that reads it.
#[repr(transparent)]
pub struct NonEmpty<T>(pub(crate) H, PhantomData<fn() -> T>);

impl<T> Clone for NonEmpty<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for NonEmpty<T> {}

impl<T: Data> Data for NonEmpty<T> {
    fn ty() -> Ty {
        Ty::NonEmpty(Box::new(T::ty()))
    }
    fn from_h(h: H) -> Self {
        NonEmpty(h, PhantomData)
    }
    fn to_h(&self) -> H {
        self.0
    }
}

impl<T: Data> NonEmpty<T> {
    /// The same list, read as a list.
    pub fn list(self) -> List<T> {
        List::from_h(self.0)
    }
    /// [`List::insert`], which leaves a non-empty list non-empty.
    pub fn insert(self, at: impl Into<Int>, x: impl Into<T>) -> NonEmpty<T> {
        self.list().insert(at, x).non_empty()
    }
    /// [`List::concat`], which leaves a non-empty list non-empty.
    pub fn concat(self, more: List<T>) -> NonEmpty<T> {
        self.list().concat(more).non_empty()
    }
    /// [`List::move_at`], which leaves a list as long as it was.
    pub fn move_at(self, from: impl Into<Int>, to: impl Into<Int>) -> NonEmpty<T> {
        self.list().move_at(from, to).non_empty()
    }
    /// [`List::remove_at`]: a list, since taking out the one element leaves
    /// none — `.non_empty()` declares it one again, held where it crosses a
    /// boundary.
    pub fn remove_at(self, at: impl Into<Int>) -> List<T> {
        self.list().remove_at(at)
    }
    /// [`List::slice`]: a list, which may have no element, as
    /// [`NonEmpty::remove_at`]'s.
    pub fn slice(self, start: impl Into<Int>, end: impl Into<Int>) -> List<T> {
        self.list().slice(start, end)
    }
}

impl<T: Data> List<T> {
    /// The same list, declared non-empty: where it crosses a boundary — a
    /// transition's result — one with no element is refused there.
    pub fn non_empty(self) -> NonEmpty<T> {
        NonEmpty::from_h(self.0)
    }
}

// Per device ------------------------------------------------------------------

/// `TPerDevice t`: one slot per device standing in an instance, by device
/// id; a slot that holds nothing is not there.
#[repr(transparent)]
pub struct PerDevice<T>(pub(crate) H, PhantomData<fn() -> T>);

impl<T> Clone for PerDevice<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for PerDevice<T> {}

impl<T: Data> Data for PerDevice<T> {
    fn ty() -> Ty {
        Ty::PerDevice(Box::new(T::ty()))
    }
    fn from_h(h: H) -> Self {
        PerDevice(h, PhantomData)
    }
    fn to_h(&self) -> H {
        self.0
    }
}

/// One filled slot of a [`PerDevice`], as [`PerDevice::slots`] lists them.
///
/// Its own [`Data`] rather than a [`Record`]: a record is read as one
/// handle per field, and a slot's value may itself be a record — harken's
/// devices are — which is many. So the slot is one handle, taken apart by
/// field, and the value is whatever its type makes of the field `value`.
pub struct Slot<T> {
    pub device: Text,
    pub value: T,
}

impl<T: Data> Data for Slot<T> {
    fn ty() -> Ty {
        Ty::Struct([("device".to_string(), Ty::Text), ("value".to_string(), T::ty())].into_iter().collect())
    }
    fn from_h(h: H) -> Self {
        let part = |name: &str| {
            if cx::emitting() {
                cx::e(Expr::Field(Box::new(cx::expr(h)), name.into()))
            } else {
                cx::field(h, name.into())
            }
        };
        Slot {
            device: Text::from_h(part("device")),
            value: T::from_h(part("value")),
        }
    }
    fn to_h(&self) -> H {
        let (device, value) = (self.device.to_h(), self.value.to_h());
        if cx::emitting() {
            return cx::e(Expr::Struct(
                [("device".to_string(), cx::expr(device)), ("value".to_string(), cx::expr(value))]
                    .into_iter()
                    .collect(),
            ));
        }
        cx::lit(Value::record(vec![("device", cx::value(device)), ("value", cx::value(value))]))
    }
}

impl<T: Data> PerDevice<T> {
    /// No slot filled: a struct with no fields.
    pub fn empty() -> PerDevice<T> {
        if cx::emitting() {
            return PerDevice::from_h(cx::e(Expr::Struct(BTreeMap::new())));
        }
        PerDevice::from_h(cx::lit(Value::Struct(Box::default())))
    }
    /// `ESlot`: the device's slot.
    pub fn get(self, device: impl Into<Text>) -> Opt<T> {
        let d = device.into();
        Opt::from_h(cx::op(
            &[self.0, d.0],
            |mut es| {
                let k = es.pop().expect("two");
                let m = es.pop().expect("two");
                Expr::Slot(Box::new(m), Box::new(k))
            },
            |vs| match (vs[0], vs[1]) {
                (Value::Struct(m), Value::Text(k)) => Ok(m.get(&**k).cloned().unwrap_or(Value::Null)),
                other => Err(EvalFault::Bug(EvalError::TypeError(format!("a slot of {other:?}")))),
            },
        ))
    }
    /// `ESetSlot`: the device's slot set to the option's value, or emptied.
    pub fn set(self, device: impl Into<Text>, v: Opt<T>) -> PerDevice<T> {
        let d = device.into();
        PerDevice::from_h(cx::op(
            &[self.0, d.0, v.0],
            |mut es| {
                let v = es.pop().expect("three");
                let k = es.pop().expect("three");
                let m = es.pop().expect("three");
                Expr::SetSlot(Box::new(m), Box::new(k), Box::new(v))
            },
            |vs| match (vs[0], vs[1]) {
                (Value::Struct(m), Value::Text(k)) => {
                    let mut m = (**m).clone();
                    match vs[2] {
                        Value::Null => m.remove(&**k),
                        x => m.insert(k.to_string(), x.clone()),
                    };
                    Ok(Value::Struct(Box::new(m)))
                }
                other => Err(EvalFault::Bug(EvalError::TypeError(format!("a slot of {other:?}")))),
            },
        ))
    }
    /// The device's slot filled with `v`.
    pub fn put(self, device: impl Into<Text>, v: impl Into<T>) -> PerDevice<T> {
        self.set(device, super::some(v))
    }
    /// The device's slot emptied.
    pub fn clear(self, device: impl Into<Text>) -> PerDevice<T> {
        self.set(device, super::none())
    }
    /// `ESlots`: every filled slot, in device-id order.
    pub fn slots(self) -> List<Slot<T>> {
        List::from_h(cx::op(
            &[self.0],
            |mut es| Expr::Slots(Box::new(es.pop().expect("one"))),
            |vs| match vs[0] {
                Value::Struct(m) => Ok(Value::from(
                    m.iter()
                        .map(|(k, v)| Value::record(vec![("device", Value::text(k.clone())), ("value", v.clone())]))
                        .collect::<Vec<_>>(),
                )),
                other => Err(EvalFault::Bug(EvalError::TypeError(format!("the slots of {other:?}")))),
            },
        ))
    }
}

// The context, and a device ------------------------------------------------------

/// What a transition is handed beside the state and its input: who calls,
/// from which device, and when — the server's time when it sequences the
/// call, the device's guess of it when it previews one.
#[non_exhaustive]
pub struct LiveCtx {
    /// `ECtxUser`.
    pub user: Text,
    /// `ECtxSession`.
    pub session: Text,
    /// `ECtxDevice`: the id of the device calling.
    pub device: Text,
    /// `ECtxNow`: milliseconds since the Unix epoch.
    pub now: Int,
}

impl LiveCtx {
    fn emit() -> LiveCtx {
        LiveCtx {
            user: Text(cx::e(Expr::CtxUser)),
            session: Text(cx::e(Expr::CtxSession)),
            device: Text(cx::e(Expr::CtxDevice)),
            now: Int(cx::e(Expr::CtxNow)),
        }
    }

    fn native(c: &eval::LiveCtx) -> LiveCtx {
        LiveCtx {
            user: Text(cx::lit(Value::text(c.user.clone()))),
            session: Text(cx::lit(Value::text(c.session.clone()))),
            device: Text(cx::lit(Value::text(c.device.clone()))),
            now: Int(cx::lit(Value::Int(c.now))),
        }
    }

    /// `EHasRole`: whether the caller holds the role.
    pub fn has_role(&self, name: &str) -> Bool {
        Ctx::current().has_role(name)
    }
}

/// A device as a hook is handed it: [`crate::ir::device_ty`].
pub struct Device {
    pub can_play: Bool,
    pub id: Text,
    pub kind: Text,
    pub name: Text,
    pub user: Text,
}

impl Record for Device {
    fn fields() -> super::Fields<Self> {
        super::fields()
            .field("can_play", super::bool_())
            .field("id", super::text())
            .field("kind", super::text())
            .field("name", super::text())
            .field("user", super::text())
    }
}

// The declaration -----------------------------------------------------------------

type ScopeRun = Arc<dyn Fn() -> H + Send + Sync>;
type InitRun = Arc<dyn Fn() -> H + Send + Sync>;
/// A transition's body or a guard: over the state, the context and the
/// input's handles.
type TransRun = Arc<dyn Fn(H, &LiveCtx, &[H]) -> H + Send + Sync>;

#[derive(Clone)]
pub(crate) struct TransDecl {
    name: String,
    fields: Vec<(String, Ty, Vec<CheckSpec>)>,
    guards: Vec<(TransRun, String)>,
    body: TransRun,
}

#[derive(Clone)]
pub(crate) struct LiveDecl {
    name: String,
    ty: fn() -> Ty,
    scope: Option<ScopeRun>,
    init: Option<InitRun>,
    transitions: Vec<TransDecl>,
    on_join: Option<TransDecl>,
    on_leave: Option<TransDecl>,
    on_restore: Option<TransDecl>,
}

pub(crate) type Lives = std::rc::Rc<RefCell<Vec<LiveDecl>>>;

/// A live state being declared: every method adds to it in place, and the
/// router it was declared on carries it into the module.
pub struct LiveB<S> {
    lives: Lives,
    at: usize,
    /// Which transition `guard` adds to: the last one declared.
    last: Option<Which>,
    _s: PhantomData<fn() -> S>,
}

#[derive(Clone, Copy)]
enum Which {
    Declared(usize),
    Join,
    Leave,
    Restore,
}

pub(crate) fn declare_live<S: Data>(lives: &Lives, name: &str) -> LiveB<S> {
    let mut v = lives.borrow_mut();
    v.push(LiveDecl {
        name: name.into(),
        ty: S::ty,
        scope: None,
        init: None,
        transitions: vec![],
        on_join: None,
        on_leave: None,
        on_restore: None,
    });
    LiveB {
        lives: lives.clone(),
        at: v.len() - 1,
        last: None,
        _s: PhantomData,
    }
}

fn fields_of<I: Input>() -> Vec<(String, Ty, Vec<CheckSpec>)> {
    I::schema().fields
}

fn input_of<I: Input>(hs: &[H]) -> I {
    raw::assemble(hs, std::any::type_name::<I>())
}

fn device_fields() -> Vec<(String, Ty, Vec<CheckSpec>)> {
    vec![("device".into(), ir::device_ty(), vec![])]
}

impl<S: Data> LiveB<S> {
    fn with(&self, f: impl FnOnce(&mut LiveDecl)) {
        f(&mut self.lives.borrow_mut()[self.at]);
    }

    /// The keys of the instances a person holds: a `Text`, or a `List` of
    /// them, of the context alone — `|ctx| ctx.user` is one per account.
    pub fn scope<K: Data>(self, f: impl Fn(&Ctx) -> K + Send + Sync + 'static) -> Self {
        self.with(|d| d.scope = Some(Arc::new(move || f(&Ctx::current()).to_h())));
        self
    }

    /// The value an instance starts as: a constant.
    pub fn init(self, f: impl Fn() -> S + Send + Sync + 'static) -> Self {
        self.with(|d| d.init = Some(Arc::new(move || f().to_h())));
        self
    }

    /// A transition: `|state, ctx, input| new_state`, a refusal by
    /// `refuse(..)` or a check of the input. Its kind — ordered or
    /// ephemeral — is read off what it emits (`crate::ir::ordering`).
    pub fn transition<I: Input>(mut self, name: &str, f: impl Fn(S, &LiveCtx, I) -> S + Send + Sync + 'static) -> Self {
        let body: TransRun = Arc::new(move |s, ctx, ins| f(S::from_h(s), ctx, input_of::<I>(ins)).to_h());
        let t = TransDecl {
            name: name.into(),
            fields: fields_of::<I>(),
            guards: vec![],
            body,
        };
        let mut at = 0;
        self.with(|d| {
            d.transitions.push(t);
            at = d.transitions.len() - 1;
        });
        self.last = Some(Which::Declared(at));
        self
    }

    /// A guard on the transition declared last: a `Bool` over the state,
    /// the context and the input, which refuses the call with `why` when it
    /// is false — before the body runs.
    pub fn guard<I: Input>(self, why: &str, f: impl Fn(S, &LiveCtx, I) -> Bool + Send + Sync + 'static) -> Self {
        let g: TransRun = Arc::new(move |s, ctx, ins| f(S::from_h(s), ctx, input_of::<I>(ins)).0);
        let why = why.to_string();
        let which = self.last.expect(".guard() after a transition");
        self.with(|d| {
            let t = match which {
                Which::Declared(i) => &mut d.transitions[i],
                Which::Join => d.on_join.as_mut().expect("on_join"),
                Which::Leave => d.on_leave.as_mut().expect("on_leave"),
                Which::Restore => d.on_restore.as_mut().expect("on_restore"),
            };
            t.guards.push((g, why));
        });
        self
    }

    /// The hook the server runs when a device first stands in an instance:
    /// `|state, ctx, device| new_state`.
    pub fn on_join(mut self, f: impl Fn(S, &LiveCtx, Device) -> S + Send + Sync + 'static) -> Self {
        let body: TransRun = Arc::new(move |s, ctx, ins| f(S::from_h(s), ctx, Device::from_h(ins[0])).to_h());
        self.with(|d| {
            d.on_join = Some(TransDecl {
                name: ir::ON_JOIN.into(),
                fields: device_fields(),
                guards: vec![],
                body,
            })
        });
        self.last = Some(Which::Join);
        self
    }

    /// The hook the server runs when a device's last connection to an
    /// instance goes, after it has emptied the device's ephemeral slots.
    pub fn on_leave(mut self, f: impl Fn(S, &LiveCtx, Device) -> S + Send + Sync + 'static) -> Self {
        let body: TransRun = Arc::new(move |s, ctx, ins| f(S::from_h(s), ctx, Device::from_h(ins[0])).to_h());
        self.with(|d| {
            d.on_leave = Some(TransDecl {
                name: ir::ON_LEAVE.into(),
                fields: device_fields(),
                guards: vec![],
                body,
            })
        });
        self.last = Some(Which::Leave);
        self
    }

    /// The hook the server runs over a snapshot it restores at start, with
    /// nobody present: `|state, ctx| new_state` — harken's "paused, with no
    /// output".
    pub fn on_restore(mut self, f: impl Fn(S, &LiveCtx) -> S + Send + Sync + 'static) -> Self {
        let body: TransRun = Arc::new(move |s, ctx, _| f(S::from_h(s), ctx).to_h());
        self.with(|d| {
            d.on_restore = Some(TransDecl {
                name: ir::ON_RESTORE.into(),
                fields: vec![],
                guards: vec![],
                body,
            })
        });
        self.last = Some(Which::Restore);
        self
    }
}

// Emit --------------------------------------------------------------------------

/// The live states of a router as the IR: every closure run once under
/// `Emit`. The authoring errors, by state.
pub(crate) fn emit_states(lives: &[LiveDecl]) -> (Vec<ir::State>, Vec<String>) {
    let mut errors = vec![];
    let mut out = vec![];
    for d in lives {
        let ty = (d.ty)();
        let expr_of = |f: &Option<Arc<dyn Fn() -> H + Send + Sync>>, what: &str, errors: &mut Vec<String>| -> Expr {
            match f {
                None => {
                    errors.push(format!("{}: no {what}", d.name));
                    Expr::Lit(Value::Null)
                }
                Some(f) => {
                    let (e, cx) = cx::run(Cx::emit(), || cx::in_expr(|| cx::expr(f())));
                    let em = cx.into_emit();
                    errors.extend(em.errors.iter().map(|e| format!("{}: {what}: {e}", d.name)));
                    e
                }
            }
        };
        let scope = expr_of(&d.scope, "scope", &mut errors);
        let init = expr_of(&d.init, "init", &mut errors);
        let mut emit = |t: &TransDecl| -> Transition {
            let (t, es) = emit_transition(t);
            errors.extend(es.into_iter().map(|e| format!("{}: {e}", d.name)));
            t
        };
        let transitions = d.transitions.iter().map(&mut emit).collect();
        let on_join = d.on_join.as_ref().map(&mut emit);
        let on_leave = d.on_leave.as_ref().map(&mut emit);
        let on_restore = d.on_restore.as_ref().map(&mut emit);
        out.push(ir::State {
            name: d.name.clone(),
            ty,
            init,
            scope,
            transitions,
            on_join,
            on_leave,
            on_restore,
        });
    }
    (out, errors)
}

fn emit_transition(t: &TransDecl) -> (Transition, Vec<String>) {
    let ((input, guards, ret), cx) = cx::run(Cx::emit(), || {
        let ins: Vec<H> = t.fields.iter().map(|(n, _, _)| cx::e(Expr::Arg(n.clone()))).collect();
        let mut input = Vec::with_capacity(t.fields.len());
        for (n, ty, checks) in &t.fields {
            let mut out = Vec::with_capacity(checks.len());
            for c in checks {
                out.push(match c {
                    CheckSpec::Plain(c) => c.clone(),
                    CheckSpec::Refine(f, why) => {
                        let arg = cx::e(Expr::Arg(n.clone()));
                        Check::Refine(cx::in_expr(|| cx::expr(f(arg))), why.clone())
                    }
                });
            }
            input.push((n.clone(), Field { ty: ty.clone(), checks: out }));
        }
        let ctx = LiveCtx::emit();
        let state = cx::e(Expr::LiveState);
        let guards: Vec<(Expr, String)> = t
            .guards
            .iter()
            .map(|(g, why)| (cx::in_expr(|| cx::expr(g(state, &ctx, &ins))), why.clone()))
            .collect();
        let ret = cx::expr((t.body)(state, &ctx, &ins));
        (input, guards, ret)
    });
    let mut em = cx.into_emit();
    let mut body = em.body();
    body.push(Stmt::Return(Some(ret)));
    let mut errors: Vec<String> = em.errors.iter().map(|e| format!("{}: {e}", t.name)).collect();
    if !em.autos.is_empty() {
        errors.push(format!("{}: a transition draws no auto; the server's clock is ctx.now", t.name));
    }
    (
        Transition {
            name: t.name.clone(),
            input,
            guards,
            body,
            names: BTreeMap::new(),
        },
        errors,
    )
}

// Native ---------------------------------------------------------------------------

/// A live state run natively: the domain's own Rust over a call, held to
/// [`eval::run_transition`] over the closure it emitted.
#[derive(Clone)]
pub struct NativeLive(Arc<NativeInner>);

struct NativeInner {
    hash: Vec<u8>,
    closure: LiveClosure,
    decl: LiveDecl,
}

impl std::fmt::Debug for NativeLive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NativeLive({})", self.0.decl.name)
    }
}

pub(crate) fn natives(m: &ir::Module, lives: &[LiveDecl]) -> Vec<NativeLive> {
    lives
        .iter()
        .filter_map(|d| {
            let st = m.lookup_state(&d.name)?;
            let closure = live_closure(m, st);
            Some(NativeLive(Arc::new(NativeInner {
                hash: live_hash(&closure),
                closure,
                decl: d.clone(),
            })))
        })
        .collect()
}

impl NativeLive {
    /// The state's name.
    pub fn name(&self) -> &str {
        &self.0.decl.name
    }

    /// The hash of the closure it emitted ([`crate::hash::live_hash`]): a
    /// replica runs it natively only where its module names that hash.
    pub fn hash(&self) -> &[u8] {
        &self.0.hash
    }

    /// The closure it emitted, which a runtime without it interprets.
    pub fn closure(&self) -> &LiveClosure {
        &self.0.closure
    }

    /// §3.12 Run a transition natively, as [`eval::run_transition`] runs it
    /// over the closure: the input held to its types and checked, the
    /// guards, the body, the result held to the state's type.
    pub fn run(&self, name: &str, state: &Value, ctx: &eval::LiveCtx, input: &Args) -> Result<Value, EvalFault> {
        let d = &self.0.decl;
        let c = &self.0.closure;
        let refused = |t: String| Err(EvalFault::Verdict(Refusal::Refused(t)));
        let t = match name {
            ir::ON_JOIN => d.on_join.as_ref(),
            ir::ON_LEAVE => d.on_leave.as_ref(),
            ir::ON_RESTORE => d.on_restore.as_ref(),
            _ => d.transitions.iter().find(|t| t.name == name),
        };
        let Some(t) = t else {
            if matches!(name, ir::ON_JOIN | ir::ON_LEAVE | ir::ON_RESTORE) {
                return Ok(state.clone());
            }
            return refused(format!("{}: no transition {name}", d.name));
        };
        let Some(ir_t) = c.state.transition(name) else {
            return Err(EvalFault::Bug(EvalError::UnknownFunction(name.into())));
        };
        for k in input.keys() {
            if !t.fields.iter().any(|(n, _, _)| n == k) {
                return refused(format!("{k}: not an input of {name}"));
            }
        }
        for (n, ty, _) in &t.fields {
            let Some(v) = input.get(n) else {
                return refused(format!("{n}: missing"));
            };
            if let Err(why) = crate::types::conforms(&c.types, ty, v) {
                return refused(format!("{n}: {why}"));
            }
        }
        let author = eval::Ctx::new(ctx.user.clone(), ctx.session.clone()).with_roles(ctx.roles.iter().cloned());
        let (out, _) = cx::run(Cx::native(author, Args::new()), || -> Result<Value, EvalFault> {
            let halt = || cx::native(|n| n.halt.clone());
            let lctx = LiveCtx::native(ctx);
            let state_h = cx::lit(state.clone());
            let mut checked = input.clone();
            for (i, (n, _, specs)) in t.fields.iter().enumerate() {
                let field = &ir_t.input[i].1;
                let mut refine = |j: usize, v: &Value| -> Result<bool, EvalFault> {
                    let CheckSpec::Refine(f, _) = &specs[j] else { return Ok(true) };
                    let r = cx::value(f(cx::lit(v.clone())));
                    if let Some(fault) = halt() {
                        return Err(fault);
                    }
                    Ok(r == Value::Bool(true))
                };
                match eval::check_field_with(n, field, checked[n].clone(), &|_, _| false, &mut refine)? {
                    (v, None) => {
                        checked.insert(n.clone(), v);
                    }
                    (_, Some(msg)) => return Err(EvalFault::Verdict(Refusal::Refused(msg))),
                }
            }
            let ins: Vec<H> = t.fields.iter().map(|(n, _, _)| cx::lit(checked[n].clone())).collect();
            for (g, why) in &t.guards {
                let ok = cx::value(g(state_h, &lctx, &ins));
                if let Some(fault) = halt() {
                    return Err(fault);
                }
                if ok != Value::Bool(true) {
                    return Err(EvalFault::Verdict(Refusal::Refused(why.clone())));
                }
            }
            let out = cx::value((t.body)(state_h, &lctx, &ins));
            if let Some(fault) = halt() {
                return Err(fault);
            }
            Ok(out)
        });
        let out = out?;
        match crate::types::conforms(&c.types, &c.state.ty, &out) {
            Ok(()) => Ok(out),
            Err(why) => refused(format!("{name}: the state it makes is not a {}: {why}", d.name)),
        }
    }

    /// Hold the native run to the interpreter on one call: the same new
    /// state, or the same verdict. What the agreement tests run on every
    /// transition, and what a peer may run in a debug build.
    pub fn agrees(&self, name: &str, state: &Value, ctx: &eval::LiveCtx, input: &Args) -> Result<Result<Value, EvalFault>, String> {
        let native = self.run(name, state, ctx, input);
        let ir = eval::run_transition(&self.0.closure, name, state, ctx, input);
        if native != ir {
            return Err(format!("{}.{name}: native {native:?}, interpreted {ir:?}", self.name()));
        }
        Ok(native)
    }
}
