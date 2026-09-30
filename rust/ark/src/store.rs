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
//!
//! A [`Row`] is positional (`docs/plan-perf.md` R11): its values in the
//! table's column order behind one shared allocation, and the names beside
//! them held once, on the [`Table`], for every row of it. A row carries a
//! reference to those names rather than the store supplying them at each
//! use, because a row is read far from any store — a view binding it, a
//! change crossing the wire, a test comparing two — and every one of those
//! would otherwise need the table threaded to it to say `row["title"]`.
//! Carrying them costs a reference count per copy and keeps the change in
//! this file and the few places that build a row. The references are
//! `Arc` rather than `Rc` because a row crosses threads: the hub's frames
//! carry facts to the socket tasks, and `HubHandle::rows` answers another
//! thread. Nothing observable moved: a row is still a `Struct` of every
//! column on the wire and in the state hash ([`Row::to_value`]), equal
//! when its columns are, and built from pairs by name. A row built from a
//! struct before any table laid it out — decoded without a schema, or
//! naming a column its table lacks — keeps its own names, in name order,
//! until a store lays it out as the table's ([`Store::apply_change`]) or
//! refuses it as today ([`Store::put`]).

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Bound;
use std::sync::Arc;

use crate::ir::Plan;
use crate::schema::{Dir, Index, Ref, Relation, Schema, Table, Ty};
use crate::value::{compare_value, FieldName, TableName, Value};

/// The names of a row's columns, in the order its values are held — a
/// table's, as it declares them, once for every row of it (R11) — and the
/// same positions sorted by name, which is how a name is found and the
/// order a struct's fields are in (§1.2).
pub struct Columns {
    names: Box<[FieldName]>,
    sorted: Box<[u32]>,
}

impl Columns {
    /// Names, each once, in the order the values will be held.
    pub fn new(names: Vec<FieldName>) -> Columns {
        let mut sorted: Vec<u32> = (0..names.len() as u32).collect();
        sorted.sort_by(|a, b| names[*a as usize].cmp(&names[*b as usize]));
        Columns {
            names: names.into_boxed_slice(),
            sorted: sorted.into_boxed_slice(),
        }
    }

    /// The names, in the order the values are held.
    pub fn names(&self) -> &[FieldName] {
        &self.names
    }

    /// Where a name's value is held, if the row has the column.
    pub fn position(&self, name: &str) -> Option<usize> {
        self.sorted
            .binary_search_by(|i| self.names[*i as usize].as_str().cmp(name))
            .ok()
            .map(|at| self.sorted[at] as usize)
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

impl PartialEq for Columns {
    fn eq(&self, other: &Columns) -> bool {
        self.names == other.names
    }
}

impl Eq for Columns {}

impl fmt::Debug for Columns {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.names.iter()).finish()
    }
}

// The names of rows built from a struct that no table has laid out yet —
// a row decoded from the wire, from a vector, from a test's pairs — kept
// so that the rows of one shape share one set of names, as a table's rows
// share the table's. A page of facts is a few tables' rows; sixteen shapes
// is more than any schema here writes at once, and a miss costs only the
// names of one row.
thread_local! {
    static LOOSE: std::cell::RefCell<Vec<Arc<Columns>>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn loose_columns<'a>(names: impl ExactSizeIterator<Item = &'a FieldName> + Clone) -> Arc<Columns> {
    LOOSE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let n = names.len();
        if let Some(at) = cache
            .iter()
            .position(|c| c.len() == n && c.names.iter().zip(names.clone()).all(|(a, b)| a == b))
        {
            let hit = cache.remove(at);
            cache.push(hit.clone());
            return hit;
        }
        let made = Arc::new(Columns::new(names.cloned().collect()));
        if cache.len() == 16 {
            cache.remove(0);
        }
        cache.push(made.clone());
        made
    })
}

/// A row: its table's columns' values, in the table's column order, beside
/// the table's names (`docs/plan-perf.md` R11; the module docs say why the
/// row carries them). A stored row is never partial. Cloning one is two
/// reference counts, which is what a read hands out.
///
/// It keeps the map's interface — a column by name, the columns in order,
/// built from pairs, equal when every column is — so that what a row *is*
/// (§4: a `Struct` of every column) has not moved: [`Row::to_value`] is
/// that struct, and it is what crosses the wire and is hashed.
#[derive(Clone)]
pub struct Row {
    cols: Arc<Columns>,
    vals: Arc<[Value]>,
}

impl Row {
    /// The values of `cols`, in its order.
    ///
    /// # Panics
    ///
    /// When there are not as many values as columns.
    pub fn new(cols: Arc<Columns>, vals: impl IntoIterator<Item = Value>) -> Row {
        let vals: Arc<[Value]> = vals.into_iter().collect();
        assert_eq!(cols.len(), vals.len(), "a row has a value for every column");
        Row { cols, vals }
    }

    /// A row of `tbl` from its columns by name: laid out in the table's
    /// order, a column the pairs leave out `Null`. Pairs that name a column
    /// the table lacks, or leave out one that is not nullable, are kept as
    /// they are — a row of no table — for [`Store::put`] to refuse as
    /// malformed, as it refused the map they were.
    pub fn of(tbl: &Table, pairs: impl IntoIterator<Item = (FieldName, Value)>) -> Row {
        Row::from_struct_in(tbl, pairs.into_iter().collect())
    }

    /// [`Row::of`] from a struct's fields.
    pub fn from_struct_in(tbl: &Table, mut m: BTreeMap<FieldName, Value>) -> Row {
        let cols = tbl.row_columns();
        let fits = m.keys().all(|k| cols.position(k).is_some()) && tbl.columns.iter().all(|c| c.nullable || m.contains_key(&c.name));
        if !fits {
            return Row::from_struct(m);
        }
        let vals: Arc<[Value]> = cols.names.iter().map(|n| m.remove(n).unwrap_or(Value::Null)).collect();
        Row { cols: cols.clone(), vals }
    }

    /// A struct read back as a row of `tbl` — a snapshot's, a vector's: laid
    /// out as the table's when its fields are exactly the table's columns,
    /// and otherwise kept as it is, a row of no table, as a fact is applied
    /// raw (§4.5). Its values are copied once, into place.
    pub fn stored_in(tbl: &Table, m: &BTreeMap<FieldName, Value>) -> Row {
        let cols = tbl.row_columns();
        if m.len() == cols.len() && m.keys().all(|k| cols.position(k).is_some()) {
            let vals: Arc<[Value]> = cols.names.iter().map(|n| m[n].clone()).collect();
            return Row { cols: cols.clone(), vals };
        }
        Row::from_struct_ref(m)
    }

    /// A row of no table yet: a struct's fields as they are, in name order.
    /// What a row decoded without its schema is — from the wire, from a
    /// vector — until a store lays it out as its table's
    /// ([`Store::apply_change`], [`Store::put`]).
    pub fn from_struct(m: BTreeMap<FieldName, Value>) -> Row {
        let cols = loose_columns(m.keys());
        let vals: Vec<Value> = m.into_values().collect();
        Row { cols, vals: vals.into() }
    }

    /// [`Row::from_struct`] of a struct the caller keeps: its values copied,
    /// its names shared with every row of the same shape.
    pub fn from_struct_ref(m: &BTreeMap<FieldName, Value>) -> Row {
        let cols = loose_columns(m.keys());
        let vals: Vec<Value> = m.values().cloned().collect();
        Row { cols, vals: vals.into() }
    }

    /// A column's value, by name.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.cols.position(name).map(|i| &self.vals[i])
    }

    pub fn contains_key(&self, name: &str) -> bool {
        self.cols.position(name).is_some()
    }

    /// The columns and their values, in the row's column order — its
    /// table's declared order, once a store has laid it out.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&FieldName, &Value)> + '_ {
        self.cols.names.iter().zip(self.vals.iter())
    }

    /// The column names, in column order.
    pub fn keys(&self) -> impl ExactSizeIterator<Item = &FieldName> + '_ {
        self.cols.names.iter()
    }

    /// The value held at a position of [`Row::columns`].
    pub fn at(&self, i: usize) -> &Value {
        &self.vals[i]
    }

    /// The values, in column order.
    pub fn values(&self) -> impl ExactSizeIterator<Item = &Value> + '_ {
        self.vals.iter()
    }

    pub fn len(&self) -> usize {
        self.vals.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vals.is_empty()
    }

    /// The names this row's values are held under.
    pub fn columns(&self) -> &Arc<Columns> {
        &self.cols
    }

    /// Whether this row is laid out as `tbl`'s rows are: its names are the
    /// table's own, not merely equal to them.
    pub fn is_of(&self, tbl: &Table) -> bool {
        Arc::ptr_eq(&self.cols, tbl.row_columns())
    }

    /// The row as the struct it is (§4): every column by name.
    pub fn to_struct(&self) -> BTreeMap<FieldName, Value> {
        self.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    /// [`Row::to_struct`], as a [`Value::Struct`].
    pub fn to_value(&self) -> Value {
        Value::Struct(self.to_struct())
    }

    /// [`Row::to_value`] of a row nobody else holds, its values moved
    /// rather than copied; copied when the row is shared.
    pub fn into_value(mut self) -> Value {
        match Arc::get_mut(&mut self.vals) {
            Some(vals) => Value::Struct(
                self.cols
                    .names
                    .iter()
                    .cloned()
                    .zip(vals.iter_mut().map(|v| std::mem::replace(v, Value::Null)))
                    .collect(),
            ),
            None => self.to_value(),
        }
    }

    /// Set a column, as a map's `insert` does: its old value back — or
    /// `None`, and the row a row of no table, when it had no such column
    /// (which [`Store::put`] then refuses as malformed).
    pub fn insert(&mut self, name: FieldName, v: Value) -> Option<Value> {
        if let Some(i) = self.cols.position(&name) {
            return Some(self.set_at(i, v));
        }
        let mut m = self.to_struct();
        m.insert(name, v);
        *self = Row::from_struct(m);
        None
    }

    /// This row with one column's value replaced; the row as it was when
    /// it has no such column.
    pub fn with(mut self, name: &str, v: Value) -> Row {
        if let Some(i) = self.cols.position(name) {
            self.set_at(i, v);
        }
        self
    }

    // The values are copied first when another row shares them.
    fn set_at(&mut self, i: usize, v: Value) -> Value {
        if Arc::get_mut(&mut self.vals).is_none() {
            self.vals = self.vals.iter().cloned().collect();
        }
        let vals = Arc::get_mut(&mut self.vals).expect("the values are held once");
        std::mem::replace(&mut vals[i], v)
    }
}

