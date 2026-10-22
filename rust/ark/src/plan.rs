//! §1.10, §1.16 Views over typed plans, incremental per operator.
//!
//! A [`Plan<S, T>`] is a query a view keeps: a tree of operators over the
//! rows of `S`'s tables, built by [`scan`] and the methods on it —
//! `filter`, `map`, `sort`, `take`, `group`, `join` — each taking plain
//! Rust closures. An operator is `impl<T>` over its element type: rows,
//! and what a `map` makes of them, compared by their derived `PartialEq`,
//! ordered by **order keys** — bytes whose `memcmp` order is the element's
//! place in the list ([`crate::row::Key`]): a row's key for `scan`, kept
//! by `filter` and `map`; the sort key's bytes, then the input's order key,
//! for `sort` (a stable sort; [`Desc`] complements a key's bytes); a
//! group's key; the two sides' concatenated for a `join`. Keys are
//! prefix-free, so concatenations of them are too.
//!
//! | operator | keeps | on a delta |
//! |---|---|---|
//! | `scan` | nothing | the table's changes, net per key |
//! | `filter`, `map`, `sort` | each element's place and output (and input, when the closure reads) | the closure again on each element touched, placed again |
//! | `take` | its input, in order | the window again |
//! | `group` | each group's members | the touched groups' aggregates again |
//! | `join` | each side by its key | for an element of one side, probe the other |
//!
//! **What a closure reads** — the `_with` forms take `&Db<S>` — is recorded
//! as the reads it made ([`crate::db::recorded`]): a key, or a prefix of the
//! key or an index's entries. A change to a row a recorded read covers runs
//! the closure again for the elements whose run made it, and only those. A
//! view's own construction may read too (a playlist's owner, before its
//! items): a change to what it read builds the view again.
//!
//! [`View::push`] takes the changes of one settle — a rebase's included —
//! with the store already after them, and answers the [`Patch`]es that
//! splice the list a UI holds into the new answer. The contract: after
//! every push the list is what a fresh run of the plan gives, and the
//! patches splice the old list into it.

use std::collections::{btree_map, BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::db::{read, recorded, Db, Dep, Tables};
use crate::peer::Ctx;
use crate::row::{Key, Row, Value};
use crate::store::{Change, Read, Store};
use crate::Refusal;

/// An element's place in a list, as bytes.
pub type Okey = Vec<u8>;

/// Per element whose value moved: what it was, what it is.
type Delta<T> = BTreeMap<Okey, (Option<T>, Option<T>)>;

fn put<T>(d: &mut Delta<T>, k: Okey, old: Option<T>, new: Option<T>) {
    match d.entry(k) {
        btree_map::Entry::Occupied(mut e) => e.get_mut().1 = new,
        btree_map::Entry::Vacant(e) => {
            e.insert((old, new));
        }
    }
}

fn prune<T: PartialEq>(mut d: Delta<T>) -> Delta<T> {
    d.retain(|_, (a, b)| a != b);
    d
}

/// A row before and after, as its bytes.
type Moved = (Option<Vec<u8>>, Option<Vec<u8>>);
/// Per table, per key, how a row moved.
type Net = BTreeMap<String, BTreeMap<Vec<u8>, Moved>>;
/// Per prefix, the elements whose reads covered it.
type ByPrefix = BTreeMap<Vec<u8>, BTreeSet<Okey>>;
/// A closure over an element, reading the store.
type WithDb<S, A, B> = Box<dyn for<'t> Fn(&Db<'t, S>, &A) -> B + Send>;
/// An element's join key.
type KeyFn<T> = Box<dyn Fn(&T) -> Vec<u8> + Send>;

/// What a list's element may be.
pub trait Elem: Clone + PartialEq + Send + 'static {}
impl<T: Clone + PartialEq + Send + 'static> Elem for T {}

/// One settle's changes, as every operator reads them: per table, per key,
/// the row before and after; and every key and index entry a changed row
/// had or has, which is what a recorded read is hit by.
pub struct Pushed {
    net: Net,
    touched: BTreeMap<(String, Option<usize>), BTreeSet<Vec<u8>>>,
}

