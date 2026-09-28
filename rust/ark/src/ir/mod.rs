//! §3 Ark IR, as `Ark.IR` defines it: the program a domain is.
//!
//! A module carries a schema, routers, functions and the types of its live
//! frames. A function is a procedure on a router (a mutator or a query),
//! a middleware (a guard or a provide) or a helper, with a body
//! in a small imperative core over a pure expression language. A domain
//! is written in the vocabulary of `spec/AUTHORING.md` ([`crate::authoring`]
//! here); run under `Emit` it yields this IR, run under `Native` it applies
//! entries directly, and [`crate::eval`] is what both mean. Three
//! properties are designed in: total (no loops but `for` over a list, no
//! recursion), and deterministic (no clock, no randomness, no I/O, no
//! floats).
//!
//! Spec version 3 (`spec/AUTHORING.md` §1; version 2 had scopes): routers and middleware, an
//! input schema with checks in place of bare arguments, `insert`/`upsert`/
//! `update` in place of `put`, and `provided`.
//!
//! The submodules are §7: [`encode`] writes a module as a [`Value`],
//! [`decode`] reads one back, [`normalize`] renumbers symbols. The closure
//! and function-hash definitions of `Ark.Hash` are re-exported from
//! [`crate::hash`].

pub mod decode;
pub mod encode;
pub mod normalize;

use std::collections::BTreeMap;

use crate::schema::{Dir, Relation, Schema, Ty};
use crate::value::{FieldName, TableName, Value};

pub use crate::hash::{closure, closures, function_hash, module_hash, Closure, FnHash};
pub use decode::{closure_from_value, function_from_value, module_from_value, router_from_value, schema_from_value, ty_from_value, DecodeError};
pub use encode::{calls, check_value, closure_value, field_value, function_value, module_value, reaches, router_value, schema_value, ty_value};
pub use normalize::{normalize, normalize_module};

/// The version of this specification a module was written against.
pub type SpecVersion = i64;

/// The version this crate implements (`Ark.IR.specVersion`).
pub const SPEC_VERSION: SpecVersion = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    pub spec: SpecVersion,
    pub schema: Schema,
    /// In declaration order; a helper may be called only by functions after
    /// it, which is what makes every call graph a DAG.
    pub functions: Vec<Function>,
    /// §1.1 The routers: groups of procedures, each naming the middleware
    /// it may use.
    pub routers: Vec<Router>,
    /// §3.9 The live section: the frame types an app's realtime channel
    /// carries, by name.
    pub live: Vec<(String, Ty)>,
}

impl Module {
    /// `Ark.IR.lookupFunction`.
    pub fn lookup_function(&self, name: &str) -> Option<&Function> {
        self.functions.iter().find(|f| f.name == name)
    }

    /// `Ark.IR.lookupRouter`.
    pub fn lookup_router(&self, name: &str) -> Option<&Router> {
        self.routers.iter().find(|r| r.name == name)
    }
}

/// §1.1 A router: a group of procedures sharing middleware.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Router {
    pub name: String,
    /// Its middleware, in order, by function name. A procedure's own
    /// [`Function::uses`] is the chain it runs, in this order.
    pub uses: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FnKind {
    /// Writes: takes a context and autos, reads and writes any table, may
    /// refuse.
    Mutator,
    /// Reads: takes arguments, may select from any table, returns a value.
    Query,
    /// Pure: no store access, no refusal, returns a value.
    Helper,
    /// §1.2 Middleware: runs before a procedure's body over the same store;
    /// may refuse; returns nothing.
    Guard,
    /// §1.2 Middleware: runs before the body; may refuse; returns a value
    /// the body reads as [`Expr::Provided`].
    Provide,
}

impl FnKind {
    /// The lowercase spelling `Ark.Encode` writes.
    pub fn name(self) -> &'static str {
        match self {
            FnKind::Mutator => "mutator",
            FnKind::Query => "query",
            FnKind::Helper => "helper",
            FnKind::Guard => "guard",
            FnKind::Provide => "provide",
        }
    }

    pub fn parse(s: &str) -> Option<FnKind> {
        Some(match s {
            "mutator" => FnKind::Mutator,
            "query" => FnKind::Query,
            "helper" => FnKind::Helper,
            "guard" => FnKind::Guard,
            "provide" => FnKind::Provide,
            _ => return None,
        })
    }

    /// A mutator or a query: something on a router that a caller invokes.
    pub fn is_procedure(self) -> bool {
        matches!(self, FnKind::Mutator | FnKind::Query)
    }

    /// A guard or a provide.
    pub fn is_middleware(self) -> bool {
        matches!(self, FnKind::Guard | FnKind::Provide)
    }
}