/// Equal when every column is: the same names, each holding an equal
/// value, whatever order either holds them in.
impl PartialEq for Row {
    fn eq(&self, other: &Row) -> bool {
        if Arc::ptr_eq(&self.cols, &other.cols) || self.cols == other.cols {
            return self.vals == other.vals;
        }
        self.len() == other.len() && self.iter().all(|(k, v)| other.get(k) == Some(v))
    }
}

impl Eq for Row {}

/// As the map it was: the columns in name order.
impl fmt::Debug for Row {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(self.cols.sorted.iter().map(|i| (&self.cols.names[*i as usize], &self.vals[*i as usize])))
            .finish()
    }
}

impl std::ops::Index<&str> for Row {
    type Output = Value;

    fn index(&self, name: &str) -> &Value {
        self.get(name).unwrap_or_else(|| panic!("a row has no column {name:?}"))
    }
}

/// The row with no columns: a row of no table, to [`Row::insert`] into.
impl Default for Row {
    fn default() -> Row {
        Row::from_struct(BTreeMap::new())
    }
}

/// A row of no table yet, from a struct's fields ([`Row::from_struct`]).
impl From<BTreeMap<FieldName, Value>> for Row {
    fn from(m: BTreeMap<FieldName, Value>) -> Row {
        Row::from_struct(m)
    }
}

/// A row of no table yet, from its pairs ([`Row::from_struct`]).
impl<const N: usize> From<[(FieldName, Value); N]> for Row {
    fn from(pairs: [(FieldName, Value); N]) -> Row {
        pairs.into_iter().collect()
    }
}

/// A row of no table yet, from its pairs ([`Row::from_struct`]).
impl FromIterator<(FieldName, Value)> for Row {
    fn from_iter<I: IntoIterator<Item = (FieldName, Value)>>(pairs: I) -> Row {
        Row::from_struct(pairs.into_iter().collect())
    }
}

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

/// One column a read's filter holds between two bounds — `name >= "a" and
/// name < "b"`, `pos > 5` — handed to the store beside the equalities
/// (`docs/plan-perf.md` R6). A hint, as the equalities are: `keep` still
/// decides, so a store may ignore it, and one that has an index whose
/// columns after the held ones begin with this column reads the part of
/// that index between the bounds instead of the whole of it. The bounds
/// are under `compare_value`, the order an index's map is in and the one
/// `Pred::Cmp` compares by, so `Null` is below every bound and a range
/// with no lower bound starts at it, exactly as the filter admits it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span<'a> {
    pub column: &'a str,
    pub lo: Bound<&'a Value>,
    pub hi: Bound<&'a Value>,
}

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

    /// The rows of a table that `keep` admits, in key order: what a select
    /// with a filter reads. The default scans and then drops; a store that
    /// can look before it copies copies only what it keeps, which is what
    /// makes a select cost its answer rather than its table.
    fn scan_where(&self, table: &str, keep: &dyn Fn(&Row) -> bool) -> Vec<Row> {
        self.scan(table).into_iter().filter(|r| keep(r)).collect()
    }

    /// [`Store::scan_where`], told which columns the filter holds equal to
    /// which values and which it holds between bounds (`keep` still
    /// decides; `eq` and `spans` are hints). A store with an index over
    /// those columns — or over columns that begin with some of them, or
    /// with some of them and then a column a span bounds (R6) — reads the
    /// rows under the values, between the bounds, and never looks at the
    /// rest of the table, and when they hold the whole key, reads the one
    /// row under it; the default ignores the hints. In key order either
    /// way.
    fn scan_where_eq(&self, table: &str, eq: &[(&str, &Value)], spans: &[Span], keep: &dyn Fn(&Row) -> bool) -> Vec<Row> {
        let _ = (eq, spans);
        self.scan_where(table, keep)
    }

    /// `docs/plan-perf.md` R1: the first `limit` rows `keep` admits, in
    /// `order` — each column under its direction, then the key ascending,
    /// [`compare_rows`]'s order — read from an index that already holds
    /// them in that order, when one does: `Some` then, `None` when no
    /// index serves and the caller reads and sorts as before. `eq` says
    /// which columns `keep` holds equal to which values and `spans` which
    /// it holds between bounds, as for [`Store::scan_where_eq`]: a span on
    /// the column right after the held ones narrows the walk to its range
    /// (R6). `keep` still decides.
    ///
    /// This is what makes `MAX(pos) + 1` — `order_by(pos desc).first()`
    /// over one playlist — cost the rows up to the first `keep` admits
    /// rather than every row of the playlist: a bounded read that fetched
    /// everything is the round-2 growth R1 names. The default serves
    /// nothing, which is always correct.
    fn scan_ordered(
        &self,
        table: &str,
        eq: &[(&str, &Value)],
        spans: &[Span],
        order: &[(&str, Dir)],
        keep: &dyn Fn(&Row) -> bool,
        limit: usize,
    ) -> Option<Vec<Row>> {
        let _ = (table, eq, spans, order, keep, limit);
        None
    }

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

    /// The row under a key as a value, or `Null`.
    fn get_value(&self, table: &str, key: &[Value]) -> Value {
        self.get(table, key).map(Row::into_value).unwrap_or(Value::Null)
    }

    /// `db.select(plan)`: what [`crate::view::pull`] answers, as a list.
    /// The plan's right-hand sides must already be literals, as generated
    /// code builds them; anything else is a bug.
    fn select(&self, plan: &Plan) -> Value {
        let rows = crate::eval::select_plan(self.schema(), plan, self.as_store())
            .unwrap_or_else(|e| panic!("select: {e:?} (a bug: generated code evaluates its plans)"));
        Value::List(rows)
    }

    /// Every table's rows, as `store_before`/`store_after` in the vectors:
    /// a struct from table name to the list of rows, in schema order.
    fn store_value(&self) -> Value {
        Value::Struct(
            self.schema()
                .tables()
                .map(|t| (t.name.clone(), Value::List(self.scan(&t.name).into_iter().map(Row::into_value).collect())))
                .collect(),
        )
    }

    /// `self` as a trait object, for the shared rules.
    fn as_store(&self) -> &dyn Store;
}

// The rules -------------------------------------------------------------

/// Fill in every nullable column the row left out, as `Null`
/// (`Ark.Store.complete`) — which is laying it out as `tbl`'s row when it
/// names only the table's columns and leaves out only nullable ones. A
/// row laid out as the table's is complete already; one with the table's
/// names in another object is given the table's, which copies nothing.
/// Anything else stays a row of no table, completed by name, for
/// [`judge_put`] to refuse.
pub fn complete(tbl: &Table, row: Row) -> Row {
    if row.is_of(tbl) {
        return row;
    }
    let cols = tbl.row_columns();
    if *row.cols == **cols {
        return Row {
            cols: cols.clone(),
            vals: row.vals,
        };
    }
    let mut m = row.to_struct();
    for c in &tbl.columns {
        if c.nullable {
            m.entry(c.name.clone()).or_insert(Value::Null);
        }
    }
    Row::from_struct_in(tbl, m)
}

/// A row with a table's column names in any order, as that table's row;
/// anything else as it is. What a store holds a fact's row as: a fact is
/// applied raw (§4.5), so a row that is not the table's shape is kept as
/// it came rather than completed or refused.
fn stored(tbl: &Table, row: &Row) -> Row {
    let cols = tbl.row_columns();
    if Arc::ptr_eq(&row.cols, cols) || *row.cols == **cols {
        return Row {
            cols: cols.clone(),
            vals: row.vals.clone(),
        };
    }
    if row.len() == cols.len() && row.keys().all(|k| cols.position(k).is_some()) {
        return Row::new(cols.clone(), cols.names.iter().map(|n| row[n.as_str()].clone()));
    }
    row.clone()
}

