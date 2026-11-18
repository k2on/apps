//! Queries as plans of incremental operators, ported from `rust/ark`'s
//! `plan.rs` and made pull-based, as Zero's are: an operator holds as
//! little as it can and asks the one below it for rows when it needs them.
//!
//! ```text
//! scan(table, lo, hi) . filter(f) . map(f) . sort(by) . take(n) . group(by, of)
//! ```
//!
//! A row of a result is its **place**, bytes whose order is the row's
//! order in the result, and its value. A scan's places are its table's
//! keys; `filter`, `map` and `take` keep their input's; a `sort` puts its
//! sort key before them, and a `group` places a group by its key. An index
//! is a table like any other, its key the indexed bytes and then the row's
//! key, its value the row, kept by the handlers that write the row: a scan
//! of its range reads the rows in the index's order.
//!
//! **Pull.** Every operator answers [`Op::fetch`]: its rows in order, from
//! the start or from a place (at it, or after it), ascending or not, for as
//! long as the caller wants them, so a fetch is resumed where the last one
//! stopped. A scan answers it with the store's ordered range scan.
//!
//! | operator | holds | on a change |
//! |---|---|---|
//! | `scan` | nothing | its table's changes in its range |
//! | `filter`, `map` | nothing | the closure again on the old and new row |
//! | `take` | its size and its last place | fetches the neighbours of its window that come in or go out |
//! | `sort` | its input, sorted | the expensive case: a sort no index provides |
//! | `group` | each group's running count, sum, min and max | count and sum in constant time; min or max fetched again when the extreme leaves |
//!
//! **Push.** One settle's [`Changes`] (what a node's overlay or the log's
//! facts did to what it reads, its store already after them) go up through
//! the operators, each answering the [`Diff`]s of its own rows; a [`Live`]
//! answer turns them into the positional [`Patch`]es that splice a list a
//! view holds into the new answer. The contract, held by the tests: after
//! every push, every operator's rows are a fresh run's, and the patches
//! splice the old answer into the new one.
//!
//! Closures read their row only; reads of other rows (recorded, as the old
//! engine's are) and `join` are for later.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::ops::Bound;

use crate::codec::{from_slice, to_vec, Decode, Encode, Error, Reader};
use crate::store::{Range, Read, TableId};

/// A row of an answer: its place and its value.
pub type Row = (Vec<u8>, Vec<u8>);

/// A row's change: at `key`, what it was and what it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diff {
    pub key: Vec<u8>,
    pub old: Option<Vec<u8>>,
    pub new: Option<Vec<u8>>,
}

/// One settle's changes to what a node reads: each row changed once, in
/// table and key order; or, when `reset`, anything may have (a fill), and
/// every operator builds its state again.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub reset: bool,
    pub rows: Vec<(TableId, Diff)>,
}

/// One operator of a plan.
pub trait Op {
    /// Calls `f` with each row in order, ascending or, when `reverse`,
    /// descending, from `from` (at a place, after it, or from the start),
    /// until `f` returns false or the rows end.
    fn fetch(&self, db: &dyn Read, from: Bound<&[u8]>, reverse: bool, f: &mut dyn FnMut(&[u8], &[u8]) -> bool);

    /// Takes one settle's changes, `db` already after them, and answers
    /// what they did to this operator's rows, in place order.
    fn push(&mut self, db: &dyn Read, changes: &Changes) -> Vec<Diff>;

    /// Builds its state over `db`, after its input's.
    fn hydrate(&mut self, db: &dyn Read);

    /// The bytes of state it and its inputs hold: what the tests hold to a
    /// bound.
    #[cfg(any(test, ark_selftest))]
    fn held(&self) -> usize;
}

/// A row-wise closure: its place and value in, a verdict or bytes out.
pub type Each<T> = Box<dyn Fn(&[u8], &[u8]) -> T>;

/// Whether `k` is in the range from `lo` to `hi`.
fn within(lo: Bound<&[u8]>, hi: Bound<&[u8]>, k: &[u8]) -> bool {
    let above = match lo {
        Bound::Included(l) => k >= l,
        Bound::Excluded(l) => k > l,
        Bound::Unbounded => true,
    };
    above
        && match hi {
            Bound::Included(h) => k <= h,
            Bound::Excluded(h) => k < h,
            Bound::Unbounded => true,
        }
}

