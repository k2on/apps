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
//! modulo 2^256 of `sha256(enc(Bytes table) ‖ row)` over its rows, moved by
//! one leaf out and one in per write; the state hash is
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
//! keyed by sequence, each row a [`Logged`]'s bytes. It is not the
//! schema's — not in its hash or the state hash, not in a snapshot sent,
//! not a table a function names, not in a store's equality — and its
//! invariant is the chain: a row is added at either end only when it
//! links onto its neighbour, removed only at either end, never edited
//! ([`Store::apply_kv`]). The store's rows with the log's are what a peer
//! makes durable ([`Store::kv_rows`]).

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;
use std::sync::Arc;

use ark_macros::Node;

use crate::codec::Node;
use crate::codec::{bytes_enc, int_enc, skip, Reader};
use crate::hash::{add256, sha256, sub256, EntryHash, SchemaHash, StateHash};
use crate::log::{Entry, Logged, Rows as KvRows, Seq};
use crate::schema::{Schema, Space, Table};
use crate::ty::Ty;
use crate::Refusal;

/// The log's table (§1.6).
pub const LOG: &str = "_log";

/// One write, as the store reports it and the log keeps it (a fact).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Node)]
pub enum Change {
    Add { table: String, row: Vec<u8> },
    Remove { table: String, row: Vec<u8> },
    Edit { table: String, old: Vec<u8>, new: Vec<u8> },
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

    /// `Sum [(table, row), (table, row), (table, old, new)]`, rows as
    /// `Bytes` so a change reads without its schema (derived).
    pub fn enc(&self, out: &mut Vec<u8>) {
        self.put(out);
    }

    pub fn dec(r: &mut Reader) -> Result<Change, Refusal> {
        Change::take(r)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Node)]
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

/// Each column's span in a row ([`Table::spans`]).
type Spans<'a> = &'a [(usize, usize)];

#[derive(Clone, Debug, PartialEq, Eq)]
struct Rows {
    /// `enc(Bytes name)`, the first half of every leaf.
    name: Vec<u8>,
    rows: BTreeMap<Vec<u8>, Vec<u8>>,
    indexes: Vec<BTreeSet<Vec<u8>>>,
    digest: [u8; 32],
}

#[derive(Clone, Debug)]
pub struct Store {
    schema: Arc<Schema>,
    shash: SchemaHash,
    tables: Vec<Rows>,
    space: Space,
    /// The log's table: sequence's bytes to the row.
    log: BTreeMap<Vec<u8>, Vec<u8>>,
}

impl PartialEq for Store {
    fn eq(&self, other: &Store) -> bool {
        self.shash == other.shash && self.space == other.space && self.tables == other.tables
    }
}
impl Eq for Store {}

/// The log as its table holds it, read (§1.6). Every peer's log stands on
/// a first row; the entries are the rows after it.
#[derive(Clone, Copy)]
pub struct Log<'a>(&'a BTreeMap<Vec<u8>, Vec<u8>>);

impl PartialEq for Log<'_> {
    fn eq(&self, other: &Log) -> bool {
        self.0 == other.0
    }
}

impl std::fmt::Debug for Log<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_list().entries(self.0.values().map(|b| read(b))).finish()
    }
}

fn read(b: &[u8]) -> Logged {
    Logged::from_bytes(b).expect("a row of the log reads")
}

/// A row's sequence, state and hash, read past its entry and facts.
fn ends(b: &[u8]) -> (Seq, StateHash, EntryHash) {
    static TYS: std::sync::OnceLock<(Ty, Ty)> = std::sync::OnceLock::new();
    let (entry, facts) = TYS.get_or_init(|| (Entry::ty(), Change::list_ty()));
    let mut r = Reader::new(b);
    let mut take = || -> Result<_, Refusal> {
        let seq = r.int()?;
        skip(entry, &mut r)?;
        skip(facts, &mut r)?;
        let h = |b: Vec<u8>| <[u8; 32]>::try_from(b).map_err(|_| Refusal::new("not a hash"));
        Ok((seq, StateHash(h(r.bytes()?)?), EntryHash(h(r.bytes()?)?)))
    };
    take().expect("a row of the log reads")
}

