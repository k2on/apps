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

/// An element's place in a list, as bytes.
pub type Okey = Vec<u8>;

/// Per element that may have moved: what it is now (none: gone). What it
/// was is the reader's own to know; a move to the same value is no move.
type Delta<T> = BTreeMap<Okey, Option<T>>;

/// A row before and after, as its bytes.
type Moved = (Option<Vec<u8>>, Option<Vec<u8>>);
/// A closure over an element, reading the store (or not: then it read
/// nothing, and nothing runs it again).
type WithDb<S, A, B> = Box<dyn for<'t> Fn(&Db<'t, S>, &A) -> B + Send>;

/// What a list's element may be.
pub trait Elem: Clone + PartialEq + Send + 'static {}
impl<T: Clone + PartialEq + Send + 'static> Elem for T {}

/// One settle's changes, as every operator reads them: per table, per key,
/// the row before and after; and every key and index entry (`None`: the
/// key) a changed row had or has, which is what a recorded read is hit by.
/// One `Pushed` serves every view a settle moves.
pub struct Pushed<'a> {
    store: &'a Store,
    net: BTreeMap<&'a str, BTreeMap<Vec<u8>, Moved>>,
    touched: BTreeMap<(&'a str, Option<usize>), BTreeSet<Vec<u8>>>,
}

impl<'a> Pushed<'a> {
    pub fn of(store: &'a Store, changes: &'a [Change]) -> Pushed<'a> {
        let mut p = Pushed {
            store,
            net: BTreeMap::new(),
            touched: BTreeMap::new(),
        };
        for c in changes {
            let (t, (old, new)) = match c {
                Change::Add { table, row } => (table, (None, Some(row))),
                Change::Remove { table, row } => (table, (Some(row), None)),
                Change::Edit { table, old, new } => (table, (Some(old), Some(new))),
            };
            for r in [old, new].into_iter().flatten() {
                let Ok((key, entries)) = store.paths_of(t, r) else { continue };
                p.touched.entry((t, None)).or_default().insert(key);
                for (i, e) in entries.into_iter().enumerate() {
                    p.touched.entry((t, Some(i))).or_default().insert(e);
                }
            }
            let Ok(key) = store.key_of(t, new.or(old).unwrap()) else { continue };
            p.net.entry(t).or_default().entry(key).or_insert((old.cloned(), None)).1 = new.cloned();
        }
        p.net.values_mut().for_each(|t| t.retain(|_, (a, b)| a != b));
        p
    }

    pub fn is_empty(&self) -> bool {
        self.touched.is_empty()
    }

    /// Whether anything a read in `deps` covered moved.
    fn hits(&self, deps: &[Dep]) -> bool {
        deps.iter().any(|(t, i, p)| {
            self.touched
                .get(&(*t, *i))
                .is_some_and(|es| es.range::<Vec<u8>, _>(p..).next().is_some_and(|e| e.starts_with(p)))
        })
    }
}

/// Per prefix read, the elements whose reads covered it.
type ByPrefix = BTreeMap<Vec<u8>, BTreeSet<Okey>>;
/// A row of a table, read.
type Decode<T> = Box<dyn Fn(&Store, &[u8]) -> T + Send>;
/// An element's join key.
type KeyFn<T> = Box<dyn Fn(&T) -> Vec<u8> + Send>;

/// What each element's run of a closure read, and which elements a moved
/// row reaches through it: per table and index, per prefix read, the
/// elements.
#[derive(Default)]
struct Deps {
    by: BTreeMap<(&'static str, Option<usize>), ByPrefix>,
    of: BTreeMap<Okey, Vec<Dep>>,
}

impl Deps {
    fn set(&mut self, k: &Okey, deps: Vec<Dep>) {
        for (t, i, p) in self.of.remove(k).unwrap_or_default() {
            let Some(m) = self.by.get_mut(&(t, i)) else { continue };
            if m.get_mut(&p).is_some_and(|s| s.remove(k) && s.is_empty()) {
                m.remove(&p);
            }
        }
        for (t, i, p) in &deps {
            self.by.entry((*t, *i)).or_default().entry(p.clone()).or_default().insert(k.clone());
        }
        if !deps.is_empty() {
            self.of.insert(k.clone(), deps);
        }
    }