/// The later of two lower bounds (`up`), or the earlier of two upper ones.
fn tighter<'a>(a: Bound<&'a [u8]>, b: Bound<&'a [u8]>, up: bool) -> Bound<&'a [u8]> {
    let (ka, kb) = match (a, b) {
        (Bound::Unbounded, b) => return b,
        (a, Bound::Unbounded) => return a,
        (Bound::Included(x) | Bound::Excluded(x), Bound::Included(y) | Bound::Excluded(y)) => (x, y),
    };
    if ka == kb {
        return if matches!(a, Bound::Excluded(_)) { a } else { b };
    }
    if (ka > kb) == up {
        a
    } else {
        b
    }
}

/// The range a fetch from `from` reads within `lo` to `hi`.
fn span<'a>(lo: Bound<&'a [u8]>, hi: Bound<&'a [u8]>, from: Bound<&'a [u8]>, reverse: bool) -> Range<'a> {
    if reverse {
        Range {
            lo,
            hi: tighter(hi, from, false),
            reverse,
        }
    } else {
        Range {
            lo: tighter(lo, from, true),
            hi,
            reverse,
        }
    }
}

/// Calls `f` with each of `rows` a fetch from `from` reads, in its order.
fn fetch_map<V>(rows: &BTreeMap<Vec<u8>, V>, from: Bound<&[u8]>, reverse: bool, f: &mut dyn FnMut(&[u8], &V) -> bool) {
    // One end is open, so the range is never one a `BTreeMap` refuses.
    let r = span(Bound::Unbounded, Bound::Unbounded, from, reverse);
    let mut it = rows.range::<[u8], _>((r.lo, r.hi));
    let mut g = |(k, v): (&Vec<u8>, &V)| f(k, v);
    if reverse {
        it.rev().all(&mut g);
    } else {
        it.all(&mut g);
    }
}

/// Two lists of diffs in place order as one, a place in both taking its
/// old from `a` and its new from `b`, with any whose old and new are the
/// same left out.
fn merge(a: Vec<Diff>, b: Vec<Diff>) -> Vec<Diff> {
    let mut out = Vec::with_capacity(a.len() + b.len());
    let mut b = b.into_iter().peekable();
    for x in a {
        while let Some(y) = b.next_if(|y| y.key < x.key) {
            out.push(y);
        }
        match b.next_if(|y| y.key == x.key) {
            Some(y) => out.push(Diff { new: y.new, ..x }),
            None => out.push(x),
        }
    }
    out.extend(b);
    out.retain(|d| d.old != d.new);
    out
}

/// A table's rows from `lo` to `hi`, by their keys.
struct Scan {
    table: TableId,
    lo: Bound<Vec<u8>>,
    hi: Bound<Vec<u8>>,
}

impl Op for Scan {
    fn fetch(&self, db: &dyn Read, from: Bound<&[u8]>, reverse: bool, f: &mut dyn FnMut(&[u8], &[u8]) -> bool) {
        db.scan(
            self.table,
            span(self.lo.as_ref().map(Vec::as_slice), self.hi.as_ref().map(Vec::as_slice), from, reverse),
            f,
        );
    }

    fn push(&mut self, _: &dyn Read, changes: &Changes) -> Vec<Diff> {
        let (lo, hi) = (self.lo.as_ref().map(Vec::as_slice), self.hi.as_ref().map(Vec::as_slice));
        let mine = changes.rows.iter().filter(|(t, d)| *t == self.table && within(lo, hi, &d.key));
        mine.map(|(_, d)| d.clone()).collect()
    }

    fn hydrate(&mut self, _: &dyn Read) {}

    #[cfg(any(test, ark_selftest))]
    fn held(&self) -> usize {
        0
    }
}

/// Its input's rows `keep` keeps.
struct Filter {
    input: Box<dyn Op>,
    keep: Each<bool>,
}

impl Op for Filter {
    fn fetch(&self, db: &dyn Read, from: Bound<&[u8]>, reverse: bool, f: &mut dyn FnMut(&[u8], &[u8]) -> bool) {
        self.input.fetch(db, from, reverse, &mut |k, v| !(self.keep)(k, v) || f(k, v));
    }

    fn push(&mut self, db: &dyn Read, changes: &Changes) -> Vec<Diff> {
        let keep = &self.keep;
        let mut out = self.input.push(db, changes);
        for d in &mut out {
            d.old = d.old.take().filter(|v| keep(&d.key, v));
            d.new = d.new.take().filter(|v| keep(&d.key, v));
        }
        out.retain(|d| d.old != d.new);
        out
    }

    fn hydrate(&mut self, db: &dyn Read) {
        self.input.hydrate(db);
    }

    #[cfg(any(test, ark_selftest))]
    fn held(&self) -> usize {
        self.input.held()
    }
}

/// Its input's rows, each value made another by `make`.
struct Map {
    input: Box<dyn Op>,
    make: Each<Vec<u8>>,
}