impl Pushed {
    pub fn of(store: &Store, changes: &[Change]) -> Pushed {
        let mut net = Net::new();
        let mut touched: BTreeMap<(String, Option<usize>), BTreeSet<Vec<u8>>> = BTreeMap::new();
        for c in changes {
            let (old, new) = match c {
                Change::Add { row, .. } => (None, Some(row)),
                Change::Remove { row, .. } => (Some(row), None),
                Change::Edit { old, new, .. } => (Some(old), Some(new)),
            };
            let t = c.table();
            for r in [old, new].into_iter().flatten() {
                let Ok((key, entries)) = store.paths_of(t, r) else { continue };
                touched.entry((t.into(), None)).or_default().insert(key);
                for (i, e) in entries.into_iter().enumerate() {
                    touched.entry((t.into(), Some(i))).or_default().insert(e);
                }
            }
            let r = new.or(old).unwrap();
            let Ok(key) = store.key_of(t, r) else { continue };
            let e = net.entry(t.into()).or_default().entry(key).or_insert((old.cloned(), None));
            e.1 = new.cloned();
        }
        for t in net.values_mut() {
            t.retain(|_, (a, b)| a != b);
        }
        Pushed { net, touched }
    }

    /// Whether anything a read in `deps` covered moved.
    fn hits(&self, deps: &[Dep]) -> bool {
        deps.iter().any(|(t, i, p)| {
            self.touched
                .get(&((*t).to_string(), *i))
                .is_some_and(|es| es.range::<Vec<u8>, _>(p..).next().is_some_and(|e| e.starts_with(p)))
        })
    }
}

/// What each element's run of a closure read, and which elements a moved
/// row reaches through it.
struct Deps {
    by: BTreeMap<(&'static str, Option<usize>), ByPrefix>,
    of: BTreeMap<Okey, Vec<Dep>>,
}

impl Deps {
    fn new() -> Deps {
        Deps {
            by: BTreeMap::new(),
            of: BTreeMap::new(),
        }
    }

    fn clear(&mut self, k: &Okey) {
        for (t, i, p) in self.of.remove(k).unwrap_or_default() {
            if let Some(m) = self.by.get_mut(&(t, i)) {
                if let Some(s) = m.get_mut(&p) {
                    s.remove(k);
                    if s.is_empty() {
                        m.remove(&p);
                    }
                }
            }
        }
    }

    fn set(&mut self, k: &Okey, deps: Vec<Dep>) {
        self.clear(k);
        if deps.is_empty() {
            return;
        }
        for (t, i, p) in &deps {
            self.by.entry((*t, *i)).or_default().entry(p.clone()).or_default().insert(k.clone());
        }
        self.of.insert(k.clone(), deps);
    }

    /// The elements a settle's moved rows reach.
    fn hit(&self, p: &Pushed) -> BTreeSet<Okey> {
        let mut out = BTreeSet::new();
        for ((t, i), m) in &self.by {
            let Some(es) = p.touched.get(&((*t).to_string(), *i)) else { continue };
            for e in es {
                for n in 0..=e.len() {
                    if let Some(ks) = m.get(&e[..n]) {
                        out.extend(ks.iter().cloned());
                    }
                }
            }
        }
        out
    }
}

/// What an operator evaluates in: the store, and who reads.
pub struct Cx<'a> {
    pub store: &'a Store,
    pub ctx: &'a Ctx,
}

/// One operator of a plan, over elements `T`.
trait Op<S, T>: Send {
    /// Every element, in order; kept when `keep`, for the pushes after.
    fn hydrate(&mut self, cx: &Cx, keep: bool) -> Vec<(Okey, T)>;
    /// What a settle did to its output.
    fn push(&mut self, cx: &Cx, p: &Pushed) -> Delta<T>;
}

