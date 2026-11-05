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
//! invariant is the sequence: a row is added at the head only when it
//! follows it, removed only at either end, never edited. On an authority
//! it is also the chain ([`Log::chained`]); a replica holds the hashes it
//! was sent. The store's rows with the log's are what a peer makes durable
//! ([`Store::kv_rows`]).

use std::collections::BTreeMap;
use std::ops::Bound;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::format::Encode;
use crate::hash::{add256, sha256, sub256, EntryHash, SchemaHash, StateHash};
use crate::log::{Logged, Rows as KvRows, Seq};
use crate::schema::{Parsed, Schema, Space, Table};

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

    /// The same change, each row as `f` makes it.
    pub fn map(&self, mut f: impl FnMut(&[u8]) -> Vec<u8>) -> Change {
        let table = self.table().to_string();
        match self {
            Change::Add { row, .. } => Change::Add { table, row: f(row) },
            Change::Remove { row, .. } => Change::Remove { table, row: f(row) },
            Change::Edit { old, new, .. } => Change::Edit {
                table,
                old: f(old),
                new: f(new),
            },
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
    /// The log: the row it stands on, then its entries, in order.
    log: Vec<Logged>,
}

impl PartialEq for Store {
    fn eq(&self, other: &Store) -> bool {
        self.shash == other.shash && self.space == other.space && self.tables == other.tables
    }
}
impl Eq for Store {}

/// The log, read (§1.6): the row it stands on, then its entries, one per
/// sequence.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Log<'a>(&'a [Logged]);

impl<'a> Log<'a> {
    /// The row the log stands on.
    pub fn base(&self) -> Logged {
        self.0[0].clone()
    }

    pub fn base_seq(&self) -> Seq {
        self.0[0].seq
    }

    /// The last sequence and the chain's hash there.
    pub fn head(&self) -> (Seq, EntryHash) {
        let l = self.0.last().expect("a log stands on a row");
        (l.seq, l.hash)
    }

    fn index(&self, seq: Seq) -> Option<usize> {
        usize::try_from(seq - self.base_seq()).ok().filter(|i| *i < self.0.len())
    }

    /// The entry at `seq`, above the row the log stands on.
    pub fn at(&self, seq: Seq) -> Option<Logged> {
        self.index(seq).filter(|i| *i > 0).map(|i| self.0[i].clone())
    }

    /// The chain's hash at `seq`, the row the log stands on or above it.
    pub fn hash_at(&self, seq: Seq) -> Option<EntryHash> {
        self.index(seq).map(|i| self.0[i].hash)
    }

    /// Whether each entry links onto the row before it: the chain, as an
    /// authority holds it (a replica holds the hashes it was sent).
    pub fn chained(&self) -> bool {
        self.0.windows(2).all(|w| w[1].links(&w[0].hash))
    }

    /// The entries after `seq`, in order.
    pub fn after(&self, seq: Seq) -> impl Iterator<Item = Logged> + 'a {
        let from = self.index(seq.max(self.base_seq())).map_or(self.0.len(), |i| i + 1);
        self.0[from..].iter().cloned()
    }

    /// Every entry above the row the log stands on.
    pub fn entries(&self) -> Vec<Logged> {
        self.0[1..].to_vec()
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
            log: vec![],
        }
    }

    /// The log, as its table holds it.
    pub fn log(&self) -> Log<'_> {
        Log(&self.log)
    }

    /// The log emptied and these rows, in order, written to it: each after
    /// the first the sequence after the one before it (§1.6).
    pub fn start_log(&mut self, rows: impl IntoIterator<Item = Vec<u8>>) -> Result<(), String> {
        self.log.clear();
        for row in rows {
            let l = Logged::from_bytes(&row)?;
            if let Some(b) = self.log.last().filter(|b| l.seq != b.seq + 1) {
                return Err(format!("{LOG}: entry {} does not follow {}", l.seq, b.seq));
            }
            self.log.push(l);
        }
        Ok(())
    }

    /// A logged entry appended, as a peer opens its log (§1.6): the entry
    /// must follow the head, and its facts apply ([`Store::apply_facts`]).
    pub fn append(&mut self, l: &Logged) -> Result<(), String> {
        if l.seq != self.log().head().0 + 1 {
            return Err(format!("{LOG}: entry {} does not follow the head", l.seq));
        }
        self.apply_facts(&l.facts)?;
        self.log_push(l);
        Ok(())
    }

    /// Facts as some schema this one grows from or into wrote them, applied,
    /// all or none (§1.2): as they are, or else each row read under this
    /// one ([`Table::fit`]) and the changes to a table it lacks dropped.
    /// What was applied.
    pub fn apply_facts(&mut self, facts: &[Change]) -> Result<Vec<Change>, String> {
        if self.apply_all(facts).is_ok() {
            return Ok(facts.to_vec());
        }
        let fit = |c: &Change| {
            let (_, t) = self.schema.table(c.table())?;
            Some(c.map(|r| t.fit(r).unwrap_or_else(|_| r.to_vec())))
        };
        let fitted: Vec<Change> = facts.iter().filter_map(fit).collect();
        self.apply_all(&fitted).map(|_| fitted)
    }

    /// An entry added at the head of the log by a peer that chained it
    /// onto the head, or checked that it links there.
    pub(crate) fn log_push(&mut self, l: &Logged) {
        self.log.push(l.clone());
    }

    /// The horizon: the log's rows below `seq` deleted, so that it stands
    /// on the row there.
    pub fn log_horizon(&mut self, seq: Seq) {
        if let Some(i) = self.log().index(seq) {
            self.log.drain(..i);
        }
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

    fn find(&self, table: &str) -> Result<usize, String> {
        self.schema
            .table(table)
            .map(|(i, _)| i)
            .ok_or_else(|| format!("no table {table}"))
    }

    /// A table's rows in key order; none for a table the schema lacks.
    pub fn rows(&self, table: &str) -> impl Iterator<Item = &[u8]> {
        self.find(table)
            .ok()
            .into_iter()
            .flat_map(move |t| self.tables[t].rows.values().map(|r| &r[..]))
    }

    /// How many rows its tables hold, the log's not among them.
    pub fn held(&self) -> usize {
        self.tables.iter().map(|t| t.rows.len()).sum()
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

    /// A row's key, and its entry in each of its table's indexes: what a
    /// read through the key or an index covers of it (§1.10).
    pub fn paths_of(&self, table: &str, row: &[u8]) -> Result<(Vec<u8>, Vec<Vec<u8>>), String> {
        let p = self.schema.tables[self.find(table)?].parse(row)?;
        Ok((p.key, p.entries))
    }

    /// A row's key.
    pub fn key_of(&self, table: &str, row: &[u8]) -> Result<Vec<u8>, String> {
        Ok(self.schema.tables[self.find(table)?].parse(row)?.key)
    }

    /// Applies one change, judging it: the row before must be what the
    /// change says, the row after canonical and within every constraint.
    pub fn apply(&mut self, c: &Change) -> Result<(), String> {
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
            (Some(n), Some(o)) if n.key != o.key => return Err(format!("{}: an edit keeps its key", tab.name)),
            (Some(n), _) => &n.key,
            (None, Some(o)) => &o.key,
            _ => unreachable!(),
        };
        match (self.tables[t].rows.get(key), old) {
            (Some(have), Some(o)) if have == o => {}
            (None, None) => {}
            (Some(_), None) => return Err(format!("{}: a row with this key exists", tab.name)),
            (None, Some(_)) => return Err(format!("{}: no row has this key", tab.name)),
            (Some(_), Some(_)) => return Err(format!("{}: the row is not the one the change replaces", tab.name)),
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
    pub fn apply_all(&mut self, cs: &[Change]) -> Result<(), String> {
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
    pub fn undo<'c, I>(&mut self, cs: I) -> Result<Vec<Change>, String>
    where
        I: IntoIterator<Item = &'c Change>,
        I::IntoIter: DoubleEndedIterator,
    {
        cs.into_iter().rev().map(|c| c.inverse()).map(|c| self.apply(&c).map(|_| c)).collect()
    }

    /// Puts a row, adding it or replacing the one under its key.
    pub fn put(&mut self, table: &str, row: Vec<u8>) -> Result<Change, String> {
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
    pub fn delete(&mut self, table: &str, key: &[u8]) -> Result<Change, String> {
        let t = self.find(table)?;
        let row = self.tables[t]
            .rows
            .get(key)
            .cloned()
            .ok_or_else(|| format!("{table}: no row has this key"))?;
        let c = Change::Remove { table: table.into(), row };
        self.apply(&c).map(|_| c)
    }

    // The constraints on a row about to be written under its key.
    fn judge(&self, tab: &Table, p: &Parsed) -> Result<(), String> {
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
                return Err(format!("{}: another row has this ({})", tab.name, names.join(", ")));
            }
        }
        for r in &tab.refs {
            let Some(k) = p.named(tab, r.column)? else { continue };
            if !self.tables[self.find(&r.table)?].rows.contains_key(&k) {
                return Err(format!(
                    "{}.{}: no {} has this key",
                    tab.name, tab.columns[r.column].name, r.table
                ));
            }
        }
        Ok(())
    }

    // No row names this one, about to be removed, by a reference: through an
    // index the referring column leads, or the key when it leads it, or
    // else by reading the referring table.
    fn unnamed(&self, tab: &Table, key: &[u8]) -> Result<(), String> {
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
                    return Err(format!("{}: named by {}.{c}", tab.name, rt.name));
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
        rows.push((LOG.into(), self.log.iter().map(Encode::to_bytes).collect()));
        rows
    }

    /// A store of `space` holding exactly these rows, as some schema this
    /// one grows from or into wrote them ([`Table::fit`]), each parsed once
    /// unless fitted; constraints judged once all are in, so the order
    /// tables come in does not matter.
    pub fn from_rows(schema: Arc<Schema>, space: Space, rows: &[(String, Vec<Vec<u8>>)]) -> Result<Store, String> {
        let mut s = Store::new(schema.clone(), space);
        let mut parsed: Vec<Vec<Parsed>> = schema.tables.iter().map(|_| vec![]).collect();
        for (table, rs) in rows {
            // A table this schema lacks is a newer one's: none of its.
            let Ok(t) = s.find(table) else { continue };
            for row in rs {
                let (row, p) = schema.tables[t].parse_fit(row)?;
                if s.tables[t].rows.contains_key(&p.key) {
                    return Err(format!("{table}: two rows have one key"));
                }
                let key = p.key.clone();
                s.write(t, &key, None, Some((&row, &p)));
                parsed[t].push(p);
            }
        }
        for (t, tab) in schema.tables.iter().enumerate() {
            for p in &parsed[t] {
                s.judge(tab, p)?;
            }
        }
        Ok(s)
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

    /// `rows` of `table` put, each under its key, judging nothing: how a
    /// replica filling holds the rows it was sent, whose neighbours it may
    /// not hold yet. What moved, as changes.
    pub(crate) fn put_rows(&mut self, table: &str, rows: &[Vec<u8>]) -> Result<Vec<Change>, String> {
        rows.iter().filter_map(|r| self.force_row(table, r, false).transpose()).collect()
    }

    /// Changes applied as they are, judging nothing but each row's bytes:
    /// an `Add` or an `Edit` puts its row, a `Remove` takes its key's away,
    /// whatever was held — how a replica filling takes an entry's facts
    /// over rows it may not all hold yet. What moved, as changes.
    pub(crate) fn force_all(&mut self, cs: &[Change]) -> Result<Vec<Change>, String> {
        let force = |c: &Change| {
            let (row, gone) = match c {
                Change::Add { row, .. } | Change::Edit { new: row, .. } => (row, false),
                Change::Remove { row, .. } => (row, true),
            };
            self.force_row(c.table(), row, gone).transpose()
        };
        cs.iter().filter_map(force).collect()
    }

    // A row as some schema of this one wrote it ([`Table::fit`]) written
    // under its key — or, `gone`, its key's row taken away — judging
    // nothing; a table this schema lacks is none of its.
    fn force_row(&mut self, table: &str, row: &[u8], gone: bool) -> Result<Option<Change>, String> {
        let Ok(t) = self.find(table) else { return Ok(None) };
        let (row, p) = self.schema.tables[t].parse_fit(row)?;
        let key = p.key.clone();
        self.force(t, &key, (!gone).then_some((row, p)))
    }

    // A key's row written as `row`, parsed already (none: gone), judging
    // nothing; the change, when one was made.
    fn force(&mut self, t: usize, key: &[u8], new: Option<(Vec<u8>, Parsed)>) -> Result<Option<Change>, String> {
        let schema = self.schema.clone();
        let tab = &schema.tables[t];
        let old = self.tables[t].rows.get(key).cloned();
        if old.as_ref() == new.as_ref().map(|(r, _)| r) {
            return Ok(None);
        }
        let po = old.as_ref().map(|o| tab.parse_held(o)).transpose()?;
        let (row, pn) = new.unzip();
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
