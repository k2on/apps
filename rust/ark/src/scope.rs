//! `docs/plan-guards.md` D2 What a person holds: the union of a module's
//! scopes, evaluated for one identity, and everything that asks it.
//!
//! A scope ([`crate::ir::FnKind::Scope`]) says, as a function of `ctx`
//! alone, which rows and which columns of each table it names a person
//! holds. A person holds the **union**, per table, of every scope on every
//! procedure of the module ([`Scopes::of`]):
//!
//! - **rows** are the disjunction of the scopes' filters with the context
//!   put in — the user, the login and the roles are known at `Hello`, so a
//!   `When` and every comparison of the context fold to a constant — and a
//!   scope whose filter folds to false contributes nothing;
//! - **columns** are the union of the contributing scopes' projections, the
//!   key always among them. Per table, not per row: a column one scope
//!   keeps is held on every row the person holds of the table, whichever
//!   scope admitted it — per-row column sets are not built;
//! - a table no scope names is held **whole**; one that every scope naming
//!   it folds to nothing is held **empty**, with every column, since no row
//!   is there to show one.
//!
//! [`Holdings::is_whole`] is the fast path's whole question: a person who
//! holds every table whole is served as a module with no scope always was,
//! by intents, byte for byte. Everything else here is for the other kind,
//! and sans-io, so the authority, the client, the verifier and the fuzzer
//! ask it one way:
//!
//! - [`Holdings::admits`] and [`Holdings::project`]: one row;
//! - [`Holdings::schema`]: the tables as the person's device has them —
//!   only the held columns, and only the indexes and references over them
//!   (a reference is kept only where its parent table is held whole, so a
//!   device never refuses a write for a parent it cannot see; the
//!   authority checks every reference, and a refused parent comes back as
//!   the ordinary verdict);
//! - [`Holdings::held_rows`] and [`Holdings::digest`]: what a person holds
//!   of a store, and its state hash (§8.1 over those rows, projected);
//! - [`Holdings::filter_facts`]: what a person is sent of one entry.
//!
//! [`check`] is the verifier's half: what a client-run part of a procedure
//! may name, for every role set the module names.

use std::collections::{BTreeMap, BTreeSet};

use crate::hash::{leaf, state_hash_of, Digest};
use crate::ir::{CmpOp, Expr, FnKind, Function, Hold, Module, Op, Pred, Projection, Stmt};
use crate::log::Facts;
use crate::schema::{Column, Index, Ref, Schema, Table};
use crate::store::{fold as fold_text, Change, Row, Store};
use crate::value::{FieldName, TableName, Value};
use crate::verify::{Complaint, VerifyError};
use crate::view::cmp;

/// Who the union is asked about: the user and the login the authenticator
/// named at `Hello`, and the roles it said they hold.
#[derive(Clone, Copy, Debug)]
pub struct Who<'a> {
    pub user: &'a str,
    pub session: &'a str,
    pub roles: &'a BTreeSet<String>,
}

/// Every scope a module's procedures run, gathered by the table each hold
/// names: what [`Holdings`] are computed from. Empty for a module with no
/// scope, whose every person holds everything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scopes {
    schema: Schema,
    holds: BTreeMap<TableName, Vec<Hold>>,
}

impl Scopes {
    /// The scopes of `m` that some procedure lists in its `uses`, each once.
    pub fn of(m: &Module) -> Scopes {
        let used: BTreeSet<&str> = m
            .functions
            .iter()
            .filter(|f| f.kind.is_procedure())
            .flat_map(|f| f.uses.iter().map(String::as_str))
            .collect();
        let mut holds: BTreeMap<TableName, Vec<Hold>> = BTreeMap::new();
        for f in m.functions.iter().filter(|f| f.kind == FnKind::Scope && used.contains(f.name.as_str())) {
            for h in &f.holds {
                holds.entry(h.table.clone()).or_default().push(h.clone());
            }
        }
        Scopes {
            schema: m.schema.clone(),
            holds,
        }
    }