impl Op for Map {
    fn fetch(&self, db: &dyn Read, from: Bound<&[u8]>, reverse: bool, f: &mut dyn FnMut(&[u8], &[u8]) -> bool) {
        self.input.fetch(db, from, reverse, &mut |k, v| f(k, &(self.make)(k, v)));
    }

    fn push(&mut self, db: &dyn Read, changes: &Changes) -> Vec<Diff> {
        let make = &self.make;
        let mut out = self.input.push(db, changes);
        for d in &mut out {
            d.old = d.old.take().map(|v| make(&d.key, &v));
            d.new = d.new.take().map(|v| make(&d.key, &v));
        }
        out.retain(|d| d.old != d.new);
        out
    }

    fn hydrate(&mut self, db: &dyn Read) {
        self.input.hydrate(db);
    }

    #[cfg(any(test, ark_selftest))]
    fn held(&self) -> usize {
        self.input.held()
    }
}

/// Its input's rows placed by `by` first, then by their place: a sort no
/// index provides, which holds its input, sorted, to answer a fetch in
/// order. The expensive case; a sort an index provides is a scan of it.
struct Sort {
    input: Box<dyn Op>,
    by: Each<Vec<u8>>,
    rows: BTreeMap<Vec<u8>, Vec<u8>>,
}

impl Sort {
    fn place(&self, k: &[u8], v: &[u8]) -> Vec<u8> {
        let mut at = (self.by)(k, v);
        at.extend_from_slice(k);
        at
    }
}

impl Op for Sort {
    fn fetch(&self, _: &dyn Read, from: Bound<&[u8]>, reverse: bool, f: &mut dyn FnMut(&[u8], &[u8]) -> bool) {
        fetch_map(&self.rows, from, reverse, &mut |k, v| f(k, v));
    }

    fn push(&mut self, db: &dyn Read, changes: &Changes) -> Vec<Diff> {
        if changes.reset {
            self.hydrate(db);
            return Vec::new();
        }
        let mut out: BTreeMap<Vec<u8>, Diff> = BTreeMap::new();
        for d in self.input.push(db, changes) {
            if let Some(old) = d.old {
                let at = self.place(&d.key, &old);
                self.rows.remove(&at);
                out.insert(
                    at.clone(),
                    Diff {
                        key: at,
                        old: Some(old),
                        new: None,
                    },
                );
            }
            if let Some(new) = d.new {
                let at = self.place(&d.key, &new);
                self.rows.insert(at.clone(), new.clone());
                out.entry(at.clone())
                    .or_insert(Diff {
                        key: at,
                        old: None,
                        new: None,
                    })
                    .new = Some(new);
            }
        }
        merge(out.into_values().collect(), Vec::new())
    }

    fn hydrate(&mut self, db: &dyn Read) {
        self.input.hydrate(db);
        let mut rows = BTreeMap::new();
        self.input.fetch(db, Bound::Unbounded, false, &mut |k, v| {
            rows.insert(self.place(k, v), v.to_vec());
            true
        });
        self.rows = rows;
    }

    #[cfg(any(test, ark_selftest))]
    fn held(&self) -> usize {
        self.input.held() + self.rows.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>()
    }
}

/// The first `limit` of its input's rows, or the last when `last`. It
/// holds only how many it has and its last place in its own direction,
/// the window's bound, and fetches from its input the rows that come into
/// the window or go out of it.
struct Take {
    input: Box<dyn Op>,
    limit: usize,
    last: bool,
    size: usize,
    bound: Option<Vec<u8>>,
}

impl Take {
    /// Where the window ends: its bound, or `None` while it is not full
    /// and so holds all of its input.
    fn end(&self) -> Option<&[u8]> {
        self.bound.as_deref().filter(|_| self.size == self.limit)
    }
}

/// Whether `k` is in a window that ends at `end`, read in the take's
/// direction (`last` for descending).
fn inside(end: Option<&[u8]>, last: bool, k: &[u8]) -> bool {
    end.is_none_or(|b| if last { k >= b } else { k <= b })
}

/// From a window's end back toward its start: at the end, or from the
/// input's last row when it has none.
fn back(end: Option<&[u8]>) -> Bound<&[u8]> {
    end.map_or(Bound::Unbounded, Bound::Included)
}

impl Op for Take {
    fn fetch(&self, db: &dyn Read, from: Bound<&[u8]>, reverse: bool, f: &mut dyn FnMut(&[u8], &[u8]) -> bool) {
        if self.size == 0 {
            return;
        }
        let end = self.end();
        if reverse == self.last {
            // Toward the window's end: stop past it.
            self.input.fetch(db, from, reverse, &mut |k, v| inside(end, self.last, k) && f(k, v));
        } else {
            // From the end back: start at it, or before.
            self.input.fetch(db, tighter(from, back(end), self.last), reverse, f);
        }
    }

