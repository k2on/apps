//! Queries as plans of incremental operators, ported from `rust/ark`'s
//! `plan.rs` and made pull-based, as Zero's are: an operator holds as
//! little as it can and asks the one below it for rows when it needs them.
//!
//! ```text
//! scan(table, lo, hi) . filter(f) . map(f) . sort(by) . take(n) . group(by, of)
//!   . related(children) . join(children)
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
//! | `filter`, `map` | nothing (what each row read, when it reads) | the closure again on the old and new row |
//! | `take` | its size and its last place | fetches the neighbours of its window that come in or go out |
//! | `sort` | its input, sorted | the expensive case: a sort no index provides |
//! | `group` | each group's running count, sum, min and max | count and sum in constant time; min or max fetched again when the extreme leaves |
//! | `related`, `join` | per parent, its children's plan (a take's size and bound), never their rows | a parent's children pushed the settle; a parent that changed, its children made again |
//!
//! **Push.** One settle's [`Changes`] (what a node's overlay or the log's
//! facts did to what it reads, its store already after them) go up through
//! the operators, each answering the [`Diff`]s of its own rows; a [`Live`]
//! answer turns them into the positional [`Patch`]es that splice a list a
//! view holds into the new answer. An operator is given the rows after the
//! settle and as they were before it ([`Settle`]), so a row's old side is
//! made over the rows it was made over. The contract, held by the tests:
//! after every push, every operator's rows are a fresh run's, and the
//! patches splice the old answer into the new one.
//!
//! **Reads.** The `_with` forms' closures read other rows through the
//! `&dyn Read` they are given. What each row's run read is recorded, per
//! row, as ranges of keys (a key read is the range of itself; a scan
//! stopped early read up to where it stopped), and a change a recorded
//! range covers runs the closure again for just the rows whose runs read
//! it: their old side over the rows before, their new over the rows after.
//! An operator whose closures read holds those ranges for every row of its
//! input, and nothing for a row that read nothing.
//!
//! **Parents and children.** `related` answers each of its input's rows (a
//! parent) followed by its children, the rows of a plan made for it (a
//! [`prefix`] of an index by the parent's key, say, limited by a `take`):
//! Zero's hierarchical join. Places nest, a parent's escaped and ended and
//! its child's after it, so the answer is one ordered list a view shows as
//! a nested one: a row's parent is the nearest row before it whose place
//! begins its own, and a patch's position is in that list. `join` is the
//! same without the parents' own rows: an equi-join, each pair a child.
//!
//! **What syncs.** An upstream sends a subscriber the base rows its query's
//! answer is made of ([`Live::base`]): each answer row's row of its scan,
//! the rows its closures read for it, and a child's parent's, so the node
//! runs the same plan over what it holds and gets the same answer. A
//! subscription's query is a scan, its filters, maps, takes, related and
//! joins; sorts and groups are the node's own.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::ops::Bound;

use crate::codec::{from_slice, put_bytes, to_vec, Decode, Encode, Error, Reader};
use crate::store::{walk, Layer, Range, Read, Sparse, TableId, Tree, View};

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

/// One settle, as an operator takes it: the rows after it (`db`), the rows
/// as they were before it (`was`), and what changed.
pub struct Settle<'a> {
    pub db: &'a dyn Read,
    pub was: &'a dyn Read,
    pub changes: &'a Changes,
}

/// One operator of a plan.
pub trait Op {
    /// Calls `f` with each row in order, ascending or, when `reverse`,
    /// descending, from `from` (at a place, after it, or from the start),
    /// until `f` returns false or the rows end.
    fn fetch(&self, db: &dyn Read, from: Bound<&[u8]>, reverse: bool, f: &mut dyn FnMut(&[u8], &[u8]) -> bool);

