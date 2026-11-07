//! §1.16 A domain: its schema and its functions by name.
//!
//! A [`Domain`] is what a crate of plain Rust registers: the schema its
//! `#[derive(Schema)]` struct is, and per name — `playlists.add_to_playlist`
//! — a function and its input's type description. `#[mutator(..)]`,
//! `#[view(..)]` and `#[scope(..)]` only declare a function by name, and
//! [`functions!`](crate::functions) registers every one a crate declares;
//! what it does is the Rust.
//!
//! - a **mutator** is `fn(&Ctx, &mut Db<S, P>, I) -> Result<(), String>`:
//!   the machine of the space it writes (`P`, durable unless it says
//!   [`Volatile`](crate::db::Volatile): a live transition, §1.9) runs it
//!   ([`Run`]), its input read and refined first ([`Input`]);
//! - a **view** is `fn(&Ctx, &Db<S>, I) -> Result<Plan<S, T>, String>`,
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

use crate::db::{write_in, Db, Tables, Writes};
use crate::hash::SchemaHash;
use crate::peer::Ctx;
use crate::plan::{answer, Maintained, Plan, View};
use crate::row::{Describe, Input, Row, Type, Value};
use crate::schema::{Schema, Space};
use crate::store::{Change, Store};

/// What a function is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Mutator,
    View,
    Scope,
}

/// What the machine runs of a mutator, opaque to it: run over the store,
/// its changes left applied, all or none — or its refusal. It reads its
/// own writes as it makes them.
pub type Run = Arc<dyn Fn(&mut Store, &Ctx, &[u8]) -> Result<Vec<Change>, String> + Send + Sync>;
/// What a scope holds (§1.8): whether the union of who reads holds a row
/// of the table named, by the row's bytes.
pub type Holds = Arc<dyn Fn(&Ctx, &str, &[u8]) -> Result<bool, String> + Send + Sync>;
/// A function's answer over a store, encoded.
pub type Ask = Arc<dyn Fn(&Store, &Ctx, &[u8]) -> Result<Vec<u8>, String> + Send + Sync>;
/// A view of a function, kept.
pub type Watch = Arc<dyn Fn(&Store, &Ctx, &[u8]) -> Result<Box<dyn Maintained>, String> + Send + Sync>;
/// An input's refinements over its encoding: every field that fails, and why.
pub type Check = Arc<dyn Fn(&[u8]) -> Result<(), Vec<(String, String)>> + Send + Sync>;

/// One function of a domain, by name — what a generator reads (`arkdb
/// gen`) — and what runs it.
#[derive(Clone)]
pub struct Function {
    pub name: String,
    pub kind: Kind,
    /// The space a mutator writes, and its intents are on.
    pub space: Space,
    /// What it takes, as a form reads it.
    pub input: Type,
    /// What it answers: a view's element.
    pub output: Option<Type>,
    /// What the machine runs: a mutator's.
    pub proc: Option<Run>,
    /// What a scope holds (§1.8).
    pub scope: Option<Holds>,
    pub ask: Option<Ask>,
    pub watch: Option<Watch>,
    pub check: Check,
}

/// A function taking `I` that runs nothing yet: what each kind is made from.
fn function<I: Input>(name: &str, kind: Kind, output: Option<Type>) -> Function {
    Function {
        name: name.into(),
        kind,
        space: Space::Durable,
        input: I::describe(),
        output,
        proc: None,
        scope: None,
        ask: None,
        watch: None,
        check: check_of::<I>(),
    }
}

impl std::fmt::Debug for Function {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?} {}", self.kind, self.name)
    }
}

/// A domain: its name and version, its schema, its functions by name.
#[derive(Clone, Debug, Default)]
pub struct Domain {
    pub name: String,
    pub version: String,
    pub schema: Arc<Schema>,
    pub fns: BTreeMap<String, Function>,
}

fn check_of<I: Input>() -> Check {
    Arc::new(|b| I::decode(b).map_err(|e| vec![(String::new(), e)])?.check().map(|_| ()))
}

/// An input from its encoding (no bytes are `()`'s), refined — as an
/// older or a newer build wrote it, as a row is read (§1.2): a field
/// appended since an empty value, one this build lacks dropped
/// ([`crate::json::fit`]).
fn input_of<I: Input>(b: &[u8]) -> Result<I, String> {
    match I::decode(b) {
        Ok(i) => i.refine(),
        Err(e) => fitted(b, e),
    }
}

/// [`input_of`] of bytes another build wrote: out of the way of the rest.
#[cold]
fn fitted<I: Input>(b: &[u8], e: String) -> Result<I, String> {
    I::decode(&crate::json::fit(&I::describe(), b).ok_or(e)?)?.refine()
}

/// A mutator's function (`P` the space it writes).
pub type WriteFn<S, P, I> = fn(&Ctx, &mut Db<S, P>, I) -> Result<(), String>;

/// A scope's predicate on a row's encoding.
type OnRow = Arc<dyn Fn(&Ctx, &[u8]) -> Result<bool, String> + Send + Sync>;

/// A view's function, as a domain writes it.
pub type ViewFn<S, I, T> = fn(&Ctx, &Db<S>, I) -> Result<Plan<S, T>, String>;

