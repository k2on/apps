//! §4 The store, as `Ark.Store` defines it.
//!
//! An ordered key-value backend under five operations — `get`, `scan`,
//! `put`, `delete`, and the commit a batch of them makes. Not-null,
//! uniqueness and references are enforced here, identically everywhere, as
//! deterministic refusals; an index changes nothing.
//!
//! [`Store`] is the interface a runtime's backend implements; the
//! constraints and the `select` semantics are default methods over its four
//! primitives, so that every backend judges a write the same way.
//! [`MemoryStore`] is the spec's map of maps; [`Overlay`] is the optimistic
//! overlay a mutator writes into, consulted first on every read and dropped
//! on a verdict.

use std::collections::BTreeMap;
use std::fmt;

use crate::ir::{Expr, Plan};
use crate::schema::{Index, Ref, Relation, Schema, Table, Ty};
use crate::value::{FieldName, TableName, Value};
use crate::view;

/// A row: every column of its table, by name. Never partial once stored.
pub type Row = BTreeMap<FieldName, Value>;

/// The key columns' values, in key order.
pub type Key = Vec<Value>;

/// §4.1 What a write reports (`Ark.Store.Change`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Add(TableName, Row),
    Remove(TableName, Row),
    /// Old, new.
    Edit(TableName, Row, Row),
}

impl Change {
    pub fn table(&self) -> &str {
        match self {
            Change::Add(t, _) | Change::Remove(t, _) | Change::Edit(t, _, _) => t,
        }
    }
}

/// §4.2 A refusal: a verdict about the write, reached identically by every
/// replica (`Ark.Store.Refusal`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    NoSuchTable(TableName),
    /// The row is not a full, well-typed row of the table.
    MalformedRow(TableName, String),
    NotNull(TableName, FieldName),
    UniqueViolation(TableName, Vec<FieldName>),
    /// A reference names a parent row that does not exist: table, column,
    /// parent table.
    MissingParent(TableName, FieldName, TableName),
    /// Deleting a row that other rows reference: table, child table.
    StillReferenced(TableName, TableName),
    /// An explicit `refuse` from a mutator, or a checked arithmetic fault.
    Refused(String),
}

impl fmt::Display for Refusal {
    /// The spec's `show`, as far as Rust's `{:?}` on a string agrees with
    /// Haskell's (it does for ASCII without control characters): this is
    /// the text a server puts in a `Reject` frame.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::NoSuchTable(t) => write!(f, "NoSuchTable {t:?}"),
            Refusal::MalformedRow(t, why) => write!(f, "MalformedRow {t:?} {why:?}"),
            Refusal::NotNull(t, c) => write!(f, "NotNull {t:?} {c:?}"),
            Refusal::UniqueViolation(t, cs) => write!(f, "UniqueViolation {t:?} {cs:?}"),
            Refusal::MissingParent(t, c, p) => write!(f, "MissingParent {t:?} {c:?} {p:?}"),
            Refusal::StillReferenced(t, c) => write!(f, "StillReferenced {t:?} {c:?}"),
            Refusal::Refused(why) => write!(f, "Refused {why:?}"),
        }
    }
}

impl std::error::Error for Refusal {}

/// The store's interface. A backend implements the four primitives; the
/// constraints, the reads generated code makes and `select` are given.
///
/// `scan` must return rows in key order under `compare_value`, and a table
/// with no rows is empty rather than absent.
pub trait Store {
    fn schema(&self) -> &Schema;

    /// The row under a key, if any.
    fn get(&self, table: &str, key: &[Value]) -> Option<Row>;

    /// Every row of a table, in key order.
    fn scan(&self, table: &str) -> Vec<Row>;

    /// §4.5 Apply a change as a fact: raw, unjudged. `Add` and `Edit` write
    /// the new row under its key; `Remove` drops the key. An unknown table
    /// is ignored.
    fn apply_change(&mut self, change: &Change);

    fn apply_changes(&mut self, changes: &[Change]) {
        for c in changes {
            self.apply_change(c);
        }
    }

    /// §4.3 Write a row (`Ark.Store.put`). Nullable columns the row omits
    /// are filled with `Null`; then the row must be exactly the table's
    /// columns at their types, no non-nullable column `Null`, every unique
    /// index still unique against every other row, every reference finding
    /// its parent. A new key is an `Add`, an equal row is nothing, anything
    /// else an `Edit`.
    fn put(&mut self, table: &str, row: Row) -> Result<Option<Change>, Refusal> {
        let change = judge_put(self.as_store(), table, row)?;
        if let Some(c) = &change {
            self.apply_change(c);
        }
        Ok(change)
    }