impl<'a> Log<'a> {
    /// The row the log stands on.
    pub fn base(&self) -> Logged {
        read(self.0.values().next().expect("a log stands on a row"))
    }

    pub fn base_seq(&self) -> Seq {
        ends(self.0.values().next().expect("a log stands on a row")).0
    }

    fn last(&self) -> (Seq, StateHash, EntryHash) {
        ends(self.0.values().next_back().expect("a log stands on a row"))
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
        (seq > self.base_seq()).then(|| self.0.get(&int_enc(seq)[..]).map(|b| read(b))).flatten()
    }

    /// Whether this log has `hash` at `seq`: one history up to there.
    pub fn has(&self, seq: Seq, hash: &EntryHash) -> bool {
        self.0.get(&int_enc(seq)[..]).is_some_and(|b| ends(b).2 == *hash)
    }

    /// The entries after `seq`, in order.
    pub fn after(&self, seq: Seq) -> impl Iterator<Item = Logged> + 'a {
        let from = int_enc(seq.max(self.base_seq())).to_vec();
        self.0.range((Bound::Excluded(from), Bound::Unbounded)).map(|(_, b)| read(b))
    }

    /// Every entry above the row the log stands on.
    pub fn entries(&self) -> Vec<Logged> {
        self.after(Seq::MIN).collect()
    }

    /// How many entries there are above the row the log stands on.
    pub fn len(&self) -> usize {
        self.0.len() - 1
    }

    pub fn is_empty(&self) -> bool {
        self.0.len() <= 1
    }
}