    /// The scopes the procedures among `closures` run, each once, over
    /// `schema`: [`Scopes::of`] for a peer that holds closures and no module
    /// — the simulation, a vector's runner — since a procedure's closure
    /// carries every scope it uses.
    pub fn of_closures<'c>(schema: &Schema, closures: impl IntoIterator<Item = &'c crate::hash::Closure>) -> Scopes {
        let mut seen: BTreeMap<String, &Function> = BTreeMap::new();
        for c in closures {
            if !c.function.kind.is_procedure() {
                continue;
            }
            for u in &c.function.uses {
                if let Some(f) = c.helpers.iter().find(|h| h.name == *u && h.kind == FnKind::Scope) {
                    seen.entry(f.name.clone()).or_insert(f);
                }
            }
        }
        let mut holds: BTreeMap<TableName, Vec<Hold>> = BTreeMap::new();
        for f in seen.values() {
            for h in &f.holds {
                holds.entry(h.table.clone()).or_default().push(h.clone());
            }
        }
        Scopes {
            schema: schema.clone(),
            holds,
        }
    }

    /// Whether the module has any scope at all: when not, every person holds
    /// everything and nothing here is asked.
    pub fn is_empty(&self) -> bool {
        self.holds.is_empty()
    }

    /// The role names the scopes test (`When(has_role(..))`), in order.
    pub fn roles(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for p in self.holds.values().flatten().filter_map(|h| h.filter.as_ref()) {
            pred_roles(p, &mut out);
        }
        out
    }

    /// What `who` holds.
    pub fn holdings(&self, who: Who) -> Holdings {
        let known = Ctx {
            user: Some(who.user),
            session: Some(who.session),
            roles: who.roles,
        };
        let mut tables = BTreeMap::new();
        for t in self.schema.tables() {
            if let Some(hs) = self.holds.get(&t.name) {
                let held = union(t, hs, &known);
                if !held.is_whole() {
                    tables.insert(t.name.clone(), held);
                }
            }
        }
        let mut h = Holdings {
            full: self.schema.clone(),
            schema: self.schema.clone(),
            tables,
        };
        h.schema = device_schema(&self.schema, &h.columns());
        h
    }
}

/// What one person holds of one table: the rows (`None`: every row;
/// otherwise those any of the filters admits, none for an empty list) and
/// the columns (`None`: every column; otherwise these, in the table's
/// order, the key among them).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Held {
    pub rows: Option<Vec<Pred>>,
    pub columns: Option<Vec<FieldName>>,
}

impl Held {
    pub fn is_whole(&self) -> bool {
        self.rows.is_none() && self.columns.is_none()
    }
}

/// `docs/plan-guards.md` D2 One person's union of the module's scopes: per
/// table, what they hold of it. A table not listed is held whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Holdings {
    full: Schema,
    schema: Schema,
    tables: BTreeMap<TableName, Held>,
}

impl Holdings {
    /// Every table whole: what a module with no scope gives everybody.
    pub fn whole(schema: &Schema) -> Holdings {
        Holdings {
            full: schema.clone(),
            schema: schema.clone(),
            tables: BTreeMap::new(),
        }
    }

    /// Whether this person holds every table whole, and so is served the
    /// log by intents exactly as before scopes.
    pub fn is_whole(&self) -> bool {
        self.tables.is_empty()
    }

    /// What is held of `table`, or `None` when it is held whole.
    pub fn held(&self, table: &str) -> Option<&Held> {
        self.tables.get(table)
    }

    /// Every table held in part, with the columns held of it in the
    /// table's order: what a partial snapshot says beside its rows, and
    /// all a device needs to lay out its schema ([`device_schema`]).
    pub fn columns(&self) -> BTreeMap<TableName, Vec<FieldName>> {
        self.tables
            .iter()
            .filter_map(|(t, h)| {
                let all = || self.full.lookup_table(t).map(|tb| tb.columns.iter().map(|c| c.name.clone()).collect());
                Some((t.clone(), h.columns.clone().or_else(all)?))
            })
            .collect()
    }

