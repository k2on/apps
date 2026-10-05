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
use crate::ir::{CmpOp, Expr, FnKind, Function, Key, Lookup, Op, Plan, Pred, Related, Source, StdFn, Stmt, Sym};
use crate::schema::{Dir, Schema, Table};
use crate::store::{self, compare_rows, Change, Row, Span, Store};
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
            Node::Lookup(l) => Value::from(sch.lookup_table(&l.table).map(|t| t.key_of(row)).unwrap_or_default()),
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
    /// R9 The numbers kept for each kept related plan beneath it
    /// ([`Shape::kept`]), at every depth, by `(node, dependency)`: each
    /// key is a dependency the entry recorded, as [`Entry::deps`] are, and
    /// is indexed the same way. Empty for an entry [`pull`] made, which
    /// keeps nothing.
    pub held: HeldMap,
    /// R9 A group's members as numbers, one per aggregate of
    /// [`Face::members`]; empty when they are a list. An extreme (D4) is
    /// the column's value, or `Null` for none.
    pub members: Vec<Num>,
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
    // A text search is read through its text index (D4) and then sorted:
    // an ordered walk would examine every row up to the limit's last
    // match, which is the table when the needle is rare.
    let searched = needles(filter.as_ref()).is_some_and(|bs| bs.iter().flatten().any(|(_, n)| !store::trigrams(n).is_empty()));
    if !searched {
        if let Some(rows) = st.scan_ordered(plan.table(), &eq, &spans, &order, &keep, lim) {
            return Ok(rows.into_iter().map(Row::into_value).collect());
        }
    }
    let mut rows = fetch(st, plan.table(), filter.as_ref());
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
    Ok(rows.into_iter().map(Row::into_value).collect())
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
                out.push(entry(sch, plan, base, scope, st, key, Cand::Row(row), None)?);
            }
        }
        Source::Group { by, .. } => {
            rows.sort_by_key(|r| tbl.key_of(r));
            let mut groups: BTreeMap<Vec<Value>, Vec<Value>> = BTreeMap::new();
            for row in rows {
                let k: Vec<Value> = by.iter().map(|c| row.get(c).cloned().unwrap_or(Value::Null)).collect();
                groups.entry(k).or_default().push(row.into_value());
            }
            for (k, members) in groups {
                let key_row = Cand::Group(Value::Struct(Box::new(by.iter().cloned().zip(k.iter().cloned()).collect())));
                out.push(entry(sch, plan, base, scope, st, k, key_row, Some(Value::from(members)))?);
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
    Ok((tbl, fetch(st, plan.table(), filter.as_ref())))
}

// The rows a filter admits, in key order, through whatever of the store's
// indexes serve it: its equalities and spans (R1, R6) and its text
// searches (`docs/plan-db.md` D4) — a store with a text index on a
// searched column reads the rows holding every trigram of the needle and
// never the rest.
fn fetch(st: &dyn Store, table: &str, filter: Option<&Filter>) -> Vec<Row> {
    let (eq, sp) = (equalities(filter), spans(filter));
    let keep = |r: &Row| admits(filter, r);
    match needles(filter) {
        None => st.scan_where_eq(table, &eq, &sp, &keep),
        Some(has) => st.scan_where_text(table, &eq, &sp, &has, &keep),
    }
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
    row: Cand,
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
            node: row.into_value(),
            held: HeldMap::new(),
            members: vec![],
        });
    }
    // The row is bound by reference — the entry keeps it for its order
    // keys — and a node that projects never builds the default struct of
    // the row and its related lists, so a node's own reads copy only what
    // they read (`docs/plan-perf.md` R5).
    let mut node = scope.node();
    if let Some(x) = plan.row {
        row.bind(&mut node, x);
    }
    if let (Some(x), Some(m)) = (plan.members, members) {
        node.bind(x, m);
    }
    let mut id = base;
    for l in &plan.lookups {
        let k = l.key.iter().map(|e| node.eval(e)).collect::<Result<Vec<_>, _>>()?;
        look_up(&mut node, l, st, k, id, &mut deps);
        id += 1;
    }
    let mut fields: Option<BTreeMap<FieldName, Value>> = plan.project.is_none().then(|| row.fields());
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
        let list = Value::from(answer_owned(&r.plan, kids));
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
        (None, true) => Value::from(fields.unwrap_or_default()),
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
        held: HeldMap::new(),
        members: vec![],
    })
}

// What an entry is built over: a row of the plan's table, bound as the
// store holds it (`docs/plan-perf.md` R11), or a group's key as the struct
// of its `by` columns.
enum Cand {
    Row(Row),
    Group(Value),
}

impl Cand {
    fn bind<'s>(&'s self, node: &mut NodeScope<'s>, x: Sym) {
        match self {
            Cand::Row(r) => node.bind_row(x, r),
            Cand::Group(v) => node.bind_ref(x, v),
        }
    }

    // A column, as `Value::field` reads one: fatal when it is missing.
    fn field(&self, c: &str) -> Value {
        match self {
            Cand::Row(r) => r
                .get(c)
                .cloned()
                .unwrap_or_else(|| panic!("field: no field {c:?} in {r:?} (a bug: a verified module never mismatches)")),
            Cand::Group(v) => v.field(c),
        }
    }

    // The default node's own fields: every column, or the group's key.
    fn fields(&self) -> BTreeMap<FieldName, Value> {
        match self {
            Cand::Row(r) => r.to_struct(),
            Cand::Group(Value::Struct(m)) => (**m).clone(),
            Cand::Group(_) => BTreeMap::new(),
        }
    }

    fn into_value(self) -> Value {
        match self {
            Cand::Row(r) => r.into_value(),
            Cand::Group(v) => v,
        }
    }
}

// A lookup, bound: the row under its key as the store holds it, or `Null`
// — read, and recorded as a dependency, only when no part of the key is
// `Null`.
fn look_up(node: &mut NodeScope, l: &Lookup, st: &dyn Store, k: Vec<Value>, id: NodeId, deps: &mut Vec<(NodeId, Value)>) {
    if k.iter().any(Value::is_null) {
        node.bind(l.sym, Value::Null);
        return;
    }
    match st.get(&l.table, &k) {
        Some(r) => node.bind_row_owned(l.sym, r),
        None => node.bind(l.sym, Value::Null),
    }
    deps.push((id, Value::from(k)));
}

fn order_key(k: &Key, row: &Cand, node: Option<&NodeScope>) -> Result<Value, EvalFault> {
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
    /// D4 The needle, folded once for the read ([`store::fold`]); `None`
    /// for a needle that is not text, which a verified module never has
    /// and which admits nothing.
    Has(FieldName, Option<String>),
}

fn eval_pred<E>(p: &Pred, ev: &mut dyn FnMut(&Expr) -> Result<Value, E>) -> Result<Filter, E> {
    Ok(match p {
        Pred::Cmp(c, op, e) => Filter::Cmp(c.clone(), *op, ev(e)?),
        Pred::In(c, es) => Filter::In(c.clone(), es.iter().map(&mut *ev).collect::<Result<_, _>>()?),
        Pred::All(ps) => Filter::All(ps.iter().map(|q| eval_pred(q, ev)).collect::<Result<_, _>>()?),
        Pred::Any(ps) => Filter::Any(ps.iter().map(|q| eval_pred(q, ev)).collect::<Result<_, _>>()?),
        Pred::Not(q) => Filter::Not(Box::new(eval_pred(q, ev)?)),
        Pred::Has(c, e) => Filter::Has(
            c.clone(),
            match ev(e)? {
                Value::Text(t) => Some(store::fold(&t)),
                _ => None,
            },
        ),
        // A rule's leaves, which a verified plan never has
        // (`RuleLeafInPlan`): admitting nothing is the reading that cannot
        // widen what a read returns.
        Pred::Role(_) | Pred::Exists(..) => Filter::Any(vec![]),
    })
}

// D4 The text searches a filter holds however it is satisfied, as a
// disjunction of conjunctions of `(column, folded needle)`: a row the
// filter admits holds every needle of at least one branch. A `Has` is one
// branch of one; an `All` is the product of what its members say (a member
// that says nothing — a comparison, a `Not` — restricts nothing and is
// left to `keep`); an `Any` is the branches of its members, and nothing
// when one of them says nothing. `None` is no restriction at all. What a
// text index serves ([`Store::scan_where_text`]): harken's search is a
// title *or* a creator, two branches. Past sixteen branches it says
// nothing, which is only a wider read.
fn needles(f: Option<&Filter>) -> Option<Vec<Vec<(&str, &str)>>> {
    const MOST: usize = 16;
    fn go(f: &Filter) -> Option<Vec<Vec<(&str, &str)>>> {
        match f {
            Filter::Has(c, Some(n)) => Some(vec![vec![(c.as_str(), n.as_str())]]),
            Filter::All(fs) => {
                let mut out: Option<Vec<Vec<(&str, &str)>>> = None;
                for g in fs.iter().filter_map(go) {
                    out = Some(match out {
                        None => g,
                        Some(bs) => {
                            let prod: Vec<_> = bs
                                .iter()
                                .flat_map(|b| g.iter().map(move |h| b.iter().chain(h).copied().collect()))
                                .collect();
                            if prod.len() > MOST {
                                return Some(bs);
                            }
                            prod
                        }
                    });
                }
                out
            }
            Filter::Any(fs) => {
                let bs: Vec<_> = fs.iter().map(go).collect::<Option<Vec<_>>>()?.into_iter().flatten().collect();
                (bs.len() <= MOST).then_some(bs)
            }
            _ => None,
        }
    }
    f.and_then(go).filter(|bs| !bs.is_empty())
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
            // D4 Both sides folded by the pinned `lower`; a `Null` holds
            // nothing, and every text the empty needle.
            Filter::Has(c, n) => match (field(c), n) {
                (Value::Text(_), Some(n)) if n.is_empty() => true,
                (Value::Text(s), Some(n)) => store::fold(s).contains(n.as_str()),
                _ => false,
            },
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

// §1.13, R9 Aggregates a view keeps as numbers ------------------------------

/// `docs/plan-perf.md` R9: an aggregate a plan node's expressions take of
/// one of its lists — a related list, or a group's `members` — recognised
/// from the expressions rather than declared: `len(list)` is a
/// [`Agg::Count`], and `fold(list, init, |acc, x| acc + f(x))` with `f`
/// mentioning nothing but `x` is an [`Agg::Sum`] of `f`. A helper whose
/// whole body is one of the two over its one parameter (harken's `total`)
/// is the same use. Integer addition is associative and commutative, so a
/// sum is the same number whichever order its terms arrive in, and a view
/// can keep it by adding and subtracting terms (§1.13).
///
/// `docs/plan-db.md` D4 adds the extremes. A least or greatest value is not
/// a group under subtraction — a departure of the extreme says nothing of
/// what is next — so a view keeps one only where the store can answer that
/// question in one indexed read ([`Store::scan_ordered`], limit 1): an
/// arrival compares, a departure of the extreme reads it again, an edit is
/// both. Where no index serves the column the list stays a list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Agg {
    /// How many admitted nodes the list holds.
    Count,
    /// `init` plus the sum of `f` over the list's nodes, `x` being each
    /// node in `f`; `init` reads no binder, so the number is a function of
    /// the list alone. Kept as an integer of any size and judged only when
    /// a node reads it (§13, `docs/plan-db.md` D4).
    Sum { x: Sym, f: Expr, init: Expr },
    /// D4 The least value of a column over the list's rows, `Null` when the
    /// list is empty: `fold(list, init, |acc, x| min(acc, x.col))`, or
    /// `first` of a list ordered by the column ascending (`last`, descending)
    /// whose element is read for that column alone — ties then cannot tell
    /// two rows apart, which is what lets the value stand for the row.
    Min(FieldName),
    /// D4 The greatest: `max` as the step, or `first` of a list ordered by
    /// the column descending (`last`, ascending).
    Max(FieldName),
}