    /// The elements a settle's moved rows reach.
    fn hit(&self, p: &Pushed) -> BTreeSet<Okey> {
        let mut out = BTreeSet::new();
        for ((t, i), m) in &self.by {
            for e in p.touched.get(&(*t, *i)).into_iter().flatten() {
                (0..=e.len()).filter_map(|n| m.get(&e[..n])).for_each(|ks| out.extend(ks.iter().cloned()));
            }
        }
        out
    }
}

/// One operator of a plan, over elements `T`.
trait Op<S, T>: Send {
    /// Every element, in order; kept for the pushes after when `keep`.
    fn hydrate(&mut self, store: &Store, keep: bool) -> Vec<(Okey, T)>;
    /// What a settle did to its output.
    fn push(&mut self, p: &Pushed) -> Delta<T>;
}

// * The operators

/// Every row of one table whose key begins with `prefix`, by its key, each
/// as `decode` reads it.
struct ScanOp<T> {
    table: String,
    prefix: Vec<u8>,
    decode: Decode<T>,
}

impl<S: Tables, T: Elem> Op<S, T> for ScanOp<T> {
    fn hydrate(&mut self, store: &Store, _: bool) -> Vec<(Okey, T)> {
        store
            .keyed_from(&self.table, &self.prefix)
            .take_while(|(k, _)| k.starts_with(&self.prefix))
            .map(|(k, b)| (k.to_vec(), (self.decode)(store, b)))
            .collect()
    }

    fn push(&mut self, p: &Pushed) -> Delta<T> {
        let dec = |b: &Option<Vec<u8>>| b.as_ref().map(|b| (self.decode)(p.store, b));
        let moved = p.net.get(self.table.as_str()).into_iter().flat_map(|t| t.range(self.prefix.clone()..));
        moved
            .take_while(|(k, _)| k.starts_with(&self.prefix))
            .map(|(k, (_, b))| (k.clone(), dec(b)))
            .collect()
    }
}

/// One element at a time: `f` makes of each input element what goes
/// before its place and what it puts out, if anything — a filter keeps or
/// drops it, a map makes another of it, a sort puts its sort key first.
struct EachOp<S: Tables, T, U> {
    input: Box<dyn Op<S, T>>,
    f: WithDb<S, T, (Okey, Option<U>)>,
    /// Per input element: itself (when `f` read, to run it again), and its
    /// output's place and value.
    kept: BTreeMap<Okey, (Option<T>, Okey, Option<U>)>,
    deps: Deps,
}

impl<S: Tables, T: Elem, U: Elem> EachOp<S, T, U> {
    /// The element at `k` through `f`: its output's place and value, kept
    /// when `keep`.
    fn run(&mut self, store: &Store, k: &Okey, t: T, keep: bool) -> (Okey, Option<U>) {
        let ((mut at, u), deps) = recorded::<S, _>(store, |db| (self.f)(db, &t));
        at.extend_from_slice(k);
        if keep {
            let reads = !deps.is_empty();
            self.deps.set(k, deps);
            self.kept.insert(k.clone(), (reads.then_some(t), at.clone(), u.clone()));
        }
        (at, u)
    }
}

impl<S: Tables, T: Elem, U: Elem> Op<S, U> for EachOp<S, T, U> {
    fn hydrate(&mut self, store: &Store, keep: bool) -> Vec<(Okey, U)> {
        let input = self.input.hydrate(store, keep);
        let mut out: Vec<(Okey, U)> = input
            .into_iter()
            .filter_map(|(k, t)| Some(self.run(store, &k, t, keep)).and_then(|(at, u)| Some((at, u?))))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    fn push(&mut self, p: &Pushed) -> Delta<U> {
        let mut moved: BTreeMap<Okey, Option<T>> = self
            .deps
            .hit(p)
            .into_iter()
            .map(|k| {
                let t = self.kept.get(&k).and_then(|e| e.0.clone());
                (k, t)
            })
            .collect();
        moved.extend(self.input.push(p));
        let mut out = Delta::new();
        for (k, new) in moved {
            let was = self.kept.remove(&k).and_then(|(_, at, u)| Some((at, u?)));
            let now = match new {
                Some(t) => {
                    let (at, u) = self.run(p.store, &k, t, true);
                    u.map(|u| (at, u))
                }
                None => {
                    self.deps.set(&k, vec![]);
                    None
                }
            };
            if was != now {
                out.extend(was.map(|(at, _)| (at, None)));
                out.extend(now.map(|(at, u)| (at, Some(u))));
            }
        }
        out
    }
}

/// The first `n` of its input, which it keeps whole.
struct TakeOp<S, T> {
    input: Box<dyn Op<S, T>>,
    n: usize,
    all: BTreeMap<Okey, T>,
}

impl<S: Tables, T: Elem> Op<S, T> for TakeOp<S, T> {
    fn hydrate(&mut self, store: &Store, keep: bool) -> Vec<(Okey, T)> {
        let all = self.input.hydrate(store, keep);
        let out = all.iter().take(self.n).cloned().collect();
        if keep {
            self.all = all.into_iter().collect();
        }
        out
    }