/// A closure over an element, pure or reading through `&Db`.
enum F<S: Tables, A: ?Sized, B> {
    Pure(Box<dyn Fn(&A) -> B + Send>),
    With(WithDb<S, A, B>),
}

impl<S: Tables, A: ?Sized, B> F<S, A, B> {
    /// Its answer for `a`, and what it read.
    fn run(&self, store: &Store, a: &A) -> (B, Vec<Dep>) {
        match self {
            F::Pure(f) => (f(a), vec![]),
            F::With(f) => recorded::<S, B>(store, |db| f(db, a)),
        }
    }
}

// * The operators

/// A row of a table, read by `decode`.
type Decode<T> = Box<dyn Fn(&Store, &[u8]) -> T + Send>;

/// Every row of one table, by its key, each as `decode` reads it.
struct ScanOp<T> {
    table: String,
    decode: Decode<T>,
}

impl<S: Tables, T: Elem> Op<S, T> for ScanOp<T> {
    fn hydrate(&mut self, cx: &Cx, _: bool) -> Vec<(Okey, T)> {
        cx.store
            .keyed(&self.table)
            .map(|(k, b)| (k.to_vec(), (self.decode)(cx.store, b)))
            .collect()
    }

    fn push(&mut self, cx: &Cx, p: &Pushed) -> Delta<T> {
        let dec = |b: &Option<Vec<u8>>| b.as_ref().map(|b| (self.decode)(cx.store, b));
        p.net
            .get(&self.table)
            .map(|t| t.iter().map(|(k, (a, b))| (k.clone(), (dec(a), dec(b)))).collect())
            .unwrap_or_default()
    }
}

/// One element at a time: `f` makes of each input element what goes
/// before its place and what it puts out, if anything — a filter keeps or
/// drops it, a map makes another of it, a sort puts its sort key first.
struct EachOp<S: Tables, T, U> {
    input: Box<dyn Op<S, T>>,
    f: F<S, T, (Okey, Option<U>)>,
    /// Per input element: itself (when `f` reads, to run it again), and its
    /// output's place and value.
    kept: BTreeMap<Okey, (Option<T>, Okey, Option<U>)>,
    deps: Deps,
}

impl<S: Tables, T: Elem, U: Elem> EachOp<S, T, U> {
    /// The element at `k` through `f`: its output's place and value, kept
    /// when `keep`.
    fn run(&mut self, store: &Store, k: &Okey, t: T, keep: bool) -> (Okey, Option<U>) {
        let ((mut at, u), deps) = self.f.run(store, &t);
        at.extend_from_slice(k);
        if keep {
            self.deps.set(k, deps);
            let reads = matches!(self.f, F::With(_));
            self.kept.insert(k.clone(), (reads.then_some(t), at.clone(), u.clone()));
        }
        (at, u)
    }

    /// The element at `k` gone from the output, and `new` put in its place.
    fn place(&mut self, store: &Store, out: &mut Delta<U>, k: Okey, new: Option<T>) {
        if let Some((_, at, Some(u))) = self.kept.remove(&k) {
            put(out, at, Some(u), None);
        }
        self.deps.clear(&k);
        if let Some(t) = new {
            if let (at, Some(u)) = self.run(store, &k, t, true) {
                put(out, at, None, Some(u));
            }
        }
    }
}

