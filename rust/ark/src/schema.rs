//! §2 The schema, as `Ark.Schema` defines them.
//!
//! A schema is a value in the module, not DDL: which tables exist, their
//! columns and key, which indexes are unique, and which columns reference
//! which table. A module has one set of tables and one log, so every
//! reference is checked and any function may read any table
//! (`docs/scopes.md` says what spec version 2's scopes were).

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use crate::store::{Columns, Row};
use crate::value::{FieldName, TableName, Value};

/// §2.1 The static types of the IR (`Ark.Schema.Ty`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ty {
    Bool,
    Int,
    Text,
    Bytes,
    Id(TableName),
    Enum(Vec<String>),
    Option(Box<Ty>),
    List(Box<Ty>),
    Struct(BTreeMap<FieldName, Ty>),
}

/// The types a column may have (before nullability).
pub fn is_scalar(t: &Ty) -> bool {
    matches!(t, Ty::Bool | Ty::Int | Ty::Text | Ty::Bytes | Ty::Id(_) | Ty::Enum(_))
}

/// A column; a nullable column's values are `Null` or a value of `ty`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Column {
    pub name: FieldName,
    pub ty: Ty,
    pub nullable: bool,
}

/// A declared index: a uniqueness constraint when `unique`, otherwise only a
/// statement of intent about performance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Index {
    pub columns: Vec<FieldName>,
    pub unique: bool,
}

/// `column REFERENCES table(key)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ref {
    pub column: FieldName,
    pub table: TableName,
}

/// A table. Built by [`Table::new`], which lays out its rows once: the
/// column names every row of it shares, and where its key columns are
/// among them (`docs/plan-perf.md` R11, `store.rs`'s module docs).
#[derive(Clone)]
pub struct Table {
    pub name: TableName,
    pub columns: Vec<Column>,
    pub key: Vec<FieldName>,
    pub indexes: Vec<Index>,
    pub refs: Vec<Ref>,
    /// `docs/plan-db.md` D4 The text indexes: each a text column whose
    /// folded value's trigrams name the rows holding them, which is what
    /// serves a `Pred::Has` (`store.rs`, `Trigrams`). An index kind beside
    /// [`Table::indexes`] — on the wire an `index` node of kind `text`, in
    /// the same list — kept apart here so that an [`Index`] is still the
    /// two fields every caller writes. Like a plain index it says nothing
    /// about the rows: additive under `compat`, and a store from before it
    /// builds it as the rows are put. Set by [`Table::with_text`].
    pub text: Vec<FieldName>,
    rows: Arc<Columns>,
    key_at: Vec<usize>,
}

/// A table is its declaration: how its rows are laid out follows from it.
impl PartialEq for Table {
    fn eq(&self, other: &Table) -> bool {
        self.name == other.name
            && self.columns == other.columns
            && self.key == other.key
            && self.indexes == other.indexes
            && self.refs == other.refs
            && self.text == other.text
    }
}

impl Eq for Table {}

impl fmt::Debug for Table {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Table")
            .field("name", &self.name)
            .field("columns", &self.columns)
            .field("key", &self.key)
            .field("indexes", &self.indexes)
            .field("refs", &self.refs)
            .field("text", &self.text)
            .finish()
    }
}

/// The tables, in declaration order — which is also the order a state
/// hash walks them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schema {
    pub tables: Vec<Table>,
}

/// Ascending or descending, for an order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Dir {
    Asc,
    Desc,
}

impl Schema {
    /// A schema with no tables.
    pub fn empty() -> Schema {
        Schema { tables: vec![] }
    }

    /// Every table, in schema order.
    pub fn tables(&self) -> impl Iterator<Item = &Table> {
        self.tables.iter()
    }

    /// `Ark.Schema.lookupTable`.
    pub fn lookup_table(&self, name: &str) -> Option<&Table> {
        self.tables().find(|t| t.name == name)
    }

    /// §2.2 Every relationship in a schema (`Ark.Schema.relations`).
    pub fn relations(&self) -> Vec<Relation> {
        self.tables()
            .flat_map(|t| {
                t.refs.iter().map(move |r| Relation {
                    parent: r.table.clone(),
                    child: t.name.clone(),
                    column: r.column.clone(),
                })
            })
            .collect()
    }

    /// The relationships reaching down from a table (`childrenOf`).
    pub fn children_of(&self, parent: &str) -> Vec<Relation> {
        self.relations().into_iter().filter(|r| r.parent == parent).collect()
    }

