//! What an edit becomes, and what the console runs — the explorer's
//! decisions, with no window: a cell's text read back as its column's
//! value, a cell edit or a row delete routed to an exposed CRUD mutation or
//! to the authority's raw write (`docs/plan-guards.md` D4), and a query run
//! by name, read-only.

use ark5::module::Module;
use ark5::schema::Table;
use ark5::store::Change;

use crate::data::{CrudVerbs, Source, Writer};
use crate::value::{self, decode, encode, Ty, Value};

/// A cell as it is shown and as its editor starts ([`value::text_of`]),
/// read back by [`parse_cell`] to the same value.
pub fn cell_text(v: &Value) -> String {
    value::text_of(v)
}

/// A cell's text read as a value of its column's type: an option takes
/// `null` (or nothing) as none; a text is the text, an enum `#n` or `n`; an
/// int in decimal; `true` or `false`; an id as 32 hex digits, dashes
/// anywhere; bytes as hex. The reason, where it is not one.
pub fn parse_cell(name: &str, ty: &Ty, text: &str) -> Result<Value, String> {
    let t = text.trim();
    let bad = |what: &str| Err(format!("{name}: {what}"));
    match ty {
        Ty::Option(_) if t.is_empty() || t == "null" => Ok(Value::Null),
        Ty::Option(inner) => parse_cell(name, inner, text),
        Ty::Text => Ok(Value::text(text)),
        Ty::Int => t.parse::<i64>().map(Value::Int).or_else(|_| bad("a whole number")),
        Ty::Bool => match t {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => bad("true or false"),
        },
        Ty::Id(_) => match crate::form::parse_id(t) {
            Some(id) => Ok(Value::Id(id)),
            None => bad("an id, 32 hex digits"),
        },
        Ty::Bytes => match ark5::hash::unhex(t) {
            Some(b) => Ok(Value::Bytes(b)),
            None => bad("hex digits"),
        },
        Ty::Enum(vs) => match t.trim_start_matches('#').parse::<usize>() {
            Ok(i) if i < vs.len() => Ok(Value::Variant(i as u8, vec![])),
            _ => bad(&format!("one of {}", vs.join(", "))),
        },
        other => bad(&format!("a {} is not edited as text", value::ty_name(other))),
    }
}

/// Where an edit goes: a mutation of the domain's, authored as the host's
/// identity with its input's bytes, or the authority's raw write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Author { function: String, input: Vec<u8> },
    Raw(Change),
}

impl Route {
    /// Hand it to the writer.
    pub fn send(self, w: &mut dyn Writer) -> Result<(), String> {
        match self {
            Route::Author { function, input } => w.author(&function, input).map(|_| ()),
            Route::Raw(change) => w.raw(change),
        }
    }

