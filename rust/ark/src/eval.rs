//! §6 Evaluation: what a domain means (`Ark.Eval`).
//!
//! An interpreter of the IR over any [`Store`]. A mutator authored in the
//! vocabulary of `spec/AUTHORING.md` and run `Native`
//! ([`crate::authoring`]) is the fast path and must agree with this file
//! over its own `Emit`; the conformance runner checks the `eval/` vectors
//! through it, and a peer uses it for a closure it received but holds no
//! native procedure for.
//!
//! A read — a query's plan, a mutator's `select` — means what
//! [`crate::view::pull`] answers (§1.4), and is asked of it through
//! [`crate::view::read`]: that is the one evaluator of plans, and it
//! evaluates the expressions inside one through [`Scope`], which is this
//! file's evaluator with the plan's binders in scope.
//!
//! A procedure runs as spec version 2 has it: its input is decoded and
//! checked field by field ([`check_field`]), then each middleware in its
//! `uses` order, then the body — all of it one transaction, and a refusal
//! anywhere leaves the store untouched.
//!
//! Two kinds of failure are kept apart: a [`Refusal`] is a verdict every
//! replica reaches; an [`EvalError`] is a bug — a module the verifier would
//! have refused, or a native procedure that disagrees with this file.
//!
//! Evaluation order is part of the meaning: `and`/`or` short-circuit left
//! to right, `if` and `match` evaluate only the taken arm, list elements
//! and call arguments left to right, a struct's fields in field-name order.

use std::borrow::Cow;
use std::collections::BTreeMap;

use crate::hash::{closure, Closure};
use crate::ir::{Check, Expr, Field, FnKind, Function, Key, Module, Op, Plan, Stmt, Sym};
use crate::schema::{Dir, Schema, Ty};
use crate::stdlib::{self, StdError};
use crate::store::{self, Change, Overlay, Refusal, Row, Store};
use crate::value::{FieldName, TableName, Value};
use crate::view::cmp;

pub use crate::stdlib::Args;

/// Who authored an entry: the user the authority verified for the
/// connection, and the login it was authored under (`Ark.Eval.Ctx`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Ctx {
    pub user: String,
    pub session: String,
}

impl Ctx {
    /// §11.2a Who authors before anyone has signed in (`Ark.Peer.nobody`):
    /// an empty user and session. No server accepts an entry authored as
    /// nobody; [`crate::peer::Replica::sign_in`] makes it somebody's.
    pub fn nobody() -> Ctx {
        Ctx::new("", "")
    }

    /// Whether this is [`Ctx::nobody`].
    pub fn is_nobody(&self) -> bool {
        self.user.is_empty() && self.session.is_empty()
    }

    pub fn new(user: impl Into<String>, session: impl Into<String>) -> Ctx {
        Ctx {
            user: user.into(),
            session: session.into(),
        }
    }
}

/// A bug, never a verdict (`Ark.Eval.EvalError`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvalError {
    UnknownFunction(String),
    WrongKind(String, FnKind),
    MissingArg(String),
    MissingAuto(String),
    UnboundVar(Sym),
    NoSuchField(FieldName),
    TypeError(String),
    Arity(String),
    /// A helper or query fell off the end of its body without returning.
    NoReturn(String),
    /// A helper reached a read or a write, or a query a write.
    Impure(String),
    UnknownTable(TableName),
    /// A relationship whose parent key is not a single column.
    CompositeParentKey(TableName),
    /// `EProvided` of a middleware the procedure did not run.
    NotProvided(String),
}

impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for EvalError {}

/// The two ways a computation stops short (`Ark.Eval.Fault`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvalFault {
    Verdict(Refusal),
    Bug(EvalError),
}

// Why a block stopped: a return, a verdict, or a bug.
enum Stop {
    Returned(Option<Value>),
    Halt(EvalFault),
}

type Run<T> = Result<T, Stop>;

fn bug<T>(e: EvalError) -> Run<T> {
    Err(Stop::Halt(EvalFault::Bug(e)))
}

fn verdict<T>(r: Refusal) -> Run<T> {
    Err(Stop::Halt(EvalFault::Verdict(r)))
}

// Everything an expression is evaluated in, borrowed: copying one is
// copying a handful of references, which is what makes binding a local
// free (`docs/plan-perf.md` R5).
#[derive(Clone, Copy)]
struct Env<'a> {
    schema: &'a Schema,
    /// The helpers and middleware in reach: a closure's, never a module's.
    helpers: &'a [Function],
    kind: FnKind,
    ctx: &'a Ctx,
    args: Params<'a>,
    autos: &'a Args,
    /// What each `Provide` the procedure has run so far returned.
    provided: &'a Args,
    /// A plan node's binders ([`NodeScope`]), under every frame.
    node: &'a [(Sym, Val<'a>)],
    /// The innermost local, which points at the one it was bound over.
    locals: Option<&'a Frame<'a>>,
    /// Whether this is a plan's node, or a helper one calls: there a sum's
    /// `fold` is arithmetic over the integers, judged on its result alone
    /// (§13, `docs/plan-db.md` D4), so that a fresh read and a view that
    /// keeps the sum as a number agree. A procedure's body keeps checked
    /// arithmetic step by step (§6.5): its natives run the fold as a Rust
    /// closure, which no recogniser can see into.
    wide: bool,
}

// §6.4 A local: one per `let`, per element a list function or a `for`
// binds, per `match`'s `some` and `update`'s row. Each is pushed on the
// evaluator's own call stack over the one before and is gone when the
// scope that bound it returns — so a bind copies no other local, and an
// element is bound by reference into the list it is in, which is what
// makes a `map` over a local of N elements cost N rather than N² (R5).
// The newest frame answers first, which is the shadowing a map's `insert`
// gave.
struct Frame<'a> {
    sym: Sym,
    value: Place<'a>,
    up: Option<&'a Frame<'a>>,
}

// Where a bound value is: a value, or a row as the store holds it
// (`docs/plan-perf.md` R11). A row is bound as it is — a plan's row, a
// lookup's, an `update`'s old row, a `get`'s — and a field of it is read
// by position; only a use of the whole row as a value builds the struct
// it is (§4), and that is the one copy a row costs the evaluator.
#[derive(Clone, Copy)]
enum Place<'a> {
    Value(&'a Value),
    Row(&'a Row),
}

impl<'a> Place<'a> {
    fn val(self) -> Val<'a> {
        match self {
            Place::Value(v) => Val::Ref(v),
            Place::Row(r) => Val::Row(r),
        }
    }
}

