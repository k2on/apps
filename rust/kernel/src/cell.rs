//! Cells and watches: the single values and live answers a node keeps
//! beside its rows, the readers that record what they read of them, and
//! the [`Sink`] a settle tells.
//!
//! - A **cell** ([`Cell<T>`]) is a single value that never travels: a
//!   view's state, a flag, a selection. It is never encoded; the `local`
//!   space is cells. A write is a pointer swap and a version bump, and an
//!   equal write changes nothing.
//! - A **watch** ([`Watch`]) is a plan's [`Live`] answer, kept by the node
//!   by its query and arguments, one per pair, and pushed each settle.
//! - A **reader** ([`Reader`]) is one of a sink's leaves, a binding: a
//!   thunk whose every read of a cell, a row or a watch, through whatever
//!   functions it calls, is recorded as it pulls ([`crate::Node::read`]).
//!   The settle tells its sink when any of it changed, and the sink cuts
//!   it off there if what it pulls is equal to what it shows. A value
//!   derived from cells and rows is a plain function a reader calls; there
//!   is no memo.
//!
//! Cells and watches are numbered in one space, and every read of either
//! is recorded the way a plan's closures record rows: a range of keys of a
//! table, the sources' table being [`SOURCES`], its keys their numbers. So
//! a plan's filter that reads a cell runs again for exactly the rows that
//! read it, with no mechanism of its own, and one test of a read set
//! against what changed serves plans and readers alike.

use alloc::vec::Vec;
use core::marker::PhantomData;

use crate::entry::Space;
use crate::plan::{spans, within, Changes, Live, Patch};
use crate::slot::{Boxed, Slots};
use crate::store::{Range, Read, TableId};

/// The table cells and watches are read in, as a read set records them:
/// each one's key is its number, in four bytes.
pub const SOURCES: TableId = TableId(u32::MAX);

/// A cell: a typed handle to a value the node holds.
pub struct Cell<T>(pub(crate) u32, PhantomData<fn() -> T>);

/// A plan's answer, kept live by the node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Watch(pub(crate) u32);

/// A sink's leaf: what it read is recorded, and it is told when that
/// changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Reader(pub u32);

impl<T> Clone for Cell<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Cell<T> {}

impl<T> PartialEq for Cell<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<T> core::fmt::Debug for Cell<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Cell").field(&self.0).finish()
    }
}

impl<T: 'static> Cell<T> {
    /// The `n`th source a node makes, as the build numbers them: a node's
    /// handlers name their cells so. One that is not a `T` reads as none.
    pub const fn at(n: u32) -> Cell<T> {
        Cell(n, PhantomData)
    }

    /// Its value, as `db` reads it (recorded, when `db` records), or none
    /// where `db` holds no cells.
    pub fn get<'a>(&self, db: &'a dyn Read) -> Option<&'a T> {
        let (s, was) = db.sources(self.0)?;
        s.slots.get(self.0, was)
    }
}

impl Watch {
    /// The `n`th source a node makes, as [`Cell::at`]: a screen's sources
    /// name its watches so.
    pub const fn at(n: u32) -> Watch {
        Watch(n)
    }

    /// The answer, as `db` reads it (recorded, when `db` records).
    pub fn get<'a>(&self, db: &'a dyn Read) -> Option<&'a Live> {
        db.sources(self.0)?.0.live(*self)
    }
}

/// What a settle tells: the one seam between the kernel and whatever
/// shows or keeps its state. A toolkit's views, a disk's flush, an
/// upstream's records are sinks; none of them needs the others.
pub trait Sink {
    /// What `reader` read changed: it pulls when it next draws, and shows
    /// what it pulls unless it is equal to what it shows. Told at most once
    /// a settle.
    fn dirty(&mut self, reader: Reader);

    /// A watch's answer changed by `patches`, before any reader is told.
    fn patches(&mut self, _watch: Watch, _patches: &[Patch]) {}

    /// What changed in a space's rows: what a disk keeps.
    fn changes(&mut self, _space: Space, _changes: &Changes) {}
}

/// A sink that is told nothing it keeps: a node that only keeps rows.
impl Sink for () {
    fn dirty(&mut self, _: Reader) {}
}

