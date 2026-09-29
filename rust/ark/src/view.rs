//! §13 Views: what a plan means, and a plan kept up to date.
//!
//! [`pull`] is the one evaluator of plans (`docs/plan-v4.md` §1.4): a
//! query's value, a mutator's `select`, a hydrating view are all what it
//! answers. It returns one [`Entry`] per candidate — a source row the
//! filter admits, or a group — carrying what §1.5 names: its key, the
//! dependencies its subtree read (by plan node, [`nodes`]), its order
//! keys, whether `having` admitted it, and its node. [`answer`] is the
//! list a caller sees: the admitted entries, in order, cut to the limit.
//!
//! The incremental half below — [`ViewPlan`], [`hydrate`], [`push`],
//! [`contract`] — is v3's, and maintains a v3-shaped plan only
//! ([`Plan::is_v3_shaped`]): a table, a filter, a column order, a limit and
//! references beneath. `push_all` over entries replaces it (§1.5, B1b).
//! A view there is a plan kept up to date: [`hydrate`] pulls what `select`
//! gives; [`push`] is told each change the store made and moves that answer
//! to what `select` would give now, reporting what it did to its own list
//! as positions ([`Patch`]). The contract ([`contract`]): after any
//! sequence of changes the rows equal a fresh hydrate, and splicing the
//! patches into the old list gives the new one. The store is already at the
//! new state when a change arrives.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::eval::{EvalError, EvalFault, NodeScope, Scope};
use crate::ir::{CmpOp, Expr, Key, Lookup, Plan, Pred, Related, Source};
use crate::schema::{Dir, Relation, Schema, Table};
use crate::store::{Change, Row, Store};
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
    let mut rows: Vec<Row> = st.scan_where_eq(table, &equalities(filter.as_ref()), &|r| admits(filter.as_ref(), r));
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
    let mut node = scope.node();
    if let Some(x) = plan.row {
        node.bind(x, row.clone());
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
    let mut fields: BTreeMap<FieldName, Value> = match &row {
        Value::Struct(m) => m.clone(),
        _ => BTreeMap::new(),
    };
    for r in &plan.related {
        let on =
            r.on.iter()
                .map(|(c, e)| Ok((c.clone(), node.eval(e)?)))
                .collect::<Result<Vec<_>, EvalFault>>()?;
        deps.push((id, Value::List(on.iter().map(|(_, v)| v.clone()).collect())));
        let kids = pull_at(sch, &r.plan, id + 1, &on, scope, st)?;
        for k in &kids {
            deps.extend(k.deps.iter().cloned());
        }
        let list = Value::List(answer(&r.plan, &kids));
        node.bind(r.sym, list.clone());
        fields.insert(r.name.clone(), list);
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
        (None, true) => Value::Struct(fields),
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

// §13.1 The v3 view (B1b replaces it) ------------------------------------

// §13.1 The plan a view maintains ----------------------------------------

/// A filter with its right-hand sides evaluated: one constructor per
/// [`Pred`] constructor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Filter {
    Cmp(FieldName, CmpOp, Value),
    In(FieldName, Vec<Value>),
    All(Vec<Filter>),
    Any(Vec<Filter>),
    Not(Box<Filter>),
}

/// A [`Plan`] with every right-hand side evaluated, to any depth.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewPlan {
    pub table: TableName,
    pub filter: Option<Filter>,
    /// The plan's order; `compare_rows` appends the key, which makes the
    /// order total when a caller hands the view a plan the verifier never
    /// saw.
    pub order: Vec<(FieldName, Dir)>,
    pub limit: Option<i64>,
    /// Each relationship read beneath a row: the field it appears as, the
    /// relationship, and the child plan (whose limit is per parent).
    pub related: Vec<(FieldName, Relation, ViewPlan)>,
}

