//! §13 Views: what a plan means, and a plan kept up to date.
//!
//! [`pull`] is the one evaluator of plans (`docs/plan-v4.md` §1.4): a
//! query's value, a mutator's `select`, a hydrating view are all what it
//! answers. It returns one [`Entry`] per candidate — a source row the
//! filter admits, or a group — carrying what §1.5 names: its key, the
//! dependencies its subtree read (by plan node, [`nodes`]), its order
//! keys, whether `having` admitted it, and its node. [`answer`] is the
//! list a caller sees: the admitted entries, in order, cut to the limit;
//! [`read`] is that answer for a caller that keeps no entries, which is
//! how a query and a mutator's `select` ask.
//!
//! A [`View`] is a plan kept up to date (§1.5): [`hydrate`] pulls it once
//! and keeps the entries, indexed by key and by `(node, dependency)`;
//! [`push_all`] is told the changes of one settle — the store already at
//! the state after all of them — rebuilds the entries they touch, once
//! each, against that store, and reports what it did to the answer as
//! positions ([`Patch`]). Every plan is maintained this way: a group
//! source, lookups, related plans at any depth, a having, expression order
//! keys and a limit window. The contract ([`contract`]): after any
//! sequence of changes the view is exactly a fresh hydrate over the same
//! store, and splicing the patches into the old answer gives the new one.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;

use crate::eval::{Args, Ctx, EvalError, EvalFault, NodeScope, Scope};
use crate::ir::{CmpOp, Expr, Function, Key, Lookup, Plan, Pred, Related, Source};
use crate::schema::{Dir, Schema, Table};
use crate::store::{compare_rows, Change, Row, Span, Store};
use crate::value::{compare_value, FieldName, TableName, Value};

// §1.3 The one evaluator ----------------------------------------------------

/// A `Lookup` or a `Related` of a plan tree, by its position in a
/// pre-order walk: a plan's lookups in order, then each related plan
/// followed by the nodes of its child plan. The root plan's first lookup
/// is 0.
pub type NodeId = usize;

/// One node of a plan tree that reads another table: what a dependency is
/// recorded against.
#[derive(Clone, Copy, Debug)]
pub enum Node<'p> {
    Lookup(&'p Lookup),
    Related(&'p Related),
}

impl Node<'_> {
    /// The table this node reads.
    pub fn table(&self) -> &TableName {
        match self {
            Node::Lookup(l) => &l.table,
            Node::Related(r) => r.plan.table(),
        }
    }

    /// The dependency value a row of [`Node::table`] would satisfy: the
    /// row's key for a lookup, the values of the `on` columns for a related
    /// plan — each as a list, the shape [`Entry::deps`] records.
    pub fn dependency(&self, sch: &Schema, row: &Row) -> Value {
        match self {
            Node::Lookup(l) => Value::List(sch.lookup_table(&l.table).map(|t| t.key_of(row)).unwrap_or_default()),
            Node::Related(r) => Value::List(r.on.iter().map(|(c, _)| row.get(c).cloned().unwrap_or(Value::Null)).collect()),
        }
    }
}

/// Every lookup and related node of a plan tree, by [`NodeId`].
pub fn nodes(plan: &Plan) -> Vec<(NodeId, Node<'_>)> {
    fn walk<'p>(p: &'p Plan, out: &mut Vec<(NodeId, Node<'p>)>) {
        for l in &p.lookups {
            out.push((out.len(), Node::Lookup(l)));
        }
        for r in &p.related {
            out.push((out.len(), Node::Related(r)));
            walk(&r.plan, out);
        }
    }
    let mut out = vec![];
    walk(plan, &mut out);
    out
}

/// How many lookup and related nodes a plan tree has.
pub fn node_count(plan: &Plan) -> usize {
    plan.lookups.len() + plan.related.iter().map(|r| 1 + node_count(&r.plan)).sum::<usize>()
}

/// §1.5 One candidate of a plan: a source row the filter admits, or a
/// non-empty group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The row's key, or the group's `by` tuple.
    pub key: Vec<Value>,
    /// What the subtree read, at every depth, in the order it was read:
    /// for a lookup the key it looked up, for a related plan the values its
    /// `on` computed — each a [`Value::List`] against the node's
    /// [`NodeId`] ([`Node::dependency`] is the same value computed from a
    /// row). A lookup with a `Null` key part reads nothing and records
    /// nothing. The related plans' own entries, admitted or not, add
    /// theirs: a child the limit or its having leaves out can still move
    /// into the answer.
    pub deps: Vec<(NodeId, Value)>,
    /// The order keys' values, in the plan's order.
    pub order: Vec<Value>,
    /// Whether `having` admitted it (no `having` admits every entry).
    pub admitted: bool,
    /// The node's value; `Null` when not admitted (a projection is not
    /// evaluated for a node nobody sees).
    pub node: Value,
}

/// §1.3, §1.5 Every candidate of a plan over a store, ordered: by the order
/// keys under each's direction, then by the entry's key ascending, which
/// makes the order total whether or not the verifier completed it. The
/// expressions are evaluated in `scope` (the filter) and in a node scope of
/// it (everything a node computes). A table the schema lacks is a bug.
pub fn pull(sch: &Schema, plan: &Plan, scope: &Scope, st: &dyn Store) -> Result<Vec<Entry>, EvalFault> {
    pull_at(sch, plan, 0, &[], scope, st)
}

