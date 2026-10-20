//! §1.10 Views, incremental per operator — the one evaluator of reads —
//! and §1.8's filter of a feed.
//!
//! A [`View`] keeps a query — a `Read` function returning a list — up to
//! date. Its body's relational spine is compiled into one node per
//! operator; every node keeps what it needs to update itself from a delta
//! of its input, and a delta is, per element, what it was and what it is:
//!
//! | operator | keeps | on a delta |
//! |---|---|---|
//! | `Scan` | nothing | the table's changes, net per key |
//! | `Filter`, `Map` | its output | the lambda applied to each element touched |
//! | `Join` | each side, indexed by its join key | for an element of one side, probe the other |
//! | `Group` | per key, its members, or a count and sums | the touched groups' aggregates again |
//! | `Sort` | its output in order | each element touched repositioned |
//! | `Take` | the window | refilled from its input's order |
//!
//! Anything else in a spine position is a leaf, evaluated whole. Every
//! element has an **order key**: bytes whose order is its place in the
//! list — a row's key for `Scan`, kept by `Filter` and `Map`; a group's
//! key; the two sides' concatenated for a `Join`; the sort fields'
//! encodings (complemented for `Desc`) then the input's for `Sort`, which
//! is the stable sort; a position for a leaf. Encodings are prefix-free
//! (§1.1), so concatenations of them are too.
//!
//! **A fresh read walks the same operators** ([`read`]): every relational
//! expression the interpreter meets — `Scan`, `Lookup`, `Filter`, `Map`,
//! `Join`, `Group`, `Sort`, `Take` — is evaluated here, each operator by
//! the one definition a view's node runs (`each`, `order`, `count`,
//! `tally` and `aggregate`, `join_key` and `joined`), over its
//! input as a list in order, every lambda evaluated by the run it is part
//! of (its stack, the changes it wrote so far, which a scan reads through,
//! its draws), in the order a view's hydration evaluates them. It keeps
//! no order keys, deltas or indexes: a `Sort` is a stable sort by the
//! sort key (under a `Take`, only the window is ordered), a `Group` its
//! groups, a `Join` with a key equality a one-shot hash of its smaller
//! side. **Routing** (P1) is how a fresh read's scan
//! is narrowed: a `Filter` of a `Scan`, a `Sort` of one by its columns and
//! a `Take` of a literal count of those, or `first` of one, read through
//! the key or an index — the longest run of the filter's equalities that
//! leads one, then its bounds on the next column — and only the rows the
//! filter would keep. A maintained view's own spine is not routed.
//!
//! **What a lambda reads** — a `Lookup`, a filtered `Scan` in a `Map` —
//! is recorded as the store reads it made: the key, or the index and the
//! prefix it scanned (routed, so a read is as narrow as its equalities
//! make it). A change to a row a recorded read covers re-evaluates the
//! elements that made it, and only those. A read in a `Join`'s or a
//! `Group`'s key, in a `Take`'s count or in a leaf rebuilds the view when
//! it is hit.
//!
//! **A kept aggregate.** A `Group` whose aggregate is `len(ms)`, `sum(ms)`,
//! `sum(map(ms, m => e))` with `e` pure of `m` alone, or a tuple of those
//! and parts not naming the members, keeps a count and `i128` sums per
//! group, moved by each member arriving and leaving, rather than the
//! members; a sum is judged once, when the aggregate is evaluated after
//! the whole delta, as `Std::Sum` judges it (v4's §6.5 rule): an overflow
//! on the way is none, and only the total must fit.
//!
//! [`View::push`] takes the changes of one settle — `Output::Changes`, a
//! rebase's included — with the store already after them, and answers the
//! [`Patch`]es that splice the list the UI holds into the new answer. The
//! contract (`tests/views.rs`): after every push the answer is the one a
//! fresh hydration gives ([`query`]) and the patches splice. A push that
//! refuses leaves the view stale; make it again.
//!
//! [`scoped`] is a scope's `Filter(Scan t)` over one entry's facts (§1.8):
//! the changes it makes to a connection's union.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Bound;
use std::sync::Arc;

use crate::codec::{int_enc, Typed};
use crate::hash::FnHash;
use crate::interp::{decode, effectful, free, parse, Interp, Loaded, Machine, V};
use crate::ir::{Effect, Expr, Op, Std};
use crate::module::Lib;
use crate::peer::Ctx;
use crate::schema::Schema;
use crate::store::{Change, Dir, Read, Scan};
use crate::ty::Ty;
use crate::Refusal;

/// One step from the list a UI holds towards the new answer; positions are
/// in the list as it stands when the patch is applied, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Patch {
    /// The node before the element at `at` (`at == len` appends).
    Insert {
        at: usize,
        node: V,
    },
    Remove {
        at: usize,
    },
    Update {
        at: usize,
        node: V,
    },
}

/// Each patch applied in order: what a client's list is held to.
pub fn splice(ps: &[Patch], xs: &[V]) -> Vec<V> {
    let mut vs = xs.to_vec();
    for p in ps {
        match p {
            Patch::Insert { at, node } => vs.insert((*at).min(vs.len()), node.clone()),
            Patch::Remove { at } if *at < vs.len() => drop(vs.remove(*at)),
            Patch::Update { at, node } if *at < vs.len() => vs[*at] = node.clone(),
            _ => {}
        }
    }
    vs
}

type Okey = Vec<u8>;
/// Per element whose value moved: what it was, what it is.
type Delta = BTreeMap<Okey, (Option<V>, Option<V>)>;
/// A read a lambda made: a table, an index (`None`: the key), the prefix
/// of the key or the entry it covered.
type Dep = (String, Option<usize>, Vec<u8>);
/// Per table, per key, the row before the changes and after them.
type Net = BTreeMap<String, BTreeMap<Okey, (Option<Vec<u8>>, Option<Vec<u8>>)>>;

/// The node whose dependencies rebuild the view.
const ALL: usize = usize::MAX;

fn put(d: &mut Delta, k: Okey, old: Option<V>, new: Option<V>) {
    d.entry(k).and_modify(|e| e.1 = new.clone()).or_insert((old, new));
}

fn prune(mut d: Delta) -> Delta {
    d.retain(|_, (a, b)| a != b);
    d
}

/// The changes net per key: the row before the first, after the last.
fn net(schema: &Schema, changes: &[Change]) -> Net {
    let mut out = Net::new();
    for c in changes {
        let Some((_, t)) = schema.table(c.table()) else { continue };
        let (old, new) = match c {
            Change::Add { row, .. } => (None, Some(row)),
            Change::Remove { row, .. } => (Some(row), None),
            Change::Edit { old, new, .. } => (Some(old), Some(new)),
        };
        let r = new.or(old).unwrap();
        let key = r[..t.key_len(r).expect("a fact parses")].to_vec();
        let e = out.entry(c.table().into()).or_default().entry(key).or_insert((old.cloned(), None));
        e.1 = new.cloned();
    }
    for t in out.values_mut() {
        t.retain(|_, (a, b)| a != b);
    }
    out
}

/// A store that records what is read through it.
struct Rec<'a> {
    inner: &'a dyn Read,
    deps: RefCell<Vec<Dep>>,
}

