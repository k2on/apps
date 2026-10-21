//! §1.16 The store as a domain's functions see it: typed table handles.
//!
//! A [`Db<S>`] is the store under a schema `S` (`#[derive(Schema)]` on the
//! struct of tables), one [`Tab`] per table: `db.playlist.get(id)`,
//! `db.playlist_item.by(PlaylistItem::PLAYLIST_ID_POS).from(id).last()`,
//! `db.media.scan()`. The author names the path every read takes — the key
//! or a declared index, a prefix of its columns typed by them — so a read
//! is as narrow as it says and nothing routes it at run time.
//!
//! **Effects are the borrow checker's.** A write — `insert`, `update`,
//! `put`, `delete` — takes `&mut` of its table, so a function given `&Db`
//! (a helper, a guard, a view's closure) cannot write. A write is judged by
//! the store as it is made (a key taken, a unique index, a reference) and
//! is a [`Change`] applied at once, so a function reads what it wrote; a
//! run that refuses has every change it made undone ([`write`]).
//!
//! A read through a handle made by [`recorded`] is remembered as what it
//! covered — a key, or a prefix of an index's entries — which is how a
//! view knows which of its elements a change touches (§1.10).

use std::cell::RefCell;
use std::marker::PhantomData;
use std::ops::{Bound, Deref, DerefMut};

use crate::row::{Index, Lead, Row};
use crate::schema::Schema;
use crate::store::{Change, Dir, Read, Scan, Store};
use crate::Refusal;

/// A read a function made: a table, its key (`None`) or an index, and the
/// prefix of the key or entries it covered (empty: all of them).
pub type Dep = (&'static str, Option<usize>, Vec<u8>);

enum Backing<'t> {
    Read(&'t Store),
    Write(&'t mut Store),
}

/// One run over a store: the changes it made, and what it read when that
/// is being recorded.
pub struct Txn<'t> {
    store: Backing<'t>,
    pub changes: Vec<Change>,
    deps: Option<Vec<Dep>>,
}

impl<'t> Txn<'t> {
    fn store(&self) -> &Store {
        match &self.store {
            Backing::Read(s) => s,
            Backing::Write(s) => s,
        }
    }

    fn note(&mut self, d: Dep) {
        if let Some(ds) = &mut self.deps {
            ds.push(d);
        }
    }

    fn apply(&mut self, c: Change) -> Result<(), Refusal> {
        match &mut self.store {
            Backing::Write(s) => s.apply(&c)?,
            Backing::Read(_) => return Err(Refusal::new("a read is not a write")),
        }
        self.changes.push(c);
        Ok(())
    }
}

/// The handles of a schema's tables (`#[derive(Schema)]`): its table
/// struct's fields, each a [`Tab`], and the schema they are.
pub trait Tables: 'static {
    type Db<'t>;
    fn handles<'t>(txn: &'t RefCell<Txn<'t>>) -> Self::Db<'t>;
    fn schema() -> Schema;
}

/// The store as `S`'s tables.
pub struct Db<'t, S: Tables> {
    tables: S::Db<'t>,
    txn: &'t RefCell<Txn<'t>>,
}

impl<'t, S: Tables> Db<'t, S> {
    fn new(txn: &'t RefCell<Txn<'t>>) -> Db<'t, S> {
        Db {
            tables: S::handles(txn),
            txn,
        }
    }

    /// How many changes this run has made.
    pub fn changed(&self) -> usize {
        self.txn.borrow().changes.len()
    }
}

impl<'t, S: Tables> Deref for Db<'t, S> {
    type Target = S::Db<'t>;
    fn deref(&self) -> &S::Db<'t> {
        &self.tables
    }
}

impl<'t, S: Tables> DerefMut for Db<'t, S> {
    fn deref_mut(&mut self) -> &mut S::Db<'t> {
        &mut self.tables
    }
}

/// `f` over `store`, writing: its answer and the changes it made, left
/// applied — or, when it refuses, none of them.
pub fn write<S: Tables, T>(store: &mut Store, f: impl for<'t> FnOnce(&mut Db<'t, S>) -> Result<T, Refusal>) -> Result<(T, Vec<Change>), Refusal> {
    let cell = RefCell::new(Txn {
        store: Backing::Write(store),
        changes: vec![],
        deps: None,
    });
    let r = f(&mut Db::new(&cell));
    let mut txn = cell.borrow_mut();
    let changes = std::mem::take(&mut txn.changes);
    match r {
        Ok(t) => Ok((t, changes)),
        Err(e) => {
            if let Backing::Write(s) = &mut txn.store {
                for c in changes.iter().rev() {
                    s.apply(&c.inverse()).expect("an inverse applies");
                }
            }
            Err(e)
        }
    }
}

