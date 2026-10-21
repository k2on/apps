//! §1.2 The schema: tables, their columns (each a named [`Type`]), keys,
//! declared indexes and references — the only place that knows what a
//! row is.
//!
//! A row is its table's row type in the value format ([`crate::format`]):
//! its columns in schema order; its key is its leading columns' key bytes, and an
//! index entry the indexed columns' key bytes, then the row's key
//! ([`Table::parse`]).
//!
//! **Append-only evolution.** A new schema appends columns to a table and
//! appends tables; it never removes, reorders or retypes, and a column
//! appended is an `Option` or carries a default (`#[serde(default)]`,
//! [`compat`]). So a row written under the older schema is read under the
//! newer by adding what it lacks, and a row written under the newer under
//! the older by dropping what it has beyond ([`Table::fit`]).
//!
//! A table is [`Space::Durable`] or [`Space::Volatile`] (§1.3, §1.9): a
//! volatile table is not in snapshots and not in the state hash.

#[cfg(feature = "values-json")]
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
#[cfg(feature = "values-json")]
use serde_json::value::RawValue;
use serde_json::{Map, Value as J};

use crate::format::Encode;
#[cfg(not(feature = "values-json"))]
use crate::format::{encode, take_seed};
use crate::hash::SchemaHash;
use crate::json::{typed_bytes, typed_key};
#[cfg(not(feature = "values-json"))]
use crate::json::{Seed, Typed};
use crate::row::Type;
use crate::Refusal;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Space {
    Durable,
    Volatile,
}

/// A column: its name, its type, and the value a row written before the
/// column existed takes (`None` for an `Option`, which takes null).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub ty: Type,
    #[serde(with = "crate::format::json_text")]
    pub default: Option<J>,
}

/// Columns by position; a unique index refuses a second row with the same
/// values in them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    pub columns: Vec<usize>,
    pub unique: bool,
}