impl Read for Rec<'_> {
    fn schema(&self) -> &Schema {
        self.inner.schema()
    }
    fn get(&self, table: &str, key: &[u8]) -> Option<Vec<u8>> {
        self.deps.borrow_mut().push((table.into(), None, key.to_vec()));
        self.inner.get(table, key)
    }
    fn scan(&self, table: &str, s: &Scan) -> Vec<Vec<u8>> {
        let p = match &s.lo {
            std::ops::Bound::Included(p) if Scan::prefix(s.index, p, s.dir, 0).hi == s.hi => p.clone(),
            _ => vec![],
        };
        self.deps.borrow_mut().push((table.into(), s.index, p));
        self.inner.scan(table, s)
    }
}

/// How an operator evaluates a lambda: in a maintained view's run, as a
/// machine of its own whose reads are recorded ([`Kept`]); in a fresh
/// read, as part of the run that met it ([`Fresh`]).
trait Eval {
    /// `e` with `vars` bound; its reads kept against `(node, okey)`.
    fn at(&mut self, e: &Expr, vars: Vec<V>, at: (usize, &Okey)) -> Result<V, Refusal>;
}

/// A lambda applied.
fn call(cx: &mut dyn Eval, lam: &Expr, args: Vec<V>, at: (usize, &Okey)) -> Result<V, Refusal> {
    match lam {
        Expr::Lambda(_, body) => cx.at(body, args, at),
        _ => Err(Refusal::new("not a lambda")),
    }
}

/// A maintained view's run.
struct Kept<'a> {
    store: &'a dyn Read,
    f: &'a Loaded,
    lib: &'a Lib,
    ctx: &'a Ctx,
    args: &'a [V],
    /// Elements evaluated this push, with what they read: `(node, okey)`.
    seen: Vec<(usize, Okey, Vec<Dep>)>,
}

impl Eval for Kept<'_> {
    /// Over the view's arguments (the last is `Var(0)`).
    fn at(&mut self, e: &Expr, vars: Vec<V>, (node, k): (usize, &Okey)) -> Result<V, Refusal> {
        let rec = Rec {
            inner: self.store,
            deps: RefCell::new(vec![]),
        };
        let callee = |h: &FnHash| self.lib.get(h);
        let mut m = Machine::new(&rec, self.ctx, &callee, &self.f.cx);
        m.stack = [self.args, &vars[..]].concat();
        let v = m.eval(e)?;
        self.seen.push((node, k.clone(), rec.deps.into_inner()));
        Ok(v)
    }
}

impl Kept<'_> {
    /// An element gone: what it read no longer matters.
    fn forget(&mut self, node: usize, k: &Okey) {
        self.seen.push((node, k.clone(), vec![]));
    }

    fn row(&self, table: &str, b: &[u8]) -> V {
        let t = self.store.schema().table(table).expect("a table").1;
        decode(&self.f.cx, &t.row_ty(), b).expect("a stored row decodes")
    }
}

/// A fresh read, part of the run `m` that met it.
struct Fresh<'m, 'a> {
    m: &'m mut Machine<'a>,
}

impl Eval for Fresh<'_, '_> {
    /// Over the run's stack.
    fn at(&mut self, e: &Expr, vars: Vec<V>, _: (usize, &Okey)) -> Result<V, Refusal> {
        let depth = self.m.stack.len();
        self.m.stack.extend(vars);
        let v = self.m.eval(e);
        self.m.stack.truncate(depth);
        v
    }
}

// * The operators, once: what a view's node and a fresh read both run

/// `Filter`'s verdict on an element, or `Map`'s image of it.
fn each(cx: &mut dyn Eval, filter: bool, lam: &Expr, v: V, at: (usize, &Okey)) -> Result<Option<V>, Refusal> {
    match filter {
        true => Ok(call(cx, lam, vec![v.clone()], at)?.truth().then_some(v)),
        false => Ok(Some(call(cx, lam, vec![v], at)?)),
    }
}

/// `Sort`'s key of an element: its sort fields' bytes, complemented where
/// descending. Ties keep their input order (the stable sort).
fn order(cx: &mut dyn Eval, keys: &Expr, dirs: &[Dir], v: &V, at: (usize, &Okey)) -> Result<Okey, Refusal> {
    Ok(sort_key(call(cx, keys, vec![v.clone()], at)?, dirs))
}

/// `Take`'s count.
fn count(cx: &mut dyn Eval, n: &Expr) -> Result<usize, Refusal> {
    Ok(cx.at(n, vec![], (ALL, &vec![]))?.int().max(0) as usize)
}

/// `Group`'s key of an element.
fn group_key(cx: &mut dyn Eval, by: &Expr, v: &V) -> Result<V, Refusal> {
    call(cx, by, vec![v.clone()], (ALL, &vec![]))
}

/// A member arriving (`sign` 1) or leaving (-1) a group's kept sums.
fn tally(cx: &mut dyn Eval, parts: &[Part], sums: &mut Vec<i128>, v: &V, sign: i128) -> Result<(), Refusal> {
    let terms = parts.iter().filter_map(|p| match p {
        Part::Sum(t) => Some(t),
        _ => None,
    });
    sums.resize(terms.clone().count(), 0);
    for (s, t) in sums.iter_mut().zip(terms) {
        let x = match t {
            None => v.int(),
            Some(t) => cx.at(t, vec![v.clone()], (ALL, &vec![]))?.int(),
        };
        *s += sign * x as i128;
    }
    Ok(())
}

/// A group's element `(key, aggregate)`: from its `n` members' kept sums,
/// or from the members, which are asked for only when it keeps none.
#[allow(clippy::too_many_arguments)]
fn aggregate(
    cx: &mut dyn Eval,
    kept: &Option<(Vec<Part>, bool)>,
    agg: &Expr,
    key: V,
    n: i64,
    sums: &[i128],
    members: impl FnOnce() -> Vec<V>,
    at: (usize, &Okey),
) -> Result<V, Refusal> {
    let a = match kept {
        None => call(cx, agg, vec![key.clone(), V::List(members())], at)?,
        Some((parts, tuple)) => {
            let (mut vs, mut sums) = (vec![], sums.iter());
            for p in parts.iter() {
                vs.push(match p {
                    Part::Count => V::Int(n),
                    Part::Sum(_) => crate::std::total(*sums.next().unwrap())?,
                    Part::Other(e) => cx.at(e, vec![key.clone(), V::List(vec![])], at)?,
                });
            }
            if *tuple {
                V::Tup(vs)
            } else {
                vs.remove(0)
            }
        }
    };
    Ok(V::Tup(vec![key, a]))
}

/// `Join`'s keys: a conjunct of its condition equating an expression of
/// each side alone ([`equality`]).
fn join_keys(on: &Expr) -> Option<(Expr, Expr)> {
    match on {
        Expr::Lambda(2, body) => equality(body),
        _ => None,
    }
}

/// An element's join key, on side `s` (0 the left).
fn join_key(cx: &mut dyn Eval, (ka, kb): &(Expr, Expr), s: usize, v: &V) -> Result<Vec<u8>, Refusal> {
    let vars = if s == 0 {
        vec![v.clone(), V::Tup(vec![])]
    } else {
        vec![V::Tup(vec![]), v.clone()]
    };
    Ok(cx.at(if s == 0 { ka } else { kb }, vars, (ALL, &vec![]))?.to_bytes())
}

/// A pair of partners, joined when the condition holds. Its order key is
/// the two sides' concatenated: the left's order, then the right's.
fn joined(cx: &mut dyn Eval, on: &Expr, x: &V, y: &V) -> Result<Option<V>, Refusal> {
    let holds = call(cx, on, vec![x.clone(), y.clone()], (ALL, &vec![]))?.truth();
    Ok(holds.then(|| V::Tup(vec![x.clone(), y.clone()])))
}

