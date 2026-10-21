//! Every query and mutation of a domain, callable (`docs/plan-guards.md`
//! D4): which there are, grouped by router; a form per function, a line
//! per field of its input's type description, typed as its type's values
//! are written; the field's checks, said beside it and run before calling
//! with the domain's own messages (§1.16: the input's generated check is
//! the form's validation). No window: the view draws what is here.
//!
//! A mutation called from here is authored through the host's
//! [`Writer`](crate::data::Writer) exactly as a button in the app authors
//! it — as the signed-in person on a client, never raw — and a query runs
//! on the host's store. Helpers, guards, provides, scopes and §1.9's live
//! transitions are not called by anybody, so they are not listed.

use ark::domain::Kind as Fn;
use ark::row::Type;

use crate::data::{Catalog, Source};
use crate::value::{self, decode, encode, Ty, Value};

/// What a callable is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Query,
    Mutation,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Mutation => "mutation",
            Kind::Query => "query",
        }
    }
}

/// One field of a function's input: its type, and its checks as they are
/// said beside it — the module's refinements, with their messages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub ty: Ty,
    pub checks: Vec<String>,
}

/// One function a person can call: its router, its binding, its kind, and
/// its input's fields — `record` when the input is a record of them, else
/// it is the one field itself (or nothing) — and what a query's answer is
/// a list of, its fields named by a table's columns where they are one's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Callable {
    pub router: String,
    pub name: String,
    pub kind: Kind,
    pub input: Vec<(String, Field)>,
    pub record: bool,
    pub returns: Ty,
    pub columns: Option<Vec<String>>,
}

impl Callable {
    /// The input as its bytes, from the fields' values.
    pub fn input_bytes(&self, v: &Value) -> Result<Vec<u8>, String> {
        let fs: Vec<(String, Ty)> = self.input.iter().map(|(n, f)| (n.clone(), f.ty.clone())).collect();
        match (self.record, v) {
            (true, v) => encode(&Ty::Struct(fs), v),
            (false, Value::Struct(vs)) if vs.is_empty() => Ok(vec![]),
            (false, Value::Struct(vs)) => encode(&fs[0].1, &vs[0]),
            (false, other) => Err(format!("not an input: {}", value::text_of(other))),
        }
    }
}

/// Every query and mutation of `catalog` — a mutator, or a view or a
/// query answering a list — grouped by router in the order each router
/// first appears, and in name order within one. Live transitions, scopes
/// and queries of one value are nobody's to call from here.
pub fn callables(catalog: &Catalog) -> Vec<Callable> {
    let mut out: Vec<Callable> = Vec::new();
    for f in &catalog.functions {
        let kind = match (f.kind, &f.output) {
            (Fn::Mutator, _) => Kind::Mutation,
            (Fn::View, _) | (Fn::Query, Some(Type::List(_))) => Kind::Query,
            _ => continue,
        };
        let (fields, record): (Vec<(String, Type)>, bool) = match &f.input {
            Type::Record(_, fs) => (fs.clone(), true),
            Type::Unit => (vec![], false),
            t => (vec![("input".into(), t.clone())], false),
        };
        let checks = |n: &str| f.checks.iter().find(|(c, _)| c == n).map(|(_, cs)| cs.clone()).unwrap_or_default();
        let input = fields
            .iter()
            .map(|(n, t)| {
                let field = Field {
                    ty: value::of_type(t),
                    checks: checks(n),
                };
                (n.clone(), field)
            })
            .collect();
        let element = match (f.kind, &f.output) {
            (Fn::Query, Some(Type::List(t))) => (**t).clone(),
            (_, Some(t)) => t.clone(),
            (_, None) => Type::Unit,
        };
        let columns = match &element {
            Type::Record(_, fs) => value::columns_of(&catalog.schema, fs),
            _ => None,
        };
        let mut returns = value::of_type(&element);
        if let (Ty::Struct(fs), Some(cs)) = (&mut returns, &columns) {
            fs.iter_mut().zip(cs).for_each(|((n, _), c)| n.clone_from(c));
        }
        let router = f.name.rsplit_once('.').map_or("", |(r, _)| r).to_string();
        out.push(Callable {
            router,
            name: f.name.clone(),
            kind,
            input,
            record,
            returns,
            columns,
        });
    }
    let mut routers: Vec<String> = vec![];
    for c in &out {
        if !routers.contains(&c.router) {
            routers.push(c.router.clone());
        }
    }
    routers.iter().flat_map(|r| out.iter().filter(move |c| &c.router == r).cloned()).collect()
}