/// The node's cells, watches and readers, and what changed since the last
/// settle.
#[derive(Default)]
pub struct Sources {
    pub(crate) slots: Slots,
    pub(crate) readers: Vec<Vec<u8>>,
    /// The watches' answers, each by its number and its query's key: out
    /// while they are pushed, so a plan never reads one.
    pub(crate) lives: Vec<(u32, (u32, Vec<u8>), Live)>,
    /// The cells written since the last settle.
    pub(crate) moved: Vec<u32>,
    /// What changed in the settle under way, as keys of tables: rows, and
    /// sources by their number.
    pub(crate) keys: Vec<(TableId, Vec<u8>)>,
}

/// Whether a read set covers any of `keys`.
pub(crate) fn hit(read: &[u8], keys: &[(TableId, Vec<u8>)]) -> bool {
    spans(read).any(|(t, lo, hi)| keys.iter().any(|(kt, k)| *kt == t && within(lo, hi, k)))
}

pub(crate) fn key(id: u32) -> (TableId, Vec<u8>) {
    (SOURCES, id.to_be_bytes().into())
}

impl Sources {
    pub(crate) fn cell<T: PartialEq + 'static>(&mut self, v: T) -> Cell<T> {
        Cell(self.slots.add(Some(Boxed::new(v))), PhantomData)
    }

    /// Writes a cell; a watch's number holds no value and takes none.
    pub(crate) fn write(&mut self, id: u32, v: Boxed) {
        if self.slots.swap(id, v) {
            self.moved.push(id);
        }
    }

    /// Starts a settle: the cells that changed (one written and written
    /// back did not), as changes.
    pub(crate) fn begin(&mut self) {
        let slots = &self.slots;
        let moved = self.moved.drain(..).filter(|&id| slots.changed(id));
        self.keys.extend(moved.map(key));
    }

    pub(crate) fn live(&self, w: Watch) -> Option<&Live> {
        self.lives.iter().find(|l| l.0 == w.0).map(|l| &l.2)
    }

    /// Ends a settle: what changed is forgotten, and what the cells held
    /// before it let go.
    pub(crate) fn end(&mut self) {
        self.keys.clear();
        self.slots.sweep();
    }
}

/// What a plan or a reader reads: the node's rows, as it reads them in each
/// space but `local`, and its sources, as they are now or (`was`) as they
/// were before the settle under way. A table's rows are in one space.
pub struct Scope<'a> {
    pub(crate) rows: [&'a dyn Read; 3],
    pub(crate) src: &'a Sources,
    pub(crate) was: bool,
}

impl Read for Scope<'_> {
    fn get(&self, table: TableId, key: &[u8]) -> Option<Vec<u8>> {
        self.rows.iter().find_map(|r| r.get(table, key))
    }

    fn scan(&self, table: TableId, range: Range<'_>, f: &mut dyn FnMut(&[u8], &[u8]) -> bool) {
        // A table's rows are in one space, so the others call `f` for none.
        for r in self.rows {
            r.scan(table, range, f);
        }
    }

    fn sources(&self, _: u32) -> Option<(&Sources, bool)> {
        Some((self.src, self.was))
    }
}

#[cfg(any(test, ark_selftest))]
pub(crate) mod tests {
    use super::*;
    use crate::entry::Origin;
    use crate::node::app::{node, Msg, COUNTS, HANDLERS, SEEN};
    use crate::node::{Node, Spaces};
    use crate::plan::{scan, tests::splice, Row};
    use crate::store::Memory;
    use crate::{Rng, Version};
    use alloc::vec;
    use core::ops::Bound;

    /// What a sink was told in one settle.
    #[derive(Default)]
    struct Told {
        dirty: Vec<Reader>,
        patches: Vec<(Watch, Vec<Patch>)>,
    }

    impl Sink for Told {
        fn dirty(&mut self, r: Reader) {
            self.dirty.push(r);
        }

        fn patches(&mut self, w: Watch, p: &[Patch]) {
            self.patches.push((w, p.to_vec()));
        }
    }

    fn settle(n: &mut Node<Msg, Memory>) -> Told {
        let mut told = Told::default();
        n.settle(&mut told);
        told
    }

    /// A count's low byte, as the device space holds it.
    fn count(db: &dyn Read, key: u8) -> u8 {
        db.get(COUNTS, &[key]).map_or(0, |v| v[0])
    }