    /// Takes one settle, and answers what it did to this operator's rows,
    /// in place order.
    fn push(&mut self, s: &Settle<'_>) -> Vec<Diff>;

    /// Builds its state over `db`, after its input's.
    fn hydrate(&mut self, db: &dyn Read);

    /// Calls `f` with each base row its row at `place` is made of, and each
    /// row its closures read for it, as `db` holds them: what an upstream
    /// sends a node subscribed to it.
    #[cfg(feature = "upstream")]
    fn base(&self, db: &dyn Read, place: &[u8], f: &mut dyn FnMut(TableId, &[u8]));

    /// The bytes of state it and its inputs hold: what the tests hold to a
    /// bound.
    #[cfg(any(test, ark_selftest))]
    fn held(&self) -> usize;
}

/// A row-wise closure: what it may read, and a row's place and value, in; a
/// verdict or bytes out.
pub type Fun<T> = Box<dyn Fn(&dyn Read, &[u8], &[u8]) -> T>;

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
fn fetch_map(rows: &Tree, from: Bound<&[u8]>, reverse: bool, f: &mut dyn FnMut(&[u8], &[u8]) -> bool) {
    // One end is open, so the range is never one a `BTreeMap` refuses.
    let r = span(Bound::Unbounded, Bound::Unbounded, from, reverse);
    walk(rows, r.lo, r.hi, reverse, &mut |k, v| v.as_deref().is_none_or(|v| f(k, v)));
}

/// Two lists of diffs in place order as one, a place in both taking its
/// old from `a` and its new from `b`, with any whose old and new are the
/// same left out.
fn merge(a: Vec<Diff>, b: Vec<Diff>) -> Vec<Diff> {
    let mut out = a;
    // From the end, so each insert past the last moves nothing.
    let mut i = out.len();
    for y in b.into_iter().rev() {
        while i > 0 && out[i - 1].key > y.key {
            i -= 1;
        }
        match i.checked_sub(1).filter(|&j| out[j].key == y.key) {
            Some(j) => out[j].new = y.new,
            None => out.insert(i, y),
        }
    }
    out.retain(|d| d.old != d.new);
    out
}

/// A store read through, each read noted as the range of keys it covered,
/// when `on`: what a closure that reads is given.
struct Recorder<'a> {
    db: &'a dyn Read,
    /// The ranges read, each its table and its bounds, encoded.
    spans: Option<RefCell<Vec<u8>>>,
}

impl<'a> Recorder<'a> {
    fn new(db: &'a dyn Read, on: bool) -> Recorder<'a> {
        Recorder {
            db,
            spans: on.then(Default::default),
        }
    }

    fn note(&self, table: TableId, lo: Bound<&[u8]>, hi: Bound<&[u8]>) {
        if let Some(s) = &self.spans {
            let out = &mut *s.borrow_mut();
            table.encode(out);
            for b in [lo, hi] {
                match b {
                    Bound::Unbounded => out.push(0),
                    Bound::Included(k) | Bound::Excluded(k) => {
                        out.push(if matches!(b, Bound::Included(_)) { 1 } else { 2 });
                        put_bytes(out, k);
                    }
                }
            }
        }
    }
}

impl Read for Recorder<'_> {
    fn get(&self, table: TableId, key: &[u8]) -> Option<Vec<u8>> {
        self.note(table, Bound::Included(key), Bound::Included(key));
        self.db.get(table, key)
    }

    fn scan(&self, table: TableId, range: Range<'_>, f: &mut dyn FnMut(&[u8], &[u8]) -> bool) {
        let mut stop = None;
        self.db.scan(table, range, &mut |k, v| {
            let on = f(k, v);
            if !on {
                stop = Some(k.to_vec());
            }
            on
        });
        // Stopped, it read only as far as the row it stopped at.
        let (lo, hi) = match (stop.as_deref(), range.reverse) {
            (Some(at), false) => (range.lo, Bound::Included(at)),
            (Some(at), true) => (Bound::Included(at), range.hi),
            (None, _) => (range.lo, range.hi),
        };
        self.note(table, lo, hi);
    }
}

/// A recorded range: its table and bounds.
type Span<'a> = (TableId, Bound<&'a [u8]>, Bound<&'a [u8]>);

/// The ranges `spans` records.
fn spans(spans: &[u8]) -> impl Iterator<Item = Span<'_>> {
    let mut r = Reader::new(spans);
    core::iter::from_fn(move || {
        let table = TableId::decode(&mut r).ok()?;
        Some((table, bound(&mut r).ok()?, bound(&mut r).ok()?))
    })
}

/// A recorded bound.
fn bound<'a>(r: &mut Reader<'a>) -> Result<Bound<&'a [u8]>, Error> {
    Ok(match r.byte()? {
        0 => Bound::Unbounded,
        1 => Bound::Included(r.bytes()?),
        _ => Bound::Excluded(r.bytes()?),
    })
}

/// What an operator's closures read, by the place of the input row each
/// run was for. A row whose run read nothing holds nothing.
#[derive(Default)]
struct Reads(Sparse);

impl Reads {
    /// Runs `f` over `db` for the row at `place`, keeping what it read in
    /// place of what it read before, when the operator records reads.
    fn run<T>(reads: &mut Option<Reads>, db: &dyn Read, place: &Vec<u8>, f: impl FnOnce(&dyn Read) -> T) -> T {
        let r = Recorder::new(db, reads.is_some());
        let out = f(&r);
        if let (Some(reads), Some(s)) = (reads, r.spans) {
            let s = s.into_inner();
            if s.is_empty() {
                reads.0.take(place);
            } else {
                reads.0.put(place.clone(), s);
            }
        }
        out
    }

    /// Lets go of what the row at `place` read: it left the input.
    fn drop(reads: &mut Option<Reads>, place: &Vec<u8>) {
        if let Some(reads) = reads {
            reads.0.take(place);
        }
    }