/// One part of a kept aggregate.
#[derive(Clone, Debug)]
enum Part {
    Count,
    /// The sum of each member, or of `e` of each.
    Sum(Option<Expr>),
    /// An expression not naming the members.
    Other(Expr),
}

/// The aggregate `(k, ms) => …` as parts a group keeps as numbers, and
/// whether they are a tuple; `None` when it needs the members.
fn kept(agg: &Expr) -> Option<(Vec<Part>, bool)> {
    let Expr::Lambda(2, body) = agg else { return None };
    let (es, tuple) = match &**body {
        Expr::Tuple(es) => (&es[..], true),
        e => (std::slice::from_ref(e), false),
    };
    let ms = || vec![Expr::Var(0)];
    let part = |e: &Expr| match e {
        Expr::Std(Std::Len, a) if *a == ms() => Some(Part::Count),
        Expr::Std(Std::Sum, a) if *a == ms() => Some(Part::Sum(None)),
        Expr::Std(Std::Sum, a) => match &a[..] {
            [Expr::Map(l, f)] if **l == Expr::Var(0) => match &**f {
                Expr::Lambda(1, t) if !effectful(t) && free(t).iter().all(|v| *v == Some(0)) => Some(Part::Sum(Some((**t).clone()))),
                _ => None,
            },
            _ => None,
        },
        e => free(e).iter().all(|v| v.is_some_and(|v| v != 0)).then(|| Part::Other(e.clone())),
    };
    Some((es.iter().map(part).collect::<Option<_>>()?, tuple))
}

/// A group: its key, how many members, its kept sums, or its members.
#[derive(Clone, Debug, Default)]
struct Members {
    key: Option<V>,
    n: i64,
    sums: Vec<i128>,
    rows: BTreeMap<Okey, V>,
}

/// One side of a join: its elements, and by the key `on` equates.
#[derive(Clone, Debug, Default)]
struct Side {
    rows: BTreeMap<Okey, V>,
    by: BTreeMap<Vec<u8>, BTreeSet<Okey>>,
}

/// What a node keeps beside its output, by its operator.
#[derive(Clone, Debug)]
enum Keep {
    Plain,
    /// Each input element's place in the output.
    Sort(BTreeMap<Okey, Okey>),
    /// The window's size.
    Take(usize),
    Group {
        kept: Option<(Vec<Part>, bool)>,
        groups: BTreeMap<Okey, Members>,
    },
    Join {
        keys: Option<(Expr, Expr)>,
        sides: Box<[Side; 2]>,
    },
}

#[derive(Clone, Debug)]
struct Node {
    keep: Keep,
    kids: Vec<usize>,
    out: BTreeMap<Okey, V>,
}

/// A maintained query (§1.10).
#[derive(Clone, Debug)]
pub struct View {
    f: Arc<Loaded>,
    lib: Lib,
    ctx: Ctx,
    args: Vec<V>,
    nodes: Vec<Node>,
    /// Each read some element made, to the elements that made it.
    deps: BTreeMap<Dep, BTreeSet<(usize, Okey)>>,
    held: BTreeMap<(usize, Okey), Vec<Dep>>,
    list: Vec<(Okey, V)>,
}

fn prepare(lib: &Lib, h: &FnHash, input: &[u8]) -> Result<(Arc<Loaded>, Vec<V>), Refusal> {
    let f = lib.get(h).ok_or_else(|| Refusal(format!("no function {h}")))?;
    if f.f.effect > Effect::Read {
        return Err(Refusal(format!("{h} writes; a view is of a query")));
    }
    let ty = Interp::input_ty(&f.f);
    let v = decode(&f.cx, &ty, parse(&f.cx, &ty, input).map(Typed::into_bytes)?.as_slice())?;
    let args = match (v, f.f.params.len()) {
        (V::Tup(vs), n) if n != 1 => vs,
        (v, _) => vec![v],
    };
    Ok((f, args))
}

/// A query's answer evaluated afresh — its body run, every read in it
/// hydrated: what a view is held to.
pub fn query(lib: &Lib, f: &FnHash, input: &[u8], ctx: &Ctx, store: &dyn Read) -> Result<Vec<V>, Refusal> {
    let (f, args) = prepare(lib, f, input)?;
    let callee = |h: &FnHash| lib.get(h);
    let mut m = Machine::new(store, ctx, &callee, &f.cx);
    m.stack = args;
    Ok(items(m.eval(&f.f.body)?))
}

/// Whether a fresh read of `e` may be routed: a `Filter` of a `Scan`, a
/// `Sort` of one or of a `Scan`, or a `Take` of those.
fn routable(e: &Expr) -> bool {
    let src = |l: &Expr| matches!(l, Expr::Scan(_)) || matches!(l, Expr::Filter(s, _) if matches!(**s, Expr::Scan(_)));
    match e {
        Expr::Take(l, _) => routable(l) || src(l),
        Expr::Sort(l, ..) => src(l),
        Expr::Filter(l, _) => matches!(**l, Expr::Scan(_)),
        _ => false,
    }
}

/// The spine of `e`: each node's expression and what it keeps, children
/// first; the root is the last.
fn spine<'e>(e: &'e Expr, exprs: &mut Vec<&'e Expr>, nodes: &mut Vec<Node>) -> usize {
    let mut kid = |l: &'e Expr| spine(l, exprs, nodes);
    let (keep, kids) = match e {
        Expr::Scan(_) => (Keep::Plain, vec![]),
        Expr::Filter(l, _) | Expr::Map(l, _) => (Keep::Plain, vec![kid(l)]),
        Expr::Sort(l, ..) => (Keep::Sort(BTreeMap::new()), vec![kid(l)]),
        Expr::Take(l, _) => (Keep::Take(0), vec![kid(l)]),
        Expr::Group(l, _, agg) => {
            let groups = BTreeMap::new();
            (Keep::Group { kept: kept(agg), groups }, vec![kid(l)])
        }
        Expr::Join(l, r, on) => {
            let (kids, keys) = (vec![kid(l), kid(r)], join_keys(on));
            (Keep::Join { keys, sides: Box::default() }, kids)
        }
        _ => (Keep::Plain, vec![]),
    };
    exprs.push(e);
    nodes.push(Node {
        keep,
        kids,
        out: BTreeMap::new(),
    });
    nodes.len() - 1
}

/// The expressions of a spine compiled before, node by node.
fn exprs(e: &Expr) -> Vec<&Expr> {
    let mut out = vec![];
    spine(e, &mut out, &mut vec![]);
    out
}

/// A relational expression evaluated afresh, as part of the run `m`: a
/// `Lookup` read through what the run wrote; anything else walked.
pub(crate) fn read(e: &Expr, m: &mut Machine) -> Result<V, Refusal> {
    if let Expr::Lookup(t, k) = e {
        let key = m.eval(k)?.to_bytes();
        let (ty, _) = m.row_ty(t)?;
        return Ok(match m.get(t, &key) {
            Some(r) => V::some(decode(m.cx, &ty, &r)?),
            None => V::none(),
        });
    }
    Ok(V::List(walk(e, m, usize::MAX)?))
}