/// What `put` would report and the row it would store, without writing.
pub fn judge_put(st: &dyn Store, tn: &str, row0: Row) -> Result<Option<Change>, Refusal> {
    let tbl = st.schema().lookup_table(tn).ok_or_else(|| Refusal::NoSuchTable(tn.into()))?;
    let row = complete(tbl, row0);
    well_typed(tbl, &row)?;
    let k = tbl.key_of(&row);
    for ix in tbl.indexes.iter().filter(|ix| ix.unique) {
        unique(st, tbl, &k, &row, ix)?;
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

/// A struct written to a table, as the table's row: what the evaluator and
/// a native procedure hand [`insert`], [`upsert`] and [`update`]. A table
/// the schema lacks gives a row of no table, which the write refuses as it
/// did.
pub fn row_for(st: &dyn Store, tn: &str, m: BTreeMap<FieldName, Value>) -> Row {
    match st.schema().lookup_table(tn) {
        Some(tbl) => Row::from_struct_in(tbl, m),
        None => Row::from_struct(m),
    }
}

/// §1.4 The row a write's column list matches, if any: the table's key
/// when the list is empty, otherwise every listed column equal and none of
/// them `Null` (a `Null` matches nothing, as a unique index has it).
pub fn matching(st: &dyn Store, tbl: &Table, row: &Row, on: &[FieldName]) -> Option<Row> {
    if on.is_empty() {
        return st.get(&tbl.name, &tbl.key_of(row));
    }
    let want: Vec<(&str, &Value)> = on.iter().map(|c| (c.as_str(), row.get(c).unwrap_or(&Value::Null))).collect();
    if want.iter().any(|(_, v)| v.is_null()) {
        return None;
    }
    st.scan_where_eq(&tbl.name, &want, &[], &|r| want.iter().all(|(c, v)| r.get(c) == Some(*v)))
        .into_iter()
        .next()
}

/// §1.4 `SInsert`: write the row unless one matches on the columns (the
/// key when the list is empty). A match is no change and no refusal.
pub fn insert(st: &mut dyn Store, tn: &str, row0: Row, on: &[FieldName]) -> Result<Option<Change>, Refusal> {
    // The table is borrowed from the store's schema until the write, never
    // copied (`docs/plan-perf.md` R5); so in `upsert` and `update`.
    let tbl = st.schema().lookup_table(tn).ok_or_else(|| Refusal::NoSuchTable(tn.into()))?;
    let row = complete(tbl, row0);
    if matching(st.as_store(), tbl, &row, on).is_some() {
        return Ok(None);
    }
    st.put(tn, row)
}

/// §1.4 `SUpsert`: write the row; if one matches on the columns, keep its
/// key columns and take the rest from the new row. `upsert(t, row, [])` is
/// exactly `put(t, row)`.
pub fn upsert(st: &mut dyn Store, tn: &str, row0: Row, on: &[FieldName]) -> Result<Option<Change>, Refusal> {
    let tbl = st.schema().lookup_table(tn).ok_or_else(|| Refusal::NoSuchTable(tn.into()))?;
    let mut row = complete(tbl, row0);
    if let Some(old) = matching(st.as_store(), tbl, &row, on) {
        for k in &tbl.key {
            if let Some(v) = old.get(k) {
                row = row.with(k, v.clone());
            }
        }
    }
    st.put(tn, row)
}

/// §1.4 `SUpdate`, once the new row has been computed from the old one: the
/// row under `key` replaced by `row`. The replacement must keep the key;
/// one that moves it is refused as a malformed row.
pub fn update(st: &mut dyn Store, tn: &str, key: &[Value], row0: Row) -> Result<Option<Change>, Refusal> {
    let tbl = st.schema().lookup_table(tn).ok_or_else(|| Refusal::NoSuchTable(tn.into()))?;
    let row = complete(tbl, row0);
    if tbl.key_of(&row) != key {
        return Err(Refusal::MalformedRow(tn.into(), "update changed the key".into()));
    }
    st.put(tn, row)
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
// column's type (`Null` only where nullable). A row laid out as the
// table's has exactly its columns by construction ([`complete`]), so the
// names are compared only for one that is not, and the types then in
// column order, position by position (R11).
fn well_typed(tbl: &Table, row: &Row) -> Result<(), Refusal> {
    if !row.is_of(tbl) {
        let mut have: Vec<&str> = row.keys().map(|k| k.as_str()).collect();
        have.sort_unstable();
        let want: Vec<&str> = tbl.columns.iter().map(|c| c.name.as_str()).collect();
        return Err(Refusal::MalformedRow(tbl.name.clone(), format!("columns {have:?} are not {want:?}")));
    }
    for (c, v) in tbl.columns.iter().zip(row.values()) {
        match v {
            Value::Null => {
                if !c.nullable {
                    return Err(Refusal::NotNull(tbl.name.clone(), c.name.clone()));
                }
            }
            v => {
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

/// The order a read with an order answers in (§1.5): each column under its
/// direction, then the key ascending, column by column — what
/// [`crate::view::read`] sorts a bare plan's rows by and what
/// [`Store::scan_ordered`] must hand them back in. A column a row lacks
/// reads as `Null`.
pub fn compare_rows(tbl: &Table, order: &[(&str, Dir)], a: &Row, b: &Row) -> Ordering {
    fn col<'r>(r: &'r Row, c: &str) -> &'r Value {
        r.get(c).unwrap_or(&Value::Null)
    }
    for (c, d) in order {
        let o = compare_value(col(a, c), col(b, c));
        let o = if *d == Dir::Desc { o.reverse() } else { o };
        if o != Ordering::Equal {
            return o;
        }
    }
    tbl.key
        .iter()
        .map(|k| compare_value(col(a, k), col(b, k)))
        .find(|o| *o != Ordering::Equal)
        .unwrap_or(Ordering::Equal)
}

// No other row (one under another key) holds the index's columns equal
// to this row's. Read through the index: the rows under these values, and
// nothing else in the table.
fn unique(st: &dyn Store, tbl: &Table, k: &Key, row: &Row, ix: &Index) -> Result<(), Refusal> {
    let mine: Vec<(&str, &Value)> = ix.columns.iter().map(|c| (c.as_str(), row.get(c).unwrap_or(&Value::Null))).collect();
    // A NULL is not equal to anything, itself included, so two rows that
    // are both NULL in a unique column do not clash.
    if mine.iter().any(|(_, v)| v.is_null()) {
        return Ok(());
    }
    let clash = st.scan_where_eq(&tbl.name, &mine, &[], &|r| {
        tbl.key_of(r) != *k && mine.iter().all(|(c, v)| r.get(c).is_some_and(|x| x == *v))
    });
    if clash.is_empty() {
        Ok(())
    } else {
        Err(Refusal::UniqueViolation(tbl.name.clone(), ix.columns.clone()))
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
        let held = st.scan_where_eq(&rel.child, &[(&rel.column, kv)], &[], &|r| r.get(&rel.column) == Some(kv));
        if !held.is_empty() {
            return Err(Refusal::StillReferenced(tbl.name.clone(), rel.child.clone()));
        }
    }
    Ok(())
}

// The in-memory store -----------------------------------------------------

/// The spec's store: the rows of every table, each keyed by its key
/// (`Ark.Store.Store`) — and, beside them, an index per declared index and
/// per reference column, so that a select holding one of those columns to
/// a value reads the rows under it rather than the table. The indexes are
/// derived from the rows and say nothing the rows do not: two stores are
/// equal when their rows are.
#[derive(Debug)]
pub struct MemoryStore {
    schema: Schema,
    tables: BTreeMap<TableName, BTreeMap<Key, Row>>,
    indexes: BTreeMap<TableName, Vec<Secondary>>,
}

/// A copy of every row and every index. Written out rather than derived so
/// that this crate's tests can count them: a copy is the one cost of a
/// store that grows with it whatever changed, and the replica is held to
/// making none per mutation (§11.9, `peer::tests`).
impl Clone for MemoryStore {
    fn clone(&self) -> MemoryStore {
        #[cfg(test)]
        CLONES.with(|n| n.set(n.get() + 1));
        MemoryStore {
            schema: self.schema.clone(),
            tables: self.tables.clone(),
            indexes: self.indexes.clone(),
        }
    }
}

// Per thread, so that tests running beside each other do not count each
// other's copies.
#[cfg(test)]
thread_local! {
    static CLONES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many stores this thread has copied: tests only.
#[cfg(test)]
pub(crate) fn clones() -> usize {
    CLONES.with(|n| n.get())
}

impl PartialEq for MemoryStore {
    fn eq(&self, other: &MemoryStore) -> bool {
        self.schema == other.schema && self.tables == other.tables
    }
}

impl Eq for MemoryStore {}

/// One secondary index: the keys of the rows under each value of its
/// columns, in key order.
#[derive(Clone, Debug)]
struct Secondary {
    columns: Vec<FieldName>,
    rows: BTreeMap<Vec<Value>, BTreeSet<Key>>,
}

impl Secondary {
    fn key_of(&self, row: &Row) -> Vec<Value> {
        self.columns.iter().map(|c| row.get(c).cloned().unwrap_or(Value::Null)).collect()
    }

    /// The buckets whose values begin with `prefix`, in the index's order,
    /// and — given a span, which is on the column right after the prefix
    /// (R6) — whose next value lies between the span's bounds: a range of
    /// the map. A column holds a scalar or `Null`, and a struct outranks
    /// both (`Ark.Value.rank`), so `prefix ++ [v, struct]` is above every
    /// bucket that continues `prefix ++ [v]` and below every one past it:
    /// an inclusive upper bound `v` ends there, an exclusive lower bound
    /// `v` starts there, and with no span the range is the prefix up to
    /// `prefix ++ [struct]`. Nothing outside is walked. Bounds that cross
    /// (`x > 5 and x < 3`) are the empty range, which `BTreeMap::range`
    /// would otherwise panic on.
    fn under(&self, prefix: &[Value], span: Option<&Span>) -> std::collections::btree_map::Range<'_, Vec<Value>, BTreeSet<Key>> {
        let at = |v: Option<&Value>, top: bool| -> Vec<Value> {
            let mut k = prefix.to_vec();
            k.extend(v.cloned());
            if top {
                k.push(Value::Struct(BTreeMap::new()));
            }
            k
        };
        let (lo, hi) = span.map_or((Bound::Unbounded, Bound::Unbounded), |s| (s.lo, s.hi));
        let lo = match lo {
            Bound::Unbounded => Bound::Included(at(None, false)),
            Bound::Included(v) => Bound::Included(at(Some(v), false)),
            Bound::Excluded(v) => Bound::Excluded(at(Some(v), true)),
        };
        let hi = match hi {
            Bound::Unbounded => Bound::Excluded(at(None, true)),
            Bound::Included(v) => Bound::Excluded(at(Some(v), true)),
            Bound::Excluded(v) => Bound::Excluded(at(Some(v), false)),
        };
        let (Bound::Included(l) | Bound::Excluded(l), Bound::Excluded(h)) = (&lo, &hi) else {
            unreachable!("the bounds above are never unbounded and the upper never inclusive")
        };
        if l >= h {
            let l = l.clone();
            return self.rows.range((Bound::Included(l.clone()), Bound::Excluded(l)));
        }
        self.rows.range((lo, hi))
    }

    /// Whether this index holds a read's rows in the read's order (R1),
    /// and if so how many of its leading columns the read holds equal and
    /// which way to walk it. It does when its leading columns are exactly
    /// the columns `eq` holds, in any order; its remaining columns are the
    /// order's next ones, in sequence, all one direction (a column `eq`
    /// holds is one value and says nothing about order, so it is skipped
    /// wherever the order names it); and whatever the order says after
    /// those is the key's remaining columns ascending, in key order — the
    /// order a bucket's keys are already in, and the tie-break the
    /// verifier completes every order with (§9.4). Walking backwards visits
    /// the buckets in reverse and each bucket's keys still forwards, which
    /// is exactly "these columns descending, then the key ascending".
    fn serves(&self, tbl: &Table, eq: &[(&str, &Value)], order: &[(&str, Dir)]) -> Option<(usize, Dir)> {
        let is_eq = |c: &str| eq.iter().any(|(n, _)| *n == c);
        let n = self.columns.iter().take_while(|c| is_eq(c)).count();
        let (lead, rest) = self.columns.split_at(n);
        if !eq.iter().all(|(c, _)| lead.iter().any(|x| x == c)) {
            return None;
        }
        let mut ord = order.iter().filter(|(c, _)| !is_eq(c));
        let mut dir = None;
        for c in rest {
            let (o, d) = ord.next()?;
            if *o != c.as_str() || dir.is_some_and(|x| x != *d) {
                return None;
            }
            dir = Some(*d);
        }
        let mut tail = tbl.key.iter().filter(|k| !is_eq(k) && !rest.contains(k));
        for (o, d) in ord {
            if *d != Dir::Asc || tail.next().is_none_or(|k| k.as_str() != *o) {
                return None;
            }
        }
        Some((n, dir.unwrap_or(Dir::Asc)))
    }
}

/// The indexes a table gets: each it declares (unique or not) and each
/// reference column, one index per distinct column list.
fn secondaries(tbl: &Table) -> Vec<Secondary> {
    let mut seen: BTreeSet<Vec<FieldName>> = BTreeSet::new();
    tbl.indexes
        .iter()
        .map(|i| i.columns.clone())
        .chain(tbl.refs.iter().map(|r| vec![r.column.clone()]))
        .filter(|cols| !cols.is_empty() && *cols != tbl.key && seen.insert(cols.clone()))
        .map(|columns| Secondary {
            columns,
            rows: BTreeMap::new(),
        })
        .collect()
}

/// The values `eq` holds `columns` equal to, in the columns' order (the
/// first, where `eq` names a column twice: `keep` still decides).
fn held(columns: &[FieldName], eq: &[(&str, &Value)]) -> Vec<Value> {
    columns
        .iter()
        .map(|c| eq.iter().find(|(n, _)| n == c).map(|(_, v)| (*v).clone()).unwrap_or(Value::Null))
        .collect()
}

/// The span on the column after an index's first `n`, if the index has
/// one and a span bounds it.
fn bounding<'q>(columns: &[FieldName], n: usize, spans: &'q [Span<'q>]) -> Option<&'q Span<'q>> {
    let c = columns.get(n)?;
    spans.iter().find(|s| s.column == c.as_str())
}

/// The key `eq` names, when it holds every key column equal: then at most
/// one row answers, and it is read by key, through no index at all.
fn key_held(tbl: &Table, eq: &[(&str, &Value)]) -> Option<Key> {
    let all = !tbl.key.is_empty() && tbl.key.iter().all(|c| eq.iter().any(|(n, _)| n == c));
    all.then(|| held(&tbl.key, eq))
}

impl MemoryStore {
    /// `Ark.Store.empty`.
    pub fn empty(schema: Schema) -> MemoryStore {
        let indexes = schema.tables().map(|t| (t.name.clone(), secondaries(t))).collect();
        MemoryStore {
            schema,
            tables: BTreeMap::new(),
            indexes,
        }
    }

    /// Put `row` under `k` in `t` (or take the key out, for `None`), and
    /// keep every index of the table true to the rows: the one place the
    /// rows change.
    ///
    /// A table whose last row goes is taken out of `tables` altogether, as
    /// an emptied index posting is out of its index: the representation is
    /// the rows and nothing else, so a store that wrote a table's first row
    /// and then undid it — what a rebase's inverse does — is equal to one
    /// that never wrote it (`docs/plan-perf.md` R2).
    fn set(&mut self, t: &str, k: Key, row: Option<Row>) {
        let rows = self.tables.entry(t.into()).or_default();
        let old = match &row {
            Some(r) => rows.insert(k.clone(), r.clone()),
            None => rows.remove(&k),
        };
        if rows.is_empty() {
            self.tables.remove(t);
        }
        if let Some(ixs) = self.indexes.get_mut(t) {
            for ix in ixs {
                if let Some(o) = &old {
                    let ok = ix.key_of(o);
                    if let Some(ks) = ix.rows.get_mut(&ok) {
                        ks.remove(&k);
                        if ks.is_empty() {
                            ix.rows.remove(&ok);
                        }
                    }
                }
                if let Some(r) = &row {
                    ix.rows.entry(ix.key_of(r)).or_default().insert(k.clone());
                }
            }
        }
    }

    /// The index to read `eq` through, and the values to look up in it
    /// (in the index's column order): of the secondaries every column of
    /// which `eq` holds equal, the one with the fewest rows under those
    /// values. That is a probe of each candidate's map, and it is the whole
    /// of the choice: a toggle's child pull holds both `playlist_item`
    /// references equal, one to the playlist (whose items grow with the
    /// library) and one to the media (on a handful of playlists), and
    /// which is cheaper is a fact about the rows, not about which index the
    /// schema declared last.
    ///
    /// Failing that, an index whose *leading* columns `eq` holds serves
    /// too, as a range of its map rather than one bucket, and the values
    /// returned are then that prefix, shorter than the index. When a span
    /// bounds the column right after the prefix, the range is narrowed to
    /// it and the span is returned with it (R6): that column counts as one
    /// more held, so of the prefixes the longest wins, a bounded column
    /// adding one, the first declared among equals. `create_playlist`
    /// reads one person's playlists by `user_id` whose names lie between
    /// the name and its numbered siblings — the `(user_id, name)` index,
    /// its prefix and a range — rather than every playlist of theirs. A
    /// table with none of these answers `None`.
    fn lookup<'s, 'q>(&'s self, t: &str, eq: &[(&str, &Value)], spans: &'q [Span<'q>]) -> Option<(&'s Secondary, Vec<Value>, Option<&'q Span<'q>>)> {
        let ixs = self.indexes.get(t)?;
        let is_held = |c: &FieldName| eq.iter().any(|(n, _)| n == c);
        let whole = ixs
            .iter()
            .filter(|ix| ix.columns.iter().all(is_held))
            .map(|ix| (ix, held(&ix.columns, eq), None))
            .min_by_key(|(ix, vals, _)| ix.rows.get(vals).map_or(0, BTreeSet::len));
        if whole.is_some() {
            return whole;
        }
        ixs.iter()
            .map(|ix| {
                let n = ix.columns.iter().take_while(|c| is_held(c)).count();
                (ix, n, bounding(&ix.columns, n, spans))
            })
            .filter(|(_, n, span)| *n > 0 || span.is_some())
            .min_by_key(|(_, n, span)| std::cmp::Reverse(n + usize::from(span.is_some())))
            .map(|(ix, n, span)| (ix, held(&ix.columns[..n], eq), span))
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
            for (k, r) in rows {
                if !out.tables.get(t).is_some_and(|mine| mine.contains_key(k)) {
                    out.set(t, k.clone(), Some(r.clone()));
                }
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
                            let row = match st.schema.lookup_table(t) {
                                Some(tbl) => Row::stored_in(tbl, row),
                                None => Row::from_struct_ref(row),
                            };
                            st.apply_change(&Change::Add(t.clone(), row));
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

    // Asked by every write of a row with a reference, once per parent, and
    // by every `exists` check: answered without copying the row out
    // (`docs/plan-perf.md` R5).
    fn exists(&self, table: &str, key: &[Value]) -> bool {
        self.tables.get(table).is_some_and(|t| t.contains_key(key))
    }

    fn scan(&self, table: &str) -> Vec<Row> {
        self.tables.get(table).map(|t| t.values().cloned().collect()).unwrap_or_default()
    }

    fn scan_where(&self, table: &str, keep: &dyn Fn(&Row) -> bool) -> Vec<Row> {
        self.tables
            .get(table)
            .map(|t| t.values().filter(|r| keep(r)).cloned().collect())
            .unwrap_or_default()
    }

    fn scan_where_eq(&self, table: &str, eq: &[(&str, &Value)], spans: &[Span], keep: &dyn Fn(&Row) -> bool) -> Vec<Row> {
        if let Some(k) = self.schema.lookup_table(table).and_then(|tbl| key_held(tbl, eq)) {
            return self
                .tables
                .get(table)
                .and_then(|t| t.get(&k))
                .filter(|r| keep(r))
                .cloned()
                .into_iter()
                .collect();
        }
        let Some((ix, vals, span)) = self.lookup(table, eq, spans) else {
            return self.scan_where(table, keep);
        };
        let Some(rows) = self.tables.get(table) else {
            return vec![];
        };
        if vals.len() == ix.columns.len() {
            let Some(keys) = ix.rows.get(&vals) else {
                return vec![];
            };
            return keys.iter().filter_map(|k| rows.get(k)).filter(|r| keep(r)).cloned().collect();
        }
        // A prefix, and perhaps a range after it: the buckets under it come
        // in the index's order, not the key's, and a scan answers in key
        // order — so what is kept is put back in key order before a row is
        // copied.
        let mut hits: Vec<(&Key, &Row)> = ix
            .under(&vals, span)
            .flat_map(|(_, ks)| ks.iter())
            .filter_map(|k| rows.get(k).map(|r| (k, r)))
            .filter(|(_, r)| keep(r))
            .collect();
        hits.sort_by(|a, b| a.0.cmp(b.0));
        hits.into_iter().map(|(_, r)| r.clone()).collect()
    }

    /// R1: through the first index that [serves](Secondary::serves) the
    /// read, walking the range under the held values forwards or
    /// backwards and stopping at the `limit`-th row `keep` admits. The
    /// rows examined are those up to the last one returned, plus any
    /// `keep` refuses on the way — for `MAX(pos) + 1` over a playlist of
    /// eight thousand, one. A span on the column after the held ones
    /// narrows the walk to its range first (R6), so rows outside it are
    /// never examined; a span on any other column is left to `keep`.
    fn scan_ordered(
        &self,
        table: &str,
        eq: &[(&str, &Value)],
        spans: &[Span],
        order: &[(&str, Dir)],
        keep: &dyn Fn(&Row) -> bool,
        limit: usize,
    ) -> Option<Vec<Row>> {
        let tbl = self.schema.lookup_table(table)?;
        let (ix, n, dir) = self
            .indexes
            .get(table)?
            .iter()
            .find_map(|ix| ix.serves(tbl, eq, order).map(|(n, d)| (ix, n, d)))?;
        let mut out = Vec::new();
        let Some(rows) = self.tables.get(table) else {
            return Some(out);
        };
        if limit == 0 {
            return Some(out);
        }
        let prefix = held(&ix.columns[..n], eq);
        let span = bounding(&ix.columns, n, spans);
        // One bucket's keys, ascending whichever way the buckets are
        // walked; true once the answer is full.
        let mut take = |ks: &BTreeSet<Key>| {
            for r in ks.iter().filter_map(|k| rows.get(k)) {
                if keep(r) {
                    out.push(r.clone());
                    if out.len() == limit {
                        return true;
                    }
                }
            }
            false
        };
        match dir {
            Dir::Asc => {
                for (_, ks) in ix.under(&prefix, span) {
                    if take(ks) {
                        break;
                    }
                }
            }
            Dir::Desc => {
                for (_, ks) in ix.under(&prefix, span).rev() {
                    if take(ks) {
                        break;
                    }
                }
            }
        }
        Some(out)
    }

    fn apply_change(&mut self, change: &Change) {
        match change {
            Change::Add(t, row) | Change::Edit(t, _, row) => {
                if let Some(tbl) = self.schema.lookup_table(t) {
                    let k = tbl.key_of(row);
                    let row = stored(tbl, row);
                    self.set(t, k, Some(row));
                }
            }
            Change::Remove(t, row) => {
                if let Some(tbl) = self.schema.lookup_table(t) {
                    let k = tbl.key_of(row);
                    self.set(t, k, None);
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
/// commits the changes it produced. Dropping it reports nothing; what a
/// rebase tells a view is the transitions it made — the inverse of what it
/// undid, what landed, what it re-applied — from the changes each overlay
/// recorded (`docs/plan-perf.md` R2, `docs/arkdb.md` §3.13).
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

    fn exists(&self, table: &str, key: &[Value]) -> bool {
        match self.writes.get(table).and_then(|t| t.get(key)) {
            Some(w) => w.is_some(),
            None => self.base.exists(table, key),
        }
    }

    fn scan(&self, table: &str) -> Vec<Row> {
        let Some(tbl) = self.schema().lookup_table(table) else {
            return vec![];
        };
        // A table this overlay has not written is the base's, as it is.
        let Some(ws) = self.writes.get(table).filter(|ws| !ws.is_empty()) else {
            return self.base.scan(table);
        };
        let mut merged: BTreeMap<Key, Row> = self.base.scan(table).into_iter().map(|r| (tbl.key_of(&r), r)).collect();
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
        merged.into_values().collect()
    }

    fn scan_where(&self, table: &str, keep: &dyn Fn(&Row) -> bool) -> Vec<Row> {
        self.scan_where_eq(table, &[], &[], keep)
    }

    fn scan_where_eq(&self, table: &str, eq: &[(&str, &Value)], spans: &[Span], keep: &dyn Fn(&Row) -> bool) -> Vec<Row> {
        let Some(tbl) = self.schema().lookup_table(table) else {
            return vec![];
        };
        // Every key column held equal: one row, by key, from whichever of
        // the overlay and the base has it — what the merge below would
        // come to, without walking the overlay's writes.
        if let Some(k) = key_held(tbl, eq) {
            return self.get(table, &k).filter(|r| keep(r)).into_iter().collect();
        }
        let Some(ws) = self.writes.get(table).filter(|ws| !ws.is_empty()) else {
            return self.base.scan_where_eq(table, eq, spans, keep);
        };
        // The base's rows this overlay has not written, kept; then its own
        // writes, kept; in key order. A written row is judged by `keep`
        // alone, wherever a span would have put it (R6).
        let mut merged: BTreeMap<Key, Row> = self
            .base
            .scan_where_eq(table, eq, spans, &|r| !ws.contains_key(&tbl.key_of(r)) && keep(r))
            .into_iter()
            .map(|r| (tbl.key_of(&r), r))
            .collect();
        for (k, w) in ws {
            if let Some(r) = w {
                if keep(r) {
                    merged.insert(k.clone(), r.clone());
                }
            }
        }
        merged.into_values().collect()
    }

    /// R1: the base's ordered answer for the rows this overlay has not
    /// written, and its own writes that `keep` admits, merged — re-sorted
    /// under [`compare_rows`] and cut to `limit`. Exact rather than
    /// approximate: the base is asked to leave out every key written here
    /// (a row written here is either replaced or gone), so its first
    /// `limit` rows are the first `limit` of what the overlay has not
    /// touched, and the answer's first `limit` are among those and the
    /// writes. The cost is the writes to the table once per read, which is
    /// what `scan_where_eq` pays here too; `None` when the base has no
    /// index that serves. The spans go to the base with the equalities
    /// (R6); the writes are `keep`'s to judge, as they are for those.
    fn scan_ordered(
        &self,
        table: &str,
        eq: &[(&str, &Value)],
        spans: &[Span],
        order: &[(&str, Dir)],
        keep: &dyn Fn(&Row) -> bool,
        limit: usize,
    ) -> Option<Vec<Row>> {
        let tbl = self.schema().lookup_table(table)?;
        let Some(ws) = self.writes.get(table).filter(|ws| !ws.is_empty()) else {
            return self.base.scan_ordered(table, eq, spans, order, keep, limit);
        };
        let mut rows = self
            .base
            .scan_ordered(table, eq, spans, order, &|r| !ws.contains_key(&tbl.key_of(r)) && keep(r), limit)?;
        rows.extend(ws.values().flatten().filter(|r| keep(r)).cloned());
        rows.sort_by(|a, b| compare_rows(tbl, order, a, b));
        rows.truncate(limit);
        Some(rows)
    }

    fn apply_change(&mut self, change: &Change) {
        let (t, row, present) = match change {
            Change::Add(t, row) | Change::Edit(t, _, row) => (t, row, true),
            Change::Remove(t, row) => (t, row, false),
        };
        if let Some(tbl) = self.schema().lookup_table(t) {
            let k = tbl.key_of(row);
            let w = present.then(|| stored(tbl, row));
            self.writes.entry(t.clone()).or_default().insert(k, w);
        }
    }

    fn as_store(&self) -> &dyn Store {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Column;

    fn schema() -> Schema {
        let col = |n: &str, ty: Ty, nullable: bool| Column {
            name: n.into(),
            ty,
            nullable,
        };
        Schema {
            tables: vec![
                Table::new(
                    "p",
                    vec![col("id", Ty::Int, false), col("name", Ty::Text, true)],
                    vec!["id".into()],
                    vec![Index {
                        columns: vec!["name".into()],
                        unique: true,
                    }],
                    vec![],
                ),
                Table::new(
                    "c",
                    vec![col("id", Ty::Int, false), col("p_id", Ty::Int, true)],
                    vec!["id".into()],
                    vec![],
                    vec![Ref {
                        column: "p_id".into(),
                        table: "p".into(),
                    }],
                ),
                // Two references, as `playlist_item` has, and a key that
                // takes a third column: holding both references is two
                // indexes to choose between, holding all three is the key.
                Table::new(
                    "pc",
                    vec![col("p_id", Ty::Int, false), col("c_id", Ty::Int, false), col("n", Ty::Int, false)],
                    vec!["p_id".into(), "c_id".into(), "n".into()],
                    vec![],
                    vec![
                        Ref {
                            column: "p_id".into(),
                            table: "p".into(),
                        },
                        Ref {
                            column: "c_id".into(),
                            table: "c".into(),
                        },
                    ],
                ),
                // A playlist's items, as R1 reads them: keyed by the list
                // and a number, ordered by a position an index holds under
                // the list, and by the position alone.
                Table::new(
                    "it",
                    vec![col("p_id", Ty::Int, false), col("n", Ty::Int, false), col("pos", Ty::Int, false)],
                    vec!["p_id".into(), "n".into()],
                    vec![
                        Index {
                            columns: vec!["p_id".into(), "pos".into()],
                            unique: false,
                        },
                        Index {
                            columns: vec!["pos".into()],
                            unique: false,
                        },
                    ],
                    vec![],
                ),
            ],
        }
    }

    /// Three lists of ten items, positions a shuffle of 0..10 in each, and
    /// three more on list 1 sharing position 5 with item 5, so that the key
    /// has ties to break.
    fn items() -> MemoryStore {
        let mut st = MemoryStore::empty(schema());
        for p in 1..=3 {
            for n in 1..=10 {
                st.apply_change(&Change::Add("it".into(), it(p, n, n * 7 % 10)));
            }
        }
        for n in [13, 11, 12] {
            st.apply_change(&Change::Add("it".into(), it(1, n, 5)));
        }
        st
    }

    fn it(p: i64, n: i64, pos: i64) -> Row {
        row(vec![("p_id", Value::int(p)), ("n", Value::int(n)), ("pos", Value::int(pos))])
    }

    /// What an ordered read must answer, the slow way: every row, kept,
    /// sorted, cut.
    fn sorted(st: &dyn Store, keep: &dyn Fn(&Row) -> bool, order: &[(&str, Dir)], limit: usize) -> Vec<Row> {
        let tbl = st.schema().lookup_table("it").unwrap().clone();
        let mut rows: Vec<Row> = st.scan("it").into_iter().filter(|r| keep(r)).collect();
        rows.sort_by(|a, b| compare_rows(&tbl, order, a, b));
        rows.truncate(limit);
        rows
    }

    fn ns(rows: &[Row]) -> Vec<(i64, i64)> {
        rows.iter().map(|r| (r["p_id"].as_int(), r["n"].as_int())).collect()
    }

    /// R1: a read with an equality on the index's leading column and an
    /// order on the rest walks the index, forwards or backwards, and stops
    /// at the limit — the same rows, in the same order, as reading all and
    /// sorting, and only as many examined as it took to find them. Ties on
    /// the position fall to the key ascending in both directions. The
    /// counts are of `keep`'s calls. Falsified three ways: walking a
    /// bucket's keys backwards (`.rev()` on `ks.iter()`) puts item 13
    /// before 11 at position 5; dropping the `out.len() == limit` stop
    /// examines all thirteen of list 1 for a limit of four; and taking the
    /// range from the prefix to the end of the map (`Bound::Unbounded`)
    /// starts a descending walk at the map's end, examining lists 3 and 2
    /// (twenty-one rows) before the one it wanted.
    #[test]
    fn an_ordered_read_walks_the_index_and_stops_at_the_limit() {
        let st = items();
        let (one, two) = (Value::int(1), Value::int(2));
        let looked = std::cell::Cell::new(0);
        let on = |p: i64| {
            let looked = &looked;
            move |r: &Row| {
                looked.set(looked.get() + 1);
                r["p_id"] == Value::int(p)
            }
        };
        let asc = [("pos", Dir::Asc), ("p_id", Dir::Asc), ("n", Dir::Asc)];
        let desc = [("pos", Dir::Desc), ("p_id", Dir::Asc), ("n", Dir::Asc)];
        for (order, limit, examined) in [
            (&asc, 4, 4),
            (&desc, 1, 1),
            (&desc, 3, 3),
            (&asc, usize::MAX, 13),
            (&desc, usize::MAX, 13),
        ] {
            looked.set(0);
            let got = st
                .scan_ordered("it", &[("p_id", &one)], &[], order, &on(1), limit)
                .expect("an index serves");
            assert_eq!(looked.get(), examined, "{order:?} limit {limit}");
            assert_eq!(got, sorted(&st, &on(1), order, limit), "{order:?} limit {limit}");
        }
        // The ties at position 5, both ways: the key ascending.
        let at_5: Vec<(i64, i64)> = ns(&st.scan_ordered("it", &[("p_id", &one)], &[], &desc, &on(1), 8).unwrap())[4..].to_vec();
        assert_eq!(at_5, [(1, 5), (1, 11), (1, 12), (1, 13)]);
        let at_5: Vec<(i64, i64)> = ns(&st.scan_ordered("it", &[("p_id", &one)], &[], &asc, &on(1), 9).unwrap())[5..].to_vec();
        assert_eq!(at_5, [(1, 5), (1, 11), (1, 12), (1, 13)]);
        // Only list 2's rows are under its prefix.
        looked.set(0);
        let all_two = st.scan_ordered("it", &[("p_id", &two)], &[], &desc, &on(2), usize::MAX).unwrap();
        assert_eq!((all_two.len(), looked.get()), (10, 10));
        // `keep` refusing rows on the way: the walk goes on past them to
        // the limit, and counts them as examined. Even items of list 1,
        // descending: positions 9 (item 7) is odd, 8 (item 4) even, 7
        // (item 1) odd, 6 (item 8) even.
        looked.set(0);
        let even = |r: &Row| {
            looked.set(looked.get() + 1);
            r["p_id"] == Value::int(1) && r["n"].as_int() % 2 == 0
        };
        let got = st.scan_ordered("it", &[("p_id", &one)], &[], &desc, &even, 2).unwrap();
        assert_eq!(looked.get(), 4);
        assert_eq!(ns(&got), [(1, 4), (1, 8)]);
        assert_eq!(got, sorted(&st, &even, &desc, 2));
        // No equality at all: the index on the position alone, the whole
        // table in its order.
        let any = |_: &Row| true;
        for limit in [1, 5, usize::MAX] {
            assert_eq!(
                st.scan_ordered("it", &[], &[], &desc, &any, limit).unwrap(),
                sorted(&st, &any, &desc, limit)
            );
            assert_eq!(
                st.scan_ordered("it", &[], &[], &asc, &any, limit).unwrap(),
                sorted(&st, &any, &asc, limit)
            );
        }
        assert_eq!(st.scan_ordered("it", &[("p_id", &one)], &[], &desc, &any, 0), Some(vec![]));
    }

    /// No index holds the order, so no answer: the caller reads and sorts.
    /// An order on a column no index leads with; an equality the index
    /// does not begin with; the key descending after the position, which
    /// no bucket is in; the list and the position in two directions; and a
    /// table with no index at all. Falsified by `serves` not checking the
    /// tail's direction: the third answers in the wrong order.
    #[test]
    fn an_ordered_read_no_index_holds_is_none() {
        let st = items();
        let one = Value::int(1);
        let any = |_: &Row| true;
        type Read<'a> = (&'a [(&'a str, &'a Value)], &'a [(&'a str, Dir)]);
        let cases: [Read; 5] = [
            (&[], &[("n", Dir::Desc), ("p_id", Dir::Asc)]),
            (&[("n", &one)], &[("pos", Dir::Asc), ("p_id", Dir::Asc)]),
            (&[("p_id", &one)], &[("pos", Dir::Desc), ("p_id", Dir::Asc), ("n", Dir::Desc)]),
            (&[], &[("p_id", Dir::Asc), ("pos", Dir::Desc), ("n", Dir::Asc)]),
            (&[("p_id", &one), ("n", &one)], &[("pos", Dir::Asc)]),
        ];
        for (eq, order) in cases {
            assert_eq!(st.scan_ordered("it", eq, &[], order, &any, 3), None, "{eq:?} {order:?}");
        }
        assert_eq!(st.scan_ordered("c", &[], &[], &[("id", Dir::Desc)], &any, 1), None);
        assert_eq!(Overlay::new(&st).scan_ordered("c", &[], &[], &[("id", Dir::Desc)], &any, 1), None);
    }

    /// Through an overlay: a write that belongs inside the window is in
    /// it, one that takes a row out of the window (removed, or moved to a
    /// position below it) lets the next row in, a write to another list
    /// is not in it, and every answer is what reading the overlay whole and
    /// sorting says. Falsified by asking the base for its rows without
    /// leaving out the keys written here: the removed row comes back.
    #[test]
    fn an_overlay_merges_its_writes_into_an_ordered_read() {
        let st = items();
        let one = Value::int(1);
        let on_1 = |r: &Row| r["p_id"] == Value::int(1);
        let desc = [("pos", Dir::Desc), ("p_id", Dir::Asc), ("n", Dir::Asc)];
        let window = |ov: &Overlay| {
            let got = ov.scan_ordered("it", &[("p_id", &one)], &[], &desc, &on_1, 3).unwrap();
            assert_eq!(got, sorted(ov, &on_1, &desc, 3));
            ns(&got)
        };
        let mut ov = Overlay::new(&st);
        assert_eq!(window(&ov), [(1, 7), (1, 4), (1, 1)]);
        // A new last item: at the top.
        ov.apply_change(&Change::Add("it".into(), it(1, 50, 100)));
        assert_eq!(window(&ov), [(1, 50), (1, 7), (1, 4)]);
        // The item at 9 removed, and the one at 8 moved to the bottom.
        ov.apply_change(&Change::Remove("it".into(), it(1, 7, 9)));
        ov.apply_change(&Change::Edit("it".into(), it(1, 4, 8), it(1, 4, -1)));
        assert_eq!(window(&ov), [(1, 50), (1, 1), (1, 8)]);
        // Another list's write is not this list's.
        ov.apply_change(&Change::Add("it".into(), it(2, 60, 200)));
        assert_eq!(window(&ov), [(1, 50), (1, 1), (1, 8)]);
        // And the whole list, both ways.
        let asc = [("pos", Dir::Asc), ("p_id", Dir::Asc), ("n", Dir::Asc)];
        for order in [&asc, &desc] {
            let got = ov.scan_ordered("it", &[("p_id", &one)], &[], order, &on_1, usize::MAX).unwrap();
            assert_eq!(got, sorted(&ov, &on_1, order, usize::MAX));
            assert_eq!(got.len(), 13);
        }
    }

    /// An equality on an index's leading column alone reads the range
    /// under it — only that list's rows examined — and answers in key
    /// order, as a scan would, not in the index's. Falsified by leaving
    /// out the sort: list 1 comes back by position.
    #[test]
    fn an_equality_on_an_index_prefix_reads_its_range_in_key_order() {
        let st = items();
        let looked = std::cell::Cell::new(0);
        let on_1 = |r: &Row| {
            looked.set(looked.get() + 1);
            r["p_id"] == Value::int(1)
        };
        let got = st.scan_where_eq("it", &[("p_id", &Value::int(1))], &[], &on_1);
        assert_eq!(looked.get(), 13);
        let by_scan: Vec<Row> = st.scan("it").into_iter().filter(|r| r["p_id"] == Value::int(1)).collect();
        assert_eq!(got, by_scan);
    }

    // R6: a span on `pos`, and whether a row is inside it — what the
    // filter the span came from would say, under `compare_value`.
    fn pos_span<'a>(lo: Bound<&'a Value>, hi: Bound<&'a Value>) -> Span<'a> {
        Span { column: "pos", lo, hi }
    }

    fn inside(s: &Span, r: &Row) -> bool {
        let v = &r[s.column];
        let above = match s.lo {
            Bound::Unbounded => true,
            Bound::Included(b) => compare_value(v, b) != Ordering::Less,
            Bound::Excluded(b) => compare_value(v, b) == Ordering::Greater,
        };
        let below = match s.hi {
            Bound::Unbounded => true,
            Bound::Included(b) => compare_value(v, b) != Ordering::Greater,
            Bound::Excluded(b) => compare_value(v, b) == Ordering::Less,
        };
        above && below
    }

    /// R6: a span on the column after the held ones reads that part of the
    /// index and nothing else — both bounds, each bound alone, inclusive
    /// and exclusive, under an equality prefix and with none — answering
    /// what a scan and a filter answer, in key order, having examined only
    /// the rows inside. List 1's positions are 0..10 once each and 5 four
    /// times. Bounds that cross, or meet with one side exclusive, are an
    /// empty range, not a panic (`BTreeMap::range` panics on both). A span
    /// on a column the index does not put next — `n`, under `p_id` — is
    /// the prefix read, and a table with no index serving is the scan.
    /// Falsified three ways: ignoring the span in `lookup` (`bounding`
    /// answering `None`) examines all thirteen of list 1 for `[3, 6)`;
    /// ending an inclusive upper bound without the struct suffix
    /// (`at(Some(v), false)`) leaves out the bucket at the bound, one row
    /// examined for `<= 1` where there are two; and dropping the guard on
    /// crossed bounds panics on `> 5 and <= 5`.
    #[test]
    fn a_span_after_the_held_columns_reads_only_its_range() {
        let st = items();
        let (one, n) = (Value::int(1), |i: i64| Value::int(i));
        let (v0, v1, v3, v5, v6, v7, v9) = (n(0), n(1), n(3), n(5), n(6), n(7), n(9));
        use Bound::{Excluded as X, Included as I, Unbounded as U};
        type Case<'a> = (Vec<(&'a str, &'a Value)>, Span<'a>, usize);
        let cases: Vec<Case> = vec![
            (vec![("p_id", &one)], pos_span(I(&v3), X(&v6)), 6),
            (vec![("p_id", &one)], pos_span(X(&v7), U), 2),
            (vec![("p_id", &one)], pos_span(U, I(&v1)), 2),
            (vec![("p_id", &one)], pos_span(I(&v5), I(&v5)), 4),
            (vec![("p_id", &one)], pos_span(X(&v0), X(&v1)), 0),
            (vec![], pos_span(I(&v9), U), 3),
            (vec![], pos_span(X(&v3), I(&v5)), 9),
            (vec![("p_id", &one)], pos_span(X(&v5), X(&v3)), 0),
            (vec![("p_id", &one)], pos_span(I(&v5), X(&v5)), 0),
            (vec![("p_id", &one)], pos_span(X(&v5), I(&v5)), 0),
        ];
        for (eq, span, examined) in cases {
            let looked = std::cell::Cell::new(0);
            let keep = |r: &Row| {
                looked.set(looked.get() + 1);
                eq.iter().all(|(c, v)| &r[*c] == *v) && inside(&span, r)
            };
            let got = st.scan_where_eq("it", &eq, &[span], &keep);
            assert_eq!(looked.get(), examined, "{eq:?} {span:?}");
            let by_scan: Vec<Row> = st
                .scan("it")
                .into_iter()
                .filter(|r| eq.iter().all(|(c, v)| &r[*c] == *v) && inside(&span, r))
                .collect();
            assert_eq!(got, by_scan, "{eq:?} {span:?}");
        }
        // A span on `n`, which `(p_id, pos)` does not put after `p_id`:
        // the prefix read, every row of list 1 examined, the same answer.
        let looked = std::cell::Cell::new(0);
        let span = Span {
            column: "n",
            lo: I(&v3),
            hi: X(&v6),
        };
        let keep = |r: &Row| {
            looked.set(looked.get() + 1);
            r["p_id"] == one && inside(&span, r)
        };
        let got = st.scan_where_eq("it", &[("p_id", &one)], &[span], &keep);
        assert_eq!((looked.get(), ns(&got)), (13, vec![(1, 3), (1, 4), (1, 5)]));
        // No index over `c.id` but the key: the scan, whatever the span.
        let mut st = MemoryStore::empty(schema());
        for i in 1..=5 {
            st.put("c", row(vec![("id", Value::int(i)), ("p_id", Value::Null)])).unwrap();
        }
        let looked = std::cell::Cell::new(0);
        let span = Span {
            column: "id",
            lo: I(&v3),
            hi: U,
        };
        let got = st.scan_where_eq("c", &[], &[span], &|r| {
            looked.set(looked.get() + 1);
            compare_value(&r["id"], &v3) != Ordering::Less
        });
        assert_eq!((looked.get(), got.len()), (5, 3));
        assert_eq!(st.scan_ordered("c", &[], &[span], &[("id", Dir::Desc)], &|_| true, 1), None);
    }

    /// R6: an ordered read with a span on its order column walks the range
    /// only, either way, and stops at the limit: `[2, 8]` of list 1 by
    /// position is 8, 7, 6 descending and 2, 3, 4 ascending, three rows
    /// examined each. With no equality the `(pos)` index serves. Falsified
    /// by walking `under(&prefix, None)` in `scan_ordered`: descending
    /// examines 9 first, four rows for three.
    #[test]
    fn an_ordered_read_walks_only_its_span() {
        let st = items();
        let (one, v2, v8) = (Value::int(1), Value::int(2), Value::int(8));
        let span = pos_span(Bound::Included(&v2), Bound::Included(&v8));
        let asc = [("pos", Dir::Asc), ("p_id", Dir::Asc), ("n", Dir::Asc)];
        let desc = [("pos", Dir::Desc), ("p_id", Dir::Asc), ("n", Dir::Asc)];
        for (order, want) in [(&desc, [8, 7, 6]), (&asc, [2, 3, 4])] {
            let looked = std::cell::Cell::new(0);
            let keep = |r: &Row| {
                looked.set(looked.get() + 1);
                r["p_id"] == one && inside(&span, r)
            };
            let got = st.scan_ordered("it", &[("p_id", &one)], &[span], order, &keep, 3).unwrap();
            assert_eq!(got, sorted(&st, &keep, order, 3));
            let pos: Vec<i64> = got.iter().map(|r| r["pos"].as_int()).collect();
            assert_eq!(pos, want);
            // `sorted` asked `keep` about every row; the walk, three.
            assert_eq!(looked.get(), 3 + 33);
        }
        let any_list = |r: &Row| inside(&span, r);
        let got = st.scan_ordered("it", &[], &[span], &desc, &any_list, usize::MAX).unwrap();
        assert_eq!(got, sorted(&st, &any_list, &desc, usize::MAX));
        assert_eq!(got.len(), 7 * 3 + 3);
    }

    /// R6: an overlay hands the span to the base and judges its own writes
    /// by `keep` — a row written into the range, one moved into it from
    /// outside, one moved out, one removed inside, and one written outside
    /// — so a ranged read through it is the merged table filtered, in both
    /// the key-ordered and the position-ordered read. Falsified by passing
    /// the base's rows through without leaving out the written keys: the
    /// row moved out of the range comes back at its old position.
    #[test]
    fn an_overlay_merges_its_writes_into_a_ranged_read() {
        let st = items();
        let (one, v3, v6) = (Value::int(1), Value::int(3), Value::int(6));
        let span = pos_span(Bound::Included(&v3), Bound::Excluded(&v6));
        let keep = |r: &Row| r["p_id"] == one && inside(&span, r);
        let mut ov = Overlay::new(&st);
        ov.apply_change(&Change::Add("it".into(), it(1, 40, 4)));
        ov.apply_change(&Change::Edit("it".into(), it(1, 7, 9), it(1, 7, 3)));
        ov.apply_change(&Change::Edit("it".into(), it(1, 2, 4), it(1, 2, 8)));
        ov.apply_change(&Change::Remove("it".into(), it(1, 11, 5)));
        ov.apply_change(&Change::Add("it".into(), it(1, 41, 7)));
        let by_scan: Vec<Row> = ov.scan("it").into_iter().filter(|r| keep(r)).collect();
        let got = ov.scan_where_eq("it", &[("p_id", &one)], &[span], &keep);
        assert_eq!(got, by_scan);
        assert_eq!(ns(&got), [(1, 5), (1, 7), (1, 9), (1, 12), (1, 13), (1, 40)]);
        let desc = [("pos", Dir::Desc), ("p_id", Dir::Asc), ("n", Dir::Asc)];
        for limit in [1, 3, usize::MAX] {
            let got = ov.scan_ordered("it", &[("p_id", &one)], &[span], &desc, &keep, limit).unwrap();
            assert_eq!(got, sorted(&ov, &keep, &desc, limit), "{limit}");
        }
    }

    fn row(pairs: Vec<(&str, Value)>) -> Row {
        pairs.into_iter().map(|(k, v)| (k.into(), v)).collect()
    }

    /// A read holding an indexed column to a value (`p_id`, a reference)
    /// answers exactly what a scan and a filter would, through every way a
    /// row moves: added, edited onto another value, removed, applied raw,
    /// merged, cloned. Falsified by `set` not taking the old row out of its
    /// index: the edited row still answers under its old parent.
    #[test]
    fn an_indexed_read_is_the_scan_filtered() {
        let mut st = MemoryStore::empty(schema());
        for i in 1..=3 {
            st.put("p", row(vec![("id", Value::int(i))])).unwrap();
        }
        for i in 1..=9 {
            st.put("c", row(vec![("id", Value::int(i)), ("p_id", Value::int(i % 3 + 1))])).unwrap();
        }
        let same = |st: &MemoryStore, p: i64| {
            let v = Value::int(p);
            let by_index = st.scan_where_eq("c", &[("p_id", &v)], &[], &|_| true);
            let by_scan: Vec<Row> = st.scan("c").into_iter().filter(|r| r["p_id"] == v).collect();
            assert_eq!(by_index, by_scan, "p_id = {p}");
            by_index.len()
        };
        assert_eq!((same(&st, 1), same(&st, 2), same(&st, 3)), (3, 3, 3));
        // One edited onto another parent, one removed, one added raw.
        st.put("c", row(vec![("id", Value::int(1)), ("p_id", Value::int(3))])).unwrap();
        st.delete("c", &[Value::int(2)]).unwrap();
        st.apply_change(&Change::Add("c".into(), row(vec![("id", Value::int(10)), ("p_id", Value::int(1))])));
        assert_eq!((same(&st, 1), same(&st, 2), same(&st, 3)), (4, 2, 3));
        // A value nobody holds, and a column with no index (the scan).
        assert!(st.scan_where_eq("c", &[("p_id", &Value::int(9))], &[], &|_| true).is_empty());
        assert_eq!(
            st.scan_where_eq("c", &[("id", &Value::int(3))], &[], &|r| r["id"] == Value::int(3)).len(),
            1
        );
        // Merged and cloned stores keep their indexes.
        let other = {
            let mut o = MemoryStore::empty(schema());
            o.put("p", row(vec![("id", Value::int(1))])).unwrap();
            o.put("c", row(vec![("id", Value::int(11)), ("p_id", Value::int(1))])).unwrap();
            o
        };
        let merged = st.merge(&other);
        assert_eq!(same(&merged, 1), 5);
        assert_eq!(same(&merged.clone(), 3), 3);
        // A declared unique index answers too.
        st.put("p", row(vec![("id", Value::int(2)), ("name", Value::text("two"))])).unwrap();
        assert_eq!(st.scan_where_eq("p", &[("name", &Value::text("two"))], &[], &|_| true).len(), 1);
    }

    /// A read holding every key column answers by key, and one holding
    /// two indexed columns reads through the one with fewer rows under its
    /// value — whichever the schema declared first — in the store and
    /// through an overlay that has written the table. Counted by `keep`'s
    /// calls, which are the rows the store looked at. Falsified by keeping
    /// the widest index with its tie broken by position, either way: the
    /// last declared (`max_by_key`, what this store did) looks at the
    /// forty-one rows of `c_id = 9`, the first at the forty of `p_id = 1`.
    #[test]
    fn a_read_takes_the_key_or_the_smallest_index() {
        let mut st = MemoryStore::empty(schema());
        let pc = |p: i64, c: i64| row(vec![("p_id", Value::int(p)), ("c_id", Value::int(c)), ("n", Value::int(p * 100 + c))]);
        // Forty rows under p = 1; c = 7 is under p = 1 and p = 2 only.
        for c in 1..=40 {
            st.apply_change(&Change::Add("pc".into(), pc(1, c)));
        }
        st.apply_change(&Change::Add("pc".into(), pc(2, 7)));
        // And forty-one under c = 9, which p = 5 holds once.
        for p in 2..=41 {
            st.apply_change(&Change::Add("pc".into(), pc(p, 9)));
        }
        fn counted<'a>(looked: &'a std::cell::Cell<usize>, want: &'a dyn Fn(&Row) -> bool) -> impl Fn(&Row) -> bool + 'a {
            move |r| {
                looked.set(looked.get() + 1);
                want(r)
            }
        }
        let looked = std::cell::Cell::new(0);
        let count = |want: &'static dyn Fn(&Row) -> bool| counted(&looked, want);
        let (one, seven, n) = (Value::int(1), Value::int(7), Value::int(107));
        fn both(r: &Row) -> bool {
            r["p_id"] == Value::int(1) && r["c_id"] == Value::int(7)
        }
        let by_scan: Vec<Row> = st.scan("pc").into_iter().filter(both).collect();
        assert_eq!(by_scan.len(), 1);
        // The whole key, in any order: one row looked at.
        for eq in [
            [("p_id", &one), ("c_id", &seven), ("n", &n)],
            [("n", &n), ("c_id", &seven), ("p_id", &one)],
        ] {
            looked.set(0);
            assert_eq!(st.scan_where_eq("pc", &eq, &[], &count(&both)), by_scan);
            assert_eq!(looked.get(), 1);
        }
        // A key nobody holds, and a row `keep` refuses.
        assert!(st
            .scan_where_eq("pc", &[("p_id", &one), ("c_id", &seven), ("n", &one)], &[], &|_| true)
            .is_empty());
        assert!(st
            .scan_where_eq("pc", &[("p_id", &one), ("c_id", &seven), ("n", &n)], &[], &|_| false)
            .is_empty());
        // Both references held and not the key: the posting list of
        // `c_id = 7` (two rows), not of `p_id = 1` (forty), in either order.
        for eq in [[("p_id", &one), ("c_id", &seven)], [("c_id", &seven), ("p_id", &one)]] {
            looked.set(0);
            assert_eq!(st.scan_where_eq("pc", &eq, &[], &count(&both)), by_scan);
            assert_eq!(looked.get(), 2);
        }
        // …and the other way round: `p_id = 5` (one row), not `c_id = 9`
        // (forty-one), so no order of the schema's indexes serves both.
        let (five, nine) = (Value::int(5), Value::int(9));
        let five_nine = |r: &Row| r["p_id"] == Value::int(5) && r["c_id"] == Value::int(9);
        looked.set(0);
        assert_eq!(
            st.scan_where_eq("pc", &[("p_id", &five), ("c_id", &nine)], &[], &counted(&looked, &five_nine))
                .len(),
            1
        );
        assert_eq!(looked.get(), 1);
        // Through an overlay that has written the table: the same answers,
        // its own writes first.
        let mut ov = Overlay::new(&st);
        ov.apply_change(&Change::Remove("pc".into(), pc(1, 7)));
        ov.apply_change(&Change::Add("pc".into(), pc(3, 7)));
        let merged = |ov: &Overlay, want: &dyn Fn(&Row) -> bool| ov.scan("pc").into_iter().filter(|r| want(r)).collect::<Vec<_>>();
        assert!(ov.scan_where_eq("pc", &[("p_id", &one), ("c_id", &seven)], &[], &both).is_empty());
        let three = Value::int(3);
        let three_seven = |r: &Row| r["p_id"] == Value::int(3) && r["c_id"] == Value::int(7);
        assert_eq!(
            ov.scan_where_eq("pc", &[("c_id", &seven), ("p_id", &three)], &[], &three_seven),
            merged(&ov, &three_seven)
        );
        let seven_any = |r: &Row| r["c_id"] == Value::int(7);
        assert_eq!(ov.scan_where_eq("pc", &[("c_id", &seven)], &[], &seven_any), merged(&ov, &seven_any));
        assert_eq!(merged(&ov, &seven_any).len(), 2);
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

    /// A table whose rows were all removed is the table never written:
    /// two stores with the same rows are equal, whatever was written and
    /// undone on the way (R2's rebase undoes by inverse changes). Through
    /// `delete`, through a raw `Remove`, and with the index on the way
    /// emptied too. Falsified by leaving the emptied table in `tables`:
    /// the first comparison fails.
    #[test]
    fn a_table_emptied_is_a_table_never_written() {
        let empty = MemoryStore::empty(schema());
        let mut st = MemoryStore::empty(schema());
        st.put("p", row(vec![("id", Value::int(1)), ("name", Value::text("x"))])).unwrap();
        assert_ne!(st, empty);
        st.delete("p", &[Value::int(1)]).unwrap();
        assert_eq!(st, empty);
        assert!(st.is_empty());
        st.apply_change(&Change::Add("it".into(), it(1, 1, 1)));
        st.apply_change(&Change::Remove("it".into(), it(1, 1, 1)));
        assert_eq!(st, empty);
        let any = |_: &Row| true;
        assert_eq!(st.scan_ordered("it", &[], &[], &[("pos", Dir::Asc)], &any, 5), Some(vec![]));
        assert!(st.scan_where_eq("p", &[("name", &Value::text("x"))], &[], &any).is_empty());
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
