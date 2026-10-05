//! `docs/plan-auth.md` A table's rules, asked: who may see a row, who may
//! write one, and what that makes of a store and of an entry's facts.
//!
//! The engine carries the mechanism and decides no rule. A rule is the
//! application's declaration ([`crate::schema::Table::visible`],
//! [`crate::schema::Table::writable`]) — a predicate over the row's own
//! columns, the identity's user (`Me`, [`Expr::CtxUser`]) and roles
//! ([`Pred::Role`]), and the one lookup ([`Pred::Exists`]) — and this module
//! is everything that asks one, sans-io, so the authority, a replica and
//! the fuzzer ask it the same way:
//!
//! - [`whole_to`]: whether every table is `Everyone` to an identity — no
//!   rule, or one its roles alone decide true. Such a peer is served the
//!   log whole, by intents, exactly as before rules existed; any other is
//!   served by facts, filtered ([`crate::protocol`]).
//! - [`sees`] and [`may_write`]: one row, against a store, for one
//!   identity.
//! - [`forbidden`]: the first table an entry's facts write that its
//!   `writable` rule does not admit for the author — old rows against the
//!   state before the entry, new rows against the state after it.
//! - [`filter_facts`]: what a partial peer is sent of one entry — the facts
//!   whose rows it may see, an edit across the line as the add or the remove
//!   it is to that peer, and for the lookup form the rows whose visibility
//!   the entry moved without touching them, as an `Add` or a `Remove`.
//! - [`visible_rows`] and [`partition_hash`]: what a partial peer holds of a
//!   store, and its state hash (§8.1 restricted to those rows: a table
//!   `Everyone` to it is its kept digest, any other the sum of its visible
//!   rows' leaves).
//!
//! **A table with no rule costs nothing.** Every function here first asks
//! whether the table's rule is decided by the identity alone, and a table
//! `Everyone` to it is passed through without reading a row.

use std::collections::{BTreeMap, BTreeSet};

use crate::hash::{leaf, state_hash_of, Digest};
use crate::ir::{CmpOp, Expr, Pred};
use crate::log::Facts;
use crate::schema::{Schema, Table};
use crate::store::{fold, Change, MemoryStore, Row, Store};
use crate::value::{TableName, Value};
use crate::view::cmp;

/// The identity a rule is asked about: the user `Me` is, and the roles
/// held.
#[derive(Clone, Copy, Debug)]
pub struct Who<'a> {
    pub user: &'a str,
    pub roles: &'a BTreeSet<String>,
}

/// What a predicate is for an identity before any row is read: `Some(true)`
/// for every row, `Some(false)` for none, `None` when it depends on the row.
/// Roles are known; everything else is not.
pub fn decided(p: &Pred, who: Who) -> Option<bool> {
    match p {
        Pred::Role(r) => Some(who.roles.contains(r)),
        Pred::All(ps) => {
            let mut out = Some(true);
            for q in ps {
                match decided(q, who) {
                    Some(false) => return Some(false),
                    Some(true) => {}
                    None => out = None,
                }
            }
            out
        }
        Pred::Any(ps) => {
            let mut out = Some(false);
            for q in ps {
                match decided(q, who) {
                    Some(true) => return Some(true),
                    Some(false) => {}
                    None => out = None,
                }
            }
            out
        }
        Pred::Not(q) => decided(q, who).map(|b| !b),
        Pred::Cmp(..) | Pred::In(..) | Pred::Has(..) | Pred::Exists(..) => None,
    }
}

/// Whether a rule admits every row for this identity: none declared
/// (`Everyone`), or one its roles alone make true.
pub fn admits_all(rule: Option<&Pred>, who: Who) -> bool {
    rule.is_none_or(|p| decided(p, who) == Some(true))
}

/// Whether every table of the schema is `Everyone` to this identity: such
/// a peer is whole, and is served the log by intents as it always was.
pub fn whole_to(sch: &Schema, who: Who) -> bool {
    sch.tables().all(|t| admits_all(t.visible.as_ref(), who))
}

/// Whether any table has a `writable` rule this identity's roles do not
/// decide true: whether an entry it pushes has to be checked at all.
pub fn writes_checked(sch: &Schema, who: Who) -> bool {
    !sch.tables().all(|t| admits_all(t.writable.as_ref(), who))
}