impl Agg {
    // The column and the direction an extreme is read in from an index.
    fn extreme(&self) -> Option<(&str, Dir)> {
        match self {
            Agg::Min(c) => Some((c, Dir::Asc)),
            Agg::Max(c) => Some((c, Dir::Desc)),
            Agg::Count | Agg::Sum { .. } => None,
        }
    }
}

// How one aggregate use reads its number, which is how it is rewritten.
enum Use<'e> {
    // `len(list)` (no `init`), or a sum's fold (`init + number`).
    Plain(Option<&'e Expr>),
    // D4 `fold(list, init, |acc, x| min(acc, x.c))`, or `max`: `init` when
    // the list is empty, else the step over `init` and the extreme.
    Fold(&'e Expr, StdFn),
    // D4 `match first(list) { y => some, None => none }` reading `y.col`
    // alone: the extreme bound to `y`, its column reads `y` itself.
    Pick {
        y: Sym,
        col: FieldName,
        some: &'e Expr,
        none: &'e Expr,
    },
}

/// R9 One plan node's expressions as a view evaluates them: the `having`,
/// the projection and the expression order keys, with every aggregate use
/// of a maintained list replaced by a number the view keeps — a `Count` by
/// its slot, a `Sum` by `init + slot`. A slot is a symbol no plan binds
/// (negative; a plan's are the `n`th binding of a function, from 0).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Face {
    pub having: Option<Expr>,
    pub project: Option<Expr>,
    /// One per order key: the rewritten expression of a [`Key::Expr`],
    /// `None` for a column.
    pub order: Vec<Option<Expr>>,
    /// A group source's `members`, when every use of it is an aggregate:
    /// each aggregate and its slot.
    pub members: Option<Vec<(Agg, Sym)>>,
    /// The node id of each related plan, in order.
    pub ids: Vec<NodeId>,
    /// Whether each related plan is kept as numbers ([`Shape::kept`]).
    pub kept: Vec<bool>,
}

/// R9 A related plan whose list is kept as numbers: the aggregates its
/// parent takes of it, each with its slot, and the face of the child plan.
/// The child plan is itself all numbers — no lookups, no limit, a table
/// source, and every related plan of it kept — so a node of it is a
/// function of its row and of the numbers beneath it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kept {
    pub aggs: Vec<(Agg, Sym)>,
    pub face: Face,
}

/// R9 What a view keeps as numbers, derived from the plan and the
/// function's helpers when it hydrates: nothing travels, so no frame and
/// no vector changes. [`Shape::root`] is the root plan's face, and
/// [`Shape::kept`] every related plan kept as numbers, by node id. A
/// related plan is kept when every use of its list in its parent is an
/// [`Agg`], its child plan is all numbers, and its parent is the root or
/// itself kept; anything else is pulled as a list, as before.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shape {
    pub root: Face,
    pub kept: BTreeMap<NodeId, Kept>,
}

/// R9 The shape of a plan in a function whose helpers are `helpers`, over
/// a schema — which says, for an extreme (D4), whether its column can be
/// `Null` and whether an index serves it.
pub fn shape(sch: &Schema, plan: &Plan, helpers: &[Function]) -> Shape {
    let mut next: Sym = -1;
    let (root, kept) = face(sch, plan, 0, true, helpers, &mut next).unwrap_or_default();
    Shape { root, kept }
}

// D4 The one column a list is ordered by, when it is: no limit, and an
// order of that column followed by nothing but the key columns ascending in
// key order (all of them, as the verifier completes it, or some first few:
// ties fall to the key either way). What `first` and `last` of it are an
// extreme of.
fn one_column_order(sch: &Schema, c: &Plan) -> Option<(FieldName, Dir)> {
    let tbl = sch.lookup_table(c.table())?;
    let ((Key::Column(c0), d), rest) = c.order.split_first()? else {
        return None;
    };
    if c.limit.is_some() {
        return None;
    }
    let keys = tbl.key.iter().filter(|k| *k != c0);
    let ok = rest.len() <= keys.clone().count()
        && rest
            .iter()
            .zip(keys)
            .all(|((k, d), want)| *d == Dir::Asc && *k == Key::Column(want.clone()));
    ok.then(|| (c0.clone(), *d))
}

// D4 The columns a plan's filter holds equal however it is satisfied — its
// top-level `Cmp(_, Eq, _)`s and those inside a top-level `All` — read off
// the plan rather than evaluated: what [`equalities`] will hand the store.
fn filter_eqs(p: Option<&Pred>) -> Vec<&str> {
    fn go<'p>(p: &'p Pred, out: &mut Vec<&'p str>) {
        match p {
            Pred::Cmp(c, CmpOp::Eq, _) => out.push(c),
            Pred::All(ps) => ps.iter().for_each(|q| go(q, out)),
            _ => {}
        }
    }
    let mut out = vec![];
    if let Some(p) = p {
        go(p, &mut out);
    }
    out
}

// D4 Whether a store keeping the schema's indexes reads the rows `eq` holds
// equal in `col`'s order through one of them — `MemoryStore`'s rule
// (`Secondary::serves`) for an order of one column: an index whose leading
// columns are exactly the held ones and whose next and last is `col`, or —
// ascending only — one of exactly the held ones with `col` the first key
// column they leave free. The indexes are each declared plain or unique
// index and each reference column; a text index orders nothing.
fn served(tbl: &Table, eq: &[&str], col: &str, dir: Dir) -> bool {
    let lists = tbl
        .indexes
        .iter()
        .map(|i| i.columns.clone())
        .chain(tbl.refs.iter().map(|r| vec![r.column.clone()]))
        .filter(|cols| !cols.is_empty() && *cols != tbl.key);
    let held = |c: &str| eq.contains(&c);
    for cols in lists {
        let n = cols.iter().take_while(|c| held(c)).count();
        let (lead, rest) = cols.split_at(n);
        if !eq.iter().all(|c| lead.iter().any(|x| x == c)) || held(col) {
            continue;
        }
        match rest {
            [c] if c == col => return true,
            [] if dir == Dir::Asc && tbl.key.iter().find(|k| !held(k)).is_some_and(|k| k == col) => return true,
            _ => {}
        }
    }
    false
}

// D4 Whether every extreme among `aggs` can be kept over `p`'s rows with
// `pinned` held equal: the column is the row's (a child plan whose node is
// its row — no projection, having or lists of its own), never `Null` (a
// `Null` would read as an empty list), and an index serves it in its
// direction (a departure of the extreme is then one read, not a scan).
fn extremes_kept(sch: &Schema, p: &Plan, pinned: &[&str], aggs: &[Agg], child: bool) -> bool {
    if !aggs.iter().any(|a| a.extreme().is_some()) {
        return true;
    }
    let Some(tbl) = sch.lookup_table(p.table()) else { return false };
    if child && (p.project.is_some() || p.having.is_some() || !p.related.is_empty()) {
        return false;
    }
    let mut eq: Vec<&str> = pinned.to_vec();
    eq.extend(filter_eqs(p.filter.as_ref()));
    aggs.iter()
        .filter_map(Agg::extreme)
        .all(|(c, d)| tbl.column(c).is_some_and(|col| !col.nullable) && served(tbl, &eq, c, d))
}