    fn push(&mut self, db: &dyn Read, changes: &Changes) -> Vec<Diff> {
        if changes.reset {
            self.hydrate(db);
            return Vec::new();
        }
        let d = self.input.push(db, changes);
        if d.is_empty() || self.limit == 0 {
            return Vec::new();
        }
        let (last, limit, old) = (self.last, self.limit, self.end().map(<[u8]>::to_vec));
        let end = old.as_deref();
        // The input's changes in the window as it stood, and how many of
        // the input's rows are there now.
        let mut kept: Vec<Diff> = d.into_iter().filter(|x| inside(end, last, &x.key)).collect();
        let mut count = self.size;
        for x in &kept {
            count = count + usize::from(x.new.is_some()) - usize::from(x.old.is_some());
        }
        let mut out = Vec::new();
        let mut bound = None;
        // Whether `out` was fetched descending, to be turned round.
        let down = (count > limit) != last;
        if count > limit {
            // Too many: the last of them leave, back from the end.
            let mut extra = count - limit;
            self.input.fetch(db, back(end), !last, &mut |k, v| {
                if extra == 0 {
                    bound = Some(k.to_vec());
                    return false;
                }
                extra -= 1;
                match kept.binary_search_by(|x| x.key[..].cmp(k)) {
                    Ok(i) => kept[i].new = None,
                    Err(_) => out.push(Diff {
                        key: k.to_vec(),
                        old: Some(v.to_vec()),
                        new: None,
                    }),
                }
                true
            });
            self.size = limit;
        } else {
            // Too few: the rows after the end come in, if it had one.
            let mut need = limit - count;
            if let (Some(b), true) = (end, need > 0) {
                self.input.fetch(db, Bound::Excluded(b), last, &mut |k, v| {
                    bound = Some(k.to_vec());
                    out.push(Diff {
                        key: k.to_vec(),
                        old: None,
                        new: Some(v.to_vec()),
                    });
                    need -= 1;
                    need > 0
                });
            }
            self.size = limit - need;
            if bound.is_none() && self.size > 0 {
                // The window's last row: at or before its old end.
                self.input.fetch(db, back(end), !last, &mut |k, _| {
                    bound = Some(k.to_vec());
                    false
                });
            }
        }
        self.bound = bound;
        if down {
            out.reverse();
        }
        merge(kept, out)
    }

    fn hydrate(&mut self, db: &dyn Read) {
        self.input.hydrate(db);
        let (mut size, mut bound) = (0, None);
        if self.limit > 0 {
            self.input.fetch(db, Bound::Unbounded, self.last, &mut |k, _| {
                size += 1;
                bound = Some(k.to_vec());
                size < self.limit
            });
        }
        (self.size, self.bound) = (size, bound);
    }

    #[cfg(any(test, ark_selftest))]
    fn held(&self) -> usize {
        self.input.held() + self.bound.as_ref().map_or(0, Vec::len)
    }
}

/// A group's running aggregates over the numbers its rows give: how many,
/// their sum, and (when kept) the least and the most. An average is the
/// sum over the count, so these are what sync, never the average.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Agg {
    pub count: u64,
    pub sum: i64,
    pub min: Option<i64>,
    pub max: Option<i64>,
}

impl Agg {
    fn add(&mut self, v: i64, extremes: bool) {
        self.count += 1;
        self.sum = self.sum.wrapping_add(v);
        if extremes {
            self.min = Some(self.min.map_or(v, |m| m.min(v)));
            self.max = Some(self.max.map_or(v, |m| m.max(v)));
        }
    }
}

impl Encode for Agg {
    fn encode(&self, out: &mut Vec<u8>) {
        self.count.encode(out);
        self.sum.encode(out);
        self.min.encode(out);
        self.max.encode(out);
    }
}

impl Decode for Agg {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        Ok(Agg {
            count: u64::decode(r)?,
            sum: i64::decode(r)?,
            min: Option::decode(r)?,
            max: Option::decode(r)?,
        })
    }
}

/// One row per group of its input's rows, placed by the group's key
/// (`by`), its value the group's [`Agg`] of the numbers `of` gives, as its
/// bytes. Count and sum are running totals; the least and the most, kept
/// when `extremes`, are fetched again from the input when the row that was
/// one leaves: the group's range when `prefix` says its key is a prefix of
/// its rows' places, else the whole input.
struct Group {
    input: Box<dyn Op>,
    by: Each<Vec<u8>>,
    of: Each<i64>,
    extremes: bool,
    prefix: bool,
    groups: BTreeMap<Vec<u8>, Vec<u8>>,
}