impl<S: Tables, T: Elem, U: Elem> Op<S, U> for EachOp<S, T, U> {
    fn hydrate(&mut self, cx: &Cx, keep: bool) -> Vec<(Okey, U)> {
        let input = self.input.hydrate(cx, keep);
        let mut out: Vec<(Okey, U)> = input
            .into_iter()
            .filter_map(|(k, t)| match self.run(cx.store, &k, t, keep) {
                (at, Some(u)) => Some((at, u)),
                _ => None,
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    fn push(&mut self, cx: &Cx, p: &Pushed) -> Delta<U> {
        let d = self.input.push(cx, p);
        let mut dirty = self.deps.hit(p);
        let mut out = Delta::new();
        for (k, (_, new)) in d {
            dirty.remove(&k);
            self.place(cx.store, &mut out, k, new);
        }
        for k in dirty {
            let Some((Some(t), ..)) = self.kept.get(&k).cloned() else { continue };
            self.place(cx.store, &mut out, k, Some(t));
        }
        prune(out)
    }
}

struct TakeOp<S, T> {
    input: Box<dyn Op<S, T>>,
    n: usize,
    all: BTreeMap<Okey, T>,
}

impl<S: Tables, T: Elem> Op<S, T> for TakeOp<S, T> {
    fn hydrate(&mut self, cx: &Cx, keep: bool) -> Vec<(Okey, T)> {
        let all = self.input.hydrate(cx, keep);
        let out = all.iter().take(self.n).cloned().collect();
        if keep {
            self.all = all.into_iter().collect();
        }
        out
    }

    fn push(&mut self, cx: &Cx, p: &Pushed) -> Delta<T> {
        let d = self.input.push(cx, p);
        if d.is_empty() {
            return d;
        }
        let window = |all: &BTreeMap<Okey, T>, n| all.iter().take(n).map(|(k, t)| (k.clone(), t.clone())).collect::<BTreeMap<_, _>>();
        let before = window(&self.all, self.n);
        for (k, (_, new)) in d {
            match new {
                Some(t) => self.all.insert(k, t),
                None => self.all.remove(&k),
            };
        }
        let after = window(&self.all, self.n);
        let mut out = Delta::new();
        for k in before.keys().chain(after.keys()) {
            out.insert(k.clone(), (before.get(k).cloned(), after.get(k).cloned()));
        }
        prune(out)
    }
}

/// A group's members, in their input's order: what an aggregate reads.
pub type Members<'a, T> = btree_map::Values<'a, Okey, T>;

type Agg<S, K, T, A> = Box<dyn for<'t, 'm> Fn(&Db<'t, S>, &K, Members<'m, T>) -> A + Send>;

struct Group<K, T, A> {
    key: K,
    members: BTreeMap<Okey, T>,
    agg: Option<A>,
}

struct GroupOp<S: Tables, T, K, A> {
    input: Box<dyn Op<S, T>>,
    by: Box<dyn Fn(&T) -> K + Send>,
    agg: Agg<S, K, T, A>,
    groups: BTreeMap<Okey, Group<K, T, A>>,
    of: BTreeMap<Okey, Okey>,
    deps: Deps,
}

impl<S: Tables, T: Elem, K: Key + Clone + Send + 'static, A: Elem> Op<S, A> for GroupOp<S, T, K, A> {
    fn hydrate(&mut self, cx: &Cx, keep: bool) -> Vec<(Okey, A)> {
        let mut groups: BTreeMap<Okey, (K, BTreeMap<Okey, T>)> = BTreeMap::new();
        for (k, t) in self.input.hydrate(cx, keep) {
            let key = (self.by)(&t);
            let g = key.key();
            if keep {
                self.of.insert(k.clone(), g.clone());
            }
            groups.entry(g).or_insert_with(|| (key, BTreeMap::new())).1.insert(k, t);
        }
        let mut out = vec![];
        for (g, (key, members)) in groups {
            let (a, deps) = recorded::<S, A>(cx.store, |db| (self.agg)(db, &key, members.values()));
            out.push((g.clone(), a.clone()));
            if keep {
                self.deps.set(&g, deps);
                self.groups.insert(g, Group { key, members, agg: Some(a) });
            }
        }
        out
    }

    fn push(&mut self, cx: &Cx, p: &Pushed) -> Delta<A> {
        let d = self.input.push(cx, p);
        let mut touched = self.deps.hit(p);
        let mut before: BTreeMap<Okey, Option<A>> = BTreeMap::new();
        for (k, (_, new)) in d {
            if let Some(g) = self.of.remove(&k) {
                let grp = self.groups.get_mut(&g).expect("a member's group");
                before.entry(g.clone()).or_insert_with(|| grp.agg.clone());
                grp.members.remove(&k);
                touched.insert(g);
            }
            if let Some(t) = new {
                let key = (self.by)(&t);
                let g = key.key();
                before.entry(g.clone()).or_insert_with(|| self.groups.get(&g).and_then(|x| x.agg.clone()));
                let grp = self.groups.entry(g.clone()).or_insert_with(|| Group {
                    key,
                    members: BTreeMap::new(),
                    agg: None,
                });
                grp.members.insert(k.clone(), t);
                self.of.insert(k, g.clone());
                touched.insert(g);
            }
        }
        let mut out = Delta::new();
        for g in touched {
            let was = match before.get(&g) {
                Some(a) => a.clone(),
                None => self.groups.get(&g).and_then(|x| x.agg.clone()),
            };
            let Some(grp) = self.groups.get(&g).filter(|x| !x.members.is_empty()) else {
                self.groups.remove(&g);
                self.deps.clear(&g);
                put(&mut out, g, was, None);
                continue;
            };
            let (a, deps) = recorded::<S, A>(cx.store, |db| (self.agg)(db, &grp.key, grp.members.values()));
            self.deps.set(&g, deps);
            self.groups.get_mut(&g).unwrap().agg = Some(a.clone());
            put(&mut out, g, was, Some(a));
        }
        prune(out)
    }
}

struct Side<T> {
    all: BTreeMap<Okey, (Vec<u8>, T)>,
    by: BTreeMap<Vec<u8>, BTreeSet<Okey>>,
}

impl<T: Clone> Side<T> {
    fn new() -> Side<T> {
        Side {
            all: BTreeMap::new(),
            by: BTreeMap::new(),
        }
    }
    fn add(&mut self, k: Okey, key: Vec<u8>, t: T) {
        self.by.entry(key.clone()).or_default().insert(k.clone());
        self.all.insert(k, (key, t));
    }
    fn remove(&mut self, k: &Okey) -> Option<(Vec<u8>, T)> {
        let (key, t) = self.all.remove(k)?;
        if let Some(s) = self.by.get_mut(&key) {
            s.remove(k);
            if s.is_empty() {
                self.by.remove(&key);
            }
        }
        Some((key, t))
    }
    fn partners(&self, key: &[u8]) -> Vec<(Okey, T)> {
        self.by
            .get(key)
            .map(|ks| ks.iter().map(|k| (k.clone(), self.all[k].1.clone())).collect())
            .unwrap_or_default()
    }
}

struct JoinOp<S, T, U> {
    left: Box<dyn Op<S, T>>,
    right: Box<dyn Op<S, U>>,
    lk: KeyFn<T>,
    rk: KeyFn<U>,
    l: Side<T>,
    r: Side<U>,
}

fn pair(a: &[u8], b: &[u8]) -> Okey {
    [a, b].concat()
}

impl<S: Tables, T: Elem, U: Elem> Op<S, (T, U)> for JoinOp<S, T, U> {
    fn hydrate(&mut self, cx: &Cx, keep: bool) -> Vec<(Okey, (T, U))> {
        let (mut l, mut r) = (Side::new(), Side::new());
        for (k, t) in self.left.hydrate(cx, keep) {
            l.add(k, (self.lk)(&t), t);
        }
        for (k, u) in self.right.hydrate(cx, keep) {
            r.add(k, (self.rk)(&u), u);
        }
        let mut out = vec![];
        for (a, (key, t)) in &l.all {
            for (b, u) in r.partners(key) {
                out.push((pair(a, &b), (t.clone(), u)));
            }
        }
        if keep {
            (self.l, self.r) = (l, r);
        }
        out
    }

    fn push(&mut self, cx: &Cx, p: &Pushed) -> Delta<(T, U)> {
        let (dl, dr) = (self.left.push(cx, p), self.right.push(cx, p));
        let mut out = Delta::new();
        for (a, (_, new)) in dl {
            if let Some((key, t)) = self.l.remove(&a) {
                for (b, u) in self.r.partners(&key) {
                    put(&mut out, pair(&a, &b), Some((t.clone(), u)), None);
                }
            }
            if let Some(t) = new {
                let key = (self.lk)(&t);
                for (b, u) in self.r.partners(&key) {
                    put(&mut out, pair(&a, &b), None, Some((t.clone(), u)));
                }
                self.l.add(a, key, t);
            }
        }
        for (b, (_, new)) in dr {
            if let Some((key, u)) = self.r.remove(&b) {
                for (a, t) in self.l.partners(&key) {
                    put(&mut out, pair(&a, &b), Some((t, u.clone())), None);
                }
            }
            if let Some(u) = new {
                let key = (self.rk)(&u);
                for (a, t) in self.l.partners(&key) {
                    put(&mut out, pair(&a, &b), None, Some((t, u.clone())));
                }
                self.r.add(b, key, u);
            }
        }
        prune(out)
    }
}

// * Plans

/// A query a view keeps: operators over `S`'s tables, answering `T`s.
pub struct Plan<S, T> {
    op: Box<dyn Op<S, T>>,
}

/// Every row of `R`'s table, in key order.
pub fn scan<S: Tables, R: Row>() -> Plan<S, R> {
    scan_raw(R::TABLE, |_, b| {
        R::decode(b).unwrap_or_else(|e| panic!("a row of {} reads: {e}", R::TABLE))
    })
}

/// Every row of the table named `table`, in key order, each as `decode`
/// reads it: a plan over rows without a Rust type ([`crate::dynamic`]).
pub fn scan_raw<S: Tables, T: Elem>(table: &str, decode: impl Fn(&Store, &[u8]) -> T + Send + 'static) -> Plan<S, T> {
    Plan {
        op: Box::new(ScanOp {
            table: table.into(),
            decode: Box::new(decode),
        }),
    }
}

/// A sort key the other way: its bytes complemented, so it orders last
/// first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Desc<K>(pub K);

/// In a key, the inner key's bytes complemented, written as bytes: a
/// tuple of keys may hold one, and it orders last first.
impl<K: serde::Serialize> serde::Serialize for Desc<K> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut b = vec![];
        crate::format::put_key(&self.0, &mut b);
        b.iter_mut().for_each(|x| *x = !*x);
        s.serialize_bytes(&b)
    }
}