/// The list a caller sees: the admitted entries' nodes in order, cut to
/// the plan's limit.
pub fn answer(plan: &Plan, entries: &[Entry]) -> Vec<Value> {
    let lim = plan.limit.map(|n| n.max(0) as usize).unwrap_or(usize::MAX);
    entries.iter().filter(|e| e.admitted).take(lim).map(|e| e.node.clone()).collect()
}

// [`answer`] of entries nobody keeps: the nodes are moved out rather than
// copied (`docs/plan-perf.md` R5).
fn answer_owned(plan: &Plan, entries: Vec<Entry>) -> Vec<Value> {
    let lim = plan.limit.map(|n| n.max(0) as usize).unwrap_or(usize::MAX);
    entries.into_iter().filter(|e| e.admitted).take(lim).map(|e| e.node).collect()
}

/// §1.4 What a read answers: [`answer`] of [`pull`], for a caller that
/// keeps no entries — a query's value, a mutator's `select`. A bare plan
/// (a mutator's read: a table, a filter, a column order, a limit) has one
/// entry per admitted row, its node the row and its order the row's
/// columns, so its answer is those rows under [`compare_entries`]'s order
/// cut to the limit, and none becomes an entry. The store is asked first
/// for the rows already in that order ([`Store::scan_ordered`],
/// `docs/plan-perf.md` R1): with an index over the playlist and the
/// position, `add_to_playlist`'s `MAX(pos) + 1` is one row examined
/// however long the playlist. Without one, every candidate is read and
/// only the rows the answer keeps are sorted. Anything else is pulled
/// whole. Either way the filter's bounds on a column go down with its
/// equalities ([`spans`], R6): `create_playlist` names a playlist from the
/// rows of `(user_id, name)` between the name and its numbered siblings.
pub fn read(sch: &Schema, plan: &Plan, scope: &Scope, st: &dyn Store) -> Result<Vec<Value>, EvalFault> {
    if !plan.is_bare() {
        return Ok(answer_owned(plan, pull(sch, plan, scope, st)?));
    }
    let (tbl, filter) = source(sch, plan, &[], scope)?;
    let order: Vec<(&str, Dir)> = plan
        .order
        .iter()
        .map(|(k, d)| match k {
            Key::Column(c) => (c.as_str(), *d),
            Key::Expr(_) => unreachable!("a bare plan orders by columns"),
        })
        .collect();
    let lim = plan.limit.map(|n| n.max(0) as usize).unwrap_or(usize::MAX);
    if lim == 0 {
        return Ok(vec![]);
    }
    let (eq, keep) = (equalities(filter.as_ref()), |r: &Row| admits(filter.as_ref(), r));
    let spans = spans(filter.as_ref());
    if let Some(rows) = st.scan_ordered(plan.table(), &eq, &spans, &order, &keep, lim) {
        return Ok(rows.into_iter().map(Value::Struct).collect());
    }
    let mut rows = st.scan_where_eq(plan.table(), &eq, &spans, &keep);
    // As compare_entries over a bare plan's entries: the order columns
    // under their directions, then the key, column by column.
    let cmp = |a: &Row, b: &Row| compare_rows(tbl, &order, a, b);
    // Rows of one table have distinct keys, so the order is total and an
    // unstable selection of the first `lim` is exact.
    if lim < rows.len() {
        rows.select_nth_unstable_by(lim - 1, cmp);
        rows.truncate(lim);
    }
    rows.sort_by(cmp);
    Ok(rows.into_iter().map(Value::Struct).collect())
}

/// The order of two entries of one plan (§1.5): each key under its
/// direction, then the entries' keys.
pub fn compare_entries(plan: &Plan, a: &Entry, b: &Entry) -> Ordering {
    for (i, (_, d)) in plan.order.iter().enumerate() {
        let o = compare_value(a.order.get(i).unwrap_or(&Value::Null), b.order.get(i).unwrap_or(&Value::Null));
        let o = if *d == Dir::Desc { o.reverse() } else { o };
        if o != Ordering::Equal {
            return o;
        }
    }
    a.key.cmp(&b.key)
}

// A plan whose node ids start at `base`, its rows pinned by `pins` (a child
// plan's `on`, evaluated over its parent).
fn pull_at(sch: &Schema, plan: &Plan, base: NodeId, pins: &[(FieldName, Value)], scope: &Scope, st: &dyn Store) -> Result<Vec<Entry>, EvalFault> {
    let (tbl, mut rows) = candidates(sch, plan, pins, scope, st)?;
    let mut out = Vec::new();
    match &plan.source {
        Source::Table(_) => {
            for row in rows {
                let key = tbl.key_of(&row);
                out.push(entry(sch, plan, base, scope, st, key, Value::Struct(row), None)?);
            }
        }
        Source::Group { by, .. } => {
            rows.sort_by_key(|r| tbl.key_of(r));
            let mut groups: BTreeMap<Vec<Value>, Vec<Value>> = BTreeMap::new();
            for row in rows {
                let k: Vec<Value> = by.iter().map(|c| row.get(c).cloned().unwrap_or(Value::Null)).collect();
                groups.entry(k).or_default().push(Value::Struct(row));
            }
            for (k, members) in groups {
                let key_row = Value::Struct(by.iter().cloned().zip(k.iter().cloned()).collect());
                out.push(entry(sch, plan, base, scope, st, k, key_row, Some(Value::List(members)))?);
            }
        }
    }
    out.sort_by(|a, b| compare_entries(plan, a, b));
    Ok(out)
}

