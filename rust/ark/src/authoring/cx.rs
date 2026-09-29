//! The ambient context a body runs under: `Emit` or `Native`, and the
//! arena its values live in. A value of the vocabulary is a [`H`], an index
//! into the arena of the run in progress; under `Emit` the node is an
//! expression, under `Native` a value. Nothing outside this file knows
//! which, except by asking [`emitting`].
//!
//! Under `Native` the arena is where a query's time goes, so three things
//! about it are deliberate. A value is shared (`Rc`), so iterating a list a
//! closure captured does not copy the list. An element of a list, and a
//! field of a row, are read *in place* ([`Node::Elem`], [`Node::Field`]):
//! taking a row apart into its fields allocates nothing, and comparing two
//! fields ([`op`] reads them where they are) copies nothing. And a loop's scratch is forgotten at
//! the end of each iteration ([`mark`], [`truncate`]), so a query nested
//! three deep costs the arena its widest iteration and not every one.

use std::cell::RefCell;
use std::rc::Rc;

use crate::eval::{self, EvalError, EvalFault};
use crate::ir::{Auto, Block, CmpOp, Expr, Op, StdFn, Stmt, Sym};
use crate::stdlib::{self, Args, StdError};
use crate::store::{Change, Refusal};
use crate::value::Value;

/// A value of the vocabulary: an index into the arena of the run in
/// progress. Every value type is one of these, which is what lets rows,
/// tables and inputs be plain structs (see `raw`).
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct H(u32);

pub(crate) enum Node {
    /// Emit: an expression.
    E(Box<Expr>),
    /// A value; under Emit, a literal.
    V(Rc<Value>),
    /// Native: element `i` of the list at the handle, read in place.
    Elem(H, u32),
    /// Native: the field of the struct at the handle, read in place.
    Field(H, Rc<str>),
}

pub(crate) struct Emit {
    blocks: Vec<Block>,
    next: Sym,
    pub(crate) autos: Vec<(String, Auto)>,
    pub(crate) errors: Vec<String>,
    /// How deep in expression closures (`map`, `filter`, a check…) the run
    /// is: no statement may be written there.
    in_expr: usize,
    /// How deep in a query's plan (its body, and the closures of `get`,
    /// `each`, `having`, `sort_by` and `map`) the run is: no read may be
    /// written there, because a plan reads through its lookups and related
    /// plans and never through an expression (§1.2).
    in_plan: usize,
}

pub(crate) struct Native {
    pub(crate) ctx: eval::Ctx,
    pub(crate) autos: Args,
    pub(crate) halt: Option<EvalFault>,
    pub(crate) changes: Vec<Change>,
}

pub(crate) enum Mode {
    Emit(Emit),
    Native(Native),
}

pub(crate) struct Cx {
    pub(crate) mode: Mode,
    nodes: Vec<Node>,
    /// Emit: rows taken apart into their fields, by those fields' handles: a
    /// row given back unchanged is the term it came from, not a struct
    /// rebuilt field by field (§6: `or_refuse`'s value is `EStd Unwrap
    /// [EVar s]`). Native reads that off the nodes themselves ([`whole`]).
    origins: std::collections::HashMap<Vec<u32>, H>,
}

impl Cx {
    pub(crate) fn emit() -> Cx {
        Cx {
            mode: Mode::Emit(Emit {
                blocks: vec![vec![]],
                next: 0,
                autos: vec![],
                errors: vec![],
                in_expr: 0,
                in_plan: 0,
            }),
            nodes: vec![],
            origins: std::collections::HashMap::new(),
        }
    }

    pub(crate) fn native(ctx: eval::Ctx, autos: Args) -> Cx {
        Cx {
            mode: Mode::Native(Native {
                ctx,
                autos,
                halt: None,
                changes: vec![],
            }),
            nodes: vec![],
            origins: std::collections::HashMap::new(),
        }
    }

    /// An emit run's statements, once it is over.
    pub(crate) fn into_emit(self) -> Emit {
        match self.mode {
            Mode::Emit(e) => e,
            Mode::Native(_) => unreachable!("an emit run"),
        }
    }

    pub(crate) fn into_native(self) -> Native {
        match self.mode {
            Mode::Native(n) => n,
            Mode::Emit(_) => unreachable!("a native run"),
        }
    }
}

impl Emit {
    /// The body written at the top level.
    pub(crate) fn body(&mut self) -> Block {
        std::mem::take(&mut self.blocks[0])
    }
}

thread_local! {
    static CX: RefCell<Vec<Cx>> = const { RefCell::new(Vec::new()) };
}

struct Pop;

impl Drop for Pop {
    fn drop(&mut self) {
        CX.with(|c| {
            if let Ok(mut v) = c.try_borrow_mut() {
                v.pop();
            }
        });
    }
}

