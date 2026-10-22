//! §1.16 A domain: its schema and its functions by name.
//!
//! A [`Domain`] is what a crate of plain Rust registers: the schema its
//! `#[derive(Schema)]` struct is, and per name — `playlists.add_to_playlist`
//! — a function and its input's type description. `#[mutator(..)]`,
//! `#[live(..)]`, `#[query(..)]`, `#[view(..)]` and `#[scope(..)]` only
//! register a function by name ([`register!`]); what it does is the Rust.
//!
//! - a **mutator** (and a **live transition**, over volatile tables) is
//!   `fn(&Ctx, &mut Db<S>, I) -> Result<(), Refusal>`: the machine runs it
//!   as a [`Procedure`], its input read and refined first ([`Input`]);
//! - a **query** is `fn(&Ctx, &Db<S>, I) -> Result<T, Refusal>`, answered
//!   over a store;
//! - a **view** is `fn(&Ctx, &Db<S>, I) -> Result<Plan<S, T>, Refusal>`,
//!   kept up to date ([`crate::plan::View`]) or answered afresh;
//! - a **scope** is a [`Scope`]: per table, `fn(&Ctx, &R) -> bool`, which
//!   rows a connection fed by it holds (§1.8).
//!
//! What a function must not do — act on the world outside — is a job
//! ([`effect`]): a row a handler claims and finishes.
//!
//! Determinism is an API rule: `ctx.now` and `ctx.new_id()` are a
//! function's only non-determinism; a function reads nothing else that
//! differs between runs (no clock, no randomness, no host state).

pub mod effect;

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::db::{read, write_in, Db, Tables, Volatile, Writes};
use crate::hash::SchemaHash;
use crate::peer::{self, Ctx, Functions, Procedure, Runs};
use crate::plan::{answer, Maintained, Plan, View};
use crate::row::{Describe, Input, Row, Type, Value};
use crate::schema::Schema;
use crate::store::{Change, Store};
use crate::Refusal;

/// What a function is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Kind {
    Mutator,
    Live,
    Query,
    View,
    Scope,
}

/// A function's answer over a store, encoded.
pub type Ask = Arc<dyn Fn(&Store, &Ctx, &[u8]) -> Result<Vec<u8>, Refusal> + Send + Sync>;
/// A view of a function, kept.
pub type Watch = Arc<dyn Fn(&Store, &Ctx, &[u8]) -> Result<Box<dyn Maintained>, Refusal> + Send + Sync>;
/// An input's refinements over its encoding: every field that fails, and why.
pub type Check = Arc<dyn Fn(&[u8]) -> Result<(), Vec<(String, String)>> + Send + Sync>;

/// One function of a domain, by name.
#[derive(Clone)]
pub struct Function {
    pub name: String,
    pub kind: Kind,
    /// What it takes, as a form reads it.
    pub input: Type,
    /// What it answers, a query's or a view's element.
    pub output: Option<Type>,
    /// What the machine runs: a mutator's, a live transition's.
    pub proc: Option<Arc<dyn Procedure>>,
    /// What a scope holds (§1.8).
    pub scope: Option<peer::Scope>,
    pub ask: Option<Ask>,
    pub watch: Option<Watch>,
    pub check: Check,
    /// Each refined field of the input, and its refinements as a form says
    /// them.
    pub checks: Vec<(String, Vec<String>)>,
}

impl std::fmt::Debug for Function {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?} {}", self.kind, self.name)
    }
}

/// A domain: its name and version, its schema, its functions by name.
#[derive(Clone, Debug)]
pub struct Domain {
    pub name: String,
    pub version: String,
    pub schema: Arc<Schema>,
    pub fns: BTreeMap<String, Function>,
}

fn check_of<I: Input>() -> Check {
    Arc::new(|b| I::decode(b).map_err(|e| vec![(String::new(), e.0)])?.check().map(|_| ()))
}

/// An input from its encoding (no bytes are `()`'s), refined.
fn input_of<I: Input>(b: &[u8]) -> Result<I, Refusal> {
    match b.is_empty() {
        true => I::decode(&crate::format::encode(&())?)?.refine(),
        false => I::decode(b)?.refine(),
    }
}

/// A mutator's function, or a live transition's (`P` the space it writes).
pub type WriteFn<S, P, I> = fn(&Ctx, &mut Db<S, P>, I) -> Result<(), Refusal>;

struct Mutate<S: Tables, P: Writes, I> {
    f: WriteFn<S, P, I>,
}