    /// The schema as this person's device has it: each table's held columns
    /// only, and only the indexes and references over them.
    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// Whether `row`, a row of `table` in the module's full layout, is held,
    /// reading an `exists` from `st`.
    pub fn admits(&self, table: &str, row: &Row, st: &dyn Store) -> bool {
        match self.tables.get(table).and_then(|h| h.rows.as_ref()) {
            None => true,
            Some(ps) => {
                let Some(t) = self.full.lookup_table(table) else { return false };
                ps.iter().any(|p| admits(t, p, row, st))
            }
        }
    }

    /// `row` laid out as this person's table holds it: the held columns.
    pub fn project(&self, table: &str, row: &Row) -> Row {
        match (self.tables.get(table).and_then(|h| h.columns.as_ref()), self.schema.lookup_table(table)) {
            (Some(_), Some(t)) => Row::new(
                t.row_columns().clone(),
                t.columns.iter().map(|c| row.get(&c.name).cloned().unwrap_or(Value::Null)),
            ),
            _ => row.clone(),
        }
    }

    fn project_change(&self, c: &Change) -> Option<Change> {
        if !self.tables.get(c.table()).is_some_and(|h| h.columns.is_some()) {
            return Some(c.clone());
        }
        let t = c.table();
        Some(match c {
            Change::Add(_, r) => Change::Add(t.into(), self.project(t, r)),
            Change::Remove(_, r) => Change::Remove(t.into(), self.project(t, r)),
            Change::Edit(_, o, n) => {
                let (o, n) = (self.project(t, o), self.project(t, n));
                // Only columns this person does not hold moved: nothing
                // moved, to them.
                if o == n {
                    return None;
                }
                Change::Edit(t.into(), o, n)
            }
        })
    }

    /// Every row of `st` this person holds, projected, by table in schema
    /// order: what a partial snapshot carries.
    pub fn held_rows(&self, st: &dyn Store) -> BTreeMap<TableName, Vec<Row>> {
        self.full
            .tables()
            .map(|t| {
                let rows = match self.tables.get(&t.name) {
                    None => st.scan(&t.name),
                    Some(h) => {
                        let rows = match &h.rows {
                            None => st.scan(&t.name),
                            Some(ps) => st.scan_where(&t.name, &|r| ps.iter().any(|p| admits(t, p, r, st))),
                        };
                        rows.iter().map(|r| self.project(&t.name, r)).collect()
                    }
                };
                (t.name.clone(), rows)
            })
            .collect()
    }

    /// §8.1 restricted to what this person holds of `st`: the state hash
    /// of a store holding exactly those rows, each projected to the held
    /// columns — what their device's own state hash is when it is right. A
    /// table held whole is its kept digest, read and not summed; any other
    /// is the sum of its held rows' leaves, `O(rows of the table)`, which a
    /// `Verify` from a partial peer costs the authority.
    pub fn digest(&self, st: &dyn Store) -> Vec<u8> {
        let digests: Vec<(&str, Digest)> = self
            .full
            .tables()
            .map(|t| {
                let d = match self.tables.get(&t.name) {
                    None => st
                        .digest(&t.name)
                        .unwrap_or_else(|| crate::hash::table_digest(&t.name, &st.scan(&t.name))),
                    Some(h) => {
                        let mut d = Digest::ZERO;
                        let rows = match &h.rows {
                            None => st.scan(&t.name),
                            Some(ps) => st.scan_where(&t.name, &|r| ps.iter().any(|p| admits(t, p, r, st))),
                        };
                        for r in rows {
                            d.add(&leaf(&t.name, &self.project(&t.name, &r)));
                        }
                        d
                    }
                };
                (t.name.as_str(), d)
            })
            .collect();
        state_hash_of(digests)
    }