// An expression's value, borrowed where it already exists and owned
// otherwise: a `Cow` that can also hold a row (`eval_val`).
#[derive(Clone)]
pub(crate) enum Val<'a> {
    Ref(&'a Value),
    Own(Value),
    Row(&'a Row),
    OwnRow(Row),
}

impl<'a> Val<'a> {
    fn place(&self) -> Place<'_> {
        match self {
            Val::Ref(v) => Place::Value(v),
            Val::Own(v) => Place::Value(v),
            Val::Row(r) => Place::Row(r),
            Val::OwnRow(r) => Place::Row(r),
        }
    }

    fn is_null(&self) -> bool {
        matches!(self, Val::Ref(Value::Null) | Val::Own(Value::Null))
    }

    // As a value: a row becomes the struct it is.
    fn cow(self) -> Cow<'a, Value> {
        match self {
            Val::Ref(v) => Cow::Borrowed(v),
            Val::Own(v) => Cow::Owned(v),
            Val::Row(r) => Cow::Owned(r.to_value()),
            Val::OwnRow(r) => Cow::Owned(r.into_value()),
        }
    }
}

// A function's arguments: a procedure's checked input by name, or a
// helper's, as the call evaluated them — borrowed where the caller's
// expression was a binder, an argument or a field of one, so passing a
// row to a helper does not copy it, and never gathered into a map.
#[derive(Clone, Copy)]
enum Params<'a> {
    Named(&'a Args),
    Call(&'a [(String, Field)], &'a [Val<'a>]),
}

impl<'a> Env<'a> {
    fn local(&self, x: Sym) -> Option<Place<'a>> {
        let mut at = self.locals;
        while let Some(f) = at {
            if f.sym == x {
                return Some(f.value);
            }
            at = f.up;
        }
        self.node.iter().rev().find(|(s, _)| *s == x).map(|(_, v)| v.place())
    }

    fn arg(&self, a: &str) -> Option<Place<'a>> {
        match self.args {
            Params::Named(m) => m.get(a).map(Place::Value),
            // The last of a name, as collecting the pairs into a map kept.
            Params::Call(names, vals) => names.iter().rposition(|(n, _)| n == a).map(|i| vals[i].place()),
        }
    }

    // This scope with one more local: `frame`, pushed over the rest.
    fn under<'b>(&self, frame: &'b Frame<'b>) -> Env<'b>
    where
        'a: 'b,
    {
        Env {
            schema: self.schema,
            helpers: self.helpers,
            kind: self.kind,
            ctx: self.ctx,
            args: self.args,
            autos: self.autos,
            provided: self.provided,
            node: self.node,
            locals: Some(frame),
            wide: false,
        }
    }
}

// The store as it stands and the changes made so far.
struct St<'a> {
    store: &'a mut dyn Store,
    changes: Vec<Change>,
}

/// §6.1 Apply a mutator to a store by name, through its current closure.
pub fn apply(m: &Module, name: &str, ctx: &Ctx, autos: &Args, args: &Args, store: &mut dyn Store) -> Result<Result<Vec<Change>, Refusal>, EvalError> {
    let f = function(m, name)?;
    apply_closure(&m.schema, &closure(m, f), ctx, autos, args, store)
}

/// §6.1 Apply a closure to a store: how an entry replays, by the hash it
/// recorded. Outer `Err` is a bug; inner `Err` is the verdict; `Ok` is the
/// changes made, in order, already applied to the store. On a verdict the
/// store is unchanged — the whole entry rolls back, as one transaction.
pub fn apply_closure(
    sch: &Schema,
    c: &Closure,
    ctx: &Ctx,
    autos: &Args,
    args: &Args,
    store: &mut dyn Store,
) -> Result<Result<Vec<Change>, Refusal>, EvalError> {
    let f = &c.function;
    if f.kind != FnKind::Mutator {
        return Err(EvalError::WrongKind(f.name.clone(), f.kind));
    }
    for (a, _) in &f.autos {
        if !autos.contains_key(a) {
            return Err(EvalError::MissingAuto(a.clone()));
        }
    }
    let outcome = {
        let mut overlay = Overlay::new(&*store);
        let mut st = St {
            store: &mut overlay,
            changes: Vec::new(),
        };
        let r = procedure(&mut st, sch, c, ctx, autos, args);
        (r, st.changes)
    };
    match outcome {
        (Ok(_), changes) => {
            store.apply_changes(&changes);
            Ok(Ok(changes))
        }
        (Err(EvalFault::Verdict(r)), _) => Ok(Err(r)),
        (Err(EvalFault::Bug(e)), _) => Err(e),
    }
}

/// §6.2 Run a query by name, as nobody in particular. A query never changes
/// the store; it may refuse — a failing input check, a middleware, a
/// `refuse` — and that verdict is shown and recorded nowhere.
pub fn query(m: &Module, name: &str, args: &Args, store: &dyn Store) -> Result<Value, EvalFault> {
    query_as(m, name, &Ctx::default(), args, store)
}

/// §6.2 Run a query by name, as someone: a query's middleware and body
/// read `ctx.user` like a mutator's.
pub fn query_as(m: &Module, name: &str, ctx: &Ctx, args: &Args, store: &dyn Store) -> Result<Value, EvalFault> {
    let f = function(m, name).map_err(EvalFault::Bug)?;
    query_closure(&m.schema, &closure(m, f), ctx, args, store)
}

/// §6.2 Run a query closure.
pub fn query_closure(sch: &Schema, c: &Closure, ctx: &Ctx, args: &Args, store: &dyn Store) -> Result<Value, EvalFault> {
    let f = &c.function;
    if f.kind != FnKind::Query {
        return Err(EvalFault::Bug(EvalError::WrongKind(f.name.clone(), f.kind)));
    }
    let mut overlay = Overlay::new(store);
    let mut st = St {
        store: &mut overlay,
        changes: Vec::new(),
    };
    match procedure(&mut st, sch, c, ctx, &Args::new(), args)? {
        Some(v) => Ok(v),
        None => Err(EvalFault::Bug(EvalError::NoReturn(f.name.clone()))),
    }
}

/// §1.7 What a procedure runs before its body: its input checked, then
/// each middleware in `uses` order, over `store`. The checked input and
/// what each provide returned, by name — or the verdict. A maintained query
/// runs this at hydrate, builds its plan's scope from it, and runs it again
/// when a table it reads ([`crate::ir::reads`]) moves (`docs/plan-v4.md`
/// §1.7). Nothing is written.
pub fn middleware(sch: &Schema, c: &Closure, ctx: &Ctx, args: &Args, store: &dyn Store) -> Result<(Args, Args), EvalFault> {
    let mut overlay = Overlay::new(store);
    let mut st = St {
        store: &mut overlay,
        changes: Vec::new(),
    };
    preamble(&mut st, sch, c, ctx, args)
}

