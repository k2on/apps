//! Expressions. An [`Expr`] is one node of `Ark.IR.Expr`; every method
//! builds the node it is named after, taking `&self` so an author can use
//! an argument twice without a clone at each site. Nothing here
//! type-checks — the verifier does — but nothing here can produce a shape
//! the IR has no constructor for.

use ark::ir::{CmpOp, Expr as IrExpr, Op, StdFn, Sym};
use ark::value::Value;

use crate::sym::fresh;
use crate::ty::Ty;

/// One expression of the IR.
#[derive(Clone, Debug)]
pub struct Expr(pub(crate) IrExpr);

/// A `match` on an option whose `Some` arm is written and whose `None` arm
/// is still to be chosen: [`MaybeExpr::unwrap_or`] gives the arm a default
/// of the arm's own type; [`MaybeExpr::or_none`] keeps it an option.
#[derive(Clone, Debug)]
pub struct MaybeExpr {
    scrutinee: IrExpr,
    sym: Sym,
    body: IrExpr,
}

fn ir(e: impl Into<Expr>) -> IrExpr {
    e.into().0
}

impl Expr {
    /// The `ark` crate's node.
    pub fn into_ir(self) -> IrExpr {
        self.0
    }

    /// A literal.
    pub fn lit(v: Value) -> Expr {
        Expr(IrExpr::Lit(v))
    }

    /// An int literal.
    pub fn int(n: i64) -> Expr {
        Expr::lit(Value::Int(n))
    }

    /// A text literal.
    pub fn text(s: &str) -> Expr {
        Expr::lit(Value::text(s))
    }

    /// A bool literal.
    pub fn bool(b: bool) -> Expr {
        Expr::lit(Value::Bool(b))
    }

    /// A bytes literal.
    pub fn bytes(b: Vec<u8>) -> Expr {
        Expr::lit(Value::Bytes(b))
    }

    /// `None`, at the type of the value it lacks.
    pub fn none(ty: Ty) -> Expr {
        Expr(IrExpr::None(ty.to_ir()))
    }

    /// `Some e`.
    pub fn some(e: impl Into<Expr>) -> Expr {
        Expr(IrExpr::Some(Box::new(ir(e))))
    }

    /// A call to a helper declared earlier in the module.
    pub fn call(helper: &str, args: impl IntoIterator<Item = Expr>) -> Expr {
        Expr(IrExpr::Call(helper.to_string(), args.into_iter().map(|e| e.0).collect()))
    }

    /// The nil id; the verifier takes its table from context.
    pub fn nil_id() -> Expr {
        Expr::std(StdFn::NilId, vec![])
    }

    fn std(f: StdFn, args: Vec<IrExpr>) -> Expr {
        Expr(IrExpr::Std(f, args))
    }

    fn std1(&self, f: StdFn) -> Expr {
        Expr::std(f, vec![self.0.clone()])
    }

    fn std2(&self, f: StdFn, e: impl Into<Expr>) -> Expr {
        Expr::std(f, vec![self.0.clone(), ir(e)])
    }

    fn op(&self, op: Op, e: impl Into<Expr>) -> Expr {
        Expr(IrExpr::Op(op, vec![self.0.clone(), ir(e)]))
    }

    fn cmp(&self, op: CmpOp, e: impl Into<Expr>) -> Expr {
        Expr(IrExpr::Cmp(op, Box::new(self.0.clone()), Box::new(ir(e))))
    }

    fn bound(&self, f: impl FnOnce(Expr) -> Expr) -> (Sym, IrExpr) {
        let x = fresh();
        (x, f(Expr(IrExpr::Var(x))).0)
    }

    // -- structs ------------------------------------------------------------

    /// `e.name`.
    pub fn field(&self, name: &str) -> Expr {
        Expr(IrExpr::Field(Box::new(self.0.clone()), name.to_string()))
    }

    // -- booleans -----------------------------------------------------------

    /// `not e`.
    pub fn not(&self) -> Expr {
        Expr(IrExpr::Op(Op::Not, vec![self.0.clone()]))
    }

    /// `a and b`.
    pub fn and(&self, e: impl Into<Expr>) -> Expr {
        self.op(Op::And, e)
    }

    /// `a or b`.
    pub fn or(&self, e: impl Into<Expr>) -> Expr {
        self.op(Op::Or, e)
    }

    /// `if self then a else b`, as an expression.
    pub fn then_else(&self, a: impl Into<Expr>, b: impl Into<Expr>) -> Expr {
        Expr(IrExpr::If(Box::new(self.0.clone()), Box::new(ir(a)), Box::new(ir(b))))
    }

    // -- comparisons, under `compare_value` -----------------------------------

    pub fn eq(&self, e: impl Into<Expr>) -> Expr {
        self.cmp(CmpOp::Eq, e)
    }

    pub fn ne(&self, e: impl Into<Expr>) -> Expr {
        self.cmp(CmpOp::Ne, e)
    }

    pub fn lt(&self, e: impl Into<Expr>) -> Expr {
        self.cmp(CmpOp::Lt, e)
    }

