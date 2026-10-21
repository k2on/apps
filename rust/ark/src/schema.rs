//! §1.2 The schema: tables, their columns (each a named type of §1.1),
//! keys, declared indexes and references — the only place that knows what
//! a row's bytes are.
//!
//! A row is the tuple of its table's columns in schema order, and a table's
//! key is its leading columns, so the key's bytes are a prefix of the row's
//! and rows order by key ([`Table::key_len`]). An index entry is the
//! indexed columns' bytes concatenated, then the key's.
//!
//! **Append-only evolution.** A new schema appends columns to a table and
//! appends tables; it never removes, reorders or retypes, and a column
//! appended is an `Option` or carries a default ([`compat`]). So a row
//! written under the older schema is read under the newer by appending
//! what it lacks ([`Table::complete`]), and a row written under the newer
//! is read under the older by its prefix ([`Table::prefix`]).
//!
//! A table is [`Space::Durable`] or [`Space::Volatile`] (§1.3, §1.9): a
//! volatile table is not in snapshots and not in the state hash.

use ark_macros::Node;

use crate::codec::Node;
use crate::codec::{skip, Reader};
use crate::hash::SchemaHash;
use crate::ty::Ty;
use crate::Refusal;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Node)]
pub enum Space {
    Durable,
    Volatile,
}

/// A column: its name, the name of its declared type (`Id<playlist>`,
/// `Text`, `Int`: what the type section calls it, and what a human form
/// prints by), that type's structure, and the value a row written before
/// the column existed takes.
#[derive(Clone, Debug, PartialEq, Eq, Node)]
pub struct Column {
    pub name: String,
    pub decl: String,
    pub ty: Ty,
    pub default: Option<Vec<u8>>,
}

/// Columns by position; a unique index refuses a second row with the same
/// values in them.
#[derive(Clone, Debug, PartialEq, Eq, Node)]
pub struct Index {
    pub columns: Vec<usize>,
    pub unique: bool,
}

/// A reference: the column's value names a row of `table` by its key (an
/// `Option` column only when it is `Some`). Judged at every write of the
/// referring row.
#[derive(Clone, Debug, PartialEq, Eq, Node)]
pub struct Ref {
    pub column: usize,
    pub table: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Node)]
pub struct Table {
    pub name: String,
    pub columns: Vec<Column>,
    /// How many leading columns are the key; at least one.
    pub key: usize,
    pub indexes: Vec<Index>,
    pub refs: Vec<Ref>,
    pub space: Space,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Node)]
pub struct Schema {
    pub tables: Vec<Table>,
}

impl Schema {
    pub fn table(&self, name: &str) -> Option<(usize, &Table)> {
        self.tables.iter().enumerate().find(|(_, t)| t.name == name)
    }

    /// The schema as a value: `(List (name, List (name, decl, Ty,
    /// Option<Bytes>), key, List (List Int, Bool), List (Int, name),
    /// Space))`, derived.
    pub fn enc(&self, out: &mut Vec<u8>) {
        self.put(out);
    }

    pub fn hash(&self) -> SchemaHash {
        let mut b = vec![];
        self.enc(&mut b);
        SchemaHash::of(&b)
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

    pub fn col(mut self, name: &str, decl: &str, ty: Ty) -> Table {
        self.columns.push(Column {
            name: name.into(),
            decl: decl.into(),
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

    /// `Row<t>`: the tuple of the columns.
    pub fn row_ty(&self) -> Ty {
        Ty::Tuple(self.columns.iter().map(|c| c.ty.clone()).collect())
    }

    /// Each column's span in a row, checking the row is exactly canonical
    /// bytes of [`Table::row_ty`].
    pub fn spans(&self, row: &[u8]) -> Result<Vec<(usize, usize)>, Refusal> {
        let mut r = Reader::new(row);
        let mut out = Vec::with_capacity(self.columns.len());
        for c in &self.columns {
            let s = r.at;
            skip(&c.ty, &mut r).map_err(|e| Refusal(format!("{}.{}: {e}", self.name, c.name)))?;
            out.push((s, r.at));
        }
        r.end().map_err(|e| Refusal(format!("{}: {e}", self.name)))?;
        Ok(out)
    }

    /// How many leading bytes of a well-formed row are its key.
    pub fn key_len(&self, row: &[u8]) -> Result<usize, Refusal> {
        let mut r = Reader::new(row);
        for c in &self.columns[..self.key] {
            skip(&c.ty, &mut r)?;
        }
        Ok(r.at)
    }

    /// A row written under an older schema, completed with the defaults of
    /// the columns appended since (`None` for an `Option`).
    pub fn complete(&self, row: &[u8]) -> Result<Vec<u8>, Refusal> {
        let mut r = Reader::new(row);
        let mut out = row.to_vec();
        for c in &self.columns {
            if r.at == row.len() {
                match &c.default {
                    Some(d) => out.extend_from_slice(d),
                    None if c.ty.is_option() => out.push(0),
                    None => return Err(Refusal(format!("{}: a row without {}", self.name, c.name))),
                }
            } else {
                skip(&c.ty, &mut r)?;
            }
        }
        r.end()?;
        Ok(out)
    }

    /// A row of this table as some schema of it wrote it, read under this
    /// one: as it is, completed (an older schema's), or its prefix (a newer
    /// one's); and whether it was another's.
    pub fn fit(&self, row: &[u8]) -> Result<(Vec<u8>, bool), Refusal> {
        if self.spans(row).is_ok() {
            return Ok((row.to_vec(), false));
        }
        match self.complete(row) {
            Ok(r) => Ok((r, true)),
            Err(_) => Ok((self.prefix(row)?.to_vec(), true)),
        }
    }

    /// The prefix of a row written under a newer schema that this table's
    /// columns read: what an older runtime holds of it.
    pub fn prefix<'a>(&self, row: &'a [u8]) -> Result<&'a [u8], Refusal> {
        let mut r = Reader::new(row);
        for c in &self.columns {
            skip(&c.ty, &mut r)?;
        }
        Ok(&row[..r.at])
    }
}