/// `f` over `store`, reading.
pub fn read<S: Tables, T>(store: &Store, f: impl for<'t> FnOnce(&Db<'t, S>) -> T) -> T {
    let cell = RefCell::new(Txn {
        store: Backing::Read(store),
        changes: vec![],
        deps: None,
    });
    let t = f(&Db::new(&cell));
    t
}

/// `f` over `store`, reading, and every read it made.
pub fn recorded<S: Tables, T>(store: &Store, f: impl for<'t> FnOnce(&Db<'t, S>) -> T) -> (T, Vec<Dep>) {
    let cell = RefCell::new(Txn {
        store: Backing::Read(store),
        changes: vec![],
        deps: Some(vec![]),
    });
    let t = f(&Db::new(&cell));
    let deps = cell.borrow_mut().deps.take().unwrap_or_default();
    (t, deps)
}

/// One table of a [`Db`].
pub struct Tab<'t, R> {
    txn: &'t RefCell<Txn<'t>>,
    _p: PhantomData<fn() -> R>,
}

fn decode<R: Row>(b: &[u8]) -> R {
    R::decode(b).unwrap_or_else(|e| panic!("a row of {} reads: {e}", R::TABLE))
}

/// What a whole key is given as: every column of it (`M` tells the impls
/// apart, as [`Lead`]'s do).
pub trait Whole<Q, M> {
    fn put(q: &Q, out: &mut Vec<u8>);
}

impl<A: crate::row::Key> Whole<A, crate::row::lead::First> for (A,) {
    fn put(q: &A, out: &mut Vec<u8>) {
        q.put_key(out)
    }
}
impl<'q, Q: crate::row::Key + ?Sized, A: std::borrow::Borrow<Q>> Whole<&'q Q, crate::row::lead::FirstRef> for (A,) {
    fn put(q: &&'q Q, out: &mut Vec<u8>) {
        q.put_key(out)
    }
}
impl<A, B> Whole<(A, B), crate::row::lead::Two> for (A, B)
where
    (A, B): Lead<(A, B), crate::row::lead::Two>,
{
    fn put(q: &(A, B), out: &mut Vec<u8>) {
        <(A, B) as Lead<(A, B), crate::row::lead::Two>>::put(q, out)
    }
}
impl<A, B, C> Whole<(A, B, C), crate::row::lead::Three> for (A, B, C)
where
    (A, B, C): Lead<(A, B, C), crate::row::lead::Three>,
{
    fn put(q: &(A, B, C), out: &mut Vec<u8>) {
        <(A, B, C) as Lead<(A, B, C), crate::row::lead::Three>>::put(q, out)
    }
}

impl<'t, R: Row> Tab<'t, R> {
    pub fn new(txn: &'t RefCell<Txn<'t>>) -> Tab<'t, R> {
        Tab { txn, _p: PhantomData }
    }

    fn get_bytes(&self, key: Vec<u8>) -> Option<Vec<u8>> {
        let mut t = self.txn.borrow_mut();
        let row = t.store().get(R::TABLE, &key);
        t.note((R::TABLE, None, key));
        row
    }

    /// The row with this key.
    pub fn get<Q, M>(&self, key: Q) -> Option<R>
    where
        R::Key: Whole<Q, M>,
    {
        let mut k = vec![];
        <R::Key as Whole<Q, M>>::put(&key, &mut k);
        self.get_bytes(k).map(|b| decode(&b))
    }

    /// Whether a row has this key.
    pub fn has<Q, M>(&self, key: Q) -> bool
    where
        R::Key: Whole<Q, M>,
    {
        self.get(key).is_some()
    }

    /// Every row, in key order.
    pub fn scan(&self) -> Vec<R> {
        self.by(Index::<R, R::Key>::key()).whole().all()
    }

    /// The rows by the key or an index, from which a range is read.
    pub fn by<C>(&self, ix: Index<R, C>) -> Path<'t, R, C> {
        Path {
            txn: self.txn,
            index: ix.at,
            _p: PhantomData,
        }
    }

    /// A new row; refused when its key is taken.
    pub fn insert(&mut self, row: R) -> Result<(), Refusal> {
        let c = Change::Add {
            table: R::TABLE.into(),
            row: row.encode(),
        };
        self.txn.borrow_mut().apply(c)
    }

    /// A row in place of the one with its key; refused when there is none.
    pub fn update(&mut self, row: R) -> Result<(), Refusal> {
        let old = self
            .get_bytes(row.key_bytes())
            .ok_or_else(|| Refusal(format!("{}: no row has this key", R::TABLE)))?;
        let c = Change::Edit {
            table: R::TABLE.into(),
            old,
            new: row.encode(),
        };
        self.txn.borrow_mut().apply(c)
    }

    /// A row, new or in place of the one with its key.
    pub fn put(&mut self, row: R) -> Result<(), Refusal> {
        let (table, new) = (R::TABLE.to_string(), row.encode());
        let c = match self.get_bytes(row.key_bytes()) {
            Some(old) => Change::Edit { table, old, new },
            None => Change::Add { table, row: new },
        };
        self.txn.borrow_mut().apply(c)
    }

    /// The row with this key gone; nothing when there is none.
    pub fn delete<Q, M>(&mut self, key: Q) -> Result<(), Refusal>
    where
        R::Key: Whole<Q, M>,
    {
        let mut k = vec![];
        <R::Key as Whole<Q, M>>::put(&key, &mut k);
        match self.get_bytes(k) {
            Some(row) => self.txn.borrow_mut().apply(Change::Remove { table: R::TABLE.into(), row }),
            None => Ok(()),
        }
    }

    /// This row gone, by its key.
    pub fn remove(&mut self, row: &R) -> Result<(), Refusal> {
        match self.get_bytes(row.key_bytes()) {
            Some(row) => self.txn.borrow_mut().apply(Change::Remove { table: R::TABLE.into(), row }),
            None => Ok(()),
        }
    }
}