    /// As a person would say it.
    pub fn describe(&self) -> String {
        match self {
            Route::Author { function, .. } => format!("through {function}"),
            Route::Raw(Change::Remove { table, .. }) => format!("raw: delete on {table}"),
            Route::Raw(c) => format!("raw: put on {}", c.table()),
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

/// The binding `<verb>_<table>` goes by, under whatever router.
fn bound(module: &Module, verb: &str, table: &str) -> String {
    let want = format!("{verb}_{table}");
    module
        .bindings
        .iter()
        .map(|(n, _)| n)
        .find(|n| n.rsplit('.').next() == Some(&want))
        .cloned()
        .unwrap_or(want)
}

fn table<'m>(module: &'m Module, name: &str) -> Result<&'m Table, String> {
    module.schema.table(name).map(|(_, t)| t).ok_or_else(|| format!("no table {name}"))
}

/// `docs/plan-guards.md` D4 What setting column `column` of `old` (a row's
/// bytes) to `value` in `table` becomes: through the table's `update_<t>`
/// when it is exposed and this host may author it (or `put_<t>` when only
/// that is), unless `force_raw`; through the authority's raw write
/// otherwise, where the host has one; and refused, with why, where it has
/// neither. A key column is not edited: a key is what names the row.
pub fn route_edit(module: &Module, via: Via, table_name: &str, old: &[u8], column: &str, value: Value) -> Result<Route, String> {
    let t = table(module, table_name)?;
    let c = t.column(column).ok_or_else(|| format!("{table_name} has no column {column}"))?;
    if c < t.key {
        return Err(format!(
            "{table_name}.{column} is part of the key, which names the row: delete it and put a new one"
        ));
    }
    let row = value::row_ty(&value::types(module), t);
    let Ty::Struct(fs) = &row else { unreachable!("a row is a record") };
    let mut vs = match decode(&row, old).map_err(|e| e.0)? {
        Value::Struct(vs) => vs,
        _ => unreachable!("a row reads as a record"),
    };
    encode(&fs[c].1, &value).map_err(|e| format!("{column}: {e}"))?;
    vs[c] = value;
    let new = encode(&row, &Value::Struct(vs))?;
    match (via.verbs.filter(|v| v.may_author), via.force_raw) {
        (Some(v), false) if v.update => Ok(Route::Author {
            function: bound(module, "update", table_name),
            input: new,
        }),
        (Some(v), false) if v.put => Ok(Route::Author {
            function: bound(module, "put", table_name),
            input: new,
        }),
        _ if via.can_raw => Ok(Route::Raw(Change::Edit {
            table: table_name.into(),
            old: old.to_vec(),
            new,
        })),
        _ => Err(nowhere(table_name, "update")),
    }
}

/// What deleting `row` (its bytes) from `table` becomes: through
/// `delete_<t>`, its input the key's columns, when it is exposed and this
/// host may author it, unless `force_raw`; the raw write otherwise, where
/// there is one; refused where there is neither.
pub fn route_delete(module: &Module, via: Via, table_name: &str, row: &[u8]) -> Result<Route, String> {
    let t = table(module, table_name)?;
    match (via.verbs.filter(|v| v.may_author), via.force_raw) {
        (Some(v), false) if v.delete => Ok(Route::Author {
            function: bound(module, "delete", table_name),
            input: row[..t.key_len(row).map_err(|e| e.0)?].to_vec(),
        }),
        _ if via.can_raw => Ok(Route::Raw(Change::Remove {
            table: table_name.into(),
            row: row.to_vec(),
        })),
        _ => Err(nowhere(table_name, "delete")),
    }
}

fn nowhere(table: &str, verb: &str) -> String {
    format!("{table} exposes no {verb} this host may author, and this host is not the authority: nothing here can write it")
}

/// The binding a name means: itself, or the one whose last part it is.
pub fn binding(module: &Module, name: &str) -> Option<String> {
    let name = name.trim();
    let hits: Vec<&String> = module
        .bindings
        .iter()
        .map(|(n, _)| n)
        .filter(|n| *n == name || n.rsplit('.').next() == Some(name))
        .collect();
    match hits[..] {
        [one] => Some(one.clone()),
        _ => hits.into_iter().find(|n| *n == name).cloned(),
    }
}

/// The console's one input, read: a query by its binding (or the last part
/// of it) — `playlists` — run with `args`, its input's fields in order
/// separated by commas, each written as a cell is (`0a1b…` for an id, empty
/// for none). Read-only: a mutation's name is refused. The answer, or what
/// stopped it.
pub fn console(src: &Source, name: &str, args: &str) -> Result<Value, String> {
    let f = binding(src.module, name).ok_or_else(|| format!("no function {}", name.trim()))?;
    let c = crate::form::callables(src.module, src.lib)
        .into_iter()
        .find(|c| c.name == f)
        .ok_or_else(|| format!("{f} is neither a query nor a mutation"))?;
    if c.kind != crate::form::Kind::Query {
        return Err(format!("{f} is a {}, and the console runs queries alone", c.kind.name()));
    }
    let parts: Vec<&str> = match args.trim() {
        "" => vec![],
        a => a.split(',').collect(),
    };
    if parts.len() != c.input.len() {
        return Err(format!(
            "{f} takes {} argument{}: {}",
            c.input.len(),
            if c.input.len() == 1 { "" } else { "s" },
            c.input
                .iter()
                .map(|(n, x)| format!("{n}: {}", value::ty_name(&x.ty)))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let vs = c
        .input
        .iter()
        .zip(parts)
        .map(|((n, x), p)| parse_cell(n, &x.ty, p))
        .collect::<Result<Vec<_>, _>>()?;
    crate::form::query(src, &c, &Value::Struct(vs))
}