// A procedure, whole: the input decoded and checked, each middleware in
// `uses` order, the body. What the body returned, for a query.
fn procedure(st: &mut St, sch: &Schema, c: &Closure, ctx: &Ctx, autos: &Args, args0: &Args) -> Result<Option<Value>, EvalFault> {
    let f = &c.function;
    let (args, provided) = preamble(st, sch, c, ctx, args0)?;
    let env = Env {
        schema: sch,
        helpers: &c.helpers,
        kind: f.kind,
        ctx,
        args: Params::Named(&args),
        autos,
        provided: &provided,
        node: &[],
        locals: None,
        wide: false,
    };
    // §1.4 A query is its plan: what `pull` answers, over the store the
    // middleware saw.
    if let (FnKind::Query, Some(p)) = (f.kind, &f.plan) {
        return Ok(Some(Value::List(crate::view::read(sch, p, &Scope::of(&env), &*st.store)?)));
    }
    match block(st, &env, &f.body) {
        Ok(()) => Ok(None),
        Err(Stop::Returned(v)) => Ok(v),
        Err(Stop::Halt(fault)) => Err(fault),
    }
}

// The input checked, then the middleware: the checked input and the
// provided values.
fn preamble(st: &mut St, sch: &Schema, c: &Closure, ctx: &Ctx, args0: &Args) -> Result<(Args, Args), EvalFault> {
    let f = &c.function;
    let args = checked_input(st, sch, c, ctx, args0)?;
    let none = Args::new();
    let mut provided = Args::new();
    for u in &f.uses {
        let Some(mw) = c.helpers.iter().find(|h| h.name == *u) else {
            return Err(EvalFault::Bug(EvalError::UnknownFunction(u.clone())));
        };
        if !mw.kind.is_middleware() {
            return Err(EvalFault::Bug(EvalError::WrongKind(u.clone(), mw.kind)));
        }
        let env = Env {
            schema: sch,
            helpers: &c.helpers,
            kind: mw.kind,
            ctx,
            args: Params::Named(&args),
            autos: &none,
            provided: &provided,
            node: &[],
            locals: None,
            wide: false,
        };
        match block(st, &env, &mw.body) {
            Ok(()) | Err(Stop::Returned(None)) if mw.kind == FnKind::Guard => {}
            Err(Stop::Returned(Some(v))) if mw.kind == FnKind::Provide => {
                provided.insert(u.clone(), v);
            }
            Ok(()) | Err(Stop::Returned(_)) => return Err(EvalFault::Bug(EvalError::NoReturn(u.clone()))),
            Err(Stop::Halt(fault)) => return Err(fault),
        }
    }
    Ok((args, provided))
}

// §1.3 The input, decoded and checked: every declared field present, each
// field's checks in order (a `trim` rewriting the value the later checks
// and the body see), then the refinements over the whole input.
fn checked_input(st: &mut St, sch: &Schema, c: &Closure, ctx: &Ctx, args0: &Args) -> Result<Args, EvalFault> {
    let f = &c.function;
    let mut args = args0.clone();
    for (n, _) in &f.input {
        if !args.contains_key(n) {
            return Err(EvalFault::Bug(EvalError::MissingArg(n.clone())));
        }
    }
    let none = Args::new();
    for (n, fd) in &f.input {
        let v = args[n].clone();
        let exists = |t: &str, k: &Value| st.store.exists(t, std::slice::from_ref(k));
        let checked = {
            let store_exists = exists;
            let empty_store = crate::store::MemoryStore::empty(sch.clone());
            let mut refine = |e: &Expr, v: &Value| -> Result<bool, EvalFault> {
                let mut local = args.clone();
                local.insert(n.clone(), v.clone());
                let env = Env {
                    schema: sch,
                    helpers: &c.helpers,
                    kind: FnKind::Helper,
                    ctx,
                    args: Params::Named(&local),
                    autos: &none,
                    provided: &none,
                    node: &[],
                    locals: None,
                    wide: false,
                };
                let mut empty = St {
                    store: &mut Overlay::new(&empty_store),
                    changes: vec![],
                };
                pure_bool(&mut empty, &env, e)
            };
            check_field(n, fd, v, &store_exists, &mut refine)?
        };
        match checked {
            (v, None) => {
                args.insert(n.clone(), v);
            }
            (_, Some(msg)) => return Err(EvalFault::Verdict(Refusal::Refused(msg))),
        }
    }
    for (e, why) in &f.refine {
        let env = Env {
            schema: sch,
            helpers: &c.helpers,
            kind: FnKind::Helper,
            ctx,
            args: Params::Named(&args),
            autos: &none,
            provided: &none,
            node: &[],
            locals: None,
            wide: false,
        };
        if !pure_bool(st, &env, e)? {
            return Err(EvalFault::Verdict(Refusal::Refused(why.clone().unwrap_or_else(|| "invalid".into()))));
        }
    }
    Ok(args)
}

// A check's expression: pure, a Bool.
fn pure_bool(st: &mut St, env: &Env, e: &Expr) -> Result<bool, EvalFault> {
    match eval(st, env, e) {
        Ok(Value::Bool(b)) => Ok(b),
        Ok(other) => Err(EvalFault::Bug(EvalError::TypeError(format!("a check is a Bool, not {other:?}")))),
        Err(Stop::Halt(fault)) => Err(fault),
        Err(Stop::Returned(_)) => Err(EvalFault::Bug(EvalError::TypeError("a return inside a check".into()))),
    }
}

/// §1.3 The message a check refuses with when it names none
/// (`Ark.Eval.defaultMessage`); every runtime copies these words.
pub fn default_message(field: &str, check: &Check, ty: &Ty) -> String {
    match check {
        Check::Trim => format!("{field}: invalid"),
        Check::MinLen(n, _) => format!("{field}: at least {n} characters"),
        Check::MaxLen(n, _) => format!("{field}: at most {n} characters"),
        Check::Range(Some(lo), Some(hi), _) => format!("{field}: between {lo} and {hi}"),
        Check::Range(Some(lo), None, _) => format!("{field}: at least {lo}"),
        Check::Range(None, Some(hi), _) => format!("{field}: at most {hi}"),
        Check::Range(None, None, _) => format!("{field}: invalid"),
        Check::NonEmpty(_) => format!("{field}: at least one"),
        Check::Exists(_) => format!("{field}: no such {}", id_table(ty).unwrap_or("row")),
        Check::Refine(_, _) => format!("{field}: invalid"),
    }
}

/// The table an id field names, through an option.
pub fn id_table(ty: &Ty) -> Option<&str> {
    match ty {
        Ty::Id(t) => Some(t),
        Ty::Option(t) => id_table(t),
        _ => None,
    }
}

fn message(field: &str, check: &Check, ty: &Ty) -> String {
    let given = match check {
        Check::Trim => None,
        Check::MinLen(_, w) | Check::MaxLen(_, w) | Check::Range(_, _, w) | Check::NonEmpty(w) | Check::Exists(w) | Check::Refine(_, w) => w.as_ref(),
    };
    given.cloned().unwrap_or_else(|| default_message(field, check, ty))
}