impl Group {
    /// `g`'s aggregates, from its rows in the input.
    fn refetch(&self, db: &dyn Read, g: &[u8]) -> Agg {
        let mut agg = Agg::default();
        let from = if self.prefix { Bound::Included(g) } else { Bound::Unbounded };
        self.input.fetch(db, from, false, &mut |k, v| {
            if self.prefix && !k.starts_with(g) {
                return false;
            }
            if (self.by)(k, v) == g {
                agg.add((self.of)(k, v), self.extremes);
            }
            true
        });
        agg
    }

    /// Adds `v`'s number to its group, or takes it away: whether the group
    /// lost its least or most.
    fn count(&mut self, k: &[u8], v: &[u8], add: bool) -> bool {
        let g = (self.by)(k, v);
        let n = (self.of)(k, v);
        let mut agg: Agg = self.groups.get(&g).and_then(|b| from_slice(b).ok()).unwrap_or_default();
        let mut lost = false;
        if add {
            agg.add(n, self.extremes);
        } else {
            agg.count -= 1;
            agg.sum = agg.sum.wrapping_sub(n);
            lost = self.extremes && (agg.min == Some(n) || agg.max == Some(n));
        }
        if agg.count == 0 {
            self.groups.remove(&g);
        } else {
            self.groups.insert(g, to_vec(&agg));
        }
        lost
    }
}

impl Op for Group {
    fn fetch(&self, _: &dyn Read, from: Bound<&[u8]>, reverse: bool, f: &mut dyn FnMut(&[u8], &[u8]) -> bool) {
        fetch_map(&self.groups, from, reverse, &mut |k, v| f(k, v));
    }

    fn push(&mut self, db: &dyn Read, changes: &Changes) -> Vec<Diff> {
        if changes.reset {
            self.hydrate(db);
            return Vec::new();
        }
        // Each group touched, in key order, with its row before; and those
        // whose least or most left.
        let mut before: Vec<Diff> = Vec::new();
        let mut stale: Vec<Vec<u8>> = Vec::new();
        for d in self.input.push(db, changes) {
            for (v, add) in [(&d.old, false), (&d.new, true)] {
                let Some(v) = v else { continue };
                let g = (self.by)(&d.key, v);
                if let Err(i) = before.binary_search_by(|x| x.key.cmp(&g)) {
                    let old = self.groups.get(&g).cloned();
                    before.insert(
                        i,
                        Diff {
                            key: g.clone(),
                            old,
                            new: None,
                        },
                    );
                }
                if self.count(&d.key, v, add) {
                    stale.push(g);
                }
            }
        }
        // The extremes that left, fetched again once every change is in.
        for g in stale {
            if self.groups.contains_key(&g) {
                let agg = to_vec(&self.refetch(db, &g));
                self.groups.insert(g, agg);
            }
        }
        for d in &mut before {
            d.new = self.groups.get(&d.key).cloned();
        }
        merge(before, Vec::new())
    }

    fn hydrate(&mut self, db: &dyn Read) {
        self.input.hydrate(db);
        self.groups.clear();
        let mut rows = Vec::new();
        self.input.fetch(db, Bound::Unbounded, false, &mut |k, v| {
            rows.push((k.to_vec(), v.to_vec()));
            true
        });
        for (k, v) in rows {
            self.count(&k, &v, true);
        }
    }

    #[cfg(any(test, ark_selftest))]
    fn held(&self) -> usize {
        self.input.held() + self.groups.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>()
    }
}

/// A query being built: an operator over the ones before it.
pub struct Plan(Box<dyn Op>);

/// A table's rows with keys from `lo` to `hi`: a table, or a range of an
/// index.
pub fn scan(table: TableId, lo: Bound<Vec<u8>>, hi: Bound<Vec<u8>>) -> Plan {
    Plan(Box::new(Scan { table, lo, hi }))
}

impl Plan {
    /// The rows `keep` keeps.
    pub fn filter(self, keep: impl Fn(&[u8], &[u8]) -> bool + 'static) -> Plan {
        Plan(Box::new(Filter {
            input: self.0,
            keep: Box::new(keep),
        }))
    }

    /// Each row's value made another by `make`.
    pub fn map(self, make: impl Fn(&[u8], &[u8]) -> Vec<u8> + 'static) -> Plan {
        Plan(Box::new(Map {
            input: self.0,
            make: Box::new(make),
        }))
    }

    /// The rows placed by the bytes `by` gives first: bytes whose order is
    /// the sort's, none a prefix of another. Holds its input: where an
    /// index gives the order, scan the index instead.
    pub fn sort(self, by: impl Fn(&[u8], &[u8]) -> Vec<u8> + 'static) -> Plan {
        Plan(Box::new(Sort {
            input: self.0,
            by: Box::new(by),
            rows: BTreeMap::new(),
        }))
    }