impl<S: Tables, T: Elem> Plan<S, T> {
    fn with<U>(op: impl Op<S, U> + 'static) -> Plan<S, U> {
        Plan { op: Box::new(op) }
    }

    fn each<U: Elem>(self, f: F<S, T, (Okey, Option<U>)>) -> Plan<S, U> {
        Plan::<S, T>::with(EachOp {
            input: self.op,
            f,
            kept: BTreeMap::new(),
            deps: Deps::new(),
        })
    }

    /// The elements `f` keeps.
    pub fn filter(self, f: impl Fn(&T) -> bool + Send + 'static) -> Plan<S, T> {
        self.each(F::Pure(Box::new(move |t| (vec![], f(t).then(|| t.clone())))))
    }

    /// The elements `f` keeps, reading the store.
    pub fn filter_with(self, f: impl for<'t> Fn(&Db<'t, S>, &T) -> bool + Send + 'static) -> Plan<S, T> {
        self.each(F::With(Box::new(move |db, t| (vec![], f(db, t).then(|| t.clone())))))
    }

    /// Each element as `f` makes it.
    pub fn map<U: Elem>(self, f: impl Fn(&T) -> U + Send + 'static) -> Plan<S, U> {
        self.each(F::Pure(Box::new(move |t| (vec![], Some(f(t))))))
    }

    /// Each element as `f` makes it, reading the store.
    pub fn map_with<U: Elem>(self, f: impl for<'t> Fn(&Db<'t, S>, &T) -> U + Send + 'static) -> Plan<S, U> {
        self.each(F::With(Box::new(move |db, t| (vec![], Some(f(db, t))))))
    }

    /// The elements in the order of `f`'s key, ties in the order they came.
    pub fn sort<K: Key>(self, f: impl Fn(&T) -> K + Send + 'static) -> Plan<S, T> {
        self.each(F::Pure(Box::new(move |t| (f(t).key(), Some(t.clone())))))
    }

    /// The elements in the order of `f`'s key, reading the store.
    pub fn sort_with<K: Key>(self, f: impl for<'t> Fn(&Db<'t, S>, &T) -> K + Send + 'static) -> Plan<S, T> {
        self.each(F::With(Box::new(move |db, t| (f(db, t).key(), Some(t.clone())))))
    }

    /// The first `n`.
    pub fn take(self, n: usize) -> Plan<S, T> {
        Plan::<S, T>::with(TakeOp {
            input: self.op,
            n,
            all: BTreeMap::new(),
        })
    }

    /// The elements in groups by `by`'s key, in its order, each group as
    /// `agg` makes it of its key and its members, reading the store.
    pub fn group<K, A>(
        self,
        by: impl Fn(&T) -> K + Send + 'static,
        agg: impl for<'t, 'm> Fn(&Db<'t, S>, &K, Members<'m, T>) -> A + Send + 'static,
    ) -> Plan<S, A>
    where
        K: Key + Clone + Send + 'static,
        A: Elem,
    {
        Plan::<S, T>::with(GroupOp {
            input: self.op,
            by: Box::new(by),
            agg: Box::new(agg),
            groups: BTreeMap::new(),
            of: BTreeMap::new(),
            deps: Deps::new(),
        })
    }

    /// Each pair of an element and one of `other`'s whose keys are equal.
    pub fn join<U: Elem, K: Key>(
        self,
        other: Plan<S, U>,
        l: impl Fn(&T) -> K + Send + 'static,
        r: impl Fn(&U) -> K + Send + 'static,
    ) -> Plan<S, (T, U)> {
        Plan::<S, T>::with(JoinOp {
            left: self.op,
            right: other.op,
            lk: Box::new(move |t| l(t).key()),
            rk: Box::new(move |u| r(u).key()),
            l: Side::new(),
            r: Side::new(),
        })
    }

    /// The answer, afresh: what a query is asked.
    pub fn run(mut self, store: &Store, ctx: &Ctx) -> Vec<T> {
        self.op.hydrate(&Cx { store, ctx }, false).into_iter().map(|(_, t)| t).collect()
    }
}