// A plan's face, and the kept plans beneath it; `None` when a child plan
// (`root` false) cannot be all numbers.
fn face(sch: &Schema, p: &Plan, base: NodeId, root: bool, helpers: &[Function], next: &mut Sym) -> Option<(Face, BTreeMap<NodeId, Kept>)> {
    if !root && (!p.lookups.is_empty() || p.limit.is_some() || p.members.is_some() || !matches!(p.source, Source::Table(_))) {
        return None;
    }
    let mut ids = Vec::with_capacity(p.related.len());
    let mut id = base + p.lookups.len();
    for r in &p.related {
        ids.push(id);
        id += 1 + node_count(&r.plan);
    }
    // Pass one: every use of every list, over every expression the node
    // computes after its reads.
    let mut cands: BTreeSet<Sym> = p.related.iter().map(|r| r.sym).collect();
    if let (Some(m), true) = (p.members, root) {
        cands.insert(m);
    }
    let mut uses: BTreeMap<Sym, Option<Vec<Agg>>> = cands.iter().map(|s| (*s, Some(vec![]))).collect();
    let orders: BTreeMap<Sym, (FieldName, Dir)> = p
        .related
        .iter()
        .filter_map(|r| one_column_order(sch, &r.plan).map(|o| (r.sym, o)))
        .collect();
    let rec = Rec { helpers, orders: &orders };
    let exprs = p.having.iter().chain(p.project.iter()).chain(p.order.iter().filter_map(|(k, _)| match k {
        Key::Expr(e) => Some(e),
        Key::Column(_) => None,
    }));
    for e in exprs {
        scan(e, &cands, &rec, &mut uses);
    }
    // D4 An extreme no index serves leaves its list a list.
    for r in &p.related {
        if let Some(Some(aggs)) = uses.get(&r.sym) {
            let pinned: Vec<&str> = r.on.iter().map(|(c, _)| c.as_str()).collect();
            if !extremes_kept(sch, &r.plan, &pinned, aggs, true) {
                uses.insert(r.sym, None);
            }
        }
    }
    if let (Some(m), true, Source::Group { by, .. }) = (p.members, root, &p.source) {
        if let Some(Some(aggs)) = uses.get(&m) {
            let pinned: Vec<&str> = by.iter().map(|c| c.as_str()).collect();
            if !extremes_kept(sch, p, &pinned, aggs, false) {
                uses.insert(m, None);
            }
        }
    }
    // The default node carries every related list whole.
    if p.project.is_none() {
        for r in &p.related {
            uses.insert(r.sym, None);
        }
    }
    // Members read before the numbers are bound (a lookup's key, an `on`)
    // are a list.
    if let Some(m) = p.members {
        let early = p
            .lookups
            .iter()
            .flat_map(|l| l.key.iter())
            .chain(p.related.iter().flat_map(|r| r.on.iter().map(|(_, e)| e)));
        if early.into_iter().any(|e| mentions(e, m)) {
            uses.insert(m, None);
        }
    }
    let mut kept = BTreeMap::new();
    let mut flags = Vec::with_capacity(p.related.len());
    let mut slots: BTreeMap<Sym, Vec<(Agg, Sym)>> = BTreeMap::new();
    // Each aggregate a slot of its own, from the one counter; a sum two
    // (its number as a pair of `i64`s), an extreme two (the second the
    // binder its fold's rewrite matches the first by).
    fn assign(aggs: &[Agg], next: &mut Sym) -> Vec<(Agg, Sym)> {
        aggs.iter()
            .map(|a| {
                let s = *next;
                *next -= if matches!(a, Agg::Count) { 1 } else { 2 };
                (a.clone(), s)
            })
            .collect()
    }
    for (r, id) in p.related.iter().zip(&ids) {
        let child = match &uses[&r.sym] {
            Some(_) => face(sch, &r.plan, id + 1, false, helpers, next),
            None => None,
        };
        match child {
            Some((f, below)) => {
                let aggs = assign(uses[&r.sym].as_deref().unwrap_or_default(), next);
                slots.insert(r.sym, aggs.clone());
                kept.extend(below);
                kept.insert(*id, Kept { aggs, face: f });
                flags.push(true);
            }
            None if !root => return None,
            None => flags.push(false),
        }
    }
    let members = match (p.members, root) {
        (Some(m), true) => uses[&m].as_deref().map(|aggs| {
            let a = assign(aggs, next);
            slots.insert(m, a.clone());
            a
        }),
        _ => None,
    };
    let rw = |e: &Expr| rewrite(e, &slots, &rec);
    Some((
        Face {
            having: p.having.as_ref().map(rw),
            project: p.project.as_ref().map(rw),
            order: p
                .order
                .iter()
                .map(|(k, _)| match k {
                    Key::Expr(e) => Some(rw(e)),
                    Key::Column(_) => None,
                })
                .collect(),
            members,
            ids,
            kept: flags,
        },
        kept,
    ))
}

// What recognising an aggregate needs beyond the expression: the function's
// helpers, and the one column each related list is ordered by (D4).
struct Rec<'a> {
    helpers: &'a [Function],
    orders: &'a BTreeMap<Sym, (FieldName, Dir)>,
}

// One use of a list that is an aggregate: the list's symbol, the aggregate,
// and how the use reads it ([`Use`]).
fn aggregate_use<'e>(e: &'e Expr, cands: &BTreeSet<Sym>, rec: &Rec<'e>) -> Option<(Sym, Agg, Use<'e>)> {
    match e {
        Expr::Std(StdFn::Len, es) => match es.as_slice() {
            [Expr::Var(s)] if cands.contains(s) => Some((*s, Agg::Count, Use::Plain(None))),
            _ => None,
        },
        Expr::Fold(xs, init, acc, x, body) => match &**xs {
            Expr::Var(s) if cands.contains(s) => {
                if let Some((g, c)) = extreme_step(body, *acc, *x) {
                    let agg = if g == StdFn::Min { Agg::Min(c.into()) } else { Agg::Max(c.into()) };
                    return Some((*s, agg, Use::Fold(init, g)));
                }
                let f = step(body, *acc)?;
                (closed(f, &[*x], false) && closed(init, &[], false)).then(|| {
                    (
                        *s,
                        Agg::Sum {
                            x: *x,
                            f: f.clone(),
                            init: (**init).clone(),
                        },
                        Use::Plain(Some(&**init)),
                    )
                })
            }
            _ => None,
        },
        // D4 `first`/`last` of a list ordered by one column, read for that
        // column alone: the least or greatest of it.
        Expr::Match(o, y, some, none) => match &**o {
            Expr::Std(g @ (StdFn::First | StdFn::Last), es) => match es.as_slice() {
                [Expr::Var(s)] if cands.contains(s) => {
                    let (c, d) = rec.orders.get(s)?;
                    if !only_field(some, *y, c) {
                        return None;
                    }
                    let least = (*d == Dir::Asc) == (*g == StdFn::First);
                    let agg = if least { Agg::Min(c.clone()) } else { Agg::Max(c.clone()) };
                    Some((
                        *s,
                        agg,
                        Use::Pick {
                            y: *y,
                            col: c.clone(),
                            some,
                            none,
                        },
                    ))
                }
                _ => None,
            },
            _ => None,
        },
        Expr::Call(name, es) => match es.as_slice() {
            [Expr::Var(s)] if cands.contains(s) => {
                let h = rec.helpers.iter().find(|h| h.name == *name && h.kind == FnKind::Helper)?;
                let (agg, init) = helper_agg(h)?;
                Some((*s, agg, Use::Plain(init)))
            }
            _ => None,
        },
        _ => None,
    }
}

// D4 A fold's step that is `min(acc, x.c)` or `max(acc, x.c)`, either way
// round: which, and the column.
fn extreme_step(body: &Expr, acc: Sym, x: Sym) -> Option<(StdFn, &str)> {
    let Expr::Std(g @ (StdFn::Min | StdFn::Max), es) = body else {
        return None;
    };
    match es.as_slice() {
        [Expr::Var(a), Expr::Field(v, c)] | [Expr::Field(v, c), Expr::Var(a)] if *a == acc && **v == Expr::Var(x) => Some((*g, c.as_str())),
        _ => None,
    }
}

// D4 Whether every occurrence of `y` in `e` is `y.c`.
fn only_field(e: &Expr, y: Sym, c: &str) -> bool {
    match e {
        Expr::Field(v, f) if **v == Expr::Var(y) => f == c,
        Expr::Var(s) => *s != y,
        _ => children(e).into_iter().all(|k| only_field(k, y, c)),
    }
}

// A helper whose whole body is an aggregate of its one parameter: `len` of
// it, or a fold of it whose `init` and step read nothing else — so the
// call is that aggregate wherever it is made.
fn helper_agg(h: &Function) -> Option<(Agg, Option<&Expr>)> {
    let [(p, _)] = h.input.as_slice() else { return None };
    let [Stmt::Return(Some(b))] = h.body.as_slice() else { return None };
    let is_p = |e: &Expr| matches!(e, Expr::Arg(a) if a == p);
    match b {
        Expr::Std(StdFn::Len, es) if es.len() == 1 && is_p(&es[0]) => Some((Agg::Count, None)),
        Expr::Fold(xs, init, acc, x, body) if is_p(xs) => {
            let f = step(body, *acc)?;
            (closed(f, &[*x], true) && closed(init, &[], true)).then(|| {
                (
                    Agg::Sum {
                        x: *x,
                        f: f.clone(),
                        init: (**init).clone(),
                    },
                    Some(&**init),
                )
            })
        }
        _ => None,
    }
}

// The `f` of a fold's step `acc + f` (or `f + acc`), when `f` does not
// read the accumulator: the evaluator's own test of a sum
// ([`crate::eval::sum_step`]), so that what a view keeps as a number is
// what a fresh read sums wide.
fn step(body: &Expr, acc: Sym) -> Option<&Expr> {
    crate::eval::sum_step(body, acc)
}

// Pass one: note each use of a candidate list — an aggregate, or anything
// else, which makes it a list (`None`).
fn scan(e: &Expr, cands: &BTreeSet<Sym>, rec: &Rec, uses: &mut BTreeMap<Sym, Option<Vec<Agg>>>) {
    if let Some((s, agg, u)) = aggregate_use(e, cands, rec) {
        if let Some(Some(aggs)) = uses.get_mut(&s) {
            if !aggs.contains(&agg) {
                aggs.push(agg);
            }
        }
        match u {
            Use::Plain(None) => {}
            Use::Plain(Some(init)) | Use::Fold(init, _) => scan(init, cands, rec, uses),
            Use::Pick { some, none, .. } => {
                scan(some, cands, rec, uses);
                scan(none, cands, rec, uses);
            }
        }
        return;
    }
    if let Expr::Var(s) = e {
        if cands.contains(s) {
            uses.insert(*s, None);
        }
    }
    for c in children(e) {
        scan(c, cands, rec, uses);
    }
}