    /// The input's diffs for one settle, and for each row whose recorded
    /// reads a change covers, a diff of the row (unchanged) to itself: its
    /// closures run again, over the rows before and after.
    fn pull(reads: &Option<Reads>, input: &mut dyn Op, s: &Settle<'_>) -> Vec<Diff> {
        let mut out = input.push(s);
        let Some(reads) = reads else { return out };
        let changes = &s.changes.rows;
        for (place, read) in &reads.0.tree {
            let Some(read) = read else { continue };
            let mut read = spans(read);
            if !read.any(|(t, lo, hi)| changes.iter().any(|(ct, d)| *ct == t && within(lo, hi, &d.key))) {
                continue;
            }
            let Err(i) = out.binary_search_by(|d| d.key.cmp(place)) else { continue };
            let mut row = None;
            input.fetch(s.db, Bound::Included(place), false, &mut |k, v| {
                if k == &place[..] {
                    row = Some(v.to_vec());
                }
                false
            });
            if let Some(v) = row {
                let d = Diff {
                    key: place.clone(),
                    old: Some(v.clone()),
                    new: Some(v),
                };
                out.insert(i, d);
            }
        }
        out
    }

    /// Calls `f` with each row `db` holds that the row at `place` read.
    #[cfg(feature = "upstream")]
    fn base(reads: &Option<Reads>, db: &dyn Read, place: &[u8], f: &mut dyn FnMut(TableId, &[u8])) {
        let Some(read) = reads.as_ref().and_then(|r| r.0.get(&place.to_vec())) else {
            return;
        };
        for (t, lo, hi) in spans(read) {
            db.scan(t, Range { lo, hi, reverse: false }, &mut |k, _| {
                f(t, k);
                true
            });
        }
    }

    /// The bytes it holds.
    #[cfg(any(test, ark_selftest))]
    fn held(reads: &Option<Reads>) -> usize {
        reads.as_ref().map_or(0, |r| held(&r.0.tree))
    }
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

    fn push(&mut self, s: &Settle<'_>) -> Vec<Diff> {
        let (lo, hi) = (self.lo.as_ref().map(Vec::as_slice), self.hi.as_ref().map(Vec::as_slice));
        let mine = s.changes.rows.iter().filter(|(t, d)| *t == self.table && within(lo, hi, &d.key));
        mine.map(|(_, d)| d.clone()).collect()
    }

    fn hydrate(&mut self, _: &dyn Read) {}

    #[cfg(feature = "upstream")]
    fn base(&self, _: &dyn Read, place: &[u8], f: &mut dyn FnMut(TableId, &[u8])) {
        f(self.table, place);
    }

    #[cfg(any(test, ark_selftest))]
    fn held(&self) -> usize {
        0
    }
}

/// Its input's rows, each made another by `f` or left out: a filter, or a
/// map.
struct Each {
    input: Box<dyn Op>,
    f: Fun<Option<Vec<u8>>>,
    reads: Option<Reads>,
}

impl Op for Each {
    fn fetch(&self, db: &dyn Read, from: Bound<&[u8]>, reverse: bool, f: &mut dyn FnMut(&[u8], &[u8]) -> bool) {
        self.input
            .fetch(db, from, reverse, &mut |k, v| (self.f)(db, k, v).is_none_or(|u| f(k, &u)));
    }

    fn push(&mut self, s: &Settle<'_>) -> Vec<Diff> {
        if s.changes.reset {
            self.hydrate(s.db);
            return Vec::new();
        }
        let mut out = Reads::pull(&self.reads, &mut *self.input, s);
        let each = &self.f;
        for d in &mut out {
            d.old = d.old.take().and_then(|v| each(s.was, &d.key, &v));
            d.new = match d.new.take() {
                Some(v) => Reads::run(&mut self.reads, s.db, &d.key, |db| each(db, &d.key, &v)),
                None => {
                    Reads::drop(&mut self.reads, &d.key);
                    None
                }
            };
        }
        out.retain(|d| d.old != d.new);
        out
    }

    fn hydrate(&mut self, db: &dyn Read) {
        self.input.hydrate(db);
        if self.reads.is_some() {
            self.reads = Some(Reads::default());
            for (k, v) in fetch(&*self.input, db, Bound::Unbounded, false, usize::MAX) {
                Reads::run(&mut self.reads, db, &k, |db| (self.f)(db, &k, &v));
            }
        }
    }

    #[cfg(feature = "upstream")]
    fn base(&self, db: &dyn Read, place: &[u8], f: &mut dyn FnMut(TableId, &[u8])) {
        self.input.base(db, place, f);
        Reads::base(&self.reads, db, place, f);
    }