    /// What this person is sent of one entry: `facts`, the entry's, between
    /// `before` and `after`, as they hold them.
    ///
    /// A change to a table they hold whole passes as it is. Any other is
    /// asked of its rows: an add whose row is held after, a remove whose row
    /// was held before, an edit both — and an edit across the line is what
    /// it is to them, the row arriving (`Add` of the new) or leaving
    /// (`Remove` of the old). Each is projected to the held columns, and an
    /// edit of columns they do not hold is nothing. Then the `exists` form:
    /// a table held through another has rows the entry did not touch whose
    /// visibility it moved by writing the rows that reference them. Each
    /// such row — named by a referencing fact's old or new value, and not
    /// itself among the facts — is asked before and after, and sent as an
    /// `Add` when it became held and a `Remove` when it stopped being, after
    /// the entry's own facts. Applied in order to what they held in
    /// `before`, the result is what they hold in `after`; that is the whole
    /// contract, and the fuzzer holds it.
    pub fn filter_facts(&self, before: &dyn Store, after: &dyn Store, facts: &[Change]) -> Facts {
        let mut out = Vec::new();
        for c in facts {
            let rows = self.tables.get(c.table()).and_then(|h| h.rows.as_ref());
            let seen = match (rows, c) {
                (None, _) => Some(c.clone()),
                (Some(_), Change::Add(t, new)) => self.admits(t, new, after).then(|| c.clone()),
                (Some(_), Change::Remove(t, old)) => self.admits(t, old, before).then(|| c.clone()),
                (Some(_), Change::Edit(t, old, new)) => match (self.admits(t, old, before), self.admits(t, new, after)) {
                    (true, true) => Some(c.clone()),
                    (true, false) => Some(Change::Remove(t.clone(), old.clone())),
                    (false, true) => Some(Change::Add(t.clone(), new.clone())),
                    (false, false) => None,
                },
            };
            if let Some(p) = seen.and_then(|c| self.project_change(&c)) {
                out.push(p);
            }
        }
        // The `exists` form: (the table held through another, the table it
        // reaches through, the referencing column).
        let looks: Vec<(&Table, &str, &str)> = self
            .tables
            .iter()
            .filter_map(|(t, h)| Some((self.full.lookup_table(t)?, h.rows.as_ref()?)))
            .flat_map(|(t, ps)| {
                let mut ls = vec![];
                for p in ps {
                    lookups(p, &mut ls);
                }
                ls.into_iter().map(move |(via, col)| (t, via, col))
            })
            .collect();
        if looks.is_empty() {
            return out;
        }
        let mut asked: BTreeSet<(TableName, Vec<Value>)> = BTreeSet::new();
        for (t, via, col) in looks {
            let touched: BTreeSet<Vec<Value>> = facts
                .iter()
                .filter(|c| c.table() == t.name)
                .flat_map(|c| match c {
                    Change::Add(_, r) | Change::Remove(_, r) => vec![t.key_of(r)],
                    Change::Edit(_, o, r) => vec![t.key_of(o), t.key_of(r)],
                })
                .collect();
            for c in facts.iter().filter(|c| c.table() == via) {
                let named: Vec<&Value> = match c {
                    Change::Add(_, r) | Change::Remove(_, r) => vec![r.get(col).unwrap_or(&Value::Null)],
                    Change::Edit(_, o, r) => vec![o.get(col).unwrap_or(&Value::Null), r.get(col).unwrap_or(&Value::Null)],
                };
                for k in named {
                    let key = vec![k.clone()];
                    if k.is_null() || touched.contains(&key) || !asked.insert((t.name.clone(), key.clone())) {
                        continue;
                    }
                    // Untouched by the entry, so the same row on both sides.
                    let Some(row) = after.get(&t.name, &key) else { continue };
                    match (self.admits(&t.name, &row, before), self.admits(&t.name, &row, after)) {
                        (false, true) => out.push(Change::Add(t.name.clone(), self.project(&t.name, &row))),
                        (true, false) => out.push(Change::Remove(t.name.clone(), self.project(&t.name, &row))),
                        _ => {}
                    }
                }
            }
        }
        out
    }
}

// The context a filter is folded under: everything a `Who` knows, or — for
// the verifier, which asks about every user at once — no user and no login.
struct Ctx<'a> {
    user: Option<&'a str>,
    session: Option<&'a str>,
    roles: &'a BTreeSet<String>,
}