/// A form over one function's input: one line per field, typed as
/// [`value::text_of`] writes a value of its type (a list `[a, b]`, a record
/// `(a, b)`, an enum `#1`, an id pasted or chosen, nothing for none); the
/// line the cursor is on (one past the last is the button that calls); and
/// what the checks said last, per field (`""` for the input as a whole).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Form {
    pub function: String,
    pub kind: Kind,
    pub fields: Vec<(String, Field)>,
    pub values: Vec<String>,
    pub at: usize,
    pub messages: Vec<(String, String)>,
}

impl Form {
    pub fn new(c: &Callable) -> Form {
        let values = vec![String::new(); c.input.len()];
        Form {
            function: c.name.clone(),
            kind: c.kind,
            fields: c.input.clone(),
            values,
            at: 0,
            messages: vec![],
        }
    }

    /// What the form reads as: the record of its fields where every one
    /// reads as its type, and a message for every one that does not.
    pub fn read(&self) -> (Option<Value>, Vec<(String, String)>) {
        let (mut vs, mut bad) = (Vec::new(), Vec::new());
        for ((n, f), v) in self.fields.iter().zip(&self.values) {
            match value::parse_cell(n, &f.ty, v) {
                Ok(v) => vs.push(v),
                Err(why) => bad.push((n.clone(), why)),
            }
        }
        (bad.is_empty().then_some(Value::Struct(vs)), bad)
    }

    /// Run every check before calling: each field read as its type, then the
    /// input's bytes held to its refinements by the domain's own check
    /// (§1.16), where the host holds the domain, each failure said with the
    /// domain's words against the field it is in. Nothing is written. The
    /// input, when there is nothing to say against it.
    pub fn validate(&mut self, c: &Callable, src: &Source) -> Option<Value> {
        let (read, mut messages) = self.read();
        let field = |n: String| match c.record {
            true => n,
            false => self.fields.first().map_or(String::new(), |(n, _)| n.clone()),
        };
        if let Some(v) = &read {
            match (c.input_bytes(v), src.ask) {
                (Err(why), _) => messages.push((String::new(), why)),
                (Ok(bytes), Some(ask)) => {
                    if let Err(fails) = ask.check(&c.name, &bytes) {
                        messages.extend(fails.into_iter().map(|(n, why)| (field(n), why)));
                    }
                }
                (Ok(_), None) => {}
            }
        }
        messages.sort_by_key(|(n, _)| self.fields.iter().position(|(f, _)| f == n).unwrap_or(usize::MAX));
        self.messages = messages;
        read.filter(|_| self.messages.is_empty())
    }

    /// What is wrong with field `name`, if anything.
    pub fn message(&self, name: &str) -> Option<&str> {
        self.messages.iter().find(|(n, _)| n == name).map(|(_, m)| m.as_str())
    }
}

/// A query run on the host's store as who the explorer reads as, its input
/// the record of its fields: its answer, each element read under the
/// query's element type.
pub fn query(src: &Source, c: &Callable, input: &Value) -> Result<Value, String> {
    let ask = src.ask.ok_or("this host does not hold the domain: queries are answered where it is")?;
    let bytes = ask
        .ask(src.store, &src.ctx, &c.name, &c.input_bytes(input)?)
        .map_err(|r| format!("refused: {r}"))?;
    decode(&Ty::List(Box::new(c.returns.clone())), &bytes).map_err(|e| format!("a bug: {e}"))
}

/// The picker, open on one id field of the form (by its place): which
/// table, what is typed to filter its rows, and the cursor among what the
/// filter leaves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picker {
    pub field: usize,
    pub table: String,
    pub filter: String,
    pub at: usize,
}