    /// What a reader reads: a cell, a count, a watch's answer, or a derived
    /// value, a plain function of a cell and a count (their sum mod 3, so
    /// it is often equal when they are not).
    #[derive(Clone, Copy, Debug)]
    enum In {
        Cell(u32),
        Row(u8),
        Watch(u32),
        Sum(u32, u8),
    }

    fn cell(db: &dyn Read, id: u32) -> u8 {
        match id {
            0 => SEEN.get(db).map_or(99, |&n| n as u8),
            _ => Cell::<u8>::at(id).get(db).copied().unwrap_or(99),
        }
    }

    /// What a reader shows: the values `ins` read, in order.
    fn values(db: &dyn Read, ins: &[In]) -> Vec<u8> {
        let mut out = Vec::new();
        for i in ins {
            match *i {
                In::Cell(id) => out.push(cell(db, id)),
                In::Row(k) => out.push(count(db, k)),
                In::Sum(id, k) => out.push((cell(db, id) + count(db, k)) % 3),
                In::Watch(id) => {
                    for (k, v) in Watch(id).get(db).map_or(&[][..], Live::rows) {
                        out.extend(k);
                        out.extend(v);
                    }
                }
            }
        }
        out
    }

    /// What it read, by source: what changing tells it.
    fn inputs(db: &dyn Read, ins: &[In]) -> Vec<u8> {
        let mut out = Vec::new();
        for i in ins {
            match *i {
                In::Sum(id, k) => out.extend([cell(db, id), count(db, k)]),
                _ => out.extend(values(db, &[*i])),
            }
        }
        out
    }

    /// A node with three cells, two watches (a filter whose closure reads a
    /// cell, and a sort), five random readers and the rows `device` holds.
    struct World {
        node: Node<Msg, Memory>,
        watches: Vec<Watch>,
        readers: Vec<(Reader, Vec<In>)>,
    }

    /// Sources are numbered as made: `SEEN` 0, cells 1 to 3, watches 4 and 5.
    fn world(seed: u64, device: Memory, cells: [u8; 3]) -> World {
        let mut rng = Rng::new(seed);
        let stores = Spaces {
            device,
            session: Memory::new(),
            shared: Memory::new(),
        };
        let mut node = Node::new(Origin(1), 0, Version(1), &HANDLERS, stores);
        node.cell(0u64);
        for c in cells {
            node.cell(c);
        }
        let c = Cell::<u8>::at(1 + rng.below(3) as u32);
        let all = || scan(COUNTS, Bound::Unbounded, Bound::Unbounded);
        let filter = node.watch(0, &[], || {
            all().filter_with(move |db, _, v| v[0] % 2 == c.get(db).copied().unwrap_or(0) % 2)
        });
        let sort = node.watch(1, &[], || all().sort(|_, v| vec![v[0] % 3]));
        // Asked again, the same plan is the same watch.
        assert_eq!(node.watch(1, &[], all), sort);
        let mut readers = Vec::new();
        for _ in 0..5 {
            let ins: Vec<In> = (0..1 + rng.below(3))
                .map(|_| match rng.below(4) {
                    0 => In::Sum(1 + rng.below(3) as u32, rng.below(4) as u8),
                    1 => In::Row(rng.below(4) as u8),
                    2 => In::Watch(4 + rng.below(2) as u32),
                    _ => In::Cell(rng.below(4) as u32),
                })
                .collect();
            readers.push((node.reader(), ins));
        }
        World {
            node,
            watches: vec![filter, sort],
            readers,
        }
    }

