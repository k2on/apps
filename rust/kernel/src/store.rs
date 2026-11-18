//! Rows, as a host keeps them and a node reads them: the [`Store`] trait
//! (the confirmed rows, which a host keeps), an in-memory store
//! ([`Memory`]), an [`Overlay`] of a step's writes over any rows, and the
//! [`Versioned`] store a node keeps each space in.
//!
//! A row is its key's bytes and its value's bytes in a table, the table
//! named by a number ([`TableId`]). Keys are ordered by their bytes, so a
//! range of keys is a range scan, ascending or descending, and a scan is
//! resumed by starting it past the last key read. On a watch the store is
//! [`Memory`]; on a phone, a desktop and the server it is SQLite behind
//! the same trait.
//!
//! A step's writes go into an [`Overlay`] over its space's rows, which
//! reads as the rows with the writes made, and become [`Writes`] that are
//! applied at once or not at all. A host's store is only ever given
//! confirmed writes, whole ([`Store::apply`]); every other version of a row
//! is the node's, in one [`Versioned`] store per space: the confirmed rows
//! (the host's store), what the pending entries wrote over them, the rows a
//! fill or a subscription holds apart until they are all here, and each
//! row's value before the settle under way, which is how its changes are
//! told and what plans read as the rows were ([`Versioned::before`]).

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::ops::Bound;

use crate::cell::Sources;
use crate::codec::{Decode, Encode, Error, Reader};

/// A table, by the number its build gives it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TableId(pub u32);

/// What a range scan reads: the keys between `lo` and `hi`, ascending, or
/// descending when `reverse`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range<'a> {
    pub lo: Bound<&'a [u8]>,
    pub hi: Bound<&'a [u8]>,
    pub reverse: bool,
}

impl<'a> Range<'a> {
    /// Every key, ascending.
    pub fn all() -> Range<'a> {
        Range {
            lo: Bound::Unbounded,
            hi: Bound::Unbounded,
            reverse: false,
        }
    }

    /// The same keys, read the other way.
    pub fn rev(self) -> Range<'a> {
        Range {
            reverse: !self.reverse,
            ..self
        }
    }

    /// Whether no key can be in it, which a `BTreeMap` range refuses.
    fn is_empty(&self) -> bool {
        match (self.lo, self.hi) {
            (Bound::Included(a), Bound::Included(b)) => a > b,
            (Bound::Included(a), Bound::Excluded(b)) | (Bound::Excluded(a), Bound::Included(b)) | (Bound::Excluded(a), Bound::Excluded(b)) => a >= b,
            _ => false,
        }
    }

    /// Whether `a` comes before `b` in the order this range is read.
    fn before(&self, a: &[u8], b: &[u8]) -> bool {
        if self.reverse {
            a > b
        } else {
            a < b
        }
    }
}

/// Reading rows.
pub trait Read {
    /// The row at `key`.
    fn get(&self, table: TableId, key: &[u8]) -> Option<Vec<u8>>;

    /// Calls `f` with each row in `range`, in its order, until `f` returns
    /// false.
    fn scan(&self, table: TableId, range: Range<'_>, f: &mut dyn FnMut(&[u8], &[u8]) -> bool);

    /// The node's cells and watches, if it reads them, and whether as
    /// they were before the settle under way: what [`crate::Cell::get`]
    /// reads `id` from. A store holds none.
    fn sources(&self, _id: u32) -> Option<(&Sources, bool)> {
        None
    }

    /// The node's sources as a reader shares them, unrecorded, and where
    /// its reads are recorded: what [`crate::source`] keys its watches and
    /// shared results in. A store shares none.
    fn shared(&self) -> Option<crate::cell::Shared<'_>> {
        None
    }
}

/// The confirmed rows, as a host keeps them: given only whole batches of
/// confirmed writes, or emptied by a fill. What is not confirmed (pending
/// entries' writes, rows held apart, a row's value before a settle) is
/// the node's ([`Versioned`]), never the host's.
pub trait Store: Read {
    /// Every table it holds a row of.
    fn tables(&self) -> Vec<TableId>;

