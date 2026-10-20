//! §1.10 Views, incremental per operator; and §1.8's filter of a feed.
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
//! list the interpreter would give — a row's key for `Scan`, kept by
//! `Filter` and `Map`; a group's key; the two sides' concatenated for a
//! `Join`; the sort fields' encodings (complemented for `Desc`) then the
//! input's for `Sort`, which is the stable sort; a position for a leaf.
//! Encodings are prefix-free (§1.1), so concatenations of them are too.
//!
//! **What a lambda reads** — a `Lookup`, a filtered `Scan` in a `Map` —
//! is recorded as the store reads it made: the key, or the index and the
//! prefix it scanned ([`crate::interp`] routes a filter's equalities to
//! the store's indexes, so a read is as narrow as they make it). A change
//! to a row a recorded read covers re-evaluates the elements that made
//! it, and only those. A read in a `Join`'s or a `Group`'s key, in a
//! `Take`'s count or in a leaf rebuilds the view when it is hit.
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
//! contract (`tests/views.rs`): after every push the answer is a fresh
//! evaluation's ([`query`]) and the patches splice. A push that refuses
//! leaves the view stale; make it again.
//!
//! [`scoped`] is a scope's `Filter(Scan t)` over one entry's facts (§1.8):
//! the changes it makes to a connection's union.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::codec::{int_enc, Typed};
use crate::hash::FnHash;
use crate::interp::{decode, effectful, free, parse, Interp, Loaded, Machine, V};
use crate::ir::{Effect, Expr, Std};
use crate::module::Lib;
use crate::peer::Ctx;
use crate::schema::Schema;
use crate::store::{Change, Dir, Read, Scan};
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

/// What a node's lambdas are evaluated in.
struct Cx<'a> {
    store: &'a dyn Read,
    f: &'a Loaded,
    lib: &'a Lib,
    ctx: &'a Ctx,
    args: &'a [V],
    /// Elements evaluated this push, with what they read: `(node, okey)`.
    seen: Vec<(usize, Okey, Vec<Dep>)>,
}

impl Cx<'_> {
    /// `e` with `vars` bound over the arguments (the last is `Var(0)`), its
    /// reads kept against `(node, okey)`.
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

    /// A lambda applied.
    fn call(&mut self, lam: &Expr, args: Vec<V>, at: (usize, &Okey)) -> Result<V, Refusal> {
        match lam {
            Expr::Lambda(_, body) => self.at(body, args, at),
            _ => Err(Refusal::new("not a lambda")),
        }
    }

    /// An element gone: what it read no longer matters.
    fn forget(&mut self, node: usize, k: &Okey) {
        self.seen.push((node, k.clone(), vec![]));
    }

    fn row(&self, table: &str, b: &[u8]) -> V {
        let t = self.store.schema().table(table).expect("a table").1;
        decode(&self.f.cx, &t.row_ty(), b).expect("a stored row decodes")
    }
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

#[derive(Clone, Debug)]
enum Op {
    Scan(String),
    Filter(Expr),
    Map(Expr),
    Sort {
        keys: Expr,
        dirs: Vec<Dir>,
        /// Each input element's place in the output.
        at: BTreeMap<Okey, Okey>,
    },
    Take(Expr, usize),
    Group {
        by: Expr,
        agg: Expr,
        kept: Option<(Vec<Part>, bool)>,
        groups: BTreeMap<Okey, Members>,
    },
    Join {
        on: Expr,
        keys: Option<(Expr, Expr)>,
        sides: Box<[Side; 2]>,
    },
    Leaf(Expr),
}

