//! Every query and mutation of a module, callable (`docs/plan-guards.md`
//! D4): which there are, grouped by router; a form per function with one
//! input per field by its type; the field's checks, said beside it and run
//! before calling with the module's own messages; and the picker an
//! `Id(table)` field is chosen with. No window: the view draws what is here.
//!
//! A mutation called from here is authored through the host's [`Writer`]
//! exactly as a button in the app authors it — as the signed-in person on a
//! client, never raw — and a query runs on the host's store, as the console's
//! does. Helpers, guards, provides and scopes are not called by anybody, so
//! they are not listed.
//!
//! [`Writer`]: crate::data::Writer

use ark::check::Types;
use ark::interp::{check_form, Interp};
use ark::ir::Effect;
use ark::module::{Lib, Module};
use ark::store::Store;
use ark::ty::Ty as Structure;

use crate::data::{Source, Ticket};
use crate::value::{self, decode, encode, Ty, Value};

/// How many hex digits of an id a cell or a picker draws.
pub const ID_DIGITS: usize = 8;

/// What a callable is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Query,
    Mutation,
}

impl Kind {
    /// As the list says it.
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
/// it is the one field itself (or nothing).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Callable {
    pub router: String,
    pub name: String,
    pub kind: Kind,
    pub input: Vec<(String, Field)>,
    pub record: bool,
    /// What a query's answer is a list of, and the names its fields go by
    /// when they are a table's columns.
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
/// and answers a list, of the log's tables (§1.9's live transitions are the
/// volatile machine's, not a form's) — grouped by router in the order each
/// router first appears, and in binding order within one.
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
        let fcx = Types::new(&f.f.types);
        let (fields, record) = value::fields_of(&fcx, &f.f.params);
        let labels = value::columns_of(&module.schema, &fields).unwrap_or_else(|| value::labelled(&fields));
        let input = labels
            .into_iter()
            .zip(&fields)
            .map(|(l, t)| {
                let field = Field {
                    ty: value::resolve(&fcx, t),
                    checks: value::checks_of(&fcx, t),
                };
                (l, field)
            })
            .collect();
        let element = match &f.f.ret {
            Structure::List(t) => (**t).clone(),
            t => t.clone(),
        };
        let columns = value::row_labels(&module.schema, &element, &fcx);
        let mut returns = value::resolve(&fcx, &element);
        if let (Ty::Struct(fs), Some(cs)) = (&mut returns, &columns) {
            for ((n, _), c) in fs.iter_mut().zip(cs) {
                n.clone_from(c);
            }
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

/// A field's checks, as they are said beside it.
pub fn checks_said(f: &Field) -> Vec<String> {
    f.checks.clone()
}

/// A kind as the list says it.
pub fn kind_name(k: Kind) -> &'static str {
    k.name()
}

pub use crate::value::ty_name;

/// What one input of a form holds, shaped as its type is: the text of a
/// text, an int, an id or bytes (read when called, so a half-typed number is
/// kept as typed); a bool; an enum's variant by its place; an option's
/// "is there one" beside what it would be; a list's items; a record's
/// fields, in its type's order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    Text(String),
    Bool(bool),
    Choice(usize),
    Opt(bool, Box<Input>),
    List(Vec<Input>),
    Fields(Vec<Input>),
}

impl Input {
    /// An empty input of `ty`: no text, false, the first variant, none, no
    /// items.
    pub fn blank(ty: &Ty) -> Input {
        match ty {
            Ty::Bool => Input::Bool(false),
            Ty::Enum(_) => Input::Choice(0),
            Ty::Option(t) => Input::Opt(false, Box::new(Input::blank(t))),
            Ty::List(_) => Input::List(vec![]),
            Ty::Struct(fs) => Input::Fields(fs.iter().map(|(_, t)| Input::blank(t)).collect()),
            // A sum with fields is no form's: it is offered as text, and
            // refused when it is read.
            Ty::Int | Ty::Text | Ty::Bytes | Ty::Id(_) | Ty::Sum(_) => Input::Text(String::new()),
        }
    }