    #[cfg(any(test, ark_selftest))]
    fn held(&self) -> usize {
        self.input.held() + Reads::held(&self.reads)
    }
}

/// Its input's rows placed by `by` first, then by their place: a sort no
/// index provides, which holds its input, sorted, to answer a fetch in
/// order. The expensive case; a sort an index provides is a scan of it.
struct Sort {
    input: Box<dyn Op>,
    by: Fun<Vec<u8>>,
    rows: Sparse,
    reads: Option<Reads>,
}

/// A row's place in a sort: `by`'s bytes, then its place.
fn sorted(by: &Fun<Vec<u8>>, db: &dyn Read, k: &[u8], v: &[u8]) -> Vec<u8> {
    let mut at = by(db, k, v);
    at.extend_from_slice(k);
    at
}

impl Op for Sort {
    fn fetch(&self, _: &dyn Read, from: Bound<&[u8]>, reverse: bool, f: &mut dyn FnMut(&[u8], &[u8]) -> bool) {
        fetch_map(&self.rows.tree, from, reverse, f);
    }

    fn push(&mut self, s: &Settle<'_>) -> Vec<Diff> {
        if s.changes.reset {
            self.hydrate(s.db);
            return Vec::new();
        }
        let mut out: BTreeMap<Vec<u8>, Diff> = BTreeMap::new();
        for d in Reads::pull(&self.reads, &mut *self.input, s) {
            if let Some(old) = d.old {
                let at = sorted(&self.by, s.was, &d.key, &old);
                self.rows.take(&at);
                out.insert(
                    at.clone(),
                    Diff {
                        key: at,
                        old: Some(old),
                        new: None,
                    },
                );
            }
            let Some(new) = d.new else {
                Reads::drop(&mut self.reads, &d.key);
                continue;
            };
            let by = &self.by;
            let at = Reads::run(&mut self.reads, s.db, &d.key, |db| sorted(by, db, &d.key, &new));
            self.rows.put(at.clone(), new.clone());
            out.entry(at.clone())
                .or_insert(Diff {
                    key: at,
                    old: None,
                    new: None,
                })
                .new = Some(new);
        }
        merge(out.into_values().collect(), Vec::new())
    }

    fn hydrate(&mut self, db: &dyn Read) {
        self.input.hydrate(db);
        self.rows = Sparse::default();
        self.reads = self.reads.take().map(|_| Reads::default());
        for (k, v) in fetch(&*self.input, db, Bound::Unbounded, false, usize::MAX) {
            let by = &self.by;
            let at = Reads::run(&mut self.reads, db, &k, |db| sorted(by, db, &k, &v));
            self.rows.put(at, v);
        }
    }

    /// Not a subscription's: a node sorts what it holds.
    #[cfg(feature = "upstream")]
    fn base(&self, _: &dyn Read, _: &[u8], _: &mut dyn FnMut(TableId, &[u8])) {}

    #[cfg(any(test, ark_selftest))]
    fn held(&self) -> usize {
        self.input.held() + held(&self.rows.tree) + Reads::held(&self.reads)
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

    fn push(&mut self, s: &Settle<'_>) -> Vec<Diff> {
        let db = s.db;
        if s.changes.reset {
            self.hydrate(db);
            return Vec::new();
        }
        let d = self.input.push(s);
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

    #[cfg(feature = "upstream")]
    fn base(&self, db: &dyn Read, place: &[u8], f: &mut dyn FnMut(TableId, &[u8])) {
        self.input.base(db, place, f);
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
    by: Fun<Vec<u8>>,
    of: Fun<i64>,
    extremes: bool,
    prefix: bool,
    groups: Sparse,
    reads: Option<Reads>,
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
            if (self.by)(db, k, v) == g {
                agg.add((self.of)(db, k, v), self.extremes);
            }
            true
        });
        agg
    }

    /// A row's group and number, over `db`, what it read kept when `keep`.
    fn place(&mut self, db: &dyn Read, k: &Vec<u8>, v: &[u8], keep: bool) -> (Vec<u8>, i64) {
        let (by, of) = (&self.by, &self.of);
        let run = |db: &dyn Read| (by(db, k, v), of(db, k, v));
        if keep {
            Reads::run(&mut self.reads, db, k, run)
        } else {
            run(db)
        }
    }

    /// Adds `n` to group `g`, or takes it away: whether the group lost its
    /// least or most.
    fn count(&mut self, g: Vec<u8>, n: i64, add: bool) -> bool {
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
            self.groups.take(&g);
        } else {
            self.groups.put(g, to_vec(&agg));
        }
        lost
    }
}

impl Op for Group {
    fn fetch(&self, _: &dyn Read, from: Bound<&[u8]>, reverse: bool, f: &mut dyn FnMut(&[u8], &[u8]) -> bool) {
        fetch_map(&self.groups.tree, from, reverse, f);
    }

