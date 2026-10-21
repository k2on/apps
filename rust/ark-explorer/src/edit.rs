//! What an edit becomes — the explorer's one decision about writing, with
//! no window (`docs/plan-guards.md` D4): a cell edit or a row delete routed
//! to an exposed CRUD mutation, or to the authority's raw write, or refused
//! where the host has neither.

use ark::store::Change;

use crate::data::{Catalog, CrudVerbs, Writer};
use crate::value::{self, decode, encode, Ty, Value};

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
fn bound(catalog: &Catalog, verb: &str, table: &str) -> String {
    let want = format!("{verb}_{table}");
    let named = catalog.functions.iter().map(|f| &f.name).find(|n| n.rsplit('.').next() == Some(&want));
    named.cloned().unwrap_or(want)
}

fn nowhere(table: &str, verb: &str) -> String {
    format!("{table} exposes no {verb} this host may author, and this host is not the authority: nothing here can write it")
}

/// `docs/plan-guards.md` D4 What setting column `column` of `old` (a row's
/// bytes) to `value` in `table` becomes: through the table's `update_<t>`
/// when it is exposed and this host may author it (or `put_<t>` when only
/// that is), unless `force_raw`; through the authority's raw write
/// otherwise, where the host has one; and refused, with why, where it has
/// neither. A key column is not edited: a key is what names the row.
pub fn route_edit(catalog: &Catalog, via: Via, table: &str, old: &[u8], column: &str, value: Value) -> Result<Route, String> {
    let (_, t) = catalog.schema.table(table).ok_or_else(|| format!("no table {table}"))?;
    let c = t.column(column).ok_or_else(|| format!("{table} has no column {column}"))?;
    if c < t.key {
        return Err(format!(
            "{table}.{column} is part of the key, which names the row: delete it and put a new one"
        ));
    }
    let row = value::row_ty(t);
    let (Ty::Struct(fs), Value::Struct(mut vs)) = (&row, decode(&row, old).map_err(|e| e.0)?) else {
        unreachable!("a row reads as a record")
    };
    encode(&fs[c].1, &value).map_err(|e| format!("{column}: {e}"))?;
    vs[c] = value;
    let new = encode(&row, &Value::Struct(vs))?;
    match (via.verbs.filter(|v| v.may_author && !via.force_raw), via.can_raw) {
        (Some(v), _) if v.update || v.put => {
            let function = bound(catalog, if v.update { "update" } else { "put" }, table);
            Ok(Route::Author { function, input: new })
        }
        (_, true) => Ok(Route::Raw(Change::Edit {
            table: table.into(),
            old: old.to_vec(),
            new,
        })),
        _ => Err(nowhere(table, "update")),
    }
}

/// What deleting `row` (its bytes) from `table` becomes: through
/// `delete_<t>`, its input the key's columns, when it is exposed and this
/// host may author it, unless `force_raw`; the raw write otherwise, where
/// there is one; refused where there is neither.
pub fn route_delete(catalog: &Catalog, via: Via, table: &str, row: &[u8]) -> Result<Route, String> {
    let (_, t) = catalog.schema.table(table).ok_or_else(|| format!("no table {table}"))?;
    match (via.verbs.filter(|v| v.may_author && v.delete && !via.force_raw), via.can_raw) {
        (Some(_), _) => {
            let input = value::key_input(t, row)?;
            Ok(Route::Author {
                function: bound(catalog, "delete", table),
                input,
            })
        }
        (None, true) => Ok(Route::Raw(Change::Remove {
            table: table.into(),
            row: row.to_vec(),
        })),
        (None, false) => Err(nowhere(table, "delete")),
    }
}
