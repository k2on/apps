//! §6 Evaluation: what a domain means (`Ark.Eval`).
//!
//! An interpreter of the IR over any [`Store`]. A procedure authored in the
//! vocabulary of `spec/AUTHORING.md` and run `Native`
//! ([`crate::authoring`]) is the fast path and must agree with this file
//! over its own `Emit`; the conformance runner checks the `eval/` vectors
//! through it, and a peer uses it for a closure it received but holds no
//! native procedure for.
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

use std::collections::BTreeMap;

use crate::hash::{closure, Closure};
use crate::ir::{Block, Check, CmpOp, Expr, Field, FnKind, Function, Module, Op, Plan, Pred, Related, Stmt, Sym};
use crate::schema::{Dir, Schema, Table, Ty};
use crate::stdlib::{self, StdError};
use crate::store::{self, Change, Overlay, Refusal, Row, Store};
use crate::value::{FieldName, TableName, Value};
use crate::view::{cmp, order_by};

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

#[derive(Clone)]
struct Env<'a> {
    schema: &'a Schema,
    /// The helpers and middleware in reach: a closure's, never a module's.
    helpers: &'a [Function],
    kind: FnKind,
    ctx: &'a Ctx,
    args: &'a Args,
    autos: &'a Args,
    /// What each `Provide` the procedure has run so far returned.
    provided: &'a Args,
    locals: BTreeMap<Sym, Value>,
}

impl Env<'_> {
    fn bind(&self, x: Sym, v: Value) -> Self {
        let mut e = self.clone();
        e.locals.insert(x, v);
        e
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

// A procedure, whole: the input decoded and checked, each middleware in
// `uses` order, the body. What the body returned, for a query.
fn procedure(st: &mut St, sch: &Schema, c: &Closure, ctx: &Ctx, autos: &Args, args0: &Args) -> Result<Option<Value>, EvalFault> {
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
            args: &args,
            autos: &none,
            provided: &provided,
            locals: BTreeMap::new(),
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
    let env = Env {
        schema: sch,
        helpers: &c.helpers,
        kind: f.kind,
        ctx,
        args: &args,
        autos,
        provided: &provided,
        locals: BTreeMap::new(),
    };
    match block(st, &env, &f.body) {
        Ok(()) => Ok(None),
        Err(Stop::Returned(v)) => Ok(v),
        Err(Stop::Halt(fault)) => Err(fault),
    }
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
                    args: &local,
                    autos: &none,
                    provided: &none,
                    locals: BTreeMap::new(),
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
            args: &args,
            autos: &none,
            provided: &none,
            locals: BTreeMap::new(),
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
                Value::Text(t) => {
                    v = stdlib::std(crate::ir::StdFn::Trim, &[Value::Text(t.clone())]).map_err(|_| bad("trim"))?;
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
                args: &local,
                autos: &none,
                provided: &none,
                locals: BTreeMap::new(),
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
                args: &out.values,
                autos: &none,
                provided: &none,
                locals: BTreeMap::new(),
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
        args: &none,
        autos: &none,
        provided: &none,
        locals: BTreeMap::new(),
    };
    let empty = crate::store::MemoryStore::empty(m.schema.clone());
    let mut overlay = Overlay::new(&empty);
    let mut st = St {
        store: &mut overlay,
        changes: Vec::new(),
    };
    match call(&mut st, &env, f, vals) {
        Ok(v) => Ok(v),
        Err(Stop::Returned(_)) => Err(EvalFault::Bug(EvalError::NoReturn(name.into()))),
        Err(Stop::Halt(fault)) => Err(fault),
    }
}

/// §6.6 Pull a plan whose right-hand sides are already values, over a
/// store: exactly what `ESelect` evaluates to. What a native procedure's
/// reads are, so that the two cannot answer differently.
pub fn select_plan(sch: &Schema, plan: &Plan, store: &dyn Store) -> Result<Vec<Value>, EvalFault> {
    let ctx = Ctx::default();
    let none = Args::new();
    let env = Env {
        schema: sch,
        helpers: &[],
        kind: FnKind::Query,
        ctx: &ctx,
        args: &none,
        autos: &none,
        provided: &none,
        locals: BTreeMap::new(),
    };
    let mut overlay = Overlay::new(store);
    let mut st = St {
        store: &mut overlay,
        changes: Vec::new(),
    };
    match select(&mut st, &env, plan) {
        Ok(rows) => Ok(rows),
        Err(Stop::Halt(fault)) => Err(fault),
        Err(Stop::Returned(_)) => Err(EvalFault::Bug(EvalError::TypeError("a return inside a plan".into()))),
    }
}

/// §9.4 A plan's order made total, as the verifier makes it: the table's key
/// columns ascending, after the author's, omitting any already present.
pub fn complete_order(sch: &Schema, p: &mut Plan) {
    if let Some(t) = sch.lookup_table(&p.table) {
        for k in &t.key {
            if !p.order.iter().any(|(c, _)| c == k) {
                p.order.push((k.clone(), Dir::Asc));
            }
        }
    }
    for r in &mut p.related {
        complete_order(sch, &mut r.plan);
    }
}

fn function<'m>(m: &'m Module, name: &str) -> Result<&'m Function, EvalError> {
    m.lookup_function(name).ok_or_else(|| EvalError::UnknownFunction(name.into()))
}

