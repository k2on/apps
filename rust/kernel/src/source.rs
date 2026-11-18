//! Sources as a view reads them, through whatever reader it is given:
//! what `#[view]`'s backends call, and the code the watch's emitter
//! writes, alike.
//!
//! - A function that returns a plan ([`Of<T>`]) is a **keyed watch**: the
//!   node keeps one per key (the function) and arguments, made the first
//!   time a reader asks ([`rows`], [`row`], [`count`], [`item`]). A reader
//!   that asks for one not yet kept reads none, and is read again once
//!   the node has made it ([`crate::Node::read`]).
//! - A scalar result is **shared within a settle** by its key ([`shared`]):
//!   the first reader to ask works it out, and every other reader of it
//!   until the next settle (or the next cell written) takes the same
//!   value and records the same reads. There is no memo across settles.
//! - The host keeps two cells every view may read, made first, in this
//!   order: its [`Status`] ([`STATUS`]) and the tree's time ([`NOW`]).

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::marker::PhantomData;

use crate::cell::{Cell, Watch};
use crate::codec::{from_slice, Decode};
use crate::plan::{Live, Plan};
use crate::store::Read;

/// A plan whose rows are `T`s: what a function a view reads returns.
pub struct Of<T>(pub Plan, PhantomData<fn() -> T>);

impl<T> Of<T> {
    pub fn new(plan: Plan) -> Of<T> {
        Of(plan, PhantomData)
    }
}

/// What the host says of its link and what waits, after every callback:
/// the node's first cell ([`STATUS`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub online: bool,
    /// Every query subscribed to has its rows: the link is in step.
    pub complete: bool,
    /// A control was refused for want of a link, since the link was last
    /// in step.
    pub unreached: bool,
    /// The node's shared entries not yet confirmed.
    pub pending: usize,
    /// The entries refused since the app started, but those refused
    /// offline.
    pub refused: u32,
}

/// The host's status: the first cell a host makes.
pub const STATUS: Cell<Status> = Cell::at(0);
/// The tree's time at the last settle, in milliseconds: the second.
pub const NOW: Cell<u64> = Cell::at(1);

/// The answer kept for `key` and `args`, recorded; asked for, and none,
/// while the node keeps no watch of it.
pub fn live<'a>(db: &'a dyn Read, key: u32, args: &[u8], plan: &dyn Fn() -> Plan) -> Option<&'a Live> {
    let s = db.shared()?;
    match s.src.lives.iter().find(|l| l.1 .0 == key && l.1 .1 == args) {
        Some(l) => Watch(l.0).get(db),
        None => {
            s.src.wanted.borrow_mut().push((key, args.to_vec(), plan()));
            None
        }
    }
}

/// A keyed watch's rows, as `T`s.
pub fn rows<T: Decode>(db: &dyn Read, key: u32, args: &[u8], of: fn() -> Of<T>) -> Vec<T> {
    live(db, key, args, &|| of().0).map_or_else(Vec::new, |l| l.rows().iter().filter_map(|r| from_slice(&r.1).ok()).collect())
}

/// A keyed watch's first row: a one-row plan's answer.
pub fn row<T: Decode>(db: &dyn Read, key: u32, args: &[u8], of: fn() -> Of<T>) -> Option<T> {
    item(db, key, args, of, 0)
}

/// How many rows a keyed watch holds.
pub fn count<T>(db: &dyn Read, key: u32, args: &[u8], of: fn() -> Of<T>) -> usize {
    live(db, key, args, &|| of().0).map_or(0, |l| l.rows().len())
}

/// A keyed watch's `i`th row.
pub fn item<T: Decode>(db: &dyn Read, key: u32, args: &[u8], of: fn() -> Of<T>, i: usize) -> Option<T> {
    from_slice(&live(db, key, args, &|| of().0)?.rows().get(i)?.1).ok()
}

/// A cell's value, or its default while the node holds none.
pub fn cell<T: Clone + Default + 'static>(db: &dyn Read, c: Cell<T>) -> T {
    c.get(db).cloned().unwrap_or_default()
}