    /// Makes every write, in order, at once.
    fn apply(&mut self, writes: &Writes);

    /// Takes every row out: what a whole fill does before its rows.
    fn clear(&mut self);
}

/// One write: a row's value at a key, or `None` for its removal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Write {
    pub table: TableId,
    pub key: Vec<u8>,
    pub value: Option<Vec<u8>>,
}

/// What a step wrote, in table and key order: what a store applies at
/// once, and what the server sends as an entry's facts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Writes(pub Vec<Write>);

/// A row's place in one ordered keyspace: its table's number, four bytes
/// big-endian so that each table's rows are together, then its key.
fn path(table: TableId, key: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(4 + key.len());
    p.extend_from_slice(&table.0.to_be_bytes());
    p.extend_from_slice(key);
    p
}

/// A range of paths, from and to.
type Paths = (Bound<Vec<u8>>, Bound<Vec<u8>>);

/// The paths a range of a table's keys covers, or `None` when no key can
/// be in it.
fn paths(table: TableId, range: Range<'_>) -> Option<Paths> {
    if range.is_empty() {
        return None;
    }
    let lo = match range.lo {
        Bound::Included(k) => Bound::Included(path(table, k)),
        Bound::Excluded(k) => Bound::Excluded(path(table, k)),
        Bound::Unbounded => Bound::Included(path(table, &[])),
    };
    let hi = match range.hi {
        Bound::Included(k) => Bound::Included(path(table, k)),
        Bound::Excluded(k) => Bound::Excluded(path(table, k)),
        Bound::Unbounded => match table.0.checked_add(1) {
            Some(next) => Bound::Excluded(path(TableId(next), &[])),
            None => Bound::Unbounded,
        },
    };
    Some((lo, hi))
}

/// Rows by their bytes, each a value or `None` for a removal: the one
/// B-tree the kernel links, which holds a store's rows, a layer's writes
/// and an operator's state alike, so that its code is linked once.
pub(crate) type Tree = BTreeMap<Vec<u8>, Option<Vec<u8>>>;

/// A tree rows are put in and taken out of. A row taken out is left as
/// `None` (a B-tree's removal links its rebalancing, the most code it has),
/// and the tree is built again without them once they are half of it.
#[derive(Clone, Debug, Default)]
pub(crate) struct Sparse {
    pub(crate) tree: Tree,
    dead: usize,
}

impl Sparse {
    pub(crate) fn get(&self, k: &Vec<u8>) -> Option<&Vec<u8>> {
        self.tree.get(k)?.as_ref()
    }

    pub(crate) fn put(&mut self, k: Vec<u8>, v: Vec<u8>) {
        if let Some(None) = self.tree.insert(k, Some(v)) {
            self.dead -= 1;
        }
    }

    pub(crate) fn take(&mut self, k: &Vec<u8>) {
        let Some(v @ Some(_)) = self.tree.get_mut(k) else { return };
        *v = None;
        self.dead += 1;
        if self.dead * 2 > self.tree.len() {
            let mut tree = Tree::new();
            for (k, v) in core::mem::take(&mut self.tree) {
                if v.is_some() {
                    tree.insert(k, v);
                }
            }
            *self = Sparse { tree, dead: 0 };
        }
    }
}

/// Two are equal when they hold the same rows, whatever they took out.
impl PartialEq for Sparse {
    fn eq(&self, other: &Sparse) -> bool {
        let live = |t: &Tree| {
            t.iter()
                .filter(|(_, v)| v.is_some())
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<Vec<_>>()
        };
        live(&self.tree) == live(&other.tree)
    }
}

impl Eq for Sparse {}

/// Calls `f` with each of `rows` from `lo` to `hi`, in order or reversed,
/// until `f` returns false.
pub(crate) fn walk<'r>(rows: &'r Tree, lo: Bound<&[u8]>, hi: Bound<&[u8]>, reverse: bool, f: &mut dyn FnMut(&'r [u8], &'r Option<Vec<u8>>) -> bool) {
    let mut it = rows.range::<[u8], _>((lo, hi));
    let mut g = |(k, v): (&'r Vec<u8>, &'r Option<Vec<u8>>)| f(k, v);
    if reverse {
        it.rev().all(&mut g);
    } else {
        it.all(&mut g);
    }
}