/// Run `f` under a context, and hand the context back when it is over.
pub(crate) fn run<R>(cx: Cx, f: impl FnOnce() -> R) -> (R, Cx) {
    CX.with(|c| c.borrow_mut().push(cx));
    let guard = Pop;
    let r = f();
    let cx = CX.with(|c| c.borrow_mut().pop()).expect("the context pushed above");
    std::mem::forget(guard);
    (r, cx)
}

fn with<R>(f: impl FnOnce(&mut Cx) -> R) -> R {
    CX.with(|c| {
        let mut v = c.borrow_mut();
        let cx = v
            .last_mut()
            .expect("a value of the vocabulary used outside a procedure: values exist only while Module::emit or a native run is running a body");
        f(cx)
    })
}

pub(crate) fn emitting() -> bool {
    with(|cx| matches!(cx.mode, Mode::Emit(_)))
}

fn push(cx: &mut Cx, n: Node) -> H {
    cx.nodes.push(n);
    H((cx.nodes.len() - 1) as u32)
}

pub(crate) fn node(n: Node) -> H {
    with(|cx| push(cx, n))
}

/// Emit: remember that these field handles are the row `origin` taken apart.
pub(crate) fn remember(fields: &[H], origin: H) {
    with(|cx| {
        cx.origins.insert(fields.iter().map(|h| h.0).collect(), origin);
    })
}

/// Emit: the row these field handles were taken from, if they are exactly
/// its fields, unchanged.
pub(crate) fn origin(fields: &[H]) -> Option<H> {
    with(|cx| cx.origins.get(&fields.iter().map(|h| h.0).collect::<Vec<u32>>()).copied())
}

/// A literal: the same node in both modes.
pub(crate) fn lit(v: Value) -> H {
    node(Node::V(Rc::new(v)))
}

/// A value already shared.
pub(crate) fn lit_rc(v: Rc<Value>) -> H {
    node(Node::V(v))
}

/// An expression node (Emit).
pub(crate) fn e(x: Expr) -> H {
    node(Node::E(Box::new(x)))
}

/// Native: element `i` of the list at `list`, read in place.
pub(crate) fn elem(list: H, i: usize) -> H {
    node(Node::Elem(list, i as u32))
}

/// Native: the field of the struct at `of`, read in place.
pub(crate) fn field(of: H, name: Rc<str>) -> H {
    node(Node::Field(of, name))
}

/// The expression a handle stands for (Emit). A literal is `ELit`.
pub(crate) fn expr(h: H) -> Expr {
    with(|cx| match &cx.nodes[h.0 as usize] {
        Node::E(x) => (**x).clone(),
        Node::V(v) => Expr::Lit((**v).clone()),
        Node::Elem(..) | Node::Field(..) => unreachable!("a value read in place under Emit"),
    })
}

static NULL: Value = Value::Null;

/// The value at a handle, where it lives (Native).
fn resolve(cx: &Cx, h: H) -> &Value {
    match &cx.nodes[h.0 as usize] {
        Node::V(v) => v,
        Node::Elem(l, i) => match resolve(cx, *l) {
            Value::List(xs) => xs.get(*i as usize).unwrap_or(&NULL),
            _ => &NULL,
        },
        Node::Field(o, n) => match resolve(cx, *o) {
            Value::Struct(m) => m.get(&**n).unwrap_or(&NULL),
            _ => &NULL,
        },
        Node::E(x) => panic!("a Native value that is an expression: {x:?}"),
    }
}

/// The value a handle holds (Native), copied out.
pub(crate) fn value(h: H) -> Value {
    with(|cx| resolve(cx, h).clone())
}

/// The value at a handle, shared (Native): the node's own `Rc` where it is
/// one, so a list a closure captured is iterated and never copied.
pub(crate) fn shared(h: H) -> Rc<Value> {
    with(|cx| match &cx.nodes[h.0 as usize] {
        Node::V(v) => v.clone(),
        _ => Rc::new(resolve(cx, h).clone()),
    })
}

/// Native: the struct these handles are the fields of, read in place and
/// unchanged — exactly `names`, in order, off one struct — so a row given
/// back is the row, not a copy rebuilt field by field.
pub(crate) fn whole(fields: &[H], names: &[Rc<str>]) -> Option<H> {
    if fields.is_empty() || fields.len() != names.len() {
        return None;
    }
    with(|cx| {
        let mut base = None;
        for (h, n) in fields.iter().zip(names) {
            match &cx.nodes[h.0 as usize] {
                Node::Field(o, m) if Rc::ptr_eq(m, n) || **m == **n => match base {
                    None => base = Some(*o),
                    Some(b) if b == *o => {}
                    Some(_) => return None,
                },
                _ => return None,
            }
        }
        base
    })
}

