//! §2.4 Control: statements, each a function over closures. Under `Emit`
//! every closure runs once, into a block of its own; under `Native` a
//! closure runs only when it is taken.

use crate::ir::{Expr, Stmt};
use crate::value::Value;

use super::cx;
use super::schema::{Effect, IntoEffect};
use super::values::{Bool, Data, List, Text};

fn taken(c: Bool) -> Option<bool> {
    if cx::halted() {
        return None;
    }
    match cx::value(c.to_h()) {
        Value::Bool(b) => Some(b),
        other => {
            cx::halt(crate::eval::EvalFault::Bug(crate::eval::EvalError::TypeError(format!("a condition is a Bool, not {other:?}"))));
            None
        }
    }
}

/// `SIf c then []`.
pub fn when<R: IntoEffect>(c: Bool, then: impl FnOnce() -> R) -> Effect {
    if_else(c, then, || ())
}

/// `SIf c [] else`.
pub fn unless<R: IntoEffect>(c: Bool, otherwise: impl FnOnce() -> R) -> Effect {
    if_else(c, || (), otherwise)
}

/// `SIf c then else`.
pub fn if_else<A: IntoEffect, B: IntoEffect>(c: Bool, then: impl FnOnce() -> A, otherwise: impl FnOnce() -> B) -> Effect {
    if cx::emitting() {
        let cond = cx::expr(c.to_h());
        let (_, a) = cx::block(|| then().into_effect());
        let (_, b) = cx::block(|| otherwise().into_effect());
        cx::stmt(Stmt::If(cond, a, b));
        return Effect(());
    }
    match taken(c) {
        Some(true) => then().into_effect(),
        Some(false) => otherwise().into_effect(),
        None => Effect(()),
    }
}

/// `SFor x xs body`.
pub fn for_each<T: Data, R: IntoEffect>(xs: List<T>, mut body: impl FnMut(T) -> R) -> Effect {
    if cx::emitting() {
        let e = cx::expr(xs.to_h());
        let x = cx::fresh();
        let (_, b) = cx::block(|| body(T::from_h(cx::e(Expr::Var(x)))).into_effect());
        cx::stmt(Stmt::For(x, e, b));
        return Effect(());
    }
    if cx::halted() {
        return Effect(());
    }
    let vs = match cx::value(xs.to_h()) {
        Value::List(vs) => vs,
        _ => vec![],
    };
    for v in vs {
        if cx::halted() {
            break;
        }
        body(T::from_h(cx::lit(v))).into_effect();
    }
    Effect(())
}

/// `SRefuse msg`: the entry's verdict, reached identically everywhere.
pub fn refuse(msg: impl Into<Text>) -> Effect {
    let m = msg.into();
    if cx::emitting() {
        cx::stmt(Stmt::Refuse(cx::expr(m.to_h())));
        return Effect(());
    }
    if !cx::halted() {
        match cx::value(m.to_h()) {
            Value::Text(t) => cx::refused(t),
            other => cx::refused(format!("{other:?}")),
        }
    }
    Effect(())
}