    fn push(&mut self, p: &Pushed) -> Delta<T> {
        let d = self.input.push(p);
        let window = |all: &BTreeMap<Okey, T>| all.iter().take(self.n).map(|(k, t)| (k.clone(), t.clone())).collect::<BTreeMap<_, _>>();
        let before = window(&self.all);
        for (k, new) in d {
            match new {
                Some(t) => self.all.insert(k, t),
                None => self.all.remove(&k),
            };
        }
        let after = window(&self.all);
        let keys: BTreeSet<&Okey> = before.keys().chain(after.keys()).collect();
        keys.into_iter()
            .filter(|k| before.get(*k) != after.get(*k))
            .map(|k| (k.clone(), after.get(k).cloned()))
            .collect()
    }
}

/// A group's members, in their input's order: what an aggregate reads.
pub type Members<'a, T> = btree_map::Values<'a, Okey, T>;

type Agg<S, K, T, A> = Box<dyn for<'t, 'm> Fn(&Db<'t, S>, &K, Members<'m, T>) -> A + Send>;

struct GroupOp<S: Tables, T, K, A> {
    input: Box<dyn Op<S, T>>,
    by: Box<dyn Fn(&T) -> K + Send>,
    agg: Agg<S, K, T, A>,
    /// Per group: its key and its members; and its aggregate.
    groups: BTreeMap<Okey, (K, BTreeMap<Okey, T>)>,
    aggs: BTreeMap<Okey, A>,
    /// Each member's group.
    of: BTreeMap<Okey, Okey>,
    deps: Deps,
}

impl<S: Tables, T: Elem, K: Key + Clone + Send + 'static, A: Elem> GroupOp<S, T, K, A> {
    fn join(&mut self, k: Okey, t: T) -> Okey {
        let key = (self.by)(&t);
        let g = key.key();
        self.groups
            .entry(g.clone())
            .or_insert_with(|| (key, BTreeMap::new()))
            .1
            .insert(k.clone(), t);
        self.of.insert(k, g.clone());
        g
    }

    /// Group `g`'s aggregate again, or none when it has no members left.
    fn aggregate(&mut self, store: &Store, g: &Okey) -> Option<A> {
        let Some((key, ms)) = self.groups.get(g).filter(|(_, ms)| !ms.is_empty()) else {
            self.groups.remove(g);
            self.deps.set(g, vec![]);
            return self.aggs.remove(g).and(None);
        };
        let (a, deps) = recorded::<S, A>(store, |db| (self.agg)(db, key, ms.values()));
        self.deps.set(g, deps);
        self.aggs.insert(g.clone(), a.clone());
        Some(a)
    }
}

impl<S: Tables, T: Elem, K: Key + Clone + Send + 'static, A: Elem> Op<S, A> for GroupOp<S, T, K, A> {
    fn hydrate(&mut self, store: &Store, keep: bool) -> Vec<(Okey, A)> {
        for (k, t) in self.input.hydrate(store, keep) {
            self.join(k, t);
        }
        let gs: Vec<Okey> = self.groups.keys().cloned().collect();
        gs.into_iter().filter_map(|g| Some((g.clone(), self.aggregate(store, &g)?))).collect()
    }