impl<S: Tables, P: Writes, I: Input> Procedure for Mutate<S, P, I> {
    fn apply(&self, store: &mut Store, ctx: &Ctx, input: &[u8]) -> Result<Vec<Change>, Refusal> {
        let input = input_of::<I>(input)?;
        // Every run draws from the entry's first id: a run again is the
        // same run (§1.6).
        ctx.draws.set(0);
        write_in::<S, P, _>(store, |db| (self.f)(ctx, db, input)).map(|(_, cs)| cs)
    }
}

/// A scope's predicate on a row's encoding.
type Holds = Arc<dyn Fn(&Ctx, &[u8]) -> Result<bool, Refusal> + Send + Sync>;

/// A view's function, as a domain writes it.
pub type ViewFn<S, I, T> = fn(&Ctx, &Db<S>, I) -> Result<Plan<S, T>, Refusal>;

/// Which rows a connection fed by a scope holds (§1.8): per table, a
/// predicate on its rows; a table the scope does not name holds none.
pub struct Scope<S> {
    tables: BTreeMap<&'static str, Holds>,
    _p: std::marker::PhantomData<fn() -> S>,
}

impl<S: Tables> Default for Scope<S> {
    fn default() -> Self {
        Scope {
            tables: BTreeMap::new(),
            _p: std::marker::PhantomData,
        }
    }
}

impl<S: Tables> Scope<S> {
    pub fn new() -> Scope<S> {
        Scope::default()
    }

    /// The rows of `R`'s table `f` holds.
    pub fn on<R: Row>(mut self, f: fn(&Ctx, &R) -> bool) -> Scope<S> {
        self.tables.insert(R::TABLE, Arc::new(move |ctx, b| Ok(f(ctx, &R::decode(b)?))));
        self
    }
}

impl Domain {
    /// A domain of `S`'s schema, with no functions yet.
    pub fn new<S: Tables>(name: &str, version: &str) -> Domain {
        Domain {
            name: name.into(),
            version: version.into(),
            schema: Arc::new(S::schema()),
            fns: BTreeMap::new(),
        }
    }

    fn add(mut self, f: Function) -> Domain {
        assert!(!self.fns.contains_key(&f.name), "{} is registered twice", f.name);
        self.fns.insert(f.name.clone(), f);
        self
    }

    fn writer<S: Tables, P: Writes, I: Input>(self, name: &str, kind: Kind, f: WriteFn<S, P, I>) -> Domain {
        self.add(Function {
            name: name.into(),
            kind,
            input: I::describe(),
            output: None,
            proc: Some(Arc::new(Mutate { f })),
            scope: None,
            ask: None,
            watch: None,
            check: check_of::<I>(),
            checks: I::checks(),
        })
    }

    pub fn mutator<S: Tables, I: Input>(self, name: &str, f: fn(&Ctx, &mut Db<S>, I) -> Result<(), Refusal>) -> Domain {
        self.writer(name, Kind::Mutator, f)
    }

    /// A transition of live state: a mutator over volatile tables (§1.9),
    /// which writes none other ([`crate::db`]).
    pub fn live<S: Tables, I: Input>(self, name: &str, f: WriteFn<S, Volatile, I>) -> Domain {
        self.writer(name, Kind::Live, f)
    }

    pub fn query<S: Tables, I: Input, T: Value + Describe>(self, name: &str, f: fn(&Ctx, &Db<S>, I) -> Result<T, Refusal>) -> Domain {
        let ask: Ask = Arc::new(move |store, ctx, b| {
            let input = input_of::<I>(b)?;
            read::<S, _>(store, |db| f(ctx, db, input)).map(|t| t.encode())
        });
        self.add(Function {
            name: name.into(),
            kind: Kind::Query,
            input: I::describe(),
            output: Some(T::describe()),
            proc: None,
            scope: None,
            ask: Some(ask),
            watch: None,
            check: check_of::<I>(),
            checks: I::checks(),
        })
    }

    pub fn view<S: Tables, I: Input, T: Value + Describe>(self, name: &str, f: ViewFn<S, I, T>) -> Domain {
        let make = move |b: &[u8]| -> Result<crate::plan::Make<S, T>, Refusal> {
            let input = input_of::<I>(b)?;
            Ok(Arc::new(move |ctx: &Ctx, db: &Db<S>| f(ctx, db, input.clone())))
        };
        let ask: Ask = Arc::new(move |store, ctx, b| Ok(answer(&make(b)?, ctx, store)?.encode()));
        let watch: Watch = Arc::new(move |store, ctx, b| Ok(Box::new(View::new(make(b)?, ctx.clone(), store)?) as Box<dyn Maintained>));
        self.add(Function {
            name: name.into(),
            kind: Kind::View,
            input: I::describe(),
            output: Some(T::describe()),
            proc: None,
            scope: None,
            ask: Some(ask),
            watch: Some(watch),
            check: check_of::<I>(),
            checks: I::checks(),
        })
    }

