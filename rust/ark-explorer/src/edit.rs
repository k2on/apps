//! What an edit becomes, and what the console runs — the explorer's
//! decisions, with no window: a cell's text read back as its column's
//! value, a cell edit or a row delete routed to an exposed CRUD mutation or
//! to the authority's raw write (`docs/plan-guards.md` D4), and a query run
//! by name or a plan in the IR's form, read-only.

use ark::eval::{Args, EvalFault};
use ark::ir::FnKind;
use ark::schema::{Column, Schema, Ty};
use ark::store::{Change, Row};
use ark::value::{decode_hex, hex, Value};

use crate::data::{CrudVerbs, Source, Writer};

/// A cell as it is shown and as its editor starts: text as itself, an id as
/// `8-4-4-4-12` hex, bytes as hex, `null` for a column that holds nothing —
/// read back by [`parse_cell`] to the same value.
pub fn cell_text(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Text(t) => t.to_string(),
        Value::Bytes(b) => hex(b),
        Value::Id(id) => {
            let h = hex(id);
            format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
        }
        other => ark::json::json(other),
    }
}

/// A cell's text read as a value of its column: a nullable column takes
/// `null` (or nothing) as `Null`; a text is the text, an enum one of its
/// variants; an int in decimal; `true` or `false`; an id as 32 hex digits,
/// dashes anywhere; bytes as hex. The reason, where it is not one.
pub fn parse_cell(c: &Column, text: &str) -> Result<Value, String> {
    let t = text.trim();
    if c.nullable && (t.is_empty() || t == "null") {
        return Ok(Value::Null);
    }
    let bad = |what: &str| Err(format!("{}: {what}", c.name));
    match &c.ty {
        Ty::Text => Ok(Value::text(text)),
        Ty::Enum(vs) => match vs.iter().find(|v| v.as_str() == t) {
            Some(v) => Ok(Value::text(v.clone())),
            None => bad(&format!("one of {}", vs.join(", "))),
        },
        Ty::Int => t.parse::<i64>().map(Value::Int).or_else(|_| bad("a whole number")),
        Ty::Bool => match t {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => bad("true or false"),
        },
        Ty::Id(_) => match decode_hex(&t.replace('-', "")).and_then(|b| <[u8; 16]>::try_from(b).ok()) {
            Some(id) => Ok(Value::Id(id)),
            None => bad("an id, 32 hex digits"),
        },
        Ty::Bytes => match decode_hex(t) {
            Some(b) => Ok(Value::Bytes(b.into())),
            None => bad("hex digits"),
        },
        other => bad(&format!("a {other:?} is not a column's type")),
    }
}

/// Where an edit goes: a mutation of the domain's, authored as the host's
/// identity, or the authority's raw write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Author { function: String, args: Args },
    Raw(Change),
}

impl Route {
    /// Hand it to the writer.
    pub fn send(self, w: &mut dyn Writer) -> Result<(), String> {
        match self {
            Route::Author { function, args } => w.author(&function, args),
            Route::Raw(change) => w.raw(change),
        }
    }

    /// As a person would say it.
    pub fn describe(&self) -> String {
        match self {
            Route::Author { function, .. } => format!("through {function}"),
            Route::Raw(Change::Remove(t, _)) => format!("raw: delete_row on {t}"),
            Route::Raw(c) => format!("raw: put_row on {}", c.table()),
        }
    }
}

/// What an edit may go through: the CRUD the table exposes, whether the
/// writer has the authority's raw write, and whether the switch says to use
/// it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Via<'v> {
    pub verbs: Option<&'v CrudVerbs>,
    pub can_raw: bool,
    pub force_raw: bool,
}

/// The verbs exposed for `table`, if the host may author them.
fn authorable(verbs: Option<&CrudVerbs>) -> Option<&CrudVerbs> {
    verbs.filter(|v| v.may_author)
}

/// A row's columns as a mutation's arguments: what a generated write takes.
fn as_args(row: &Row) -> Args {
    row.to_struct()
}