// A value a rule compares with: a literal, or `Me`; anything else a
// verified schema never has (`RuleBadValue`) and admits nothing.
fn value_of(e: &Expr, who: Who) -> Option<Value> {
    match e {
        Expr::Lit(v) => Some(v.clone()),
        Expr::CtxUser => Some(Value::text(who.user)),
        _ => None,
    }
}

/// Whether the predicate admits `row`, a row of `t`, for `who`, reading any
/// lookup from `st`.
pub fn admits(t: &Table, p: &Pred, row: &Row, who: Who, st: &dyn Store) -> bool {
    let field = |c: &str| row.get(c).cloned().unwrap_or(Value::Null);
    match p {
        Pred::Cmp(c, op, e) => value_of(e, who).is_some_and(|v| cmp(*op, &field(c), &v)),
        Pred::In(c, es) => es.iter().any(|e| value_of(e, who).is_some_and(|v| cmp(CmpOp::Eq, &field(c), &v))),
        Pred::All(ps) => ps.iter().all(|q| admits(t, q, row, who, st)),
        Pred::Any(ps) => ps.iter().any(|q| admits(t, q, row, who, st)),
        Pred::Not(q) => !admits(t, q, row, who, st),
        Pred::Has(c, e) => match (field(c), value_of(e, who)) {
            (Value::Text(s), Some(Value::Text(n))) => fold(&s).contains(fold(&n).as_str()),
            _ => false,
        },
        Pred::Role(r) => who.roles.contains(r),
        Pred::Exists(via, column, q) => {
            let Some(child) = st.schema().lookup_table(via) else { return false };
            let key = t.key_of(row);
            let [k] = &key[..] else { return false };
            !st.scan_where_eq(via, &[(column.as_str(), k)], &[], &|r| admits(child, q, r, who, st))
                .is_empty()
        }
    }
}

/// Whether `who` may see `row`, a row of `t`, in the state `st`.
pub fn sees(t: &Table, row: &Row, who: Who, st: &dyn Store) -> bool {
    match &t.visible {
        None => true,
        Some(p) => decided(p, who).unwrap_or_else(|| admits(t, p, row, who, st)),
    }
}

/// Whether `who` may write `row`, a row of `t`, in the state `st`.
pub fn may_write(t: &Table, row: &Row, who: Who, st: &dyn Store) -> bool {
    match &t.writable {
        None => true,
        Some(p) => decided(p, who).unwrap_or_else(|| admits(t, p, row, who, st)),
    }
}

/// The first table one of `facts` writes that its `writable` rule does not
/// admit for `who`: a removed or edited row's old side against `before`,
/// the state the entry ran over, and an added or edited row's new side
/// against `after`, the state it left. `None` when every row is admitted —
/// always, for a schema whose rules `who`'s roles decide true, which reads
/// no row.
pub fn forbidden(sch: &Schema, before: &dyn Store, after: &dyn Store, facts: &[Change], who: Who) -> Option<TableName> {
    for c in facts {
        let Some(t) = sch.lookup_table(c.table()) else { continue };
        if admits_all(t.writable.as_ref(), who) {
            continue;
        }
        let ok = match c {
            Change::Add(_, new) => may_write(t, new, who, after),
            Change::Remove(_, old) => may_write(t, old, who, before),
            Change::Edit(_, old, new) => may_write(t, old, who, before) && may_write(t, new, who, after),
        };
        if !ok {
            return Some(t.name.clone());
        }
    }
    None
}