    fn push(&mut self, s: &Settle<'_>) -> Vec<Diff> {
        if s.changes.reset {
            self.hydrate(s.db);
            return Vec::new();
        }
        // Each group touched, in key order, with its row before; and those
        // whose least or most left.
        let mut before: Vec<Diff> = Vec::new();
        let mut stale: Vec<Vec<u8>> = Vec::new();
        for d in Reads::pull(&self.reads, &mut *self.input, s) {
            if d.new.is_none() {
                Reads::drop(&mut self.reads, &d.key);
            }
            for (v, add) in [(&d.old, false), (&d.new, true)] {
                let Some(v) = v else { continue };
                let (g, n) = self.place(if add { s.db } else { s.was }, &d.key, v, add);
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
                if self.count(g.clone(), n, add) {
                    stale.push(g);
                }
            }
        }
        // The extremes that left, fetched again once every change is in.
        for g in stale {
            if self.groups.get(&g).is_some() {
                let agg = to_vec(&self.refetch(s.db, &g));
                self.groups.put(g, agg);
            }
        }
        for d in &mut before {
            d.new = self.groups.get(&d.key).cloned();
        }
        merge(before, Vec::new())
    }

    fn hydrate(&mut self, db: &dyn Read) {
        self.input.hydrate(db);
        self.groups = Sparse::default();
        self.reads = self.reads.take().map(|_| Reads::default());
        for (k, v) in fetch(&*self.input, db, Bound::Unbounded, false, usize::MAX) {
            let (g, n) = self.place(db, &k, &v, true);
            self.count(g, n, true);
        }
    }

    /// Not a subscription's: a node groups what it holds.
    #[cfg(feature = "upstream")]
    fn base(&self, _: &dyn Read, _: &[u8], _: &mut dyn FnMut(TableId, &[u8])) {}

    #[cfg(any(test, ark_selftest))]
    fn held(&self) -> usize {
        self.input.held() + held(&self.groups.tree) + Reads::held(&self.reads)
    }
}

/// What makes a parent's children's plan, from its place and value.
type Children = Box<dyn Fn(&[u8], &[u8]) -> Plan>;

/// A parent's place among its children's: its own place escaped (each 0
/// as 0 1) and ended by 0 0, so that no parent's is a prefix of another's
/// and each comes before every place that begins with it.
fn escaped(place: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(place.len() + 2);
    for &b in place {
        out.push(b);
        if b == 0 {
            out.push(1);
        }
    }
    out.extend_from_slice(&[0, 0]);
    out
}

/// A place of a related answer: its parent's place, and the child's within
/// it (empty for the parent's own row); `None` if it is not one.
#[cfg(feature = "upstream")]
fn unescaped(place: &[u8]) -> Option<(Vec<u8>, &[u8])> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < place.len() {
        let b = place[i];
        i += 1;
        if b == 0 {
            let next = *place.get(i)?;
            i += 1;
            if next == 0 {
                return Some((out, &place[i..]));
            }
        }
        out.push(b);
    }
    None
}

/// Each of its input's rows (a parent), placed by [`escaped`], followed by
/// its children: the rows of the plan `make` makes for it, each placed by
/// the parent's escaped place and then its own. Holds per parent its
/// children's operators only (an index of parent place to children: a take
/// among them holds its size and bound, so per-parent limits are a take),
/// never their rows. A parent's children are made again when its row
/// changes. When `flat`, the parents' own rows are left out: a join.
struct Related {
    input: Box<dyn Op>,
    make: Children,
    flat: bool,
    kids: Vec<(Vec<u8>, Box<dyn Op>)>,
}

/// The rows of a parent's children, over `db`, as diffs: each a removal
/// (`gone`) or an insert.
fn children(op: &dyn Op, db: &dyn Read, gone: bool) -> Vec<Diff> {
    let each = fetch(op, db, Bound::Unbounded, false, usize::MAX).into_iter();
    each.map(|(key, v)| Diff {
        key,
        old: gone.then(|| v.clone()),
        new: (!gone).then_some(v),
    })
    .collect()
}

/// Children's diffs placed under their parent's place `at`, onto `out`.
fn under(out: &mut Vec<Diff>, at: &[u8], diffs: Vec<Diff>) {
    for mut d in diffs {
        let mut key = at.to_vec();
        key.append(&mut d.key);
        out.push(Diff { key, ..d });
    }
}

impl Related {
    /// The children's operators for a parent row, built over `db`.
    fn made(&self, db: &dyn Read, p: &[u8], v: &[u8]) -> Box<dyn Op> {
        let mut op = (self.make)(p, v).0;
        op.hydrate(db);
        op
    }