    /// The contract of the settle: after random cell writes (equal ones
    /// and ones written back among them), local entries that write a cell,
    /// refused steps and row changes, every watched answer is a fresh
    /// node's and its patches splice the list; a reader is told exactly
    /// when something it read changed, never twice; and, cut off by
    /// equality as its sink does, it shows what a fresh pull shows.
    #[cfg_attr(test, test)]
    fn every_answer_and_reader_is_a_fresh_runs() {
        for seed in 0..option_env!("ARK_CELLS").and_then(|n| n.parse().ok()).unwrap_or(120) {
            let mut rng = Rng::new(seed ^ 0xce11);
            let mut w = world(seed, Memory::new(), [0, 1, 2]);
            let mut shown: Vec<Vec<Row>> = w
                .watches
                .iter()
                .map(|&x| w.node.answer(x).map_or(vec![], |l| l.rows().to_vec()))
                .collect();
            let mut pulled: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
            for (r, ins) in w.readers.clone() {
                pulled.push(w.node.read(r, |db| (values(db, &ins), inputs(db, &ins))));
            }
            for round in 0..30 {
                for _ in 0..1 + rng.below(3) {
                    match rng.below(5) {
                        0 | 1 => w.node.set(Cell::<u8>::at(1 + rng.below(3) as u32), rng.below(3) as u8),
                        2 => {
                            let e = w.node.make(Space::Local, Msg::Seen, 10);
                            w.node.step(e);
                        }
                        3 => {
                            let e = w.node.make(Space::Device, Msg::Refuse, 10);
                            w.node.step(e);
                        }
                        _ => {
                            let (key, by) = (rng.below(4) as u8, rng.below(3));
                            let msg = if by == 0 { Msg::Take { key } } else { Msg::Add { key, by } };
                            let e = w.node.make(Space::Device, msg, 10);
                            w.node.step(e);
                        }
                    }
                }
                let told = settle(&mut w.node);
                let mut dirty = told.dirty.clone();
                dirty.sort();
                dirty.dedup();
                assert_eq!(dirty.len(), told.dirty.len(), "seed {seed}: a reader told twice");
                for (x, p) in &told.patches {
                    let i = w.watches.iter().position(|y| y == x).unwrap();
                    splice(&mut shown[i], p);
                }
                for (i, (r, ins)) in w.readers.clone().into_iter().enumerate() {
                    let fresh = w.node.read(r, |db| (values(db, &ins), inputs(db, &ins)));
                    let changed = fresh.1 != pulled[i].1;
                    assert_eq!(changed, told.dirty.contains(&r), "seed {seed}, round {round}: reader {i} of {ins:?}");
                    // Untold, what it shows is what it would pull.
                    if !changed {
                        assert_eq!(fresh.0, pulled[i].0);
                    }
                    pulled[i] = fresh;
                }
                let cells = [1, 2, 3].map(|id| *w.node.get(Cell::<u8>::at(id)).unwrap());
                let fresh = world(seed, w.node.store(Space::Device).unwrap().clone(), cells);
                for (i, &x) in w.watches.iter().enumerate() {
                    let rows = fresh.node.answer(x).unwrap().rows();
                    assert_eq!(w.node.answer(x).unwrap().rows(), rows, "seed {seed}: watch {i}");
                    assert_eq!(shown[i], rows, "seed {seed}: watch {i}'s patches");
                }
            }
        }
    }

    /// A write nothing reads tells no one; an equal write, or one written
    /// back before the settle, tells no one; a refused step's cell write
    /// goes with its rows; a kept one tells its reader once.
    #[cfg_attr(test, test)]
    fn only_what_changed_is_told() {
        let mut n = node(1);
        let (c, other) = (n.cell(0u8), n.cell(0u8));
        let r = n.reader();
        assert_eq!(n.read(r, |db| c.get(db).copied()), Some(0));
        n.set(other, 5);
        assert!(settle(&mut n).dirty.is_empty());
        n.set(c, 0);
        assert!(settle(&mut n).dirty.is_empty());
        n.set(c, 1);
        n.set(c, 0);
        assert!(settle(&mut n).dirty.is_empty());
        n.set(c, 2);
        assert_eq!(settle(&mut n).dirty, [r]);
        let seen = n.reader();
        n.read(seen, |db| SEEN.get(db).copied());
        let e = n.make(Space::Device, Msg::Refuse, 10);
        n.step(e);
        assert!(settle(&mut n).dirty.is_empty());
        assert_eq!(n.get(SEEN), Some(&0));
        let e = n.make(Space::Local, Msg::Seen, 10);
        n.step(e);
        assert_eq!(settle(&mut n).dirty, [seen]);
        assert_eq!(n.get(SEEN), Some(&1));
    }

    crate::listed!(every_answer_and_reader_is_a_fresh_runs, only_what_changed_is_told);
}
