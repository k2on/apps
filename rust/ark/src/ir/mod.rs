//! §3 Ark IR, as `Ark.IR` defines it: the program a domain is.
//!
//! A module carries a schema, routers, functions and the types of its live
//! frames. A function is a procedure on a router (a mutator or a query),
//! a middleware (a guard, a provide or a scope) or a helper, with a body
//! in a small imperative core over a pure expression language. A domain
//! is written in the vocabulary of `spec/AUTHORING.md` ([`crate::authoring`]
//! here); run under `Emit` it yields this IR, run under `Native` it applies
//! entries directly, and [`crate::eval`] is what both mean. Three
//! properties are designed in: total (no loops but `for` over a list, no
//! recursion), and deterministic (no clock, no randomness, no I/O, no
//! floats).
//!
//! Spec version 4 (`docs/plan-v4.md` §1; `spec/AUTHORING.md` §1): a query
//! is a [`Plan`] and nothing else ([`Function::plan`]), and a plan is a tree
//! of reads — a source (a table, or a table grouped by columns), a filter,
//! lookups, related plans on any column equality, a having, a projection,
//! an order that may use expressions, a limit. Version 3 brought routers
//! and middleware, an input schema with checks in place of bare arguments,
//! `insert`/`upsert`/`update` in place of `put`, and `provided`; version 2
//! had scopes.
//!
//! The submodules are §7: [`encode`] writes a module as a [`Value`],
//! [`decode`] reads one back, [`normalize`] renumbers symbols. The closure
//! and function-hash definitions of `Ark.Hash` are re-exported from
//! [`crate::hash`].

pub mod decode;
pub mod encode;
pub mod normalize;
pub mod ordering;
pub mod reads;

use std::collections::BTreeMap;

use crate::schema::{Dir, Schema, Ty};
use crate::value::{FieldName, TableName, Value};

pub use crate::hash::{closure, closures, function_hash, module_hash, Closure, FnHash};
pub use decode::{
    closure_from_value, expr_from_value, function_from_value, hold_from_value, module_from_value, plan_from_value, router_from_value,
    schema_from_value, state_from_value, ty_from_value, DecodeError,
};
pub use encode::{
    calls, check_value, closure_value, expr_value, field_value, function_value, hold_value, module_value, plan_value, reaches, router_value,
    schema_value, state_calls, state_value, transition_calls, transition_value, ty_value, type_decl_value,
};
pub use normalize::{normalize, normalize_module, normalize_state, normalize_transition};
pub use reads::reads;

/// The version of this specification a module was written against.
pub type SpecVersion = i64;

/// The version this crate implements.
pub const SPEC_VERSION: SpecVersion = 4;

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
    /// §2.2 The type section (`docs/plan-live.md`): the records and sums a
    /// live state is made of, each by its name, which [`Ty::Named`]
    /// refers to — what a generator prints as a type of its own. No type
    /// refers to itself, through any number of others. On the wire
    /// `types`, a list of `{"t":"type","name","ty"}`, written only when
    /// not empty: a module without one is the bytes it was.
    pub types: Vec<(String, Ty)>,
    /// §3.12 The live states (`docs/plan-live.md`): a typed state machine
    /// per scope, beside the log. On the wire `states`, written only when
    /// not empty.
    pub states: Vec<State>,
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

    /// A live state by name.
    pub fn lookup_state(&self, name: &str) -> Option<&State> {
        self.states.iter().find(|s| s.name == name)
    }

    /// The type section as a map, which is how a named type is resolved.
    pub fn type_map(&self) -> BTreeMap<String, Ty> {
        self.types.iter().cloned().collect()
    }
}

