//! §1.3 The store: one ordered map from a row's key to its bytes, and from
//! each index entry (the indexed columns' bytes, then the key's) to
//! nothing, per table of the schema.
//!
//! Every write judges the constraints — the row is canonical bytes of the
//! table's row type, a unique index holds, a reference names a row that
//! exists at the moment of the write — and refuses rather than corrupts.
//! Every write is a [`Change`], and a change **inverts exactly**
//! ([`Change::inverse`]): an `Add` by removing its row, a `Remove` by
//! putting it back, an `Edit` by putting `old`. That is all the rebase
//! (§1.7) needs: there is no overlay; the optimistic state is the confirmed
//! one with the pending change lists applied, undone newest-first.
//!
//! The state hash is kept as the rows are written: per table, the sum
//! modulo 2^256 of `sha256(table ‖ row)` over its rows (each table's
//! digest is its own, so a leaf is never added to another table's), moved
//! by one leaf out and one in per write; the state hash is
//! `sha256(schema_hash ‖ digests of the durable tables in schema order)`.
//! The schema hash is in it, so two stores are equal only when their bytes
//! mean the same. A store is of a [`Space`]: its hash and its snapshot are
//! over that space's tables only, so a durable store's leave the volatile
//! tables out, and the volatile machine's (§1.9) the durable ones.
//!
//! A row another row names by reference is not removed while it does:
//! the reference is judged at both ends (v4's `StillReferenced`).
//!
//! **The log is a table** (§1.6): `_log` ([`LOG`]) beside the schema's,
//! keyed by sequence, each row a [`Logged`]'s encoding. It is not the
//! schema's — not in its hash or the state hash, not in a snapshot sent,
//! not a table a function names, not in a store's equality — and its
//! invariant is the chain: a row is added at either end only when it
//! links onto its neighbour, removed only at either end, never edited
//! ([`Store::apply_kv`]). The store's rows with the log's are what a peer
//! makes durable ([`Store::kv_rows`]).

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ops::Bound;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::format::{put_key, Encode};
use crate::hash::{add256, sha256, sub256, EntryHash, SchemaHash, StateHash};
use crate::log::{Logged, Rows as KvRows, Seq};
use crate::schema::{Parsed, Schema, Space, Table};
use crate::Refusal;

/// The log's table (§1.6).
pub const LOG: &str = "_log";

/// One write, as the store reports it and the log keeps it (a fact): its
/// rows each the row's encoding, carried as it is.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Change {
    Add {
        table: String,
        #[serde(with = "crate::format::raw")]
        row: Vec<u8>,
    },
    Remove {
        table: String,
        #[serde(with = "crate::format::raw")]
        row: Vec<u8>,
    },
    Edit {
        table: String,
        #[serde(with = "crate::format::raw")]
        old: Vec<u8>,
        #[serde(with = "crate::format::raw")]
        new: Vec<u8>,
    },
}

impl Change {
    pub fn table(&self) -> &str {
        match self {
            Change::Add { table, .. } | Change::Remove { table, .. } | Change::Edit { table, .. } => table,
        }
    }

    /// The change that undoes this one.
    pub fn inverse(&self) -> Change {
        match self.clone() {
            Change::Add { table, row } => Change::Remove { table, row },
            Change::Remove { table, row } => Change::Add { table, row },
            Change::Edit { table, old, new } => Change::Edit { table, old: new, new: old },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Asc,
    Desc,
}

/// A range read: over the keys, or over index `index`'s entries; between
/// `lo` and `hi`, in `dir`, at most `limit` rows. To resume after a row,
/// exclude its key (or entry) as the bound in the direction read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scan {
    pub index: Option<usize>,
    pub lo: Bound<Vec<u8>>,
    pub hi: Bound<Vec<u8>>,
    pub dir: Dir,
    pub limit: usize,
}

impl Scan {
    /// Every row, in key order.
    pub fn all() -> Scan {
        Scan {
            index: None,
            lo: Bound::Unbounded,
            hi: Bound::Unbounded,
            dir: Dir::Asc,
            limit: usize::MAX,
        }
    }