/// `first` of a list read afresh, when the list is routed: one row of it.
pub(crate) fn first(l: &Expr, m: &mut Machine) -> Result<Option<V>, Refusal> {
    match l {
        Expr::Filter(..) | Expr::Sort(..) => Ok(route(m, l, 1)?.map(|xs| xs.into_iter().next().map_or(V::none(), V::some))),
        _ => Ok(None),
    }
}

/// The first `n` rows of a table in key order as the run `m` reads them:
/// through its own changes when it wrote the table.
fn scan(m: &Machine, t: &str, n: usize) -> Result<Vec<V>, Refusal> {
    let (ty, _) = m.row_ty(t)?;
    let rows = match m.changes.iter().any(|c| c.table() == t) {
        true => m.rows(t).into_iter().take(n).collect(),
        false => m.store().scan(t, &Scan { limit: n, ..Scan::all() }),
    };
    rows.iter().map(|r| decode(m.cx, &ty, r)).collect()
}

/// A fresh read of `e`, the first `limit` of it: its operators walked once
/// over lists, in order, each as a view's node runs it and in the order a
/// view's hydration evaluates its lambdas (a node's input, then its own),
/// keeping nothing the answer does not need. A part that may be routed is
/// read through the store's indexes when a route serves.
fn walk(e: &Expr, m: &mut Machine, limit: usize) -> Result<Vec<V>, Refusal> {
    if routable(e) {
        if let Some(mut xs) = routed(m, e)? {
            xs.truncate(limit);
            return Ok(xs);
        }
    }
    let at = (ALL, &vec![]);
    let mut xs = match e {
        Expr::Scan(t) => scan(m, t, limit)?,
        Expr::Filter(l, lam) | Expr::Map(l, lam) => {
            let filter = matches!(e, Expr::Filter(..));
            let xs = walk(l, m, usize::MAX)?;
            let cx = &mut Fresh { m };
            let mut out = Vec::with_capacity(xs.len());
            for v in xs {
                out.extend(each(cx, filter, lam, v, at)?);
            }
            out
        }
        Expr::Sort(l, keys, dirs) => sort(m, (l, keys, dirs), None, limit)?,
        Expr::Take(l, n) => match &**l {
            Expr::Scan(t) => {
                let n = count(&mut Fresh { m }, n)?;
                scan(m, t, n.min(limit))?
            }
            Expr::Sort(s, keys, dirs) => match routable(l).then(|| routed(m, l)).transpose()?.flatten() {
                Some(mut xs) => {
                    xs.truncate(count(&mut Fresh { m }, n)?);
                    xs
                }
                None => sort(m, (s, keys, dirs), Some(n), limit)?,
            },
            l => {
                let mut xs = walk(l, m, usize::MAX)?;
                xs.truncate(count(&mut Fresh { m }, n)?);
                xs
            }
        },
        Expr::Group(l, by, agg) => {
            let xs = walk(l, m, usize::MAX)?;
            let cx = &mut Fresh { m };
            let kept = kept(agg);
            let mut groups: BTreeMap<Okey, (V, i64, Vec<i128>, Vec<V>)> = BTreeMap::new();
            for v in xs {
                let g = group_key(cx, by, &v)?;
                let grp = groups.entry(g.to_bytes()).or_insert_with(|| (g, 0, vec![], vec![]));
                grp.1 += 1;
                match &kept {
                    Some((parts, _)) => tally(cx, parts, &mut grp.2, &v, 1)?,
                    None => grp.3.push(v),
                }
            }
            let mut out = Vec::with_capacity(groups.len().min(limit));
            for (_, (key, n, sums, members)) in groups {
                out.push(aggregate(cx, &kept, agg, key, n, &sums, || members, at)?);
            }
            out
        }
        Expr::Join(l, r, on) => {
            let (xs, ys) = (walk(l, m, usize::MAX)?, walk(r, m, usize::MAX)?);
            let cx = &mut Fresh { m };
            let pairs: Vec<(usize, usize)> = match join_keys(on) {
                None => (0..xs.len()).flat_map(|i| (0..ys.len()).map(move |j| (i, j))).collect(),
                Some(keys) => {
                    let ka = xs.iter().map(|v| join_key(cx, &keys, 0, v)).collect::<Result<Vec<_>, _>>()?;
                    let kb = ys.iter().map(|v| join_key(cx, &keys, 1, v)).collect::<Result<Vec<_>, _>>()?;
                    // A one-shot hash of the smaller side, probed by the other.
                    let (small, big, flip) = if ka.len() <= kb.len() { (&ka, &kb, true) } else { (&kb, &ka, false) };
                    let mut by: HashMap<&[u8], Vec<usize>> = HashMap::with_capacity(small.len());
                    for (i, k) in small.iter().enumerate() {
                        by.entry(k).or_default().push(i);
                    }
                    let mut pairs = vec![];
                    for (j, k) in big.iter().enumerate() {
                        for &i in by.get(&k[..]).into_iter().flatten() {
                            pairs.push(if flip { (i, j) } else { (j, i) });
                        }
                    }
                    if flip {
                        pairs.sort_unstable();
                    }
                    pairs
                }
            };
            let mut out = vec![];
            for (i, j) in pairs {
                out.extend(joined(cx, on, &xs[i], &ys[j])?);
            }
            out
        }
        _ => items(m.eval(e)?),
    };
    xs.truncate(limit);
    Ok(xs)
}

/// `Sort(l, keys, dirs)` read afresh, the first `limit` of it, or of its
/// `Take` by `n`, whose count is evaluated after the sort keys, as a view's
/// node evaluates it: a stable sort, of only the window when there is one.
fn sort(m: &mut Machine, (l, keys, dirs): (&Expr, &Expr, &[Dir]), n: Option<&Expr>, limit: usize) -> Result<Vec<V>, Refusal> {
    let xs = walk(l, m, usize::MAX)?;
    let cx = &mut Fresh { m };
    let mut keyed = Vec::with_capacity(xs.len());
    for v in xs {
        keyed.push((order(cx, keys, dirs, &v, (ALL, &vec![]))?, keyed.len(), v));
    }
    let limit = match n {
        Some(n) => count(cx, n)?.min(limit),
        None => limit,
    };
    let by = |a: &(Okey, usize, V), b: &(Okey, usize, V)| (&a.0, a.1).cmp(&(&b.0, b.1));
    if limit < keyed.len() {
        keyed.select_nth_unstable_by(limit, by);
        keyed.truncate(limit);
    }
    keyed.sort_unstable_by(by);
    Ok(keyed.into_iter().map(|x| x.2).collect())
}

/// Every node, children first, from their deltas and the elements a read
/// of theirs was hit at; `net` is `None` at the start.
fn run(cx: &mut Kept, exprs: &[&Expr], nodes: &mut [Node], net: Option<&Net>, mut hits: BTreeMap<usize, BTreeSet<Okey>>) -> Result<Delta, Refusal> {
    let mut deltas: Vec<Delta> = Vec::with_capacity(nodes.len());
    for i in 0..nodes.len() {
        let (done, rest) = nodes.split_at_mut(i);
        let hit = hits.remove(&i).unwrap_or_default();
        let at = (i, exprs[i]);
        let d = step(cx, at, &mut rest[0], (done, exprs), &mut deltas, net, hit)?;
        deltas.push(prune(d));
    }
    Ok(deltas.pop().unwrap_or_default())
}