/// §3.12 A live state (`docs/plan-live.md`): one current value per
/// instance, an instance per scope key, held by the server and never
/// replayed tomorrow. Its value is of `ty`; it starts as `init`; `scope`
/// names the instances a person is subscribed to, as a function of who
/// they are; and it moves only by its transitions, each a pure function of
/// the state, the call's context and its input, which the server sequences
/// per instance. The three hooks are transitions the engine runs itself: a
/// device arriving, a device leaving, and a snapshot restored at start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    pub name: String,
    pub ty: Ty,
    /// A constant expression of `ty`: the value an instance starts as.
    pub init: Expr,
    /// A `Text`, or a `List` of them, of the context alone (`ctx.user`,
    /// `ctx.session`, roles, literals): the keys of the instances a person
    /// holds, as a scope's holds are a function of who they are.
    pub scope: Expr,
    pub transitions: Vec<Transition>,
    /// Run by the server when a device first stands in an instance; its
    /// input is `device`, the device's record ([`device_ty`]) as it
    /// declared itself at `Hello`.
    pub on_join: Option<Transition>,
    /// Run by the server when a device's last connection to an instance
    /// goes, its input `device` as for `on_join`. Before it runs, the
    /// engine clears the device's slot of every ephemeral field; its
    /// ordered fields are the hook's to clear.
    pub on_leave: Option<Transition>,
    /// Run by the server over a snapshot it restores at start, with no
    /// input and nobody present.
    pub on_restore: Option<Transition>,
}

impl State {
    /// A transition by name, a hook by its reserved name.
    pub fn transition(&self, name: &str) -> Option<&Transition> {
        match name {
            ON_JOIN => self.on_join.as_ref(),
            ON_LEAVE => self.on_leave.as_ref(),
            ON_RESTORE => self.on_restore.as_ref(),
            _ => self.transitions.iter().find(|t| t.name == name),
        }
    }

    /// Every transition and hook, declared ones first, then the hooks in
    /// the order join, leave, restore.
    pub fn all_transitions(&self) -> impl Iterator<Item = &Transition> {
        self.transitions
            .iter()
            .chain(self.on_join.iter())
            .chain(self.on_leave.iter())
            .chain(self.on_restore.iter())
    }
}

/// The reserved names of the three hooks: what an `applied` frame names a
/// system transition by, and what no declared transition may be called.
pub const ON_JOIN: &str = "on_join";
pub const ON_LEAVE: &str = "on_leave";
pub const ON_RESTORE: &str = "on_restore";

/// §3.12 One way a live state moves: its input, with the checks a
/// procedure's input has; its guards, each a `Bool` over the state, the
/// context and the input that refuses with its message when false; and a
/// body over the same, which returns the new state or refuses. It reads no
/// table and draws no auto: the server's clock is `ctx.now`
/// ([`Expr::CtxNow`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transition {
    pub name: String,
    pub input: Vec<(String, Field)>,
    pub guards: Vec<(Expr, String)>,
    pub body: Block,
    /// The author's names for symbols; not hashed.
    pub names: BTreeMap<Sym, String>,
}