/// §1.3 One field's checks, in order, over its value: the value as
/// normalised so far (a `trim` rewrites it for every later check and for
/// the body) and the message of the first check that fails, if one does;
/// `Err` for a fault. A field of an option type is checked only when it is `Some`.
/// `exists` answers whether a row of that table has that key; `refine`
/// evaluates a refinement over the value as it stands. Shared by the
/// interpreter, a native procedure and the form validator.
pub fn check_field(
    name: &str,
    field: &Field,
    value: Value,
    exists: &dyn Fn(&str, &Value) -> bool,
    refine: &mut dyn FnMut(&Expr, &Value) -> Result<bool, EvalFault>,
) -> Result<(Value, Option<String>), EvalFault> {
    check_field_with(name, field, value, exists, &mut |i, v| match &field.checks[i] {
        Check::Refine(e, _) => refine(e, v),
        _ => Ok(true),
    })
}

/// [`check_field`], with each refinement named by its position among the
/// field's checks: what a native procedure answers with its own closures.
pub fn check_field_with(
    name: &str,
    field: &Field,
    value: Value,
    exists: &dyn Fn(&str, &Value) -> bool,
    refine: &mut dyn FnMut(usize, &Value) -> Result<bool, EvalFault>,
) -> Result<(Value, Option<String>), EvalFault> {
    if value.is_null() && matches!(field.ty, Ty::Option(_)) {
        return Ok((value, None));
    }
    let bad = |what: &str| EvalFault::Bug(EvalError::TypeError(format!("{name}: {what}")));
    let mut v = value;
    for (i, c) in field.checks.iter().enumerate() {
        let ok = match c {
            Check::Trim => match &v {
                Value::Text(_) => {
                    let trimmed = stdlib::std(crate::ir::StdFn::Trim, &[&v]).map_err(|_| bad("trim"))?;
                    v = trimmed;
                    true
                }
                _ => return Err(bad("trim of a value that is not text")),
            },
            Check::MinLen(n, _) | Check::MaxLen(n, _) => {
                let len = match &v {
                    Value::Text(t) => t.chars().count() as i64,
                    _ => return Err(bad("a length check on a value that is not text")),
                };
                if matches!(c, Check::MinLen(..)) {
                    len >= *n
                } else {
                    len <= *n
                }
            }
            Check::Range(lo, hi, _) => match &v {
                Value::Int(x) => lo.is_none_or(|lo| *x >= lo) && hi.is_none_or(|hi| *x <= hi),
                _ => return Err(bad("a range on a value that is not an int")),
            },
            Check::NonEmpty(_) => match &v {
                Value::List(xs) => !xs.is_empty(),
                _ => return Err(bad("non_empty on a value that is not a list")),
            },
            Check::Exists(_) => match id_table(&field.ty) {
                Some(t) => exists(t, &v),
                None => return Err(bad("exists on a value that is not an id")),
            },
            Check::Refine(_, _) => refine(i, &v)?,
        };
        if !ok {
            return Ok((v, Some(message(name, c, &field.ty))));
        }
    }
    Ok((v, None))
}

/// What the form validator says about a partial input: a message per field
/// that fails, and every present field's value as normalised.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Checked {
    /// Field (or `""` for a refinement over the whole input), message; in
    /// input order.
    pub messages: Vec<(String, String)>,
    pub values: Args,
}

/// §1.3 The form validator (`check(schema, partial input, db)`): each field
/// present is run through its checks, `trim` normalising; the refinements
/// over the whole input run only when every field is present and passed.
/// Nothing is refused and nothing is written.
pub fn check(sch: &Schema, c: &Closure, ctx: &Ctx, partial: &Args, store: &dyn Store) -> Result<Checked, EvalError> {
    let f = &c.function;
    let mut out = Checked::default();
    let empty = crate::store::MemoryStore::empty(sch.clone());
    let none = Args::new();
    let mut all = true;
    for (n, fd) in &f.input {
        let Some(v) = partial.get(n) else {
            all = false;
            continue;
        };
        let exists = |t: &str, k: &Value| store.exists(t, std::slice::from_ref(k));
        let values = &out.values;
        let mut refine = |e: &Expr, v: &Value| -> Result<bool, EvalFault> {
            let mut local = values.clone();
            local.extend(
                partial
                    .iter()
                    .filter(|(k, _)| !values.contains_key(*k))
                    .map(|(k, v)| (k.clone(), v.clone())),
            );
            local.insert(n.clone(), v.clone());
            let env = Env {
                schema: sch,
                helpers: &c.helpers,
                kind: FnKind::Helper,
                ctx,
                args: Params::Named(&local),
                autos: &none,
                provided: &none,
                node: &[],
                locals: None,
                wide: false,
            };
            let mut st = St {
                store: &mut Overlay::new(&empty),
                changes: vec![],
            };
            pure_bool(&mut st, &env, e)
        };
        match check_field(n, fd, v.clone(), &exists, &mut refine) {
            Ok((v2, None)) => {
                out.values.insert(n.clone(), v2);
            }
            Ok((v2, Some(msg))) => {
                all = false;
                out.messages.push((n.clone(), msg));
                out.values.insert(n.clone(), v2);
            }
            Err(EvalFault::Verdict(r)) => {
                all = false;
                out.messages.push((n.clone(), refusal_text(&r)));
            }
            Err(EvalFault::Bug(e)) => return Err(e),
        }
    }
    if all {
        for (e, why) in &f.refine {
            let env = Env {
                schema: sch,
                helpers: &c.helpers,
                kind: FnKind::Helper,
                ctx,
                args: Params::Named(&out.values),
                autos: &none,
                provided: &none,
                node: &[],
                locals: None,
                wide: false,
            };
            let mut st = St {
                store: &mut Overlay::new(&empty),
                changes: vec![],
            };
            match pure_bool(&mut st, &env, e) {
                Ok(true) => {}
                Ok(false) => out.messages.push((String::new(), why.clone().unwrap_or_else(|| "invalid".into()))),
                Err(EvalFault::Verdict(r)) => out.messages.push((String::new(), refusal_text(&r))),
                Err(EvalFault::Bug(e)) => return Err(e),
            }
        }
    }
    Ok(out)
}

/// The text of a verdict as a person reads it: the message of a `refuse`,
/// otherwise the refusal shown.
pub fn refusal_text(r: &Refusal) -> String {
    match r {
        Refusal::Refused(t) => t.clone(),
        other => other.to_string(),
    }
}

/// Run a helper on its arguments, with no store at all.
pub fn eval_helper(m: &Module, name: &str, vals: Vec<Value>) -> Result<Value, EvalFault> {
    let f = function(m, name).map_err(EvalFault::Bug)?;
    let c = closure(m, f);
    let ctx = Ctx::default();
    let none = Args::new();
    let env = Env {
        schema: &m.schema,
        helpers: &c.helpers,
        kind: FnKind::Helper,
        ctx: &ctx,
        args: Params::Named(&none),
        autos: &none,
        provided: &none,
        node: &[],
        locals: None,
        wide: false,
    };
    let empty = crate::store::MemoryStore::empty(m.schema.clone());
    let mut overlay = Overlay::new(&empty);
    let mut st = St {
        store: &mut overlay,
        changes: Vec::new(),
    };
    let vals: Vec<Val> = vals.into_iter().map(Val::Own).collect();
    match call(&mut st, &env, f, &vals) {
        Ok(v) => Ok(v),
        Err(Stop::Returned(_)) => Err(EvalFault::Bug(EvalError::NoReturn(name.into()))),
        Err(Stop::Halt(fault)) => Err(fault),
    }
}