/// Calls `f` with each of `rows` in a table's `range`, its key without the
/// table, in the range's order, until `f` returns false.
fn each<'r>(rows: &'r Tree, table: TableId, range: Range<'_>, f: &mut dyn FnMut(&'r [u8], &'r Option<Vec<u8>>) -> bool) {
    if let Some((lo, hi)) = paths(table, range) {
        walk(
            rows,
            lo.as_ref().map(Vec::as_slice),
            hi.as_ref().map(Vec::as_slice),
            range.reverse,
            &mut |k, v| f(&k[4..], v),
        );
    }
}

/// Rows in memory: one B-tree, every table's rows in it by `path`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Memory {
    rows: Sparse,
}

impl Memory {
    pub fn new() -> Memory {
        Memory::default()
    }

    /// How many rows the table holds.
    pub fn len(&self, table: TableId) -> usize {
        let mut n = 0;
        self.scan(table, Range::all(), &mut |_, _| {
            n += 1;
            true
        });
        n
    }
}

impl Read for Memory {
    fn get(&self, table: TableId, key: &[u8]) -> Option<Vec<u8>> {
        self.rows.get(&path(table, key)).cloned()
    }

    fn scan(&self, table: TableId, range: Range<'_>, f: &mut dyn FnMut(&[u8], &[u8]) -> bool) {
        each(&self.rows.tree, table, range, &mut |k, v| v.as_deref().is_none_or(|v| f(k, v)));
    }
}

/// The tables `rows` holds a path of, each found by seeking past the last.
fn tables(rows: &Tree) -> Vec<TableId> {
    let mut out: Vec<TableId> = Vec::new();
    loop {
        let from = out.last().map(|t| t.0.checked_add(1));
        let Some(from) = from.unwrap_or(Some(0)) else { break };
        let mut next = None;
        walk(rows, Bound::Included(&from.to_be_bytes()[..]), Bound::Unbounded, false, &mut |k, _| {
            next = Some(TableId(u32::from_be_bytes([k[0], k[1], k[2], k[3]])));
            false
        });
        match next {
            Some(t) => out.push(t),
            None => break,
        }
    }
    out
}

impl Memory {
    pub fn put(&mut self, table: TableId, key: &[u8], value: &[u8]) {
        self.rows.put(path(table, key), value.to_vec());
    }

    pub fn delete(&mut self, table: TableId, key: &[u8]) {
        self.rows.take(&path(table, key));
    }
}

impl Store for Memory {
    fn tables(&self) -> Vec<TableId> {
        tables(&self.rows.tree)
    }

    fn apply(&mut self, writes: &Writes) {
        for w in &writes.0 {
            match &w.value {
                Some(v) => self.put(w.table, &w.key, v),
                None => self.delete(w.table, &w.key),
            }
        }
    }

    fn clear(&mut self) {
        self.rows = Sparse::default();
    }
}

/// Writes kept by `path`: each row's new value, or `None` for its
/// removal. What an [`Overlay`] holds, and what a node's pending entries
/// have written over its confirmed rows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Layer(Tree);

/// A layer with no writes, to view a store as it is.
pub(crate) static EMPTY: Layer = Layer(BTreeMap::new());