/// §3.12 A hook's `device`: one device standing in an instance, as it
/// declared itself at `Hello` — its id, which survives its reconnects,
/// what kind of thing it is, its name, and whether it can play — and the
/// user the server verified it as. Who is here is an app's own field (a
/// `PerDevice` of this, kept by its hooks), not the engine's.
pub fn device_ty() -> Ty {
    Ty::Struct(
        [
            ("can_play".to_string(), Ty::Bool),
            ("id".to_string(), Ty::Text),
            ("kind".to_string(), Ty::Text),
            ("name".to_string(), Ty::Text),
            ("user".to_string(), Ty::Text),
        ]
        .into_iter()
        .collect(),
    )
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
    /// `docs/plan-guards.md` D2 Middleware that runs nothing: what the
    /// person a procedure is called by holds, as a function of `ctx`
    /// alone — the rows and columns of each table it names
    /// ([`Function::holds`]). No input, no body, no return; listed in a
    /// procedure's `uses` like any middleware and hashed into its closure
    /// like one, so that what a procedure is served over is part of what it
    /// is. A person holds the union of every scope on every procedure of the
    /// module ([`crate::scope::Holdings`]); a module with none holds
    /// everything, and is served as it always was.
    Scope,
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
            FnKind::Scope => "scope",
        }
    }

    pub fn parse(s: &str) -> Option<FnKind> {
        Some(match s {
            "mutator" => FnKind::Mutator,
            "query" => FnKind::Query,
            "helper" => FnKind::Helper,
            "guard" => FnKind::Guard,
            "provide" => FnKind::Provide,
            "scope" => FnKind::Scope,
            _ => return None,
        })
    }

    /// A mutator or a query: something on a router that a caller invokes.
    pub fn is_procedure(self) -> bool {
        matches!(self, FnKind::Mutator | FnKind::Query)
    }

    /// A guard, a provide or a scope.
    pub fn is_middleware(self) -> bool {
        matches!(self, FnKind::Guard | FnKind::Provide | FnKind::Scope)
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
    /// §1.4 A query's whole meaning: `Some` for a query, whose `body` is
    /// then empty, and `None` for every other kind.
    pub plan: Option<Plan>,
    /// `docs/plan-guards.md` D2 A scope's whole meaning: what it holds of
    /// each table it names, at least one; empty for every other kind. On
    /// the wire `holds`, written only for a scope, so every function of
    /// every module before scopes is the bytes it was.
    pub holds: Vec<Hold>,
    /// `docs/plan-guards.md` D3 A mutator with a server half: its body
    /// carries [`Stmt::Private`] blocks, or carried them before they were
    /// stripped from the module a client loads ([`strip`]). Hashed, so a
    /// client knows from its own module which entries it takes by the
    /// authority's facts rather than by its own run. On the wire
    /// `private: true`, written only when true, so every function before
    /// private blocks is the bytes it was.
    pub private: bool,
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

/// `docs/plan-guards.md` D2 What a scope holds of one table: the rows its
/// filter admits — over the table's own columns, compared with `ctx`
/// ([`Expr::CtxUser`], [`Expr::CtxSession`]) or literals, with the two
/// leaves only a scope has ([`Pred::When`], [`Pred::Exists`]) — and the
/// columns its projection keeps. No filter is every row. Never `input`: a
/// scope is a function of who the person is, so what they hold is fixed
/// per person, complete offline, and computable at `Hello`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hold {
    pub table: TableName,
    pub filter: Option<Pred>,
    pub columns: Projection,
}

/// `docs/plan-guards.md` D2 Which columns of a held row a scope keeps. A
/// projection keeps the key whatever it says (the verifier refuses one that
/// drops a key column), since a row is named by its key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Projection {
    /// Every column: the absence of `pick` and `exclude` on the wire.
    All,
    /// These columns and no others: `pick`.
    Pick(Vec<FieldName>),
    /// Every column but these: `exclude`.
    Exclude(Vec<FieldName>),
}