/// §6.6 Pull a plan whose right-hand sides are already values, over a
/// store: exactly what `ESelect` evaluates to. What a native procedure's
/// reads are, so that the two cannot answer differently.
pub fn select_plan(sch: &Schema, plan: &Plan, store: &dyn Store) -> Result<Vec<Value>, EvalFault> {
    crate::view::read(sch, plan, &Scope::new(sch, &[], &NOBODY, &NO_ARGS, &NO_ARGS), store)
}

/// §9.4 A plan's order made total, as the verifier makes it: the table's key
/// columns ascending — a group's `by` columns — after the author's,
/// omitting any already there as a column key; in every related plan too.
pub fn complete_order(sch: &Schema, p: &mut Plan) {
    let keys: Vec<FieldName> = match &p.source {
        crate::ir::Source::Table(t) => sch.lookup_table(t).map(|t| t.key.clone()).unwrap_or_default(),
        crate::ir::Source::Group { by, .. } => by.clone(),
    };
    for k in keys {
        if !p.order.iter().any(|(c, _)| *c == Key::Column(k.clone())) {
            p.order.push((Key::Column(k), Dir::Asc));
        }
    }
    for r in &mut p.related {
        complete_order(sch, &mut r.plan);
    }
}

static NO_ARGS: Args = BTreeMap::new();
static NOBODY: Ctx = Ctx {
    user: String::new(),
    session: String::new(),
};

/// §1.3 What the expressions of a plan are evaluated in: the function's
/// helpers, context, arguments, autos and provided values, and the locals
/// bound around the read (a mutator's; a query has none). A plan's filter
/// is evaluated here ([`Scope::eval`]); everything a node computes is
/// evaluated in a [`NodeScope`] of it, where scope is flat: the node's own
/// binders and nothing a parent bound.
pub struct Scope<'s> {
    outer: Env<'s>,
}

impl<'s> Scope<'s> {
    /// A scope with no locals: a query's, or a read outside any procedure.
    pub fn new(schema: &'s Schema, helpers: &'s [Function], ctx: &'s Ctx, args: &'s Args, provided: &'s Args) -> Scope<'s> {
        Scope {
            outer: Env {
                schema,
                helpers,
                kind: FnKind::Helper,
                ctx,
                args: Params::Named(args),
                autos: &NO_ARGS,
                provided,
                node: &[],
                locals: None,
                wide: false,
            },
        }
    }

    fn of(env: &Env<'s>) -> Scope<'s> {
        Scope { outer: *env }
    }

    pub fn schema(&self) -> &'s Schema {
        self.outer.schema
    }

    /// An expression constant for the read: a filter's right-hand side.
    pub fn eval(&self, e: &Expr) -> Result<Value, EvalFault> {
        pure(&self.outer, e)
    }

    /// A node's scope: nothing bound yet, and no read allowed (a node
    /// computes over what the plan has read; the verifier keeps reads out).
    pub fn node(&self) -> NodeScope<'s> {
        NodeScope {
            env: Env {
                kind: FnKind::Helper,
                node: &[],
                locals: None,
                wide: true,
                ..self.outer
            },
            bound: Vec::new(),
        }
    }
}

/// One node's scope: its binders, bound as the plan evaluates them — the
/// row by reference where the caller still holds it, so a node reading
/// `media.title` copies the title and nothing else (`docs/plan-perf.md`
/// R5).
#[derive(Clone)]
pub struct NodeScope<'s> {
    env: Env<'s>,
    bound: Vec<(Sym, Val<'s>)>,
}

impl<'s> NodeScope<'s> {
    pub fn bind(&mut self, x: Sym, v: Value) {
        self.bound.push((x, Val::Own(v)));
    }

    /// [`NodeScope::bind`] of a value the caller keeps.
    pub fn bind_ref(&mut self, x: Sym, v: &'s Value) {
        self.bound.push((x, Val::Ref(v)));
    }

    /// A row, bound as the store holds it: its fields are read in place and
    /// the struct it is is built only if the node uses it whole (R11).
    pub fn bind_row(&mut self, x: Sym, r: &'s Row) {
        self.bound.push((x, Val::Row(r)));
    }

    /// [`NodeScope::bind_row`] of a row the scope keeps: a lookup's.
    pub fn bind_row_owned(&mut self, x: Sym, r: Row) {
        self.bound.push((x, Val::OwnRow(r)));
    }

    pub fn eval(&self, e: &Expr) -> Result<Value, EvalFault> {
        pure(&self.env(), e)
    }

    fn env(&self) -> Env<'_> {
        Env {
            node: &self.bound,
            ..self.env
        }
    }
}

// An expression that reads nothing, over a store that holds nothing: what
// a plan's expressions and a filter's right-hand sides are.
fn pure(env: &Env, e: &Expr) -> Result<Value, EvalFault> {
    let mut none = NoStore(env.schema);
    let mut st = St {
        store: &mut none,
        changes: Vec::new(),
    };
    match eval(&mut st, env, e) {
        Ok(v) => Ok(v),
        Err(Stop::Halt(fault)) => Err(fault),
        Err(Stop::Returned(_)) => Err(EvalFault::Bug(EvalError::TypeError("a return inside an expression".into()))),
    }
}

// The store an expression that may not read is run against: empty, and
// never written (only a mutator's statements write).
struct NoStore<'a>(&'a Schema);

impl Store for NoStore<'_> {
    fn schema(&self) -> &Schema {
        self.0
    }
    fn get(&self, _: &str, _: &[Value]) -> Option<Row> {
        None
    }
    fn scan(&self, _: &str) -> Vec<Row> {
        vec![]
    }
    fn apply_change(&mut self, _: &Change) {}
    fn as_store(&self) -> &dyn Store {
        self
    }
}

fn function<'m>(m: &'m Module, name: &str) -> Result<&'m Function, EvalError> {
    m.lookup_function(name).ok_or_else(|| EvalError::UnknownFunction(name.into()))
}

// §6.3 Statements -----------------------------------------------------------

// A `let` binds for the rest of its block and no further: its frame is
// pushed here and the rest of the block runs over it, so a block's locals
// are gone when it returns, as they were when a block copied its scope.
fn block(st: &mut St, env: &Env, blk: &[Stmt]) -> Run<()> {
    let mut rest = blk;
    while let Some((s, tail)) = rest.split_first() {
        if let Stmt::Let(x, e) = s {
            let v = eval_val(st, env, e)?;
            let frame = Frame {
                sym: *x,
                value: v.place(),
                up: env.locals,
            };
            return block(st, &env.under(&frame), tail);
        }
        exec(st, env, s)?;
        rest = tail;
    }
    Ok(())
}