    pub fn scope<S: Tables>(self, name: &str, scope: Scope<S>) -> Domain {
        let tables = scope.tables;
        let holds: peer::Scope = Arc::new(move |ctx, table, row| tables.get(table).map_or(Ok(false), |f| f(ctx, row)));
        self.add(Function {
            name: name.into(),
            kind: Kind::Scope,
            input: Type::Unit,
            output: None,
            proc: None,
            scope: Some(holds),
            ask: None,
            watch: None,
            check: check_of::<()>(),
            checks: vec![],
        })
    }

    /// `register(self)`: what `#[mutator(..)]` and the rest generate, and
    /// [`register!`] calls.
    pub fn with(self, register: fn(Domain) -> Domain) -> Domain {
        register(self)
    }

    pub fn get(&self, name: &str) -> Option<&Function> {
        self.fns.get(name)
    }

    /// The function a name means: itself, or the first whose last part it
    /// is.
    pub fn binding(&self, name: &str) -> Option<&str> {
        self.fns
            .keys()
            .map(String::as_str)
            .find(|n| *n == name || n.rsplit('.').next() == Some(name))
    }

    pub fn schema_hash(&self) -> SchemaHash {
        self.schema.hash()
    }

    /// The functions the machine runs, by name.
    pub fn functions(&self) -> Functions {
        let mut c = Functions::default();
        for f in self.fns.values() {
            let run = f.proc.clone().map(Runs::Procedure).or(f.scope.clone().map(Runs::Scope));
            run.into_iter().for_each(|r| c.put(f.name.clone(), r));
        }
        c
    }

    /// A query's or a view's answer over a store, as `ctx` reads it.
    pub fn ask(&self, store: &Store, ctx: &Ctx, name: &str, input: &[u8]) -> Result<Vec<u8>, Refusal> {
        let f = self.get(name).ok_or_else(|| Refusal(format!("no function {name}")))?;
        let ask = f.ask.as_ref().ok_or_else(|| Refusal(format!("{name} answers nothing")))?;
        ask(store, ctx, input)
    }

    fn func(&self, name: &str) -> Result<&Function, Refusal> {
        self.get(name).ok_or_else(|| Refusal(format!("no function {name}")))
    }

    /// A function's input from its JSON text (none is `()`'s), in the value
    /// format: what a host that speaks JSON calls it with.
    pub fn input_json(&self, name: &str, text: &str) -> Result<Vec<u8>, Refusal> {
        crate::json::from_text(&self.func(name)?.input, text)
    }

    /// What [`Domain::ask`] answered, as JSON: a query's value, a view's
    /// list.
    pub fn answer_json(&self, name: &str, bytes: &[u8]) -> Result<String, Refusal> {
        let f = self.func(name)?;
        let t = f.output.clone().unwrap_or(Type::Unit);
        let t = if f.kind == Kind::View { Type::List(Box::new(t)) } else { t };
        crate::json::to_text(&t, bytes)
    }

    /// One element of a view of a function, as JSON.
    pub fn element_json(&self, name: &str, bytes: &[u8]) -> Result<String, Refusal> {
        crate::json::to_text(self.func(name)?.output.as_ref().unwrap_or(&Type::Unit), bytes)
    }

    /// A row of a table of the schema, as JSON.
    pub fn row_json(&self, table: &str, bytes: &[u8]) -> Result<String, Refusal> {
        let (_, t) = self.schema.table(table).ok_or_else(|| Refusal(format!("no table {table}")))?;
        crate::json::to_text(&t.row_type(), bytes)
    }

    /// A view of a function, kept over `store`.
    pub fn watch(&self, store: &Store, ctx: &Ctx, name: &str, input: &[u8]) -> Result<Box<dyn Maintained>, Refusal> {
        let f = self.get(name).ok_or_else(|| Refusal(format!("no function {name}")))?;
        let w = f.watch.as_ref().ok_or_else(|| Refusal(format!("{name} is not a view")))?;
        w(store, ctx, input)
    }
}

/// Registers functions by their paths: `register!(domain; demo::create,
/// demo::rename)` is `domain.with(demo::create::register)…`.
#[macro_export]
macro_rules! register {
    ($d:expr; $($($p:ident)::+),* $(,)?) => {
        $d $(.with($($p)::+::register))*
    };
}