    pub fn le(&self, e: impl Into<Expr>) -> Expr {
        self.cmp(CmpOp::Le, e)
    }

    pub fn gt(&self, e: impl Into<Expr>) -> Expr {
        self.cmp(CmpOp::Gt, e)
    }

    pub fn ge(&self, e: impl Into<Expr>) -> Expr {
        self.cmp(CmpOp::Ge, e)
    }

    // -- integers: checked, never wrapping -----------------------------------

    pub fn add(&self, e: impl Into<Expr>) -> Expr {
        self.op(Op::Add, e)
    }

    pub fn sub(&self, e: impl Into<Expr>) -> Expr {
        self.op(Op::Sub, e)
    }

    pub fn mul(&self, e: impl Into<Expr>) -> Expr {
        self.op(Op::Mul, e)
    }

    /// Division truncating toward zero.
    pub fn div(&self, e: impl Into<Expr>) -> Expr {
        self.op(Op::Div, e)
    }

    /// The remainder, with the dividend's sign.
    pub fn rem(&self, e: impl Into<Expr>) -> Expr {
        self.op(Op::Mod, e)
    }

    pub fn neg(&self) -> Expr {
        Expr(IrExpr::Op(Op::Neg, vec![self.0.clone()]))
    }

    pub fn min(&self, e: impl Into<Expr>) -> Expr {
        self.std2(StdFn::Min, e)
    }

    pub fn max(&self, e: impl Into<Expr>) -> Expr {
        self.std2(StdFn::Max, e)
    }

    pub fn clamp(&self, lo: impl Into<Expr>, hi: impl Into<Expr>) -> Expr {
        Expr::std(StdFn::Clamp, vec![self.0.clone(), ir(lo), ir(hi)])
    }

    pub fn abs(&self) -> Expr {
        self.std1(StdFn::Abs)
    }

    /// The decimal spelling of an int.
    pub fn text_of_int(&self) -> Expr {
        self.std1(StdFn::TextOfInt)
    }

    // -- text -----------------------------------------------------------------

    pub fn trim(&self) -> Expr {
        self.std1(StdFn::Trim)
    }

    /// Whether a text is empty.
    pub fn is_empty(&self) -> Expr {
        self.std1(StdFn::IsEmpty)
    }

    pub fn lower(&self) -> Expr {
        self.std1(StdFn::Lower)
    }

    pub fn is_alnum(&self) -> Expr {
        self.std1(StdFn::IsAlnum)
    }

    /// A text as a list of one-character texts.
    pub fn chars(&self) -> Expr {
        self.std1(StdFn::Chars)
    }

    /// The length of a text, in characters.
    pub fn text_len(&self) -> Expr {
        self.std1(StdFn::TextLen)
    }

    pub fn starts_with(&self, prefix: impl Into<Expr>) -> Expr {
        self.std2(StdFn::StartsWith, prefix)
    }

    /// `Some { before, after }` around the first occurrence, or `None`.
    pub fn split_once(&self, sep: impl Into<Expr>) -> Expr {
        self.std2(StdFn::SplitOnce, sep)
    }

    /// A text's UTF-8 bytes.
    pub fn utf8(&self) -> Expr {
        self.std1(StdFn::Utf8)
    }

    pub fn fnv1a64(&self) -> Expr {
        self.std1(StdFn::Fnv1a64)
    }

    /// An id parsed from its text, `None` if malformed; the verifier takes
    /// the table from context.
    pub fn id_of_text(&self) -> Expr {
        self.std1(StdFn::IdOfText)
    }

    /// A list of texts joined.
    pub fn concat(&self) -> Expr {
        self.std1(StdFn::Concat)
    }

    // -- bytes and ids ----------------------------------------------------------

    pub fn hex(&self) -> Expr {
        self.std1(StdFn::Hex)
    }

    pub fn sha256(&self) -> Expr {
        self.std1(StdFn::Sha256)
    }

    pub fn text_of_id(&self) -> Expr {
        self.std1(StdFn::TextOfId)
    }

    // -- lists ----------------------------------------------------------------

    /// The first element, or `None`.
    pub fn first(&self) -> Expr {
        self.std1(StdFn::First)
    }

    /// The last element, or `None`.
    pub fn last(&self) -> Expr {
        self.std1(StdFn::Last)
    }

    /// The length of a list.
    pub fn len(&self) -> Expr {
        self.std1(StdFn::Len)
    }

    pub fn contains(&self, e: impl Into<Expr>) -> Expr {
        self.std2(StdFn::Contains, e)
    }

    pub fn reverse(&self) -> Expr {
        self.std1(StdFn::Reverse)
    }

    /// `map xs (x -> body)`.
    pub fn map(&self, f: impl FnOnce(Expr) -> Expr) -> Expr {
        let (x, body) = self.bound(f);
        Expr(IrExpr::Map(Box::new(self.0.clone()), x, Box::new(body)))
    }