    /// The first `limit` rows, or the last when `last`.
    pub fn take(self, limit: usize, last: bool) -> Plan {
        Plan(Box::new(Take {
            input: self.0,
            limit,
            last,
            size: 0,
            bound: None,
        }))
    }

    /// One row per group of the rows (`by` its key), its value the group's
    /// [`Agg`] of the numbers `of` gives; the least and most kept when
    /// `extremes`. `prefix` says each group's key is a prefix of its rows'
    /// places, so a group is fetched again as a range.
    pub fn group(
        self,
        by: impl Fn(&[u8], &[u8]) -> Vec<u8> + 'static,
        of: impl Fn(&[u8], &[u8]) -> i64 + 'static,
        extremes: bool,
        prefix: bool,
    ) -> Plan {
        Plan(Box::new(Group {
            input: self.0,
            by: Box::new(by),
            of: Box::new(of),
            extremes,
            prefix,
            groups: BTreeMap::new(),
        }))
    }

    /// The plan's answer over `db`, kept live from here.
    pub fn live(self, db: &dyn Read) -> Live {
        let mut op = self.0;
        op.hydrate(db);
        let rows = fetch(&*op, db, Bound::Unbounded, false, usize::MAX);
        Live { op, rows }
    }
}

/// Up to `limit` of `op`'s rows from `from`, in order.
fn fetch(op: &dyn Op, db: &dyn Read, from: Bound<&[u8]>, reverse: bool, limit: usize) -> Vec<Row> {
    let mut out = Vec::new();
    if limit > 0 {
        op.fetch(db, from, reverse, &mut |k, v| {
            out.push((k.to_vec(), v.to_vec()));
            out.len() < limit
        });
    }
    out
}

/// A change to a list, by position: each applies to the list as the ones
/// before it left it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Patch {
    /// A row before the one at `at` (`at` the length: at the end).
    Insert {
        at: usize,
        row: Row,
    },
    Remove {
        at: usize,
    },
    Update {
        at: usize,
        value: Vec<u8>,
    },
}

/// A plan's answer, kept: its operators and the rows they answer, which a
/// view's list holds as well.
pub struct Live {
    op: Box<dyn Op>,
    rows: Vec<Row>,
}

impl Live {
    /// The answer as it stands.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Up to `limit` rows from `from`, in order, as the plan's operators
    /// answer them: a page, resumed from the last one's last place.
    pub fn fetch(&self, db: &dyn Read, from: Bound<&[u8]>, reverse: bool, limit: usize) -> Vec<Row> {
        fetch(&*self.op, db, from, reverse, limit)
    }

    /// Takes one settle's changes, `db` already after them, and answers the
    /// patches that take the answer to the new one.
    pub fn push(&mut self, db: &dyn Read, changes: &Changes) -> Vec<Patch> {
        let diffs = if changes.reset {
            self.op.push(db, changes);
            let new = fetch(&*self.op, db, Bound::Unbounded, false, usize::MAX);
            let diff = |rows: Vec<Row>, old: bool| {
                let each = rows.into_iter().map(|(key, v)| Diff {
                    key,
                    old: old.then(|| v.clone()),
                    new: (!old).then_some(v),
                });
                each.collect()
            };
            merge(diff(self.rows.clone(), true), diff(new, false))
        } else {
            self.op.push(db, changes)
        };
        let mut out = Vec::new();
        for d in diffs {
            let found = self.rows.binary_search_by(|(k, _)| k.cmp(&d.key));
            match (found, d.new) {
                (Ok(at), None) => {
                    self.rows.remove(at);
                    out.push(Patch::Remove { at });
                }
                (Ok(at), Some(value)) => {
                    self.rows[at].1 = value.clone();
                    out.push(Patch::Update { at, value });
                }
                (Err(at), Some(value)) => {
                    let row = (d.key, value);
                    self.rows.insert(at, row.clone());
                    out.push(Patch::Insert { at, row });
                }
                (Err(_), None) => {}
            }
        }
        out
    }

    #[cfg(any(test, ark_selftest))]
    fn held(&self) -> usize {
        self.op.held()
    }
}

#[cfg(any(test, ark_selftest))]
pub(crate) mod tests {
    use super::*;
    use crate::store::{Memory, Store};
    use crate::Rng;
    use alloc::vec;

    const T: TableId = TableId(1);
    /// A table no plan reads: its changes must move nothing.
    const U: TableId = TableId(2);