fn exec(st: &mut St, env: &Env, s: &Stmt) -> Run<()> {
    match s {
        Stmt::Let(..) => unreachable!("a block binds its lets"),
        Stmt::If(c, yes, no) => {
            let b = bool(eval(st, env, c)?)?;
            block(st, env, if b { yes } else { no })?;
        }
        Stmt::For(x, xs, body) => {
            let vs = eval_ref(st, env, xs)?;
            for v in list_ref(&vs)? {
                let frame = Frame {
                    sym: *x,
                    value: Place::Value(v),
                    up: env.locals,
                };
                block(st, &env.under(&frame), body)?;
            }
        }
        Stmt::Insert(t, e, on) => {
            mutating(env)?;
            let row = strct(eval(st, env, e)?)?;
            let row = store::row_for(st.store.as_store(), t, row);
            let r = store::insert(st.store, t, row, on);
            wrote(st, r)?;
        }
        Stmt::Upsert(t, e, on) => {
            mutating(env)?;
            let row = strct(eval(st, env, e)?)?;
            let row = store::row_for(st.store.as_store(), t, row);
            let r = store::upsert(st.store, t, row, on);
            wrote(st, r)?;
        }
        Stmt::Update(t, ks, x, e) => {
            mutating(env)?;
            let key = eval_many(st, env, ks)?;
            if let Some(old) = st.store.get(t, &key) {
                let frame = Frame {
                    sym: *x,
                    value: Place::Row(&old),
                    up: env.locals,
                };
                let row = strct(eval(st, &env.under(&frame), e)?)?;
                let row = store::row_for(st.store.as_store(), t, row);
                let r = store::update(st.store, t, &key, row);
                wrote(st, r)?;
            }
        }
        Stmt::Delete(t, ks) => {
            mutating(env)?;
            let key = eval_many(st, env, ks)?;
            let r = st.store.delete(t, &key);
            wrote(st, r)?;
        }
        Stmt::Refuse(e) => {
            if env.kind == FnKind::Helper {
                return bug(EvalError::Impure("refuse inside a helper".into()));
            }
            let t = text(eval(st, env, e)?)?;
            return verdict(Refusal::Refused(t));
        }
        Stmt::Return(me) => {
            let v = match me {
                Some(e) => Some(eval(st, env, e)?),
                None => None,
            };
            return Err(Stop::Returned(v));
        }
    }
    Ok(())
}

fn wrote(st: &mut St, r: Result<Option<Change>, Refusal>) -> Run<()> {
    match r {
        Err(r) => verdict(r),
        Ok(ch) => {
            st.changes.extend(ch);
            Ok(())
        }
    }
}

fn mutating(env: &Env) -> Run<()> {
    if env.kind != FnKind::Mutator {
        bug(EvalError::Impure("write outside a mutator".into()))
    } else {
        Ok(())
    }
}

fn reading(env: &Env) -> Run<()> {
    if env.kind == FnKind::Helper {
        bug(EvalError::Impure("read inside a helper".into()))
    } else {
        Ok(())
    }
}

// §6.4 Expressions ------------------------------------------------------------

// §6.4 An expression's value, borrowed where it already exists: a
// literal, an argument, an auto, a local, a provided value, or a field of
// any of these, however deep — so reading `media.title` copies the title
// and not the row (`docs/plan-perf.md` R5). Anything else is computed, and
// owned. A value is copied only where it is kept: a struct's field, a
// list's element, a helper's result, a row written.
fn eval_ref<'v>(st: &mut St, env: &Env<'v>, e: &'v Expr) -> Run<Cow<'v, Value>> {
    eval_val(st, env, e).map(Val::cow)
}

// [`eval_ref`], where a row may be the answer: a row bound as the store
// holds it, a field of one, or a `get` — so `let x = get(..)` and
// `x.title` copy the title and never build the struct (R11). A `let`, a
// `match`'s `some`, a helper's argument and a field take this path; every
// other use of a row as a value builds its struct ([`Val::cow`]).
fn eval_val<'v>(st: &mut St, env: &Env<'v>, e: &'v Expr) -> Run<Val<'v>> {
    Ok(match e {
        Expr::Lit(v) => Val::Ref(v),
        Expr::Arg(a) => match env.arg(a) {
            Some(v) => v.val(),
            None => return bug(EvalError::MissingArg(a.clone())),
        },
        Expr::Auto(a) => match env.autos.get(a) {
            Some(v) => Val::Ref(v),
            None => return bug(EvalError::MissingAuto(a.clone())),
        },
        Expr::Var(x) => match env.local(*x) {
            Some(v) => v.val(),
            None => return bug(EvalError::UnboundVar(*x)),
        },
        Expr::Provided(n) => match env.provided.get(n) {
            Some(v) => Val::Ref(v),
            None => return bug(EvalError::NotProvided(n.clone())),
        },
        Expr::Field(e, f) => match eval_val(st, env, e)? {
            Val::Ref(Value::Struct(m)) => match m.get(f) {
                Some(v) => Val::Ref(v),
                None => return bug(EvalError::NoSuchField(f.clone())),
            },
            Val::Own(Value::Struct(mut m)) => match m.remove(f) {
                Some(v) => Val::Own(v),
                None => return bug(EvalError::NoSuchField(f.clone())),
            },
            Val::Row(r) => match r.get(f) {
                Some(v) => Val::Ref(v),
                None => return bug(EvalError::NoSuchField(f.clone())),
            },
            Val::OwnRow(r) => match r.get(f) {
                Some(v) => Val::Own(v.clone()),
                None => return bug(EvalError::NoSuchField(f.clone())),
            },
            other => return type_error("Struct", &other.cow()),
        },
        // An option is flat, and `if` evaluates only the taken arm: either
        // is the value of the expression it leads to.
        Expr::Some(e) => return eval_val(st, env, e),
        Expr::If(c, a, b) => {
            let t = bool(eval(st, env, c)?)?;
            return eval_val(st, env, if t { a } else { b });
        }
        Expr::Get(t, ks) => {
            reading(env)?;
            let key = eval_many(st, env, ks)?;
            match st.store.get(t, &key) {
                Some(r) => Val::OwnRow(r),
                None => Val::Own(Value::Null),
            }
        }
        _ => Val::Own(eval(st, env, e)?),
    })
}