// The source rows a plan's filter and its pins admit, through the store's
// indexes where they serve (`equalities`, `spans`), in no particular order.
fn candidates<'s>(
    sch: &'s Schema,
    plan: &Plan,
    pins: &[(FieldName, Value)],
    scope: &Scope,
    st: &dyn Store,
) -> Result<(&'s Table, Vec<Row>), EvalFault> {
    let (tbl, filter) = source(sch, plan, pins, scope)?;
    Ok((
        tbl,
        st.scan_where_eq(plan.table(), &equalities(filter.as_ref()), &spans(filter.as_ref()), &|r| {
            admits(filter.as_ref(), r)
        }),
    ))
}

// A plan's table, and its filter and its pins as one filter, evaluated.
fn source<'s>(sch: &'s Schema, plan: &Plan, pins: &[(FieldName, Value)], scope: &Scope) -> Result<(&'s Table, Option<Filter>), EvalFault> {
    let table = plan.table();
    let Some(tbl) = sch.lookup_table(table) else {
        return Err(EvalFault::Bug(EvalError::UnknownTable(table.clone())));
    };
    let mut all: Vec<Filter> = pins.iter().map(|(c, v)| Filter::Cmp(c.clone(), CmpOp::Eq, v.clone())).collect();
    if let Some(f) = &plan.filter {
        all.push(eval_pred(f, &mut |e: &Expr| scope.eval(e))?);
    }
    let filter = match all.len() {
        0 => None,
        1 => all.pop(),
        _ => Some(Filter::All(all)),
    };
    Ok((tbl, filter))
}

// One candidate, pulled: its lookups, its related plans, its having, its
// node and its order keys, in that order.
#[allow(clippy::too_many_arguments)]
fn entry(
    sch: &Schema,
    plan: &Plan,
    base: NodeId,
    scope: &Scope,
    st: &dyn Store,
    key: Vec<Value>,
    row: Value,
    members: Option<Value>,
) -> Result<Entry, EvalFault> {
    let mut deps = Vec::new();
    // A bare plan (the v3 shape a mutator reads with) binds nothing: its
    // node is the row, and a scope for it would be work for no one.
    if plan.is_bare() {
        let order = plan.order.iter().map(|(k, _)| order_key(k, &row, None)).collect::<Result<_, _>>()?;
        return Ok(Entry {
            key,
            deps,
            order,
            admitted: true,
            node: row,
        });
    }
    // The row is bound by reference — the entry keeps it for its order
    // keys — and a node that projects never builds the default struct of
    // the row and its related lists, so a node's own reads copy only what
    // they read (`docs/plan-perf.md` R5).
    let mut node = scope.node();
    if let Some(x) = plan.row {
        node.bind_ref(x, &row);
    }
    if let (Some(x), Some(m)) = (plan.members, members) {
        node.bind(x, m);
    }
    let mut id = base;
    for l in &plan.lookups {
        let k = l.key.iter().map(|e| node.eval(e)).collect::<Result<Vec<_>, _>>()?;
        let found = if k.iter().any(Value::is_null) {
            Value::Null
        } else {
            let found = st.get(&l.table, &k).map(Value::Struct).unwrap_or(Value::Null);
            deps.push((id, Value::List(k)));
            found
        };
        node.bind(l.sym, found);
        id += 1;
    }
    let mut fields: Option<BTreeMap<FieldName, Value>> = plan.project.is_none().then(|| match &row {
        Value::Struct(m) => m.clone(),
        _ => BTreeMap::new(),
    });
    for r in &plan.related {
        let on =
            r.on.iter()
                .map(|(c, e)| Ok((c.clone(), node.eval(e)?)))
                .collect::<Result<Vec<_>, EvalFault>>()?;
        deps.push((id, Value::List(on.iter().map(|(_, v)| v.clone()).collect())));
        let mut kids = pull_at(sch, &r.plan, id + 1, &on, scope, st)?;
        for k in &mut kids {
            deps.append(&mut k.deps);
        }
        let list = Value::List(answer_owned(&r.plan, kids));
        if let Some(fields) = &mut fields {
            fields.insert(r.name.clone(), list.clone());
        }
        node.bind(r.sym, list);
        id += 1 + node_count(&r.plan);
    }
    let admitted = match &plan.having {
        None => true,
        Some(h) => match node.eval(h)? {
            Value::Bool(b) => b,
            other => return Err(EvalFault::Bug(EvalError::TypeError(format!("a having is a Bool, not {other:?}")))),
        },
    };
    let value = match (&plan.project, admitted) {
        (_, false) => Value::Null,
        (Some(p), true) => node.eval(p)?,
        (None, true) => Value::Struct(fields.unwrap_or_default()),
    };
    let order = plan
        .order
        .iter()
        .map(|(k, _)| order_key(k, &row, Some(&node)))
        .collect::<Result<_, _>>()?;
    Ok(Entry {
        key,
        deps,
        order,
        admitted,
        node: value,
    })
}

fn order_key(k: &Key, row: &Value, node: Option<&NodeScope>) -> Result<Value, EvalFault> {
    match (k, node) {
        (Key::Column(c), _) => Ok(row.field(c)),
        (Key::Expr(e), Some(n)) => n.eval(e),
        (Key::Expr(_), None) => Err(EvalFault::Bug(EvalError::TypeError("an expression order key on a bare plan".into()))),
    }
}

// The filter, evaluated ------------------------------------------------------