    /// The relationships reaching up from a table (`parentOf`).
    pub fn parent_of(&self, child: &str) -> Vec<Relation> {
        self.relations().into_iter().filter(|r| r.child == child).collect()
    }
}

impl Table {
    pub fn new(name: impl Into<TableName>, columns: Vec<Column>, key: Vec<FieldName>, indexes: Vec<Index>, refs: Vec<Ref>) -> Table {
        let rows = Arc::new(Columns::new(columns.iter().map(|c| c.name.clone()).collect()));
        let key_at = key.iter().filter_map(|k| rows.position(k)).collect();
        Table {
            name: name.into(),
            columns,
            key,
            indexes,
            refs,
            text: vec![],
            rows,
            key_at,
        }
    }

    /// D4 The table with a text index on each of `columns`, in order.
    pub fn with_text(mut self, columns: Vec<FieldName>) -> Table {
        self.text = columns;
        self
    }

    /// The names every row of this table holds its values under, in
    /// declared order: one allocation for the table, shared by its rows.
    ///
    /// A table's columns are not changed in place once it is built — its
    /// rows' names would no longer be its own — which a debug build checks.
    pub fn row_columns(&self) -> &Arc<Columns> {
        debug_assert!(
            self.rows.names().len() == self.columns.len() && self.rows.names().iter().zip(&self.columns).all(|(n, c)| *n == c.name),
            "{}: columns changed after the table was built; build it with Table::new",
            self.name
        );
        &self.rows
    }

    /// `Ark.Schema.column`.
    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.name == name)
    }

    /// The type of a whole row: a struct of every column (`rowTy`).
    pub fn row_ty(&self) -> Ty {
        Ty::Struct(self.columns.iter().map(|c| (c.name.clone(), c.column_ty())).collect())
    }

    /// The types of the key columns, in key order (`keyTy`).
    pub fn key_ty(&self) -> Vec<Ty> {
        self.key.iter().filter_map(|k| self.column(k).map(Column::column_ty)).collect()
    }

    /// The key of a row: the key columns' values in key order; a missing
    /// column reads as `Null` (`keyOf`).
    pub fn key_of(&self, row: &Row) -> Vec<Value> {
        if row.is_of(self) && self.key_at.len() == self.key.len() {
            return self.key_at.iter().map(|i| row.at(*i).clone()).collect();
        }
        self.key.iter().map(|k| row.get(k).cloned().unwrap_or(Value::Null)).collect()
    }
}

impl Column {
    /// The static type of a column, nullability included (`columnTy`).
    pub fn column_ty(&self) -> Ty {
        if self.nullable {
            Ty::Option(Box::new(self.ty.clone()))
        } else {
            self.ty.clone()
        }
    }
}

/// §2.2 A relationship: `child.column REFERENCES parent`. From the parent
/// the child table's name reaches down to the children (a childless parent
/// is still a row); from the child the parent's name reaches up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relation {
    pub parent: TableName,
    pub child: TableName,
    /// The child column that holds the parent's key.
    pub column: FieldName,
}

/// §2.3 Why a schema is not well-formed (`Ark.Schema.SchemaError`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchemaError {
    DuplicateTable(TableName),
    DuplicateColumn(TableName, FieldName),
    NoKey(TableName),
    UnknownKeyColumn(TableName, FieldName),
    NullableKey(TableName, FieldName),
    NonScalarColumn(TableName, FieldName),
    UnknownIndexColumn(TableName, FieldName),
    UnknownRefColumn(TableName, FieldName),
    UnknownRefTable(TableName, TableName),
    RefToCompositeKey(TableName, TableName),
    /// Table, column, wanted, found.
    RefTypeMismatch(TableName, FieldName, Ty, Ty),
    IdColumnWithoutRef(TableName, FieldName),
    /// An id-typed column must be a key of its own table or a reference,
    /// and name the table it references.
    IdNamesWrongTable(TableName, FieldName),
    /// D4 A text index is on one column whose type is text (nullable or
    /// not), once.
    TextIndexNotText(TableName, FieldName),
}

fn dups(names: impl Iterator<Item = String>) -> Vec<String> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for n in names {
        *counts.entry(n).or_insert(0) += 1;
    }
    counts.into_iter().filter(|(_, n)| *n > 1).map(|(k, _)| k).collect()
}