/// In a join's condition over `(x, y)`, a conjunct `a == b` with `a` of `x`
/// alone (`Var(1)`) and `b` of `y` (`Var(0)`): the two sides' keys.
fn equality(body: &Expr) -> Option<(Expr, Expr)> {
    use crate::ir::Op::{And, Eq};
    let only = |e: &Expr, v: u32| !effectful(e) && free(e).iter().all(|x| *x == Some(v));
    match body {
        Expr::Op(And, a, b) => equality(a).or_else(|| equality(b)),
        Expr::Op(Eq, a, b) if only(a, 1) && only(b, 0) => Some(((**a).clone(), (**b).clone())),
        Expr::Op(Eq, a, b) if only(a, 0) && only(b, 1) => Some(((**b).clone(), (**a).clone())),
        _ => None,
    }
}

/// A sort key's fields' bytes, complemented where descending.
fn sort_key(k: V, dirs: &[Dir]) -> Okey {
    let fields = match (k, dirs.len()) {
        (V::Tup(ks), n) if n != 1 => ks,
        (k, _) => vec![k],
    };
    let mut out = vec![];
    for (f, d) in fields.iter().zip(dirs) {
        out.extend(f.to_bytes().into_iter().map(|x| if *d == Dir::Desc { !x } else { x }));
    }
    out
}

impl View {
    /// The view of query `f` on `input`, as `ctx` reads it, over `store`.
    pub fn new(lib: &Lib, f: &FnHash, input: &[u8], ctx: Ctx, store: &dyn Read) -> Result<View, Refusal> {
        let (f, args) = prepare(lib, f, input)?;
        View::start(f, lib.clone(), ctx, args, store)
    }

    fn start(f: Arc<Loaded>, lib: Lib, ctx: Ctx, args: Vec<V>, store: &dyn Read) -> Result<View, Refusal> {
        let mut nodes = vec![];
        spine(&f.f.body, &mut vec![], &mut nodes);
        let (deps, held, list) = (BTreeMap::new(), BTreeMap::new(), vec![]);
        let mut v = View {
            f,
            lib,
            ctx,
            args,
            nodes,
            deps,
            held,
            list,
        };
        let d = v.run(store, None, BTreeMap::new())?;
        v.list = d.into_iter().filter_map(|(k, (_, n))| Some((k, n?))).collect();
        Ok(v)
    }

    /// The answer, as the UI holds it.
    pub fn rows(&self) -> Vec<V> {
        self.list.iter().map(|(_, v)| v.clone()).collect()
    }

    /// The changes of one settle, `store` already after them: the patches
    /// that take the answer before them to the answer after.
    pub fn push(&mut self, changes: &[Change], store: &dyn Read) -> Result<Vec<Patch>, Refusal> {
        let net = net(store.schema(), changes);
        let hits = self.hits(store.schema(), &net);
        if !hits.contains_key(&ALL) {
            let d = self.run(store, Some(&net), hits)?;
            return Ok(self.patch(d));
        }
        let fresh = View::start(self.f.clone(), self.lib.clone(), self.ctx.clone(), self.args.clone(), store)?;
        let mut d = Delta::new();
        for (k, v) in &self.list {
            put(&mut d, k.clone(), Some(v.clone()), None);
        }
        for (k, v) in &fresh.list {
            put(&mut d, k.clone(), None, Some(v.clone()));
        }
        let list = std::mem::take(&mut self.list);
        *self = View { list, ..fresh };
        Ok(self.patch(prune(d)))
    }

    /// The elements whose recorded reads a changed row falls in, by node:
    /// each prefix of its key, or of an index entry, at a column's end.
    fn hits(&self, schema: &Schema, net: &Net) -> BTreeMap<usize, BTreeSet<Okey>> {
        let mut hits: BTreeMap<usize, BTreeSet<Okey>> = BTreeMap::new();
        for (table, rows) in net {
            let first = (table.clone(), None, vec![]);
            if self.deps.range(first..).next().is_none_or(|(d, _)| d.0 != *table) {
                continue;
            }
            let t = schema.table(table).expect("a table").1;
            for r in rows.values().flat_map(|(a, b)| [a, b]).flatten() {
                let spans = t.spans(r).expect("a fact parses");
                let key = &r[..spans[t.key - 1].1];
                let mut paths = vec![(None, key.to_vec(), spans[..t.key].iter().map(|s| s.1).collect::<Vec<_>>())];
                for (i, ix) in t.indexes.iter().enumerate() {
                    let (mut entry, mut ends) = (vec![], vec![]);
                    for &c in &ix.columns {
                        entry.extend_from_slice(&r[spans[c].0..spans[c].1]);
                        ends.push(entry.len());
                    }
                    let at = entry.len();
                    entry.extend_from_slice(key);
                    ends.extend(spans[..t.key].iter().map(|s| at + s.1));
                    paths.push((Some(i), entry, ends));
                }
                for (ix, entry, ends) in paths {
                    for n in [0].into_iter().chain(ends) {
                        for (node, k) in self.deps.get(&(table.clone(), ix, entry[..n].to_vec())).into_iter().flatten() {
                            hits.entry(*node).or_default().insert(k.clone());
                        }
                    }
                }
            }
        }
        hits
    }

    /// The root's delta applied to the list, in order-key order, as patches.
    fn patch(&mut self, d: Delta) -> Vec<Patch> {
        let mut out = vec![];
        for (k, (_, new)) in d {
            match (self.list.binary_search_by(|(x, _)| x.cmp(&k)), new) {
                (Ok(at), None) => {
                    self.list.remove(at);
                    out.push(Patch::Remove { at });
                }
                (Ok(at), Some(node)) => {
                    self.list[at].1 = node.clone();
                    out.push(Patch::Update { at, node });
                }
                (Err(at), Some(node)) => {
                    self.list.insert(at, (k, node.clone()));
                    out.push(Patch::Insert { at, node });
                }
                (Err(_), None) => {}
            }
        }
        out
    }

    /// The nodes run over the view's store, and what each element read kept.
    fn run(&mut self, store: &dyn Read, net: Option<&Net>, hits: BTreeMap<usize, BTreeSet<Okey>>) -> Result<Delta, Refusal> {
        let f = self.f.clone();
        let (lib, ctx, args) = (&self.lib, &self.ctx, &self.args);
        let mut cx = Kept {
            store,
            f: &f,
            lib,
            ctx,
            args,
            seen: vec![],
        };
        let d = run(&mut cx, &exprs(&f.f.body), &mut self.nodes, net, hits)?;
        for (node, k, deps) in std::mem::take(&mut cx.seen) {
            if node == ALL {
                for d in deps {
                    self.deps.entry(d).or_default().insert((ALL, vec![]));
                }
                continue;
            }
            for d in self.held.remove(&(node, k.clone())).into_iter().flatten() {
                let s = self.deps.get_mut(&d).expect("held");
                s.remove(&(node, k.clone()));
                if s.is_empty() {
                    self.deps.remove(&d);
                }
            }
            if !deps.is_empty() {
                for d in &deps {
                    self.deps.entry(d.clone()).or_default().insert((node, k.clone()));
                }
                self.held.insert((node, k), deps);
            }
        }
        Ok(d)
    }
}

/// An element of a node's output, read again: from the store for a `Scan`.
fn get(cx: &Kept, (kid, e): (&Node, &Expr), k: &Okey) -> Option<V> {
    match e {
        Expr::Scan(t) => cx.store.get(t, k).map(|r| cx.row(t, &r)),
        _ => kid.out.get(k).cloned(),
    }
}