fn eval(st: &mut St, env: &Env, e: &Expr) -> Run<Value> {
    match e {
        Expr::Lit(_) | Expr::Arg(_) | Expr::Auto(_) | Expr::Var(_) | Expr::Provided(_) | Expr::Field(..) | Expr::Get(..) => {
            eval_ref(st, env, e).map(Cow::into_owned)
        }
        Expr::CtxUser => Ok(Value::Text(env.ctx.user.clone())),
        Expr::CtxSession => Ok(Value::Text(env.ctx.session.clone())),
        // Fields are evaluated in field-name order, which is the map's order.
        Expr::Struct(fs) => {
            let mut m = BTreeMap::new();
            for (k, e) in fs {
                m.insert(k.clone(), eval(st, env, e)?);
            }
            Ok(Value::Struct(m))
        }
        Expr::List(es) => Ok(Value::List(eval_many(st, env, es)?)),
        // An option is flat: `Some v` is `v` and `None` is `Null`.
        Expr::Some(e) => eval(st, env, e),
        Expr::None(_) => Ok(Value::Null),
        Expr::Match(e, x, some, none) => {
            let v = eval_val(st, env, e)?;
            if v.is_null() {
                eval(st, env, none)
            } else {
                let frame = Frame {
                    sym: *x,
                    value: v.place(),
                    up: env.locals,
                };
                eval(st, &env.under(&frame), some)
            }
        }
        Expr::If(c, a, b) => {
            let t = bool(eval(st, env, c)?)?;
            eval(st, env, if t { a } else { b })
        }
        Expr::Op(Op::And, es) if es.len() == 2 => {
            let x = bool(eval(st, env, &es[0])?)?;
            if x {
                eval(st, env, &es[1])
            } else {
                Ok(Value::Bool(false))
            }
        }
        Expr::Op(Op::Or, es) if es.len() == 2 => {
            let x = bool(eval(st, env, &es[0])?)?;
            if x {
                Ok(Value::Bool(true))
            } else {
                eval(st, env, &es[1])
            }
        }
        Expr::Op(Op::Not, es) if es.len() == 1 => Ok(Value::Bool(!bool(eval(st, env, &es[0])?)?)),
        Expr::Op(Op::Neg, es) if es.len() == 1 => {
            let n = int(eval(st, env, &es[0])?)?;
            match n.checked_neg() {
                Some(m) => Ok(Value::Int(m)),
                None => verdict(Refusal::Refused("integer overflow".into())),
            }
        }
        Expr::Op(op @ (Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Mod), es) if es.len() == 2 => {
            let x = int(eval(st, env, &es[0])?)?;
            let y = int(eval(st, env, &es[1])?)?;
            match arith(*op, x, y) {
                Ok(n) => Ok(Value::Int(n)),
                Err(t) => verdict(Refusal::Refused(t.into())),
            }
        }
        Expr::Op(op, _) => bug(EvalError::Arity(op.show().into())),
        Expr::Cmp(op, a, b) => {
            let x = eval_ref(st, env, a)?;
            let y = eval_ref(st, env, b)?;
            Ok(Value::Bool(cmp(*op, &x, &y)))
        }
        Expr::Call(name, es) => {
            let mut vals = Vec::with_capacity(es.len());
            for e in es {
                vals.push(eval_val(st, env, e)?);
            }
            let Some(f) = env.helpers.iter().find(|h| h.name == *name) else {
                return bug(EvalError::UnknownFunction(name.clone()));
            };
            if f.kind != FnKind::Helper {
                return bug(EvalError::WrongKind(name.clone(), f.kind));
            }
            call(st, env, f, &vals)
        }
        // Its arguments are read in place (Round 4): a standard function
        // copies only what it answers, so `len` of a local list counts the
        // list where it is bound, and its arity is at most three, so they
        // are held on the stack.
        Expr::Std(f, es) => {
            let out = match es.as_slice() {
                [] => stdlib::std::<Value>(*f, &[]),
                [a] => stdlib::std(*f, &[eval_ref(st, env, a)?]),
                [a, b] => {
                    let a = eval_ref(st, env, a)?;
                    stdlib::std(*f, &[a, eval_ref(st, env, b)?])
                }
                [a, b, c] => {
                    let a = eval_ref(st, env, a)?;
                    let b = eval_ref(st, env, b)?;
                    stdlib::std(*f, &[a, b, eval_ref(st, env, c)?])
                }
                more => {
                    let mut vals = Vec::with_capacity(more.len());
                    for e in more {
                        vals.push(eval_ref(st, env, e)?);
                    }
                    stdlib::std(*f, &vals)
                }
            };
            match out {
                Ok(v) => Ok(v),
                Err(StdError::Fault(t)) => verdict(Refusal::Refused(t)),
                Err(StdError::Arity(g, n)) => bug(EvalError::Arity(format!("{}/{n}", g.show()))),
                Err(StdError::TypeMismatch(g)) => bug(EvalError::TypeError(g.show().into())),
            }
        }
        // The list functions bind each element by reference into the list,
        // which is itself borrowed when it is a local, an argument or a
        // field of one: an element is copied only into what is kept.
        Expr::Map(xs, x, body) => {
            let vs = eval_ref(st, env, xs)?;
            let vs = list_ref(&vs)?;
            let mut out = Vec::with_capacity(vs.len());
            for v in vs {
                out.push(eval(st, &env.under(&frame(*x, v, env)), body)?);
            }
            Ok(Value::List(out))
        }
        Expr::Filter(xs, x, body) => {
            let vs = eval_ref(st, env, xs)?;
            let mut keep = Vec::new();
            for v in list_ref(&vs)? {
                keep.push(bool(eval(st, &env.under(&frame(*x, v, env)), body)?)?);
            }
            Ok(Value::List(picked(vs, keep.into_iter().enumerate().filter(|(_, k)| *k).map(|(i, _)| i))))
        }
        // Every element is evaluated, as the spec's `mapM` does; the result
        // is the disjunction (conjunction).
        Expr::Any(xs, x, body) => {
            let vs = eval_ref(st, env, xs)?;
            let mut acc = false;
            for v in list_ref(&vs)? {
                acc |= bool(eval(st, &env.under(&frame(*x, v, env)), body)?)?;
            }
            Ok(Value::Bool(acc))
        }
        Expr::All(xs, x, body) => {
            let vs = eval_ref(st, env, xs)?;
            let mut acc = true;
            for v in list_ref(&vs)? {
                acc &= bool(eval(st, &env.under(&frame(*x, v, env)), body)?)?;
            }
            Ok(Value::Bool(acc))
        }
        // Stable, under compare_value of the key.
        Expr::SortBy(xs, x, key) => {
            let vs = eval_ref(st, env, xs)?;
            let mut keys = Vec::new();
            for v in list_ref(&vs)? {
                keys.push(eval(st, &env.under(&frame(*x, v, env)), key)?);
            }
            let mut order: Vec<usize> = (0..keys.len()).collect();
            order.sort_by(|a, b| keys[*a].cmp(&keys[*b]));
            Ok(Value::List(picked(vs, order.into_iter())))
        }
        Expr::Fold(xs, z, acc, x, body) => {
            let vs = eval_ref(st, env, xs)?;
            let vs = list_ref(&vs)?;
            let mut a = eval(st, env, z)?;
            // §13 In a plan's node a sum is arithmetic over the integers:
            // `init` and every term added in an `i128` and the total checked
            // once — an overflow on the way, which depends on the members'
            // order, is not one (`docs/plan-db.md` D4). Each term is still
            // checked as the evaluator checks anything.
            if let (true, Some(f), Value::Int(init)) = (env.wide, sum_step(body, *acc), &a) {
                let mut total = i128::from(*init);
                for v in vs {
                    let with_x = frame(*x, v, env);
                    total += i128::from(int(eval(st, &env.under(&with_x), f)?)?);
                }
                return match i64::try_from(total) {
                    Ok(n) => Ok(Value::Int(n)),
                    Err(_) => verdict(Refusal::Refused("integer overflow".into())),
                };
            }
            for v in vs {
                let with_acc = frame(*acc, &a, env);
                let with_x = Frame {
                    sym: *x,
                    value: Place::Value(v),
                    up: Some(&with_acc),
                };
                a = eval(st, &env.under(&with_x), body)?;
            }
            Ok(a)
        }
        Expr::Select(p) => {
            reading(env)?;
            match crate::view::read(env.schema, p, &Scope::of(env), &*st.store) {
                Ok(rows) => Ok(Value::List(rows)),
                Err(fault) => Err(Stop::Halt(fault)),
            }
        }
        Expr::Exists(t, ks) => {
            reading(env)?;
            let key = eval_many(st, env, ks)?;
            Ok(Value::Bool(st.store.exists(t, &key)))
        }
    }
}