    fn push(&mut self, p: &Pushed) -> Delta<A> {
        let mut touched = self.deps.hit(p);
        for (k, new) in self.input.push(p) {
            if let Some(g) = self.of.remove(&k) {
                self.groups.get_mut(&g).expect("a member's group").1.remove(&k);
                touched.insert(g);
            }
            if let Some(t) = new {
                touched.insert(self.join(k, t));
            }
        }
        let mut out = Delta::new();
        for g in touched {
            let was = self.aggs.get(&g).cloned();
            let now = self.aggregate(p.store, &g);
            if was != now {
                out.insert(g, now);
            }
        }
        out
    }
}

/// One side of a join: each element's join key, and by join key, the
/// elements.
struct Side<T> {
    keys: BTreeMap<Okey, Vec<u8>>,
    by: BTreeMap<Vec<u8>, BTreeMap<Okey, T>>,
    key: KeyFn<T>,
}

impl<T: Clone> Side<T> {
    fn add(&mut self, k: Okey, t: T) -> Vec<u8> {
        let key = (self.key)(&t);
        self.by.entry(key.clone()).or_default().insert(k.clone(), t);
        self.keys.insert(k, key.clone());
        key
    }

    fn remove(&mut self, k: &Okey) -> Option<(Vec<u8>, T)> {
        let key = self.keys.remove(k)?;
        let t = self.by.get_mut(&key)?.remove(k)?;
        Some((key, t))
    }

    fn partners(&self, key: &[u8]) -> impl Iterator<Item = (&Okey, &T)> {
        self.by.get(key).into_iter().flatten()
    }

    /// This side's moves through the join: each element moved, paired
    /// (by `pair`, in the join's order) with the other side's partners.
    fn push<U: Clone, P>(&mut self, d: Delta<T>, other: &Side<U>, out: &mut Delta<P>, pair: impl Fn(&Okey, &T, &Okey, &U) -> (Okey, P)) {
        for (a, new) in d {
            if let Some((key, t)) = self.remove(&a) {
                other.partners(&key).for_each(|(b, u)| {
                    let (k, _) = pair(&a, &t, b, u);
                    out.insert(k, None);
                });
            }
            if let Some(t) = new {
                let key = self.add(a.clone(), t.clone());
                other.partners(&key).for_each(|(b, u)| {
                    let (k, p) = pair(&a, &t, b, u);
                    out.insert(k, Some(p));
                });
            }
        }
    }
}

struct JoinOp<S, T, U> {
    left: Box<dyn Op<S, T>>,
    right: Box<dyn Op<S, U>>,
    l: Side<T>,
    r: Side<U>,
}

impl<S: Tables, T: Elem, U: Elem> Op<S, (T, U)> for JoinOp<S, T, U> {
    fn hydrate(&mut self, store: &Store, keep: bool) -> Vec<(Okey, (T, U))> {
        for (k, u) in self.right.hydrate(store, keep) {
            self.r.add(k, u);
        }
        let mut out = Delta::new();
        let left = self.left.hydrate(store, keep).into_iter().map(|(k, t)| (k, Some(t)));
        self.l.push(left.collect(), &self.r, &mut out, |a, t, b, u| {
            ([&a[..], b].concat(), (t.clone(), u.clone()))
        });
        out.into_iter().filter_map(|(k, p)| Some((k, p?))).collect()
    }