impl Projection {
    /// Whether the projection keeps `column` of a table.
    pub fn keeps(&self, column: &str) -> bool {
        match self {
            Projection::All => true,
            Projection::Pick(cs) => cs.iter().any(|c| c == column),
            Projection::Exclude(cs) => !cs.iter().any(|c| c == column),
        }
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
    /// `docs/plan-guards.md` D3 A mutator's server half (`ctx.private`):
    /// run only by the authority, and **after** the public body, in the
    /// order the blocks were reached, each over the locals bound where it
    /// was written — so the public body can never have read what a private
    /// block wrote, and a client reproduces it. May read and write any table
    /// and column, including what no person's union holds, and may refuse:
    /// the refusal is the entry's verdict. Never in a client's module:
    /// [`strip`] removes it, and the function's hash is of what is left. On
    /// the wire `{"t":"private","body"}`.
    Private(Block),
}

/// `docs/plan-guards.md` D3 A function as a client's module carries it:
/// every [`Stmt::Private`] taken out, wherever it is nested, and
/// [`Function::private`] kept — what the function's hash is taken of
/// ([`crate::hash::function_hash`]). A function with no block is itself.
pub fn strip(f: &Function) -> Function {
    if !f.private {
        return f.clone();
    }
    fn block(b: &Block) -> Block {
        b.iter()
            .filter(|s| !matches!(s, Stmt::Private(_)))
            .map(|s| match s {
                Stmt::If(c, a, e) => Stmt::If(c.clone(), block(a), block(e)),
                Stmt::For(x, xs, body) => Stmt::For(*x, xs.clone(), block(body)),
                other => other.clone(),
            })
            .collect()
    }
    Function {
        body: block(&f.body),
        ..f.clone()
    }
}

/// `docs/plan-guards.md` D3 The module a client loads: every function
/// [`strip`]ped. `harken.ark` is one; the server keeps the blocks. The two
/// name every function by the same hash, and differ by module hash only
/// where a block was taken out.
pub fn strip_module(m: &Module) -> Module {
    Module {
        functions: m.functions.iter().map(strip).collect(),
        ..m.clone()
    }
}

/// `docs/plan-guards.md` D3 Whether a block holds a [`Stmt::Private`],
/// at any depth.
pub fn has_private(b: &Block) -> bool {
    b.iter().any(|s| match s {
        Stmt::Private(_) => true,
        Stmt::If(_, a, e) => has_private(a) || has_private(e),
        Stmt::For(_, _, body) => has_private(body),
        _ => false,
    })
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
    /// Whether the entry's author holds the named role: a `Bool`, read off
    /// the roles frozen in the entry, which the authority stamped with the
    /// connection's (`docs/plan-guards.md` D1). Reads no table. The name is
    /// not empty (`Complaint::EmptyRole`).
    HasRole(String),
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
    /// §2.2 A value of a sum: the variant `name` of the sum `ty` (usually
    /// [`Ty::Named`]), with these fields. Its value is the struct of the
    /// fields with `"t"` the variant's name. On the wire
    /// `{"t":"variant","ty","name","fields"}`, written only where used.
    Variant(Ty, String, BTreeMap<FieldName, Expr>),
    /// §2.2 A sum taken apart: one arm per variant, in the sum's variant
    /// order (by name), each binding the variant's record — its fields,
    /// without the tag — to its symbol. On the wire
    /// `{"t":"case","e","arms":[{"t":"arm","name","sym","e"}]}`.
    Case(Box<Expr>, Vec<(String, Sym, Expr)>),
    /// §3.12 The state a transition runs over. On the wire
    /// `{"t":"live_state"}`; only in a transition.
    LiveState,
    /// §3.12 The time a transition runs at, in milliseconds since the Unix
    /// epoch: the server's clock when it sequences a call, the device's own
    /// guess of the server's when it previews one. `{"t":"ctx_now"}`; only
    /// in a transition.
    CtxNow,
    /// §3.12 The id of the device calling — for a hook, the device
    /// arriving or leaving. `{"t":"ctx_device"}`; only in a transition.
    CtxDevice,
    /// §2.2 One device's slot of a `PerDevice`: an `Option` of its value
    /// type, `None` where the slot holds nothing. `{"t":"slot","e","key"}`.
    Slot(Box<Expr>, Box<Expr>),
    /// §2.2 A `PerDevice` with one slot set to the option's value, or
    /// emptied by `None`. `{"t":"set_slot","e","key","v"}`.
    SetSlot(Box<Expr>, Box<Expr>, Box<Expr>),
    /// §2.2 A `PerDevice`'s filled slots as a list of `{device, value}`
    /// records in device-id order. `{"t":"slots","e"}`.
    Slots(Box<Expr>),
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

/// §1.3 A plan: a tree of reads, what a query is, what `select` pulls and
/// what a view maintains. Each node is evaluated in this order: the source
/// rows the filter admits; `row` (and `members`) bound; the lookups in
/// order; the related plans; `having`; `project`; the order keys. The
/// limit is a window over the admitted nodes in order.
///
/// Scope is flat per node: `having`, `project`, lookup keys, `on` and
/// expression order keys see this node's binders and the function's
/// arguments, context and provided values — never a parent's, which a
/// child reaches through its `on`. The filter sees only what is constant
/// for the read (and, in a mutator's body, the locals bound before it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub source: Source,
    /// Over the source table's columns; the right-hand sides are constant
    /// for the read. What a store's indexes serve.
    pub filter: Option<Pred>,
    /// The binder for the source row, or for a group its key struct.
    /// Absent exactly when nothing could reference it: a plan with no
    /// lookups, related plans, having, projection, expression order key or
    /// group — the v3 shape a mutator's reads have, whose bytes and symbol
    /// numbering (and so whose function hashes) are unchanged by v4.
    pub row: Option<Sym>,
    /// A group source only: the group's rows, as a list in key order.
    pub members: Option<Sym>,
    /// Rows by key from other tables, in order; each may use the ones
    /// before it.
    pub lookups: Vec<Lookup>,
    /// Child plans, each a list per node.
    pub related: Vec<Related>,
    /// Keep the node when true; the node still exists to a view, which is
    /// what lets it appear when a child arrives.
    pub having: Option<Expr>,
    /// The node's value. Absent, the node is the row's columns (a group's
    /// key columns) plus one field per related list, named by its `name`.
    pub project: Option<Expr>,
    /// The verifier makes every order total by appending the key columns
    /// ascending (a group's `by` columns).
    pub order: Vec<(Key, Dir)>,
    pub limit: Option<i64>,
}

/// Where a plan's nodes come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// One node per row the filter admits.
    Table(TableName),
    /// The rows the filter admits, grouped by the values of the `by`
    /// columns: one node per distinct tuple.
    Group { table: TableName, by: Vec<FieldName> },
}