// * Views

/// One step from the list a UI holds towards the new answer; positions are
/// in the list as it stands when the patch is applied, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Patch<T> {
    /// The node before the element at `at` (`at == len` appends).
    Insert {
        at: usize,
        node: T,
    },
    Remove {
        at: usize,
    },
    Update {
        at: usize,
        node: T,
    },
}

impl<T> Patch<T> {
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Patch<U> {
        match self {
            Patch::Insert { at, node } => Patch::Insert { at, node: f(node) },
            Patch::Remove { at } => Patch::Remove { at },
            Patch::Update { at, node } => Patch::Update { at, node: f(node) },
        }
    }
}

/// Each patch applied in order: what a client's list is held to.
pub fn splice<T: Clone>(ps: &[Patch<T>], xs: &[T]) -> Vec<T> {
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

/// How a view is made: its plan, built over the store as `ctx` reads it —
/// or why not.
pub type Make<S, T> = Arc<dyn for<'t> Fn(&Ctx, &Db<'t, S>) -> Result<Plan<S, T>, Refusal> + Send + Sync>;

/// A plan kept up to date (§1.10).
pub struct View<S: Tables, T> {
    make: Make<S, T>,
    ctx: Ctx,
    plan: Plan<S, T>,
    /// What building the plan read: a change to it builds it again.
    built: Vec<Dep>,
    list: BTreeMap<Okey, T>,
}