impl Layer {
    pub fn new() -> Layer {
        Layer::default()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Makes every write over what the layer holds.
    pub fn apply(&mut self, writes: &Writes) {
        for w in &writes.0 {
            self.0.insert(path(w.table, &w.key), w.value.clone());
        }
    }

    /// Holds `value` for the row, unless the layer holds one for it.
    pub(crate) fn keep(&mut self, table: TableId, key: &[u8], value: impl FnOnce() -> Option<Vec<u8>>) {
        if let alloc::collections::btree_map::Entry::Vacant(v) = self.0.entry(path(table, key)) {
            v.insert(value());
        }
    }

    /// Makes `w` here if the layer holds its row: whether it did.
    pub(crate) fn update(&mut self, w: &Write) -> bool {
        let Some(v) = self.0.get_mut(&path(w.table, &w.key)) else { return false };
        v.clone_from(&w.value);
        true
    }

    /// The rows the layer has written, in table and key order.
    pub(crate) fn keys(&self) -> impl Iterator<Item = (TableId, &[u8])> {
        self.0.keys().map(|p| (TableId(u32::from_be_bytes([p[0], p[1], p[2], p[3]])), &p[4..]))
    }

    /// The writes held, in table and key order.
    pub fn into_writes(self) -> Writes {
        let mut out = Vec::with_capacity(self.0.len());
        for (mut key, value) in self.0 {
            let table = TableId(u32::from_be_bytes([key[0], key[1], key[2], key[3]]));
            key.drain(..4);
            out.push(Write { table, key, value });
        }
        Writes(out)
    }
}

/// A store read with a layer's writes made, changing neither.
pub struct View<'a> {
    pub base: &'a dyn Read,
    pub layer: &'a Layer,
}

/// No rows: the `local` space's, whose state is cells.
pub(crate) struct Nothing;

impl Read for Nothing {
    fn get(&self, _: TableId, _: &[u8]) -> Option<Vec<u8>> {
        None
    }

    fn scan(&self, _: TableId, _: Range<'_>, _: &mut dyn FnMut(&[u8], &[u8]) -> bool) {}
}

impl Read for View<'_> {
    fn sources(&self, id: u32) -> Option<(&Sources, bool)> {
        self.base.sources(id)
    }

    fn get(&self, table: TableId, key: &[u8]) -> Option<Vec<u8>> {
        match self.layer.0.get(&path(table, key)) {
            Some(w) => w.clone(),
            None => self.base.get(table, key),
        }
    }

    fn scan(&self, table: TableId, range: Range<'_>, f: &mut dyn FnMut(&[u8], &[u8]) -> bool) {
        // The layer's writes in the range, in its order, merged into the
        // store's.
        let mut held: Vec<(&[u8], Option<&[u8]>)> = Vec::new();
        each(&self.layer.0, table, range, &mut |k, v| {
            held.push((k, v.as_deref()));
            true
        });
        if held.is_empty() {
            return self.base.scan(table, range, f);
        }
        let mut i = 0;
        let mut open = true;
        self.base.scan(table, range, &mut |k, v| {
            while let Some(&(hk, hv)) = held.get(i) {
                if !range.before(hk, k) {
                    break;
                }
                i += 1;
                if let Some(hv) = hv {
                    if !f(hk, hv) {
                        open = false;
                        return false;
                    }
                }
            }
            let row = match held.get(i) {
                Some(&(hk, hv)) if hk == k => {
                    i += 1;
                    hv
                }
                _ => Some(v),
            };
            open = row.is_none_or(|v| f(k, v));
            open
        });
        if open {
            for &(hk, hv) in &held[i..] {
                if let Some(hv) = hv {
                    if !f(hk, hv) {
                        return;
                    }
                }
            }
        }
    }
}

/// Writes held over a store, which read as the store with them made: a
/// step's transaction, kept or dropped whole.
pub struct Overlay<'a> {
    base: &'a dyn Read,
    layer: Layer,
}

impl<'a> Overlay<'a> {
    pub fn new(base: &'a dyn Read) -> Overlay<'a> {
        Overlay { base, layer: Layer::new() }
    }

    /// The writes held, for the store to apply.
    pub fn into_writes(self) -> Writes {
        self.layer.into_writes()
    }

    fn view(&self) -> View<'_> {
        View {
            base: self.base,
            layer: &self.layer,
        }
    }
}

impl Read for Overlay<'_> {
    fn get(&self, table: TableId, key: &[u8]) -> Option<Vec<u8>> {
        self.view().get(table, key)
    }

    fn scan(&self, table: TableId, range: Range<'_>, f: &mut dyn FnMut(&[u8], &[u8]) -> bool) {
        self.view().scan(table, range, f)
    }
}

/// What a handler reads and writes (`Cx::db`): its step's transaction,
/// kept whole or dropped whole.
pub trait Txn: Read {
    fn put(&mut self, table: TableId, key: &[u8], value: &[u8]);