    /// A value of `ty` as an input: how a recent call's arguments are put
    /// back in its form.
    pub fn of(ty: &Ty, v: &Value) -> Input {
        match (ty, v) {
            (Ty::Option(t), Value::Null) => Input::Opt(false, Box::new(Input::blank(t))),
            (Ty::Option(t), v) => Input::Opt(true, Box::new(Input::of(t, v))),
            (Ty::Bool, Value::Bool(b)) => Input::Bool(*b),
            (Ty::Enum(_), Value::Variant(i, _)) => Input::Choice(*i as usize),
            (Ty::List(t), Value::List(xs)) => Input::List(xs.iter().map(|x| Input::of(t, x)).collect()),
            (Ty::Struct(fs), Value::Struct(xs)) => Input::Fields(fs.iter().zip(xs).map(|((_, t), x)| Input::of(t, x)).collect()),
            (Ty::Int | Ty::Text | Ty::Bytes | Ty::Id(_), v) => Input::Text(crate::edit::cell_text(v)),
            (t, _) => Input::blank(t),
        }
    }

    /// The input `path` names below this one of type `ty`, and its type: a
    /// list's item by its place, a record's field by its place, an option's
    /// inner by `0`.
    pub fn at_mut<'a>(&'a mut self, ty: &'a Ty, path: &[usize]) -> Option<(&'a Ty, &'a mut Input)> {
        let Some((&i, rest)) = path.split_first() else {
            return Some((ty, self));
        };
        match ty {
            Ty::Option(t) if i == 0 => match self {
                Input::Opt(_, inner) => inner.at_mut(t, rest),
                _ => None,
            },
            Ty::List(t) => match self {
                Input::List(xs) => xs.get_mut(i)?.at_mut(t, rest),
                _ => None,
            },
            Ty::Struct(fs) => match self {
                Input::Fields(xs) => xs.get_mut(i)?.at_mut(&fs.get(i)?.1, rest),
                _ => None,
            },
            _ => None,
        }
    }

    /// What this input reads as, a value of `ty`, or why not — `name` is
    /// where it is, for the message: `count: a whole number`.
    pub fn value(&self, ty: &Ty, name: &str) -> Result<Value, String> {
        let bad = |what: &str| Err(format!("{name}: {what}"));
        match (ty, self) {
            (Ty::Int | Ty::Text | Ty::Bytes | Ty::Id(_), Input::Text(t)) => crate::edit::parse_cell(name, ty, t),
            (Ty::Bool, Input::Bool(b)) => Ok(Value::Bool(*b)),
            (Ty::Enum(vs), Input::Choice(i)) if *i < vs.len() => Ok(Value::Variant(*i as u8, vec![])),
            (Ty::Option(_), Input::Opt(false, _)) => Ok(Value::Null),
            (Ty::Option(t), Input::Opt(true, inner)) => inner.value(t, name),
            (Ty::List(t), Input::List(xs)) => xs
                .iter()
                .enumerate()
                .map(|(k, x)| x.value(t, &format!("{name}.{}", k + 1)))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::List),
            (Ty::Struct(fs), Input::Fields(xs)) => fs
                .iter()
                .zip(xs)
                .map(|((n, t), x)| x.value(t, &format!("{name}.{n}")))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Struct),
            (t, _) => bad(&format!("not an input of {}", ty_name(t))),
        }
    }
}

/// An id as text — 32 hex digits, dashes anywhere, as a cell shows one and a
/// person pastes one.
pub fn parse_id(text: &str) -> Option<[u8; 16]> {
    ark::hash::unhex(&text.trim().replace('-', "")).and_then(|b| <[u8; 16]>::try_from(b).ok())
}

/// An id as a cell shows it whole: `8-4-4-4-12`.
pub fn id_text(id: &[u8; 16]) -> String {
    crate::edit::cell_text(&Value::Id(*id))
}

/// One line of a form, as the keyboard walks it and the view draws it: where
/// its input is, how deep, what it is called, and its type. An option, a
/// list and a record are a line each with what is inside them below; an
/// option of a single value draws that value on its own line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    pub path: Vec<usize>,
    pub depth: usize,
    pub label: String,
    pub ty: Ty,
    /// An item of a list, which can be taken out of it.
    pub item: bool,
}

/// A form over one function's input: what is typed into each field, which
/// line the cursor is on, and what the checks said of it last.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Form {
    pub function: String,
    pub kind: Kind,
    pub fields: Vec<(String, Field)>,
    pub values: Vec<Input>,
    /// The line the cursor is on; one past the last is the run button.
    pub at: usize,
    /// Per field (or `""` for the input as a whole), what is wrong with it:
    /// its type's reading or its refinements', with the module's own
    /// messages.
    pub messages: Vec<(String, String)>,
}