    /// The keys (or entries) that start with `p`: since encodings are
    /// prefix-free, those whose leading columns are the ones `p` encodes.
    pub fn prefix(index: Option<usize>, p: &[u8], dir: Dir, limit: usize) -> Scan {
        let mut hi = p.to_vec();
        while hi.last() == Some(&0xFF) {
            hi.pop();
        }
        let hi = match hi.last_mut() {
            Some(b) => {
                *b += 1;
                Bound::Excluded(hi)
            }
            None => Bound::Unbounded,
        };
        Scan {
            index,
            lo: Bound::Included(p.to_vec()),
            hi,
            dir,
            limit,
        }
    }
}

/// What a function may do with a store: read it (§1.4's effect `Read`).
pub trait Read {
    fn schema(&self) -> &Schema;
    fn get(&self, table: &str, key: &[u8]) -> Option<Vec<u8>>;
    fn scan(&self, table: &str, scan: &Scan) -> Vec<Vec<u8>>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Rows {
    /// The table's name, the first half of every leaf.
    name: Vec<u8>,
    rows: BTreeMap<Vec<u8>, Vec<u8>>,
    /// Per index, each entry and the key of its row.
    indexes: Vec<BTreeMap<Vec<u8>, Vec<u8>>>,
    digest: [u8; 32],
}

#[derive(Clone, Debug)]
pub struct Store {
    schema: Arc<Schema>,
    shash: SchemaHash,
    tables: Vec<Rows>,
    space: Space,
    /// The log's table: sequence's bytes to the row.
    log: BTreeMap<Vec<u8>, LogRow>,
}

impl PartialEq for Store {
    fn eq(&self, other: &Store) -> bool {
        self.shash == other.shash && self.space == other.space && self.tables == other.tables
    }
}
impl Eq for Store {}

/// Per table, the leaves undoing some changes adds to its digest and
/// takes from it ([`Store::undo_of`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Undo(Vec<(usize, [u8; 32], [u8; 32])>);

impl Undo {
    /// Digests, in schema order, with the changes undone.
    pub fn apply(&self, digests: &mut [[u8; 32]]) {
        for (t, plus, minus) in &self.0 {
            add256(&mut digests[*t], plus);
            sub256(&mut digests[*t], minus);
        }
    }
}

/// The log as its table holds it, read (§1.6). Every peer's log stands on
/// a first row; the entries are the rows after it.
#[derive(Clone, Copy)]
pub struct Log<'a>(&'a BTreeMap<Vec<u8>, LogRow>);

/// A row of the log's table, and its sequence, state and hash, taken from
/// the entry it was read or written as.
#[derive(Clone, Debug, PartialEq, Eq)]
struct LogRow {
    bytes: Vec<u8>,
    ends: (Seq, StateHash, EntryHash),
}

impl LogRow {
    fn of(l: &Logged, bytes: Vec<u8>) -> LogRow {
        LogRow {
            ends: (l.seq, l.state, l.hash),
            bytes,
        }
    }
}

impl PartialEq for Log<'_> {
    fn eq(&self, other: &Log) -> bool {
        self.0 == other.0
    }
}

impl std::fmt::Debug for Log<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_list().entries(self.0.values().map(read)).finish()
    }
}

fn read(b: &LogRow) -> Logged {
    Logged::from_bytes(&b.bytes).expect("a row of the log reads")
}

/// A sequence's key in the log's table.
fn seq_key(seq: Seq) -> Vec<u8> {
    let mut k = vec![];
    put_key(&seq, &mut k);
    k
}

impl<'a> Log<'a> {
    /// The row the log stands on.
    pub fn base(&self) -> Logged {
        read(self.0.values().next().expect("a log stands on a row"))
    }

    pub fn base_seq(&self) -> Seq {
        self.0.values().next().expect("a log stands on a row").ends.0
    }

    fn last(&self) -> (Seq, StateHash, EntryHash) {
        self.0.values().next_back().expect("a log stands on a row").ends
    }

    /// The last sequence and the chain's hash there.
    pub fn head(&self) -> (Seq, EntryHash) {
        let (n, _, h) = self.last();
        (n, h)
    }

    pub fn head_state(&self) -> StateHash {
        self.last().1
    }

    /// The entry at `seq`, above the row the log stands on.
    pub fn at(&self, seq: Seq) -> Option<Logged> {
        (seq > self.base_seq()).then(|| self.0.get(&seq_key(seq)).map(read)).flatten()
    }

    /// Whether this log has `hash` at `seq`: one history up to there.
    pub fn has(&self, seq: Seq, hash: &EntryHash) -> bool {
        self.0.get(&seq_key(seq)).is_some_and(|b| b.ends.2 == *hash)
    }