    fn kid(&self, p: &[u8]) -> Option<&dyn Op> {
        let i = self.kids.binary_search_by(|k| k.0[..].cmp(p)).ok()?;
        Some(&*self.kids[i].1)
    }
}

/// Whether `k` is at or past `from` in a fetch's direction.
fn reached(from: Bound<&[u8]>, reverse: bool, k: &[u8]) -> bool {
    match from {
        Bound::Unbounded => true,
        Bound::Included(x) => k == x || (k > x) != reverse,
        Bound::Excluded(x) => k != x && (k > x) != reverse,
    }
}

impl Op for Related {
    fn fetch(&self, db: &dyn Read, from: Bound<&[u8]>, reverse: bool, f: &mut dyn FnMut(&[u8], &[u8]) -> bool) {
        let mut on = true;
        self.input.fetch(db, Bound::Unbounded, reverse, &mut |p, v| {
            let at = escaped(p);
            // A parent wholly before where the fetch starts is passed over.
            if let Bound::Included(x) | Bound::Excluded(x) = from {
                if if reverse { at[..] > *x } else { at[..] < *x && !x.starts_with(&at) } {
                    return true;
                }
            }
            let mut emit = |k: &[u8], v: &[u8]| !reached(from, reverse, k) || f(k, v);
            if !reverse && !self.flat {
                on = emit(&at, v);
            }
            if let (true, Some(kid)) = (on, self.kid(p)) {
                kid.fetch(db, Bound::Unbounded, reverse, &mut |c, cv| {
                    on = emit(&[&at[..], c].concat(), cv);
                    on
                });
            }
            if on && reverse && !self.flat {
                on = emit(&at, v);
            }
            on
        });
    }

    fn push(&mut self, s: &Settle<'_>) -> Vec<Diff> {
        if s.changes.reset {
            self.hydrate(s.db);
            return Vec::new();
        }
        let d = self.input.push(s);
        // The children of the parents that stay take the settle.
        let mut out = Vec::new();
        for (p, op) in &mut self.kids {
            if d.binary_search_by(|x| x.key.cmp(p)).is_err() {
                under(&mut out, &escaped(p), op.push(s));
            }
        }
        // A parent that changed: its children before, over the rows before,
        // and its children made again.
        let mut changed = Vec::new();
        for x in d {
            let at = escaped(&x.key);
            let mut gone = Vec::new();
            if let Ok(i) = self.kids.binary_search_by(|k| k.0.cmp(&x.key)) {
                let (_, op) = self.kids.remove(i);
                gone = children(&*op, s.was, true);
            }
            let mut came = Vec::new();
            if let Some(v) = &x.new {
                let op = self.made(s.db, &x.key, v);
                came = children(&*op, s.db, false);
                let i = self.kids.binary_search_by(|k| k.0.cmp(&x.key)).unwrap_or_else(|i| i);
                self.kids.insert(i, (x.key, op));
            }
            if !self.flat {
                changed.push(Diff {
                    key: at.clone(),
                    old: x.old,
                    new: x.new,
                });
            }
            under(&mut changed, &at, merge(gone, came));
        }
        merge(out, changed)
    }

    fn hydrate(&mut self, db: &dyn Read) {
        self.input.hydrate(db);
        self.kids = Vec::new();
        for (p, v) in fetch(&*self.input, db, Bound::Unbounded, false, usize::MAX) {
            let op = self.made(db, &p, &v);
            self.kids.push((p, op));
        }
    }

    #[cfg(feature = "upstream")]
    fn base(&self, db: &dyn Read, place: &[u8], f: &mut dyn FnMut(TableId, &[u8])) {
        let Some((p, c)) = unescaped(place) else { return };
        // A child is made from its parent: the parent's rows go with it.
        self.input.base(db, &p, f);
        if let (false, Some(kid)) = (c.is_empty(), self.kid(&p)) {
            kid.base(db, c, f);
        }
    }

    #[cfg(any(test, ark_selftest))]
    fn held(&self) -> usize {
        self.input.held() + self.kids.iter().map(|(p, op)| p.len() + op.held()).sum::<usize>()
    }
}

/// The bytes a tree holds: what the tests hold an operator's state to.
#[cfg(any(test, ark_selftest))]
fn held(rows: &Tree) -> usize {
    rows.iter().map(|(k, v)| k.len() + v.as_ref().map_or(0, Vec::len)).sum()
}

/// A query being built: an operator over the ones before it.
pub struct Plan(Box<dyn Op>);

/// A table's rows with keys from `lo` to `hi`: a table, or a range of an
/// index.
pub fn scan(table: TableId, lo: Bound<Vec<u8>>, hi: Bound<Vec<u8>>) -> Plan {
    Plan(Box::new(Scan { table, lo, hi }))
}