    /// §4.4 Delete by key (`Ark.Store.delete`). A missing row is nothing; a
    /// row another row references is a refusal.
    fn delete(&mut self, table: &str, key: &[Value]) -> Result<Option<Change>, Refusal> {
        let change = judge_delete(self.as_store(), table, key)?;
        if let Some(c) = &change {
            self.apply_change(c);
        }
        Ok(change)
    }

    /// `exists`, as a value.
    fn exists(&self, table: &str, key: &[Value]) -> bool {
        self.get(table, key).is_some()
    }

    /// `db.get` as GENERATED.md has it: the row, or `Null`.
    fn get_value(&self, table: &str, key: &[Value]) -> Value {
        self.get(table, key).map(Value::Struct).unwrap_or(Value::Null)
    }

    /// `db.select(plan)`: the nodes `Ark.Eval.select` builds, as a list.
    /// The plan's right-hand sides must already be literals, as generated
    /// code builds them; anything else is a bug.
    fn select(&self, plan: &Plan) -> Value {
        let vp = view::eval_plan(plan, &mut |e: &Expr| match e {
            Expr::Lit(v) => Ok(v.clone()),
            other => Err(format!("select: a right-hand side that is not a literal: {other:?}")),
        })
        .unwrap_or_else(|e| panic!("{e} (a bug: generated code evaluates its plans)"));
        Value::List(
            view::pull(self.schema(), &vp, self.as_store())
                .into_iter()
                .map(|(_, node)| node)
                .collect(),
        )
    }

    /// Every table's rows, as `store_before`/`store_after` in the vectors:
    /// a struct from table name to the list of rows, in schema order.
    fn store_value(&self) -> Value {
        Value::Struct(
            self.schema()
                .tables()
                .map(|t| (t.name.clone(), Value::List(self.scan(&t.name).into_iter().map(Value::Struct).collect())))
                .collect(),
        )
    }

    /// `self` as a trait object, for the shared rules.
    fn as_store(&self) -> &dyn Store;
}

// The rules -------------------------------------------------------------

/// Fill in every nullable column the row left out, as `Null`
/// (`Ark.Store.complete`).
pub fn complete(tbl: &Table, mut row: Row) -> Row {
    for c in &tbl.columns {
        if c.nullable {
            row.entry(c.name.clone()).or_insert(Value::Null);
        }
    }
    row
}

/// What `put` would report and the row it would store, without writing.
pub fn judge_put(st: &dyn Store, tn: &str, row0: Row) -> Result<Option<Change>, Refusal> {
    let tbl = st.schema().lookup_table(tn).ok_or_else(|| Refusal::NoSuchTable(tn.into()))?;
    let row = complete(tbl, row0);
    well_typed(tbl, &row)?;
    let k = tbl.key_of(&row);
    let here = st.scan(tn);
    let others: Vec<&Row> = here.iter().filter(|r| tbl.key_of(r) != k).collect();
    for ix in tbl.indexes.iter().filter(|ix| ix.unique) {
        unique(tbl, &others, &row, ix)?;
    }
    for r in &tbl.refs {
        parent_exists(st, tbl, &row, r)?;
    }
    Ok(match st.get(tn, &k) {
        None => Some(Change::Add(tn.into(), row)),
        Some(old) if old == row => None,
        Some(old) => Some(Change::Edit(tn.into(), old, row)),
    })
}

/// What `delete` would report, without writing.
pub fn judge_delete(st: &dyn Store, tn: &str, k: &[Value]) -> Result<Option<Change>, Refusal> {
    let tbl = st.schema().lookup_table(tn).ok_or_else(|| Refusal::NoSuchTable(tn.into()))?;
    match st.get(tn, k) {
        None => Ok(None),
        Some(row) => {
            for rel in st.schema().children_of(tn) {
                no_child(st, tbl, k, &rel)?;
            }
            Ok(Some(Change::Remove(tn.into(), row)))
        }
    }
}

