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

use crate::ir::{Expr, Pred};
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
    /// `docs/plan-auth.md` Who may see a row of this table: a predicate
    /// over its own columns, the identity's user (`Me`, which is
    /// [`Expr::CtxUser`] here) and roles ([`Pred::Role`]), and the one
    /// lookup — some row of another table referencing this one, admitted
    /// by its own predicate ([`Pred::Exists`]). `None` is `Everyone`, and
    /// is not encoded: a table that declares no rule is the bytes it
    /// always was, so a module with none hashes as it did. A peer to which
    /// every table is `Everyone` is served the log whole, by intents; any
    /// other is served the facts its rules admit (`protocol.rs`).
    pub visible: Option<Pred>,
    /// Who may write a row of this table, the same kind of predicate: an
    /// entry every one of whose facts' rows — old and new, for an edit —
    /// it admits for the author, or a refusal (`Refusal::Forbidden`),
    /// checked at the authority after the run and on the device before an
    /// intent is recorded pending. `None` is `Everyone`.
    pub writable: Option<Pred>,
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
            && self.visible == other.visible
            && self.writable == other.writable
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
            .field("visible", &self.visible)
            .field("writable", &self.writable)
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
            visible: None,
            writable: None,
            rows,
            key_at,
        }
    }

    /// `docs/plan-auth.md` The table with these rules: who may see a row,
    /// and who may write one; `None` for `Everyone`.
    pub fn with_rules(mut self, visible: Option<Pred>, writable: Option<Pred>) -> Table {
        self.visible = visible;
        self.writable = writable;
        self
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
    /// `docs/plan-auth.md` A rule names a column its table does not have
    /// (the table, then the table the column was looked for in, which is a
    /// lookup's own for the predicate inside one), and the column.
    RuleUnknownColumn(TableName, TableName, FieldName),
    /// A rule compares a column with something other than a literal of
    /// the column's type or `Me` on a text column: a rule is asked of a
    /// row and an identity, and has nothing else to read.
    RuleBadValue(TableName, FieldName),
    /// A role named by the empty text.
    RuleEmptyRole(TableName),
    /// A lookup through a table and column that is not a reference of that
    /// table to this one: table, the looked-up table, the column.
    RuleNotAReference(TableName, TableName, FieldName),
    /// A lookup inside a lookup: a rule reaches one table away, so that a
    /// change to a row moves the visibility of the rows it references and
    /// no others.
    RuleNested(TableName),
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
    for rule in [&t.visible, &t.writable].into_iter().flatten() {
        rule_ok(sch, t, t, rule, false, errs);
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

// `docs/plan-auth.md` A rule of `owner`, over the columns of `t` — the
// owner's own, or a looked-up table's inside a lookup (`inside`).
fn rule_ok(sch: &Schema, owner: &Table, t: &Table, p: &Pred, inside: bool, errs: &mut Vec<SchemaError>) {
    let col = |c: &str, errs: &mut Vec<SchemaError>| {
        let found = t.column(c);
        if found.is_none() {
            errs.push(SchemaError::RuleUnknownColumn(owner.name.clone(), t.name.clone(), c.into()));
        }
        found
    };
    let value = |c: &Column, e: &Expr, errs: &mut Vec<SchemaError>| {
        let ok = match e {
            Expr::Lit(v) => crate::store::of_type(&c.column_ty(), v),
            Expr::CtxUser => c.ty == Ty::Text,
            _ => false,
        };
        if !ok {
            errs.push(SchemaError::RuleBadValue(owner.name.clone(), c.name.clone()));
        }
    };
    match p {
        Pred::Cmp(c, _, e) | Pred::Has(c, e) => {
            if let Some(c) = col(c, errs) {
                if matches!(p, Pred::Has(..)) && c.ty != Ty::Text {
                    errs.push(SchemaError::RuleBadValue(owner.name.clone(), c.name.clone()));
                }
                value(c, e, errs);
            }
        }
        Pred::In(c, es) => {
            if let Some(c) = col(c, errs) {
                es.iter().for_each(|e| value(c, e, errs));
            }
        }
        Pred::All(ps) | Pred::Any(ps) => ps.iter().for_each(|q| rule_ok(sch, owner, t, q, inside, errs)),
        Pred::Not(q) => rule_ok(sch, owner, t, q, inside, errs),
        Pred::Role(r) => {
            if r.is_empty() {
                errs.push(SchemaError::RuleEmptyRole(owner.name.clone()));
            }
        }
        Pred::Exists(via, c, q) => {
            if inside {
                errs.push(SchemaError::RuleNested(owner.name.clone()));
                return;
            }
            let reached = sch
                .lookup_table(via)
                .filter(|v| v.name != t.name && v.refs.iter().any(|r| r.column == *c && r.table == t.name));
            match reached {
                Some(v) => rule_ok(sch, owner, v, q, true, errs),
                None => errs.push(SchemaError::RuleNotAReference(owner.name.clone(), via.clone(), c.clone())),
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

    // `docs/plan-auth.md` `owner(id, user_id, n?)` and `member(id, owner_id
    // → owner, user_id)`, with whatever rules a test gives `owner`.
    fn ruled(visible: Option<Pred>, writable: Option<Pred>) -> Schema {
        Schema {
            tables: vec![
                Table::new(
                    "owner",
                    vec![
                        col("id", Ty::Id("owner".into())),
                        col("user_id", Ty::Text),
                        Column {
                            name: "n".into(),
                            ty: Ty::Int,
                            nullable: true,
                        },
                    ],
                    vec!["id".into()],
                    vec![],
                    vec![],
                )
                .with_rules(visible, writable),
                Table::new(
                    "member",
                    vec![col("id", Ty::Int), col("owner_id", Ty::Id("owner".into())), col("user_id", Ty::Text)],
                    vec!["id".into()],
                    vec![],
                    vec![Ref {
                        column: "owner_id".into(),
                        table: "owner".into(),
                    }],
                ),
            ],
        }
    }

    /// `docs/plan-auth.md` A rule is checked as the schema is: every column
    /// it names is its table's — a lookup's predicate, the looked-up
    /// table's — a value is a literal of the column's type or `Me` on a
    /// text column, a role has a name, and a lookup goes through a
    /// reference of that table to this one, once. Falsified by accepting
    /// any value (`RuleBadValue` never pushed): the `Me`-on-an-int case
    /// came back clean.
    #[test]
    fn a_rule_names_only_what_its_table_has() {
        use crate::ir::{CmpOp, Expr};
        let me = |c: &str| Pred::Cmp(c.into(), CmpOp::Eq, Expr::CtxUser);
        let lit = |c: &str, v: Value| Pred::Cmp(c.into(), CmpOp::Eq, Expr::Lit(v));
        let through = |t: &str, c: &str, p: Pred| Pred::Exists(t.into(), c.into(), Box::new(p));
        let ok = ruled(
            Some(Pred::Any(vec![
                me("user_id"),
                through("member", "owner_id", me("user_id")),
                Pred::Role("admin".into()),
            ])),
            Some(Pred::All(vec![
                me("user_id"),
                lit("n", Value::Null),
                Pred::Not(Box::new(lit("n", Value::Int(3)))),
            ])),
        );
        assert_eq!(check_schema(&ok), vec![]);
        let errs = |v: Pred| check_schema(&ruled(Some(v), None));
        let o = || "owner".to_string();
        assert_eq!(errs(me("nope")), vec![SchemaError::RuleUnknownColumn(o(), o(), "nope".into())]);
        assert_eq!(errs(me("n")), vec![SchemaError::RuleBadValue(o(), "n".into())]);
        assert_eq!(
            errs(lit("user_id", Value::Int(1))),
            vec![SchemaError::RuleBadValue(o(), "user_id".into())]
        );
        assert_eq!(
            errs(Pred::Cmp("user_id".into(), CmpOp::Eq, Expr::Arg("x".into()))),
            vec![SchemaError::RuleBadValue(o(), "user_id".into())]
        );
        assert_eq!(errs(Pred::Role(String::new())), vec![SchemaError::RuleEmptyRole(o())]);
        assert_eq!(
            errs(through("member", "user_id", me("user_id"))),
            vec![SchemaError::RuleNotAReference(o(), "member".into(), "user_id".into())]
        );
        assert_eq!(
            errs(through("member", "owner_id", me("nope"))),
            vec![SchemaError::RuleUnknownColumn(o(), "member".into(), "nope".into())]
        );
        assert_eq!(
            errs(through("member", "owner_id", through("member", "owner_id", me("user_id")))),
            vec![SchemaError::RuleNested(o())]
        );
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