impl Form {
    pub fn new(c: &Callable) -> Form {
        Form {
            function: c.name.clone(),
            kind: c.kind,
            fields: c.input.clone(),
            values: c.input.iter().map(|(_, f)| Input::blank(&f.ty)).collect(),
            at: 0,
            messages: vec![],
        }
    }

    /// The form filled with a call's arguments, a record of its fields.
    pub fn filled(c: &Callable, args: &Value) -> Form {
        let mut f = Form::new(c);
        if let Value::Struct(vs) = args {
            f.values = c.input.iter().zip(vs).map(|((_, fd), v)| Input::of(&fd.ty, v)).collect();
        }
        f
    }

    /// Every line, in order.
    pub fn slots(&self) -> Vec<Slot> {
        let mut out = Vec::new();
        for (i, ((n, f), v)) in self.fields.iter().zip(&self.values).enumerate() {
            slots_of(&f.ty, v, vec![i], 0, n.clone(), false, &mut out);
        }
        out
    }

    /// The input at `path`, and its type.
    pub fn at_mut(&mut self, path: &[usize]) -> Option<(&Ty, &mut Input)> {
        let (&i, rest) = path.split_first()?;
        let ty = &self.fields.get(i)?.1.ty;
        self.values.get_mut(i)?.at_mut(ty, rest)
    }

    /// Type into a text, an int, an id or bytes.
    pub fn set_text(&mut self, path: &[usize], text: String) {
        if let Some((_, Input::Text(t))) = self.at_mut(path) {
            *t = text;
        }
    }

    /// Flip a bool, or an option between none and a value.
    pub fn toggle(&mut self, path: &[usize]) {
        match self.at_mut(path) {
            Some((_, Input::Bool(b))) => *b = !*b,
            Some((_, Input::Opt(some, _))) => *some = !*some,
            _ => {}
        }
    }

    /// Choose an enum's variant.
    pub fn choose(&mut self, path: &[usize], i: usize) {
        if let Some((Ty::Enum(vs), Input::Choice(c))) = self.at_mut(path) {
            if i < vs.len() {
                *c = i;
            }
        }
    }

    /// Add an empty item to a list.
    pub fn add(&mut self, path: &[usize]) {
        if let Some((Ty::List(t), Input::List(xs))) = self.at_mut(path) {
            xs.push(Input::blank(t));
        }
    }

    /// Take the item at `path` out of its list.
    pub fn remove(&mut self, path: &[usize]) {
        let Some((last, parent)) = path.split_last() else { return };
        if let Some((_, Input::List(xs))) = self.at_mut(parent) {
            if *last < xs.len() {
                xs.remove(*last);
            }
        }
        let n = self.slots().len();
        self.at = self.at.min(n);
    }

    /// `<Enter>` on a line: a bool flips, an enum takes its next variant, an
    /// option flips between none and a value, a list gains an item. What a
    /// text or an id line does is the caller's (an id opens the picker).
    pub fn activate(&mut self, path: &[usize]) {
        match self.at_mut(path) {
            Some((_, Input::Bool(b))) => *b = !*b,
            Some((Ty::Enum(vs), Input::Choice(c))) => *c = (*c + 1) % vs.len().max(1),
            Some((_, Input::Opt(some, _))) => *some = !*some,
            Some((Ty::List(t), Input::List(xs))) => xs.push(Input::blank(t)),
            _ => {}
        }
    }

    /// What the form reads as: the record of its fields where every one
    /// reads as its type, and a message for every one that does not.
    pub fn read(&self) -> (Option<Value>, Vec<(String, String)>) {
        let mut vs = Vec::new();
        let mut bad = Vec::new();
        for ((n, f), v) in self.fields.iter().zip(&self.values) {
            match v.value(&f.ty, n) {
                Ok(v) => vs.push(v),
                Err(why) => bad.push((n.clone(), why)),
            }
        }
        (bad.is_empty().then_some(Value::Struct(vs)), bad)
    }

