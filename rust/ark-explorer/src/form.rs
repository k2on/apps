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

use ark::eval::{Args, Checked, Ctx};
use ark::ir::{Check, Field, FnKind, Function, Module};
use ark::schema::{is_scalar, Schema, Ty};
use ark::store::Store;
use ark::value::{decode_hex, hex, Value};

use crate::data::Ticket;

/// One function a person can call: its router, name, kind and input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Callable {
    pub router: String,
    pub name: String,
    pub kind: FnKind,
    pub input: Vec<(String, Field)>,
}

/// Every query and mutation of `module`, grouped by router in the order
/// each router first appears, and in the module's order within one.
pub fn callables(module: &Module) -> Vec<Callable> {
    let mut routers: Vec<String> = Vec::new();
    let callable = |f: &&Function| matches!(f.kind, FnKind::Query | FnKind::Mutator);
    for f in module.functions.iter().filter(callable) {
        let r = f.router.clone().unwrap_or_default();
        if !routers.contains(&r) {
            routers.push(r);
        }
    }
    routers
        .iter()
        .flat_map(|r| {
            module
                .functions
                .iter()
                .filter(callable)
                .filter(move |f| f.router.as_deref().unwrap_or_default() == r)
                .map(|f| Callable {
                    router: r.clone(),
                    name: f.name.clone(),
                    kind: f.kind,
                    input: f.input.clone(),
                })
        })
        .collect()
}

/// A kind as the list says it.
pub fn kind_name(k: FnKind) -> &'static str {
    match k {
        FnKind::Mutator => "mutation",
        FnKind::Query => "query",
        other => other.name(),
    }
}

/// A type as a person reads it: `text`, `id(playlist)`, `option(int)`,
/// `list(text)`, `enum(a | b)`, `{a: int, b: text}`.
pub fn ty_name(t: &Ty) -> String {
    match t {
        Ty::Bool => "bool".into(),
        Ty::Int => "int".into(),
        Ty::Text => "text".into(),
        Ty::Bytes => "bytes".into(),
        Ty::Id(table) => format!("id({table})"),
        Ty::Enum(vs) => format!("enum({})", vs.join(" | ")),
        Ty::Option(t) => format!("option({})", ty_name(t)),
        Ty::List(t) => format!("list({})", ty_name(t)),
        Ty::Struct(fs) => format!(
            "{{{}}}",
            fs.iter().map(|(n, t)| format!("{n}: {}", ty_name(t))).collect::<Vec<_>>().join(", ")
        ),
        Ty::Named(n) => n.clone(),
        Ty::NonEmpty(t) => format!("non_empty_list({})", ty_name(t)),
        Ty::PerDevice(t) => format!("per_device({})", ty_name(t)),
        Ty::Ephemeral(t) => format!("ephemeral({})", ty_name(t)),
        Ty::Sum(vs) => format!(
            "sum({})",
            vs.iter()
                .map(|(v, fs)| format!(
                    "{v} {{{}}}",
                    fs.iter().map(|(n, t)| format!("{n}: {}", ty_name(t))).collect::<Vec<_>>().join(", ")
                ))
                .collect::<Vec<_>>()
                .join(" | ")
        ),
    }
}

/// A field's checks, as they are said beside it: `trimmed`, `min 1 char`,
/// `max 120 chars`, `1 to 10`, `non-empty`, `exists`, and a refinement by its
/// message. A check with a message of its own says it too, so what a refusal
/// will read is on screen before it happens.
pub fn checks_said(f: &Field) -> Vec<String> {
    let chars = |n: i64| if n == 1 { "char" } else { "chars" };
    let why = |w: &Option<String>| w.as_ref().map_or(String::new(), |w| format!(" (\u{201c}{w}\u{201d})"));
    f.checks
        .iter()
        .map(|c| match c {
            Check::Trim => "trimmed".to_string(),
            Check::MinLen(n, w) => format!("min {n} {}{}", chars(*n), why(w)),
            Check::MaxLen(n, w) => format!("max {n} {}{}", chars(*n), why(w)),
            Check::Range(Some(lo), Some(hi), w) => format!("{lo} to {hi}{}", why(w)),
            Check::Range(Some(lo), None, w) => format!("at least {lo}{}", why(w)),
            Check::Range(None, Some(hi), w) => format!("at most {hi}{}", why(w)),
            Check::Range(None, None, w) => format!("any int{}", why(w)),
            Check::NonEmpty(w) => format!("non-empty{}", why(w)),
            Check::Exists(w) => format!("must exist{}", why(w)),
            Check::Refine(_, w) => format!("refined{}", why(w)),
        })
        .collect()
}