impl Source {
    /// The table the rows are read from.
    pub fn table(&self) -> &TableName {
        match self {
            Source::Table(t) | Source::Group { table: t, .. } => t,
        }
    }
}

/// A row by key from another table, bound to `sym` as an option: the row
/// under the key the expressions compute, `None` when there is none or
/// when any key part is `Null`. How a reference is followed upward.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lookup {
    pub name: FieldName,
    pub sym: Sym,
    pub table: TableName,
    pub key: Vec<Expr>,
}

/// A child plan beneath each node, bound to `sym` as the list of its
/// nodes: evaluated with its own filter and `child.column == expr(parent)`
/// for every pair in `on`, its order and limit per parent. A reference
/// declared in the schema is one case — `on = [(fk, row.key)]`, what
/// `.with(Table::rel)` writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Related {
    pub name: FieldName,
    pub sym: Sym,
    pub on: Vec<(FieldName, Expr)>,
    pub plan: Plan,
}

/// One key of an order: a column of the source row (a group's key), or an
/// expression over the node's binders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Column(FieldName),
    Expr(Expr),
}

impl Plan {
    /// The table the plan reads.
    pub fn table(&self) -> &TableName {
        self.source.table()
    }

    /// §1.4 The shape a mutator's read may have — v3's: a table source, no
    /// lookups, having, projection or expression order key, and every
    /// related plan the `on` form of a reference (`[(column,
    /// row.field)]`, which the verifier holds to the schema) and itself of
    /// this shape. What the incremental view of v3 maintains.
    pub fn is_v3_shaped(&self) -> bool {
        matches!(self.source, Source::Table(_))
            && self.members.is_none()
            && self.lookups.is_empty()
            && self.having.is_none()
            && self.project.is_none()
            && self.order.iter().all(|(k, _)| matches!(k, Key::Column(_)))
            && self
                .related
                .iter()
                .all(|r| matches!((&r.on[..], self.row), ([(_, Expr::Field(e, _))], Some(row)) if **e == Expr::Var(row)) && r.plan.is_v3_shaped())
    }

    /// v3-shaped with nothing beneath: a plan whose `row` nothing can
    /// reference, and which therefore carries none.
    pub fn is_bare(&self) -> bool {
        self.is_v3_shaped() && self.related.is_empty()
    }

    /// The columns an order sorts by, when every key is a column.
    pub fn order_columns(&self) -> Option<Vec<(FieldName, Dir)>> {
        self.order
            .iter()
            .map(|(k, d)| match k {
                Key::Column(c) => Some((c.clone(), *d)),
                Key::Expr(_) => None,
            })
            .collect()
    }
}