/// §13 The term of a fold whose step is a sum — `acc + f` or `f + acc`,
/// `f` not reading the accumulator — which is the shape whose result alone
/// is judged in a plan's node (`docs/plan-db.md` D4). The view recognises
/// the same step when it keeps a sum as a number.
pub fn sum_step(body: &Expr, acc: Sym) -> Option<&Expr> {
    fn mentions(e: &Expr, x: Sym) -> bool {
        match e {
            Expr::Var(s) => *s == x,
            _ => crate::view::children(e).into_iter().any(|c| mentions(c, x)),
        }
    }
    match body {
        Expr::Op(Op::Add, es) if es.len() == 2 => match (&es[0], &es[1]) {
            (Expr::Var(a), f) | (f, Expr::Var(a)) if *a == acc && !mentions(f, acc) => Some(f),
            _ => None,
        },
        _ => None,
    }
}

// A frame binding `x` to `v` over `env`'s locals.
fn frame<'b>(x: Sym, v: &'b Value, env: &Env<'b>) -> Frame<'b> {
    Frame {
        sym: x,
        value: Place::Value(v),
        up: env.locals,
    }
}

// The elements of a list at `at`, in that order: moved out of a list the
// evaluation owns, copied out of one it borrows.
fn picked(vs: Cow<Value>, at: impl Iterator<Item = usize>) -> Vec<Value> {
    match vs {
        Cow::Owned(Value::List(xs)) => {
            let mut slots: Vec<Option<Value>> = xs.into_iter().map(Some).collect();
            at.filter_map(|i| slots[i].take()).collect()
        }
        other => match &*other {
            Value::List(xs) => at.map(|i| xs[i].clone()).collect(),
            _ => vec![],
        },
    }
}

fn eval_many(st: &mut St, env: &Env, es: &[Expr]) -> Run<Vec<Value>> {
    let mut out = Vec::with_capacity(es.len());
    for e in es {
        out.push(eval(st, env, e)?);
    }
    Ok(out)
}

// Call a helper: bind its arguments as a fresh environment, run its body,
// and take what it returned. Helpers never see locals, autos, arguments or
// the context of their caller.
fn call(st: &mut St, env: &Env, f: &Function, vals: &[Val]) -> Run<Value> {
    if vals.len() != f.input.len() {
        return bug(EvalError::Arity(f.name.clone()));
    }
    let env2 = Env {
        schema: env.schema,
        helpers: env.helpers,
        kind: FnKind::Helper,
        ctx: env.ctx,
        args: Params::Call(&f.input, vals),
        autos: &NO_ARGS,
        provided: &NO_ARGS,
        node: &[],
        locals: None,
        wide: env.wide,
    };
    match block(st, &env2, &f.body) {
        Ok(()) | Err(Stop::Returned(None)) => bug(EvalError::NoReturn(f.name.clone())),
        Err(Stop::Returned(Some(v))) => Ok(v),
        Err(other) => Err(other),
    }
}

/// §6.5 Checked arithmetic (`Ark.Eval.arith`). Division truncates toward
/// zero and the remainder takes the dividend's sign; `MIN / -1` and
/// `MIN % -1` are overflows. Every fault here is the same text in every
/// runtime, because it may become a refusal recorded against an entry.
pub fn arith(op: Op, x: i64, y: i64) -> Result<i64, &'static str> {
    const OVERFLOW: &str = "integer overflow";
    match op {
        Op::Add => x.checked_add(y).ok_or(OVERFLOW),
        Op::Sub => x.checked_sub(y).ok_or(OVERFLOW),
        Op::Mul => x.checked_mul(y).ok_or(OVERFLOW),
        Op::Div | Op::Mod => {
            if y == 0 {
                Err("division by zero")
            } else if x == i64::MIN && y == -1 {
                Err(OVERFLOW)
            } else if op == Op::Div {
                Ok(x / y)
            } else {
                Ok(x % y)
            }
        }
        _ => Err("not an arithmetic operator"),
    }
}

// Coercions: a bug when the verifier's type does not hold ---------------------

fn type_error<T>(want: &str, v: &Value) -> Run<T> {
    let shown: String = format!("{v:?}").chars().take(60).collect();
    bug(EvalError::TypeError(format!("expected {want}, got {shown}")))
}

fn bool(v: Value) -> Run<bool> {
    match v {
        Value::Bool(b) => Ok(b),
        other => type_error("Bool", &other),
    }
}

fn int(v: Value) -> Run<i64> {
    match v {
        Value::Int(n) => Ok(n),
        other => type_error("Int", &other),
    }
}

fn text(v: Value) -> Run<String> {
    match v {
        Value::Text(t) => Ok(t),
        other => type_error("Text", &other),
    }
}

fn list_ref(v: &Value) -> Run<&[Value]> {
    match v {
        Value::List(xs) => Ok(xs),
        other => type_error("List", other),
    }
}

fn strct(v: Value) -> Run<BTreeMap<FieldName, Value>> {
    match v {
        Value::Struct(m) => Ok(m),
        other => type_error("Struct", &other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_is_checked() {
        assert_eq!(arith(Op::Add, i64::MAX, 1), Err("integer overflow"));
        assert_eq!(arith(Op::Div, 7, 0), Err("division by zero"));
        assert_eq!(arith(Op::Div, i64::MIN, -1), Err("integer overflow"));
        assert_eq!(arith(Op::Mod, i64::MIN, -1), Err("integer overflow"));
        assert_eq!(arith(Op::Div, -7, 2), Ok(-3));
        assert_eq!(arith(Op::Mod, -7, 2), Ok(-1));
    }
}