    /// Run every check before calling (§1.1: the form validator is `parse`):
    /// each field read as its type, then the input's bytes held to its
    /// refinements by the module's own `check_form`, each failure said with
    /// the module's message against the field it is in. Nothing is written.
    /// The input as its fields' values, when there is nothing to say
    /// against it.
    pub fn validate(&mut self, c: &Callable, lib: &Lib, module: &Module) -> Option<Value> {
        let (read, mut messages) = self.read();
        if let Some(v) = &read {
            match c.input_bytes(v) {
                Err(why) => messages.push((String::new(), why)),
                Ok(bytes) => match module.get(&c.name).and_then(|h| lib.get(&h)) {
                    None => messages.push((String::new(), format!("the module has no function {}", c.name))),
                    Some(f) => {
                        if let Err(fails) = check_form(&f.cx, &Interp::input_ty(&f.f), &bytes) {
                            for (path, why) in fails {
                                let field = match (c.record, path.first()) {
                                    (true, Some(&i)) => self.fields.get(i as usize).map_or(String::new(), |(n, _)| n.clone()),
                                    (false, _) => self.fields.first().map_or(String::new(), |(n, _)| n.clone()),
                                    (true, None) => String::new(),
                                };
                                messages.push((field, why));
                            }
                        }
                    }
                },
            }
        }
        let order = |n: &str| self.fields.iter().position(|(f, _)| f == n).unwrap_or(usize::MAX);
        messages.sort_by_key(|(n, _)| order(n));
        self.messages = messages;
        if self.messages.is_empty() {
            read
        } else {
            None
        }
    }

    /// What is wrong with field `name`, if anything.
    pub fn message(&self, name: &str) -> Option<&str> {
        self.messages.iter().find(|(n, _)| n == name).map(|(_, m)| m.as_str())
    }
}

fn slots_of(ty: &Ty, v: &Input, path: Vec<usize>, depth: usize, label: String, item: bool, out: &mut Vec<Slot>) {
    out.push(Slot {
        path: path.clone(),
        depth,
        label,
        ty: ty.clone(),
        item,
    });
    let deeper = |k: usize| {
        let mut p = path.clone();
        p.push(k);
        p
    };
    match (ty, v) {
        (Ty::Option(t), Input::Opt(true, inner)) => slots_of(t, inner, deeper(0), depth + 1, "value".into(), false, out),
        (Ty::List(t), Input::List(xs)) => {
            for (k, x) in xs.iter().enumerate() {
                slots_of(t, x, deeper(k), depth + 1, format!("{}", k + 1), true, out);
            }
        }
        (Ty::Struct(fs), Input::Fields(xs)) => {
            for (k, ((n, t), x)) in fs.iter().zip(xs).enumerate() {
                slots_of(t, x, deeper(k), depth + 1, n.clone(), false, out);
            }
        }
        _ => {}
    }
}

/// Whether a line of type `ty` is edited in place — a value on its own —
/// rather than being a heading for what is inside it.
pub fn is_value(ty: &Ty) -> bool {
    ty.is_scalar()
}

/// A query run on the host's store as who the explorer reads as, its input
/// the record of its fields: its answer, each element read under the
/// query's element type.
pub fn query(src: &Source, c: &Callable, input: &Value) -> Result<Value, String> {
    let h = src.module.get(&c.name).ok_or_else(|| format!("no function {}", c.name))?;
    let bytes = c.input_bytes(input)?;
    let rows = ark::view::query(src.lib, &h, &bytes, &src.ctx, src.store).map_err(|r| format!("refused: {}", r.0))?;
    rows.iter()
        .map(|v| decode(&c.returns, &v.to_bytes()).map_err(|e| format!("a bug: {e}")))
        .collect::<Result<Vec<_>, _>>()
        .map(Value::List)
}

/// What a call came to, kept for the session (`docs/plan-guards.md` D4): a
/// query's answer or why it failed; a mutation authored — its verdict is
/// the host's to say, asked through the ticket — or refused before it was.
#[derive(Clone, Debug, PartialEq)]
pub enum Called {
    Answer(Value),
    Failed(String),
    Authored(Ticket),
    Refused(String),
}

/// A call made from the explorer: the function, its input as its fields'
/// values, and what it came to.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub function: String,
    pub kind: Kind,
    pub args: Value,
    /// What a query's answer is a list of.
    pub returns: Ty,
    pub called: Called,
}

impl Call {
    /// The arguments, as a cell writes them.
    pub fn args_text(&self) -> String {
        crate::edit::cell_text(&self.args)
    }
}

/// One row of a table, as the picker offers it for an `Id(table)` field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pickable {
    pub id: [u8; 16],
    /// What it is called: the label column's value, or its short id.
    pub label: String,
    /// What else tells it apart, where the table has it.
    pub secondary: Option<String>,
}

