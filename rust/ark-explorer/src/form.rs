//! Every query and mutation of a module, callable (`docs/plan-guards.md`
//! D4): which there are, grouped by router; a form per function, a line
//! per field of its input type, typed as its type's values are written; the field's checks,
//! said beside it and run before calling with the module's own messages
//! (§1.1: the form validator is `parse`). No window: the view draws what is
//! here.
//!
//! A mutation called from here is authored through the host's
//! [`Writer`](crate::data::Writer) exactly as a button in the app authors
//! it — as the signed-in person on a client, never raw — and a query runs
//! on the host's store. Helpers, guards, provides, scopes and §1.9's live
//! transitions are not called by anybody, so they are not listed.

use ark::check::Types;
use ark::interp::{check_form, Interp};
use ark::ir::Effect;
use ark::module::{Lib, Module};
use ark::ty::Ty as Structure;

use crate::data::Source;
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

/// Every query and mutation of `module` — a function that writes, or reads
/// and answers a list — grouped by router in the order each router first
/// appears, and in binding order within one.
pub fn callables(module: &Module, lib: &Lib) -> Vec<Callable> {
    let mut out: Vec<Callable> = Vec::new();
    for (name, h) in &module.bindings {
        let Some(f) = lib.get(h) else { continue };
        let kind = match (f.f.effect, &f.f.ret) {
            (Effect::Write, _) => Kind::Mutation,
            (Effect::Read, Structure::List(_)) => Kind::Query,
            _ => continue,
        };
        if crate::data::is_live(module, lib, h) {
            continue;
        }
        let cx = Types::new(&f.f.types);
        let (fields, record) = value::fields_of(&cx, &f.f.params);
        let labels = value::columns_of(&module.schema, &fields).unwrap_or_else(|| value::labelled(&fields));
        let field = |t: &Structure| Field {
            ty: value::resolve(&cx, t),
            checks: value::checks_of(&cx, t),
        };
        let input = labels.into_iter().zip(&fields).map(|(l, t)| (l, field(t))).collect();
        let element = match &f.f.ret {
            Structure::List(t) => (**t).clone(),
            t => t.clone(),
        };
        let columns = value::row_labels(&module.schema, &element, &cx);
        let mut returns = value::resolve(&cx, &element);
        if let (Ty::Struct(fs), Some(cs)) = (&mut returns, &columns) {
            fs.iter_mut().zip(cs).for_each(|((n, _), c)| n.clone_from(c));
        }
        let router = name.rsplit_once('.').map_or("", |(r, _)| r).to_string();
        out.push(Callable {
            router,
            name: name.clone(),
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
    /// input's bytes held to its refinements by the module's own
    /// `check_form` (§1.1: the form validator is `parse`), each failure said
    /// with the module's message against the field it is in. Nothing is
    /// written. The input, when there is nothing to say against it.
    pub fn validate(&mut self, c: &Callable, lib: &Lib, module: &Module) -> Option<Value> {
        let (read, mut messages) = self.read();
        let field = |i: Option<u32>| match (c.record, i) {
            (true, Some(i)) => self.fields.get(i as usize).map_or(String::new(), |(n, _)| n.clone()),
            (false, _) => self.fields.first().map_or(String::new(), |(n, _)| n.clone()),
            (true, None) => String::new(),
        };
        if let Some(v) = &read {
            match (c.input_bytes(v), module.get(&c.name).and_then(|h| lib.get(&h))) {
                (Err(why), _) => messages.push((String::new(), why)),
                (_, None) => messages.push((String::new(), format!("the module has no function {}", c.name))),
                (Ok(bytes), Some(f)) => {
                    if let Err(fails) = check_form(&f.cx, &Interp::input_ty(&f.f), &bytes) {
                        messages.extend(fails.into_iter().map(|(path, why)| (field(path.first().copied()), why)));
                    }
                }
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
    let h = src.module.get(&c.name).ok_or_else(|| format!("no function {}", c.name))?;
    let rows = ark::view::query(src.lib, &h, &c.input_bytes(input)?, &src.ctx, src.store).map_err(|r| format!("refused: {}", r.0))?;
    let read = rows.iter().map(|v| decode(&c.returns, &v.to_bytes()).map_err(|e| format!("a bug: {e}")));
    read.collect::<Result<Vec<_>, _>>().map(Value::List)
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