// §6.3 Statements -----------------------------------------------------------

fn block(st: &mut St, env: &Env, blk: &Block) -> Run<()> {
    let mut env = env.clone();
    for s in blk {
        exec(st, &mut env, s)?;
    }
    Ok(())
}

fn exec(st: &mut St, env: &mut Env, s: &Stmt) -> Run<()> {
    match s {
        Stmt::Let(x, e) => {
            let v = eval(st, env, e)?;
            env.locals.insert(*x, v);
        }
        Stmt::If(c, yes, no) => {
            let b = bool(eval(st, env, c)?)?;
            block(st, env, if b { yes } else { no })?;
        }
        Stmt::For(x, xs, body) => {
            let vs = list(eval(st, env, xs)?)?;
            for v in vs {
                block(st, &env.bind(*x, v), body)?;
            }
        }
        Stmt::Insert(t, e, on) => {
            mutating(env)?;
            let row = strct(eval(st, env, e)?)?;
            let r = store::insert(st.store, t, row, on);
            wrote(st, r)?;
        }
        Stmt::Upsert(t, e, on) => {
            mutating(env)?;
            let row = strct(eval(st, env, e)?)?;
            let r = store::upsert(st.store, t, row, on);
            wrote(st, r)?;
        }
        Stmt::Update(t, ks, x, e) => {
            mutating(env)?;
            let key = eval_many(st, env, ks)?;
            if let Some(old) = st.store.get(t, &key) {
                let row = strct(eval(st, &env.bind(*x, Value::Struct(old)), e)?)?;
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

fn eval(st: &mut St, env: &Env, e: &Expr) -> Run<Value> {
    match e {
        Expr::Lit(v) => Ok(v.clone()),
        Expr::Arg(a) => env.args.get(a).cloned().map_or_else(|| bug(EvalError::MissingArg(a.clone())), Ok),
        Expr::Auto(a) => env.autos.get(a).cloned().map_or_else(|| bug(EvalError::MissingAuto(a.clone())), Ok),
        Expr::Var(x) => env.locals.get(x).cloned().map_or_else(|| bug(EvalError::UnboundVar(*x)), Ok),
        Expr::CtxUser => Ok(Value::Text(env.ctx.user.clone())),
        Expr::CtxSession => Ok(Value::Text(env.ctx.session.clone())),
        Expr::Provided(n) => env.provided.get(n).cloned().map_or_else(|| bug(EvalError::NotProvided(n.clone())), Ok),
        Expr::Field(e, f) => {
            let m = strct(eval(st, env, e)?)?;
            m.get(f).cloned().map_or_else(|| bug(EvalError::NoSuchField(f.clone())), Ok)
        }
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
            let v = eval(st, env, e)?;
            if v.is_null() {
                eval(st, env, none)
            } else {
                eval(st, &env.bind(*x, v), some)
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
            let x = eval(st, env, a)?;
            let y = eval(st, env, b)?;
            Ok(Value::Bool(cmp(*op, &x, &y)))
        }
        Expr::Call(name, es) => {
            let vals = eval_many(st, env, es)?;
            let Some(f) = env.helpers.iter().find(|h| h.name == *name) else {
                return bug(EvalError::UnknownFunction(name.clone()));
            };
            if f.kind != FnKind::Helper {
                return bug(EvalError::WrongKind(name.clone(), f.kind));
            }
            call(st, env, f, vals)
        }
        Expr::Std(f, es) => {
            let vals = eval_many(st, env, es)?;
            match stdlib::std(*f, &vals) {
                Ok(v) => Ok(v),
                Err(StdError::Fault(t)) => verdict(Refusal::Refused(t)),
                Err(StdError::Arity(g, n)) => bug(EvalError::Arity(format!("{}/{n}", g.show()))),
                Err(StdError::TypeMismatch(g)) => bug(EvalError::TypeError(g.show().into())),
            }
        }
        Expr::Map(xs, x, body) => {
            let vs = list(eval(st, env, xs)?)?;
            let mut out = Vec::with_capacity(vs.len());
            for v in vs {
                out.push(eval(st, &env.bind(*x, v), body)?);
            }
            Ok(Value::List(out))
        }
        Expr::Filter(xs, x, body) => {
            let vs = list(eval(st, env, xs)?)?;
            let mut out = Vec::new();
            for v in vs {
                if bool(eval(st, &env.bind(*x, v.clone()), body)?)? {
                    out.push(v);
                }
            }
            Ok(Value::List(out))
        }
        // Every element is evaluated, as the spec's `mapM` does; the result
        // is the disjunction (conjunction).
        Expr::Any(xs, x, body) => {
            let vs = list(eval(st, env, xs)?)?;
            let mut acc = false;
            for v in vs {
                acc |= bool(eval(st, &env.bind(*x, v), body)?)?;
            }
            Ok(Value::Bool(acc))
        }
        Expr::All(xs, x, body) => {
            let vs = list(eval(st, env, xs)?)?;
            let mut acc = true;
            for v in vs {
                acc &= bool(eval(st, &env.bind(*x, v), body)?)?;
            }
            Ok(Value::Bool(acc))
        }
        // Stable, under compare_value of the key.
        Expr::SortBy(xs, x, key) => {
            let vs = list(eval(st, env, xs)?)?;
            let mut keyed = Vec::with_capacity(vs.len());
            for v in vs {
                let k = eval(st, &env.bind(*x, v.clone()), key)?;
                keyed.push((v, k));
            }
            keyed.sort_by(|a, b| a.1.cmp(&b.1));
            Ok(Value::List(keyed.into_iter().map(|(v, _)| v).collect()))
        }
        Expr::Fold(xs, z, acc, x, body) => {
            let vs = list(eval(st, env, xs)?)?;
            let mut a = eval(st, env, z)?;
            for v in vs {
                a = eval(st, &env.bind(*acc, a).bind(*x, v), body)?;
            }
            Ok(a)
        }
        Expr::Select(p) => {
            reading(env)?;
            Ok(Value::List(select(st, env, p)?))
        }
        Expr::Get(t, ks) => {
            reading(env)?;
            let key = eval_many(st, env, ks)?;
            Ok(st.store.get(t, &key).map(Value::Struct).unwrap_or(Value::Null))
        }
        Expr::Exists(t, ks) => {
            reading(env)?;
            let key = eval_many(st, env, ks)?;
            Ok(Value::Bool(st.store.exists(t, &key)))
        }
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
fn call(st: &mut St, env: &Env, f: &Function, vals: Vec<Value>) -> Run<Value> {
    if vals.len() != f.input.len() {
        return bug(EvalError::Arity(f.name.clone()));
    }
    let args: Args = f.input.iter().map(|(n, _)| n.clone()).zip(vals).collect();
    let autos = Args::new();
    let env2 = Env {
        schema: env.schema,
        helpers: env.helpers,
        kind: FnKind::Helper,
        ctx: env.ctx,
        args: &args,
        autos: &autos,
        provided: &autos,
        locals: BTreeMap::new(),
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

// §6.6 Select --------------------------------------------------------------------

/// Pull a plan: scan the table, keep the rows the filter admits, sort them
/// by the order (stably), take the limit, and hang each relationship's rows
/// beneath as a field of the relationship's name. A child plan runs once per
/// parent with the join column pinned to the parent's key.
fn select(st: &mut St, env: &Env, p: &Plan) -> Run<Vec<Value>> {
    let Some(tbl) = env.schema.lookup_table(&p.table) else {
        return bug(EvalError::UnknownTable(p.table.clone()));
    };
    let keep: Keep = match &p.filter {
        None => Box::new(|_| true),
        Some(f) => predicate(st, env, f)?,
    };
    let held = match &p.filter {
        None => vec![],
        Some(f) => equalities(st, env, f)?,
    };
    let eq: Vec<(&str, &Value)> = held.iter().map(|(c, v)| (c.as_str(), v)).collect();
    let mut admitted: Vec<Row> = st.store.scan_where_eq(&p.table, &eq, &|r| keep(r));
    admitted.sort_by(|a, b| order_by(&p.order, a, b));
    if let Some(lim) = p.limit {
        admitted.truncate(lim.max(0) as usize);
    }
    let mut out = Vec::with_capacity(admitted.len());
    for row in admitted {
        out.push(attach(st, env, tbl, &p.related, row)?);
    }
    Ok(out)
}

fn attach(st: &mut St, env: &Env, tbl: &Table, rels: &[Related], row: Row) -> Run<Value> {
    let key = tbl.key_of(&row);
    let pk = match key.as_slice() {
        [k] => k.clone(),
        _ if rels.is_empty() => Value::Null,
        _ => return bug(EvalError::CompositeParentKey(tbl.name.clone())),
    };
    let mut fields = row;
    for r in rels {
        let pin = Pred::Cmp(r.relation.column.clone(), CmpOp::Eq, Expr::Lit(pk.clone()));
        let filter = match &r.plan.filter {
            None => pin,
            Some(f) => Pred::All(vec![pin, f.clone()]),
        };
        let child = Plan {
            filter: Some(filter),
            ..r.plan.clone()
        };
        let kids = select(st, env, &child)?;
        fields.insert(r.name.clone(), Value::List(kids));
    }
    Ok(Value::Struct(fields))
}

// The right-hand sides of a filter are evaluated once, before the scan.
// A compiled filter over one row.
type Keep = Box<dyn Fn(&Row) -> bool>;

// The columns a predicate holds equal to a value, the values evaluated:
// what an indexed store looks rows up by. `predicate` still decides.
fn equalities(st: &mut St, env: &Env, p: &Pred) -> Run<Vec<(FieldName, Value)>> {
    let mut out = vec![];
    match p {
        Pred::Cmp(c, CmpOp::Eq, e) => out.push((c.clone(), eval(st, env, e)?)),
        Pred::All(ps) => {
            for q in ps {
                out.extend(equalities(st, env, q)?);
            }
        }
        _ => {}
    }
    Ok(out)
}

fn predicate(st: &mut St, env: &Env, p: &Pred) -> Run<Keep> {
    Ok(match p {
        Pred::Cmp(c, op, e) => {
            let v = eval(st, env, e)?;
            let (c, op) = (c.clone(), *op);
            Box::new(move |row| cmp(op, row.get(&c).unwrap_or(&Value::Null), &v))
        }
        Pred::In(c, es) => {
            let vs = eval_many(st, env, es)?;
            let c = c.clone();
            Box::new(move |row| vs.iter().any(|v| cmp(CmpOp::Eq, row.get(&c).unwrap_or(&Value::Null), v)))
        }
        Pred::All(ps) => {
            let fs = ps.iter().map(|q| predicate(st, env, q)).collect::<Run<Vec<_>>>()?;
            Box::new(move |row| fs.iter().all(|f| f(row)))
        }
        Pred::Any(ps) => {
            let fs = ps.iter().map(|q| predicate(st, env, q)).collect::<Run<Vec<_>>>()?;
            Box::new(move |row| fs.iter().any(|f| f(row)))
        }
        Pred::Not(q) => {
            let f = predicate(st, env, q)?;
            Box::new(move |row| !f(row))
        }
    })
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

fn list(v: Value) -> Run<Vec<Value>> {
    match v {
        Value::List(xs) => Ok(xs),
        other => type_error("List", &other),
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
