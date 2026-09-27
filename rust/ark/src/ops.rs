//! The operators GENERATED.md names (`Ark.Eval` §6.4, §6.5), as associated
//! functions of the unit struct [`Ops`], so that generated code reads
//! `Ops::add(a, b)?`.
//!
//! Arithmetic is checked and faults as the spec does: `refuse("integer
//! overflow")`, `refuse("division by zero")`. `cmp` is the total order.
//! `and`/`or` are not here: they are emitted as Rust's short-circuit
//! operators over `as_bool()`, wrapped in `Value::bool`.

use crate::eval::arith;
use crate::fault::Fault;
use crate::ir::{CmpOp, Op};
use crate::stdlib::Args;
use crate::value::Value;
use crate::view::cmp;

pub struct Ops;

fn int(v: &Value) -> Result<i64, Fault> {
    match v {
        Value::Int(n) => Ok(*n),
        other => Err(Fault::bug(format!("expected Int, got {other:?}"))),
    }
}

fn binary(op: Op, a: Value, b: Value) -> Result<Value, Fault> {
    let (x, y) = (int(&a)?, int(&b)?);
    arith(op, x, y).map(Value::Int).map_err(Fault::refuse)
}

fn list(v: Value) -> Result<Vec<Value>, Fault> {
    match v {
        Value::List(xs) => Ok(xs),
        other => Err(Fault::bug(format!("expected List, got {other:?}"))),
    }
}

fn truth(v: Value) -> Result<bool, Fault> {
    match v {
        Value::Bool(b) => Ok(b),
        other => Err(Fault::bug(format!("expected Bool, got {other:?}"))),
    }
}

impl Ops {
    /// Checked addition.
    pub fn add(a: Value, b: Value) -> Result<Value, Fault> {
        binary(Op::Add, a, b)
    }

    /// Checked subtraction.
    pub fn sub(a: Value, b: Value) -> Result<Value, Fault> {
        binary(Op::Sub, a, b)
    }

    /// Checked multiplication.
    pub fn mul(a: Value, b: Value) -> Result<Value, Fault> {
        binary(Op::Mul, a, b)
    }

    /// Division truncating toward zero; faults on zero and on `MIN / -1`.
    pub fn div(a: Value, b: Value) -> Result<Value, Fault> {
        binary(Op::Div, a, b)
    }

    /// The remainder with the dividend's sign; faults on zero and `MIN % -1`.
    pub fn r#mod(a: Value, b: Value) -> Result<Value, Fault> {
        binary(Op::Mod, a, b)
    }

    /// Checked negation; faults on `MIN`.
    pub fn neg(a: Value) -> Result<Value, Fault> {
        int(&a)?.checked_neg().map(Value::Int).ok_or_else(|| Fault::refuse("integer overflow"))
    }

    /// `Value::bool` of the comparison under the total order; cannot fault.
    pub fn cmp(op: CmpOp, a: Value, b: Value) -> Value {
        Value::Bool(cmp(op, &a, &b))
    }

    /// Boolean negation; a non-bool is fatal, as `as_bool` is.
    pub fn not(a: Value) -> Value {
        Value::Bool(!a.as_bool())
    }

    /// The named argument (or auto), cloned; missing is fatal, as an
    /// accessor mismatch is, because a verified module names only its own
    /// arguments.
    pub fn arg(args: &Args, name: &str) -> Value {
        args.get(name)
            .cloned()
            .unwrap_or_else(|| panic!("Ops::arg: no argument {name:?} (a bug: a verified module names only its arguments)"))
    }

    /// `match opt { Some v -> some(v); None -> none() }`.
    pub fn match_opt(
        opt: Value,
        some: impl FnOnce(Value) -> Result<Value, Fault>,
        none: impl FnOnce() -> Result<Value, Fault>,
    ) -> Result<Value, Fault> {
        if opt.is_null() {
            none()
        } else {
            some(opt)
        }
    }

    /// Map over a list, left to right.
    pub fn map(xs: Value, mut f: impl FnMut(Value) -> Result<Value, Fault>) -> Result<Value, Fault> {
        let mut out = Vec::new();
        for x in list(xs)? {
            out.push(f(x)?);
        }
        Ok(Value::List(out))
    }

    /// Keep the elements the closure answers true for.
    pub fn filter(xs: Value, mut f: impl FnMut(Value) -> Result<Value, Fault>) -> Result<Value, Fault> {
        let mut out = Vec::new();
        for x in list(xs)? {
            if truth(f(x.clone())?)? {
                out.push(x);
            }
        }
        Ok(Value::List(out))
    }

    /// Whether any element answers true; every element is evaluated, as
    /// the spec does.
    pub fn any(xs: Value, mut f: impl FnMut(Value) -> Result<Value, Fault>) -> Result<Value, Fault> {
        let mut acc = false;
        for x in list(xs)? {
            acc |= truth(f(x)?)?;
        }
        Ok(Value::Bool(acc))
    }

    /// Whether every element answers true; every element is evaluated.
    pub fn all(xs: Value, mut f: impl FnMut(Value) -> Result<Value, Fault>) -> Result<Value, Fault> {
        let mut acc = true;
        for x in list(xs)? {
            acc &= truth(f(x)?)?;
        }
        Ok(Value::Bool(acc))
    }

    /// Stable sort by a key, under the total order.
    pub fn sort_by(xs: Value, mut key: impl FnMut(Value) -> Result<Value, Fault>) -> Result<Value, Fault> {
        let mut keyed = Vec::new();
        for x in list(xs)? {
            let k = key(x.clone())?;
            keyed.push((x, k));
        }
        keyed.sort_by(|a, b| a.1.cmp(&b.1));
        Ok(Value::List(keyed.into_iter().map(|(x, _)| x).collect()))
    }

    /// `fold xs init f(acc, x)`, left to right.
    pub fn fold(xs: Value, init: Value, mut f: impl FnMut(Value, Value) -> Result<Value, Fault>) -> Result<Value, Fault> {
        let mut acc = init;
        for x in list(xs)? {
            acc = f(acc, x)?;
        }
        Ok(acc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faults_carry_the_spec_text() {
        assert_eq!(Ops::add(Value::int(i64::MAX), Value::int(1)), Err(Fault::refuse("integer overflow")));
        assert_eq!(Ops::div(Value::int(1), Value::int(0)), Err(Fault::refuse("division by zero")));
        assert_eq!(Ops::r#mod(Value::int(-7), Value::int(2)), Ok(Value::int(-1)));
        assert_eq!(Ops::cmp(CmpOp::Lt, Value::Null, Value::int(0)), Value::bool(true));
    }

    #[test]
    fn sort_by_is_stable() {
        let xs = Value::list(vec![Value::int(2), Value::int(1), Value::int(3)]);
        let sorted = Ops::sort_by(xs, |v| Ok(Value::int(v.as_int() % 2))).unwrap();
        assert_eq!(sorted, Value::list(vec![Value::int(2), Value::int(1), Value::int(3)]));
    }
}