// A filter with its right-hand sides evaluated: one constructor per
// [`Pred`] constructor.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Filter {
    Cmp(FieldName, CmpOp, Value),
    In(FieldName, Vec<Value>),
    All(Vec<Filter>),
    Any(Vec<Filter>),
    Not(Box<Filter>),
}

fn eval_pred<E>(p: &Pred, ev: &mut dyn FnMut(&Expr) -> Result<Value, E>) -> Result<Filter, E> {
    Ok(match p {
        Pred::Cmp(c, op, e) => Filter::Cmp(c.clone(), *op, ev(e)?),
        Pred::In(c, es) => Filter::In(c.clone(), es.iter().map(&mut *ev).collect::<Result<_, _>>()?),
        Pred::All(ps) => Filter::All(ps.iter().map(|q| eval_pred(q, ev)).collect::<Result<_, _>>()?),
        Pred::Any(ps) => Filter::Any(ps.iter().map(|q| eval_pred(q, ev)).collect::<Result<_, _>>()?),
        Pred::Not(q) => Filter::Not(Box::new(eval_pred(q, ev)?)),
    })
}

// The columns a filter holds equal to a value however it is satisfied: its
// top-level `Cmp(_, Eq, _)`, and every one inside a top-level `All`. What
// an indexed store looks rows up by ([`Store::scan_where_eq`]).
fn equalities(f: Option<&Filter>) -> Vec<(&str, &Value)> {
    fn go<'a>(f: &'a Filter, out: &mut Vec<(&'a str, &'a Value)>) {
        match f {
            Filter::Cmp(c, CmpOp::Eq, v) => out.push((c, v)),
            Filter::All(fs) => fs.iter().for_each(|g| go(g, out)),
            _ => {}
        }
    }
    let mut out = vec![];
    if let Some(f) = f {
        go(f, &mut out);
    }
    out
}

// The columns a filter holds between bounds however it is satisfied: its
// top-level `Cmp(_, Ge|Gt|Le|Lt, _)`, and every one inside a top-level
// `All`, one span per column with the tightest bound of each side (the
// greater lower, the lesser upper, exclusive over inclusive at a tie).
// What an indexed store reads a range of an index by (R6); `keep` still
// decides, so a bound left out is only a wider read.
fn spans(f: Option<&Filter>) -> Vec<Span<'_>> {
    fn go<'a>(f: &'a Filter, out: &mut Vec<Span<'a>>) {
        match f {
            Filter::Cmp(c, op @ (CmpOp::Ge | CmpOp::Gt | CmpOp::Le | CmpOp::Lt), v) => {
                let i = out.iter().position(|s| s.column == c.as_str()).unwrap_or_else(|| {
                    out.push(Span {
                        column: c,
                        lo: Bound::Unbounded,
                        hi: Bound::Unbounded,
                    });
                    out.len() - 1
                });
                let s = &mut out[i];
                match op {
                    CmpOp::Ge | CmpOp::Gt => {
                        let new = if *op == CmpOp::Ge { Bound::Included(v) } else { Bound::Excluded(v) };
                        if tighter(new, s.lo, Ordering::Greater) {
                            s.lo = new;
                        }
                    }
                    _ => {
                        let new = if *op == CmpOp::Le { Bound::Included(v) } else { Bound::Excluded(v) };
                        if tighter(new, s.hi, Ordering::Less) {
                            s.hi = new;
                        }
                    }
                }
            }
            Filter::All(fs) => fs.iter().for_each(|g| go(g, out)),
            _ => {}
        }
    }
    // Whether `new` narrows past `old`, `inward` being the direction a
    // bound moves to narrow: up for a lower one, down for an upper.
    fn tighter(new: Bound<&Value>, old: Bound<&Value>, inward: Ordering) -> bool {
        match (new, old) {
            (_, Bound::Unbounded) => true,
            (Bound::Unbounded, _) => false,
            (Bound::Included(n) | Bound::Excluded(n), Bound::Included(o) | Bound::Excluded(o)) => {
                let by = compare_value(n, o);
                by == inward || (by == Ordering::Equal && matches!(new, Bound::Excluded(_)))
            }
        }
    }
    let mut out = vec![];
    if let Some(f) = f {
        go(f, &mut out);
    }
    out
}

// Whether a row passes the filter; no filter admits every row. A column
// the row lacks reads as `Null`. Each column is compared where it is, not
// copied out first (`docs/plan-perf.md` R5).
fn admits(f: Option<&Filter>, row: &Row) -> bool {
    fn go(f: &Filter, row: &Row) -> bool {
        let field = |c: &str| row.get(c).unwrap_or(&Value::Null);
        match f {
            Filter::Cmp(c, op, v) => cmp(*op, field(c), v),
            Filter::In(c, vs) => vs.iter().any(|v| cmp(CmpOp::Eq, field(c), v)),
            Filter::All(fs) => fs.iter().all(|g| go(g, row)),
            Filter::Any(fs) => fs.iter().any(|g| go(g, row)),
            Filter::Not(g) => !go(g, row),
        }
    }
    f.is_none_or(|f| go(f, row))
}

/// Comparison under the one total order, so `NULL = NULL` is true and
/// `NULL < 0` is true — which is also why a related plan's `on` with a
/// `Null` parent value joins the children whose column is `Null`.
pub fn cmp(op: CmpOp, a: &Value, b: &Value) -> bool {
    let o = compare_value(a, b);
    match op {
        CmpOp::Eq => o == Ordering::Equal,
        CmpOp::Ne => o != Ordering::Equal,
        CmpOp::Lt => o == Ordering::Less,
        CmpOp::Le => o != Ordering::Greater,
        CmpOp::Gt => o == Ordering::Greater,
        CmpOp::Ge => o != Ordering::Less,
    }
}