impl Pickable {
    /// The id as a cell draws it: its first eight hex digits.
    pub fn short(&self) -> String {
        ark::hash::hex(&self.id)[..ID_DIGITS].to_string()
    }

    /// Everything the line says, which is what a filter matches.
    pub fn line(&self) -> String {
        match &self.secondary {
            Some(s) => format!("{} \u{b7} {s} \u{b7} {}", self.label, self.short()),
            None => format!("{} \u{b7} {}", self.label, self.short()),
        }
    }
}

/// A table's rows, each its columns' values, in key order.
pub fn rows_of(store: &Store, module: &Module, table: &str) -> Vec<Vec<Value>> {
    let Some((_, t)) = module.schema.table(table) else { return vec![] };
    let cx = value::types(module);
    let tys: Vec<Ty> = t.columns.iter().map(|c| value::column_ty(&cx, c)).collect();
    store.rows(table).filter_map(|r| value::decode_all(&tys, r).ok()).collect()
}

/// Which of a table's columns label its rows in the picker: the first text
/// column that is not a reference and that tells the rows apart well — at
/// least half as well as the one that tells them apart best, and never a
/// column that holds one value on every row (a kind, an owner), which says
/// nothing about which row it is. The *first* such, not the best: a file
/// path is different on every row and a title on nearly every one, and a
/// person reads the title. After it, the next such column that holds more
/// than one value, if any does.
pub fn label_columns(store: &Store, module: &Module, table: &str) -> (Option<usize>, Option<usize>) {
    let Some((_, tbl)) = module.schema.table(table) else {
        return (None, None);
    };
    let rows = rows_of(store, module, table);
    let distinct = |c: usize| {
        let mut seen: Vec<&str> = rows
            .iter()
            .filter_map(|r| match &r[c] {
                Value::Text(t) if !t.trim().is_empty() => Some(t.as_str()),
                _ => None,
            })
            .collect();
        seen.sort_unstable();
        seen.dedup();
        seen.len()
    };
    let candidates: Vec<(usize, usize)> = tbl
        .columns
        .iter()
        .enumerate()
        .filter(|(i, c)| c.decl == "Text" && !tbl.refs.iter().any(|r| r.column == *i))
        .map(|(i, _)| (i, distinct(i)))
        .filter(|(_, n)| *n > 0)
        .collect();
    let best = candidates.iter().map(|(_, n)| *n).max().unwrap_or(0);
    let enough = if rows.len() > 1 { best.div_ceil(2).max(2) } else { 1 };
    let Some(first) = candidates.iter().position(|(_, n)| *n >= enough) else {
        return (None, None);
    };
    let second = candidates
        .iter()
        .enumerate()
        .find(|(i, (_, n))| *i != first && *n > 1)
        .map(|(_, (c, _))| *c);
    (Some(candidates[first].0), second)
}

/// The rows of `table` an `Id(table)` field can name, labelled, in the
/// store's order: a row named by its key, which for such a table is one id.
pub fn pickables(store: &Store, module: &Module, table: &str) -> Vec<Pickable> {
    let (first, second) = label_columns(store, module, table);
    let text = |r: &[Value], c: Option<usize>| match c.map(|c| &r[c]) {
        Some(Value::Text(t)) if !t.trim().is_empty() => Some(t.clone()),
        _ => None,
    };
    rows_of(store, module, table)
        .iter()
        .filter_map(|r| {
            let Some(Value::Id(id)) = r.first() else { return None };
            let short = ark::hash::hex(id)[..ID_DIGITS].to_string();
            Some(Pickable {
                id: *id,
                label: text(r, first).unwrap_or(short),
                secondary: text(r, second),
            })
        })
        .collect()
}

/// The rows whose line holds `filter`, ignoring case: by place in `items`.
pub fn filter(items: &[Pickable], filter: &str) -> Vec<usize> {
    let needle = filter.trim().to_lowercase();
    items
        .iter()
        .enumerate()
        .filter(|(_, p)| needle.is_empty() || p.line().to_lowercase().contains(&needle))
        .map(|(i, _)| i)
        .collect()
}

/// The picker, open on one id field of the form: which table, what is typed
/// to filter it, and the cursor among what the filter leaves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picker {
    pub path: Vec<usize>,
    pub table: String,
    pub filter: String,
    pub at: usize,
}