/// A table's rows whose keys begin with `prefix`: an index's rows for one
/// value, as a parent's children are.
pub fn prefix(table: TableId, prefix: &[u8]) -> Plan {
    // The first key past every key that begins with it, if one is.
    let mut hi = prefix.to_vec();
    while hi.pop_if(|b| *b == 0xff).is_some() {}
    let hi = match hi.last_mut() {
        Some(b) => {
            *b += 1;
            Bound::Excluded(hi)
        }
        None => Bound::Unbounded,
    };
    scan(table, Bound::Included(prefix.to_vec()), hi)
}

impl Plan {
    /// The rows `keep` keeps.
    pub fn filter(self, keep: impl Fn(&[u8], &[u8]) -> bool + 'static) -> Plan {
        self.each(Box::new(move |_, k, v| keep(k, v).then(|| v.to_vec())), false)
    }

    /// The rows `keep` keeps, reading what it will of the rows given it:
    /// what it read is recorded per row, and runs it again for that row
    /// when it changes.
    pub fn filter_with(self, keep: impl Fn(&dyn Read, &[u8], &[u8]) -> bool + 'static) -> Plan {
        self.each(Box::new(move |db, k, v| keep(db, k, v).then(|| v.to_vec())), true)
    }

    /// Each row's value made another by `make`.
    pub fn map(self, make: impl Fn(&[u8], &[u8]) -> Vec<u8> + 'static) -> Plan {
        self.each(Box::new(move |_, k, v| Some(make(k, v))), false)
    }

    /// Each row's value made another by `make`, reading what it will, as
    /// [`Plan::filter_with`] does.
    pub fn map_with(self, make: impl Fn(&dyn Read, &[u8], &[u8]) -> Vec<u8> + 'static) -> Plan {
        self.each(Box::new(move |db, k, v| Some(make(db, k, v))), true)
    }

    fn each(self, f: Fun<Option<Vec<u8>>>, reads: bool) -> Plan {
        Plan(Box::new(Each {
            input: self.0,
            f,
            reads: reads.then(Reads::default),
        }))
    }

    /// The rows placed by the bytes `by` gives first: bytes whose order is
    /// the sort's, none a prefix of another. Holds its input: where an
    /// index gives the order, scan the index instead.
    pub fn sort(self, by: impl Fn(&[u8], &[u8]) -> Vec<u8> + 'static) -> Plan {
        self.sorted(Box::new(move |_, k, v| by(k, v)), false)
    }

    /// [`Plan::sort`], `by` reading what it will, as [`Plan::filter_with`]
    /// does.
    pub fn sort_with(self, by: impl Fn(&dyn Read, &[u8], &[u8]) -> Vec<u8> + 'static) -> Plan {
        self.sorted(Box::new(by), true)
    }