/// A reference: the column's value names a row of `table` by its key (an
/// `Option` column only when it is `Some`). Judged at every write of the
/// referring row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ref {
    pub column: usize,
    pub table: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Table {
    pub name: String,
    pub columns: Vec<Column>,
    /// How many leading columns are the key; at least one.
    pub key: usize,
    pub indexes: Vec<Index>,
    pub refs: Vec<Ref>,
    pub space: Space,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schema {
    pub tables: Vec<Table>,
}

impl Schema {
    pub fn table(&self, name: &str) -> Option<(usize, &Table)> {
        self.tables.iter().enumerate().find(|(_, t)| t.name == name)
    }

    /// `sha256(enc(schema))`, the schema in the value format.
    pub fn hash(&self) -> SchemaHash {
        SchemaHash::of(&self.to_bytes())
    }
}

/// §1.2 Whether `new` is a prefix-extension of `old`: every old table, in
/// its place, with its columns, key, indexes, references and space as they
/// were, columns appended only as an `Option` or with a default; tables
/// appended only.
pub fn compat(old: &Schema, new: &Schema) -> Result<(), Refusal> {
    if new.tables.len() < old.tables.len() {
        return Err(Refusal::new("a table was removed"));
    }
    for (o, n) in old.tables.iter().zip(&new.tables) {
        let at = |what: &str| Refusal(format!("{}: {what}", o.name));
        if o.name != n.name || o.space != n.space || o.key != n.key {
            return Err(at("renamed, moved, rekeyed or moved to another space"));
        }
        if n.columns.len() < o.columns.len() || n.columns[..o.columns.len()] != o.columns[..] {
            return Err(at("a column was removed, reordered or retyped"));
        }
        if let Some(c) = n.columns[o.columns.len()..].iter().find(|c| !c.ty.is_option() && c.default.is_none()) {
            return Err(at(&format!("{} is appended with neither an Option type nor a default", c.name)));
        }
        if !n.indexes.starts_with(&o.indexes) || !n.refs.starts_with(&o.refs) {
            return Err(at("an index or a reference was removed or changed"));
        }
    }
    Ok(())
}

impl Table {
    /// A durable table keyed by its first column, with no columns yet.
    pub fn new(name: &str) -> Table {
        Table {
            name: name.into(),
            columns: vec![],
            key: 1,
            indexes: vec![],
            refs: vec![],
            space: Space::Durable,
        }
    }

    pub fn col(mut self, name: &str, ty: Type) -> Table {
        self.columns.push(Column {
            name: name.into(),
            ty,
            default: None,
        });
        self
    }

    pub fn keyed(mut self, n: usize) -> Table {
        self.key = n;
        self
    }

    pub fn index(mut self, columns: &[&str], unique: bool) -> Table {
        let columns = columns.iter().map(|c| self.column(c).expect("an index over a declared column")).collect();
        self.indexes.push(Index { columns, unique });
        self
    }

    pub fn refs(mut self, column: &str, table: &str) -> Table {
        let column = self.column(column).expect("a reference from a declared column");
        self.refs.push(Ref { column, table: table.into() });
        self
    }

    pub fn volatile(mut self) -> Table {
        self.space = Space::Volatile;
        self
    }

    pub fn column(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name == name)
    }

    /// A row's type: the record of its columns.
    pub fn row_type(&self) -> Type {
        Type::Record(self.name.clone(), self.columns.iter().map(|c| (c.name.clone(), c.ty.clone())).collect())
    }

    /// A row read: its columns as JSON, checking its bytes are the row's
    /// one encoding; and its key's bytes and each index's entry.
    pub fn parse(&self, row: &[u8]) -> Result<Parsed, Refusal> {
        self.read(row, true)
    }

    /// [`Table::parse`] of a row known to be its one encoding — one the
    /// store holds — not checked again.
    pub fn parse_held(&self, row: &[u8]) -> Result<Parsed, Refusal> {
        self.read(row, false)
    }

    fn read(&self, row: &[u8], check: bool) -> Result<Parsed, Refusal> {
        let cols = self.columns_of(row, check).map_err(|e| Refusal(format!("{}: {e}", self.name)))?;
        let col = |c: usize| cols[c].as_ref().expect("a key's or an index's column is read");
        let mut key = vec![];
        for c in 0..self.key {
            typed_key(&self.columns[c].ty, col(c), &mut key)?;
        }
        let mut entries = Vec::with_capacity(self.indexes.len());
        for ix in &self.indexes {
            let mut e = vec![];
            for &c in &ix.columns {
                typed_key(&self.columns[c].ty, col(c), &mut e)?;
            }
            e.extend_from_slice(&key);
            entries.push(e);
        }
        Ok(Parsed { cols, key, entries })
    }

    /// The columns a row's key, indexes and references read: every one
    /// when `check`, which also holds the bytes to the row's one encoding.
    fn needed(&self, check: bool) -> Vec<bool> {
        let mut need = vec![check; self.columns.len()];
        let read = (0..self.key)
            .chain(self.indexes.iter().flat_map(|ix| ix.columns.iter().copied()))
            .chain(self.refs.iter().map(|r| r.column));
        read.for_each(|c| need[c] = true);
        need
    }

    #[cfg(feature = "values-json")]
    fn columns_of(&self, row: &[u8], check: bool) -> Result<Vec<Option<J>>, String> {
        let raw: BTreeMap<&str, &RawValue> = serde_json::from_slice(row).map_err(|e| e.to_string())?;
        let value = |c: &Column| -> Result<J, String> {
            let r = raw.get(c.name.as_str()).ok_or_else(|| format!("a row without {}", c.name))?;
            serde_json::from_str(r.get()).map_err(|e| e.to_string())
        };
        let need = self.needed(check);
        let mut cols: Vec<Option<J>> = vec![None; self.columns.len()];
        let mut want = Vec::with_capacity(if check { row.len() } else { 0 });
        for (i, c) in self.columns.iter().enumerate().filter(|(i, _)| need[*i]) {
            let v = value(c)?;
            if check {
                // The row's one encoding: its columns in order, each its own.
                want.push(if i == 0 { b'{' } else { b',' });
                serde_json::to_writer(&mut want, &c.name).map_err(|e| e.to_string())?;
                want.push(b':');
                want.extend(typed_bytes(&c.ty, &v).map_err(|e| e.0)?);
            }
            cols[i] = Some(v);
        }
        want.push(b'}');
        if check && want != row {
            return Err("not a row's one encoding".into());
        }
        Ok(cols)
    }

    #[cfg(not(feature = "values-json"))]
    fn columns_of(&self, row: &[u8], check: bool) -> Result<Vec<Option<J>>, String> {
        let need = self.needed(check);
        let upto = need.iter().rposition(|n| *n).map_or(0, |i| i + 1);
        let mut cols: Vec<Option<J>> = vec![None; self.columns.len()];
        let mut rest = row;
        for (i, c) in self.columns[..upto].iter().enumerate() {
            let (v, r) = take_seed(Seed(&c.ty), rest).map_err(|e| format!("{}: {e}", c.name))?;
            if check && encode(&Typed(&c.ty, &v)).map_err(|e| e.0)? != rest[..rest.len() - r.len()] {
                return Err("not a row's one encoding".into());
            }
            (cols[i], rest) = (Some(v), r);
        }
        if check && !rest.is_empty() {
            return Err("not a row's one encoding".into());
        }
        Ok(cols)
    }

    /// A row's columns as some schema of this table wrote them, by name
    /// (JSON) or in order up to where its bytes end (postcard): a column
    /// beyond them is missing, one beyond this table's dropped.
    #[cfg(feature = "values-json")]
    fn columns_as_written(&self, row: &[u8]) -> Result<Vec<Option<J>>, String> {
        let mut v: Map<String, J> = serde_json::from_slice(row).map_err(|e| e.to_string())?;
        Ok(self.columns.iter().map(|c| v.remove(&c.name)).collect())
    }

    #[cfg(not(feature = "values-json"))]
    fn columns_as_written(&self, row: &[u8]) -> Result<Vec<Option<J>>, String> {
        let mut rest = row;
        let mut out = vec![];
        for c in &self.columns {
            if rest.is_empty() {
                out.push(None);
                continue;
            }
            let (v, r) = take_seed(Seed(&c.ty), rest).map_err(|e| format!("{}: {e}", c.name))?;
            out.push(Some(v));
            rest = r;
        }
        Ok(out)
    }

    /// A row of this table as some schema of it wrote it, read under this
    /// one: as it is; completed with the defaults of the columns appended
    /// since (an older schema's); or without the columns this one lacks (a
    /// newer one's) — and whether it was another's.
    pub fn fit(&self, row: &[u8]) -> Result<(Vec<u8>, bool), Refusal> {
        if self.parse(row).is_ok() {
            return Ok((row.to_vec(), false));
        }
        let at = |e: String| Refusal(format!("{}: {e}", self.name));
        let written = self.columns_as_written(row).map_err(at)?;
        let mut out = Map::new();
        for (c, v) in self.columns.iter().zip(written) {
            let x = match (v, &c.default) {
                (Some(x), _) => x,
                (None, Some(d)) => d.clone(),
                (None, None) if c.ty.is_option() => J::Null,
                (None, None) => return Err(at(format!("a row without {}", c.name))),
            };
            out.insert(c.name.clone(), x);
        }
        Ok((typed_bytes(&self.row_type(), &J::Object(out))?, true))
    }
}

/// A row as the store reads it: its columns, its key's bytes and its entry
/// in each of its table's indexes, in order.
pub struct Parsed {
    /// The columns read: every one of a row checked, of a held row those
    /// its key, indexes and references cover.
    pub cols: Vec<Option<J>>,
    pub key: Vec<u8>,
    pub entries: Vec<Vec<u8>>,
}

impl Parsed {
    /// The key bytes a reference from column `c` names, if it names one:
    /// none for an `Option` column that is null.
    pub fn named(&self, t: &Table, c: usize) -> Result<Option<Vec<u8>>, Refusal> {
        let col = &t.columns[c];
        let v = self.cols[c].as_ref().expect("a reference's column is read");
        let (ty, v) = match (&col.ty, v) {
            (Type::Option(_), J::Null) => return Ok(None),
            (Type::Option(t), v) => (&**t, v),
            (t, v) => (t, v),
        };
        let mut k = vec![];
        typed_key(ty, v, &mut k)?;
        Ok(Some(k))
    }
}