/// `docs/plan-guards.md` D4 What setting `column` of `old` to `value` in
/// `table` becomes: through the table's `update_<t>` when it is exposed and
/// this host may author it (or `put_<t>` when only that is), unless
/// `force_raw`; through the authority's raw write otherwise, where the host
/// has one; and refused, with why, where it has neither. A key column is
/// not edited: a key is what names the row, and changing it is a delete and
/// a new row.
pub fn route_edit(schema: &Schema, via: Via, table: &str, old: &Row, column: &str, value: Value) -> Result<Route, String> {
    let tbl = schema.lookup_table(table).ok_or_else(|| format!("no table {table}"))?;
    if tbl.key.iter().any(|k| k == column) {
        return Err(format!(
            "{table}.{column} is part of the key, which names the row: delete it and put a new one"
        ));
    }
    if tbl.column(column).is_none() {
        return Err(format!("{table} has no column {column}"));
    }
    let new = old.clone().with(column, value);
    match (authorable(via.verbs), via.force_raw) {
        (Some(v), false) if v.update => Ok(Route::Author {
            function: format!("update_{table}"),
            args: as_args(&new),
        }),
        (Some(v), false) if v.put => Ok(Route::Author {
            function: format!("put_{table}"),
            args: as_args(&new),
        }),
        _ if via.can_raw => Ok(Route::Raw(Change::Edit(table.into(), old.clone(), new))),
        _ => Err(nowhere(table, "update")),
    }
}

/// What deleting `row` from `table` becomes: through `delete_<t>` when it is
/// exposed and this host may author it, unless `force_raw`; the raw write
/// otherwise, where there is one; refused where there is neither.
pub fn route_delete(schema: &Schema, via: Via, table: &str, row: &Row) -> Result<Route, String> {
    let tbl = schema.lookup_table(table).ok_or_else(|| format!("no table {table}"))?;
    match (authorable(via.verbs), via.force_raw) {
        (Some(v), false) if v.delete => Ok(Route::Author {
            function: format!("delete_{table}"),
            args: tbl.key.iter().map(|k| (k.clone(), row.get(k).cloned().unwrap_or(Value::Null))).collect(),
        }),
        _ if via.can_raw => Ok(Route::Raw(Change::Remove(table.into(), row.clone()))),
        _ => Err(nowhere(table, "delete")),
    }
}

fn nowhere(table: &str, verb: &str) -> String {
    format!("{table} exposes no {verb} this host may author, and this host is not the authority: nothing here can write it")
}

/// The console's one input, read: a query by name — `items` — run with
/// `args`, a struct in the vectors' JSON dialect (`{"playlist_id":
/// {"$id": "…"}}`, empty for none); or, when it starts with `{`, a plan in
/// the IR's form, as a module writes one, run with no arguments. Read-only
/// either way: a query cannot write, and a mutator's name is refused. The
/// answer, or what stopped it.
pub fn console(src: &Source, input: &str, args: &str) -> Result<Value, String> {
    let input = input.trim();
    if input.starts_with('{') {
        let v = ark::json::decode(input).map_err(|e| format!("the plan is not JSON of the dialect: {e}"))?;
        let mut plan = ark::ir::plan_from_value(&v).map_err(|e| format!("not a plan: {e}"))?;
        ark::eval::complete_order(src.schema, &mut plan);
        return ark::eval::select_plan(src.store.schema(), &plan, src.store)
            .map(Value::from)
            .map_err(|e| fault(&e));
    }
    let f = src.module.lookup_function(input).ok_or_else(|| format!("no function {input}"))?;
    if f.kind != FnKind::Query {
        return Err(format!("{input} is a {}, and the console runs queries alone", f.kind.name()));
    }
    let args = match args.trim() {
        "" => Args::new(),
        text => match ark::json::decode(text).map_err(|e| format!("the arguments are not JSON of the dialect: {e}"))? {
            Value::Struct(m) => *m,
            other => return Err(format!("the arguments are a struct, not {}", ark::json::json(&other))),
        },
    };
    ark::eval::query_as(src.module, input, &src.ctx, &args, src.store).map_err(|e| fault(&e))
}

fn fault(e: &EvalFault) -> String {
    match e {
        EvalFault::Verdict(r) => format!("refused: {}", ark::protocol::refusal_text(r)),
        EvalFault::Bug(b) => format!("a bug: {b:?}"),
    }
}