/// A filter over one row; the right-hand sides may not mention the row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pred {
    Cmp(FieldName, CmpOp, Expr),
    In(FieldName, Vec<Expr>),
    All(Vec<Pred>),
    Any(Vec<Pred>),
    Not(Box<Pred>),
    /// `docs/plan-db.md` D4 The text column holds the needle as a
    /// substring, both sides folded by the pinned Unicode tables (`lower`,
    /// `unicode_tables.rs`), so that every peer finds the same rows. A
    /// `Null` column holds nothing; every text holds the empty needle. What
    /// a text index on the column serves (`Table::text`); without one it is
    /// a scan, as any filter. On the wire `phas`, written only where used,
    /// so no existing plan's bytes move.
    Has(FieldName, Expr),
    /// `docs/plan-guards.md` D2 A scope's leaf: a `Bool` that reads no row
    /// — `has_role(..)`, or a comparison of the context with a literal — so
    /// it holds for every row or for none, decided by who the person is.
    /// Only in a scope ([`Hold::filter`]); the verifier refuses it in a
    /// plan (`ScopeLeafInPlan`). On the wire `pwhen`, written only where
    /// used.
    When(Expr),
    /// `docs/plan-guards.md` D2 A scope's one reach into another table:
    /// some row of `table` whose reference column `column` names this row
    /// is admitted by the predicate — `membership` rows by `org`, with
    /// `user == ctx.user` — which is over that table's own columns and the
    /// context and has no further `Exists`. Read through the reference
    /// index a store keeps on every reference column, so it costs the
    /// referencing rows of one row; what it reads is what
    /// [`crate::ir::reads`] says of the scope. Only in a scope, as `When`
    /// is; on the wire `pexists`.
    Exists(TableName, FieldName, Box<Pred>),
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
    /// `ListConcat(a, b)`: the elements of `a`, then those of `b`. `Concat`
    /// is the text one, which joins a list of texts. Like the four below,
    /// added after spec version 4 was written and a name a module that does
    /// not use it never carries, so no module before it moves.
    ListConcat,
    /// `Insert(xs, i, x)`: `xs` with `x` at index `i` and every element from
    /// `i` on one place later; `i` is `0..=len`, `len` being the end. Any
    /// other index is a refusal ([`crate::stdlib`]).
    Insert,
    /// `RemoveAt(xs, i)`: `xs` without the element at `i`, which is
    /// `0..len`; any other index is a refusal.
    RemoveAt,
    /// `Move(xs, from, to)`: `xs` with the element at `from` taken out and
    /// put back so that it is at `to`, every element between them one place
    /// over; both are `0..len`, and any other index is a refusal, `from`'s
    /// first.
    Move,
    /// `Slice(xs, start, end)`: the elements from `start` up to and without
    /// `end`, where `0 <= start <= end <= len`; anything else is a refusal.
    Slice,
    IsSome,
    UnwrapOr,
    /// The option's value, or the refusal `unwrapped none`
    /// (`spec/AUTHORING.md` §6: what `or_refuse` lowers to).
    Unwrap,
}

impl StdFn {
    /// Every function, in the spec's order.
    pub const ALL: [StdFn; 34] = [
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
        StdFn::ListConcat,
        StdFn::Insert,
        StdFn::RemoveAt,
        StdFn::Move,
        StdFn::Slice,
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
            StdFn::ListConcat => "ListConcat",
            StdFn::Insert => "Insert",
            StdFn::RemoveAt => "RemoveAt",
            StdFn::Move => "Move",
            StdFn::Slice => "Slice",
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
            StdFn::StartsWith
            | StdFn::SplitOnce
            | StdFn::Min
            | StdFn::Max
            | StdFn::Contains
            | StdFn::UnwrapOr
            | StdFn::ListConcat
            | StdFn::RemoveAt => 2,
            StdFn::Clamp | StdFn::Insert | StdFn::Move | StdFn::Slice => 3,
            StdFn::NilId => 0,
            _ => 1,
        }
    }
}