    /// The entries after `seq`, in order.
    pub fn after(&self, seq: Seq) -> impl Iterator<Item = Logged> + 'a {
        let from = seq_key(seq.max(self.base_seq()));
        self.0.range((Bound::Excluded(from), Bound::Unbounded)).map(|(_, b)| read(b))
    }

    /// Every entry above the row the log stands on.
    pub fn entries(&self) -> Vec<Logged> {
        self.after(Seq::MIN).collect()
    }
}

fn bound(b: &Bound<Vec<u8>>) -> Bound<&[u8]> {
    match b {
        Bound::Included(x) => Bound::Included(x),
        Bound::Excluded(x) => Bound::Excluded(x),
        Bound::Unbounded => Bound::Unbounded,
    }
}

// A range `BTreeMap::range` would panic on reads nothing instead.
fn empty(s: &Scan) -> bool {
    match (&s.lo, &s.hi) {
        (Bound::Included(l), Bound::Included(h)) => l > h,
        (Bound::Included(l) | Bound::Excluded(l), Bound::Excluded(h)) | (Bound::Excluded(l), Bound::Included(h)) => l >= h,
        _ => false,
    }
}

fn ordered<'a, T: 'a>(it: impl DoubleEndedIterator<Item = T> + 'a, dir: Dir) -> Box<dyn Iterator<Item = T> + 'a> {
    match dir {
        Dir::Asc => Box::new(it),
        Dir::Desc => Box::new(it.rev()),
    }
}

impl Store {
    /// An empty store whose hash and snapshot are over `space`'s tables.
    pub fn new(schema: Arc<Schema>, space: Space) -> Store {
        let tables = schema
            .tables
            .iter()
            .map(|t| Rows {
                name: t.name.as_bytes().to_vec(),
                rows: BTreeMap::new(),
                indexes: vec![BTreeMap::new(); t.indexes.len()],
                digest: [0; 32],
            })
            .collect();
        Store {
            shash: schema.hash(),
            schema,
            tables,
            space,
            log: BTreeMap::new(),
        }
    }

    /// The log, as its table holds it.
    pub fn log(&self) -> Log<'_> {
        Log(&self.log)
    }

    /// One write to the store, the log's table's included: a row of the
    /// schema's judged as [`Store::apply`] judges it, a row of the log's by
    /// the chain — added at either end only when it links onto the row
    /// beside it (any row starts an empty table), removed only at either
    /// end, never edited.
    pub fn apply_kv(&mut self, c: &Change) -> Result<(), Refusal> {
        let row = match c {
            _ if c.table() != LOG => return self.apply(c),
            Change::Edit { .. } => return Err(Refusal::new("the log is not edited")),
            Change::Add { row, .. } | Change::Remove { row, .. } => row,
        };
        let l = Logged::from_bytes(row)?;
        let key = seq_key(l.seq);
        let (first, last) = (self.log.first_key_value(), self.log.last_key_value());
        let ok = match c {
            Change::Add { .. } => match (first, last) {
                (Some((_, f)), Some((_, b))) => {
                    let (n, _, h) = b.ends;
                    l.seq == n + 1 && l.links(&h) || read(f).seq == l.seq + 1 && read(f).links(&l.hash)
                }
                _ => true,
            },
            _ => self.log.get(&key).map(|r| &r.bytes) == Some(row) && (first.map(|f| f.0) == Some(&key) || last.map(|b| b.0) == Some(&key)),
        };
        if !ok {
            return Err(Refusal(format!("{LOG}: entry {} is not an end of the chain", l.seq)));
        }
        match c {
            Change::Add { .. } => {
                self.log.insert(key, LogRow::of(&l, row.clone()));
            }
            _ => {
                self.log.remove(&key);
            }
        }
        Ok(())
    }

    /// The log's table emptied and these rows, in order, written to it.
    pub fn start_log(&mut self, rows: impl IntoIterator<Item = Vec<u8>>) -> Result<(), Refusal> {
        self.log.clear();
        rows.into_iter()
            .try_for_each(|row| self.apply_kv(&Change::Add { table: LOG.into(), row }))
    }

    /// A logged entry appended as its row of the log's table, as a peer
    /// opens its log (§1.6): the entry must link onto the head, and
    /// `facts` — its own, as this store's schema reads them — apply, all or
    /// none.
    pub fn append(&mut self, l: &Logged, facts: &[Change]) -> Result<(), Refusal> {
        let (n, h) = self.log().head();
        if l.seq != n + 1 || !l.links(&h) {
            return Err(Refusal(format!("{LOG}: entry {} does not link onto the head", l.seq)));
        }
        self.apply_all(facts)?;
        self.log_push(l, l.to_bytes());
        Ok(())
    }

    /// An entry added at the head of the log as its row, `row`, by a peer
    /// that chained it onto the head, or checked that it links there.
    pub(crate) fn log_push(&mut self, l: &Logged, row: Vec<u8>) {
        self.log.insert(seq_key(l.seq), LogRow::of(l, row));
    }

    /// The log's rows above `seq` deleted, newest first.
    pub fn log_truncate(&mut self, seq: Seq) {
        self.log.split_off(&seq_key(seq + 1));
    }

    /// The horizon: the log's rows below `seq` deleted, so that it stands
    /// on the row there.
    pub fn log_horizon(&mut self, seq: Seq) {
        self.log = self.log.split_off(&seq_key(seq));
    }

    pub fn space(&self) -> Space {
        self.space
    }

    pub fn schema_arc(&self) -> &Arc<Schema> {
        &self.schema
    }

    pub fn schema_hash(&self) -> SchemaHash {
        self.shash
    }

    fn find(&self, table: &str) -> Result<usize, Refusal> {
        self.schema
            .table(table)
            .map(|(i, _)| i)
            .ok_or_else(|| Refusal(format!("no table {table}")))
    }

    /// A table's rows in key order; none for a table the schema lacks.
    pub fn rows(&self, table: &str) -> impl Iterator<Item = &[u8]> {
        self.find(table)
            .ok()
            .into_iter()
            .flat_map(move |t| self.tables[t].rows.values().map(|r| &r[..]))
    }

    /// `sha256(table ‖ row)`, read as a 256-bit integer.
    pub fn leaf(table: &str, row: &[u8]) -> [u8; 32] {
        sha256(&[table.as_bytes(), row])
    }

    /// A table's digest: the sum of its leaves modulo 2^256.
    pub fn digest(&self, table: &str) -> Option<[u8; 32]> {
        self.find(table).ok().map(|t| self.tables[t].digest)
    }

    fn hash_of(&self, digests: impl Iterator<Item = [u8; 32]>) -> StateHash {
        let mut parts: Vec<u8> = self.shash.0.to_vec();
        for (d, t) in digests.zip(&self.schema.tables) {
            if t.space == self.space {
                parts.extend_from_slice(&d);
            }
        }
        StateHash::of(&parts)
    }

    pub fn state_hash(&self) -> StateHash {
        self.hash_of(self.tables.iter().map(|r| r.digest))
    }

    /// What undoing `changes` does to the tables' digests, summed per
    /// table: worked out once for changes [`Store::hash_beside`] is asked
    /// past many times.
    pub fn undo_of<'a>(&self, changes: impl IntoIterator<Item = &'a Change>) -> Undo {
        let mut u: Vec<(usize, [u8; 32], [u8; 32])> = vec![];
        for c in changes {
            let Ok(t) = self.find(c.table()) else { continue };
            let i = u.iter().position(|x| x.0 == t).unwrap_or_else(|| {
                u.push((t, [0; 32], [0; 32]));
                u.len() - 1
            });
            let (_, plus, minus) = &mut u[i];
            match c {
                Change::Add { table, row } => add256(minus, &Store::leaf(table, row)),
                Change::Remove { table, row } => add256(plus, &Store::leaf(table, row)),
                Change::Edit { table, old, new } => {
                    add256(minus, &Store::leaf(table, new));
                    add256(plus, &Store::leaf(table, old));
                }
            }
        }
        Undo(u)
    }

    /// The state hash the store would have with `undone` undone, by the
    /// digests' arithmetic alone: what a replica checks a confirmed state
    /// by while its pending changes stay applied.
    pub fn hash_beside<'a>(&self, undone: impl IntoIterator<Item = &'a Undo>) -> StateHash {
        let mut ds = self.digests();
        undone.into_iter().for_each(|u| u.apply(&mut ds));
        self.hash_with(&ds)
    }

    /// The tables' digests, in schema order.
    pub fn digests(&self) -> Vec<[u8; 32]> {
        self.tables.iter().map(|r| r.digest).collect()
    }

    /// The state hash these digests, in schema order, give.
    pub fn hash_with(&self, digests: &[[u8; 32]]) -> StateHash {
        self.hash_of(digests.iter().copied())
    }

    /// A row's key, and its entry in each of its table's indexes: what a
    /// read through the key or an index covers of it (§1.10).
    pub fn paths_of(&self, table: &str, row: &[u8]) -> Result<(Vec<u8>, Vec<Vec<u8>>), Refusal> {
        let p = self.schema.tables[self.find(table)?].parse(row)?;
        Ok((p.key, p.entries))
    }

    /// A row's key.
    pub fn key_of(&self, table: &str, row: &[u8]) -> Result<Vec<u8>, Refusal> {
        Ok(self.schema.tables[self.find(table)?].parse(row)?.key)
    }

    /// Applies one change, judging it: the row before must be what the
    /// change says, the row after canonical and within every constraint.
    pub fn apply(&mut self, c: &Change) -> Result<(), Refusal> {
        let schema = self.schema.clone();
        let t = self.find(c.table())?;
        let tab = &schema.tables[t];
        let (old, new) = match c {
            Change::Add { row, .. } => (None, Some(row)),
            Change::Remove { row, .. } => (Some(row), None),
            Change::Edit { old, new, .. } => (Some(old), Some(new)),
        };
        let parsed_new = new.map(|r| tab.parse(r)).transpose()?;
        // An old row must be the one held, which is canonical: checked below.
        let parsed_old = old.map(|r| tab.parse_held(r)).transpose()?;
        let key = match (&parsed_new, &parsed_old) {
            (Some(n), Some(o)) if n.key != o.key => return Err(Refusal(format!("{}: an edit keeps its key", tab.name))),
            (Some(n), _) => &n.key,
            (None, Some(o)) => &o.key,
            _ => unreachable!(),
        };
        match (self.tables[t].rows.get(key), old) {
            (Some(have), Some(o)) if have == o => {}
            (None, None) => {}
            (Some(_), None) => return Err(Refusal(format!("{}: a row with this key exists", tab.name))),
            (None, Some(_)) => return Err(Refusal(format!("{}: no row has this key", tab.name))),
            (Some(_), Some(_)) => return Err(Refusal(format!("{}: the row is not the one the change replaces", tab.name))),
        }
        match &parsed_new {
            Some(n) => self.judge(tab, n)?,
            None => self.unnamed(tab, key)?,
        }
        let key = key.clone();
        self.write(
            t,
            &key,
            old.map(|o| &o[..]).zip(parsed_old.as_ref()),
            new.map(|n| &n[..]).zip(parsed_new.as_ref()),
        );
        Ok(())
    }

    /// Applies changes in order, all or none.
    pub fn apply_all(&mut self, cs: &[Change]) -> Result<(), Refusal> {
        for (i, c) in cs.iter().enumerate() {
            if let Err(e) = self.apply(c) {
                self.undo(&cs[..i]).expect("an inverse applies");
                return Err(e);
            }
        }
        Ok(())
    }

    /// Changes made, undone: each one's inverse applied, newest first, up
    /// to the first that does not apply; the inverses, as applied.
    pub fn undo<'c, I>(&mut self, cs: I) -> Result<Vec<Change>, Refusal>
    where
        I: IntoIterator<Item = &'c Change>,
        I::IntoIter: DoubleEndedIterator,
    {
        cs.into_iter().rev().map(|c| c.inverse()).map(|c| self.apply(&c).map(|_| c)).collect()
    }

    /// A table's rows by their keys, in key order.
    pub(crate) fn keyed(&self, table: &str) -> impl Iterator<Item = (&[u8], &[u8])> {
        self.find(table)
            .ok()
            .into_iter()
            .flat_map(move |t| self.tables[t].rows.iter().map(|(k, r)| (&k[..], &r[..])))
    }

    /// Puts a row, adding it or replacing the one under its key.
    pub fn put(&mut self, table: &str, row: Vec<u8>) -> Result<Change, Refusal> {
        let t = self.find(table)?;
        let key = self.schema.tables[t].parse(&row)?.key;
        let c = match self.tables[t].rows.get(&key) {
            None => Change::Add { table: table.into(), row },
            Some(old) => Change::Edit {
                table: table.into(),
                old: old.clone(),
                new: row,
            },
        };
        self.apply(&c).map(|_| c)
    }

    /// Deletes the row under `key`.
    pub fn delete(&mut self, table: &str, key: &[u8]) -> Result<Change, Refusal> {
        let t = self.find(table)?;
        let row = self.tables[t]
            .rows
            .get(key)
            .cloned()
            .ok_or_else(|| Refusal(format!("{table}: no row has this key")))?;
        let c = Change::Remove { table: table.into(), row };
        self.apply(&c).map(|_| c)
    }

    // The constraints on a row about to be written under its key.
    fn judge(&self, tab: &Table, p: &Parsed) -> Result<(), Refusal> {
        let t = self.find(&tab.name)?;
        for (i, (ix, e)) in tab.indexes.iter().zip(&p.entries).enumerate() {
            let cols = &e[..e.len() - p.key.len()];
            let clash = ix.unique
                && self.tables[t].indexes[i]
                    .range::<[u8], _>((Bound::Included(cols), Bound::Unbounded))
                    .take_while(|(x, _)| x.starts_with(cols))
                    .any(|(_, k)| *k != p.key);
            if clash {
                let names: Vec<&str> = ix.columns.iter().map(|c| &tab.columns[*c].name[..]).collect();
                return Err(Refusal(format!("{}: another row has this ({})", tab.name, names.join(", "))));
            }
        }
        for r in &tab.refs {
            let Some(k) = p.named(tab, r.column)? else { continue };
            if !self.tables[self.find(&r.table)?].rows.contains_key(&k) {
                return Err(Refusal(format!(
                    "{}.{}: no {} has this key",
                    tab.name, tab.columns[r.column].name, r.table
                )));
            }
        }
        Ok(())
    }

    // No row names this one, about to be removed, by a reference: through an
    // index the referring column leads, or the key when it leads it, or
    // else by reading the referring table.
    fn unnamed(&self, tab: &Table, key: &[u8]) -> Result<(), Refusal> {
        for (rt, rows) in self.schema.tables.iter().zip(&self.tables) {
            for r in rt.refs.iter().filter(|r| r.table == tab.name) {
                let opt = rt.columns[r.column].ty.is_option();
                let want = [if opt { &[1u8][..] } else { &[][..] }, key].concat();
                let first = |it: &mut dyn Iterator<Item = &Vec<u8>>| it.next().is_some_and(|e| e.starts_with(&want));
                let from = (Bound::Included(&want[..]), Bound::Unbounded);
                let hit = match rt.indexes.iter().position(|ix| ix.columns.first() == Some(&r.column)) {
                    Some(i) => first(&mut rows.indexes[i].range::<[u8], _>(from).map(|(e, _)| e)),
                    None if r.column == 0 => first(&mut rows.rows.range::<[u8], _>(from).map(|(k, _)| k)),
                    None => rows.rows.values().any(|row| {
                        let p = rt.parse_held(row).expect("a stored row parses");
                        p.named(rt, r.column).expect("a stored row's reference reads").as_deref() == Some(key)
                    }),
                };
                if hit {
                    let c = &rt.columns[r.column].name;
                    return Err(Refusal(format!("{}: named by {}.{c}", tab.name, rt.name)));
                }
            }
        }
        Ok(())
    }

    // The write itself, judged already: indexes, digest, rows.
    fn write(&mut self, t: usize, key: &[u8], old: Option<(&[u8], &Parsed)>, new: Option<(&[u8], &Parsed)>) {
        let rows = &mut self.tables[t];
        if let Some((o, p)) = old {
            for (map, e) in rows.indexes.iter_mut().zip(&p.entries) {
                map.remove(e);
            }
            sub256(&mut rows.digest, &sha256(&[&rows.name, o]));
            rows.rows.remove(key);
        }
        if let Some((n, p)) = new {
            for (map, e) in rows.indexes.iter_mut().zip(&p.entries) {
                map.insert(e.clone(), key.to_vec());
            }
            add256(&mut rows.digest, &sha256(&[&rows.name, n]));
            rows.rows.insert(key.to_vec(), n.to_vec());
        }
    }

    /// The rows of this store's space's tables in key order: what a snapshot
    /// carries.
    pub fn snapshot_rows(&self) -> Vec<(String, Vec<Vec<u8>>)> {
        self.schema
            .tables
            .iter()
            .zip(&self.tables)
            .filter(|(t, _)| t.space == self.space)
            .map(|(t, r)| (t.name.clone(), r.rows.values().cloned().collect()))
            .collect()
    }

    /// Every row of this store's space's tables and of the log's, the log's
    /// last: what a peer makes durable whole.
    pub fn kv_rows(&self) -> KvRows {
        let mut rows = self.snapshot_rows();
        rows.push((LOG.into(), self.log.values().map(|r| r.bytes.clone()).collect()));
        rows
    }

    /// A store of `space` holding exactly these rows, as some schema of this
    /// one wrote them (§1.2): each read as it is, and only when it is not
    /// this schema's completed or cut to its prefix ([`Table::fit`]); a
    /// table this schema lacks dropped; constraints judged once all are in,
    /// so the order tables come in does not matter. And whether any was
    /// another schema's (a table of this space missing counts). Each row is
    /// parsed once, unless it is fitted.
    pub fn from_rows(schema: Arc<Schema>, space: Space, rows: &[(String, Vec<Vec<u8>>)]) -> Result<(Store, bool), Refusal> {
        Store::of_rows(schema, space, rows, true)
    }

    /// [`Store::from_rows`], its constraints judged or not: a replica's
    /// filling holds rows whose neighbours are not all there yet.
    pub(crate) fn of_rows(schema: Arc<Schema>, space: Space, rows: &[(String, Vec<Vec<u8>>)], judged: bool) -> Result<(Store, bool), Refusal> {
        let mut s = Store::new(schema.clone(), space);
        let mut other = schema.tables.iter().filter(|t| t.space == space).count() != rows.len();
        let mut parsed: Vec<Vec<Parsed>> = schema.tables.iter().map(|_| vec![]).collect();
        for (table, rs) in rows {
            let t = match s.find(table) {
                Err(_) => {
                    other = true;
                    continue;
                }
                t => t?,
            };
            let tab = &schema.tables[t];
            for row in rs {
                let (row, p) = match tab.parse(row) {
                    Ok(p) => (Cow::Borrowed(&row[..]), p),
                    Err(e) => {
                        let (r, o) = tab.fit(row).map_err(|_| e)?;
                        other |= o;
                        let p = tab.parse(&r)?;
                        (Cow::Owned(r), p)
                    }
                };
                if s.tables[t].rows.contains_key(&p.key) {
                    return Err(Refusal(format!("{table}: two rows have one key")));
                }
                let key = p.key.clone();
                s.write(t, &key, None, Some((&row, &p)));
                parsed[t].push(p);
            }
        }
        for (t, tab) in schema.tables.iter().enumerate().filter(|_| judged) {
            for p in &parsed[t] {
                s.judge(tab, p)?;
            }
        }
        Ok((s, other))
    }

    /// Every row of `table` in key order from `from` on, each with its key.
    pub(crate) fn keyed_from<'a>(&'a self, table: &str, from: &'a [u8]) -> impl Iterator<Item = (&'a [u8], &'a [u8])> + 'a {
        self.find(table).ok().into_iter().flat_map(move |t| {
            self.tables[t]
                .rows
                .range::<[u8], _>((Bound::Included(from), Bound::Unbounded))
                .map(|(k, r)| (&k[..], &r[..]))
        })
    }

    /// The rows of `table` from `from` to `to` (none: its end) replaced by
    /// `rows`, as some schema of this one wrote them, judging nothing: how a
    /// replica filling holds a range it was sent, whose neighbours it may
    /// not hold yet. What moved, as changes; and whether a row was another
    /// schema's. A table this schema lacks is none of its.
    pub(crate) fn replace(&mut self, table: &str, from: &[u8], to: Option<&[u8]>, rows: &[Vec<u8>]) -> Result<(Vec<Change>, bool), Refusal> {
        let Ok(t) = self.find(table) else {
            return Ok((vec![], true));
        };
        let schema = self.schema.clone();
        let tab = &schema.tables[t];
        let mut other = false;
        let mut sent = BTreeMap::new();
        for row in rows {
            let (row, p) = match tab.parse(row) {
                Ok(p) => (row.clone(), p),
                Err(e) => {
                    let (r, o) = tab.fit(row).map_err(|_| e)?;
                    other |= o;
                    let p = tab.parse(&r)?;
                    (r, p)
                }
            };
            sent.insert(p.key, row);
        }
        let hi = to.map_or(Bound::Unbounded, Bound::Excluded);
        let held: Vec<Vec<u8>> = self.tables[t]
            .rows
            .range::<[u8], _>((Bound::Included(from), hi))
            .map(|(k, _)| k.clone())
            .filter(|k| !sent.contains_key(k))
            .collect();
        let mut out = vec![];
        for k in held {
            out.extend(self.force(t, &k, None)?);
        }
        for (k, row) in sent {
            out.extend(self.force(t, &k, Some(row))?);
        }
        Ok((out, other))
    }

    /// Changes applied as they are, judging nothing but each row's bytes:
    /// an `Add` or an `Edit` puts its row, a `Remove` takes its key's away,
    /// whatever was held — how a replica filling takes an entry's facts
    /// over rows it may not all hold yet. What moved, as changes.
    pub(crate) fn force_all(&mut self, cs: &[Change]) -> Result<Vec<Change>, Refusal> {
        let mut out = vec![];
        for c in cs {
            let Ok(t) = self.find(c.table()) else { continue };
            let tab = &self.schema.tables[t];
            let (row, gone) = match c {
                Change::Add { row, .. } | Change::Edit { new: row, .. } => (row, false),
                Change::Remove { row, .. } => (row, true),
            };
            let key = tab.parse(row)?.key;
            out.extend(self.force(t, &key, (!gone).then(|| row.clone()))?);
        }
        Ok(out)
    }

    // A key's row written as `row` (none: gone), judging nothing; the
    // change, when one was made.
    fn force(&mut self, t: usize, key: &[u8], row: Option<Vec<u8>>) -> Result<Option<Change>, Refusal> {
        let schema = self.schema.clone();
        let tab = &schema.tables[t];
        let old = self.tables[t].rows.get(key).cloned();
        if old == row {
            return Ok(None);
        }
        let po = old.as_ref().map(|o| tab.parse_held(o)).transpose()?;
        let pn = row.as_ref().map(|r| tab.parse(r)).transpose()?;
        self.write(t, key, old.as_deref().zip(po.as_ref()), row.as_deref().zip(pn.as_ref()));
        let table = tab.name.clone();
        Ok(Some(match (old, row) {
            (None, Some(row)) => Change::Add { table, row },
            (Some(row), None) => Change::Remove { table, row },
            (Some(old), Some(new)) => Change::Edit { table, old, new },
            (None, None) => unreachable!(),
        }))
    }

    /// The changes that take this store to `new`, table by table in schema
    /// order and key order within one: how a view is told a store was
    /// replaced (a snapshot), as changes like any other.
    pub fn diff(&self, new: &Store) -> Vec<Change> {
        let mut out = vec![];
        for (t, tab) in self.schema.tables.iter().enumerate() {
            let (a, b) = (&self.tables[t].rows, &new.tables[t].rows);
            let table = || tab.name.clone();
            for (k, row) in a {
                match b.get(k) {
                    None => out.push(Change::Remove {
                        table: table(),
                        row: row.clone(),
                    }),
                    Some(n) if n != row => out.push(Change::Edit {
                        table: table(),
                        old: row.clone(),
                        new: n.clone(),
                    }),
                    _ => {}
                }
            }
            out.extend(b.iter().filter(|(k, _)| !a.contains_key(*k)).map(|(_, row)| Change::Add {
                table: table(),
                row: row.clone(),
            }));
        }
        out
    }
}

impl Read for Store {
    fn schema(&self) -> &Schema {
        &self.schema
    }

    fn get(&self, table: &str, key: &[u8]) -> Option<Vec<u8>> {
        self.tables[self.find(table).ok()?].rows.get(key).cloned()
    }

    fn scan(&self, table: &str, s: &Scan) -> Vec<Vec<u8>> {
        let Ok(t) = self.find(table) else { return vec![] };
        if empty(s) {
            return vec![];
        }
        let (rows, range) = (&self.tables[t], (bound(&s.lo), bound(&s.hi)));
        match s.index {
            None => ordered(rows.rows.range::<[u8], _>(range), s.dir)
                .take(s.limit)
                .map(|(_, r)| r.clone())
                .collect(),
            Some(i) => {
                let Some(ix) = rows.indexes.get(i) else { return vec![] };
                ordered(ix.range::<[u8], _>(range), s.dir)
                    .take(s.limit)
                    .map(|(_, k)| rows.rows[k].clone())
                    .collect()
            }
        }
    }
}