/// The entries a row has in its table's indexes.
fn entries(tab: &Table, row: &[u8], spans: &[(usize, usize)], key: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::with_capacity(tab.indexes.len());
    for ix in &tab.indexes {
        let mut e = Vec::new();
        for &c in &ix.columns {
            e.extend_from_slice(&row[spans[c].0..spans[c].1]);
        }
        e.extend_from_slice(key);
        out.push(e);
    }
    out
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
    /// An empty durable store.
    pub fn new(schema: Arc<Schema>) -> Store {
        Store::new_in(schema, Space::Durable)
    }

    /// An empty store whose hash and snapshot are over `space`'s tables.
    pub fn new_in(schema: Arc<Schema>, space: Space) -> Store {
        let tables = schema
            .tables
            .iter()
            .map(|t| Rows {
                name: bytes_enc(t.name.as_bytes()),
                rows: BTreeMap::new(),
                indexes: vec![BTreeSet::new(); t.indexes.len()],
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
        let key = int_enc(l.seq).to_vec();
        let (first, last) = (self.log.first_key_value(), self.log.last_key_value());
        let ok = match c {
            Change::Add { .. } => match (first, last) {
                (Some((_, f)), Some((_, b))) => {
                    let (n, _, h) = ends(b);
                    l.seq == n + 1 && l.links(&h) || read(f).seq == l.seq + 1 && read(f).links(&l.hash)
                }
                _ => true,
            },
            _ => self.log.get(&key) == Some(row) && (first.map(|f| f.0) == Some(&key) || last.map(|b| b.0) == Some(&key)),
        };
        if !ok {
            return Err(Refusal(format!("{LOG}: entry {} is not an end of the chain", l.seq)));
        }
        match c {
            Change::Add { .. } => self.log.insert(key, row.clone()),
            _ => self.log.remove(&key),
        };
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
        self.log_push(l.seq, l.to_bytes());
        Ok(())
    }

    /// A row added at the head of the log by a peer that chained it onto
    /// the head, or checked that it links there.
    pub(crate) fn log_push(&mut self, seq: Seq, row: Vec<u8>) {
        self.log.insert(int_enc(seq).to_vec(), row);
    }

    /// The log's rows above `seq` deleted, newest first.
    pub fn log_truncate(&mut self, seq: Seq) {
        self.log.split_off(&int_enc(seq + 1)[..]);
    }

    /// The horizon: the log's rows below `seq` deleted, so that it stands
    /// on the row there.
    pub fn log_horizon(&mut self, seq: Seq) {
        self.log = self.log.split_off(&int_enc(seq)[..]);
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

    /// `sha256(enc(Bytes table) ‖ row)`, read as a 256-bit integer.
    pub fn leaf(table: &str, row: &[u8]) -> [u8; 32] {
        sha256(&[&bytes_enc(table.as_bytes()), row])
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

    /// The state hash the store would have with `undone` undone, by the
    /// digests' arithmetic alone: what a replica checks a confirmed state
    /// by while its pending changes stay applied.
    pub fn hash_beside<'a>(&self, undone: impl IntoIterator<Item = &'a Change>) -> StateHash {
        let mut ds: Vec<[u8; 32]> = self.tables.iter().map(|r| r.digest).collect();
        for c in undone {
            let Ok(t) = self.find(c.table()) else { continue };
            let d = &mut ds[t];
            match c {
                Change::Add { table, row } => sub256(d, &Store::leaf(table, row)),
                Change::Remove { table, row } => add256(d, &Store::leaf(table, row)),
                Change::Edit { table, old, new } => {
                    sub256(d, &Store::leaf(table, new));
                    add256(d, &Store::leaf(table, old));
                }
            }
        }
        self.hash_of(ds.into_iter())
    }

    /// A row's key, and its entry in each of its table's indexes: what a
    /// read through the key or an index covers of it (§1.10).
    pub fn paths_of(&self, table: &str, row: &[u8]) -> Result<(Vec<u8>, Vec<Vec<u8>>), Refusal> {
        let tab = &self.schema.tables[self.find(table)?];
        let spans = tab.spans(row)?;
        let key = row[..spans[tab.key - 1].1].to_vec();
        let es = entries(tab, row, &spans, &key);
        Ok((key, es))
    }

    /// A row's key.
    pub fn key_of(&self, table: &str, row: &[u8]) -> Result<Vec<u8>, Refusal> {
        let tab = &self.schema.tables[self.find(table)?];
        Ok(row[..tab.key_len(row)?].to_vec())
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
        let spans = new.map(|r| tab.spans(r)).transpose()?;
        let key = match (old, new, &spans) {
            (_, Some(n), Some(s)) => &n[..s[tab.key - 1].1],
            (Some(o), _, _) => &o[..tab.key_len(o)?],
            _ => unreachable!(),
        };
        if let (Some(o), Some(_)) = (old, new) {
            if !o.starts_with(key) || tab.key_len(o)? != key.len() {
                return Err(Refusal(format!("{}: an edit keeps its key", tab.name)));
            }
        }
        match (self.tables[t].rows.get(key), old) {
            (Some(have), Some(o)) if have == o => {}
            (None, None) => {}
            (Some(_), None) => return Err(Refusal(format!("{}: a row with this key exists", tab.name))),
            (None, Some(_)) => return Err(Refusal(format!("{}: no row has this key", tab.name))),
            (Some(_), Some(_)) => return Err(Refusal(format!("{}: the row is not the one the change replaces", tab.name))),
        }
        if let (Some(n), Some(s)) = (new, &spans) {
            self.judge(tab, n, s, key)?;
        } else {
            self.unnamed(tab, key)?;
        }
        let key = key.to_vec();
        self.write(tab, t, &key, old.map(|o| &o[..]), new.map(|n| &n[..]).zip(spans.as_deref()));
        Ok(())
    }

    /// Applies changes in order, all or none.
    pub fn apply_all(&mut self, cs: &[Change]) -> Result<(), Refusal> {
        for (i, c) in cs.iter().enumerate() {
            if let Err(e) = self.apply(c) {
                for d in cs[..i].iter().rev() {
                    self.apply(&d.inverse()).expect("an inverse applies");
                }
                return Err(e);
            }
        }
        Ok(())
    }

    /// Puts a row, adding it or replacing the one under its key.
    pub fn put(&mut self, table: &str, row: Vec<u8>) -> Result<Change, Refusal> {
        let t = self.find(table)?;
        let key = &row[..self.schema.tables[t].key_len(&row)?];
        let c = match self.tables[t].rows.get(key) {
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

    // The constraints on a row about to be written under `key`.
    fn judge(&self, tab: &Table, row: &[u8], spans: &[(usize, usize)], key: &[u8]) -> Result<(), Refusal> {
        let t = self.find(&tab.name)?;
        for (i, (ix, e)) in tab.indexes.iter().zip(entries(tab, row, spans, key)).enumerate() {
            let cols = &e[..e.len() - key.len()];
            let clash = ix.unique
                && self.tables[t].indexes[i]
                    .range::<[u8], _>((Bound::Included(cols), Bound::Unbounded))
                    .take_while(|x| x.starts_with(cols))
                    .any(|x| &x[cols.len()..] != key);
            if clash {
                let names: Vec<&str> = ix.columns.iter().map(|c| &tab.columns[*c].name[..]).collect();
                return Err(Refusal(format!("{}: another row has this ({})", tab.name, names.join(", "))));
            }
        }
        for r in &tab.refs {
            let (s, e) = spans[r.column];
            let mut b = &row[s..e];
            if tab.columns[r.column].ty.is_option() {
                if b[0] == 0 {
                    continue;
                }
                b = &b[1..];
            }
            if !self.tables[self.find(&r.table)?].rows.contains_key(b) {
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
                    Some(i) => first(&mut rows.indexes[i].range::<[u8], _>(from)),
                    None if r.column == 0 => first(&mut rows.rows.range::<[u8], _>(from).map(|(k, _)| k)),
                    None => rows.rows.values().any(|row| {
                        let s = rt.spans(row).expect("a stored row parses");
                        row[s[r.column].0..s[r.column].1] == want[..]
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
    fn write(&mut self, tab: &Table, t: usize, key: &[u8], old: Option<&[u8]>, new: Option<(&[u8], Spans)>) {
        let rows = &mut self.tables[t];
        if let Some(o) = old {
            let spans = tab.spans(o).expect("a stored row parses");
            for (set, e) in rows.indexes.iter_mut().zip(entries(tab, o, &spans, key)) {
                set.remove(&e);
            }
            sub256(&mut rows.digest, &sha256(&[&rows.name, o]));
            rows.rows.remove(key);
        }
        if let Some((n, spans)) = new {
            for (set, e) in rows.indexes.iter_mut().zip(entries(tab, n, spans, key)) {
                set.insert(e);
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
        rows.push((LOG.into(), self.log.values().cloned().collect()));
        rows
    }

    /// A store holding exactly these rows, each parsed; constraints are
    /// judged once all are in, so the order tables come in does not matter.
    pub fn from_rows(schema: Arc<Schema>, rows: &[(String, Vec<Vec<u8>>)]) -> Result<Store, Refusal> {
        Store::from_rows_in(schema, Space::Durable, rows)
    }

    /// [`Store::from_rows`], a store of `space`.
    pub fn from_rows_in(schema: Arc<Schema>, space: Space, rows: &[(String, Vec<Vec<u8>>)]) -> Result<Store, Refusal> {
        let mut s = Store::new_in(schema.clone(), space);
        for (table, rs) in rows {
            let t = s.find(table)?;
            let tab = &schema.tables[t];
            for row in rs {
                let spans = tab.spans(row)?;
                let key = row[..spans[tab.key - 1].1].to_vec();
                if s.tables[t].rows.contains_key(&key) {
                    return Err(Refusal(format!("{table}: two rows have one key")));
                }
                s.write(tab, t, &key, None, Some((row, &spans)));
            }
        }
        for (t, tab) in schema.tables.iter().enumerate() {
            for (key, row) in &s.tables[t].rows {
                let spans = tab.spans(row)?;
                s.judge(tab, row, &spans, key)?;
            }
        }
        Ok(s)
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
                let tab = &self.schema.tables[t];
                let Some(ix) = tab.indexes.get(i) else { return vec![] };
                ordered(rows.indexes[i].range::<[u8], _>(range), s.dir)
                    .take(s.limit)
                    .map(|e| {
                        let mut r = Reader::new(e);
                        for &c in &ix.columns {
                            skip(&tab.columns[c].ty, &mut r).expect("an index entry parses");
                        }
                        rows.rows[&e[r.at..]].clone()
                    })
                    .collect()
            }
        }
    }
}