// Pass two: each aggregate use of a kept list replaced by its number — a
// count by its slot, a sum by `init + slot`, an extreme's fold by `init`
// or the step over `init` and the slot, an extreme's pick by a match on
// the slot whose `y.col` is `y` (D4).
fn rewrite(e: &Expr, slots: &BTreeMap<Sym, Vec<(Agg, Sym)>>, rec: &Rec) -> Expr {
    let kept: BTreeSet<Sym> = slots.keys().copied().collect();
    map_expr(e, &mut |x: &Expr| {
        let (s, agg, u) = aggregate_use(x, &kept, rec)?;
        let slot = slots[&s].iter().find(|(a, _)| *a == agg).map(|(_, s)| *s)?;
        let v = |s: Sym| Box::new(Expr::Var(s));
        Some(match u {
            Use::Plain(None) => Expr::Var(slot),
            // The sum's `init` is in its number; the number is two slots
            // whose checked addition overflows exactly when it is not an
            // `i64` (§13: a sum is judged on its result alone).
            Use::Plain(Some(_)) => Expr::Op(Op::Add, vec![Expr::Var(slot), Expr::Var(slot - 1)]),
            Use::Fold(init, g) => {
                let init = rewrite(init, slots, rec);
                let y = slot - 1;
                Expr::Match(v(slot), y, Box::new(Expr::Std(g, vec![init.clone(), Expr::Var(y)])), Box::new(init))
            }
            Use::Pick { y, col, some, none } => {
                let some = map_expr(some, &mut |z: &Expr| match z {
                    Expr::Field(w, c) if **w == Expr::Var(y) && *c == col => Some(Expr::Var(y)),
                    _ => None,
                });
                Expr::Match(v(slot), y, Box::new(rewrite(&some, slots, rec)), Box::new(rewrite(none, slots, rec)))
            }
        })
    })
}

// Whether `x` occurs free in `e`.
fn mentions(e: &Expr, x: Sym) -> bool {
    match e {
        Expr::Var(s) => *s == x,
        _ => children(e).into_iter().any(|c| mentions(c, x)),
    }
}

// Whether every symbol free in `e` is one of `allowed` and nothing is
// read; `strict` also refuses what only a procedure binds (an argument,
// an auto, a provided value), which a helper's body cannot see.
fn closed(e: &Expr, allowed: &[Sym], strict: bool) -> bool {
    fn go(e: &Expr, bound: &mut Vec<Sym>, strict: bool) -> bool {
        match e {
            Expr::Var(s) => bound.contains(s),
            Expr::Arg(_) | Expr::Auto(_) | Expr::Provided(_) => !strict,
            Expr::Select(_) | Expr::Get(..) | Expr::Exists(..) => false,
            Expr::Match(v, x, some, none) => {
                if !go(v, bound, strict) || !go(none, bound, strict) {
                    return false;
                }
                bound.push(*x);
                let ok = go(some, bound, strict);
                bound.pop();
                ok
            }
            Expr::Map(xs, x, b) | Expr::Filter(xs, x, b) | Expr::Any(xs, x, b) | Expr::All(xs, x, b) | Expr::SortBy(xs, x, b) => {
                if !go(xs, bound, strict) {
                    return false;
                }
                bound.push(*x);
                let ok = go(b, bound, strict);
                bound.pop();
                ok
            }
            Expr::Fold(xs, z, acc, x, b) => {
                if !go(xs, bound, strict) || !go(z, bound, strict) {
                    return false;
                }
                bound.push(*acc);
                bound.push(*x);
                let ok = go(b, bound, strict);
                bound.truncate(bound.len() - 2);
                ok
            }
            _ => children(e).into_iter().all(|c| go(c, bound, strict)),
        }
    }
    go(e, &mut allowed.to_vec(), strict)
}

// The expressions directly inside one, binders' bodies included; a plan
// inside a `Select` is not looked into (a node's expressions have none).
pub(crate) fn children(e: &Expr) -> Vec<&Expr> {
    match e {
        Expr::Lit(_)
        | Expr::Arg(_)
        | Expr::Auto(_)
        | Expr::Var(_)
        | Expr::CtxUser
        | Expr::CtxSession
        | Expr::Provided(_)
        | Expr::None(_)
        | Expr::Select(_) => vec![],
        Expr::Field(x, _) | Expr::Some(x) => vec![x],
        Expr::Struct(fs) => fs.values().collect(),
        Expr::List(es) | Expr::Op(_, es) | Expr::Call(_, es) | Expr::Std(_, es) | Expr::Get(_, es) | Expr::Exists(_, es) => es.iter().collect(),
        Expr::Match(v, _, a, b) => vec![v, a, b],
        Expr::If(c, a, b) => vec![c, a, b],
        Expr::Cmp(_, a, b) => vec![a, b],
        Expr::Map(xs, _, b) | Expr::Filter(xs, _, b) | Expr::Any(xs, _, b) | Expr::All(xs, _, b) | Expr::SortBy(xs, _, b) => vec![xs, b],
        Expr::Fold(xs, z, _, _, b) => vec![xs, z, b],
    }
}

// `e` with `f` applied top-down: where `f` answers, its answer replaces
// the subexpression; elsewhere the children are mapped.
fn map_expr(e: &Expr, f: &mut dyn FnMut(&Expr) -> Option<Expr>) -> Expr {
    if let Some(x) = f(e) {
        return x;
    }
    let mut m = |x: &Expr| Box::new(map_expr(x, f));
    match e {
        Expr::Lit(_)
        | Expr::Arg(_)
        | Expr::Auto(_)
        | Expr::Var(_)
        | Expr::CtxUser
        | Expr::CtxSession
        | Expr::Provided(_)
        | Expr::None(_)
        | Expr::Select(_) => e.clone(),
        Expr::Field(x, n) => Expr::Field(m(x), n.clone()),
        Expr::Some(x) => Expr::Some(m(x)),
        Expr::Struct(fs) => Expr::Struct(fs.iter().map(|(k, v)| (k.clone(), *m(v))).collect()),
        Expr::List(es) => Expr::List(es.iter().map(|x| *m(x)).collect()),
        Expr::Op(op, es) => Expr::Op(*op, es.iter().map(|x| *m(x)).collect()),
        Expr::Call(n, es) => Expr::Call(n.clone(), es.iter().map(|x| *m(x)).collect()),
        Expr::Std(g, es) => Expr::Std(*g, es.iter().map(|x| *m(x)).collect()),
        Expr::Get(t, es) => Expr::Get(t.clone(), es.iter().map(|x| *m(x)).collect()),
        Expr::Exists(t, es) => Expr::Exists(t.clone(), es.iter().map(|x| *m(x)).collect()),
        Expr::Match(v, x, a, b) => Expr::Match(m(v), *x, m(a), m(b)),
        Expr::If(c, a, b) => Expr::If(m(c), m(a), m(b)),
        Expr::Cmp(op, a, b) => Expr::Cmp(*op, m(a), m(b)),
        Expr::Map(xs, x, b) => Expr::Map(m(xs), *x, m(b)),
        Expr::Filter(xs, x, b) => Expr::Filter(m(xs), *x, m(b)),
        Expr::Any(xs, x, b) => Expr::Any(m(xs), *x, m(b)),
        Expr::All(xs, x, b) => Expr::All(m(xs), *x, m(b)),
        Expr::SortBy(xs, x, b) => Expr::SortBy(m(xs), *x, m(b)),
        Expr::Fold(xs, z, a, x, b) => Expr::Fold(m(xs), m(z), *a, *x, m(b)),
    }
}

// §1.13, R9 The numbers, kept ------------------------------------------------

/// R9 What one entry keeps of a kept related plan for one `(node,
/// dependency)`: the numbers its [`Kept::aggs`] come to over the child
/// nodes that dependency joins — which are a function of the dependency
/// alone, since a child plan sees nothing of its parent but its `on`
/// (§1.3, scope) — the child nodes themselves when the child plan has kept
/// lists of its own (their rows, so a node is re-evaluated without reading
/// it), and which parent nodes reference it. A child plan with no related
/// plans keeps no children: a row's term is computed from the row a change
/// carries, old and new.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Held {
    /// One per aggregate of the node, in [`Kept::aggs`]'s order: an `Int`
    /// for a count or a sum, the column's value for an extreme (D4), `Null`
    /// when there is none.
    pub nums: Vec<Num>,
    /// The child nodes, by key, when the child plan has related plans.
    pub kids: BTreeMap<Vec<Value>, Kid>,
    /// Who reads these numbers: `None` for the entry's own node, or a
    /// child node of a kept plan, as `(node, dependency, key)`.
    pub parents: BTreeSet<Parent>,
}

/// R9 A reader of a [`Held`]: the entry's node, or a [`Kid`] by where it is.
pub type Parent = Option<(NodeId, Value, Vec<Value>)>;

/// The numbers an entry keeps, by `(node, dependency)`.
pub type HeldMap = BTreeMap<(NodeId, Value), Held>;

/// R9 One child node of a kept related plan whose own lists are kept: its
/// row, whether its having admitted it, its node (`Null` when not), and
/// the dependency each of its related plans computed, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kid {
    pub row: Value,
    pub admitted: bool,
    pub node: Value,
    pub subs: Vec<Value>,
}

// The `(node, dependency)`s an entry's numbers came to hold or let go, in
// the order it happened: what `by_dep` is told.
type Ops = Vec<((NodeId, Value), bool)>;

// A group's members as the entry is built with them: the list, or its
// numbers.
enum Members {
    None,
    List(Value),
    Nums(Vec<Num>),
}

// Everything the kept numbers are computed against: the plan, its shape,
// the scope, the store, and each node by id; a kept child plan's own
// filter and the root's, evaluated once.
struct Cx<'a, 's> {
    sch: &'a Schema,
    plan: &'a Plan,
    shape: &'a Shape,
    scope: &'a Scope<'s>,
    st: &'a dyn Store,
    rel: Vec<Option<&'a Related>>,
    filters: BTreeMap<NodeId, Option<Filter>>,
    root_filter: Option<Filter>,
}