// A context expression, folded: a value where the context decides it.
fn value(e: &Expr, ctx: &Ctx) -> Option<Value> {
    Some(match e {
        Expr::Lit(v) => v.clone(),
        Expr::CtxUser => Value::text(ctx.user?),
        Expr::CtxSession => Value::text(ctx.session?),
        Expr::HasRole(r) => Value::Bool(ctx.roles.contains(r)),
        Expr::Op(Op::Not, xs) => match &xs[..] {
            [x] => Value::Bool(value(x, ctx)? == Value::Bool(false)),
            _ => return None,
        },
        Expr::Op(Op::And, xs) => {
            let vs: Vec<Option<Value>> = xs.iter().map(|x| value(x, ctx)).collect();
            if vs.iter().any(|v| *v == Some(Value::Bool(false))) {
                Value::Bool(false)
            } else if vs.iter().all(|v| *v == Some(Value::Bool(true))) {
                Value::Bool(true)
            } else {
                return None;
            }
        }
        Expr::Op(Op::Or, xs) => {
            let vs: Vec<Option<Value>> = xs.iter().map(|x| value(x, ctx)).collect();
            if vs.iter().any(|v| *v == Some(Value::Bool(true))) {
                Value::Bool(true)
            } else if vs.iter().all(|v| *v == Some(Value::Bool(false))) {
                Value::Bool(false)
            } else {
                return None;
            }
        }
        Expr::Cmp(op, a, b) => Value::Bool(cmp(*op, &value(a, ctx)?, &value(b, ctx)?)),
        _ => return None,
    })
}

// A filter folded under a context: decided for every row, a predicate over
// the row's columns, or — only where the context does not say, which is
// the verifier's question — unknown.
#[derive(Clone, Debug, PartialEq)]
enum F {
    True,
    False,
    Rows(Pred),
    Unknown,
}

fn fold(p: &Pred, ctx: &Ctx) -> F {
    // A comparison of a column is over rows whatever the context: where
    // the context is not known — the verifier's question, about every user
    // at once — it is left as it was, still a predicate over rows.
    let rhs = |e: &Expr| value(e, ctx).map(Expr::Lit).unwrap_or_else(|| e.clone());
    match p {
        Pred::Cmp(c, op, e) => F::Rows(Pred::Cmp(c.clone(), *op, rhs(e))),
        Pred::In(c, es) => F::Rows(Pred::In(c.clone(), es.iter().map(rhs).collect())),
        Pred::Has(c, e) => F::Rows(Pred::Has(c.clone(), rhs(e))),
        Pred::When(e) => match value(e, ctx) {
            Some(Value::Bool(true)) => F::True,
            Some(_) => F::False,
            None => F::Unknown,
        },
        Pred::All(ps) => {
            let mut rows = vec![];
            let mut unknown = false;
            for q in ps {
                match fold(q, ctx) {
                    F::False => return F::False,
                    F::True => {}
                    F::Rows(r) => rows.push(r),
                    F::Unknown => unknown = true,
                }
            }
            match (unknown, rows.len()) {
                (true, _) => F::Unknown,
                (false, 0) => F::True,
                (false, 1) => F::Rows(rows.pop().expect("one")),
                (false, _) => F::Rows(Pred::All(rows)),
            }
        }
        Pred::Any(ps) => {
            let mut rows = vec![];
            let mut unknown = false;
            for q in ps {
                match fold(q, ctx) {
                    F::True => return F::True,
                    F::False => {}
                    F::Rows(r) => rows.push(r),
                    F::Unknown => unknown = true,
                }
            }
            // An unknown alternative beside rows that are known: the rows
            // are a lower bound, which is what the verifier asks for.
            match (rows.len(), unknown) {
                (0, true) => F::Unknown,
                (0, false) => F::False,
                (1, _) => F::Rows(rows.pop().expect("one")),
                _ => F::Rows(Pred::Any(rows)),
            }
        }
        Pred::Not(q) => match fold(q, ctx) {
            F::True => F::False,
            F::False => F::True,
            F::Rows(r) => F::Rows(Pred::Not(Box::new(r))),
            F::Unknown => F::Unknown,
        },
        Pred::Exists(t, c, q) => match fold(q, ctx) {
            F::False => F::False,
            F::True => F::Rows(Pred::Exists(t.clone(), c.clone(), Box::new(Pred::All(vec![])))),
            F::Rows(r) => F::Rows(Pred::Exists(t.clone(), c.clone(), Box::new(r))),
            F::Unknown => F::Unknown,
        },
    }
}