/// The non-determinism a mutator is allowed, by type; drawn once at the
/// originating peer and frozen in the entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Auto {
    /// A fresh id naming a row of this table.
    NewId(TableName),
    /// Milliseconds since the Unix epoch, as an int.
    Now,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Function {
    pub name: String,
    pub kind: FnKind,
    /// The router a procedure is on; `None` for helpers and middleware.
    pub router: Option<String>,
    /// The middleware a procedure runs before its body, in order: a
    /// subsequence of its router's [`Router::uses`]. Empty for anything
    /// that is not a procedure.
    pub uses: Vec<String>,
    pub autos: Vec<(String, Auto)>,
    /// §1.3 The input: each field's type and the checks run on it, in
    /// order, before anything else. For a middleware, the fields of the
    /// procedure's input it reads, by name and type.
    pub input: Vec<(String, Field)>,
    /// §1.3 Checks over the whole input, after the fields.
    pub refine: Vec<(Expr, Option<String>)>,
    /// The result type of a query, helper or provide; `None` for a mutator
    /// or a guard.
    pub ret: Option<Ty>,
    pub body: Block,
    /// The author's names for symbols; not hashed, not required.
    pub names: BTreeMap<Sym, String>,
}

impl Function {
    /// The input's fields by name and type alone, which is what a
    /// middleware declares and what the old `args` were.
    pub fn arg_types(&self) -> Vec<(String, Ty)> {
        self.input.iter().map(|(n, f)| (n.clone(), f.ty.clone())).collect()
    }
}

/// §1.3 One field of a procedure's input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub ty: Ty,
    pub checks: Vec<Check>,
}

impl Field {
    pub fn plain(ty: Ty) -> Field {
        Field { ty, checks: vec![] }
    }
}

/// §1.3 A check on one field. The message is `None` for the default
/// (`crate::eval::default_message`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Check {
    /// Text: normalise before every later check and before the body.
    Trim,
    /// Text: length in code points at least `n`.
    MinLen(i64, Option<String>),
    /// Text: length in code points at most `n`.
    MaxLen(i64, Option<String>),
    /// Int: `lo <= v <= hi`, either bound optional.
    Range(Option<i64>, Option<i64>, Option<String>),
    /// List: at least one element.
    NonEmpty(Option<String>),
    /// Id: a row with that key exists.
    Exists(Option<String>),
    /// Any: the expression, over `Arg <this field>`, is true.
    Refine(Expr, Option<String>),
}

/// A local variable, alpha-normalised: the `n`th binding in a function.
pub type Sym = i64;

pub type Block = Vec<Stmt>;

/// §3.1 Statements.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stmt {
    /// Bind a value; a read may appear only as the whole right-hand side.
    Let(Sym, Expr),
    If(Expr, Block, Block),
    /// Iterate a list, binding each element.
    For(Sym, Expr, Block),
    /// §1.4 Write the row unless one matches on the columns (the table's
    /// key when the list is empty, otherwise a declared unique index).
    Insert(TableName, Expr, Vec<FieldName>),
    /// §1.4 Write the row; if one matches on the columns, keep its key
    /// columns and take the rest from the new row. `Upsert t e []` is the
    /// old `Put t e`.
    Upsert(TableName, Expr, Vec<FieldName>),
    /// §1.4 The row under the key, bound to the symbol, replaced by the
    /// expression; a no-op when absent.
    Update(TableName, Vec<Expr>, Sym, Expr),
    /// Delete by key.
    Delete(TableName, Vec<Expr>),
    /// End the mutator with a deterministic verdict.
    Refuse(Expr),
    /// Leave the function, with a value for a query or helper.
    Return(Option<Expr>),
}