// The plan's own filter, evaluated once for a hydrate or a push: its
// right-hand sides are constant for the read.
fn root_filter(plan: &Plan, scope: &Scope) -> Result<Option<Filter>, EvalFault> {
    plan.filter.as_ref().map(|f| eval_pred(f, &mut |e: &Expr| scope.eval(e))).transpose()
}

// A group source's key: the row's values of the `by` columns.
fn group_of(by: &[FieldName], row: &Row) -> Vec<Value> {
    by.iter().map(|c| row.get(c).cloned().unwrap_or(Value::Null)).collect()
}

// §1.5 The view -----------------------------------------------------------

/// What a view's expressions are evaluated in, owned: the function's
/// helpers, the context, the (checked) arguments and the provided values.
/// A [`Scope`] borrows these; a view rebuilds entries long after the call
/// that opened it, so it keeps its own ([`Env::scope`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Env {
    pub helpers: Vec<Function>,
    pub ctx: Ctx,
    pub args: Args,
    pub provided: Args,
}

impl Env {
    /// The scope a plan is pulled in.
    pub fn scope<'s>(&'s self, sch: &'s Schema) -> Scope<'s> {
        Scope::new(sch, &self.helpers, &self.ctx, &self.args, &self.provided)
    }
}

/// §1.5 A plan kept up to date: the plan, its environment, and one
/// [`Entry`] per candidate, with the two indexes that route a change.
///
/// Every field is a function of the plan, the environment and the store —
/// which is what [`contract`] checks, by comparing the whole view with a
/// fresh [`hydrate`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    pub plan: Plan,
    pub env: Env,
    /// The admitted entries in answer order, *not* cut to the limit: the
    /// answer is the first `limit` of them, and the rest are what a window
    /// refills from without reading the store (§1.5, 4). Each is its
    /// entry's *place* — its order keys followed by its key ([`place`]) —
    /// so that finding where an entry sits compares these and reads
    /// nothing else: a search that looked each probe up in `by_key` paid a
    /// walk of that map per probe, log² of the list, and at sixteen times
    /// the demo that was most of a toggle.
    pub entries: Vec<Vec<Value>>,
    /// Every candidate — a source row the filter admits, or a non-empty
    /// group — admitted or not, by key. A refused one is kept so that a
    /// child arriving can admit it (§1.3, having).
    pub by_key: BTreeMap<Vec<Value>, Entry>,
    /// For every `(node, dependency value)` some entry recorded, the keys of
    /// the entries that recorded it: how a change to a table read beneath
    /// the source finds what it touches, at any depth (§1.5, 2).
    pub by_dep: BTreeMap<(NodeId, Value), BTreeSet<Vec<Value>>>,
    /// A group source only: the keys of the rows the filter admits, by the
    /// group they are in. Kept from the changes, so that rebuilding a group
    /// costs its members and not a scan of the table (§1.5, 1); empty
    /// groups are not kept.
    pub groups: BTreeMap<Vec<Value>, BTreeSet<Vec<Value>>>,
}

/// §1.5 Pull everything, and keep it: the view whose answer is what
/// [`read`] gives for the plan in `env`.
pub fn hydrate(sch: &Schema, plan: &Plan, env: Env, st: &dyn Store) -> Result<View, EvalFault> {
    let (pulled, groups) = {
        let scope = env.scope(sch);
        let pulled = pull(sch, plan, &scope, st)?;
        let mut groups: BTreeMap<Vec<Value>, BTreeSet<Vec<Value>>> = BTreeMap::new();
        if let Source::Group { by, .. } = &plan.source {
            let (tbl, rows) = candidates(sch, plan, &[], &scope, st)?;
            for r in rows {
                groups.entry(group_of(by, &r)).or_default().insert(tbl.key_of(&r));
            }
        }
        (pulled, groups)
    };
    let mut v = View {
        plan: plan.clone(),
        env,
        entries: Vec::new(),
        by_key: BTreeMap::new(),
        by_dep: BTreeMap::new(),
        groups,
    };
    for e in pulled {
        if e.admitted {
            v.entries.push(place(&e));
        }
        v.index(&e);
        v.by_key.insert(e.key.clone(), e);
    }
    Ok(v)
}

/// Where an entry sorts, as one list: its order keys, then its key — what
/// [`View::entries`] holds for each admitted entry. [`compare_place`] reads
/// it the way [`compare_entries`] reads the entry.
pub fn place(e: &Entry) -> Vec<Value> {
    e.order.iter().chain(e.key.iter()).cloned().collect()
}

/// [`compare_entries`] of the entry a place was taken from, and `e`.
pub fn compare_place(plan: &Plan, place: &[Value], e: &Entry) -> Ordering {
    let n = plan.order.len().min(place.len());
    for (i, (_, d)) in plan.order.iter().enumerate() {
        let o = compare_value(place.get(i).unwrap_or(&Value::Null), e.order.get(i).unwrap_or(&Value::Null));
        let o = if *d == Dir::Desc { o.reverse() } else { o };
        if o != Ordering::Equal {
            return o;
        }
    }
    place[n..].cmp(&e.key[..])
}