// The union of the holds naming one table, under a context: the rows of
// every contributing hold, and their columns.
fn union(t: &Table, hs: &[Hold], ctx: &Ctx) -> Held {
    let mut rows: Option<Vec<Pred>> = Some(vec![]);
    let mut cols: BTreeSet<&str> = BTreeSet::new();
    let mut all_cols = false;
    let mut any = false;
    for h in hs {
        let f = h.filter.as_ref().map_or(F::True, |p| fold(p, ctx));
        match f {
            F::False | F::Unknown => continue,
            F::True => rows = None,
            F::Rows(p) => {
                if let Some(rs) = &mut rows {
                    rs.push(p);
                }
            }
        }
        any = true;
        match &h.columns {
            Projection::All => all_cols = true,
            p => cols.extend(t.columns.iter().map(|c| c.name.as_str()).filter(|c| p.keeps(c))),
        }
    }
    // Nothing contributes: no row, so every column — there is none to show.
    if !any || all_cols {
        return Held { rows, columns: None };
    }
    cols.extend(t.key.iter().map(String::as_str));
    let columns: Vec<FieldName> = t.columns.iter().map(|c| c.name.clone()).filter(|c| cols.contains(c.as_str())).collect();
    let columns = (columns.len() < t.columns.len()).then_some(columns);
    Held { rows, columns }
}

// Whether a folded predicate admits `row`, a row of `t`, reading any
// `exists` from `st`.
fn admits(t: &Table, p: &Pred, row: &Row, st: &dyn Store) -> bool {
    let field = |c: &str| row.get(c).cloned().unwrap_or(Value::Null);
    let lit = |e: &Expr| match e {
        Expr::Lit(v) => Some(v.clone()),
        _ => None,
    };
    match p {
        Pred::Cmp(c, op, e) => lit(e).is_some_and(|v| cmp(*op, &field(c), &v)),
        Pred::In(c, es) => es.iter().any(|e| lit(e).is_some_and(|v| cmp(CmpOp::Eq, &field(c), &v))),
        Pred::All(ps) => ps.iter().all(|q| admits(t, q, row, st)),
        Pred::Any(ps) => ps.iter().any(|q| admits(t, q, row, st)),
        Pred::Not(q) => !admits(t, q, row, st),
        Pred::Has(c, e) => match (field(c), lit(e)) {
            (Value::Text(s), Some(Value::Text(n))) => fold_text(&s).contains(fold_text(&n).as_str()),
            _ => false,
        },
        Pred::When(e) => lit(e) == Some(Value::Bool(true)),
        Pred::Exists(via, column, q) => {
            let Some(child) = st.schema().lookup_table(via) else { return false };
            let key = t.key_of(row);
            let [k] = &key[..] else { return false };
            // The equality is a hint a store may serve from the reference
            // index and may not — an overlay's own writes are in no index —
            // so `keep` decides it too, as `Store::scan_where_eq` says.
            let names = |r: &Row| r.get(column).is_some_and(|v| cmp(CmpOp::Eq, v, k));
            !st.scan_where_eq(via, &[(column.as_str(), k)], &[], &|r| names(r) && admits(child, q, r, st))
                .is_empty()
        }
    }
}

// The (table, column) every `exists` in a folded filter reaches through.
fn lookups<'p>(p: &'p Pred, out: &mut Vec<(&'p str, &'p str)>) {
    match p {
        Pred::Exists(via, col, _) => out.push((via.as_str(), col.as_str())),
        Pred::All(ps) | Pred::Any(ps) => ps.iter().for_each(|q| lookups(q, out)),
        Pred::Not(q) => lookups(q, out),
        Pred::Cmp(..) | Pred::In(..) | Pred::Has(..) | Pred::When(_) => {}
    }
}