impl<S: Tables, T: Elem> View<S, T> {
    /// The view, hydrated over `store`.
    pub fn new(make: Make<S, T>, ctx: Ctx, store: &Store) -> Result<View<S, T>, Refusal> {
        let (plan, built) = recorded::<S, _>(store, |db| make(&ctx, db));
        let mut v = View {
            make,
            ctx,
            plan: plan?,
            built,
            list: BTreeMap::new(),
        };
        v.list = v.plan.op.hydrate(&Cx { store, ctx: &v.ctx }, true).into_iter().collect();
        Ok(v)
    }

    /// The answer as it stands.
    pub fn rows(&self) -> Vec<T> {
        self.list.values().cloned().collect()
    }

    /// A settle's changes, the store after them: the patches that take the
    /// list to the new answer. A view whose plan cannot be built again any
    /// more refuses, and is stale.
    pub fn push(&mut self, changes: &[Change], store: &Store) -> Result<Vec<Patch<T>>, Refusal> {
        if changes.is_empty() {
            return Ok(vec![]);
        }
        let p = Pushed::of(store, changes);
        let d = if p.hits(&self.built) {
            let (plan, built) = recorded::<S, _>(store, |db| (self.make)(&self.ctx, db));
            (self.plan, self.built) = (plan?, built);
            let new: BTreeMap<Okey, T> = self.plan.op.hydrate(&Cx { store, ctx: &self.ctx }, true).into_iter().collect();
            let mut d = Delta::new();
            for (k, t) in &self.list {
                d.insert(k.clone(), (Some(t.clone()), new.get(k).cloned()));
            }
            for (k, t) in &new {
                d.entry(k.clone()).or_insert((None, Some(t.clone())));
            }
            prune(d)
        } else {
            self.plan.op.push(&Cx { store, ctx: &self.ctx }, &p)
        };
        Ok(self.patch(d))
    }