/// What one input of a form holds, shaped as its type is: the text of a
/// text, an int, an id or bytes (read when called, so a half-typed number is
/// kept as typed); a bool; an enum's variant by its place; an option's
/// "is there one" beside what it would be; a list's items; a struct's
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
            Ty::Struct(fs) => Input::Fields(fs.values().map(Input::blank).collect()),
            Ty::Int | Ty::Text | Ty::Bytes | Ty::Id(_) => Input::Text(String::new()),
            // A live state's types are no mutation's input: the explorer
            // forms the log's procedures, and offers them as text.
            Ty::Named(_) | Ty::NonEmpty(_) | Ty::Sum(_) | Ty::PerDevice(_) | Ty::Ephemeral(_) => Input::Text(String::new()),
        }
    }

    /// A value of `ty` as an input: how a recent call's arguments are put
    /// back in its form.
    pub fn of(ty: &Ty, v: &Value) -> Input {
        match (ty, v) {
            (Ty::Option(t), Value::Null) => Input::Opt(false, Box::new(Input::blank(t))),
            (Ty::Option(t), v) => Input::Opt(true, Box::new(Input::of(t, v))),
            (Ty::Bool, Value::Bool(b)) => Input::Bool(*b),
            (Ty::Enum(vs), Value::Text(t)) => Input::Choice(vs.iter().position(|x| x.as_str() == &**t).unwrap_or(0)),
            (Ty::List(t), Value::List(xs)) => Input::List(xs.iter().map(|x| Input::of(t, x)).collect()),
            (Ty::Struct(fs), Value::Struct(m)) => Input::Fields(
                fs.iter()
                    .map(|(n, t)| m.get(n).map_or_else(|| Input::blank(t), |x| Input::of(t, x)))
                    .collect(),
            ),
            (Ty::Int | Ty::Text | Ty::Bytes | Ty::Id(_), v) => Input::Text(crate::edit::cell_text(v)),
            (t, _) => Input::blank(t),
        }
    }

    /// The input `path` names below this one of type `ty`, and its type: a
    /// list's item by its place, a struct's field by its place, an option's
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
                Input::Fields(xs) => xs.get_mut(i)?.at_mut(fs.values().nth(i)?, rest),
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
            (Ty::Text, Input::Text(t)) => Ok(Value::text(t.as_str())),
            (Ty::Int, Input::Text(t)) => t.trim().parse::<i64>().map(Value::Int).or_else(|_| bad("a whole number")),
            (Ty::Id(_), Input::Text(t)) => parse_id(t).map(Value::Id).ok_or(()).or_else(|_| bad("an id, 32 hex digits")),
            (Ty::Bytes, Input::Text(t)) => match decode_hex(t.trim()) {
                Some(b) => Ok(Value::Bytes(b.into())),
                None => bad("hex digits"),
            },
            (Ty::Bool, Input::Bool(b)) => Ok(Value::Bool(*b)),
            (Ty::Enum(vs), Input::Choice(i)) => match vs.get(*i) {
                Some(v) => Ok(Value::text(v.clone())),
                None => bad(&format!("one of {}", vs.join(", "))),
            },
            (Ty::Option(_), Input::Opt(false, _)) => Ok(Value::Null),
            (Ty::Option(t), Input::Opt(true, inner)) => inner.value(t, name),
            (Ty::List(t), Input::List(xs)) => xs
                .iter()
                .enumerate()
                .map(|(k, x)| x.value(t, &format!("{name}.{}", k + 1)))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::list),
            (Ty::Struct(fs), Input::Fields(xs)) => fs
                .iter()
                .zip(xs)
                .map(|((n, t), x)| x.value(t, &format!("{name}.{n}")).map(|v| (n.clone(), v)))
                .collect::<Result<std::collections::BTreeMap<_, _>, _>>()
                .map(|m| Value::Struct(Box::new(m))),
            (t, _) => bad(&format!("not an input of {}", ty_name(t))),
        }
    }
}

/// An id as text — 32 hex digits, dashes anywhere, as a cell shows one and a
/// person pastes one.
pub fn parse_id(text: &str) -> Option<[u8; 16]> {
    decode_hex(&text.trim().replace('-', "")).and_then(|b| <[u8; 16]>::try_from(b).ok())
}

/// An id as a cell shows it whole: `8-4-4-4-12`.
pub fn id_text(id: &[u8; 16]) -> String {
    crate::edit::cell_text(&Value::Id(*id))
}