// A row is exactly the table's columns, each holding a value of the
// column's type (`Null` only where nullable).
fn well_typed(tbl: &Table, row: &Row) -> Result<(), Refusal> {
    let want: std::collections::BTreeSet<&str> = tbl.columns.iter().map(|c| c.name.as_str()).collect();
    let have: std::collections::BTreeSet<&str> = row.keys().map(|k| k.as_str()).collect();
    if want != have {
        let have: Vec<&str> = have.into_iter().collect();
        let want: Vec<&str> = tbl.columns.iter().map(|c| c.name.as_str()).collect();
        return Err(Refusal::MalformedRow(tbl.name.clone(), format!("columns {have:?} are not {want:?}")));
    }
    for c in &tbl.columns {
        match row.get(&c.name) {
            None => return Err(Refusal::MalformedRow(tbl.name.clone(), c.name.clone())),
            Some(Value::Null) => {
                if !c.nullable {
                    return Err(Refusal::NotNull(tbl.name.clone(), c.name.clone()));
                }
            }
            Some(v) => {
                if !of_type(&c.ty, v) {
                    return Err(Refusal::MalformedRow(tbl.name.clone(), format!("{} has the wrong type", c.name)));
                }
            }
        }
    }
    Ok(())
}

/// Whether a value inhabits a scalar type. Ids are untyped at run time.
pub fn of_type(t: &Ty, v: &Value) -> bool {
    match (t, v) {
        (Ty::Bool, Value::Bool(_)) => true,
        (Ty::Int, Value::Int(_)) => true,
        (Ty::Text, Value::Text(_)) => true,
        (Ty::Bytes, Value::Bytes(_)) => true,
        (Ty::Id(_), Value::Id(_)) => true,
        (Ty::Enum(vs), Value::Text(x)) => vs.contains(x),
        (Ty::Option(_), Value::Null) => true,
        (Ty::Option(t), v) => of_type(t, v),
        _ => false,
    }
}

fn unique(tbl: &Table, others: &[&Row], row: &Row, ix: &Index) -> Result<(), Refusal> {
    let proj = |r: &Row| -> Vec<Option<Value>> { ix.columns.iter().map(|c| r.get(c).cloned()).collect() };
    let mine = proj(row);
    // A NULL is not equal to anything, itself included, so two rows that
    // are both NULL in a unique column do not clash.
    let comparable = mine.iter().all(|v| matches!(v, Some(v) if !v.is_null()));
    if comparable && others.iter().any(|r| proj(r) == mine) {
        Err(Refusal::UniqueViolation(tbl.name.clone(), ix.columns.clone()))
    } else {
        Ok(())
    }
}

fn parent_exists(st: &dyn Store, tbl: &Table, row: &Row, r: &Ref) -> Result<(), Refusal> {
    match row.get(&r.column) {
        Some(Value::Null) => Ok(()),
        Some(v) => {
            if st.exists(&r.table, std::slice::from_ref(v)) {
                Ok(())
            } else {
                Err(Refusal::MissingParent(tbl.name.clone(), r.column.clone(), r.table.clone()))
            }
        }
        None => Err(Refusal::MalformedRow(tbl.name.clone(), r.column.clone())),
    }
}

fn no_child(st: &dyn Store, tbl: &Table, k: &[Value], rel: &Relation) -> Result<(), Refusal> {
    if let [kv] = k {
        if st.scan(&rel.child).iter().any(|r| r.get(&rel.column) == Some(kv)) {
            return Err(Refusal::StillReferenced(tbl.name.clone(), rel.child.clone()));
        }
    }
    Ok(())
}

// The in-memory store -----------------------------------------------------

/// The spec's store: the rows of every table, each keyed by its key
/// (`Ark.Store.Store`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryStore {
    schema: Schema,
    tables: BTreeMap<TableName, BTreeMap<Key, Row>>,
}

impl MemoryStore {
    /// `Ark.Store.empty`.
    pub fn empty(schema: Schema) -> MemoryStore {
        MemoryStore {
            schema,
            tables: BTreeMap::new(),
        }
    }

    /// The rows of a table, by key (`Ark.Store.rows`).
    pub fn rows(&self, table: &str) -> BTreeMap<Key, Row> {
        self.tables.get(table).cloned().unwrap_or_default()
    }

    /// Every table of the schema, in schema order (`Ark.Store.tableNames`).
    pub fn table_names(&self) -> Vec<TableName> {
        self.schema.tables().map(|t| t.name.clone()).collect()
    }

    /// Whether any table holds a row.
    pub fn is_empty(&self) -> bool {
        self.tables.values().all(|t| t.is_empty())
    }