/// §1.6 A rebase rolled the optimistic store back: hydrate again, in the
/// same environment.
pub fn rebuild(sch: &Schema, st: &dyn Store, view: &View) -> Result<View, EvalFault> {
    hydrate(sch, &view.plan, view.env.clone(), st)
}

impl View {
    /// The answer, as it stands: the admitted entries' nodes in order, cut
    /// to the limit — what [`read`] would answer now.
    pub fn rows(&self) -> Vec<Value> {
        (0..self.entries.len().min(self.limit())).map(|i| self.node_at(i)).collect()
    }

    fn limit(&self) -> usize {
        self.plan.limit.map(|n| n.max(0) as usize).unwrap_or(usize::MAX)
    }

    // The key of the admitted entry at `i`: what follows the order keys in
    // its place.
    fn key_at(&self, i: usize) -> &[Value] {
        &self.entries[i][self.plan.order.len()..]
    }

    // Where an entry of this plan sits, or would sit, among the admitted: a
    // binary search over the places, reading nothing else. An entry's own
    // place compares equal to it, so the old entry of a key being settled
    // is found at its place.
    fn position(&self, e: &Entry) -> usize {
        self.position_in(0..self.entries.len(), e)
    }

    // [`View::position`] within a stretch of the admitted entries: the
    // index, into the whole list, of the first entry in `range` that does
    // not sort before `e`.
    fn position_in(&self, range: std::ops::Range<usize>, e: &Entry) -> usize {
        range.start + self.entries[range].partition_point(|s| compare_place(&self.plan, s, e) == Ordering::Less)
    }

    // Whether `e` belongs at `p`, the place its key holds now: it sorts
    // after the entry before and before the entry after. The one question a
    // rebuilt entry whose order keys did not move needs answered, in two
    // comparisons rather than a search.
    fn fits_at(&self, p: usize, e: &Entry) -> bool {
        let before = p == 0 || compare_place(&self.plan, &self.entries[p - 1], e) == Ordering::Less;
        let after = p + 1 >= self.entries.len() || compare_place(&self.plan, &self.entries[p + 1], e) == Ordering::Greater;
        before && after
    }

    fn index(&mut self, e: &Entry) {
        for d in &e.deps {
            self.by_dep.entry(d.clone()).or_default().insert(e.key.clone());
        }
    }

    fn unindex(&mut self, e: &Entry) {
        for d in &e.deps {
            if let Some(ks) = self.by_dep.get_mut(d) {
                ks.remove(&e.key);
                if ks.is_empty() {
                    self.by_dep.remove(d);
                }
            }
        }
    }

    // The node at a position of the whole admitted list.
    fn node_at(&self, i: usize) -> Value {
        self.by_key[self.key_at(i)].node.clone()
    }

    // §1.5, 3–4 One entry's new state against its old, as patches against
    // the window the client holds. A moved entry is a `Remove` at its old
    // place and an `Insert` at its new one; an entry that stays where it
    // was is an `Update` when its node changed and nothing when it did
    // not. Under a limit, an entry leaving the window lets the next one in
    // and an entry entering it pushes the last one out — both read from
    // the entries, not the store.
    //
    // The admitted list is a vector, so what it costs to keep is how far an
    // entry moves in it: one that stays where it was — the common rebuild,
    // a child arriving or leaving beneath a row whose order keys did not
    // move — is written in place, checked against its two neighbours
    // rather than searched for again; one that moves is rotated across the
    // stretch it crosses; only an entry arriving or leaving shifts the tail.
    // Removing and reinserting would move the tail twice for every
    // rebuild, which is a cost of the list's length and not of the change
    // (§1.5, "Cost"). Where it was is found among the places alone, and
    // `by_key` is walked once to take the old entry out and once to put
    // the new one in.
    fn settle(&mut self, key: Vec<Value>, new: Option<Entry>, out: &mut Vec<Patch>) {
        let lim = self.limit();
        let old = self.by_key.remove(&key);
        let was = match &old {
            Some(o) if o.admitted => Some(self.position(o)),
            _ => None,
        };
        // An entry whose dependencies did not change leaves `by_dep` as it
        // is: taking each out and putting it back is two probes apiece.
        let same_deps = matches!((&old, &new), (Some(o), Some(n)) if o.deps == n.deps);
        if let (Some(o), false) = (&old, same_deps) {
            self.unindex(o);
        }
        let is = match (&new, was) {
            (Some(n), _) if !n.admitted => None,
            (None, _) => None,
            (Some(n), None) => Some(self.position(n)),
            (Some(n), Some(p)) => Some(if self.fits_at(p, n) {
                p
            } else if p > 0 && compare_place(&self.plan, &self.entries[p - 1], n) == Ordering::Greater {
                self.position_in(0..p, n)
            } else {
                // Among the list without `p`: everything up to `p`, and as
                // many after it as sort before `n`.
                self.position_in(p + 1..self.entries.len(), n) - 1
            }),
        };
        let changed = match (&old, &new) {
            (Some(o), Some(n)) => o.node != n.node,
            _ => true,
        };
        match (was, is) {
            (Some(p), None) => {
                self.entries.remove(p);
            }
            (None, Some(q)) => self.entries.insert(q, Vec::new()),
            (Some(p), Some(q)) if q < p => self.entries[q..=p].rotate_right(1),
            (Some(p), Some(q)) if q > p => self.entries[p..=q].rotate_left(1),
            _ => {}
        }
        // The place at `q` says the new order keys; one whose keys did not
        // move is left as it is rather than built again.
        if let (Some(q), Some(n)) = (is, &new) {
            let olen = self.plan.order.len();
            let slot = &mut self.entries[q];
            if slot.len() != olen + n.key.len() || slot[..olen] != n.order[..] {
                *slot = place(n);
            }
        }
        if let Some(n) = new {
            if !same_deps {
                self.index(&n);
            }
            self.by_key.insert(key, n);
        }
        match (was, is) {
            (None, None) => {}
            (Some(p), None) => {
                if p < lim {
                    out.push(Patch::Remove { at: p });
                    if self.entries.len() >= lim {
                        out.push(Patch::Insert {
                            at: lim - 1,
                            node: self.node_at(lim - 1),
                        });
                    }
                }
            }
            (None, Some(q)) => {
                if q < lim {
                    out.push(Patch::Insert {
                        at: q,
                        node: self.node_at(q),
                    });
                    if self.entries.len() > lim {
                        out.push(Patch::Remove { at: lim });
                    }
                }
            }
            (Some(p), Some(q)) if p == q => {
                if p < lim && changed {
                    out.push(Patch::Update {
                        at: p,
                        node: self.node_at(p),
                    });
                }
            }
            (Some(p), Some(q)) => match (p < lim, q < lim) {
                (true, true) => {
                    out.push(Patch::Remove { at: p });
                    out.push(Patch::Insert {
                        at: q,
                        node: self.node_at(q),
                    });
                }
                (true, false) => {
                    out.push(Patch::Remove { at: p });
                    out.push(Patch::Insert {
                        at: lim - 1,
                        node: self.node_at(lim - 1),
                    });
                }
                (false, true) => {
                    out.push(Patch::Insert {
                        at: q,
                        node: self.node_at(q),
                    });
                    out.push(Patch::Remove { at: lim });
                }
                (false, false) => {}
            },
        }
    }
}