/// Every rule a schema breaks, in the spec's order (`checkSchema`).
pub fn check_schema(sch: &Schema) -> Vec<SchemaError> {
    let mut errs = Vec::new();
    let dup_tables = dups(sch.tables().map(|t| t.name.clone()));
    for t in sch.tables() {
        if dup_tables.contains(&t.name) {
            errs.push(SchemaError::DuplicateTable(t.name.clone()));
        }
    }
    for t in &sch.tables {
        per_table(sch, t, &mut errs);
    }
    errs
}

fn per_table(sch: &Schema, t: &Table, errs: &mut Vec<SchemaError>) {
    let tn = || t.name.clone();
    for c in dups(t.columns.iter().map(|c| c.name.clone())) {
        errs.push(SchemaError::DuplicateColumn(tn(), c));
    }
    if t.key.is_empty() {
        errs.push(SchemaError::NoKey(tn()));
    }
    for k in &t.key {
        if t.column(k).is_none() {
            errs.push(SchemaError::UnknownKeyColumn(tn(), k.clone()));
        }
    }
    for k in &t.key {
        if let Some(c) = t.column(k) {
            if c.nullable {
                errs.push(SchemaError::NullableKey(tn(), k.clone()));
            }
        }
    }
    for c in &t.columns {
        if !is_scalar(&c.ty) {
            errs.push(SchemaError::NonScalarColumn(tn(), c.name.clone()));
        }
    }
    for ix in &t.indexes {
        for c in &ix.columns {
            if t.column(c).is_none() {
                errs.push(SchemaError::UnknownIndexColumn(tn(), c.clone()));
            }
        }
    }
    for (i, c) in t.text.iter().enumerate() {
        match t.column(c) {
            None => errs.push(SchemaError::UnknownIndexColumn(tn(), c.clone())),
            Some(col) if col.ty != Ty::Text || t.text[..i].contains(c) => errs.push(SchemaError::TextIndexNotText(tn(), c.clone())),
            Some(_) => {}
        }
    }
    for r in &t.refs {
        per_ref(sch, t, r, errs);
    }
    for c in &t.columns {
        if let Ty::Id(of) = &c.ty {
            let is_ref = t.refs.iter().any(|r| r.column == c.name);
            let is_own_key = *of == t.name && t.key.contains(&c.name);
            if !is_ref && !is_own_key {
                errs.push(SchemaError::IdColumnWithoutRef(tn(), c.name.clone()));
            }
        }
    }
}

fn per_ref(sch: &Schema, t: &Table, r: &Ref, errs: &mut Vec<SchemaError>) {
    match (t.column(&r.column), sch.lookup_table(&r.table)) {
        (None, _) => errs.push(SchemaError::UnknownRefColumn(t.name.clone(), r.column.clone())),
        (_, None) => errs.push(SchemaError::UnknownRefTable(t.name.clone(), r.table.clone())),
        (Some(c), Some(p)) => {
            let key_ty = p.key_ty();
            if key_ty.len() == 1 {
                let want = match &key_ty[0] {
                    Ty::Id(_) => Ty::Id(r.table.clone()),
                    other => other.clone(),
                };
                if c.ty != want {
                    errs.push(SchemaError::RefTypeMismatch(t.name.clone(), r.column.clone(), want, c.ty.clone()));
                }
                if let Ty::Id(of) = &c.ty {
                    if *of != r.table {
                        errs.push(SchemaError::IdNamesWrongTable(t.name.clone(), r.column.clone()));
                    }
                }
            } else {
                errs.push(SchemaError::RefToCompositeKey(t.name.clone(), r.table.clone()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, ty: Ty) -> Column {
        Column {
            name: name.into(),
            ty,
            nullable: false,
        }
    }

    #[test]
    fn a_reference_is_two_relations_and_checks_its_type() {
        let sch = Schema {
            tables: vec![
                Table::new("p", vec![col("id", Ty::Id("p".into()))], vec!["id".into()], vec![], vec![]),
                Table::new(
                    "c",
                    vec![col("id", Ty::Int), col("p_id", Ty::Int)],
                    vec!["id".into()],
                    vec![],
                    vec![Ref {
                        column: "p_id".into(),
                        table: "p".into(),
                    }],
                ),
            ],
        };
        assert_eq!(sch.children_of("p").len(), 1);
        assert_eq!(sch.parent_of("c")[0].column, "p_id");
        assert_eq!(
            check_schema(&sch),
            vec![SchemaError::RefTypeMismatch("c".into(), "p_id".into(), Ty::Id("p".into()), Ty::Int)]
        );
    }
}