    fn push(&mut self, p: &Pushed) -> Delta<(T, U)> {
        let (dl, dr) = (self.left.push(p), self.right.push(p));
        let mut out = Delta::new();
        self.l
            .push(dl, &self.r, &mut out, |a, t, b, u| ([&a[..], b].concat(), (t.clone(), u.clone())));
        self.r
            .push(dr, &self.l, &mut out, |b, u, a, t| ([&a[..], b].concat(), (t.clone(), u.clone())));
        out
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

/// The rows of `R`'s table whose leading key columns are `q`, in key order:
/// what [`scan`] filtered by them gives, read from those keys alone, and
/// moved by changes to them alone — an episode's lines of a table of every
/// episode's.
pub fn scan_from<S: Tables, R: Row, Q, M>(q: Q) -> Plan<S, R>
where
    R::Key: crate::row::Lead<Q, M>,
{
    let mut prefix = vec![];
    <R::Key as crate::row::Lead<Q, M>>::put(&q, &mut prefix);
    Plan {
        op: Box::new(ScanOp {
            table: R::TABLE.into(),
            prefix,
            decode: Box::new(|_, b| R::decode(b).unwrap_or_else(|e| panic!("a row of {} reads: {e}", R::TABLE))),
        }),
    }
}

/// Every row of the table named `table`, in key order, each as `decode`
/// reads it: a plan over rows without a Rust type ([`crate::dynamic`]).
pub fn scan_raw<S: Tables, T: Elem>(table: &str, decode: impl Fn(&Store, &[u8]) -> T + Send + 'static) -> Plan<S, T> {
    Plan {
        op: Box::new(ScanOp {
            table: table.into(),
            prefix: vec![],
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

    fn each<U: Elem>(self, f: impl for<'t> Fn(&Db<'t, S>, &T) -> (Okey, Option<U>) + Send + 'static) -> Plan<S, U> {
        Plan::<S, T>::with(EachOp {
            input: self.op,
            f: Box::new(f),
            kept: BTreeMap::new(),
            deps: Deps::default(),
        })
    }

    /// The elements `f` keeps.
    pub fn filter(self, f: impl Fn(&T) -> bool + Send + 'static) -> Plan<S, T> {
        self.filter_with(move |_, t| f(t))
    }

    /// The elements `f` keeps, reading the store.
    pub fn filter_with(self, f: impl for<'t> Fn(&Db<'t, S>, &T) -> bool + Send + 'static) -> Plan<S, T> {
        self.each(move |db, t| (vec![], f(db, t).then(|| t.clone())))
    }

    /// Each element as `f` makes it.
    pub fn map<U: Elem>(self, f: impl Fn(&T) -> U + Send + 'static) -> Plan<S, U> {
        self.map_with(move |_, t| f(t))
    }

    /// Each element as `f` makes it, reading the store.
    pub fn map_with<U: Elem>(self, f: impl for<'t> Fn(&Db<'t, S>, &T) -> U + Send + 'static) -> Plan<S, U> {
        self.each(move |db, t| (vec![], Some(f(db, t))))
    }

    /// The elements in the order of `f`'s key, ties in the order they came.
    pub fn sort<K: Key>(self, f: impl Fn(&T) -> K + Send + 'static) -> Plan<S, T> {
        self.sort_with(move |_, t| f(t))
    }

    /// The elements in the order of `f`'s key, reading the store.
    pub fn sort_with<K: Key>(self, f: impl for<'t> Fn(&Db<'t, S>, &T) -> K + Send + 'static) -> Plan<S, T> {
        self.each(move |db, t| (f(db, t).key(), Some(t.clone())))
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
            aggs: BTreeMap::new(),
            of: BTreeMap::new(),
            deps: Deps::default(),
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
            l: Side {
                keys: BTreeMap::new(),
                by: BTreeMap::new(),
                key: Box::new(move |t| l(t).key()),
            },
            r: Side {
                keys: BTreeMap::new(),
                by: BTreeMap::new(),
                key: Box::new(move |u| r(u).key()),
            },
        })
    }

    /// The answer, afresh: what a query is asked.
    pub fn run(mut self, store: &Store, _: &Ctx) -> Vec<T> {
        self.op.hydrate(store, false).into_iter().map(|(_, t)| t).collect()
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
pub type Make<S, T> = Arc<dyn for<'t> Fn(&Ctx, &Db<'t, S>) -> Result<Plan<S, T>, String> + Send + Sync>;

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
    pub fn new(make: Make<S, T>, ctx: Ctx, store: &Store) -> Result<View<S, T>, String> {
        let (plan, built) = recorded::<S, _>(store, |db| make(&ctx, db));
        let mut v = View {
            make,
            ctx,
            plan: plan?,
            built,
            list: BTreeMap::new(),
        };
        v.list = v.plan.op.hydrate(store, true).into_iter().collect();
        Ok(v)
    }

    /// The answer as it stands.
    pub fn rows(&self) -> Vec<T> {
        self.list.values().cloned().collect()
    }

    /// A settle's changes, the store after them: the patches that take the
    /// list to the new answer. A view whose plan cannot be built again any
    /// more refuses, and is stale.
    pub fn push(&mut self, changes: &[Change], store: &Store) -> Result<Vec<Patch<T>>, String> {
        self.pushed(&Pushed::of(store, changes))
    }

    /// [`View::push`], of a settle read once for every view it moves.
    pub fn pushed(&mut self, p: &Pushed) -> Result<Vec<Patch<T>>, String> {
        if p.is_empty() {
            return Ok(vec![]);
        }
        let store = p.store;
        let d = if p.hits(&self.built) {
            let (plan, built) = recorded::<S, _>(store, |db| (self.make)(&self.ctx, db));
            (self.plan, self.built) = (plan?, built);
            let mut d: Delta<T> = self.list.keys().map(|k| (k.clone(), None)).collect();
            d.extend(self.plan.op.hydrate(store, true).into_iter().map(|(k, t)| (k, Some(t))));
            d
        } else {
            self.plan.op.push(p)
        };
        Ok(self.patch(d))
    }

    /// The delta applied to the list, as positional patches in order.
    fn patch(&mut self, d: Delta<T>) -> Vec<Patch<T>> {
        let mut out = vec![];
        for (k, new) in d {
            let same = self.list.get(&k).map(|was| Some(was) == new.as_ref());
            let at = || self.list.range::<Okey, _>(..&k).count();
            let patch = match (same, new) {
                (Some(_), None) => Patch::Remove { at: at() },
                (Some(false), Some(t)) => Patch::Update { at: at(), node: t },
                (None, Some(t)) => Patch::Insert { at: at(), node: t },
                _ => continue,
            };
            match &patch {
                Patch::Remove { .. } => self.list.remove(&k),
                Patch::Update { node, .. } | Patch::Insert { node, .. } => self.list.insert(k, node.clone()),
            };
            out.push(patch);
        }
        out
    }
}

/// A view as a peer keeps it whatever it answers: its rows and patches as
/// their encodings.
pub trait Maintained: Send {
    /// A settle read once for every view it moves ([`Pushed`]).
    fn pushed(&mut self, p: &Pushed) -> Result<Vec<Patch<Vec<u8>>>, String>;
    fn push(&mut self, changes: &[Change], store: &Store) -> Result<Vec<Patch<Vec<u8>>>, String> {
        self.pushed(&Pushed::of(store, changes))
    }
    fn rows(&self) -> Vec<Vec<u8>>;
}

impl<S: Tables, T: Value> Maintained for View<S, T> {
    fn pushed(&mut self, p: &Pushed) -> Result<Vec<Patch<Vec<u8>>>, String> {
        Ok(View::pushed(self, p)?.into_iter().map(|p| p.map(|t| t.encode())).collect())
    }
    fn rows(&self) -> Vec<Vec<u8>> {
        self.list.values().map(Value::encode).collect()
    }
}

/// A plan's answer afresh over `store` as `ctx` reads it: what a query
/// asked once is.
pub fn answer<S: Tables, T: Elem>(make: &Make<S, T>, ctx: &Ctx, store: &Store) -> Result<Vec<T>, String> {
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