// §13.4 Patches -------------------------------------------------------------

/// What [`push_all`] did to the view's answer, as positions into the list
/// as it stands when the patch is applied, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Patch {
    /// Puts the node before the element at `at` (`at == len` appends).
    Insert {
        at: usize,
        node: Value,
    },
    Remove {
        at: usize,
    },
    Update {
        at: usize,
        node: Value,
    },
}

/// Apply each patch in order: the definition a client's list is held to.
pub fn splice(ps: &[Patch], xs: &[Value]) -> Vec<Value> {
    let mut vs = xs.to_vec();
    for p in ps {
        match p {
            Patch::Insert { at, node } => vs.insert((*at).min(vs.len()), node.clone()),
            Patch::Remove { at } => {
                if *at < vs.len() {
                    vs.remove(*at);
                }
            }
            Patch::Update { at, node } => {
                if *at < vs.len() {
                    vs[*at] = node.clone();
                }
            }
        }
    }
    vs
}

// §1.5 Maintenance ----------------------------------------------------------

/// §1.5 The changes of one settle, pushed through the view: `st` is already
/// the store after all of them. Every entry a change touches is rebuilt
/// from `st`, once, however many changes touched it:
///
/// 1. a change in the source table names a key — a row's key, or for a
///    group source the group of the old row and of the new one — and the
///    store now decides that key's entry: gone, new, or rebuilt;
/// 2. a change in a table some `Lookup` or `Related` node reads, at any
///    depth, selects through [`View::by_dep`] the entries whose recorded
///    dependencies hold the value the old or the new row satisfies
///    ([`Node::dependency`]), and each is rebuilt; the rebuild decides
///    whether the row was really in its subtree;
/// 3. each rebuilt entry is settled against the old one, in key order, into
///    patches (`Insert`, `Remove`, `Update`, a move as `Remove` then
///    `Insert`), with the limit window refilled from the entries.
///
/// Nothing is changed unless every rebuild succeeds: an `Err` (a fault in
/// one of the plan's expressions) leaves the view as it was, which is then
/// stale — [`rebuild`] it.
///
/// Because the rebuild reads the final store, the order of the changes and
/// how many there are do not matter beyond which keys they name; a batch
/// of *n* changes touching *k* entries costs *k* rebuilds and no rollback.
/// What a rebuild costs beyond its reads is the indexes' probes — a
/// logarithm of the view, not a pass over it: an entry that stays put is
/// not moved in [`View::entries`], and a read that holds a table's key is
/// one `get` ([`Store::scan_where_eq`]). `tests/toggle.rs` holds a
/// playlist toggle through harken's `library` to two store reads at any
/// size, and times it from 250 media to 16000.
pub fn push_all(sch: &Schema, st: &dyn Store, changes: &[Change], view: &mut View) -> Result<Vec<Patch>, EvalFault> {
    let (rebuilt, groups) = touched(sch, st, changes, view)?;
    for (g, members) in groups {
        if members.is_empty() {
            view.groups.remove(&g);
        } else {
            view.groups.insert(g, members);
        }
    }
    let mut out = Vec::new();
    for (k, e) in rebuilt {
        view.settle(k, e, &mut out);
    }
    Ok(out)
}

type Rebuilt = Vec<(Vec<Value>, Option<Entry>)>;
type Groups = BTreeMap<Vec<Value>, BTreeSet<Vec<Value>>>;