    /// The rows of two stores over one schema, together; where both hold a
    /// table, `self` wins (`Ark.Store.merge`).
    pub fn merge(&self, other: &MemoryStore) -> MemoryStore {
        let mut out = self.clone();
        for (t, rows) in &other.tables {
            let mine = out.tables.entry(t.clone()).or_default();
            for (k, r) in rows {
                mine.entry(k.clone()).or_insert_with(|| r.clone());
            }
        }
        out
    }

    /// A store from the vectors' shape: a struct from table name to rows,
    /// applied raw.
    pub fn from_value(schema: Schema, v: &Value) -> MemoryStore {
        let mut st = MemoryStore::empty(schema);
        if let Value::Struct(m) = v {
            for (t, rows) in m {
                if let Value::List(rs) = rows {
                    for r in rs {
                        if let Value::Struct(row) = r {
                            st.apply_change(&Change::Add(t.clone(), row.clone()));
                        }
                    }
                }
            }
        }
        st
    }
}

impl Store for MemoryStore {
    fn schema(&self) -> &Schema {
        &self.schema
    }

    fn get(&self, table: &str, key: &[Value]) -> Option<Row> {
        self.tables.get(table).and_then(|t| t.get(key)).cloned()
    }

    fn scan(&self, table: &str) -> Vec<Row> {
        self.tables.get(table).map(|t| t.values().cloned().collect()).unwrap_or_default()
    }

    fn apply_change(&mut self, change: &Change) {
        match change {
            Change::Add(t, row) | Change::Edit(t, _, row) => {
                if let Some(tbl) = self.schema.lookup_table(t) {
                    let k = tbl.key_of(row);
                    self.tables.entry(t.clone()).or_default().insert(k, row.clone());
                }
            }
            Change::Remove(t, row) => {
                if let Some(tbl) = self.schema.lookup_table(t) {
                    let k = tbl.key_of(row);
                    if let Some(rows) = self.tables.get_mut(t) {
                        rows.remove(&k);
                    }
                }
            }
        }
    }

    fn as_store(&self) -> &dyn Store {
        self
    }
}

// The overlay -------------------------------------------------------------

/// An optimistic overlay over a base store: `(table, key) -> Option<row>`,
/// consulted first on every read, with the base untouched until the caller
/// commits the changes it produced. Dropping it reports nothing, which is
/// why a view is told `Rebuilt` after a rebase.
pub struct Overlay<'a> {
    base: &'a dyn Store,
    writes: BTreeMap<TableName, BTreeMap<Key, Option<Row>>>,
}

impl<'a> Overlay<'a> {
    pub fn new(base: &'a dyn Store) -> Overlay<'a> {
        Overlay {
            base,
            writes: BTreeMap::new(),
        }
    }
}

impl Store for Overlay<'_> {
    fn schema(&self) -> &Schema {
        self.base.schema()
    }

    fn get(&self, table: &str, key: &[Value]) -> Option<Row> {
        match self.writes.get(table).and_then(|t| t.get(key)) {
            Some(w) => w.clone(),
            None => self.base.get(table, key),
        }
    }

    fn scan(&self, table: &str) -> Vec<Row> {
        let Some(tbl) = self.schema().lookup_table(table) else {
            return vec![];
        };
        let mut merged: BTreeMap<Key, Row> = self.base.scan(table).into_iter().map(|r| (tbl.key_of(&r), r)).collect();
        if let Some(ws) = self.writes.get(table) {
            for (k, w) in ws {
                match w {
                    Some(r) => {
                        merged.insert(k.clone(), r.clone());
                    }
                    None => {
                        merged.remove(k);
                    }
                }
            }
        }
        merged.into_values().collect()
    }

    fn apply_change(&mut self, change: &Change) {
        let (t, row, present) = match change {
            Change::Add(t, row) | Change::Edit(t, _, row) => (t, row, true),
            Change::Remove(t, row) => (t, row, false),
        };
        if let Some(tbl) = self.schema().lookup_table(t) {
            let k = tbl.key_of(row);
            self.writes
                .entry(t.clone())
                .or_default()
                .insert(k, if present { Some(row.clone()) } else { None });
        }
    }

    fn as_store(&self) -> &dyn Store {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Column, Scope};