/// `f`'s result, shared by every reader that asks for `key` until the
/// next settle: worked out once, and each reader recording what it read.
/// Read unrecorded, it is worked out and not kept.
pub fn shared<T: Clone + 'static>(db: &dyn Read, key: u64, f: &dyn Fn(&dyn Read) -> T) -> T {
    let Some(s) = db.shared() else { return f(db) };
    let Some(spans) = s.spans else { return f(db) };
    if let Some((_, v, read)) = s.src.calls.borrow().iter().find(|c| c.0 == key) {
        if let Some(v) = v.downcast_ref::<T>() {
            spans.borrow_mut().extend_from_slice(read);
            return v.clone();
        }
    }
    let mark = spans.borrow().len();
    let v = f(db);
    let read = spans.borrow()[mark..].to_vec();
    s.src.calls.borrow_mut().push((key, Box::new(v.clone()), read));
    v
}

/// A key for a function, by its name: what a view's keyed watches are
/// kept by (FNV-1a, so the build and every device agree).
pub const fn key(name: &str) -> u32 {
    let b = name.as_bytes();
    let mut h: u32 = 0x811c_9dc5;
    let mut i = 0;
    while i < b.len() {
        h ^= b[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

#[cfg(any(test, ark_selftest))]
pub(crate) mod tests {
    use super::*;
    use crate::cell::{Reader, Sink};
    use crate::node::app::{node, Msg, COUNTS};
    use crate::plan::scan;
    use crate::store::Memory;
    use crate::{Node, Space};
    use core::ops::Bound;

    crate::listed!(a_function_of_a_plan_is_a_keyed_watch, a_shared_result_is_worked_out_once_a_settle);

    #[derive(Default)]
    struct Told(Vec<Reader>);

    impl Sink for Told {
        fn dirty(&mut self, r: Reader) {
            self.0.push(r);
        }
    }

    fn counts() -> Of<u8> {
        Of::new(scan(COUNTS, Bound::Unbounded, Bound::Unbounded))
    }

    fn bump(n: &mut Node<Msg, Memory>, k: u8) {
        let e = n.make(Space::Device, Msg::Add { key: k, by: 1 }, 0);
        let mut up = Vec::new();
        n.route(e, &mut up);
    }

    #[cfg_attr(test, test)]
    fn a_function_of_a_plan_is_a_keyed_watch() {
        let mut n = node(1);
        bump(&mut n, 3);
        n.settle(&mut ());
        let (a, b) = (n.reader(), n.reader());
        let k = key("counts");
        // Asked for first, it is made, and the reader read again over it.
        assert_eq!(n.read(a, |db| count(db, k, &[], counts)), 1);
        // Asked again, by another reader, it is the same watch.
        assert_eq!(n.read(b, |db| count(db, k, &[], counts)), 1);
        assert_eq!(n.sources.lives.len(), 1);
        bump(&mut n, 5);
        let mut told = Told::default();
        n.settle(&mut told);
        assert_eq!(told.0, [a, b]);
        assert_eq!(n.read(a, |db| count(db, k, &[], counts)), 2);
        // Other arguments are another watch.
        assert_eq!(n.read(a, |db| count(db, k, &[1], counts)), 2);
        assert_eq!(n.sources.lives.len(), 2);
    }

    #[cfg_attr(test, test)]
    fn a_shared_result_is_worked_out_once_a_settle() {
        let mut n = node(1);
        bump(&mut n, 3);
        n.settle(&mut ());
        let c = n.cell(7u32);
        let (a, b) = (n.reader(), n.reader());
        let runs = core::cell::Cell::new(0);
        let f = |db: &dyn Read| {
            runs.set(runs.get() + 1);
            c.get(db).copied().unwrap_or(0) + db.get(COUNTS, &[3]).map_or(0, |v| u32::from(v[0]))
        };
        assert_eq!(n.read(a, |db| shared(db, 9, &f)), 8);
        assert_eq!(n.read(b, |db| shared(db, 9, &f)), 8);
        assert_eq!(runs.get(), 1);
        // The second reader took the first's reads: told when the cell is.
        n.set(c, 9);
        let mut told = Told::default();
        n.settle(&mut told);
        assert_eq!(told.0, [a, b]);
        assert_eq!(n.read(b, |db| shared(db, 9, &f)), 10);
        assert_eq!(runs.get(), 2);
        // And when the row is.
        bump(&mut n, 3);
        let mut told = Told::default();
        n.settle(&mut told);
        assert_eq!(told.0, [a, b]);
    }
}