/// A table's rows by its key or an index, before a range is chosen.
pub struct Path<'t, R, C> {
    txn: &'t RefCell<Txn<'t>>,
    index: Option<usize>,
    _p: PhantomData<fn() -> (R, C)>,
}

impl<'t, R: Row, C> Path<'t, R, C> {
    fn range(self, lo: Bound<Vec<u8>>, hi: Bound<Vec<u8>>, prefix: Vec<u8>) -> Range<'t, R> {
        Range {
            txn: self.txn,
            index: self.index,
            lo,
            hi,
            prefix,
            dir: Dir::Asc,
            _p: PhantomData,
        }
    }

    /// Every row, in this path's order.
    pub fn whole(self) -> Range<'t, R> {
        self.range(Bound::Unbounded, Bound::Unbounded, vec![])
    }

    /// The rows whose leading columns are `q`.
    pub fn from<Q, M>(self, q: Q) -> Range<'t, R>
    where
        C: Lead<Q, M>,
    {
        let mut p = vec![];
        C::put(&q, &mut p);
        let s = Scan::prefix(None, &p, Dir::Asc, 0);
        self.range(s.lo, s.hi, p)
    }

    /// The rows from `lo` up to, not including, `hi`, each a prefix of the
    /// columns of one shape.
    pub fn between<Q, M>(self, lo: Q, hi: Q) -> Range<'t, R>
    where
        C: Lead<Q, M>,
    {
        let (mut l, mut h) = (vec![], vec![]);
        C::put(&lo, &mut l);
        C::put(&hi, &mut h);
        let common = l.iter().zip(&h).take_while(|(a, b)| a == b).count();
        let p = l[..common].to_vec();
        self.range(Bound::Included(l), Bound::Excluded(h), p)
    }
}

/// Rows in a range of a path, read when it is asked for them.
pub struct Range<'t, R> {
    txn: &'t RefCell<Txn<'t>>,
    index: Option<usize>,
    lo: Bound<Vec<u8>>,
    hi: Bound<Vec<u8>>,
    prefix: Vec<u8>,
    dir: Dir,
    _p: PhantomData<fn() -> R>,
}

impl<'t, R: Row> Range<'t, R> {
    /// The range read from its end.
    pub fn rev(mut self) -> Range<'t, R> {
        self.dir = match self.dir {
            Dir::Asc => Dir::Desc,
            Dir::Desc => Dir::Asc,
        };
        self
    }

    fn read(self, limit: usize) -> Vec<R> {
        let mut t = self.txn.borrow_mut();
        let scan = Scan {
            index: self.index,
            lo: self.lo,
            hi: self.hi,
            dir: self.dir,
            limit,
        };
        let rows = t.store().scan(R::TABLE, &scan);
        t.note((R::TABLE, self.index, self.prefix));
        rows.iter().map(|b| decode(b)).collect()
    }

    /// The first `n` rows.
    pub fn take(self, n: usize) -> Vec<R> {
        self.read(n)
    }

    pub fn all(self) -> Vec<R> {
        self.read(usize::MAX)
    }

    pub fn first(self) -> Option<R> {
        self.read(1).pop()
    }

    pub fn last(self) -> Option<R> {
        self.rev().first()
    }

    pub fn count(self) -> usize {
        self.read(usize::MAX).len()
    }

    pub fn any(self) -> bool {
        self.first().is_some()
    }
}