    /// `filter xs (x -> keep)`.
    pub fn filter(&self, f: impl FnOnce(Expr) -> Expr) -> Expr {
        let (x, body) = self.bound(f);
        Expr(IrExpr::Filter(Box::new(self.0.clone()), x, Box::new(body)))
    }

    /// `any xs (x -> holds)`.
    pub fn any(&self, f: impl FnOnce(Expr) -> Expr) -> Expr {
        let (x, body) = self.bound(f);
        Expr(IrExpr::Any(Box::new(self.0.clone()), x, Box::new(body)))
    }

    /// `all xs (x -> holds)`.
    pub fn all(&self, f: impl FnOnce(Expr) -> Expr) -> Expr {
        let (x, body) = self.bound(f);
        Expr(IrExpr::All(Box::new(self.0.clone()), x, Box::new(body)))
    }

    /// A stable sort by a key, under `compare_value`.
    pub fn sort_by(&self, f: impl FnOnce(Expr) -> Expr) -> Expr {
        let (x, key) = self.bound(f);
        Expr(IrExpr::SortBy(Box::new(self.0.clone()), x, Box::new(key)))
    }

    /// `fold xs init (acc, x -> body)`.
    pub fn fold(&self, init: impl Into<Expr>, f: impl FnOnce(Expr, Expr) -> Expr) -> Expr {
        let acc = fresh();
        let x = fresh();
        let body = f(Expr(IrExpr::Var(acc)), Expr(IrExpr::Var(x))).0;
        Expr(IrExpr::Fold(Box::new(self.0.clone()), Box::new(ir(init)), acc, x, Box::new(body)))
    }

    // -- options ----------------------------------------------------------------

    pub fn is_some(&self) -> Expr {
        self.std1(StdFn::IsSome)
    }

    /// The value, or the default: `Std.unwrapOr`.
    pub fn unwrap_or(&self, default: impl Into<Expr>) -> Expr {
        self.std2(StdFn::UnwrapOr, default)
    }

    /// The `Some` arm of a `match`, with the `None` arm still to be chosen.
    pub fn map_some(&self, f: impl FnOnce(Expr) -> Expr) -> MaybeExpr {
        let (sym, body) = self.bound(f);
        MaybeExpr {
            scrutinee: self.0.clone(),
            sym,
            body,
        }
    }
}

impl MaybeExpr {
    /// `match e { Some x -> body; None -> default }`: the value the `Some`
    /// arm computes, or the default, which has the arm's own type.
    pub fn unwrap_or(self, default: impl Into<Expr>) -> Expr {
        Expr(IrExpr::Match(
            Box::new(self.scrutinee),
            self.sym,
            Box::new(self.body),
            Box::new(ir(default)),
        ))
    }

    /// `match e { Some x -> Some body; None -> None }`, the `None` at the
    /// arm's type, which the builder cannot infer and so must be told.
    pub fn or_none(self, ty: Ty) -> Expr {
        Expr(IrExpr::Match(
            Box::new(self.scrutinee),
            self.sym,
            Box::new(IrExpr::Some(Box::new(self.body))),
            Box::new(IrExpr::None(ty.to_ir())),
        ))
    }
}

impl From<i64> for Expr {
    fn from(n: i64) -> Expr {
        Expr::int(n)
    }
}

impl From<i32> for Expr {
    fn from(n: i32) -> Expr {
        Expr::int(n as i64)
    }
}

impl From<bool> for Expr {
    fn from(b: bool) -> Expr {
        Expr::bool(b)
    }
}

impl From<&str> for Expr {
    fn from(s: &str) -> Expr {
        Expr::text(s)
    }
}

impl From<String> for Expr {
    fn from(s: String) -> Expr {
        Expr::lit(Value::Text(s))
    }
}

impl From<Value> for Expr {
    fn from(v: Value) -> Expr {
        Expr::lit(v)
    }
}

impl From<&Expr> for Expr {
    fn from(e: &Expr) -> Expr {
        e.clone()
    }
}

/// A struct: `{ name: e, … }`. Fields are sorted by name in the encoding
/// whatever order they are written in.
pub fn record<K: Into<String>>(fields: impl IntoIterator<Item = (K, Expr)>) -> Expr {
    Expr(IrExpr::Struct(fields.into_iter().map(|(k, e)| (k.into(), e.0)).collect()))
}

/// A list: `[e, …]`. An empty one needs its type from context.
pub fn list(items: impl IntoIterator<Item = Expr>) -> Expr {
    Expr(IrExpr::List(items.into_iter().map(|e| e.0).collect()))
}

/// The user the authority verified for the entry's connection.
pub fn ctx_user() -> Expr {
    Expr(IrExpr::CtxUser)
}

/// The login the entry was authored under.
pub fn ctx_session() -> Expr {
    Expr(IrExpr::CtxSession)
}

/// `None`, at the type of the value it lacks.
pub fn none(ty: Ty) -> Expr {
    Expr::none(ty)
}

/// `Some e`.
pub fn some(e: impl Into<Expr>) -> Expr {
    Expr::some(e)
}