/// One line of a form, as the keyboard walks it and the view draws it: where
/// its input is, how deep, what it is called, and its type. An option, a
/// list and a struct are a line each with what is inside them below; an
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
    pub kind: FnKind,
    pub fields: Vec<(String, Field)>,
    pub values: Vec<Input>,
    /// The line the cursor is on; one past the last is the run button.
    pub at: usize,
    /// Per field (or `""` for the input as a whole), what is wrong with it:
    /// its type's reading or its checks', with the module's own messages.
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

    /// The form filled with a call's arguments.
    pub fn filled(c: &Callable, args: &Args) -> Form {
        let mut f = Form::new(c);
        f.values = c
            .input
            .iter()
            .map(|(n, fd)| args.get(n).map_or_else(|| Input::blank(&fd.ty), |v| Input::of(&fd.ty, v)))
            .collect();
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

    /// What the form reads as: the arguments of every field that reads as its
    /// type, and a message for every one that does not.
    pub fn read(&self) -> (Args, Vec<(String, String)>) {
        let mut args = Args::new();
        let mut bad = Vec::new();
        for ((n, f), v) in self.fields.iter().zip(&self.values) {
            match v.value(&f.ty, n) {
                Ok(v) => {
                    args.insert(n.clone(), v);
                }
                Err(why) => bad.push((n.clone(), why)),
            }
        }
        (args, bad)
    }

    /// Run every check before calling (`spec/AUTHORING.md` §1.3's form
    /// validator, `ark::eval::check`): each field that reads as its type
    /// through its own checks, with the module's messages, and the whole
    /// input's refinements once every field passes. Nothing is written. The
    /// arguments, when there is nothing to say against them.
    pub fn validate(&mut self, module: &Module, schema: &Schema, ctx: &Ctx, store: &dyn Store) -> Option<Args> {
        let (args, mut messages) = self.read();
        let checked = module.lookup_function(&self.function).map(|f| {
            let c = ark::hash::closure(module, f);
            ark::eval::check(schema, &c, ctx, &args, store)
        });
        let values = match checked {
            Some(Ok(Checked { messages: m, values })) => {
                messages.extend(m);
                values
            }
            Some(Err(bug)) => {
                messages.push((String::new(), format!("the checks could not run: {bug:?}")));
                Args::new()
            }
            None => {
                messages.push((String::new(), format!("the module has no function {}", self.function)));
                Args::new()
            }
        };
        // Back in the order of the fields, the whole input's last.
        let order = |n: &str| self.fields.iter().position(|(f, _)| f == n).unwrap_or(usize::MAX);
        messages.sort_by_key(|(n, _)| order(n));
        self.messages = messages;
        // What the checks normalised (a trim) is what is sent; a field they
        // did not reach is sent as it reads.
        self.messages.is_empty().then(|| {
            let mut out = args;
            out.extend(values);
            out
        })
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
    is_scalar(ty)
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

/// A call made from the explorer: the function, its arguments, and what it
/// came to.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub function: String,
    pub kind: FnKind,
    pub args: Args,
    pub called: Called,
}

impl Call {
    /// The arguments, as the vectors' dialect writes them.
    pub fn args_text(&self) -> String {
        ark::json::json(&Value::Struct(Box::new(self.args.clone())))
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
        hex(&self.id)[..crate::cells::ID_DIGITS].to_string()
    }

    /// Everything the line says, which is what a filter matches.
    pub fn line(&self) -> String {
        match &self.secondary {
            Some(s) => format!("{} \u{b7} {s} \u{b7} {}", self.label, self.short()),
            None => format!("{} \u{b7} {}", self.label, self.short()),
        }
    }
}

/// Which of a table's columns label its rows in the picker: the first text
/// column that is not an id or a reference and that tells the rows apart
/// well — at least half as well as the one that tells them apart best, and
/// never a column that holds one value on every row (a kind, an owner),
/// which says nothing about which row it is. The *first* such, not the
/// best: a file path is different on every row and a title on nearly every
/// one, and a person reads the title. After it, the next such column that
/// holds more than one value, if any does.
pub fn label_columns(store: &dyn Store, table: &str) -> (Option<String>, Option<String>) {
    let Some(tbl) = store.schema().lookup_table(table) else {
        return (None, None);
    };
    let rows = store.scan(table);
    let distinct = |c: &str| {
        let mut seen: Vec<&str> = rows
            .iter()
            .filter_map(|r| match r.get(c) {
                Some(Value::Text(t)) if !t.trim().is_empty() => Some(&**t),
                _ => None,
            })
            .collect();
        seen.sort_unstable();
        seen.dedup();
        seen.len()
    };
    let candidates: Vec<(String, usize)> = tbl
        .columns
        .iter()
        .filter(|c| c.ty == Ty::Text && !tbl.refs.iter().any(|r| r.column == c.name))
        .map(|c| (c.name.clone(), distinct(&c.name)))
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
        .map(|(_, (c, _))| c.clone());
    (Some(candidates[first].0.clone()), second)
}

/// The rows of `table` an `Id(table)` field can name, labelled, in the
/// store's order. A row is named by its key, which for such a table is one
/// id.
pub fn pickables(store: &dyn Store, table: &str) -> Vec<Pickable> {
    let Some(tbl) = store.schema().lookup_table(table) else {
        return vec![];
    };
    let (first, second) = label_columns(store, table);
    let text = |r: &ark::store::Row, c: &Option<String>| match c.as_ref().and_then(|c| r.get(c)) {
        Some(Value::Text(t)) if !t.trim().is_empty() => Some(t.to_string()),
        _ => None,
    };
    store
        .scan(table)
        .iter()
        .filter_map(|r| {
            let id = match tbl.key_of(r).as_slice() {
                [Value::Id(id)] => *id,
                _ => return None,
            };
            let short = hex(&id)[..crate::cells::ID_DIGITS].to_string();
            Some(Pickable {
                id,
                label: text(r, &first).unwrap_or(short),
                secondary: text(r, &second),
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