    fn delete(&mut self, table: TableId, key: &[u8]);
}

impl Txn for Overlay<'_> {
    fn put(&mut self, table: TableId, key: &[u8], value: &[u8]) {
        self.layer.0.insert(path(table, key), Some(value.to_vec()));
    }

    fn delete(&mut self, table: TableId, key: &[u8]) {
        self.layer.0.insert(path(table, key), None);
    }
}

/// A space's rows as a node keeps them, every version of each in one
/// store: the confirmed rows (the host's [`Store`]), what the pending
/// entries wrote over them (shared only), the rows held apart until all of
/// a fill or a subscription is here, and each row's value before the
/// settle under way, kept the first time anything writes it. Every write
/// goes through it, so its changes are known without anyone noting them
/// ([`Versioned::take_changes`]), and the rows as they were are read
/// from it ([`Versioned::before`]): what plans read in a settle.
pub struct Versioned<S> {
    base: S,
    pending: Layer,
    held: Layer,
    before: Layer,
    reset: bool,
}

impl<S: Store> Versioned<S> {
    pub fn new(base: S) -> Versioned<S> {
        Versioned {
            base,
            pending: Layer::new(),
            held: Layer::new(),
            before: Layer::new(),
            reset: false,
        }
    }

    /// The confirmed rows.
    pub fn base(&self) -> &S {
        &self.base
    }

    /// The rows as they read now: the confirmed, the pending writes over
    /// them.
    pub fn now(&self) -> View<'_> {
        View {
            base: &self.base,
            layer: &self.pending,
        }
    }

    /// The rows as they read before the settle under way: each row written
    /// since its changes were last taken as it was then.
    pub fn before(&self) -> Before<'_> {
        Before {
            now: self.now(),
            before: &self.before,
        }
    }

    /// Confirmed writes, made in the host's store.
    pub(crate) fn confirm(&mut self, writes: &Writes) {
        keep(
            &self.base,
            &self.pending,
            &mut self.before,
            &mut writes.0.iter().map(|w| (w.table, &w.key[..])),
        );
        self.base.apply(writes);
    }

    /// A pending entry's writes, over the rest.
    pub(crate) fn propose(&mut self, writes: &Writes) {
        keep(
            &self.base,
            &self.pending,
            &mut self.before,
            &mut writes.0.iter().map(|w| (w.table, &w.key[..])),
        );
        self.pending.apply(writes);
    }

    /// The pending writes made again whole (a rebase), or none.
    pub(crate) fn repend(&mut self, pending: Layer) {
        keep(
            &self.base,
            &self.pending,
            &mut self.before,
            &mut self.pending.keys().chain(pending.keys()),
        );
        self.pending = pending;
    }

    /// The pending writes made again whole, as a node put them back from
    /// its records: no change to tell, the node's rows being told whole.
    pub(crate) fn resume(&mut self, pending: Layer) {
        self.pending = pending;
        self.reset = true;
    }

    /// Every confirmed row taken out (a whole fill): its changes are told
    /// as a reset.
    pub(crate) fn clear(&mut self) {
        self.base.clear();
        self.reset();
    }

    /// Its rows changed in ways told whole: a fill's.
    pub(crate) fn reset(&mut self) {
        self.before = Layer::new();
        self.reset = true;
    }

    /// Rows to hold apart until all of what they are part of is here.
    pub(crate) fn hold(&mut self, rows: &Writes) {
        self.held.apply(rows);
    }

    /// Makes `w` among the rows held, if they hold its row: whether they did.
    pub(crate) fn update_held(&mut self, w: &Write) -> bool {
        self.held.update(w)
    }

    /// Whether no rows are held apart.
    pub(crate) fn holds_none(&self) -> bool {
        self.held.is_empty()
    }

    /// The rows held apart, taken out.
    pub(crate) fn take_held(&mut self) -> Writes {
        core::mem::take(&mut self.held).into_writes()
    }

    /// What changed since this was last called: each row written as it was
    /// and as it reads now, or a reset when its rows changed whole.
    pub fn take_changes(&mut self) -> crate::plan::Changes {
        let changes = self.changes();
        self.told();
        changes
    }

    /// The changes told: the rows now are what the next are told from.
    pub(crate) fn told(&mut self) {
        self.before = Layer::new();
        self.reset = false;
    }

    /// What changed since the changes were last taken, kept.
    pub fn changes(&self) -> crate::plan::Changes {
        let mut rows = Vec::new();
        if !self.reset {
            let now = self.now();
            for (table, key) in self.before.keys() {
                let old = self.before.0.get(&path(table, key)).cloned().flatten();
                let new = now.get(table, key);
                if old != new {
                    rows.push((table, crate::plan::Diff { key: key.to_vec(), old, new }));
                }
            }
        }
        crate::plan::Changes { reset: self.reset, rows }
    }
}