// §1.5, 1–2 The keys the changes touch, each with its entry rebuilt from
// the store (`None`: no candidate there any more), and a group source's
// touched groups as they now stand. Reads the view; changes nothing.
fn touched(sch: &Schema, st: &dyn Store, changes: &[Change], view: &View) -> Result<(Rebuilt, Groups), EvalFault> {
    let plan = &view.plan;
    let table = plan.table();
    let Some(tbl) = sch.lookup_table(table) else {
        return Err(EvalFault::Bug(EvalError::UnknownTable(table.clone())));
    };
    let scope = view.env.scope(sch);
    let filter = root_filter(plan, &scope)?;
    let ns = nodes(plan);
    let mut keys: BTreeSet<Vec<Value>> = BTreeSet::new();
    let mut groups: Groups = BTreeMap::new();
    for ch in changes {
        let (old, new) = match ch {
            Change::Add(_, r) => (None, Some(r)),
            Change::Remove(_, r) => (Some(r), None),
            Change::Edit(_, o, n) => (Some(o), Some(n)),
        };
        if ch.table() == table.as_str() {
            match &plan.source {
                Source::Table(_) => keys.extend(old.iter().chain(new.iter()).map(|r| tbl.key_of(r))),
                // In change order, so an edit within a group, out of one
                // and into another, all leave the members right.
                Source::Group { by, .. } => {
                    for (r, arrives) in [(old, false), (new, true)] {
                        let Some(r) = r else { continue };
                        let g = group_of(by, r);
                        let members = groups
                            .entry(g.clone())
                            .or_insert_with(|| view.groups.get(&g).cloned().unwrap_or_default());
                        if !arrives {
                            members.remove(&tbl.key_of(r));
                        } else if admits(filter.as_ref(), r) {
                            members.insert(tbl.key_of(r));
                        }
                        keys.insert(g);
                    }
                }
            }
        }
        for (id, n) in &ns {
            if n.table() != ch.table() {
                continue;
            }
            for r in old.iter().chain(new.iter()) {
                if let Some(ks) = view.by_dep.get(&(*id, n.dependency(sch, r))) {
                    keys.extend(ks.iter().cloned());
                }
            }
        }
    }
    let mut rebuilt = Vec::with_capacity(keys.len());
    for k in keys {
        let e = match &plan.source {
            Source::Table(_) => match st.get(table, &k) {
                Some(row) if admits(filter.as_ref(), &row) => Some(entry(sch, plan, 0, &scope, st, k.clone(), Value::Struct(row), None)?),
                _ => None,
            },
            Source::Group { by, .. } => match groups.get(&k).or_else(|| view.groups.get(&k)) {
                Some(ms) if !ms.is_empty() => {
                    let rows: Vec<Value> = ms.iter().filter_map(|m| st.get(table, m)).map(Value::Struct).collect();
                    let key_row = Value::Struct(by.iter().cloned().zip(k.iter().cloned()).collect());
                    Some(entry(sch, plan, 0, &scope, st, k.clone(), key_row, Some(Value::List(rows)))?)
                }
                _ => None,
            },
        };
        rebuilt.push((k, e));
    }
    Ok((rebuilt, groups))
}

// §1.5 The contract -----------------------------------------------------------

/// A maintained view is right when it is indistinguishable from one
/// hydrated now: the same entries, the same order, the same indexes. (The
/// answer being right, and the patches splicing the old answer into it,
/// follow; the tests check those too.)
pub fn contract(sch: &Schema, st: &dyn Store, view: &View) -> bool {
    rebuild(sch, st, view).is_ok_and(|fresh| fresh == *view)
}

#[cfg(test)]
mod span_tests {
    use super::*;

    /// R6: the bounds a filter holds a column between, as the store is told
    /// them — one span per column, from the top level and a top-level `All`
    /// only, each side the tightest the filter says (at a tie, exclusive).
    /// A bound under an `Any` or a `Not` is no bound on every row and is
    /// left out; an equality is the equalities' and not a span. Falsified
    /// by dropping the tie rule in `tighter`: `>= "a"` after `> "a"` widens
    /// the lower bound back to inclusive.
    #[test]
    fn a_filter_s_bounds_become_one_span_per_column() {
        let t = |s: &str| Value::Text(s.into());
        let cmp = |c: &str, op, v| Filter::Cmp(c.into(), op, v);
        let f = Filter::All(vec![
            cmp("user_id", CmpOp::Eq, t("ada")),
            cmp("name", CmpOp::Gt, t("a")),
            cmp("name", CmpOp::Ge, t("a")),
            cmp("name", CmpOp::Lt, t("c")),
            cmp("name", CmpOp::Le, t("b")),
            cmp("name", CmpOp::Le, t("bb")),
            Filter::Any(vec![cmp("pos", CmpOp::Gt, Value::int(3))]),
            Filter::Not(Box::new(cmp("pos", CmpOp::Lt, Value::int(1)))),
            Filter::All(vec![cmp("pos", CmpOp::Ge, Value::int(7))]),
        ]);
        let (a, b, seven) = (t("a"), t("b"), Value::int(7));
        assert_eq!(
            spans(Some(&f)),
            vec![
                Span {
                    column: "name",
                    lo: Bound::Excluded(&a),
                    hi: Bound::Included(&b),
                },
                Span {
                    column: "pos",
                    lo: Bound::Included(&seven),
                    hi: Bound::Unbounded,
                },
            ]
        );
        assert_eq!(spans(Some(&cmp("pos", CmpOp::Ne, Value::int(1)))), vec![]);
        assert_eq!(spans(None), vec![]);
    }
}