#[derive(Clone, Debug)]
struct Node {
    op: Op,
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

/// A query's answer evaluated afresh by the interpreter: what a view is
/// held to.
pub fn query(lib: &Lib, f: &FnHash, input: &[u8], ctx: &Ctx, store: &dyn Read) -> Result<Vec<V>, Refusal> {
    let (f, args) = prepare(lib, f, input)?;
    let callee = |h: &FnHash| lib.get(h);
    let mut m = Machine::new(store, ctx, &callee, &f.cx);
    m.stack = args;
    Ok(m.eval(&f.f.body)?.list().to_vec())
}

/// The spine of `e` as nodes, children first; the root is the last.
fn compile(e: &Expr, nodes: &mut Vec<Node>) -> usize {
    let b = |e: &Expr| e.clone();
    let (op, kids) = match e {
        Expr::Scan(t) => (Op::Scan(t.clone()), vec![]),
        Expr::Filter(l, p) => (Op::Filter(b(p)), vec![compile(l, nodes)]),
        Expr::Map(l, f) => (Op::Map(b(f)), vec![compile(l, nodes)]),
        Expr::Sort(l, keys, dirs) => {
            let at = BTreeMap::new();
            (
                Op::Sort {
                    keys: b(keys),
                    dirs: dirs.clone(),
                    at,
                },
                vec![compile(l, nodes)],
            )
        }
        Expr::Take(l, n) => (Op::Take(b(n), 0), vec![compile(l, nodes)]),
        Expr::Group(l, by, agg) => {
            let (kept, groups) = (kept(agg), BTreeMap::new());
            (
                Op::Group {
                    by: b(by),
                    agg: b(agg),
                    kept,
                    groups,
                },
                vec![compile(l, nodes)],
            )
        }
        Expr::Join(l, r, on) => {
            let keys = match &**on {
                Expr::Lambda(2, body) => equality(body),
                _ => None,
            };
            let kids = vec![compile(l, nodes), compile(r, nodes)];
            (
                Op::Join {
                    on: b(on),
                    keys,
                    sides: Box::default(),
                },
                kids,
            )
        }
        e => (Op::Leaf(e.clone()), vec![]),
    };
    nodes.push(Node {
        op,
        kids,
        out: BTreeMap::new(),
    });
    nodes.len() - 1
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
        compile(&f.f.body, &mut nodes);
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

    /// Every node, children first, from their deltas and the elements a
    /// read of theirs was hit at; `net` is `None` at the start.
    fn run(&mut self, store: &dyn Read, net: Option<&Net>, mut hits: BTreeMap<usize, BTreeSet<Okey>>) -> Result<Delta, Refusal> {
        let (f, lib, ctx, args) = (&self.f, &self.lib, &self.ctx, &self.args);
        let mut cx = Cx {
            store,
            f,
            lib,
            ctx,
            args,
            seen: vec![],
        };
        let mut deltas: Vec<Delta> = Vec::with_capacity(self.nodes.len());
        for i in 0..self.nodes.len() {
            let (done, rest) = self.nodes.split_at_mut(i);
            let hit = hits.remove(&i).unwrap_or_default();
            deltas.push(prune(step(&mut cx, i, &mut rest[0], done, &deltas, net, hit)?));
        }
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
        Ok(deltas.pop().unwrap_or_default())
    }
}

/// An element of a node's output, read again: from the store for a `Scan`.
fn get(cx: &Cx, kid: &Node, k: &Okey) -> Option<V> {
    match &kid.op {
        Op::Scan(t) => cx.store.get(t, k).map(|r| cx.row(t, &r)),
        _ => kid.out.get(k).cloned(),
    }
}

/// The first `n` elements of a node's output.
fn first(cx: &Cx, kid: &Node, n: usize) -> Vec<(Okey, V)> {
    match &kid.op {
        Op::Scan(t) => {
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
fn touched(cx: &Cx, kid: &Node, d: &Delta, hit: BTreeSet<Okey>) -> Vec<(Okey, Option<V>)> {
    let mut out: BTreeMap<Okey, Option<V>> = d.iter().map(|(k, (_, b))| (k.clone(), b.clone())).collect();
    for k in hit {
        if let std::collections::btree_map::Entry::Vacant(e) = out.entry(k) {
            let v = get(cx, kid, e.key());
            e.insert(v);
        }
    }
    out.into_iter().collect()
}

fn set(out: &mut BTreeMap<Okey, V>, d: &mut Delta, k: Okey, new: Option<V>) {
    let old = match &new {
        Some(n) => out.insert(k.clone(), n.clone()),
        None => out.remove(&k),
    };
    put(d, k, old, new);
}

/// One node's delta from its input's.
fn step(cx: &mut Cx, i: usize, node: &mut Node, done: &[Node], deltas: &[Delta], net: Option<&Net>, hit: BTreeSet<Okey>) -> Result<Delta, Refusal> {
    let Node { op, kids, out } = node;
    let input = |j: usize| (&done[kids[j]], &deltas[kids[j]]);
    let mut d = Delta::new();
    let none: Okey = vec![];
    let filter = matches!(op, Op::Filter(_));
    match op {
        Op::Scan(t) => match net {
            Some(net) => {
                for (k, (a, b)) in net.get(t).into_iter().flatten() {
                    d.insert(k.clone(), (a.as_ref().map(|r| cx.row(t, r)), b.as_ref().map(|r| cx.row(t, r))));
                }
            }
            None => {
                let scan = Node {
                    op: Op::Scan(t.clone()),
                    kids: vec![],
                    out: BTreeMap::new(),
                };
                d.extend(first(cx, &scan, usize::MAX).into_iter().map(|(k, v)| (k, (None, Some(v)))));
            }
        },
        Op::Filter(lam) | Op::Map(lam) => {
            let (kid, kd) = input(0);
            for (k, v) in touched(cx, kid, kd, hit) {
                let new = match v {
                    None => {
                        cx.forget(i, &k);
                        None
                    }
                    Some(v) if filter => cx.call(lam, vec![v.clone()], (i, &k))?.truth().then_some(v),
                    Some(v) => Some(cx.call(lam, vec![v], (i, &k))?),
                };
                set(out, &mut d, k, new);
            }
        }
        Op::Sort { keys, dirs, at } => {
            let (kid, kd) = input(0);
            for (k, v) in touched(cx, kid, kd, hit) {
                if let Some(was) = at.remove(&k) {
                    set(out, &mut d, was, None);
                }
                match v {
                    None => cx.forget(i, &k),
                    Some(v) => {
                        let place = [sort_key(cx.call(keys, vec![v.clone()], (i, &k))?, dirs), k.clone()].concat();
                        at.insert(k, place.clone());
                        set(out, &mut d, place, Some(v));
                    }
                }
            }
        }
        Op::Take(n, limit) => {
            let (kid, kd) = input(0);
            if net.is_none() {
                *limit = cx.at(n, vec![], (ALL, &none))?.int().max(0) as usize;
            } else if kd.is_empty() {
                return Ok(d);
            }
            let old = std::mem::replace(out, first(cx, kid, *limit).into_iter().collect());
            for k in old.keys().chain(out.keys()).cloned().collect::<BTreeSet<_>>() {
                put(&mut d, k.clone(), old.get(&k).cloned(), out.get(&k).cloned());
            }
        }
        Op::Group { by, agg, kept, groups } => {
            let (_, kd) = input(0);
            let mut dirty = hit;
            for (k, (a, b)) in kd {
                for (v, sign) in [(a, -1), (b, 1)] {
                    let Some(v) = v else { continue };
                    let g = cx.call(by, vec![v.clone()], (ALL, &none))?;
                    let gk = g.to_bytes();
                    let m = groups.entry(gk.clone()).or_default();
                    (m.key, m.n) = (Some(g), m.n + sign);
                    match kept {
                        Some((parts, _)) => {
                            let terms = parts.iter().filter_map(|p| match p {
                                Part::Sum(t) => Some(t),
                                _ => None,
                            });
                            m.sums.resize(terms.clone().count(), 0);
                            for (s, t) in m.sums.iter_mut().zip(terms) {
                                let x = match t {
                                    None => v.int(),
                                    Some(t) => cx.at(t, vec![v.clone()], (ALL, &none))?.int(),
                                };
                                *s += sign as i128 * x as i128;
                            }
                        }
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
                let a = match kept {
                    None => cx.call(agg, vec![key.clone(), V::List(m.rows.values().cloned().collect())], (i, &gk))?,
                    Some((parts, tuple)) => {
                        let (mut vs, mut sums) = (vec![], m.sums.iter());
                        for p in parts.iter() {
                            vs.push(match p {
                                Part::Count => V::Int(m.n),
                                Part::Sum(_) => crate::std::total(*sums.next().unwrap())?,
                                Part::Other(e) => cx.at(e, vec![key.clone(), V::List(vec![])], (i, &gk))?,
                            });
                        }
                        if *tuple {
                            V::Tup(vs)
                        } else {
                            vs.remove(0)
                        }
                    }
                };
                set(out, &mut d, gk, Some(V::Tup(vec![key, a])));
            }
        }
        Op::Join { on, keys, sides } => {
            let (da, db) = (input(0).1, input(1).1);
            let mut key = |cx: &mut Cx, s: usize, v: &V| -> Result<Option<Vec<u8>>, Refusal> {
                let Some(ks) = keys else { return Ok(None) };
                let vars = if s == 0 {
                    vec![v.clone(), V::Tup(vec![])]
                } else {
                    vec![V::Tup(vec![]), v.clone()]
                };
                Ok(Some(cx.at(if s == 0 { &ks.0 } else { &ks.1 }, vars, (ALL, &none))?.to_bytes()))
            };
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
                        let w = sides[1 - s].rows[&p].clone();
                        let (x, y, pk) = if s == 0 {
                            (v.clone(), w, [k.clone(), p].concat())
                        } else {
                            (w, v.clone(), [p, k.clone()].concat())
                        };
                        if cx.call(on, vec![x.clone(), y.clone()], (ALL, &none))?.truth() {
                            set(out, &mut d, pk, Some(V::Tup(vec![x, y])));
                        }
                    }
                }
            }
        }
        Op::Leaf(e) => {
            if net.is_none() {
                for (n, v) in cx.at(e, vec![], (ALL, &none))?.list().iter().enumerate() {
                    set(out, &mut d, int_enc(n as i64).to_vec(), Some(v.clone()));
                }
            }
        }
    }
    Ok(d)
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
