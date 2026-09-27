//! §6 Evaluation: what generated code must mean (`Ark.Eval`).
//!
//! An interpreter of the IR over any [`Store`]. Generated code is the fast
//! path and must agree with it; the conformance runner checks the `eval/`
//! vectors through it, and a peer uses it for a closure it received but
//! has no generated code for.
//!
//! Two kinds of failure are kept apart: a [`Refusal`] is a verdict every
//! replica reaches; an [`EvalError`] is a bug — a module the verifier would
//! have refused, or a generator that disagrees with this file.
//!
//! Evaluation order is part of the meaning: `and`/`or` short-circuit left
//! to right, `if` and `match` evaluate only the taken arm, list elements
//! and call arguments left to right, a struct's fields in field-name order.

use std::collections::BTreeMap;

use crate::hash::{closure, Closure};
use crate::ir::{Block, CmpOp, Expr, FnKind, Function, Module, Op, Plan, Pred, Related, Stmt, Sym};
use crate::schema::{Schema, Table};
use crate::stdlib::{self, StdError};
use crate::store::{Change, Overlay, Refusal, Row, Store};
use crate::value::{FieldName, TableName, Value};
use crate::view::{cmp, order_by};

pub use crate::stdlib::Args;

/// Who authored an entry: the user the authority verified for the
/// connection, and the login it was authored under (`Ark.Eval.Ctx`).
/// Generated code reads them as `Value::text(ctx.user.clone())`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Ctx {
    pub user: String,
    pub session: String,
}

impl Ctx {
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
    /// The helpers in reach: a closure's, never a module's.
    helpers: &'a [Function],
    kind: FnKind,
    ctx: &'a Ctx,
    args: &'a Args,
    autos: &'a Args,
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
    for (a, _) in &f.args {
        if !args.contains_key(a) {
            return Err(EvalError::MissingArg(a.clone()));
        }
    }
    let env = Env {
        schema: sch,
        helpers: &c.helpers,
        kind: FnKind::Mutator,
        ctx,
        args,
        autos,
        locals: BTreeMap::new(),
    };
    let outcome = {
        let mut overlay = Overlay::new(&*store);
        let mut st = St {
            store: &mut overlay,
            changes: Vec::new(),
        };
        let r = block(&mut st, &env, &f.body);
        (r, st.changes)
    };
    match outcome {
        (Ok(()) | Err(Stop::Returned(_)), changes) => {
            store.apply_changes(&changes);
            Ok(Ok(changes))
        }
        (Err(Stop::Halt(EvalFault::Verdict(r))), _) => Ok(Err(r)),
        (Err(Stop::Halt(EvalFault::Bug(e))), _) => Err(e),
    }
}

/// §6.2 Run a query by name. A query never changes the store; a verdict here
/// is a fault such as an overflow, shown as an error and recorded nowhere.
pub fn query(m: &Module, name: &str, args: &Args, store: &dyn Store) -> Result<Value, EvalFault> {
    let f = function(m, name).map_err(EvalFault::Bug)?;
    query_closure(&m.schema, &closure(m, f), args, store)
}

/// §6.2 Run a query closure.
pub fn query_closure(sch: &Schema, c: &Closure, args: &Args, store: &dyn Store) -> Result<Value, EvalFault> {
    let f = &c.function;
    if f.kind != FnKind::Query {
        return Err(EvalFault::Bug(EvalError::WrongKind(f.name.clone(), f.kind)));
    }
    for (a, _) in &f.args {
        if !args.contains_key(a) {
            return Err(EvalFault::Bug(EvalError::MissingArg(a.clone())));
        }
    }
    let ctx = Ctx::default();
    let autos = Args::new();
    let env = Env {
        schema: sch,
        helpers: &c.helpers,
        kind: FnKind::Query,
        ctx: &ctx,
        args,
        autos: &autos,
        locals: BTreeMap::new(),
    };
    let mut overlay = Overlay::new(store);
    let mut st = St {
        store: &mut overlay,
        changes: Vec::new(),
    };
    match block(&mut st, &env, &f.body) {
        Ok(()) | Err(Stop::Returned(None)) => Err(EvalFault::Bug(EvalError::NoReturn(f.name.clone()))),
        Err(Stop::Returned(Some(v))) => Ok(v),
        Err(Stop::Halt(fault)) => Err(fault),
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
        Stmt::Put(t, e) => {
            mutating(env)?;
            let row = strct(eval(st, env, e)?)?;
            match st.store.put(t, row) {
                Err(r) => return verdict(r),
                Ok(ch) => st.changes.extend(ch),
            }
        }
        Stmt::Delete(t, ks) => {
            mutating(env)?;
            let key = ks.iter().map(|k| eval(st, env, k)).collect::<Run<Vec<Value>>>()?;
            match st.store.delete(t, &key) {
                Err(r) => return verdict(r),
                Ok(ch) => st.changes.extend(ch),
            }
        }
        Stmt::Refuse(e) => {
            if env.kind != FnKind::Mutator {
                return bug(EvalError::Impure("refuse outside a mutator".into()));
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
    if vals.len() != f.args.len() {
        return bug(EvalError::Arity(f.name.clone()));
    }
    let args: Args = f.args.iter().map(|(n, _)| n.clone()).zip(vals).collect();
    let autos = Args::new();
    let env2 = Env {
        schema: env.schema,
        helpers: env.helpers,
        kind: FnKind::Helper,
        ctx: env.ctx,
        args: &args,
        autos: &autos,
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
    let mut admitted: Vec<Row> = st.store.scan(&p.table).into_iter().filter(|r| keep(r)).collect();
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