/// How many nodes there are: what [`truncate`] goes back to.
pub(crate) fn mark() -> usize {
    with(|cx| cx.nodes.len())
}

/// Native: forget every node made since `mark` — one iteration's scratch,
/// once its result has been copied out. Emit keeps everything: a statement
/// written inside a closure names its nodes later.
pub(crate) fn truncate(mark: usize) {
    with(|cx| {
        if matches!(cx.mode, Mode::Native(_)) {
            cx.nodes.truncate(mark);
        }
    })
}

/// Native: whether the run has stopped (a verdict or a bug). Everything
/// after a halt is a no-op that yields `Null`, so a body runs to its end
/// without effect and the verdict is the first one reached.
pub(crate) fn halted() -> bool {
    with(|cx| match &cx.mode {
        Mode::Native(n) => n.halt.is_some(),
        Mode::Emit(_) => false,
    })
}

pub(crate) fn halt(f: EvalFault) {
    with(|cx| {
        if let Mode::Native(n) = &mut cx.mode {
            if n.halt.is_none() {
                n.halt = Some(f);
            }
        }
    })
}

pub(crate) fn refused(why: String) {
    halt(EvalFault::Verdict(Refusal::Refused(why)))
}

/// An operation over values: in Emit the expression `emit(args)`, in
/// Native the value `native(args)`, a fault halting the run. The values
/// are read in place; `native` copies what it keeps.
pub(crate) fn op(args: &[H], emit: impl FnOnce(Vec<Expr>) -> Expr, native: impl FnOnce(&[&Value]) -> Result<Value, EvalFault>) -> H {
    if emitting() {
        let es = args.iter().map(|h| expr(*h)).collect();
        return e(emit(es));
    }
    if halted() {
        return lit(Value::Null);
    }
    let r = with(|cx| {
        let cx: &Cx = cx;
        let vs: Vec<&Value> = args.iter().map(|h| resolve(cx, *h)).collect();
        native(&vs)
    });
    match r {
        Ok(v) => lit(v),
        Err(f) => {
            halt(f);
            lit(Value::Null)
        }
    }
}

pub(crate) fn std_fault(e: StdError) -> EvalFault {
    match e {
        StdError::Fault(t) => EvalFault::Verdict(Refusal::Refused(t)),
        StdError::Arity(g, n) => EvalFault::Bug(EvalError::Arity(format!("{}/{n}", g.show()))),
        StdError::TypeMismatch(g) => EvalFault::Bug(EvalError::TypeError(g.show().into())),
    }
}

/// A standard function. The ones a loop body reaches for — what is in a
/// list, whether an option holds anything — are answered in place; the
/// rest copy their arguments and go through `stdlib::std`, as the
/// interpreter does.
pub(crate) fn std_op(f: StdFn, args: &[H]) -> H {
    op(
        args,
        |es| Expr::Std(f, es),
        |vs| match (f, vs) {
            (StdFn::Len, [Value::List(xs)]) => Ok(Value::Int(xs.len() as i64)),
            (StdFn::First, [Value::List(xs)]) => Ok(xs.first().cloned().unwrap_or(Value::Null)),
            (StdFn::Last, [Value::List(xs)]) => Ok(xs.last().cloned().unwrap_or(Value::Null)),
            (StdFn::Contains, [Value::List(xs), v]) => Ok(Value::Bool(xs.iter().any(|x| x == *v))),
            (StdFn::IsSome, [v]) => Ok(Value::Bool(!v.is_null())),
            (StdFn::IsEmpty, [Value::Text(t)]) => Ok(Value::Bool(t.is_empty())),
            (StdFn::UnwrapOr, [Value::Null, d]) => Ok((*d).clone()),
            (StdFn::UnwrapOr, [v, _]) => Ok((*v).clone()),
            _ => {
                let owned: Vec<Value> = vs.iter().map(|v| (*v).clone()).collect();
                stdlib::std(f, &owned).map_err(std_fault)
            }
        },
    )
}

/// An arithmetic or boolean operator, checked as `Ark.Eval` checks it.
pub(crate) fn arith_op(o: Op, args: &[H]) -> H {
    op(
        args,
        |es| Expr::Op(o, es),
        |vs| match (o, vs) {
            (Op::And, [Value::Bool(a), Value::Bool(b)]) => Ok(Value::Bool(*a && *b)),
            (Op::Or, [Value::Bool(a), Value::Bool(b)]) => Ok(Value::Bool(*a || *b)),
            (Op::Not, [Value::Bool(a)]) => Ok(Value::Bool(!a)),
            (Op::Neg, [Value::Int(n)]) => n
                .checked_neg()
                .map(Value::Int)
                .ok_or_else(|| EvalFault::Verdict(Refusal::Refused("integer overflow".into()))),
            (_, [Value::Int(x), Value::Int(y)]) => eval::arith(o, *x, *y)
                .map(Value::Int)
                .map_err(|t| EvalFault::Verdict(Refusal::Refused(t.into()))),
            _ => Err(EvalFault::Bug(EvalError::TypeError(format!("{} of {vs:?}", o.show())))),
        },
    )
}