/// The first `n` elements of a node's output.
fn head(cx: &Kept, (kid, e): (&Node, &Expr), n: usize) -> Vec<(Okey, V)> {
    match e {
        Expr::Scan(t) => {
            let tab = cx.store.schema().table(t).expect("a table").1;
            let rows = cx.store.scan(t, &Scan { limit: n, ..Scan::all() });
            rows.into_iter()
                .map(|r| (r[..tab.key_len(&r).unwrap()].to_vec(), cx.row(t, &r)))
                .collect()
        }
        _ => kid.out.iter().take(n).map(|(k, v)| (k.clone(), v.clone())).collect(),
    }
}

/// What became of each input element touched — by the kid's delta, or by a
/// read of this node's that was hit — as it is now.
fn touched(cx: &Kept, kid: (&Node, &Expr), d: Delta, hit: BTreeSet<Okey>) -> Vec<(Okey, Option<V>)> {
    let now = d.into_iter().map(|(k, (_, b))| (k, b));
    if hit.is_empty() {
        return now.collect();
    }
    let mut out: BTreeMap<Okey, Option<V>> = now.collect();
    for k in hit {
        if let std::collections::btree_map::Entry::Vacant(e) = out.entry(k) {
            let v = get(cx, kid, e.key());
            e.insert(v);
        }
    }
    out.into_iter().collect()
}

/// A list's elements, owned.
fn items(v: V) -> Vec<V> {
    match v {
        V::List(xs) => xs,
        v => panic!("{v:?} is not a list"),
    }
}

/// An element of a node's output set.
fn set(out: &mut BTreeMap<Okey, V>, d: &mut Delta, k: Okey, new: Option<V>) {
    let old = match &new {
        Some(n) => out.insert(k.clone(), n.clone()),
        None => out.remove(&k),
    };
    put(d, k, old, new);
}

/// One node's delta from its input's.
#[allow(clippy::too_many_arguments)]
fn step(
    cx: &mut Kept,
    (i, e): (usize, &Expr),
    node: &mut Node,
    (done, exprs): (&[Node], &[&Expr]),
    deltas: &mut [Delta],
    net: Option<&Net>,
    hit: BTreeSet<Okey>,
) -> Result<Delta, Refusal> {
    let Node { keep, kids, out } = node;
    // Each kid's delta is its one parent's to take.
    let kid = |j: usize| (&done[kids[j]], exprs[kids[j]]);
    let mut delta = |j: usize| std::mem::take(&mut deltas[kids[j]]);
    let mut d = Delta::new();
    match (e, keep) {
        (Expr::Scan(t), _) => match net {
            Some(net) => {
                for (k, (a, b)) in net.get(t).into_iter().flatten() {
                    d.insert(k.clone(), (a.as_ref().map(|r| cx.row(t, r)), b.as_ref().map(|r| cx.row(t, r))));
                }
            }
            None => {
                let none = Node {
                    keep: Keep::Plain,
                    kids: vec![],
                    out: BTreeMap::new(),
                };
                d.extend(head(cx, (&none, e), usize::MAX).into_iter().map(|(k, v)| (k, (None, Some(v)))));
            }
        },
        (Expr::Filter(_, lam) | Expr::Map(_, lam), _) => {
            let filter = matches!(e, Expr::Filter(..));
            for (k, v) in touched(cx, kid(0), delta(0), hit) {
                let new = match v {
                    None => {
                        cx.forget(i, &k);
                        None
                    }
                    Some(v) => each(cx, filter, lam, v, (i, &k))?,
                };
                set(out, &mut d, k, new);
            }
        }
        (Expr::Sort(_, keys, dirs), Keep::Sort(at)) => {
            for (k, v) in touched(cx, kid(0), delta(0), hit) {
                if let Some(was) = at.remove(&k) {
                    set(out, &mut d, was, None);
                }
                match v {
                    None => cx.forget(i, &k),
                    Some(v) => {
                        let place = [order(cx, keys, dirs, &v, (i, &k))?, k.clone()].concat();
                        at.insert(k, place.clone());
                        set(out, &mut d, place, Some(v));
                    }
                }
            }
        }
        (Expr::Take(_, n), Keep::Take(limit)) => {
            let kd = delta(0);
            if net.is_none() {
                *limit = count(cx, n)?;
            } else if kd.is_empty() {
                return Ok(d);
            }
            // Hydrated, the input's delta is its output whole.
            let window = match net.is_none() && !matches!(kid(0).1, Expr::Scan(_)) {
                true => kd.into_iter().filter_map(|(k, (_, v))| Some((k, v?))).take(*limit).collect(),
                false => head(cx, kid(0), *limit).into_iter().collect(),
            };
            let old = std::mem::replace(out, window);
            for k in old.keys().chain(out.keys()).cloned().collect::<BTreeSet<_>>() {
                put(&mut d, k.clone(), old.get(&k).cloned(), out.get(&k).cloned());
            }
        }
        (Expr::Group(_, by, agg), Keep::Group { kept, groups }) => {
            let kd = delta(0);
            let mut dirty = hit;
            for (k, (a, b)) in &kd {
                for (v, sign) in [(a, -1), (b, 1)] {
                    let Some(v) = v else { continue };
                    let g = group_key(cx, by, v)?;
                    let gk = g.to_bytes();
                    let m = groups.entry(gk.clone()).or_default();
                    (m.key, m.n) = (Some(g), m.n + sign);
                    match kept {
                        Some((parts, _)) => tally(cx, parts, &mut m.sums, v, sign as i128)?,
                        None if sign < 0 => drop(m.rows.remove(k)),
                        None => drop(m.rows.insert(k.clone(), v.clone())),
                    }
                    dirty.insert(gk);
                }
            }
            for gk in dirty {
                let Some(m) = groups.get(&gk).filter(|m| m.n > 0) else {
                    groups.remove(&gk);
                    cx.forget(i, &gk);
                    set(out, &mut d, gk, None);
                    continue;
                };
                let key = m.key.clone().expect("a group has its key");
                let members = || m.rows.values().cloned().collect();
                let a = aggregate(cx, kept, agg, key, m.n, &m.sums, members, (i, &gk))?;
                set(out, &mut d, gk, Some(a));
            }
        }
        (Expr::Join(_, _, on), Keep::Join { keys, sides }) => {
            let (da, db) = (delta(0), delta(1));
            let (da, db) = (&da, &db);
            let key = |cx: &mut Kept, s: usize, v: &V| keys.as_ref().map(|ks| join_key(cx, ks, s, v)).transpose();
            let partners = |side: &Side, k: Option<Vec<u8>>| -> Vec<Okey> {
                match k {
                    Some(k) => side.by.get(&k).into_iter().flatten().cloned().collect(),
                    None => side.rows.keys().cloned().collect(),
                }
            };
            for k in da.keys() {
                let gone: Vec<Okey> = out
                    .range(k.clone()..)
                    .take_while(|(p, _)| p.starts_with(k))
                    .map(|(p, _)| p.clone())
                    .collect();
                gone.into_iter().for_each(|p| set(out, &mut d, p, None));
            }
            for (k, (old, _)) in db {
                let Some(y) = old else { continue };
                for a in partners(&sides[0], key(cx, 1, y)?) {
                    set(out, &mut d, [a, k.clone()].concat(), None);
                }
            }
            for (s, dd) in [(0, da), (1, db)] {
                for (k, (old, new)) in dd {
                    for (v, add) in [(old, false), (new, true)] {
                        let Some(v) = v else { continue };
                        let side = &mut sides[s];
                        match add {
                            false => drop(side.rows.remove(k)),
                            true => drop(side.rows.insert(k.clone(), v.clone())),
                        }
                        if let Some(jk) = key(cx, s, v)? {
                            let by = side.by.entry(jk.clone()).or_default();
                            if add {
                                by.insert(k.clone());
                            } else {
                                by.remove(k);
                            }
                            if by.is_empty() {
                                side.by.remove(&jk);
                            }
                        }
                    }
                }
            }
            for (s, dd) in [(0, da), (1, db)] {
                for (k, (_, new)) in dd {
                    let Some(v) = new else { continue };
                    for p in partners(&sides[1 - s], key(cx, s, v)?) {
                        if s == 1 && da.contains_key(&p) {
                            continue;
                        }
                        let w = &sides[1 - s].rows[&p];
                        let (x, y, pk) = if s == 0 {
                            (v, w, [k.clone(), p].concat())
                        } else {
                            (w, v, [p, k.clone()].concat())
                        };
                        if let Some(t) = joined(cx, on, x, y)? {
                            set(out, &mut d, pk, Some(t));
                        }
                    }
                }
            }
        }
        _ => {
            if net.is_none() {
                let xs = items(cx.at(e, vec![], (ALL, &vec![]))?);
                for (n, v) in xs.into_iter().enumerate() {
                    set(out, &mut d, int_enc(n as i64).to_vec(), Some(v));
                }
            }
        }
    }
    Ok(d)
}