/// §3.2 Expressions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expr {
    Lit(Value),
    Arg(String),
    Auto(String),
    Var(Sym),
    /// The user the authority verified for the entry's connection.
    CtxUser,
    /// The login the entry was authored under.
    CtxSession,
    /// §1.2 What the named `Provide` middleware returned.
    Provided(String),
    Field(Box<Expr>, FieldName),
    Struct(BTreeMap<FieldName, Expr>),
    List(Vec<Expr>),
    /// `Some e`.
    Some(Box<Expr>),
    /// `None`, at the type given.
    None(Ty),
    /// `match e { Some x -> a; None -> b }`.
    Match(Box<Expr>, Sym, Box<Expr>, Box<Expr>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    Op(Op, Vec<Expr>),
    Cmp(CmpOp, Box<Expr>, Box<Expr>),
    /// A call to a helper declared earlier in the module.
    Call(String, Vec<Expr>),
    Std(StdFn, Vec<Expr>),
    Map(Box<Expr>, Sym, Box<Expr>),
    Filter(Box<Expr>, Sym, Box<Expr>),
    Any(Box<Expr>, Sym, Box<Expr>),
    All(Box<Expr>, Sym, Box<Expr>),
    /// Stable sort by a key expression, under `compare_value`.
    SortBy(Box<Expr>, Sym, Box<Expr>),
    /// `fold xs init (acc, x -> body)`.
    Fold(Box<Expr>, Box<Expr>, Sym, Sym, Box<Expr>),
    Select(Box<Plan>),
    Get(TableName, Vec<Expr>),
    Exists(TableName, Vec<Expr>),
}

/// Arithmetic and boolean operators. Integer arithmetic is checked:
/// overflow, division by zero and `MIN / -1` are refusals, never wrapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Op {
    Add,
    Sub,
    Mul,
    /// Truncating toward zero.
    Div,
    /// The remainder with the dividend's sign.
    Mod,
    Neg,
    And,
    Or,
    Not,
}

impl Op {
    /// The lowercase spelling `Ark.Encode` writes.
    pub fn name(self) -> &'static str {
        match self {
            Op::Add => "add",
            Op::Sub => "sub",
            Op::Mul => "mul",
            Op::Div => "div",
            Op::Mod => "mod",
            Op::Neg => "neg",
            Op::And => "and",
            Op::Or => "or",
            Op::Not => "not",
        }
    }

    /// The Haskell constructor's spelling, as `show` gives it.
    pub fn show(self) -> &'static str {
        match self {
            Op::Add => "Add",
            Op::Sub => "Sub",
            Op::Mul => "Mul",
            Op::Div => "Div",
            Op::Mod => "Mod",
            Op::Neg => "Neg",
            Op::And => "And",
            Op::Or => "Or",
            Op::Not => "Not",
        }
    }

    pub fn parse(s: &str) -> Option<Op> {
        Some(match s {
            "add" => Op::Add,
            "sub" => Op::Sub,
            "mul" => Op::Mul,
            "div" => Op::Div,
            "mod" => Op::Mod,
            "neg" => Op::Neg,
            "and" => Op::And,
            "or" => Op::Or,
            "not" => Op::Not,
            _ => return None,
        })
    }
}

/// Comparison under `compare_value`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl CmpOp {
    /// The lowercase spelling `Ark.Encode` writes.
    pub fn name(self) -> &'static str {
        match self {
            CmpOp::Eq => "eq",
            CmpOp::Ne => "ne",
            CmpOp::Lt => "lt",
            CmpOp::Le => "le",
            CmpOp::Gt => "gt",
            CmpOp::Ge => "ge",
        }
    }

    pub fn parse(s: &str) -> Option<CmpOp> {
        Some(match s {
            "eq" => CmpOp::Eq,
            "ne" => CmpOp::Ne,
            "lt" => CmpOp::Lt,
            "le" => CmpOp::Le,
            "gt" => CmpOp::Gt,
            "ge" => CmpOp::Ge,
            _ => return None,
        })
    }
}

/// §3.3 A plan: a query's maintainable half, what `select` pulls and what a
/// view maintains.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub table: TableName,
    pub filter: Option<Pred>,
    /// The verifier makes every order total by appending the key columns
    /// ascending.
    pub order: Vec<(FieldName, Dir)>,
    pub limit: Option<i64>,
    /// Relationships read beneath each row, each a field of that name
    /// holding a list of child rows.
    pub related: Vec<Related>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Related {
    pub name: FieldName,
    pub relation: Relation,
    pub plan: Plan,
}