pub(crate) fn cmp_op(o: CmpOp, a: H, b: H) -> H {
    op(
        &[a, b],
        |mut es| {
            let r = es.pop().expect("two");
            let l = es.pop().expect("two");
            Expr::Cmp(o, Box::new(l), Box::new(r))
        },
        |vs| Ok(Value::Bool(crate::view::cmp(o, vs[0], vs[1]))),
    )
}

// Emit: statements and symbols --------------------------------------------

fn emit_mut<R>(f: impl FnOnce(&mut Emit) -> R) -> R {
    with(|cx| match &mut cx.mode {
        Mode::Emit(em) => f(em),
        Mode::Native(_) => unreachable!("an Emit-only operation under Native"),
    })
}

/// A fresh symbol.
pub(crate) fn fresh() -> Sym {
    emit_mut(|em| {
        let s = em.next;
        em.next += 1;
        s
    })
}

/// Write a statement into the block being recorded.
pub(crate) fn stmt(s: Stmt) {
    emit_mut(|em| {
        if em.in_expr > 0 {
            em.errors
                .push(format!("a statement inside an expression closure (map, filter, a check…): {s:?}"));
            return;
        }
        em.blocks.last_mut().expect("a block").push(s);
    })
}

/// `SLet s e`, and the value `EVar s`.
pub(crate) fn bind(x: Expr) -> H {
    let s = fresh();
    stmt(Stmt::Let(s, x));
    e(Expr::Var(s))
}

/// Record the statements `f` writes as a block of their own.
pub(crate) fn block<R>(f: impl FnOnce() -> R) -> (R, Block) {
    emit_mut(|em| em.blocks.push(vec![]));
    let r = f();
    let b = emit_mut(|em| em.blocks.pop().expect("the block pushed above"));
    (r, b)
}

/// Record `f` as the body of a function of its own inside the run in
/// progress (a helper's): a block of its own, with statements allowed
/// again even when called from inside an expression closure.
pub(crate) fn detached<R>(f: impl FnOnce() -> R) -> (R, Block) {
    let depth = emit_mut(|em| std::mem::replace(&mut em.in_expr, 0));
    let r = block(f);
    emit_mut(|em| em.in_expr = depth);
    r
}

/// Run an expression closure: no statement may be written inside it.
pub(crate) fn in_expr<R>(f: impl FnOnce() -> R) -> R {
    if !emitting() {
        return f();
    }
    emit_mut(|em| em.in_expr += 1);
    let r = f();
    emit_mut(|em| em.in_expr -= 1);
    r
}

/// Run a query's body, or a closure of one of its plan's nodes: no
/// statement may be written inside it, and no read (§1.9).
pub(crate) fn in_plan<R>(f: impl FnOnce() -> R) -> R {
    if !emitting() {
        return f();
    }
    emit_mut(|em| {
        em.in_plan += 1;
        em.in_expr += 1;
    });
    let r = f();
    emit_mut(|em| {
        em.in_plan -= 1;
        em.in_expr -= 1;
    });
    r
}

/// Emit: whether the run is inside a query's plan, where a read is an
/// authoring error rather than a statement.
pub(crate) fn planning() -> bool {
    with(|cx| match &cx.mode {
        Mode::Emit(em) => em.in_plan > 0,
        Mode::Native(_) => false,
    })
}

/// Emit: an authoring error, reported by `Module::build`.
pub(crate) fn complain(what: String) {
    emit_mut(|em| em.errors.push(what));
}

/// Register an auto by name. Naming it again is reading the same frozen
/// value again — `ctx.now("added_ms")` in three rows of one entry is one
/// time — so a repeated name of the same kind is that auto; a repeated
/// name of another kind is an error.
pub(crate) fn auto(name: &str, a: Auto) {
    emit_mut(|em| match em.autos.iter().find(|(n, _)| n == name) {
        Some((_, had)) if *had == a => {}
        Some((_, had)) => em
            .errors
            .push(format!("the auto {name:?} is drawn as {had:?} and again as {a:?}; one name is one auto")),
        None => em.autos.push((name.into(), a)),
    })
}

// Native: the run's own state ----------------------------------------------

pub(crate) fn native<R>(f: impl FnOnce(&mut Native) -> R) -> R {
    with(|cx| match &mut cx.mode {
        Mode::Native(n) => f(n),
        Mode::Emit(_) => unreachable!("a Native-only operation under Emit"),
    })
}