/// Keeps what each of `keys` reads now (`base` with `pending` over it) in
/// `before`, unless kept since the changes were last taken: one copy
/// linked, whatever the keys come from.
#[inline(never)]
fn keep(base: &dyn Read, pending: &Layer, before: &mut Layer, keys: &mut dyn Iterator<Item = (TableId, &[u8])>) {
    let now = View { base, layer: pending };
    for (table, key) in keys {
        before.keep(table, key, || now.get(table, key));
    }
}

/// The rows as they read now.
impl<S: Store> Read for Versioned<S> {
    fn get(&self, table: TableId, key: &[u8]) -> Option<Vec<u8>> {
        self.now().get(table, key)
    }

    fn scan(&self, table: TableId, range: Range<'_>, f: &mut dyn FnMut(&[u8], &[u8]) -> bool) {
        self.now().scan(table, range, f)
    }
}

/// A space's rows as they read before the settle under way: each row
/// written since its changes were last taken as it was then, over the rows
/// now.
pub struct Before<'a> {
    now: View<'a>,
    before: &'a Layer,
}

impl Read for Before<'_> {
    fn get(&self, table: TableId, key: &[u8]) -> Option<Vec<u8>> {
        View {
            base: &self.now,
            layer: self.before,
        }
        .get(table, key)
    }

    fn scan(&self, table: TableId, range: Range<'_>, f: &mut dyn FnMut(&[u8], &[u8]) -> bool) {
        View {
            base: &self.now,
            layer: self.before,
        }
        .scan(table, range, f)
    }
}

impl Encode for TableId {
    fn encode(&self, out: &mut Vec<u8>) {
        self.0.encode(out);
    }
}

impl Decode for TableId {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        Ok(TableId(u32::decode(r)?))
    }
}

impl Encode for Write {
    fn encode(&self, out: &mut Vec<u8>) {
        self.table.encode(out);
        self.key.encode(out);
        self.value.encode(out);
    }
}

impl Decode for Write {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        Ok(Write {
            table: TableId::decode(r)?,
            key: Vec::decode(r)?,
            value: Option::decode(r)?,
        })
    }
}

impl Encode for Writes {
    fn encode(&self, out: &mut Vec<u8>) {
        self.0.encode(out);
    }
}

impl Decode for Writes {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        Ok(Writes(Vec::decode(r)?))
    }
}

#[cfg(any(test, ark_selftest))]
pub(crate) mod tests {
    use super::*;
    use crate::codec::{from_slice, to_vec};
    use crate::Rng;
    use alloc::vec;

    const T: TableId = TableId(1);

    fn read(s: &dyn Read, table: TableId, range: Range<'_>) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut out = Vec::new();
        s.scan(table, range, &mut |k, v| {
            out.push((k.to_vec(), v.to_vec()));
            true
        });
        out
    }

    fn keys(s: &dyn Read, range: Range<'_>) -> Vec<u8> {
        read(s, T, range).into_iter().map(|(k, _)| k[0]).collect()
    }