/// What a peer that is not whole is sent of one entry: `facts`, the
/// entry's, between `before` and `after`, as `who` may see them.
///
/// A change to a table `Everyone` to `who` passes as it is. Any other is
/// asked of its rows: an add whose row it may see after, a remove whose row
/// it could see before, an edit both — and an edit across the line is what
/// it is to this peer, the row arriving (`Add` of the new) or leaving
/// (`Remove` of the old). Then the lookup form: a table whose `visible`
/// rule reads another through [`Pred::Exists`] has rows the entry did not
/// touch whose visibility it moved, by writing the rows that reference
/// them. Each such row — named by a referencing fact's old or new value,
/// and not itself among the facts — is asked before and after, and sent as
/// an `Add` when it became visible and a `Remove` when it stopped being,
/// after the entry's own facts. Applied in order to the rows `who` could
/// see in `before`, the result is the rows it can see in `after`; that is
/// the whole contract, and the fuzzer holds it.
pub fn filter_facts(sch: &Schema, before: &dyn Store, after: &dyn Store, facts: &[Change], who: Who) -> Facts {
    let mut out = Vec::new();
    for c in facts {
        let Some(t) = sch.lookup_table(c.table()) else {
            out.push(c.clone());
            continue;
        };
        if admits_all(t.visible.as_ref(), who) {
            out.push(c.clone());
            continue;
        }
        match c {
            Change::Add(_, new) => {
                if sees(t, new, who, after) {
                    out.push(c.clone());
                }
            }
            Change::Remove(_, old) => {
                if sees(t, old, who, before) {
                    out.push(c.clone());
                }
            }
            Change::Edit(tn, old, new) => match (sees(t, old, who, before), sees(t, new, who, after)) {
                (true, true) => out.push(c.clone()),
                (true, false) => out.push(Change::Remove(tn.clone(), old.clone())),
                (false, true) => out.push(Change::Add(tn.clone(), new.clone())),
                (false, false) => {}
            },
        }
    }
    // The lookup form: (the table whose rule looks, the table it looks
    // through, the referencing column), for every rule this identity's
    // roles do not decide.
    let looks: Vec<(&Table, &str, &str)> = sch
        .tables()
        .filter(|t| !admits_all(t.visible.as_ref(), who))
        .flat_map(|t| {
            let mut ls = vec![];
            if let Some(p) = &t.visible {
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
                match (sees(t, &row, who, before), sees(t, &row, who, after)) {
                    (false, true) => out.push(Change::Add(t.name.clone(), row)),
                    (true, false) => out.push(Change::Remove(t.name.clone(), row)),
                    _ => {}
                }
            }
        }
    }
    out
}

// The (table, column) every lookup in a rule reads through.
fn lookups<'p>(p: &'p Pred, out: &mut Vec<(&'p str, &'p str)>) {
    match p {
        Pred::Exists(via, col, _) => out.push((via.as_str(), col.as_str())),
        Pred::All(ps) | Pred::Any(ps) => ps.iter().for_each(|q| lookups(q, out)),
        Pred::Not(q) => lookups(q, out),
        Pred::Cmp(..) | Pred::In(..) | Pred::Has(..) | Pred::Role(_) => {}
    }
}

/// Every row of `st` that `who` may see, by table, in schema order: what a
/// partial peer's snapshot holds.
pub fn visible_rows(st: &MemoryStore, who: Who) -> BTreeMap<TableName, Vec<Row>> {
    st.schema()
        .tables()
        .map(|t| {
            let rows = if admits_all(t.visible.as_ref(), who) {
                st.scan(&t.name)
            } else {
                st.scan_where(&t.name, &|r| sees(t, r, who, st))
            };
            (t.name.clone(), rows)
        })
        .collect()
}

/// §8.1 restricted to what `who` may see of `st`: the state hash of the
/// store holding only those rows. A table `Everyone` to `who` is its kept
/// digest, read and not summed; any other is the sum of its visible rows'
/// leaves, `O(rows of the table)` — what a partial peer's `Verify` costs
/// the authority, and verifies are rare (`docs/plan-auth.md`).
pub fn partition_hash(st: &dyn Store, who: Who) -> Vec<u8> {
    let sch = st.schema();
    let digests: Vec<(&str, Digest)> = sch
        .tables()
        .map(|t| {
            let d = if admits_all(t.visible.as_ref(), who) {
                st.digest(&t.name)
                    .unwrap_or_else(|| crate::hash::table_digest(&t.name, &st.scan(&t.name)))
            } else {
                let mut d = Digest::ZERO;
                for r in st.scan_where(&t.name, &|r| sees(t, r, who, st)) {
                    d.add(&leaf(&t.name, &r));
                }
                d
            };
            (t.name.as_str(), d)
        })
        .collect();
    state_hash_of(digests)
}

/// A change undone: the transition back, exact because a fact carries the
/// whole row on each side — what taking an entry's facts back off a later
/// state applies, newest first.
pub fn undo(c: &Change) -> Change {
    match c {
        Change::Add(t, r) => Change::Remove(t.clone(), r.clone()),
        Change::Remove(t, r) => Change::Add(t.clone(), r.clone()),
        Change::Edit(t, old, new) => Change::Edit(t.clone(), new.clone(), old.clone()),
    }
}