impl<'a, 's> Cx<'a, 's> {
    fn new(sch: &'a Schema, plan: &'a Plan, shape: &'a Shape, scope: &'a Scope<'s>, st: &'a dyn Store) -> Result<Cx<'a, 's>, EvalFault> {
        let rel = nodes(plan)
            .into_iter()
            .map(|(_, n)| match n {
                Node::Related(r) => Some(r),
                Node::Lookup(_) => None,
            })
            .collect::<Vec<_>>();
        let mut filters = BTreeMap::new();
        for id in shape.kept.keys() {
            let c = &rel[*id].expect("a kept node is a related plan").plan;
            filters.insert(*id, root_filter(c, scope)?);
        }
        Ok(Cx {
            sch,
            plan,
            shape,
            scope,
            st,
            rel,
            filters,
            root_filter: root_filter(plan, scope)?,
        })
    }

    fn related(&self, id: NodeId) -> &'a Related {
        self.rel[id].expect("a kept node is a related plan")
    }

    fn kept(&self, id: NodeId) -> &'a Kept {
        &self.shape.kept[&id]
    }

    fn table(&self, t: &TableName) -> Result<&'a Table, EvalFault> {
        self.sch.lookup_table(t).ok_or_else(|| EvalFault::Bug(EvalError::UnknownTable(t.clone())))
    }
}

fn int_of(v: Value) -> Result<i64, EvalFault> {
    match v {
        Value::Int(n) => Ok(n),
        other => Err(EvalFault::Bug(EvalError::TypeError(format!(
            "an aggregate's term is an Int, not {other:?}"
        )))),
    }
}

fn bool_of(v: Value) -> Result<bool, EvalFault> {
    match v {
        Value::Bool(b) => Ok(b),
        other => Err(EvalFault::Bug(EvalError::TypeError(format!("a having is a Bool, not {other:?}")))),
    }
}

/// What one aggregate a view keeps comes to (R9, `docs/plan-db.md` D4): a
/// count or a sum as an integer of any size — §13 judges a sum on its
/// result alone, so a total that passes outside an `i64` on the way, in
/// whatever order its terms arrive, is not an overflow — and an extreme as
/// the column's value, `Null` for none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Num {
    Int(i128),
    Val(Value),
}

// What `aggs` come to over no node: nothing counted, a sum's `init`, no
// extreme.
fn zero(cx: &Cx, aggs: &[(Agg, Sym)]) -> Result<Vec<Num>, EvalFault> {
    aggs.iter()
        .map(|(a, _)| match a {
            Agg::Count => Ok(Num::Int(0)),
            Agg::Sum { init, .. } => int_of(cx.scope.node().eval(init)?).map(|n| Num::Int(n.into())),
            Agg::Min(_) | Agg::Max(_) => Ok(Num::Val(Value::Null)),
        })
        .collect()
}

fn wide(n: &Num) -> Result<i128, EvalFault> {
    match n {
        Num::Int(n) => Ok(*n),
        Num::Val(v) => Err(EvalFault::Bug(EvalError::TypeError(format!("a count or a sum at {v:?}")))),
    }
}

// One admitted node's terms put in: added to a count or a sum, compared
// with an extreme (D4), which it replaces when it is beyond it. An `i128`
// does not overflow by adding `i64`s one at a time.
fn put_in(aggs: &[(Agg, Sym)], nums: &mut [Num], terms: &[Num]) -> Result<(), EvalFault> {
    for (((a, _), n), t) in aggs.iter().zip(nums.iter_mut()).zip(terms) {
        match (a.extreme(), n, t) {
            (None, n, t) => *n = Num::Int(wide(n)? + wide(t)?),
            (Some((_, d)), Num::Val(n), Num::Val(t)) => {
                let o = compare_value(t, n);
                if n.is_null() || o == if d == Dir::Asc { Ordering::Less } else { Ordering::Greater } {
                    *n = t.clone();
                }
            }
            (Some(_), n, t) => return Err(EvalFault::Bug(EvalError::TypeError(format!("an extreme {n:?} by {t:?}")))),
        }
    }
    Ok(())
}

// One admitted node's terms taken out: subtracted from a count or a sum.
// An extreme it was (or one already lost) cannot be moved — what is next is
// not in the terms — so the answer is `true`, and the caller reads it again
// from the store ([`extremes`]), which is already final: arrivals compared
// after that are compared against an answer that has them already, and
// comparing is idempotent (D4).
fn take_out(aggs: &[(Agg, Sym)], nums: &mut [Num], terms: &[Num]) -> Result<bool, EvalFault> {
    let mut stale = false;
    for (((a, _), n), t) in aggs.iter().zip(nums.iter_mut()).zip(terms) {
        match (a.extreme(), n) {
            (None, n) => *n = Num::Int(wide(n)? - wide(t)?),
            (Some(_), Num::Val(n)) => stale |= n.is_null() || Num::Val(n.clone()) == *t,
            (Some(_), n) => return Err(EvalFault::Bug(EvalError::TypeError(format!("an extreme at {n:?}")))),
        }
    }
    Ok(stale)
}

// The numbers bound for a node to read, at their slots: a count as itself;
// a sum as two `i64`s — the total clamped, and what is left clamped — whose
// checked sum, which is what the rewritten expression computes, is the
// total when it is an `i64` and the evaluator's overflow when it is not,
// exactly as a fresh `fold` judges it (§13); an extreme as its value.
fn bind_nums(node: &mut NodeScope, aggs: &[(Agg, Sym)], nums: &[Num]) {
    let clamp = |n: i128| n.clamp(i64::MIN.into(), i64::MAX.into()) as i64;
    for ((a, slot), n) in aggs.iter().zip(nums) {
        match (a, n) {
            (Agg::Count, Num::Int(n)) => node.bind(*slot, Value::Int(clamp(*n))),
            (Agg::Sum { .. }, Num::Int(n)) => {
                let first = clamp(*n);
                node.bind(*slot, Value::Int(first));
                node.bind(*slot - 1, Value::Int(clamp(*n - i128::from(first))));
            }
            (_, Num::Val(v)) => node.bind(*slot, v.clone()),
            (_, Num::Int(n)) => node.bind(*slot, Value::Int(clamp(*n))),
        }
    }
}