    #[cfg_attr(test, test)]
    fn memory_scans_ranges() {
        let mut m = Memory::new();
        for k in [5u8, 1, 3, 9, 7] {
            m.put(T, &[k], &[k * 10]);
        }
        m.put(TableId(2), &[4], &[0]);
        assert_eq!(m.get(T, &[3]), Some(vec![30]));
        assert_eq!(keys(&m, Range::all()), [1, 3, 5, 7, 9]);
        assert_eq!(keys(&m, Range::all().rev()), [9, 7, 5, 3, 1]);
        let r = Range {
            lo: Bound::Excluded(&[3][..]),
            hi: Bound::Included(&[7][..]),
            reverse: false,
        };
        assert_eq!(keys(&m, r), [5, 7]);
        assert_eq!(keys(&m, r.rev()), [7, 5]);
        // Ranges no key can be in read nothing, rather than panic.
        let empty = Range {
            lo: Bound::Excluded(&[3][..]),
            hi: Bound::Excluded(&[3][..]),
            reverse: false,
        };
        assert_eq!(keys(&m, empty), []);
        let backwards = Range {
            lo: Bound::Included(&[7][..]),
            hi: Bound::Included(&[3][..]),
            reverse: false,
        };
        assert_eq!(keys(&m, backwards), []);
        // A scan stops when told, and resumes past the last key read.
        let mut first = Vec::new();
        m.scan(T, Range::all(), &mut |k, _| {
            first.push(k[0]);
            first.len() < 2
        });
        assert_eq!(first, [1, 3]);
        let rest = Range {
            lo: Bound::Excluded(&[3][..]),
            ..Range::all()
        };
        assert_eq!(keys(&m, rest), [5, 7, 9]);
        m.delete(T, &[5]);
        m.delete(T, &[6]);
        assert_eq!(m.len(T), 4);
        assert_eq!(m.get(T, &[5]), None);
    }

    #[cfg_attr(test, test)]
    fn an_overlay_reads_through() {
        let mut m = Memory::new();
        for k in [1u8, 3, 5] {
            m.put(T, &[k], &[k]);
        }
        let mut o = Overlay::new(&m);
        o.put(T, &[2], &[20]);
        o.put(T, &[3], &[30]);
        o.delete(T, &[5]);
        o.put(T, &[6], &[60]);
        assert_eq!(o.get(T, &[3]), Some(vec![30]));
        assert_eq!(o.get(T, &[5]), None);
        assert_eq!(o.get(T, &[1]), Some(vec![1]));
        assert_eq!(
            read(&o, T, Range::all()),
            [(vec![1], vec![1]), (vec![2], vec![20]), (vec![3], vec![30]), (vec![6], vec![60])]
        );
        assert_eq!(keys(&o, Range::all().rev()), [6, 3, 2, 1]);
        let w = o.into_writes();
        assert_eq!(w.0.len(), 4);
        // The store is untouched until the writes are applied.
        assert_eq!(keys(&m, Range::all()), [1, 3, 5]);
        m.apply(&w);
        assert_eq!(keys(&m, Range::all()), [1, 2, 3, 6]);
        assert_eq!(from_slice::<Writes>(&to_vec(&w)), Ok(w));
    }

    /// Random writes into an overlay and the same into a copy of its
    /// store: every range reads the same from both, both ways, and the
    /// overlay's writes applied make the copy.
    #[cfg_attr(test, test)]
    fn an_overlay_is_its_writes_made() {
        let mut rng = Rng::new(3);
        for _ in 0..100 {
            let mut base = Memory::new();
            for _ in 0..rng.below(12) {
                base.put(T, &[rng.below(16) as u8], &[rng.next() as u8]);
            }
            let mut copy = base.clone();
            let mut o = Overlay::new(&base);
            for _ in 0..rng.below(12) {
                let k = [rng.below(16) as u8];
                if rng.below(3) == 0 {
                    o.delete(T, &k);
                    copy.delete(T, &k);
                } else {
                    let v = [rng.next() as u8];
                    o.put(T, &k, &v);
                    copy.put(T, &k, &v);
                }
            }
            for _ in 0..8 {
                let (a, b) = ([rng.below(17) as u8], [rng.below(17) as u8]);
                let bound = |rng: &mut Rng, k| match rng.below(3) {
                    0 => Bound::Included(k),
                    1 => Bound::Excluded(k),
                    _ => Bound::Unbounded,
                };
                let r = Range {
                    lo: bound(&mut rng, &a[..]),
                    hi: bound(&mut rng, &b[..]),
                    reverse: rng.below(2) == 0,
                };
                assert_eq!(read(&o, T, r), read(&copy, T, r));
                // Stopped early, it has read the same first rows.
                let stop = rng.below(4) as usize;
                let mut seen = Vec::new();
                o.scan(T, r, &mut |k, _| {
                    seen.push(k[0]);
                    seen.len() <= stop
                });
                let all: Vec<u8> = read(&copy, T, r).into_iter().map(|(k, _)| k[0]).collect();
                assert_eq!(seen[..], all[..all.len().min(stop + 1)]);
            }
            let writes = o.into_writes();
            base.apply(&writes);
            assert_eq!(base, copy);
        }
    }