    /// Every row of `table`.
    fn all(db: &dyn Read, table: TableId) -> Vec<Row> {
        let mut out = Vec::new();
        db.scan(table, Range::all(), &mut |k, v| {
            out.push((k.to_vec(), v.to_vec()));
            true
        });
        out
    }

    /// What changed from `a` to `b`, as a node's changes say it.
    pub(crate) fn diff(a: &dyn Read, b: &dyn Read, tables: &[TableId]) -> Changes {
        let mut rows = Vec::new();
        for &t in tables {
            let mut by: BTreeMap<Vec<u8>, Diff> = BTreeMap::new();
            for (k, v) in all(a, t) {
                by.insert(
                    k.clone(),
                    Diff {
                        key: k,
                        old: Some(v),
                        new: None,
                    },
                );
            }
            for (k, v) in all(b, t) {
                by.entry(k.clone())
                    .or_insert(Diff {
                        key: k,
                        old: None,
                        new: None,
                    })
                    .new = Some(v);
            }
            rows.extend(merge(by.into_values().collect(), Vec::new()).into_iter().map(|d| (t, d)));
        }
        Changes { reset: false, rows }
    }

    /// Each patch applied in order: what a view's list is held to.
    pub(crate) fn splice(list: &mut Vec<Row>, patches: &[Patch]) {
        for p in patches {
            match p {
                Patch::Insert { at, row } => list.insert(*at, row.clone()),
                Patch::Remove { at } => drop(list.remove(*at)),
                Patch::Update { at, value } => list[*at].1 = value.clone(),
            }
        }
    }

    fn key(rng: &mut Rng) -> Vec<u8> {
        (0..1 + rng.below(2)).map(|_| rng.below(6) as u8).collect()
    }

    fn bound(rng: &mut Rng, key: Vec<u8>) -> Bound<Vec<u8>> {
        match rng.below(3) {
            0 => Bound::Unbounded,
            1 => Bound::Included(key),
            _ => Bound::Excluded(key),
        }
    }

    /// A random plan, the same for the same seed: a scan of a range, then
    /// up to four of filter, map, sort, take and group, in any order.
    fn plan(seed: u64) -> Plan {
        let mut rng = Rng::new(seed);
        let (lo, hi) = (key(&mut rng), key(&mut rng));
        let mut p = scan(T, bound(&mut rng, lo), bound(&mut rng, hi));
        for _ in 0..rng.below(5) {
            p = match rng.below(7) {
                0 => {
                    let m = 2 + rng.below(3) as u8;
                    p.filter(move |_, v| v[0] % m != 0)
                }
                1 => p.map(|k, v| vec![v[0] / 2, k[0]]),
                2 => p.sort(|_, v| vec![v[0] % 4]),
                3 | 4 => p.take(rng.below(5) as usize, rng.below(2) == 0),
                5 => p.group(|k, _| vec![k[0]], |_, v| i64::from(v[0]) - 3, rng.below(2) == 0, true),
                _ => p.group(|_, v| vec![v[0] % 3], |_, v| i64::from(v[0]), true, false),
            };
        }
        p
    }

    fn random_from(rng: &mut Rng, rows: &[Row]) -> Bound<Vec<u8>> {
        let k = match rows.len() {
            0 => key(rng),
            n if rng.below(3) > 0 => rows[rng.below(n as u64) as usize].0.clone(),
            _ => key(rng),
        };
        bound(rng, k)
    }

    /// The contract: after any sequence of random changes (and now and
    /// then a reset, as a fill makes), a random plan's answer is a fresh
    /// run's, its patches splice the old answer into it, and a fetch from
    /// anywhere, either way, of any length, is the fresh run's.
    #[cfg_attr(test, test)]
    fn every_answer_is_a_fresh_runs() {
        for seed in 0..150 {
            let mut rng = Rng::new(seed ^ 0x5eed);
            let mut db = Memory::new();
            for _ in 0..rng.below(12) {
                let (t, k) = ([T, U][rng.below(2) as usize], key(&mut rng));
                db.put(t, &k, &[rng.below(9) as u8]);
            }
            let mut live = plan(seed).live(&db);
            let mut shown = live.rows().to_vec();
            for round in 0..25 {
                let before = db.clone();
                for _ in 0..1 + rng.below(5) {
                    let (t, k) = ([T, T, U][rng.below(3) as usize], key(&mut rng));
                    if rng.below(3) == 0 {
                        db.delete(t, &k);
                    } else {
                        let v: Vec<u8> = (0..1 + rng.below(2)).map(|_| rng.below(9) as u8).collect();
                        db.put(t, &k, &v);
                    }
                }
                let changes = if rng.below(12) == 0 {
                    Changes { reset: true, rows: vec![] }
                } else {
                    diff(&before, &db, &[T, U])
                };
                let patches = live.push(&db, &changes);
                splice(&mut shown, &patches);
                let fresh = plan(seed).live(&db);
                assert_eq!(live.rows(), fresh.rows(), "seed {seed}, round {round}");
                assert_eq!(shown, fresh.rows(), "seed {seed}, round {round}: the patches");
                for _ in 0..4 {
                    let from = random_from(&mut rng, fresh.rows());
                    let (reverse, limit) = (rng.below(2) == 0, rng.below(5) as usize);
                    let from = from.as_ref().map(Vec::as_slice);
                    assert_eq!(
                        live.fetch(&db, from, reverse, limit),
                        fresh.fetch(&db, from, reverse, limit),
                        "seed {seed}: {from:?} {reverse}"
                    );
                }
            }
        }
    }