/// Which rows a connection fed by a scope holds (§1.8): per table, a
/// predicate on its rows; a table the scope does not name holds what its
/// row declares ([`Row::fed`]): every row, its owner's, or none.
pub struct Scope<S> {
    tables: BTreeMap<&'static str, OnRow>,
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

    /// A mutator of the space `P` its `Db` writes, which it writes alone
    /// ([`crate::db`]): a durable one an intent on the log, a volatile one a
    /// transition of live state (§1.9).
    pub fn mutator<S: Tables, P: Writes, I: Input>(self, name: &str, f: WriteFn<S, P, I>) -> Domain {
        let run: Run = Arc::new(move |store, ctx, input| {
            let input = input_of::<I>(input)?;
            // Every run draws from the entry's first id: a run again is the
            // same run (§1.6).
            ctx.draws.set(0);
            write_in::<S, P, _>(store, |db| f(ctx, db, input)).map(|(_, cs)| cs)
        });
        let mut f = Function {
            proc: Some(run),
            ..function::<I>(name, Kind::Mutator, None)
        };
        f.space = P::SPACE;
        self.add(f)
    }

    pub fn view<S: Tables, I: Input, T: Value + Describe>(self, name: &str, f: ViewFn<S, I, T>) -> Domain {
        let make = move |b: &[u8]| -> Result<crate::plan::Make<S, T>, String> {
            let input = input_of::<I>(b)?;
            Ok(Arc::new(move |ctx: &Ctx, db: &Db<S>| f(ctx, db, input.clone())))
        };
        let ask: Ask = Arc::new(move |store, ctx, b| Ok(answer(&make(b)?, ctx, store)?.encode()));
        let watch: Watch = Arc::new(move |store, ctx, b| Ok(Box::new(View::new(make(b)?, ctx.clone(), store)?) as Box<dyn Maintained>));
        self.add(Function {
            ask: Some(ask),
            watch: Some(watch),
            ..function::<I>(name, Kind::View, Some(T::describe()))
        })
    }

    pub fn scope<S: Tables>(self, name: &str, scope: Scope<S>) -> Domain {
        let tables = scope.tables;
        let holds: Holds = Arc::new(move |ctx, table, row| tables.get(table).map_or_else(|| S::fed(ctx, table, row), |f| f(ctx, row)));
        self.add(Function {
            scope: Some(holds),
            ..function::<()>(name, Kind::Scope, None)
        })
    }

    /// `register(self)`: a set of functions registered together, a
    /// crate's (`ark_auth::register`) or a job's.
    pub fn with(self, register: fn(Domain) -> Domain) -> Domain {
        register(self)
    }

    pub fn get(&self, name: &str) -> Option<&Function> {
        self.fns.get(name)
    }

    pub fn schema_hash(&self) -> SchemaHash {
        self.schema.hash()
    }

    /// `name` run by `run` over the store as it is, in place of what it
    /// ran, or a mutator of no typed input: what an authority's own raw
    /// write, or a test, registers without a `Db`.
    pub fn run(mut self, name: &str, run: Run) -> Domain {
        let f = self.fns.entry(name.into()).or_insert_with(|| function::<()>(name, Kind::Mutator, None));
        f.proc = Some(run);
        self
    }

    /// A view's answer over a store, as `ctx` reads it.
    pub fn ask(&self, store: &Store, ctx: &Ctx, name: &str, input: &[u8]) -> Result<Vec<u8>, String> {
        let f = self.get(name).ok_or_else(|| format!("no function {name}"))?;
        let ask = f.ask.as_ref().ok_or_else(|| format!("{name} answers nothing"))?;
        ask(store, ctx, input)
    }

    fn func(&self, name: &str) -> Result<&Function, String> {
        self.get(name).ok_or_else(|| format!("no function {name}"))
    }

    /// A function's input from its JSON text (none is `()`'s), in the value
    /// format: what a host that speaks JSON calls it with.
    pub fn input_json(&self, name: &str, text: &str) -> Result<Vec<u8>, String> {
        crate::json::from_text(&self.func(name)?.input, text)
    }

    /// What [`Domain::ask`] answered, as JSON: a view's list.
    pub fn answer_json(&self, name: &str, bytes: &[u8]) -> Result<String, String> {
        let t = self.func(name)?.output.clone().unwrap_or(Type::Unit);
        crate::json::to_text(&Type::List(Box::new(t)), bytes)
    }

    /// One element of a view of a function, as JSON.
    pub fn element_json(&self, name: &str, bytes: &[u8]) -> Result<String, String> {
        crate::json::to_text(self.func(name)?.output.as_ref().unwrap_or(&Type::Unit), bytes)
    }

    /// A row of a table of the schema, as JSON.
    pub fn row_json(&self, table: &str, bytes: &[u8]) -> Result<String, String> {
        let (_, t) = self.schema.table(table).ok_or_else(|| format!("no table {table}"))?;
        crate::json::to_text(&t.row_type(), bytes)
    }

    /// A view of a function, kept over `store`.
    pub fn watch(&self, store: &Store, ctx: &Ctx, name: &str, input: &[u8]) -> Result<Box<dyn Maintained>, String> {
        let f = self.get(name).ok_or_else(|| format!("no function {name}"))?;
        let w = f.watch.as_ref().ok_or_else(|| format!("{name} is not a view"))?;
        w(store, ctx, input)
    }
}