    /// Rows taken out are left in the tree until they are half of it, then
    /// it is built again without them: a store that churns stays the size
    /// of what it holds.
    #[cfg_attr(test, test)]
    fn rows_taken_out_are_let_go() {
        let mut m = Memory::new();
        for round in 0..50u8 {
            for k in 0..20u8 {
                m.put(T, &[k], &[round]);
            }
            for k in 0..20u8 {
                if (k + round) % 3 != 0 {
                    m.delete(T, &[k]);
                }
            }
            assert!(m.rows.tree.len() <= 2 * m.len(T) + 1, "{} for {}", m.rows.tree.len(), m.len(T));
            assert_eq!(m.get(T, &[round % 3]).is_some(), (round % 3 + round) % 3 == 0);
        }
        let mut again = Memory::new();
        for (k, v) in read(&m, T, Range::all()) {
            again.put(T, &k, &v);
        }
        assert_eq!(m, again);
    }

    /// One store keeps every version of a row: the confirmed, the pending
    /// over it, those held apart, and what each read before, from which
    /// its changes are told and the rows as they were are read.
    #[cfg_attr(test, test)]
    fn a_versioned_store_tells_what_changed() {
        let w = |k: u8, v: Option<u8>| Write {
            table: TableId(1),
            key: alloc::vec![k],
            value: v.map(|v| alloc::vec![v]),
        };
        let mut rows = Versioned::new(Memory::new());
        rows.confirm(&Writes(alloc::vec![w(1, Some(1)), w(2, Some(2))]));
        rows.take_changes();
        rows.propose(&Writes(alloc::vec![w(1, Some(10))]));
        rows.confirm(&Writes(alloc::vec![w(2, None)]));
        rows.hold(&Writes(alloc::vec![w(3, Some(3))]));
        assert_eq!(rows.now().get(TableId(1), &[1]), Some(alloc::vec![10]));
        assert_eq!(rows.base().get(TableId(1), &[1]), Some(alloc::vec![1]));
        // Held apart: not read until taken.
        assert_eq!(rows.get(TableId(1), &[3]), None);
        // As it was: before the proposal and the removal.
        let before = rows.before();
        assert_eq!(
            read(&before, TableId(1), Range::all()),
            [(alloc::vec![1], alloc::vec![1]), (alloc::vec![2], alloc::vec![2])]
        );
        let changes = rows.take_changes();
        assert_eq!(
            changes
                .rows
                .iter()
                .map(|(_, d)| (d.key[0], d.old.clone(), d.new.clone()))
                .collect::<Vec<_>>(),
            [(1, Some(alloc::vec![1]), Some(alloc::vec![10])), (2, Some(alloc::vec![2]), None)]
        );
        // A rebase that writes what was pending tells nothing.
        let mut same = Layer::new();
        same.apply(&Writes(alloc::vec![w(1, Some(10))]));
        rows.repend(same);
        assert_eq!(rows.take_changes(), crate::plan::Changes::default());
        let held = rows.take_held();
        rows.confirm(&held);
        rows.clear();
        assert!(rows.take_changes().reset);
    }

    crate::listed!(
        memory_scans_ranges,
        an_overlay_reads_through,
        an_overlay_is_its_writes_made,
        rows_taken_out_are_let_go,
        a_versioned_store_tells_what_changed
    );
}