    fn schema() -> Schema {
        let col = |n: &str, ty: Ty, nullable: bool| Column {
            name: n.into(),
            ty,
            nullable,
        };
        Schema {
            scopes: vec![Scope {
                name: "s".into(),
                tables: vec![
                    Table {
                        name: "p".into(),
                        columns: vec![col("id", Ty::Int, false), col("name", Ty::Text, true)],
                        key: vec!["id".into()],
                        indexes: vec![Index {
                            columns: vec!["name".into()],
                            unique: true,
                        }],
                        refs: vec![],
                    },
                    Table {
                        name: "c".into(),
                        columns: vec![col("id", Ty::Int, false), col("p_id", Ty::Int, true)],
                        key: vec!["id".into()],
                        indexes: vec![],
                        refs: vec![Ref {
                            column: "p_id".into(),
                            table: "p".into(),
                        }],
                    },
                ],
            }],
        }
    }

    fn row(pairs: Vec<(&str, Value)>) -> Row {
        pairs.into_iter().map(|(k, v)| (k.into(), v)).collect()
    }

    #[test]
    fn put_completes_judges_and_reports() {
        let mut st = MemoryStore::empty(schema());
        // An omitted nullable column is filled; the stored row is full.
        let ch = st.put("p", row(vec![("id", Value::int(1))])).unwrap();
        assert_eq!(ch, Some(Change::Add("p".into(), row(vec![("id", Value::int(1)), ("name", Value::Null)]))));
        // The same row again is nothing.
        assert_eq!(st.put("p", row(vec![("id", Value::int(1)), ("name", Value::Null)])).unwrap(), None);
        // Two NULLs in a unique column do not clash; two names do.
        assert!(st.put("p", row(vec![("id", Value::int(2))])).is_ok());
        st.put("p", row(vec![("id", Value::int(3)), ("name", Value::text("x"))])).unwrap();
        assert_eq!(
            st.put("p", row(vec![("id", Value::int(4)), ("name", Value::text("x"))])),
            Err(Refusal::UniqueViolation("p".into(), vec!["name".into()]))
        );
        // Editing 3 to keep its own name is not a clash with itself.
        assert!(matches!(
            st.put("p", row(vec![("id", Value::int(3)), ("name", Value::text("x"))])),
            Ok(None)
        ));
        // A missing non-nullable column, an unknown column, a wrong type.
        assert!(matches!(
            st.put("p", row(vec![("name", Value::text("y"))])),
            Err(Refusal::MalformedRow(_, _))
        ));
        assert!(matches!(
            st.put("p", row(vec![("id", Value::int(9)), ("zz", Value::int(1))])),
            Err(Refusal::MalformedRow(_, _))
        ));
        assert!(matches!(
            st.put("p", row(vec![("id", Value::text("no"))])),
            Err(Refusal::MalformedRow(_, _))
        ));
        assert_eq!(
            st.put("p", row(vec![("id", Value::Null)])),
            Err(Refusal::NotNull("p".into(), "id".into()))
        );
        // References.
        assert_eq!(
            st.put("c", row(vec![("id", Value::int(1)), ("p_id", Value::int(99))])),
            Err(Refusal::MissingParent("c".into(), "p_id".into(), "p".into()))
        );
        st.put("c", row(vec![("id", Value::int(1)), ("p_id", Value::int(1))])).unwrap();
        st.put("c", row(vec![("id", Value::int(2))])).unwrap();
        assert_eq!(st.delete("p", &[Value::int(1)]), Err(Refusal::StillReferenced("p".into(), "c".into())));
        assert_eq!(st.delete("p", &[Value::int(77)]), Ok(None));
        assert!(matches!(st.delete("p", &[Value::int(2)]), Ok(Some(Change::Remove(_, _)))));
        assert_eq!(st.put("nope", row(vec![])), Err(Refusal::NoSuchTable("nope".into())));
    }

    #[test]
    fn an_overlay_reads_through_and_commits_by_changes() {
        let mut base = MemoryStore::empty(schema());
        base.put("p", row(vec![("id", Value::int(1))])).unwrap();
        let changes = {
            let mut ov = Overlay::new(&base);
            let mut chs = Vec::new();
            chs.extend(ov.put("p", row(vec![("id", Value::int(0))])).unwrap());
            chs.extend(ov.delete("p", &[Value::int(1)]).unwrap());
            assert_eq!(ov.scan("p").len(), 1);
            assert!(ov.get("p", &[Value::int(1)]).is_none());
            chs
        };
        assert_eq!(base.scan("p").len(), 1);
        base.apply_changes(&changes);
        assert_eq!(base.scan("p"), vec![row(vec![("id", Value::int(0)), ("name", Value::Null)])]);
    }
}