// * Routing: a fresh read through the store's indexes

/// A row's column, `Field(Var(0), c)`.
fn column(e: &Expr) -> Option<usize> {
    match e {
        Expr::Field(x, c) if **x == Expr::Var(0) => Some(*c as usize),
        _ => None,
    }
}

/// The comparison `e op row.c` as the row sees it: `row.c flip(op) e`.
fn flip(o: Op) -> Op {
    match o {
        Op::Lt => Op::Gt,
        Op::Le => Op::Ge,
        Op::Gt => Op::Lt,
        Op::Ge => Op::Le,
        o => o,
    }
}

/// The least bytes past every string `p` leads, if any.
fn after(p: &[u8]) -> Option<Vec<u8>> {
    match Scan::prefix(None, p, Dir::Asc, 0).hi {
        Bound::Excluded(h) => Some(h),
        _ => None,
    }
}

/// A read through the key or an index, as `route` chose it.
struct Path {
    lead: usize,
    ranged: bool,
    index: Option<usize>,
    cols: Vec<usize>,
    lo: Vec<u8>,
    hi: Option<Vec<u8>>,
    dir: Dir,
    sorted: bool,
    /// The sorted columns, when the path gives them descending but ties
    /// by the key descending too, where the stable sort keeps key order.
    ties: Option<Vec<usize>>,
}

/// A filter's conjuncts `row.c == e`, and `row.c < e` (`<=`, `>`,
/// `>=`), `e` not of the row and safe to evaluate once, as `(c, bytes
/// of e)` and `(c, op, bytes of e)`; and whether any conjunct is other
/// than an equality (a bound is one: the filter still checks it). One
/// that refuses is left to the filter, which refuses only when a row
/// asks it.
#[allow(clippy::type_complexity)]
fn conjuncts(m: &mut Machine, lam: &Expr) -> (Vec<(usize, Vec<u8>)>, Vec<(usize, Op, Vec<u8>)>, bool) {
    let Expr::Lambda(1, body) = lam else { return (vec![], vec![], true) };
    let (mut todo, mut eqs, mut bounds, mut rest) = (vec![&**body], vec![], vec![], false);
    while let Some(c) = todo.pop() {
        let found = match c {
            Expr::Op(Op::And, a, b) => {
                todo.extend([&**b, &**a]);
                continue;
            }
            Expr::Op(o @ (Op::Eq | Op::Lt | Op::Le | Op::Gt | Op::Ge), a, b) => [(a, b, *o), (b, a, flip(*o))].into_iter().find_map(|(a, b, o)| {
                let ok = !effectful(b) && free(b).iter().all(|v| v.is_some_and(|v| v > 0));
                column(a).filter(|_| ok).map(|c| (c, o, &**b))
            }),
            _ => None,
        };
        let fresh = |(c, o, _): &(usize, Op, &Expr)| *o != Op::Eq || !eqs.iter().any(|(d, _)| d == c);
        let Some((col, o, x)) = found.filter(fresh) else {
            rest = true;
            continue;
        };
        rest |= o != Op::Eq;
        m.stack.push(V::Tup(vec![]));
        let v = m.eval(x);
        m.stack.pop();
        match (v, o) {
            (Ok(v), Op::Eq) => eqs.push((col, v.to_bytes())),
            (Ok(v), o) => bounds.push((col, o, v.to_bytes())),
            (Err(_), _) => rest = true,
        }
    }
    (eqs, bounds, rest)
}

/// A `Take` of a literal count, or else the list whole, routed.
fn routed(m: &mut Machine, e: &Expr) -> Result<Option<Vec<V>>, Refusal> {
    match e {
        Expr::Take(l, n) => match &**n {
            Expr::Lit(Ty::Int, b) => {
                let n = decode(m.cx, &Ty::Int, b)?.int().max(0) as usize;
                route(m, l, n)
            }
            _ => Ok(None),
        },
        e => route(m, e, usize::MAX),
    }
}