// A column of a node that is its row; `Null` where it has none.
fn column(node: &Value, c: &str) -> Value {
    match node {
        Value::Struct(m) => m.get(c).cloned().unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

// The terms one admitted node adds to the aggregates `aggs`: 1 to a count,
// `f(node)` to a sum, the column to an extreme (whose node is its row).
fn terms(cx: &Cx, aggs: &[(Agg, Sym)], node: &Value) -> Result<Vec<Num>, EvalFault> {
    aggs.iter()
        .map(|(a, _)| match a {
            Agg::Count => Ok(Num::Int(1)),
            Agg::Sum { x, f, .. } => {
                let mut n = cx.scope.node();
                n.bind_ref(*x, node);
                int_of(n.eval(f)?).map(|t| Num::Int(t.into()))
            }
            Agg::Min(c) | Agg::Max(c) => Ok(Num::Val(column(node, c))),
        })
        .collect()
}

// A related plan's pins for one dependency: each `on` column held to the
// value the parent computed for it.
fn pins_of(r: &Related, on: &Value) -> Vec<(FieldName, Value)> {
    match on {
        Value::List(vs) => r.on.iter().map(|(col, _)| col.clone()).zip(vs.iter().cloned()).collect(),
        _ => vec![],
    }
}

// D4 Every extreme among `aggs` read again from the store, which is final:
// the least or greatest value of its column over the rows `plan`'s filter
// and `pins` admit. That is the first row of an index that holds them in
// the column's order — [`Store::scan_ordered`], limit 1, one row examined
// past those `keep` refuses — and [`shape`] keeps an extreme only where the
// schema has such an index. A store that serves none has every candidate
// read, which is right and is the scan the index is there to spare.
fn extremes(cx: &Cx, plan: &Plan, pins: &[(FieldName, Value)], aggs: &[(Agg, Sym)], nums: &mut [Num]) -> Result<(), EvalFault> {
    if !aggs.iter().any(|(a, _)| a.extreme().is_some()) {
        return Ok(());
    }
    let (_, filter) = source(cx.sch, plan, pins, cx.scope)?;
    let (eq, sp) = (equalities(filter.as_ref()), spans(filter.as_ref()));
    let keep = |r: &Row| admits(filter.as_ref(), r);
    for ((a, _), n) in aggs.iter().zip(nums.iter_mut()) {
        let Some((c, d)) = a.extreme() else { continue };
        let row = match cx.st.scan_ordered(plan.table(), &eq, &sp, &[(c, d)], &keep, 1) {
            Some(rows) => rows.into_iter().next(),
            None => fetch(cx.st, plan.table(), filter.as_ref()).into_iter().reduce(|a, b| {
                let o = compare_value(&column_of(&b, c), &column_of(&a, c));
                if o == if d == Dir::Asc { Ordering::Less } else { Ordering::Greater } {
                    b
                } else {
                    a
                }
            }),
        };
        *n = Num::Val(row.map_or(Value::Null, |r| column_of(&r, c)));
    }
    Ok(())
}

fn column_of(r: &Row, c: &str) -> Value {
    r.get(c).cloned().unwrap_or(Value::Null)
}

// A child row of a kept plan with no related plans: its node, when its
// having admits it. The row's pins and filter are the caller's.
fn row_node(cx: &Cx, id: NodeId, row: &Value) -> Result<Option<Value>, EvalFault> {
    let c = &cx.related(id).plan;
    let face = &cx.kept(id).face;
    let mut node = cx.scope.node();
    if let Some(x) = c.row {
        node.bind_ref(x, row);
    }
    if let Some(h) = &face.having {
        if !bool_of(node.eval(h)?)? {
            return Ok(None);
        }
    }
    Ok(Some(match &face.project {
        Some(p) => node.eval(p)?,
        None => row.clone(),
    }))
}

// The terms a child row adds to its parent's numbers: none when the filter
// or the having refuses it.
fn row_terms(cx: &Cx, id: NodeId, row: &Row) -> Result<Option<Vec<Num>>, EvalFault> {
    if !admits(cx.filters[&id].as_ref(), row) {
        return Ok(None);
    }
    let v = row.to_value();
    match row_node(cx, id, &v)? {
        Some(node) => terms(cx, &cx.kept(id).aggs, &node).map(Some),
        None => Ok(None),
    }
}

// A child node of a kept plan whose lists are kept: whether its having
// admits it and its node, over its row and the numbers its dependencies
// hold now.
fn kid_node(cx: &Cx, id: NodeId, row: &Value, subs: &[Value], held: &HeldMap) -> Result<(bool, Value), EvalFault> {
    let c = &cx.related(id).plan;
    let face = &cx.kept(id).face;
    let mut node = cx.scope.node();
    if let Some(x) = c.row {
        node.bind_ref(x, row);
    }
    for (jid, on) in face.ids.iter().zip(subs) {
        bind_nums(&mut node, &cx.kept(*jid).aggs, &held[&(*jid, on.clone())].nums);
    }
    let admitted = match &face.having {
        None => true,
        Some(h) => bool_of(node.eval(h)?)?,
    };
    let value = match (&face.project, admitted) {
        (_, false) => Value::Null,
        (Some(p), true) => node.eval(p)?,
        // A kept child plan with related plans always projects (a default
        // node would carry its lists, which are then not kept).
        (None, true) => row.clone(),
    };
    Ok((admitted, value))
}

fn kid_terms(cx: &Cx, id: NodeId, kid: &Kid) -> Result<Option<Vec<Num>>, EvalFault> {
    if !kid.admitted {
        return Ok(None);
    }
    terms(cx, &cx.kept(id).aggs, &kid.node).map(Some)
}

// A child row of a kept plan whose lists are kept, as a node: the numbers
// of each of its dependencies held (pulled if no one holds them yet), and
// its having and node over them.
fn build_kid(cx: &Cx, id: NodeId, on: &Value, key: Vec<Value>, row: Value, held: &mut HeldMap, ops: &mut Ops) -> Result<Kid, EvalFault> {
    let c = &cx.related(id).plan;
    let face = &cx.kept(id).face;
    let mut subs = Vec::with_capacity(c.related.len());
    for (r, jid) in c.related.iter().zip(&face.ids) {
        let sub = {
            let mut node = cx.scope.node();
            if let Some(x) = c.row {
                node.bind_ref(x, &row);
            }
            Value::List(r.on.iter().map(|(_, e)| node.eval(e)).collect::<Result<_, _>>()?)
        };
        ensure(cx, *jid, sub.clone(), Some((id, on.clone(), key.clone())), held, ops)?;
        subs.push(sub);
    }
    let (admitted, node) = kid_node(cx, id, &row, &subs, held)?;
    Ok(Kid { row, admitted, node, subs })
}

// The numbers of `(id, on)`, read by `parent`: kept if some reader holds
// them already, pulled from the store otherwise — the child rows the
// dependency joins, through the store's indexes, each a term or a node.
fn ensure(cx: &Cx, id: NodeId, on: Value, parent: Parent, held: &mut HeldMap, ops: &mut Ops) -> Result<(), EvalFault> {
    let k = (id, on);
    if let Some(h) = held.get_mut(&k) {
        h.parents.insert(parent);
        return Ok(());
    }
    let r = cx.related(id);
    let c = &r.plan;
    let kept = cx.kept(id);
    let pins = pins_of(r, &k.1);
    let mut h = Held {
        nums: zero(cx, &kept.aggs)?,
        kids: BTreeMap::new(),
        parents: BTreeSet::from([parent]),
    };
    if c.related.is_empty() && kept.aggs.iter().all(|(a, _)| a.extreme().is_some()) {
        // D4 Extremes alone: one indexed read each, not the list.
        extremes(cx, c, &pins, &kept.aggs, &mut h.nums)?;
    } else {
        let (tbl, rows) = candidates(cx.sch, c, &pins, cx.scope, cx.st)?;
        for row in rows {
            if c.related.is_empty() {
                if let Some(node) = row_node(cx, id, &row.into_value())? {
                    put_in(&kept.aggs, &mut h.nums, &terms(cx, &kept.aggs, &node)?)?;
                }
            } else {
                let key = tbl.key_of(&row);
                let kid = build_kid(cx, id, &k.1, key.clone(), row.into_value(), held, ops)?;
                if let Some(t) = kid_terms(cx, id, &kid)? {
                    put_in(&kept.aggs, &mut h.nums, &t)?;
                }
                h.kids.insert(key, kid);
            }
        }
    }
    ops.push((k.clone(), true));
    held.insert(k, h);
    Ok(())
}

// `parent` stops reading `k`; the numbers nobody reads are let go, and the
// dependencies their children read with them.
fn release(cx: &Cx, k: (NodeId, Value), parent: &Parent, held: &mut HeldMap, ops: &mut Ops) {
    let Some(h) = held.get_mut(&k) else { return };
    h.parents.remove(parent);
    if !h.parents.is_empty() {
        return;
    }
    let Some(h) = held.remove(&k) else { return };
    let ids = &cx.kept(k.0).face.ids;
    for (key, kid) in h.kids {
        let me = Some((k.0, k.1.clone(), key));
        for (jid, sub) in ids.iter().zip(kid.subs) {
            release(cx, (*jid, sub), &me, held, ops);
        }
    }
    ops.push((k, false));
}

/// R9 A change to a table a kept plan reads, as one entry's numbers see
/// it: the node and dependency it hit, and the old row and the new one
/// where each satisfies that dependency.
struct Hit<'c> {
    id: NodeId,
    on: Value,
    old: Option<&'c Row>,
    new: Option<&'c Row>,
}

// What a held number is waiting for in a sweep: its numbers before it was
// first touched, the child rows to read again, the child nodes to
// evaluate again over numbers that moved beneath them.
#[derive(Default)]
struct Dirt {
    before: Option<Vec<Num>>,
    reread: BTreeSet<Vec<Value>>,
    reeval: BTreeSet<Vec<Value>>,
    /// D4 An extreme left with a row: read it again ([`extremes`]).
    stale: bool,
}

// §1.13, R9 An entry's kept numbers brought up to the store, from the hits
// its dependencies took — without pulling any list whole. A child plan
// with no related plans moves its parent's numbers by each hit's old and
// new row's terms (−old, +new), read from the change alone; one with
// related plans reads the rows the hits name again, by key, and builds
// those nodes over the numbers beneath them. Then, deepest first (a
// node's id is below every id beneath it, §1.8's pre-order), every number
// that moved re-evaluates the child nodes that read it — their rows are
// kept — and moves its own parent's numbers by the difference, up to the
// entry, whose node the caller rebuilds. A song arriving under a movement
// of a work of a composer is a count moved by one, two sums moved by one,
// and the composer's node evaluated again: the rows on that song's path,
// and not the composer's works, movements or songs.
fn sweep(cx: &Cx, held: &mut HeldMap, hits: &[Hit], ops: &mut Ops) -> Result<(), EvalFault> {
    // Boxed: a node of this map is then under a kilobyte, which glibc
    // serves from its small bins. Unboxed it was a larger request, and the
    // first such after a burst of frees pays for consolidating all of them
    // — 90 µs of `bench_views`' warm toggle on `playlists_of`, a view with
    // one hit to sweep.
    let mut dirty: BTreeMap<(NodeId, Value), Box<Dirt>> = BTreeMap::new();
    for hit in hits {
        let k = (hit.id, hit.on.clone());
        let Some(h) = held.get_mut(&k) else { continue };
        let d = dirty.entry(k).or_default();
        if d.before.is_none() {
            d.before = Some(h.nums.clone());
        }
        let c = &cx.related(hit.id).plan;
        if c.related.is_empty() {
            let aggs = &cx.kept(hit.id).aggs;
            if let Some(t) = hit.old.map(|r| row_terms(cx, hit.id, r)).transpose()?.flatten() {
                d.stale |= take_out(aggs, &mut h.nums, &t)?;
            }
            if let Some(t) = hit.new.map(|r| row_terms(cx, hit.id, r)).transpose()?.flatten() {
                put_in(aggs, &mut h.nums, &t)?;
            }
        } else {
            let tbl = cx.table(c.table())?;
            for row in hit.old.into_iter().chain(hit.new) {
                d.reread.insert(tbl.key_of(row));
            }
        }
    }
    while let Some((k, d)) = dirty.pop_last() {
        let Some(mut h) = held.remove(&k) else { continue };
        let before = d.before.unwrap_or_else(|| h.nums.clone());
        let (id, on) = (k.0, &k.1);
        let r = cx.related(id);
        let ids = &cx.kept(id).face.ids;
        if d.stale {
            extremes(cx, &r.plan, &pins_of(r, on), &cx.kept(id).aggs, &mut h.nums)?;
        }
        // A child plan with lists of its own keeps no extreme
        // (`extremes_kept`), so taking a child node out never leaves one
        // stale below.
        let aggs = &cx.kept(id).aggs;
        for key in &d.reread {
            let old = h.kids.remove(key);
            let row = cx
                .st
                .get(r.plan.table(), key)
                .filter(|row| admits(cx.filters[&id].as_ref(), row) && Node::Related(r).dependency(cx.sch, row) == *on);
            let new = match row {
                Some(row) => Some(build_kid(cx, id, on, key.clone(), row.into_value(), held, ops)?),
                None => None,
            };
            if let Some(o) = &old {
                let me = Some((id, on.clone(), key.clone()));
                for (j, (jid, sub)) in ids.iter().zip(&o.subs).enumerate() {
                    if new.as_ref().is_none_or(|n| n.subs[j] != *sub) {
                        release(cx, (*jid, sub.clone()), &me, held, ops);
                    }
                }
                if let Some(t) = kid_terms(cx, id, o)? {
                    take_out(aggs, &mut h.nums, &t)?;
                }
            }
            if let Some(n) = new {
                if let Some(t) = kid_terms(cx, id, &n)? {
                    put_in(aggs, &mut h.nums, &t)?;
                }
                h.kids.insert(key.clone(), n);
            }
        }
        for key in d.reeval.difference(&d.reread) {
            let Some(kid) = h.kids.get(key) else { continue };
            let (admitted, node) = kid_node(cx, id, &kid.row, &kid.subs, held)?;
            if admitted == kid.admitted && node == kid.node {
                continue;
            }
            let old = kid_terms(cx, id, kid)?;
            let kid = h.kids.get_mut(key).expect("just read");
            kid.admitted = admitted;
            kid.node = node;
            let new = kid_terms(cx, id, kid)?;
            if let Some(t) = old {
                take_out(aggs, &mut h.nums, &t)?;
            }
            if let Some(t) = new {
                put_in(aggs, &mut h.nums, &t)?;
            }
        }
        if h.nums != before {
            for p in h.parents.iter().flatten() {
                dirty.entry((p.0, p.1.clone())).or_default().reeval.insert(p.2.clone());
            }
        }
        held.insert(k, h);
    }
    Ok(())
}

// A group's members as numbers, from its rows: how many, each sum, each
// extreme.
fn member_nums(cx: &Cx, aggs: &[(Agg, Sym)], rows: &[Value]) -> Result<Vec<Num>, EvalFault> {
    let mut nums = zero(cx, aggs)?;
    for r in rows {
        put_in(aggs, &mut nums, &terms(cx, aggs, r)?)?;
    }
    Ok(nums)
}

// §1.5, 3 and R9 One candidate of the root plan, built: its lookups, its
// related plans — a kept one as the numbers `held` has for its
// dependency, pulled only when nobody holds them — its having, its node
// and its order keys, over the face's expressions. `held` is what the
// entry kept before (empty for a new one), already swept; a dependency the
// node no longer computes is let go.
fn entry_at(cx: &Cx, key: Vec<Value>, row: Cand, members: Members, mut held: HeldMap, ops: &mut Ops) -> Result<Entry, EvalFault> {
    let plan = cx.plan;
    let face = &cx.shape.root;
    let mut deps = Vec::new();
    // A bare plan binds nothing: its node is the row (as `entry`).
    if plan.is_bare() {
        let order = plan.order.iter().map(|(k, _)| order_key(k, &row, None)).collect::<Result<_, _>>()?;
        return Ok(Entry {
            key,
            deps,
            order,
            admitted: true,
            node: row.into_value(),
            held,
            members: vec![],
        });
    }
    let mut node = cx.scope.node();
    if let Some(x) = plan.row {
        row.bind(&mut node, x);
    }
    let mut nums = vec![];
    match members {
        Members::None => {}
        Members::List(m) => {
            if let Some(x) = plan.members {
                node.bind(x, m);
            }
        }
        Members::Nums(ns) => {
            bind_nums(&mut node, face.members.as_deref().unwrap_or_default(), &ns);
            nums = ns;
        }
    }
    let mut id = 0;
    for l in &plan.lookups {
        let k = l.key.iter().map(|e| node.eval(e)).collect::<Result<Vec<_>, _>>()?;
        look_up(&mut node, l, cx.st, k, id, &mut deps);
        id += 1;
    }
    let mut fields: Option<BTreeMap<FieldName, Value>> = plan.project.is_none().then(|| row.fields());
    for (j, r) in plan.related.iter().enumerate() {
        let on =
            r.on.iter()
                .map(|(c, e)| Ok((c.clone(), node.eval(e)?)))
                .collect::<Result<Vec<_>, EvalFault>>()?;
        if face.kept[j] {
            let dep = Value::List(on.into_iter().map(|(_, v)| v).collect());
            ensure(cx, id, dep.clone(), None, &mut held, ops)?;
            let stale: Vec<(NodeId, Value)> = held
                .range((id, Value::Null)..(id + 1, Value::Null))
                .map(|(k, _)| k.clone())
                .filter(|k| k.1 != dep)
                .collect();
            for k in stale {
                release(cx, k, &None, &mut held, ops);
            }
            bind_nums(&mut node, &cx.kept(id).aggs, &held[&(id, dep)].nums);
        } else {
            deps.push((id, Value::List(on.iter().map(|(_, v)| v.clone()).collect())));
            let mut kids = pull_at(cx.sch, &r.plan, id + 1, &on, cx.scope, cx.st)?;
            for k in &mut kids {
                deps.append(&mut k.deps);
            }
            let list = Value::from(answer_owned(&r.plan, kids));
            if let Some(fields) = &mut fields {
                fields.insert(r.name.clone(), list.clone());
            }
            node.bind(r.sym, list);
        }
        id += 1 + node_count(&r.plan);
    }
    let admitted = match &face.having {
        None => true,
        Some(h) => bool_of(node.eval(h)?)?,
    };
    let value = match (&face.project, admitted) {
        (_, false) => Value::Null,
        (Some(p), true) => node.eval(p)?,
        (None, true) => Value::from(fields.unwrap_or_default()),
    };
    let order = plan
        .order
        .iter()
        .zip(&face.order)
        .map(|((k, _), e)| match (k, e) {
            (Key::Column(c), _) => Ok(row.field(c)),
            (Key::Expr(_), Some(e)) => node.eval(e),
            (Key::Expr(_), None) => Err(EvalFault::Bug(EvalError::TypeError("an expression order key with no face".into()))),
        })
        .collect::<Result<_, _>>()?;
    Ok(Entry {
        key,
        deps,
        order,
        admitted,
        node: value,
        held,
        members: nums,
    })
}

// §1.5 and R9 Every candidate of the root plan, pulled with its shape: what
// a hydrate keeps.
fn pull_root(cx: &Cx) -> Result<Vec<Entry>, EvalFault> {
    let plan = cx.plan;
    let (tbl, mut rows) = candidates(cx.sch, plan, &[], cx.scope, cx.st)?;
    let mut out = Vec::new();
    let mut ops = Ops::new();
    match &plan.source {
        Source::Table(_) => {
            for row in rows {
                let key = tbl.key_of(&row);
                out.push(entry_at(cx, key, Cand::Row(row), Members::None, HeldMap::new(), &mut ops)?);
            }
        }
        Source::Group { by, .. } => {
            rows.sort_by_key(|r| tbl.key_of(r));
            let mut groups: BTreeMap<Vec<Value>, Vec<Value>> = BTreeMap::new();
            for row in rows {
                groups.entry(group_of(by, &row)).or_default().push(row.into_value());
            }
            for (k, members) in groups {
                let key_row = Cand::Group(Value::Struct(Box::new(by.iter().cloned().zip(k.iter().cloned()).collect())));
                let members = match &cx.shape.root.members {
                    Some(aggs) => Members::Nums(member_nums(cx, aggs, &members)?),
                    None => Members::List(Value::from(members)),
                };
                out.push(entry_at(cx, k, key_row, members, HeldMap::new(), &mut ops)?);
            }
        }
    }
    out.sort_by(|a, b| compare_entries(plan, a, b));
    Ok(out)
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
    /// R9 What the view keeps as numbers rather than lists: derived from
    /// the plan and the helpers at [`hydrate`], never sent.
    pub shape: Shape,
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
    let shape = shape(sch, plan, &env.helpers);
    let (pulled, groups) = {
        let scope = env.scope(sch);
        let pulled = pull_root(&Cx::new(sch, plan, &shape, &scope, st)?)?;
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
        shape,
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

    // Every dependency an entry recorded, its kept numbers' included.
    fn index(&mut self, e: &Entry) {
        self.index_deps(e);
        for d in e.held.keys() {
            self.by_dep.entry(d.clone()).or_default().insert(e.key.clone());
        }
    }

    // The dependencies an entry recorded as it pulled ([`Entry::deps`]);
    // its kept numbers' are moved by [`push_all`] as they are taken and
    // let go.
    fn index_deps(&mut self, e: &Entry) {
        for d in &e.deps {
            self.by_dep.entry(d.clone()).or_default().insert(e.key.clone());
        }
    }

    // What the kept numbers of the entry under `key` came to hold (`true`)
    // or let go, in order.
    fn apply_ops(&mut self, key: &[Value], ops: Ops) {
        for (d, held) in ops {
            if held {
                self.by_dep.entry(d).or_default().insert(key.to_vec());
            } else if let Some(ks) = self.by_dep.get_mut(&d) {
                ks.remove(key);
                if ks.is_empty() {
                    self.by_dep.remove(&d);
                }
            }
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
                self.index_deps(&n);
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
/// Kept numbers (R9, [`Shape`]) are moved rather than recounted: a change
/// to a table a kept plan reads moves the numbers of the dependency it
/// hits by the terms of its old and new rows, and the numbers above it by
/// what moved beneath them ([`sweep`]), and the entry's node is evaluated
/// again over them; no kept list is pulled whole. A group's kept members
/// are counted from the kept member keys and summed by the changes' terms.
/// What is not kept is rebuilt as above — which is also what a fresh
/// [`hydrate`] would find, and what [`contract`] holds either to.
///
/// An `Err` (a fault in one of the plan's expressions) leaves the view
/// stale, possibly with some entries' numbers moved and their nodes not —
/// [`rebuild`] it, as `ark_client::View` does.
///
/// Because the rebuild reads the final store, the order of the changes and
/// how many there are do not matter beyond which keys they name; a batch
/// of *n* changes touching *k* entries costs *k* rebuilds and no rollback.
/// What a rebuild costs beyond its reads is the indexes' probes — a
/// logarithm of the view, not a pass over it: an entry that stays put is
/// not moved in [`View::entries`], and a read that holds a table's key is
/// one `get` ([`Store::scan_where_eq`]). `tests/toggle.rs` holds a
/// playlist toggle through harken's `library` to two store reads at any
/// size, and times it from 250 media to 16000; `tests/aggregates.rs`
/// holds a song added under a composer of the `composers` view to the
/// rows on its path.
pub fn push_all(sch: &Schema, st: &dyn Store, changes: &[Change], view: &mut View) -> Result<Vec<Patch>, EvalFault> {
    let scope = view.env.scope(sch);
    let cx = Cx::new(sch, &view.plan, &view.shape, &scope, st)?;
    let t = touched(&cx, changes, &view.by_dep, &view.groups, &view.by_key)?;
    let mut rebuilt: Vec<(Vec<Value>, Option<Entry>, Ops)> = Vec::with_capacity(t.keys.len());
    for (k, hits) in &t.keys {
        let mut held = view.by_key.get_mut(k).map(|e| std::mem::take(&mut e.held)).unwrap_or_default();
        let mut ops = Ops::new();
        sweep(&cx, &mut held, hits, &mut ops)?;
        let e = root_of(&cx, k, held, &t, &view.groups, view.by_key.get(k), &mut ops)?;
        rebuilt.push((k.clone(), e, ops));
    }
    for (g, moves) in t.groups {
        let members = view.groups.entry(g.clone()).or_default();
        for k in &moves.left {
            members.remove(k);
        }
        members.extend(moves.arrived);
        if members.is_empty() {
            view.groups.remove(&g);
        }
    }
    let mut out = Vec::new();
    for (k, e, ops) in rebuilt {
        view.settle(k.clone(), e, &mut out);
        view.apply_ops(&k, ops);
    }
    Ok(out)
}

type Groups = BTreeMap<Vec<Value>, BTreeSet<Vec<Value>>>;

// R9 What the changes did to one group's members, over the kept set rather
// than a copy of it: the keys that arrived and were not members, and the
// members that left. A group of a thousand touched by one row costs that
// row, not the thousand (`artists`, Bach's group).
#[derive(Default)]
struct Moves {
    arrived: BTreeSet<Vec<Value>>,
    left: BTreeSet<Vec<Value>>,
}

impl Moves {
    fn has(&self, base: Option<&BTreeSet<Vec<Value>>>, k: &Vec<Value>) -> bool {
        self.arrived.contains(k) || (!self.left.contains(k) && base.is_some_and(|b| b.contains(k)))
    }

    // `k` leaves; whether it was a member.
    fn leave(&mut self, base: Option<&BTreeSet<Vec<Value>>>, k: Vec<Value>) -> bool {
        if !self.has(base, &k) {
            return false;
        }
        if !self.arrived.remove(&k) {
            self.left.insert(k);
        }
        true
    }

    // `k` arrives; whether it was not a member.
    fn arrive(&mut self, base: Option<&BTreeSet<Vec<Value>>>, k: Vec<Value>) -> bool {
        if self.has(base, &k) {
            return false;
        }
        if !self.left.remove(&k) {
            self.arrived.insert(k);
        }
        true
    }

    // How many members the group has now.
    fn len(&self, base: Option<&BTreeSet<Vec<Value>>>) -> usize {
        base.map_or(0, |b| b.len()) + self.arrived.len() - self.left.len()
    }

    // The members' keys now, in key order.
    fn keys(&self, base: Option<&BTreeSet<Vec<Value>>>) -> Vec<Vec<Value>> {
        let mut out: Vec<Vec<Value>> = base.into_iter().flatten().filter(|k| !self.left.contains(*k)).cloned().collect();
        out.extend(self.arrived.iter().cloned());
        out.sort();
        out
    }
}

// §1.5, 1–2 What the changes touch: the keys of the entries to rebuild,
// each with the hits its kept numbers took (R9); what the changes did to
// each touched group's members, and what each one's kept member numbers
// came to. Reads the view; changes nothing.
struct Touched<'c> {
    keys: BTreeMap<Vec<Value>, Vec<Hit<'c>>>,
    groups: BTreeMap<Vec<Value>, Moves>,
    sums: BTreeMap<Vec<Value>, Track>,
}

// R9, D4 One touched group's kept member numbers: those its entry held,
// moved by each row that really left or arrived, in change order — `None`
// for a group with no entry, which is counted from its rows — and whether
// an extreme left with a row, to be read again.
struct Track {
    nums: Option<Vec<Num>>,
    stale: bool,
}

fn touched<'c>(
    cx: &Cx,
    changes: &'c [Change],
    by_dep: &BTreeMap<(NodeId, Value), BTreeSet<Vec<Value>>>,
    view_groups: &Groups,
    by_key: &BTreeMap<Vec<Value>, Entry>,
) -> Result<Touched<'c>, EvalFault> {
    let (sch, plan) = (cx.sch, cx.plan);
    let table = plan.table();
    let tbl = cx.table(table)?;
    let ns = nodes(plan);
    let mut t = Touched {
        keys: BTreeMap::new(),
        groups: BTreeMap::new(),
        sums: BTreeMap::new(),
    };
    let member_aggs = cx.shape.root.members.as_deref().filter(|a| a.iter().any(|(a, _)| *a != Agg::Count));
    for ch in changes {
        let (old, new) = match ch {
            Change::Add(_, r) => (None, Some(r)),
            Change::Remove(_, r) => (Some(r), None),
            Change::Edit(_, o, n) => (Some(o), Some(n)),
        };
        if ch.table() == table.as_str() {
            match &plan.source {
                Source::Table(_) => {
                    for r in old.iter().chain(new.iter()) {
                        t.keys.entry(tbl.key_of(r)).or_default();
                    }
                }
                // In change order, so an edit within a group, out of one
                // and into another, all leave the members right — and a
                // kept sum moved by each row that really left or arrived.
                Source::Group { by, .. } => {
                    for (r, arrives) in [(old, false), (new, true)] {
                        let Some(r) = r else { continue };
                        let g = group_of(by, r);
                        let base = view_groups.get(&g);
                        let members = t.groups.entry(g.clone()).or_default();
                        let moved = if !arrives {
                            members.leave(base, tbl.key_of(r))
                        } else {
                            admits(cx.root_filter.as_ref(), r) && members.arrive(base, tbl.key_of(r))
                        };
                        if let (true, Some(aggs)) = (moved, member_aggs) {
                            let terms = terms(cx, aggs, &r.to_value())?;
                            let tr = t.sums.entry(g.clone()).or_insert_with(|| Track {
                                nums: by_key.get(&g).filter(|e| e.members.len() == aggs.len()).map(|e| e.members.clone()),
                                stale: false,
                            });
                            if let Some(nums) = &mut tr.nums {
                                if arrives {
                                    put_in(aggs, nums, &terms)?;
                                } else {
                                    tr.stale |= take_out(aggs, nums, &terms)?;
                                }
                            }
                        }
                        t.keys.entry(g).or_default();
                    }
                }
            }
        }
        for (id, n) in &ns {
            if n.table() != ch.table() {
                continue;
            }
            let kept = cx.shape.kept.contains_key(id);
            let d_old = old.map(|r| n.dependency(sch, r));
            let d_new = new.map(|r| n.dependency(sch, r));
            let both = d_old.is_some() && d_old == d_new;
            for (d, is_old) in [(&d_old, true), (&d_new, false)] {
                let Some(d) = d else { continue };
                if both && !is_old {
                    continue;
                }
                let Some(ks) = by_dep.get(&(*id, d.clone())) else { continue };
                for k in ks {
                    let hits = t.keys.entry(k.clone()).or_default();
                    if kept {
                        hits.push(Hit {
                            id: *id,
                            on: d.clone(),
                            old: old.filter(|_| is_old),
                            new: new.filter(|_| !is_old || both),
                        });
                    }
                }
            }
        }
    }
    Ok(t)
}

// §1.5, 1 and R9 The entry under `k` as the store now decides it — gone,
// new, or rebuilt over the numbers `held` keeps (already swept) — with
// every dependency it let go told to `ops`.
fn root_of(
    cx: &Cx,
    k: &[Value],
    held: HeldMap,
    t: &Touched,
    view_groups: &Groups,
    old: Option<&Entry>,
    ops: &mut Ops,
) -> Result<Option<Entry>, EvalFault> {
    let plan = cx.plan;
    let table = plan.table();
    let gone = |held: HeldMap, ops: &mut Ops| {
        ops.extend(held.into_keys().map(|d| (d, false)));
        Ok(None)
    };
    match &plan.source {
        Source::Table(_) => match cx.st.get(table, k) {
            Some(row) if admits(cx.root_filter.as_ref(), &row) => entry_at(cx, k.to_vec(), Cand::Row(row), Members::None, held, ops).map(Some),
            _ => gone(held, ops),
        },
        Source::Group { by, .. } => match t
            .groups
            .get(k)
            .map_or_else(|| view_groups.get(k).map_or(0, |b| b.len()), |m| m.len(view_groups.get(k)))
        {
            0 => gone(held, ops),
            size => {
                let base = view_groups.get(k);
                let rows = || match t.groups.get(k) {
                    Some(m) => m.keys(base),
                    None => base.into_iter().flatten().cloned().collect(),
                };
                let rows = || rows().iter().filter_map(|m| cx.st.get(table, m)).map(Row::into_value).collect::<Vec<_>>();
                let members = match &cx.shape.root.members {
                    None => Members::List(Value::from(rows())),
                    // A count is the kept keys; a sum is what it was moved
                    // by the rows that left and arrived, an extreme what
                    // they compared to — or read again, when it left (D4);
                    // a group that is new, its rows'.
                    Some(aggs) => Members::Nums({
                        let moved = match (old, t.sums.get(k)) {
                            (Some(o), None) if o.members.len() == aggs.len() => Some(o.members.clone()),
                            (_, Some(Track { nums: Some(nums), stale })) => {
                                let mut nums = nums.clone();
                                if *stale {
                                    let pins: Vec<(FieldName, Value)> = by.iter().cloned().zip(k.iter().cloned()).collect();
                                    extremes(cx, plan, &pins, aggs, &mut nums)?;
                                }
                                Some(nums)
                            }
                            _ => None,
                        };
                        match moved {
                            Some(mut nums) => {
                                for (n, (a, _)) in nums.iter_mut().zip(aggs) {
                                    if *a == Agg::Count {
                                        *n = Num::Int(size as i128);
                                    }
                                }
                                nums
                            }
                            None if aggs.iter().all(|(a, _)| *a == Agg::Count) => vec![Num::Int(size as i128); aggs.len()],
                            None => member_nums(cx, aggs, &rows())?,
                        }
                    }),
                };
                let key_row = Cand::Group(Value::Struct(Box::new(by.iter().cloned().zip(k.iter().cloned()).collect())));
                entry_at(cx, k.to_vec(), key_row, members, held, ops).map(Some)
            }
        },
    }
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