fn pred_roles(p: &Pred, out: &mut BTreeSet<String>) {
    match p {
        Pred::When(e) | Pred::Cmp(_, _, e) | Pred::Has(_, e) => expr_roles(e, out),
        Pred::In(_, es) => es.iter().for_each(|e| expr_roles(e, out)),
        Pred::All(ps) | Pred::Any(ps) => ps.iter().for_each(|q| pred_roles(q, out)),
        Pred::Not(q) | Pred::Exists(_, _, q) => pred_roles(q, out),
    }
}

fn expr_roles(e: &Expr, out: &mut BTreeSet<String>) {
    match e {
        Expr::HasRole(r) => {
            out.insert(r.clone());
        }
        Expr::Op(_, es) => es.iter().for_each(|e| expr_roles(e, out)),
        Expr::Cmp(_, a, b) => {
            expr_roles(a, out);
            expr_roles(b, out);
        }
        _ => {}
    }
}

/// The schema a device has, from the module's and the tables held in part
/// with their columns ([`Holdings::columns`], what a partial snapshot
/// says): each such table narrowed to its held columns, its indexes and
/// text indexes to those over them, and every table's references to those
/// whose column is held and whose parent is held whole — a device never
/// refuses a write for a parent it cannot see, and the authority checks
/// every reference.
pub fn device_schema(full: &Schema, held: &BTreeMap<TableName, Vec<FieldName>>) -> Schema {
    if held.is_empty() {
        return full.clone();
    }
    let whole = |t: &str| !held.contains_key(t);
    Schema {
        tables: full
            .tables()
            .map(|t| {
                let cols = held.get(&t.name);
                let keeps = |c: &str| cols.is_none_or(|cs| cs.iter().any(|x| x == c));
                let narrowed = cols.is_some_and(|cs| cs.len() < t.columns.len());
                let refs_move = t.refs.iter().any(|r| !whole(&r.table));
                if !narrowed && !refs_move {
                    return t.clone();
                }
                let columns: Vec<Column> = t.columns.iter().filter(|c| keeps(&c.name)).cloned().collect();
                let indexes: Vec<Index> = t.indexes.iter().filter(|ix| ix.columns.iter().all(|c| keeps(c))).cloned().collect();
                let refs: Vec<Ref> = t.refs.iter().filter(|r| keeps(&r.column) && whole(&r.table)).cloned().collect();
                let text: Vec<FieldName> = t.text.iter().filter(|c| keeps(c)).cloned().collect();
                Table::new(t.name.clone(), columns, t.key.clone(), indexes, refs).with_text(text)
            })
            .collect(),
    }
}

// The verifier's half --------------------------------------------------------

/// Role sets at most this many names make: past it, each role alone and
/// none at all — which is not every combination, and is said so.
const POWERSET_UP_TO: usize = 10;

