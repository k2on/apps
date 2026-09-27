//! §3 Ark IR, as `Ark.IR` defines it: the program a domain is.
//!
//! A module carries a schema, functions and the types of its live frames. A
//! function is a mutator, a query or a helper, with a body in a small
//! imperative core over a pure expression language. Nobody runs the IR in
//! production; generated code is held to what [`crate::eval`] says it
//! means. Three properties are designed in: total (no loops but `for` over
//! a list, no recursion), deterministic (no clock, no randomness, no I/O,
//! no floats), scoped (a mutator touches one scope).
//!
//! The submodules are §7: [`encode`] writes a module as a [`Value`],
//! [`decode`] reads one back, [`normalize`] renumbers symbols. The closure
//! and function-hash definitions of `Ark.Hash` are re-exported from
//! [`crate::hash`].

pub mod decode;
pub mod encode;
pub mod normalize;

use std::collections::BTreeMap;

use crate::schema::{Dir, Relation, Schema, ScopeName, Ty};
use crate::value::{FieldName, TableName, Value};

pub use crate::hash::{closure, closures, function_hash, module_hash, Closure, FnHash};
pub use decode::{closure_from_value, function_from_value, module_from_value, schema_from_value, ty_from_value, DecodeError};
pub use encode::{calls, closure_value, function_value, module_value, schema_value, ty_value};
pub use normalize::{normalize, normalize_module};

/// The version of this specification a module was written against.
pub type SpecVersion = i64;

/// The version this crate implements (`Ark.IR.specVersion`).
pub const SPEC_VERSION: SpecVersion = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    pub spec: SpecVersion,
    pub schema: Schema,
    /// In declaration order; a helper may be called only by functions after
    /// it, which is what makes every call graph a DAG.
    pub functions: Vec<Function>,
    /// §3.9 The live section: the frame types an app's realtime channel
    /// carries, by name.
    pub live: Vec<(String, Ty)>,
}

impl Module {
    /// `Ark.IR.lookupFunction`.
    pub fn lookup_function(&self, name: &str) -> Option<&Function> {
        self.functions.iter().find(|f| f.name == name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FnKind {
    /// Writes: takes a context and autos, reads and writes one scope, may
    /// refuse.
    Mutator,
    /// Reads: takes arguments, may select from any scope, returns a value.
    Query,
    /// Pure: no store access, no refusal, returns a value.
    Helper,
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
    /// The scope a mutator belongs to; `None` for queries and helpers.
    pub scope: Option<ScopeName>,
    pub autos: Vec<(String, Auto)>,
    pub args: Vec<(String, Ty)>,
    /// The result type of a query or helper; `None` for a mutator.
    pub ret: Option<Ty>,
    pub body: Block,
    /// The author's names for symbols; not hashed, not required.
    pub names: BTreeMap<Sym, String>,
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
    /// Write a full row; see `Ark.Store.put`.
    Put(TableName, Expr),
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
}

impl StdFn {
    /// Every function, in the spec's order.
    pub const ALL: [StdFn; 28] = [
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