    fn sorted(self, by: Fun<Vec<u8>>, reads: bool) -> Plan {
        Plan(Box::new(Sort {
            input: self.0,
            by,
            rows: Sparse::default(),
            reads: reads.then(Reads::default),
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
        self.grouped(
            Box::new(move |_, k, v| by(k, v)),
            Box::new(move |_, k, v| of(k, v)),
            extremes,
            prefix,
            false,
        )
    }

    /// [`Plan::group`], `by` and `of` reading what they will, as
    /// [`Plan::filter_with`] does.
    pub fn group_with(
        self,
        by: impl Fn(&dyn Read, &[u8], &[u8]) -> Vec<u8> + 'static,
        of: impl Fn(&dyn Read, &[u8], &[u8]) -> i64 + 'static,
        extremes: bool,
        prefix: bool,
    ) -> Plan {
        self.grouped(Box::new(by), Box::new(of), extremes, prefix, true)
    }

    fn grouped(self, by: Fun<Vec<u8>>, of: Fun<i64>, extremes: bool, prefix: bool, reads: bool) -> Plan {
        Plan(Box::new(Group {
            input: self.0,
            by,
            of,
            extremes,
            prefix,
            groups: Sparse::default(),
            reads: reads.then(Reads::default),
        }))
    }

    /// Each row followed by its children: the rows of the plan `children`
    /// makes from its place and value (often a [`prefix`] of an index, by
    /// its key), each placed after it. A row's place is its own escaped
    /// and ended (so that none is a prefix of another), and a child's is
    /// its parent's and then its own: a row's parent is the nearest row
    /// before it whose place begins its own, at any depth, so an answer is
    /// a nested list in order. Children are not held, only their plans'
    /// state per parent: a `take` among them is a limit per parent. Their
    /// changes come under their parent's place.
    pub fn related(self, children: impl Fn(&[u8], &[u8]) -> Plan + 'static) -> Plan {
        self.relate(Box::new(children), false)
    }

    /// The children [`Plan::related`] finds, without their parents: an
    /// equi-join, each child placed under its parent as there. To pair
    /// values, `children` maps its rows with what it takes of the parent's.
    pub fn join(self, children: impl Fn(&[u8], &[u8]) -> Plan + 'static) -> Plan {
        self.relate(Box::new(children), true)
    }

    fn relate(self, make: Children, flat: bool) -> Plan {
        Plan(Box::new(Related {
            input: self.0,
            make,
            flat,
            kids: Vec::new(),
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
        let diffs = self.settle(db, changes);
        self.apply(diffs)
    }

    /// [`Live::push`], answering what changed of each row by its place: what
    /// an upstream reads to know which rows came into a query or left it.
    #[cfg(feature = "upstream")]
    pub fn diffs(&mut self, db: &dyn Read, changes: &Changes) -> Vec<Diff> {
        let diffs = self.settle(db, changes);
        self.apply(diffs.clone());
        diffs
    }

    /// The base rows the answer is made of, and the rows its closures read
    /// for it, each once, in table and key order, as `db` holds them: what
    /// an upstream sends a node subscribed to its query.
    #[cfg(feature = "upstream")]
    pub fn base(&self, db: &dyn Read) -> Vec<(TableId, Vec<u8>)> {
        let mut out = Vec::new();
        for (k, _) in &self.rows {
            self.op.base(db, k, &mut |t, key| out.push((t, key.to_vec())));
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    fn settle(&mut self, db: &dyn Read, changes: &Changes) -> Vec<Diff> {
        // The rows as they were: what changed, put back over the rows now.
        let mut layer = Layer::new();
        for (t, d) in &changes.rows {
            layer.keep(*t, &d.key, || d.old.clone());
        }
        let was = View { base: db, layer: &layer };
        let s = Settle { db, was: &was, changes };
        if changes.reset {
            self.op.push(&s);
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
            self.op.push(&s)
        }
    }

    /// The diffs made to the answer, as patches.
    fn apply(&mut self, diffs: Vec<Diff>) -> Vec<Patch> {
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

    /// The first byte of the first row of the other table at or after
    /// `from`, or 7: a read of a range, stopped early.
    fn first(db: &dyn Read, from: &[u8]) -> u8 {
        let mut out = 7;
        let range = Range {
            lo: Bound::Included(from),
            hi: Bound::Unbounded,
            reverse: false,
        };
        db.scan(U, range, &mut |_, u| {
            out = u[0];
            false
        });
        out
    }

    /// A random plan, the same for the same seed: a scan of a range, then
    /// up to four of filter, map, sort, take and group, in any order, and
    /// their forms whose closures read the other table.
    fn plan(seed: u64) -> Plan {
        let mut rng = Rng::new(seed);
        let (lo, hi) = (key(&mut rng), key(&mut rng));
        let mut p = scan(T, bound(&mut rng, lo), bound(&mut rng, hi));
        for _ in 0..rng.below(5) {
            p = match rng.below(14) {
                0 => {
                    let m = 2 + rng.below(3) as u8;
                    p.filter(move |_, v| v[0] % m != 0)
                }
                1 => p.map(|k, v| vec![v[0] / 2, k[0]]),
                2 => p.sort(|_, v| vec![v[0] % 4]),
                3 | 4 => p.take(rng.below(5) as usize, rng.below(2) == 0),
                5 => p.group(|k, _| vec![k[0]], |_, v| i64::from(v[0]) - 3, rng.below(2) == 0, true),
                6 => p.group(|_, v| vec![v[0] % 3], |_, v| i64::from(v[0]), true, false),
                // Closures that read the other table: a key, and a scan
                // stopped at its first row.
                7 => p.filter_with(|db, k, _| db.get(U, &k[..1]).is_none_or(|u| u[0] % 2 == 0)),
                8 => p.map_with(|db, k, v| vec![v[0], first(db, &k[..1])]),
                9 => p.sort_with(|db, _, v| vec![first(db, &[v[0] % 6]) % 3]),
                // Each row with its children of the other table, by its
                // first byte: the first two, or those whose parent's first
                // byte has a row (read); or the children alone, paired.
                11 => p.related(|k, _| prefix(U, &k[..1]).take(2, false)),
                12 => p.related(|k, _| prefix(U, &k[..1]).filter_with(|db, k, _| db.get(T, &k[..1]).is_some())),
                13 => p.join(|k, v| {
                    let v0 = v[0];
                    prefix(U, &k[..1]).map(move |_, c| vec![c[0], v0])
                }),
                _ => p.group_with(
                    |db, k, _| vec![db.get(U, &k[..1]).map_or(9, |u| u[0] % 2)],
                    |db, _, v| i64::from(v[0]) + i64::from(first(db, &[])),
                    true,
                    false,
                ),
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
        let seeds = option_env!("ARK_PLANS").and_then(|n| n.parse().ok()).unwrap_or(150);
        for seed in 0..seeds {
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