/// Resolve a v3-shaped plan ([`Plan::is_v3_shaped`]) with an evaluator
/// for its right-hand sides; the first failure is the answer, in the plan's
/// own order. A related plan's `on` is read back as the reference it is:
/// the child's column against the parent's key.
///
/// # Panics
///
/// On a plan that is not v3-shaped: this view cannot maintain one.
pub fn eval_plan<E>(p: &Plan, ev: &mut dyn FnMut(&Expr) -> Result<Value, E>) -> Result<ViewPlan, E> {
    assert!(p.is_v3_shaped(), "a v3 view maintains a v3-shaped plan only: {p:?}");
    let filter = match &p.filter {
        None => None,
        Some(f) => Some(eval_pred(f, ev)?),
    };
    let mut related = Vec::with_capacity(p.related.len());
    for r in &p.related {
        let child = eval_plan(&r.plan, ev)?;
        let relation = Relation {
            parent: p.table().clone(),
            child: r.plan.table().clone(),
            column: r.on[0].0.clone(),
        };
        related.push((r.name.clone(), relation, child));
    }
    Ok(ViewPlan {
        table: p.table().clone(),
        filter,
        order: p.order_columns().unwrap_or_default(),
        limit: p.limit,
        related,
    })
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

/// Whether a row passes the filter; no filter admits every row. A column
/// the row lacks reads as `Null`.
/// The columns a filter holds equal to a value however it is satisfied:
/// its top-level `Cmp(_, Eq, _)`, and every one inside a top-level `All`.
/// What an indexed store looks rows up by ([`Store::scan_where_eq`]).
pub fn equalities(f: Option<&Filter>) -> Vec<(&str, &Value)> {
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

pub fn admits(f: Option<&Filter>, row: &Row) -> bool {
    match f {
        None => true,
        Some(f) => go(f, row),
    }
}

fn go(f: &Filter, row: &Row) -> bool {
    let field = |c: &str| row.get(c).cloned().unwrap_or(Value::Null);
    match f {
        Filter::Cmp(c, op, v) => cmp(*op, &field(c), v),
        Filter::In(c, vs) => vs.iter().any(|v| cmp(CmpOp::Eq, &field(c), v)),
        Filter::All(fs) => fs.iter().all(|g| go(g, row)),
        Filter::Any(fs) => fs.iter().any(|g| go(g, row)),
        Filter::Not(g) => !go(g, row),
    }
}

/// Comparison under the one total order, so `NULL = NULL` is true and
/// `NULL < 0` is true (`Ark.Eval.cmp`, which `Ark.View.cmp` must equal).
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

/// The plan's order alone, stable ties (`Ark.Eval.orderBy`).
pub fn order_by(cols: &[(FieldName, Dir)], a: &Row, b: &Row) -> Ordering {
    for (c, d) in cols {
        let o = compare_value(a.get(c).unwrap_or(&Value::Null), b.get(c).unwrap_or(&Value::Null));
        let o = if *d == Dir::Desc { o.reverse() } else { o };
        if o != Ordering::Equal {
            return o;
        }
    }
    Ordering::Equal
}

/// §13.2 The order a view keeps its rows in: the plan's order, then the key
/// ascending — a total comparison (`Ark.View.compareRows`).
pub fn compare_rows(tbl: &Table, cols: &[(FieldName, Dir)], a: &Row, b: &Row) -> Ordering {
    order_by(cols, a, b).then_with(|| tbl.key_of(a).cmp(&tbl.key_of(b)))
}

// §13.3 The view -----------------------------------------------------------

/// A maintained plan: the plan and its nodes, in the plan's order, each
/// node beside the row it was built over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    pub plan: ViewPlan,
    pub nodes: Vec<(Row, Value)>,
}

/// Pull everything: what `select` answers for the plan, as a view.
pub fn hydrate(sch: &Schema, vp: &ViewPlan, st: &dyn Store) -> View {
    View {
        plan: vp.clone(),
        nodes: pull_view(sch, vp, st),
    }
}

/// A rebase rolled the optimistic store back: hydrate again.
pub fn rebuild(sch: &Schema, st: &dyn Store, view: &View) -> View {
    hydrate(sch, &view.plan, st)
}

impl View {
    /// The current nodes, in order.
    pub fn rows(&self) -> Vec<Value> {
        self.nodes.iter().map(|(_, n)| n.clone()).collect()
    }
}

/// `select` over an evaluated plan: scan, filter, sort, take, attach. A
/// table the schema lacks is empty here, so that a view is total.
pub fn pull_view(sch: &Schema, vp: &ViewPlan, st: &dyn Store) -> Vec<(Row, Value)> {
    let Some(tbl) = sch.lookup_table(&vp.table) else {
        return vec![];
    };
    let mut admitted: Vec<Row> = st.scan_where_eq(&vp.table, &equalities(vp.filter.as_ref()), &|r| admits(vp.filter.as_ref(), r));
    admitted.sort_by(|a, b| compare_rows(tbl, &vp.order, a, b));
    if let Some(lim) = vp.limit {
        admitted.truncate(lim.max(0) as usize);
    }
    admitted
        .into_iter()
        .map(|row| {
            let node = node_of(sch, vp, st, tbl, &row);
            (row, node)
        })
        .collect()
}

/// A node over a row: the row's columns plus one field per relationship
/// holding the child nodes (the relationship's field wins a name clash).
pub fn node_of(sch: &Schema, vp: &ViewPlan, st: &dyn Store, tbl: &Table, row: &Row) -> Value {
    let pk = parent_key(tbl, row);
    let mut fields: BTreeMap<FieldName, Value> = row.clone();
    for (name, rel, child) in &vp.related {
        let pin = Filter::Cmp(rel.column.clone(), CmpOp::Eq, pk.clone());
        let pinned = ViewPlan {
            filter: Some(match &child.filter {
                None => pin,
                Some(f) => Filter::All(vec![pin, f.clone()]),
            }),
            ..child.clone()
        };
        let kids: Vec<Value> = pull_view(sch, &pinned, st).into_iter().map(|(_, n)| n).collect();
        fields.insert(name.clone(), Value::List(kids));
    }
    Value::Struct(fields)
}

/// The value a child's join column holds for this parent: the single key
/// column's value, or the whole key as a list where a verified module never
/// arrives.
pub fn parent_key(tbl: &Table, row: &Row) -> Value {
    let mut ks = tbl.key_of(row);
    if ks.len() == 1 {
        ks.pop().unwrap_or(Value::Null)
    } else {
        Value::List(ks)
    }
}

// §13.4 Patches -------------------------------------------------------------

/// What `push` did to the view's list, as positions into the list as it
/// stands when the patch is applied, in order.
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

/// §13.5 A change arrives; the store is the store after it. A change in the
/// plan's own table moves the list; a change in a table read beneath it
/// updates the parent nodes; both run when the table is both.
pub fn push(sch: &Schema, st: &dyn Store, ch: &Change, view: &View) -> (View, Vec<Patch>) {
    let Some(tbl) = sch.lookup_table(&view.plan.table) else {
        return (view.clone(), vec![]);
    };
    let (v1, mut ps1) = if ch.table() == view.plan.table {
        push_top(sch, st, tbl, ch, view)
    } else {
        (view.clone(), vec![])
    };
    let (v2, ps2) = push_below(sch, st, tbl, ch, &v1);
    ps1.extend(ps2);
    (v2, ps1)
}

// A change in the plan's own table: an add, a remove, an edit in place or
// across, or nothing. Under a limit the view is a window; whenever a row
// leaves a full window, or moves to its end, the store is asked what comes
// next (`refill`).
fn push_top(sch: &Schema, st: &dyn Store, tbl: &Table, ch: &Change, view: &View) -> (View, Vec<Patch>) {
    let vp = &view.plan;
    let nodes = &view.nodes;
    let keep = |r: &Row| admits(vp.filter.as_ref(), r);
    let key = |r: &Row| tbl.key_of(r);
    let order = |a: &Row, b: &Row| compare_rows(tbl, &vp.order, a, b);
    let position = |row: &Row| nodes.iter().position(|(r, _)| key(r) == key(row));
    let insert_pos = |row: &Row, ns: &[(Row, Value)]| ns.iter().take_while(|(r, _)| order(r, row) == Ordering::Less).count();
    let build = |row: &Row| (row.clone(), node_of(sch, vp, st, tbl, row));
    let full = vp.limit == Some(nodes.len() as i64);
    let with = |ns: Vec<(Row, Value)>| View { plan: vp.clone(), nodes: ns };

    // The pull: the first admitted row beyond the bound, the bound being the
    // last row the window still holds.
    let refill = |ns: &[(Row, Value)]| -> Option<(Row, Value)> {
        let bound = ns.last().map(|(r, _)| r);
        let mut candidates: Vec<Row> = st.scan_where_eq(&vp.table, &equalities(vp.filter.as_ref()), &|r| {
            keep(r) && bound.is_none_or(|b| order(b, r) == Ordering::Less)
        });
        candidates.sort_by(|a, b| order(a, b));
        candidates.first().map(&build)
    };

    let add = |row: &Row| -> (View, Vec<Patch>) {
        if !keep(row) {
            return (view.clone(), vec![]);
        }
        let j = insert_pos(row, nodes);
        if let Some(lim) = vp.limit {
            if j as i64 >= lim {
                return (view.clone(), vec![]);
            }
        }
        let n = build(row);
        let mut ns = nodes.clone();
        ns.insert(j, n.clone());
        match vp.limit {
            Some(lim) if ns.len() as i64 > lim => {
                ns.truncate(lim as usize);
                (with(ns), vec![Patch::Insert { at: j, node: n.1 }, Patch::Remove { at: lim as usize }])
            }
            _ => (with(ns), vec![Patch::Insert { at: j, node: n.1 }]),
        }
    };

    let remove_at = |i: usize| -> (View, Vec<Patch>) {
        let mut ns = nodes.clone();
        ns.remove(i);
        match if full { refill(&ns) } else { None } {
            Some(n) => {
                let at = ns.len();
                ns.push(n.clone());
                (with(ns), vec![Patch::Remove { at: i }, Patch::Insert { at, node: n.1 }])
            }
            None => (with(ns), vec![Patch::Remove { at: i }]),
        }
    };

    // The row at i is now `new`, and still admitted. The one case that asks
    // the store: a full window whose edited row now orders last.
    let edit = |i: usize, new: &Row| -> (View, Vec<Patch>) {
        let mut ns = nodes.clone();
        ns.remove(i);
        let j = insert_pos(new, &ns);
        let n = build(new);
        let hidden = if full && j == ns.len() { refill(&ns) } else { None };
        match hidden {
            Some(h) if key(&h.0) != key(new) => {
                let at = ns.len();
                ns.push(h.clone());
                (with(ns), vec![Patch::Remove { at: i }, Patch::Insert { at: j.min(at), node: h.1 }])
            }
            _ => {
                ns.insert(j, n.clone());
                if j == i {
                    (with(ns), vec![Patch::Update { at: i, node: n.1 }])
                } else {
                    (with(ns), vec![Patch::Remove { at: i }, Patch::Insert { at: j, node: n.1 }])
                }
            }
        }
    };

    match ch {
        Change::Add(_, row) => add(row),
        Change::Remove(_, row) => match position(row) {
            Some(i) => remove_at(i),
            None => (view.clone(), vec![]),
        },
        Change::Edit(_, old, new) => match (position(old), keep(new)) {
            (None, false) => (view.clone(), vec![]),
            (None, true) => add(new),
            (Some(i), false) => remove_at(i),
            (Some(i), true) => edit(i, new),
        },
    }
}

/// §13.6 A change beneath the plan: every parent node it could have moved is
/// rebuilt from the store and, if it differs, reported as an `Update` at the
/// parent's position. A direct relationship's parents are those whose key
/// equals the changed row's join column (old and new of an edit); a table
/// reached only deeper updates every parent.
fn push_below(sch: &Schema, st: &dyn Store, tbl: &Table, ch: &Change, view: &View) -> (View, Vec<Patch>) {
    let vp = &view.plan;
    let t = ch.table();
    let direct: Vec<&FieldName> = vp
        .related
        .iter()
        .filter(|(_, rel, _)| rel.child == t)
        .map(|(_, rel, _)| &rel.column)
        .collect();
    let deeper = vp.related.iter().any(|(_, _, c)| descendants(c).iter().any(|d| d == t));
    if direct.is_empty() && !deeper {
        return (view.clone(), vec![]);
    }
    let joins: Vec<Value> = direct
        .iter()
        .flat_map(|col| changed_rows(ch).into_iter().filter_map(|row| row.get(*col).cloned()))
        .collect();
    let affected = |row: &Row| deeper || joins.contains(&parent_key(tbl, row));
    let mut nodes = Vec::with_capacity(view.nodes.len());
    let mut patches = Vec::new();
    for (i, (row, old)) in view.nodes.iter().enumerate() {
        if affected(row) {
            let new = node_of(sch, vp, st, tbl, row);
            if new != *old {
                patches.push(Patch::Update { at: i, node: new.clone() });
                nodes.push((row.clone(), new));
                continue;
            }
        }
        nodes.push((row.clone(), old.clone()));
    }
    (View { plan: vp.clone(), nodes }, patches)
}

fn descendants(c: &ViewPlan) -> Vec<TableName> {
    let mut out: Vec<TableName> = c.related.iter().map(|(_, _, g)| g.table.clone()).collect();
    for (_, _, g) in &c.related {
        out.extend(descendants(g));
    }
    out
}

fn changed_rows(ch: &Change) -> Vec<&Row> {
    match ch {
        Change::Add(_, r) | Change::Remove(_, r) => vec![r],
        Change::Edit(_, o, n) => vec![o, n],
    }
}

// §13.7 The contract ---------------------------------------------------------

/// A maintained view is right when it is indistinguishable from one
/// hydrated now.
pub fn contract(sch: &Schema, vp: &ViewPlan, st: &dyn Store, view: &View) -> bool {
    *view == hydrate(sch, vp, st)
}