/// The first `limit` of `Filter(Scan t)`, `Scan t`, or a `Sort` of one
/// by its columns, read through the key or an index: the longest run
/// of the filter's equalities that leads one, then its bounds on the
/// next column as the range read; in the sort's order when that path
/// gives the stable sort's (ties impossible past the sorted columns,
/// or broken by the key as the stable sort breaks them — or, read
/// descending, broken by the key backwards, which is put right and,
/// when the read stopped inside a run of ties, finished by reading
/// the rest of that run). The count goes to the store when the
/// equalities lead the path and are the whole filter. `None` when
/// nothing serves, or this run wrote the table, which it reads through
/// its own changes.
fn route<'a>(m: &mut Machine<'a>, e: &Expr, limit: usize) -> Result<Option<Vec<V>>, Refusal> {
    let (src, sort) = match e {
        Expr::Sort(l, k, dirs) => (&**l, Some((&**k, dirs))),
        e => (e, None),
    };
    let (table, lam) = match src {
        Expr::Scan(t) => (t, None),
        Expr::Filter(l, p) => match &**l {
            Expr::Scan(t) => (t, Some(&**p)),
            _ => return Ok(None),
        },
        _ => return Ok(None),
    };
    if !m.reads() || m.changes.iter().any(|c| c.table() == table) {
        return Ok(None);
    }
    let (eqs, bounds, rest) = lam.map_or((vec![], vec![], false), |p| conjuncts(m, p));
    let order: Option<Vec<(usize, Dir)>> = match sort {
        None => None,
        Some((Expr::Lambda(1, k), dirs)) => {
            let cols: Option<Vec<usize>> = match &**k {
                Expr::Tuple(ks) if dirs.len() != 1 => ks.iter().map(column).collect(),
                k => column(k).map(|c| vec![c]),
            };
            match cols {
                Some(cs) if cs.len() == dirs.len() => Some(cs.into_iter().zip(dirs.iter().copied()).collect()),
                _ => return Ok(None),
            }
        }
        Some(_) => return Ok(None),
    };
    let store: &'a dyn Read = m.store();
    let tab = store.schema().table(table).ok_or_else(|| Refusal(format!("no table {table}")))?.1;
    let fixed = |c: &usize| eqs.iter().any(|(d, _)| d == c);
    let key: Vec<usize> = (0..tab.key).collect();
    let loose: Vec<usize> = key.iter().copied().filter(|c| !fixed(c)).collect();
    // Each path: the index, its columns then the key's, how many lead
    // a unique one, how many are the index's own.
    let mut paths = vec![(None, key.clone(), tab.key, 0)];
    for (i, ix) in tab.indexes.iter().enumerate() {
        let n = if ix.unique { ix.columns.len() } else { usize::MAX };
        paths.push((Some(i), [&ix.columns[..], &key[..]].concat(), n, ix.columns.len()));
    }
    let uniques: Vec<Vec<usize>> = paths.iter().filter(|p| p.2 != usize::MAX).map(|p| p.1[..p.2].to_vec()).collect();
    let mut best: Option<Path> = None;
    for (index, cols, _, width) in &paths {
        let lead = cols.iter().take_while(|c| fixed(c)).count();
        let rest_cols: Vec<usize> = cols[lead..].iter().copied().filter(|c| !fixed(c)).collect();
        let (dir, sorted, ties) = match &order {
            None => (Dir::Asc, index.is_none() || lead >= *width, None),
            Some(o) => {
                let o: Vec<(usize, Dir)> = o.iter().copied().filter(|(c, _)| !fixed(c)).collect();
                let unique = |n: usize| uniques.iter().any(|u| u.iter().all(|c| fixed(c) || o[..n].iter().any(|(d, _)| d == c)));
                let n = (0..=o.len()).find(|n| unique(*n)).unwrap_or(o.len());
                let dir = o.first().map_or(Dir::Asc, |x| x.1);
                let sorted_cols: Vec<usize> = o[..n].iter().map(|x| x.0).collect();
                if !(o[..n].iter().all(|x| x.1 == dir) && rest_cols.starts_with(&sorted_cols)) {
                    continue;
                }
                let keyed = rest_cols[n..].starts_with(&loose);
                match (unique(n), dir, keyed) {
                    (true, ..) | (false, Dir::Asc, true) => (dir, true, None),
                    (false, Dir::Desc, true) => (dir, true, Some(sorted_cols)),
                    _ => continue,
                }
            }
        };
        let next = cols.get(lead).copied().filter(|c| !fixed(c));
        let ranged = next.is_some_and(|c| bounds.iter().any(|b| b.0 == c));
        if lead == 0 && !ranged && order.is_none() || best.as_ref().is_some_and(|b| (b.lead, b.ranged) >= (lead, ranged)) {
            continue;
        }
        let prefix: Vec<u8> = cols[..lead]
            .iter()
            .flat_map(|c| eqs.iter().find(|(d, _)| d == c).unwrap().1.clone())
            .collect();
        let (mut lo, mut hi) = (prefix.clone(), after(&prefix));
        for (_, o, v) in bounds.iter().filter(|b| Some(b.0) == next) {
            let at = [&prefix[..], v].concat();
            let (l, h) = match o {
                Op::Ge => (Some(at), None),
                Op::Gt => (after(&at), None),
                Op::Lt => (None, Some(at)),
                _ => (None, after(&at)),
            };
            lo = lo.max(l.unwrap_or_default());
            hi = match (hi, h) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
        }
        best = Some(Path {
            lead,
            ranged,
            index: *index,
            cols: cols.clone(),
            lo,
            hi,
            dir,
            sorted,
            ties,
        });
    }
    let Some(p) = best else { return Ok(None) };
    let (row_ty, _) = m.row_ty(table)?;
    let whole = !rest && eqs.iter().all(|(c, _)| p.cols[..p.lead].contains(c));
    let read = if whole && p.sorted { limit } else { usize::MAX };
    let scan = |lo: Vec<u8>, hi: Option<Vec<u8>>, dir: Dir, limit: usize| Scan {
        index: p.index,
        lo: Bound::Included(lo),
        hi: hi.map_or(Bound::Unbounded, Bound::Excluded),
        dir,
        limit,
    };
    let mut rows = store.scan(table, &scan(p.lo.clone(), p.hi.clone(), p.dir, read));
    if let (Some(ties), Some(last)) = (&p.ties, rows.last().filter(|_| read != usize::MAX && rows.len() == read)) {
        // Stopped inside a run of ties, read backwards: the rest of
        // the run sits below the last row read, in the run's prefix.
        let spans = tab.spans(last)?;
        let enc = |cs: &[usize]| -> Vec<u8> { cs.iter().flat_map(|c| last[spans[*c].0..spans[*c].1].to_vec()).collect() };
        let mut end = p.lead;
        for _ in ties {
            end += p.cols[end..].iter().position(|c| !fixed(c)).unwrap() + 1;
        }
        let run = enc(&p.cols[..end]);
        rows.extend(store.scan(table, &scan(run, Some(enc(&p.cols)), Dir::Asc, limit)));
    }
    let mut out = vec![];
    for r in rows {
        let v = decode(m.cx, &row_ty, &r)?;
        if let Some(p) = lam {
            if !m.apply(p, vec![v.clone()])?.truth() {
                continue;
            }
        }
        out.push((r, v));
    }
    let key_of = |r: &[u8]| r[..tab.key_len(r).unwrap()].to_vec();
    if let Some(ties) = &p.ties {
        let tied = |r: &[u8]| -> Vec<Vec<u8>> {
            let s = tab.spans(r).unwrap();
            ties.iter().map(|c| r[s[*c].0..s[*c].1].to_vec()).collect()
        };
        out.sort_by_cached_key(|(r, _)| (std::cmp::Reverse(tied(r)), key_of(r)));
    } else if !p.sorted {
        out.sort_by_cached_key(|(r, _)| key_of(r));
    }
    out.truncate(limit);
    Ok(Some(out.into_iter().map(|x| x.1).collect()))
}

/// A scope's verdict on a row of a table.
pub type Holds<'a> = dyn FnMut(&str, &[u8]) -> Result<bool, Refusal> + 'a;

/// §1.8 A scope's `Filter(Scan t)` over facts, in order: the changes they
/// make to the rows it holds — an edit across its line as the add or the
/// remove it is. `holds` is the scope's verdict on a row of a table; a
/// scope is `Pure`, so a row's verdict is the row's alone and the filter
/// keeps nothing.
pub fn scoped(facts: &[Change], holds: &mut Holds) -> Result<Vec<Change>, Refusal> {
    let mut out = vec![];
    for c in facts {
        let t = || c.table().to_string();
        let kept = match c {
            Change::Add { row, .. } | Change::Remove { row, .. } => holds(c.table(), row)?.then(|| c.clone()),
            Change::Edit { old, new, .. } => match (holds(c.table(), old)?, holds(c.table(), new)?) {
                (true, true) => Some(c.clone()),
                (true, false) => Some(Change::Remove {
                    table: t(),
                    row: old.clone(),
                }),
                (false, true) => Some(Change::Add {
                    table: t(),
                    row: new.clone(),
                }),
                (false, false) => None,
            },
        };
        out.extend(kept);
    }
    Ok(out)
}
