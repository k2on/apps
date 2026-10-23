//! §1.16 Rows without a Rust type: a JSON value under the schema, and a
//! table browsed.
//!
//! A domain's rows are Rust types and its views typed plans, run by name
//! ([`crate::Domain`]). What has no Rust type of a table — a vector's rows,
//! a host speaking JSON — holds a row as a JSON object
//! of its columns, read under the types the schema gives them ([`row`]),
//! written back as its row type's bytes in the value format ([`row_bytes`]),
//! its key the schema's types' key bytes ([`key_bytes`]).
//!
//! **Browsing** ([`Browse`]) is the one query over such rows: a table's
//! rows whose columns equal given values, whose key is within a range,
//! sorted by a column (last first, or first; ties in key order), the first
//! `n`. Values compare and sort as their columns' key bytes, which order as
//! the typed values do. It is a plan of the same operators ([`crate::plan`]
//! — `scan`, `filter`, `sort`, `take`), so a view of it is kept by patches
//! like any other.

use std::sync::Arc;

use serde_json::Value as Json;

use crate::db::Tables;
use crate::json::{typed_bytes, typed_key, typed_value};
use crate::peer::Ctx;
use crate::plan::{scan_raw, Desc, Make, Plan};
use crate::schema::{Schema, Table};
use crate::store::Store;
use crate::Refusal;

/// A schema known only at run time: the store's.
pub struct Dynamic;

impl Tables for Dynamic {
    type Db<'t, P: crate::db::Writes> = ();
    fn handles<'t, P: crate::db::Writes>(_: &'t std::cell::RefCell<crate::db::Txn<'t>>) {}
    fn schema() -> Schema {
        Schema::default()
    }
}

fn no(why: impl Into<String>) -> Refusal {
    Refusal(why.into())
}

// * Rows

/// A table's row as JSON: an object of its columns.
pub fn row(table: &Table, bytes: &[u8]) -> Result<Json, Refusal> {
    typed_value(&table.row_type(), bytes).map_err(|e| no(format!("a row of {}: {e}", table.name)))
}

/// A row's bytes from its JSON object, a column missing read as null: the
/// row's one encoding, as its Rust type's would be.
pub fn row_bytes(table: &Table, v: &Json) -> Result<Vec<u8>, Refusal> {
    typed_bytes(&table.row_type(), v)
}

/// The bytes of a key, given as its one column's value or, keyed by more,
/// an array of them.
pub fn key_bytes(table: &Table, key: &Json) -> Result<Vec<u8>, Refusal> {
    let cols = &table.columns[..table.key];
    let parts: Vec<&Json> = match (cols.len(), key) {
        (1, k) => vec![k],
        (_, Json::Array(ks)) if ks.len() == cols.len() => ks.iter().collect(),
        _ => return Err(no(format!("a key of {} is {} columns", table.name, cols.len()))),
    };
    let mut out = vec![];
    for (c, k) in cols.iter().zip(parts) {
        typed_key(&c.ty, k, &mut out)?;
    }
    Ok(out)
}

// * Browsing

/// A table browsed: its rows whose columns equal `eq`'s values, whose key
/// is from `from` (inclusive) and below `below`, sorted by a column (`true`
/// last first) or else by key, the first `take`. An element is a row's
/// bytes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Browse {
    pub table: String,
    pub eq: Vec<(String, Json)>,
    pub from: Option<Json>,
    pub below: Option<Json>,
    pub sort: Option<(String, bool)>,
    pub take: Option<usize>,
}

/// A column's key bytes in a row's bytes.
fn cell(t: &Table, c: usize, b: &[u8]) -> Vec<u8> {
    let v = row(t, b).unwrap_or_else(|e| panic!("{e}"));
    let mut out = vec![];
    typed_key(&t.columns[c].ty, &v[&t.columns[c].name], &mut out).unwrap_or_else(|e| panic!("{e}"));
    out
}

impl Browse {
    pub fn table(name: &str) -> Browse {
        Browse {
            table: name.into(),
            ..Browse::default()
        }
    }

    pub fn eq(mut self, column: &str, v: Json) -> Browse {
        self.eq.push((column.into(), v));
        self
    }

    pub fn range(mut self, from: Option<Json>, below: Option<Json>) -> Browse {
        (self.from, self.below) = (from, below);
        self
    }

    pub fn sort(mut self, column: &str, desc: bool) -> Browse {
        self.sort = Some((column.into(), desc));
        self
    }

    pub fn take(mut self, n: usize) -> Browse {
        self.take = Some(n);
        self
    }

    /// Its plan over `schema`, or why a column, a value or a key does not
    /// read under it.
    pub fn plan(&self, schema: &Schema) -> Result<Plan<Dynamic, Vec<u8>>, Refusal> {
        let t = Arc::new(schema.table(&self.table).ok_or_else(|| no(format!("no table {}", self.table)))?.1.clone());
        let col = |n: &str| t.column(n).ok_or_else(|| no(format!("{} has no column {n}", t.name)));
        let mut eq = vec![];
        for (n, v) in &self.eq {
            let (c, mut want) = (col(n)?, vec![]);
            typed_key(&t.columns[c].ty, v, &mut want)?;
            eq.push((c, want));
        }
        let bound = |k: &Option<Json>| k.as_ref().map(|k| key_bytes(&t, k)).transpose();
        let (from, below) = (bound(&self.from)?, bound(&self.below)?);
        let mut p = scan_raw::<Dynamic, _>(&self.table, |_, b| b.to_vec());
        if !eq.is_empty() || from.is_some() || below.is_some() {
            let t = t.clone();
            p = p.filter(move |b| {
                let key = t.parse_held(b).map(|p| p.key).unwrap_or_default();
                from.as_ref().is_none_or(|f| key >= *f)
                    && below.as_ref().is_none_or(|h| key < *h)
                    && eq.iter().all(|(c, want)| cell(&t, *c, b) == *want)
            });
        }
        if let Some((n, desc)) = &self.sort {
            let (c, t) = (col(n)?, t.clone());
            p = match desc {
                false => p.sort(move |b| cell(&t, c, b)),
                true => p.sort(move |b| Desc(cell(&t, c, b))),
            };
        }
        Ok(match self.take {
            Some(n) => p.take(n),
            None => p,
        })
    }

    /// How a view of it over `schema` is made ([`crate::plan::View::new`]).
    pub fn make(&self, schema: &Schema) -> Result<Make<Dynamic, Vec<u8>>, Refusal> {
        self.plan(schema)?;
        let (b, s) = (self.clone(), schema.clone());
        Ok(Arc::new(move |_, _| b.plan(&s)))
    }

    /// Its rows over `store`, afresh.
    pub fn rows(&self, store: &Store) -> Result<Vec<Vec<u8>>, Refusal> {
        let ctx = Ctx::default();
        Ok(self.plan(store.schema_arc())?.run(store, &ctx))
    }
}