/// `docs/plan-guards.md` D2 What a client-run part of a procedure may name.
///
/// For every role set the module names — the powerset of the role names
/// its scopes and its procedures' guards test, up to ten names, and past
/// that each name alone and none — with the user unknown (a `When` that
/// reads the user holds nothing here, which makes the union a lower
/// bound): the union's columns per table. A procedure is checked under a
/// role set unless one of its guards refuses on every path once its
/// `has_role`s are folded for the set — then nobody holding that set runs
/// it past the guard. Under every other role set, every column its
/// client-run parts name — its input checks and refinements, the guards
/// and provides it runs, its body or its plan, and the helpers they call,
/// all as `named` collected them — must be among the held columns of its
/// table, or it is [`Complaint::NotHeld`]: on that person's device the
/// column does not exist. A table held empty holds every column, so
/// reading one is no error; it returns no rows. A column outside every
/// person's union that a procedure must write is the server half's to
/// write (`ctx.private`, D3).
pub fn check(m: &Module, named: &BTreeMap<String, BTreeSet<(TableName, FieldName)>>) -> Vec<VerifyError> {
    let scopes = Scopes::of(m);
    if scopes.is_empty() {
        return vec![];
    }
    let mut names = scopes.roles();
    for f in m.functions.iter().filter(|f| f.kind == FnKind::Guard) {
        stmts_roles(&f.body, &mut names);
    }
    let names: Vec<String> = names.into_iter().collect();
    let sets: Vec<BTreeSet<String>> = if names.len() <= POWERSET_UP_TO {
        (0u32..(1 << names.len()))
            .map(|bits| {
                names
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| bits & (1 << i) != 0)
                    .map(|(_, n)| n.clone())
                    .collect()
            })
            .collect()
    } else {
        std::iter::once(BTreeSet::new()).chain(names.iter().map(|n| [n.clone()].into())).collect()
    };
    let unions: Vec<BTreeMap<TableName, Option<BTreeSet<FieldName>>>> = sets
        .iter()
        .map(|roles| {
            let ctx = Ctx {
                user: None,
                session: None,
                roles,
            };
            m.schema
                .tables()
                .map(|t| {
                    let cols = scopes
                        .holds
                        .get(&t.name)
                        .and_then(|hs| union(t, hs, &ctx).columns)
                        .map(|cs| cs.into_iter().collect());
                    (t.name.clone(), cols)
                })
                .collect()
        })
        .collect();
    let mut out = Vec::new();
    for p in m.functions.iter().filter(|f| f.kind.is_procedure()) {
        let c = crate::hash::closure(m, p);
        let parts: Vec<&Function> = std::iter::once(&c.function)
            .chain(&c.helpers)
            .filter(|f| f.kind != FnKind::Scope)
            .collect();
        let guards: Vec<&Function> = p
            .uses
            .iter()
            .filter_map(|u| m.lookup_function(u))
            .filter(|g| g.kind == FnKind::Guard)
            .collect();
        let wanted: BTreeSet<&(TableName, FieldName)> = parts.iter().filter_map(|f| named.get(&f.name)).flatten().collect();
        let mut reported: BTreeSet<&(TableName, FieldName)> = BTreeSet::new();
        for (roles, held) in sets.iter().zip(&unions) {
            if guards.iter().any(|g| refuses(&g.body, roles)) {
                continue;
            }
            for tc in &wanted {
                let (t, col) = tc;
                let ok = held.get(t).is_none_or(|cs| cs.as_ref().is_none_or(|cs| cs.contains(col)));
                if !ok && reported.insert(tc) {
                    out.push(VerifyError::In(
                        p.name.clone(),
                        Complaint::NotHeld(t.clone(), col.clone(), roles.iter().cloned().collect()),
                    ));
                }
            }
        }
    }
    out
}

fn stmts_roles(b: &[Stmt], out: &mut BTreeSet<String>) {
    for s in b {
        match s {
            Stmt::If(c, a, e) => {
                expr_roles(c, out);
                stmts_roles(a, out);
                stmts_roles(e, out);
            }
            Stmt::Let(_, e) | Stmt::Refuse(e) => expr_roles(e, out),
            _ => {}
        }
    }
}

// Whether a guard's body refuses on every path once its `has_role`s are
// folded for `roles`: an `if` whose condition folds is the branch it takes,
// one that does not fold may go either way, and a `refuse` reached is a
// refusal. Anything else passes on.
fn refuses(b: &[Stmt], roles: &BTreeSet<String>) -> bool {
    let ctx = Ctx {
        user: None,
        session: None,
        roles,
    };
    for s in b {
        match s {
            Stmt::Refuse(_) => return true,
            Stmt::If(c, a, e) => match value(c, &ctx) {
                Some(Value::Bool(true)) if refuses(a, roles) => return true,
                Some(Value::Bool(false)) if refuses(e, roles) => return true,
                None if refuses(a, roles) && refuses(e, roles) => return true,
                _ => {}
            },
            Stmt::Return(_) => return false,
            _ => {}
        }
    }
    false
}