    fn row(i: u16) -> (Vec<u8>, Vec<u8>) {
        (i.to_be_bytes().to_vec(), vec![(i % 7) as u8])
    }

    /// A bounded take over a table, or an index's range, holds its size and
    /// one place, however many rows are under it and however they change;
    /// a sort no index gives holds its input.
    #[cfg_attr(test, test)]
    fn a_bounded_take_holds_its_limit() {
        let mut held = Vec::new();
        for n in [50u16, 1500] {
            let mut db = Memory::new();
            for i in 0..n {
                let (k, v) = row(i);
                db.put(T, &k, &v);
            }
            let take = |db: &Memory| {
                scan(T, Bound::Unbounded, Bound::Unbounded)
                    .filter(|_, v| v[0] != 3)
                    .take(10, false)
                    .live(db)
            };
            let mut live = take(&db);
            assert_eq!(live.rows().len(), 10);
            held.push(live.held());
            // The window's rows leave and come back, rows before it come
            // in: it fetches what it needs and still holds one place.
            let change = |db: &mut Memory, i: u16, put: bool| {
                let (key, v) = row(i);
                let (old, new) = if put { (None, Some(v)) } else { (db.get(T, &key), None) };
                match &new {
                    Some(v) => db.put(T, &key, v),
                    None => db.delete(T, &key),
                }
                Changes {
                    reset: false,
                    rows: vec![(T, Diff { key, old, new })],
                }
            };
            for (i, put) in [
                (0u16, false),
                (1, false),
                (4, false),
                (9, false),
                (2, false),
                (12, false),
                (0, true),
                (4, true),
            ] {
                let changes = change(&mut db, i, put);
                assert!(!live.push(&db, &changes).is_empty());
                assert!(live.held() <= 2);
            }
            assert_eq!(live.rows(), take(&db).rows());
            let sorted = scan(T, Bound::Unbounded, Bound::Unbounded)
                .sort(|_, v| vec![v[0]])
                .take(10, false)
                .live(&db);
            assert!(sorted.held() > 3 * usize::from(n) - 30);
        }
        assert_eq!(held, [2, 2]);
    }

    /// A group keeps running totals; the least and the most are fetched
    /// again only when the row that was one leaves.
    #[cfg_attr(test, test)]
    fn a_group_runs_its_totals() {
        let mut db = Memory::new();
        for (k, v) in [([1u8, 1], 5u8), ([1, 2], 9), ([2, 1], 4)] {
            db.put(T, &k, &[v]);
        }
        let mut live = scan(T, Bound::Unbounded, Bound::Unbounded)
            .group(|k, _| vec![k[0]], |_, v| i64::from(v[0]), true, true)
            .live(&db);
        let agg = |count, sum, min, max| {
            to_vec(&Agg {
                count,
                sum,
                min: Some(min),
                max: Some(max),
            })
        };
        assert_eq!(live.rows(), [(vec![1], agg(2, 14, 5, 9)), (vec![2], agg(1, 4, 4, 4))]);
        let before = db.clone();
        db.delete(T, &[1, 2]);
        db.delete(T, &[2, 1]);
        db.put(T, &[3], &[1]);
        assert_eq!(
            live.push(&db, &diff(&before, &db, &[T])),
            [
                Patch::Update {
                    at: 0,
                    value: agg(1, 5, 5, 5)
                },
                Patch::Remove { at: 1 },
                Patch::Insert {
                    at: 1,
                    row: (vec![3], agg(1, 1, 1, 1))
                }
            ]
        );
        assert_eq!(from_slice::<Agg>(&live.rows()[0].1).map(|a| a.sum), Ok(5));
    }

    crate::listed!(every_answer_is_a_fresh_runs, a_bounded_take_holds_its_limit, a_group_runs_its_totals);
}