    /// The delta applied to the list, as positional patches in order.
    fn patch(&mut self, d: Delta<T>) -> Vec<Patch<T>> {
        let mut out = vec![];
        for (k, (_, new)) in d {
            let at = self.list.range::<Okey, _>(..&k).count();
            match (self.list.contains_key(&k), new) {
                (true, None) => {
                    self.list.remove(&k);
                    out.push(Patch::Remove { at });
                }
                (true, Some(t)) => {
                    self.list.insert(k, t.clone());
                    out.push(Patch::Update { at, node: t });
                }
                (false, Some(t)) => {
                    self.list.insert(k, t.clone());
                    out.push(Patch::Insert { at, node: t });
                }
                (false, None) => {}
            }
        }
        out
    }
}

/// A view as a peer keeps it whatever it answers: its rows and patches as
/// their encodings.
pub trait Maintained: Send {
    fn push(&mut self, changes: &[Change], store: &Store) -> Result<Vec<Patch<Vec<u8>>>, Refusal>;
    fn rows(&self) -> Vec<Vec<u8>>;
}

impl<S: Tables, T: Value> Maintained for View<S, T> {
    fn push(&mut self, changes: &[Change], store: &Store) -> Result<Vec<Patch<Vec<u8>>>, Refusal> {
        Ok(View::push(self, changes, store)?.into_iter().map(|p| p.map(|t| t.encode())).collect())
    }
    fn rows(&self) -> Vec<Vec<u8>> {
        self.list.values().map(Value::encode).collect()
    }
}

/// A plan's answer afresh over `store` as `ctx` reads it: what a query
/// asked once is.
pub fn answer<S: Tables, T: Elem>(make: &Make<S, T>, ctx: &Ctx, store: &Store) -> Result<Vec<T>, Refusal> {
    let plan = read::<S, _>(store, |db| make(ctx, db))?;
    Ok(plan.run(store, ctx))
}

/// The rows of `R`'s table a read of `store` gives, in key order.
pub fn rows<R: Row>(store: &dyn Read) -> Vec<R> {
    store
        .scan(R::TABLE, &crate::store::Scan::all())
        .iter()
        .map(|b| R::decode(b).unwrap_or_else(|e| panic!("a row of {} reads: {e}", R::TABLE)))
        .collect()
}