/// A filter over one row; the right-hand sides may not mention the row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pred {
    Cmp(FieldName, CmpOp, Expr),
    In(FieldName, Vec<Expr>),
    All(Vec<Pred>),
    Any(Vec<Pred>),
    Not(Box<Pred>),
}

/// §3.4 The standard library, by name; `Ark.Std` is its meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StdFn {
    Trim,
    IsEmpty,
    Concat,
    Lower,
    IsAlnum,
    Chars,
    TextLen,
    StartsWith,
    SplitOnce,
    TextOfInt,
    Hex,
    Min,
    Max,
    Clamp,
    Abs,
    Fnv1a64,
    Sha256,
    IdOfText,
    TextOfId,
    NilId,
    Utf8,
    First,
    Last,
    Len,
    Contains,
    Reverse,
    IsSome,
    UnwrapOr,
    /// The option's value, or the refusal `unwrapped none`
    /// (`spec/AUTHORING.md` §6: what `or_refuse` lowers to).
    Unwrap,
}

impl StdFn {
    /// Every function, in the spec's order.
    pub const ALL: [StdFn; 29] = [
        StdFn::Trim,
        StdFn::IsEmpty,
        StdFn::Concat,
        StdFn::Lower,
        StdFn::IsAlnum,
        StdFn::Chars,
        StdFn::TextLen,
        StdFn::StartsWith,
        StdFn::SplitOnce,
        StdFn::TextOfInt,
        StdFn::Hex,
        StdFn::Min,
        StdFn::Max,
        StdFn::Clamp,
        StdFn::Abs,
        StdFn::Fnv1a64,
        StdFn::Sha256,
        StdFn::IdOfText,
        StdFn::TextOfId,
        StdFn::NilId,
        StdFn::Utf8,
        StdFn::First,
        StdFn::Last,
        StdFn::Len,
        StdFn::Contains,
        StdFn::Reverse,
        StdFn::IsSome,
        StdFn::UnwrapOr,
        StdFn::Unwrap,
    ];

    /// The constructor's `show` spelling, which is how a module names it.
    pub fn show(self) -> &'static str {
        match self {
            StdFn::Trim => "Trim",
            StdFn::IsEmpty => "IsEmpty",
            StdFn::Concat => "Concat",
            StdFn::Lower => "Lower",
            StdFn::IsAlnum => "IsAlnum",
            StdFn::Chars => "Chars",
            StdFn::TextLen => "TextLen",
            StdFn::StartsWith => "StartsWith",
            StdFn::SplitOnce => "SplitOnce",
            StdFn::TextOfInt => "TextOfInt",
            StdFn::Hex => "Hex",
            StdFn::Min => "Min",
            StdFn::Max => "Max",
            StdFn::Clamp => "Clamp",
            StdFn::Abs => "Abs",
            StdFn::Fnv1a64 => "Fnv1a64",
            StdFn::Sha256 => "Sha256",
            StdFn::IdOfText => "IdOfText",
            StdFn::TextOfId => "TextOfId",
            StdFn::NilId => "NilId",
            StdFn::Utf8 => "Utf8",
            StdFn::First => "First",
            StdFn::Last => "Last",
            StdFn::Len => "Len",
            StdFn::Contains => "Contains",
            StdFn::Reverse => "Reverse",
            StdFn::IsSome => "IsSome",
            StdFn::UnwrapOr => "UnwrapOr",
            StdFn::Unwrap => "Unwrap",
        }
    }

    pub fn parse(name: &str) -> Option<StdFn> {
        StdFn::ALL.iter().copied().find(|f| f.show() == name)
    }

    /// How many arguments each function takes (`Ark.Std.arity`).
    pub fn arity(self) -> usize {
        match self {
            StdFn::StartsWith | StdFn::SplitOnce | StdFn::Min | StdFn::Max | StdFn::Contains | StdFn::UnwrapOr => 2,
            StdFn::Clamp => 3,
            StdFn::NilId => 0,
            _ => 1,
        }
    }
}
