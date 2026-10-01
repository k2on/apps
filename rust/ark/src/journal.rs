//! A log kept on a key/value storage: a snapshot, and pages of records
//! after it (§10; `docs/plan-alone.md` §2).
//!
//! ```text
//! snapshot  canonical CBOR of `log_to_value`:
//!           { t: "log",
//!             base: { seq: Int, hash: Bytes, hashing: Int,
//!                     rows: { table: [row…] }, log: Id },
//!             entries: [ { seq: Int, entry: Entry, facts: [Change…] } … ],
//!             ids: [ { id: Id, seq: Int } …,             exact; then
//!                    { key: Bytes(8), seq: Int } … ] }   below the horizon
//! page      records one after another, each a 4-byte big-endian length
//!           and then the canonical CBOR of { seq, entry, facts } — one item
//!           of the snapshot's `entries`, and of a `Batch` frame's
//! ```
//!
//! The shapes reuse the protocol's own encoders for an entry and a change,
//! so a log on a disk carries exactly what a `Batch` frame would. Two
//! programs keep a log this way, and differ only in [`Layout`]:
//!
//! - **the server** (`ark-server`'s `persist`): the snapshot is
//!   `log.ark-log`, and there is one page, `log.ark-journal`, appended to
//!   and synced once per batch of appends ([`Paging::Append`]). Compaction
//!   rewrites the snapshot at the head, entries and all, and empties the
//!   page once the page is larger than the snapshot
//!   ([`Compaction::Snapshot`]) — the client's rule: a snapshot of *S*
//!   bytes is written only after at least *S* bytes of records, so the
//!   bytes written per append stay within a small multiple of the record's.
//! - **a peer alone** (`ark-client`'s alone log): the snapshot is `log`,
//!   and it is the **fork** — the store the peer last shared with a server,
//!   at that server's cursor, or the empty store at 0 — with no entries.
//!   Each write is a page of its own, `log.1`, `log.2`, … saved whole
//!   ([`Paging::Pages`]), because a browser's `localStorage` can replace a
//!   value and cannot append to one. Compaction merges pages into fewer
//!   pages and never touches the snapshot ([`Compaction::Merge`]): the
//!   entries above the fork are the local history a later join re-queues,
//!   so nothing may be folded into a base above the fork. Two neighbours
//!   are merged whenever the older is no larger than the newer, which
//!   keeps the pages a binary counter — about log₂ of the writes of them,
//!   each record rewritten about as many times — rather than one merged
//!   page rewritten whole per write.
//!
//! **What a stop leaves behind is safe, by the order of the writes.**
//! Records are read snapshot first, then page 1, 2, … until one is absent
//! (a storage cannot list its keys). Within a page, records at or below the
//! head *before any that extend it* are ones a compaction already holds and
//! are skipped: the server's snapshot renamed into place before its page
//! was emptied, or a merged page saved before the newer page it absorbed
//! was removed. The first record that is short, does not decode, or does
//! not carry the next sequence is where the log ends — a torn write — and
//! it and everything after it are dropped, never applied out of order.
//! [`Journal::open`] repairs what it read so that the next write follows
//! on: a torn page is cut back to its whole records, pages after it and
//! trailing pages wholly skipped are removed newest first, and whether
//! skipped records call for a snapshot is the caller's to say
//! ([`Opened::stale`]), since only it holds the log whole. Nothing is
//! written for a storage whose records are whole.

use std::collections::BTreeMap;

use crate::canon;
use crate::hash::{state_hash_by, HASH_VERSION};
use crate::log::{snapshot_of, Entry, Facts, Log, Seq};
use crate::protocol::{change_from_value, change_value, entry_from_value, entry_value};
use crate::schema::Schema;
use crate::store::{Change, MemoryStore, Row, Store};
use crate::value::{FieldName, Id, Value};

/// A key/value place for a log's records: what the server's directory and
/// a peer's `ark_client::storage::Storage` both are. `save` replaces its
/// record whole or not at all, which is what the rules above rest on.
pub trait Keys {
    fn load(&self, key: &str) -> Result<Option<Vec<u8>>, String>;
    fn save(&mut self, key: &str, bytes: &[u8]) -> Result<(), String>;
    fn remove(&mut self, key: &str) -> Result<(), String>;

    /// Add `bytes` to the end of `key`, creating it, durable when this
    /// returns. By default the record is rewritten whole, which is right
    /// for a storage that cannot append; a file appends
    /// ([`Paging::Append`]).
    fn append(&mut self, key: &str, bytes: &[u8]) -> Result<(), String> {
        let mut all = self.load(key)?.unwrap_or_default();
        all.extend_from_slice(bytes);
        self.save(key, &all)
    }

    /// Cut `key` to its first `len` bytes. By default, rewritten whole.
    fn truncate(&mut self, key: &str, len: usize) -> Result<(), String> {
        let all = self.load(key)?.unwrap_or_default();
        self.save(key, &all[..len.min(all.len())])
    }
}

/// How records reach a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paging {
    /// One page, appended to: a file.
    Append,
    /// A page per write, each saved whole under `{page}.{n}`: a storage
    /// that can only replace a value.
    Pages,
}

/// What a compaction does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compaction {
    /// Rewrite the snapshot at the head, entries and all, and empty the
    /// pages, once they are larger than the snapshot: the server.
    Snapshot,
    /// Merge pages into fewer pages; the snapshot stays where it is: a peer
    /// alone, whose snapshot is its fork.
    Merge,
}

/// Where a log's records are kept, and how they move.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    /// The snapshot's key.
    pub snapshot: String,
    /// The page's key ([`Paging::Append`]) or the prefix of the pages'
    /// ([`Paging::Pages`]: `{page}.{n}`, from 1).
    pub page: String,
    pub paging: Paging,
    pub compaction: Compaction,
}

impl Layout {
    /// The server's data directory: `log.ark-log` and `log.ark-journal`.
    pub fn server() -> Layout {
        Layout {
            snapshot: "log.ark-log".into(),
            page: "log.ark-journal".into(),
            paging: Paging::Append,
            compaction: Compaction::Snapshot,
        }
    }

    /// A peer alone's local history: `log`, then `log.1`, `log.2`, …
    pub fn alone() -> Layout {
        Layout {
            snapshot: "log".into(),
            page: "log".into(),
            paging: Paging::Pages,
            compaction: Compaction::Merge,
        }
    }

    /// The key of the `n`th page, counting from 1.
    pub fn page_key(&self, n: usize) -> String {
        match self.paging {
            Paging::Append => self.page.clone(),
            Paging::Pages => format!("{}.{n}", self.page),
        }
    }
}

// -- the records -----------------------------------------------------------

/// One entry as the snapshot and the pages both carry it.
pub fn record_value(n: Seq, e: &Entry, facts: &Facts) -> Value {
    Value::record(vec![
        ("seq", Value::int(n)),
        ("entry", entry_value(e)),
        ("facts", Value::list(facts.iter().map(change_value).collect())),
    ])
}

/// A page record's bytes: the length, then the record.
pub fn encode_record(n: Seq, e: &Entry, facts: &Facts) -> Vec<u8> {
    let body = canon::encode(&record_value(n, e, facts));
    let mut out = Vec::with_capacity(body.len() + 4);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(&body);
    out
}

/// A log as a value; the snapshot is `canon::encode` of this.
pub fn log_to_value(log: &Log) -> Value {
    let store = &log.base.store;
    let rows: Vec<(String, Value)> = store
        .table_names()
        .into_iter()
        .map(|t| {
            let rs = store.scan(&t).into_iter().map(Row::into_value).collect();
            (t, Value::list(rs))
        })
        .collect();
    let entries: Vec<Value> = log.entries.iter().map(|(n, (e, f))| record_value(*n, e, f)).collect();
    // Exact ids, then the keys of those below the horizon (`Log::below`,
    // `docs/plan-db.md` D6): `{ key: Bytes(8), seq }`.
    let ids: Vec<Value> = log
        .ids
        .iter()
        .map(|(id, n)| Value::record(vec![("id", Value::Id(*id)), ("seq", Value::int(*n))]))
        .chain(
            log.below
                .iter()
                .map(|(k, n)| Value::record(vec![("key", Value::bytes(k.to_be_bytes().to_vec())), ("seq", Value::int(n))])),
        )
        .collect();
    let mut base = vec![
        ("seq", Value::int(log.base.seq)),
        ("hash", Value::bytes(log.base.hash.clone())),
        ("hashing", Value::int(HASH_VERSION)),
        ("rows", Value::record(rows)),
    ];
    // Unnamed, it is written as a file was before logs had names.
    if let Some(id) = log.id() {
        base.push(("log", Value::Id(id)));
    }
    Value::record(vec![
        ("t", Value::text("log")),
        ("base", Value::record(base)),
        ("entries", Value::list(entries)),
        ("ids", Value::list(ids)),
    ])
}

fn fields(v: &Value) -> Result<&BTreeMap<FieldName, Value>, String> {
    match v {
        Value::Struct(m) => Ok(m),
        other => Err(format!("expected a struct, found {other:?}")),
    }
}

fn need<'a>(m: &'a BTreeMap<FieldName, Value>, k: &str) -> Result<&'a Value, String> {
    m.get(k).ok_or_else(|| format!("missing field {k}"))
}

fn int(v: &Value) -> Result<i64, String> {
    match v {
        Value::Int(n) => Ok(*n),
        other => Err(format!("expected an int, found {other:?}")),
    }
}

fn items(v: &Value) -> Result<&[Value], String> {
    match v {
        Value::List(xs) => Ok(xs),
        other => Err(format!("expected a list, found {other:?}")),
    }
}

/// One entry back from its value.
pub fn record_from_value(v: &Value) -> Result<(Seq, Entry, Facts), String> {
    let im = fields(v)?;
    let n = int(need(im, "seq")?)?;
    let e = entry_from_value(need(im, "entry")?).map_err(|e| format!("entry {n}: {e}"))?;
    let facts = items(need(im, "facts")?)?
        .iter()
        .map(change_from_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("facts of {n}: {e}"))?;
    Ok((n, e, facts))
}

/// Which construction a snapshot's `hash` is by ([`HASH_VERSION`]): its
/// `hashing`, 1 where it has none, as a snapshot written before there were
/// two does not. One this build does not know is refused rather than read
/// as either.
pub fn hashing_of(base: &BTreeMap<FieldName, Value>) -> Result<i64, String> {
    match base.get("hashing") {
        None => Ok(1),
        Some(Value::Int(n)) if (1..=HASH_VERSION).contains(n) => Ok(*n),
        Some(other) => Err(format!("a state hash by a construction this build does not know: {other:?}")),
    }
}

/// A log from its value, over the module's schema.
pub fn log_from_value(schema: &Schema, v: &Value) -> Result<Log, String> {
    let base = fields(need(fields(v)?, "base")?)?;
    let store = MemoryStore::from_value(schema.clone(), need(base, "rows")?);
    log_from_parts(v, store)
}

// [`log_from_value`], given the store its snapshot's rows make.
fn log_from_parts(v: &Value, store: MemoryStore) -> Result<Log, String> {
    let m = fields(v)?;
    match need(m, "t")? {
        Value::Text(t) if t == "log" => {}
        other => return Err(format!("not a log file: t = {other:?}")),
    }
    let base = fields(need(m, "base")?)?;
    need(base, "rows")?;
    let log_id = match base.get("log") {
        None => None,
        Some(Value::Id(i)) => Some(*i),
        Some(other) => return Err(format!("the log's identity is not an id: {other:?}")),
    };
    // Checked by the construction it was hashed by, and held hashed by
    // this one ([`HASH_VERSION`]): a snapshot written before `docs/plan-db.md`
    // D3 opens, its hash checked as it was written.
    let claimed = state_hash_by(hashing_of(base)?, &store);
    match need(base, "hash")? {
        Value::Bytes(h) if Some(h) == claimed.as_ref() => {}
        _ => return Err("the snapshot's hash does not match its rows".into()),
    }
    let snapshot = snapshot_of(int(need(base, "seq")?)?, store).of_log(log_id);
    let mut log = Log {
        base: snapshot,
        entries: BTreeMap::new(),
        ids: BTreeMap::new(),
        below: Default::default(),
    };
    for item in items(need(m, "entries")?)? {
        let (n, e, facts) = record_from_value(item)?;
        log.entries.insert(n, (e, facts));
    }
    let mut keys = vec![];
    for item in items(need(m, "ids")?)? {
        let im = fields(item)?;
        let n = int(need(im, "seq")?)?;
        match (im.get("id"), im.get("key")) {
            (Some(Value::Id(i)), _) => {
                log.ids.insert(*i, n);
            }
            (None, Some(Value::Bytes(k))) if k.len() == 8 => {
                keys.push((u64::from_be_bytes(k[..].try_into().expect("eight bytes")), n));
            }
            (other, key) => return Err(format!("expected an id or an 8-byte key, found {other:?} {key:?}")),
        }
    }
    log.below.extend(keys);
    if !log.contiguous() {
        return Err(format!(
            "the entries do not run without a gap from {} to {}",
            log.horizon() + 1,
            log.head_seq()
        ));
    }
    Ok(log)
}

/// The snapshot's bytes back, as a log.
pub fn decode_snapshot(schema: &Schema, bytes: &[u8]) -> Result<Log, String> {
    // The snapshot's rows are built as they are read (`docs/plan-db.md`
    // D7.4), into the store `log_from_value` would have built from them:
    // the same rows, applied raw, in the same order — and what is not a
    // list of rows ignored, as `MemoryStore::from_value` ignores it.
    let mut store = MemoryStore::empty(schema.clone());
    let v = canon::decode_rows(bytes, &["base", "rows"], &mut |t, fields| {
        let row = Row::from_fields(schema.lookup_table(t), fields);
        store.apply_change(&Change::Add(t.into(), row));
    })
    .map_err(|e| format!("decoding: {e}"))?;
    log_from_parts(&v, store)
}

/// What reading one page's records over a head found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Replayed {
    /// Records that extended the head.
    pub applied: usize,
    /// Records at or below the head, ahead of the rest: what a compaction
    /// that stopped before emptying or removing this page left.
    pub stale: usize,
    /// Where the good records end: the page's length, or the offset of the
    /// first record that is short, does not decode, or does not follow on.
    pub end: u64,
    /// The page's length as read.
    pub len: u64,
}

impl Replayed {
    /// Whether everything after `end` is a torn tail.
    pub fn torn(&self) -> bool {
        self.end < self.len
    }
}

/// Hand a page's records to `each`, in order, from the one after `head`,
/// stopping at the first that is short, does not decode, or does not carry
/// the next sequence (the module docs). `head` moves with them.
pub fn replay_page(head: &mut Seq, bytes: &[u8], mut each: impl FnMut(Seq, Entry, Facts)) -> Replayed {
    let mut out = Replayed {
        len: bytes.len() as u64,
        ..Replayed::default()
    };
    let mut at = 0usize;
    while at < bytes.len() {
        let Some(len) = bytes.get(at..at + 4) else { break };
        let n = u32::from_be_bytes([len[0], len[1], len[2], len[3]]) as usize;
        let Some(body) = bytes.get(at + 4..at + 4 + n) else { break };
        let Ok((seq, e, facts)) = canon::decode(body).map_err(|e| e.to_string()).and_then(|v| record_from_value(&v)) else {
            break;
        };
        if seq == *head + 1 {
            each(seq, e, facts);
            *head = seq;
            out.applied += 1;
        } else if seq <= *head && out.applied == 0 {
            out.stale += 1;
        } else {
            break;
        }
        at += 4 + n;
    }
    out.end = at as u64;
    out
}

/// Append a page's records to `log` ([`replay_page`]).
pub fn replay_into(log: &mut Log, bytes: &[u8]) -> Replayed {
    let mut head = log.head_seq();
    let mut got = vec![];
    let out = replay_page(&mut head, bytes, |_, e, f| got.push((e, f)));
    for (e, f) in got {
        log.append(e, f);
    }
    out
}

/// What reading a log's records found: the snapshot, and what the pages
/// after it came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opened {
    /// The snapshot as written, entries it holds included; `None` where
    /// there is none.
    pub snapshot: Option<Log>,
    /// The head the pages reached: the snapshot's where they added nothing.
    pub head: Seq,
    /// Records the pages added above the snapshot.
    pub applied: usize,
    /// Records skipped as ones a compaction already holds.
    pub stale: usize,
    /// Bytes dropped as a torn tail, and the page they were in.
    pub torn: Option<(usize, u64)>,
}

/// Every page as read: its key's number and its good length.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Walk {
    /// The good bytes of each page that extended the log, in order.
    good: Vec<u64>,
    /// Pages found after the good ones: wholly stale, torn, or after a tear.
    after: usize,
    /// The torn page's number and its good length, if one was.
    torn_at: Option<(usize, u64, u64)>,
    stale: usize,
    applied: usize,
    head: Seq,
}

/// A log open for writing: what the storage holds of it, so that a write
/// adds what moved since and nothing else.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Journal {
    layout: Layout,
    /// The size of each page on the storage, in order. [`Paging::Append`]
    /// holds at most one, and holds it while the page exists, even empty.
    pages: Vec<u64>,
    snapshot_bytes: u64,
    /// The head and horizon of the log as last written.
    head: Seq,
    horizon: Seq,
    /// The log's identity as the snapshot on the storage has it.
    named: Option<Id>,
    /// A page may hold bytes that are not whole records — an append failed
    /// part-way — so the next write is a snapshot, which empties it.
    due: bool,
    /// Every byte written, snapshots and pages both: what a test counts.
    pub written: u64,
}

fn read_snapshot<K: Keys + ?Sized>(keys: &K, layout: &Layout, schema: &Schema) -> Result<Option<(Log, usize)>, String> {
    let Some(bytes) = keys.load(&layout.snapshot)? else { return Ok(None) };
    let log = decode_snapshot(schema, &bytes).map_err(|e| format!("reading {}: {e}", layout.snapshot))?;
    Ok(Some((log, bytes.len())))
}

fn walk<K: Keys + ?Sized>(keys: &K, layout: &Layout, mut head: Seq, mut each: impl FnMut(Seq, Entry, Facts)) -> Result<Walk, String> {
    let mut w = Walk::default();
    // Each page read whole, and whether every record of it was skipped.
    let mut read: Vec<(u64, bool)> = vec![];
    let mut ended = false;
    let mut n = 1;
    while let Some(bytes) = keys.load(&layout.page_key(n))? {
        if ended {
            w.after += 1;
        } else {
            let r = replay_page(&mut head, &bytes, &mut each);
            w.applied += r.applied;
            w.stale += r.stale;
            if r.torn() {
                w.torn_at = Some((n, r.end, r.len));
                w.after += 1;
                ended = true;
            } else {
                read.push((r.len, r.applied == 0 && r.stale > 0));
            }
        }
        if layout.paging == Paging::Append {
            break;
        }
        n += 1;
    }
    // Pages wholly skipped at the end are a merge's leftovers, newer than
    // every page that extended the log; one in the middle holds nothing
    // this reader lacks, and is kept where it is.
    while read.last().is_some_and(|(_, skipped)| *skipped) && w.torn_at.is_none() {
        read.pop();
        w.after += 1;
    }
    w.good = read.into_iter().map(|(len, _)| len).collect();
    w.head = head;
    Ok(w)
}

/// Read a log back: the snapshot, then every record the pages add, handed
/// to `each` in order. Reads, never writes — a tool beside a running
/// server, or a peer about to join, sees a whole prefix of what was
/// written.
pub fn read<K: Keys + ?Sized>(keys: &K, layout: &Layout, schema: &Schema, each: impl FnMut(Seq, Entry, Facts)) -> Result<Opened, String> {
    let snapshot = read_snapshot(keys, layout, schema)?;
    let head = snapshot.as_ref().map_or(0, |(l, _)| l.head_seq());
    let w = walk(keys, layout, head, each)?;
    Ok(Opened {
        snapshot: snapshot.map(|(l, _)| l),
        head: w.head,
        applied: w.applied,
        stale: w.stale,
        torn: w.torn_at.map(|(n, end, len)| (n, len - end)),
    })
}

/// Read a log back whole: the snapshot with every record the pages add
/// appended; `None` where there is neither.
pub fn load<K: Keys + ?Sized>(keys: &K, layout: &Layout, schema: &Schema) -> Result<Option<Log>, String> {
    let mut got = vec![];
    let o = read(keys, layout, schema, |_, e, f| got.push((e, f)))?;
    if o.snapshot.is_none() && got.is_empty() {
        return Ok(None);
    }
    let mut log = o.snapshot.unwrap_or_else(|| Log::empty(schema.clone()));
    for (e, f) in got {
        log.append(e, f);
    }
    Ok(Some(log))
}

impl Journal {
    /// Open a log on `keys`: read as [`read`] reads it, then repaired so
    /// that what is written next follows on — a torn page cut back to its
    /// whole records, and every page after the good ones removed, newest
    /// first. Nothing is written where the records are whole. Skipped
    /// records under a [`Compaction::Snapshot`] layout are left for the
    /// caller, who holds the log, to compact ([`Opened::stale`]).
    pub fn open<K: Keys + ?Sized>(
        keys: &mut K,
        layout: Layout,
        schema: &Schema,
        each: impl FnMut(Seq, Entry, Facts),
    ) -> Result<(Journal, Opened), String> {
        let snapshot = read_snapshot(keys, &layout, schema)?;
        let snapshot_bytes = snapshot.as_ref().map_or(0, |(_, n)| *n as u64);
        let base_head = snapshot.as_ref().map_or(0, |(l, _)| l.head_seq());
        let w = walk(keys, &layout, base_head, each)?;
        let mut pages = w.good.clone();
        match layout.paging {
            Paging::Append => {
                if let Some((_, end, _)) = w.torn_at {
                    keys.truncate(&layout.page_key(1), end as usize)?;
                    pages = vec![end];
                } else if w.after > 0 {
                    // Wholly stale: kept, and the caller's compaction
                    // empties it.
                    pages = vec![keys.load(&layout.page_key(1))?.map_or(0, |b| b.len() as u64)];
                }
            }
            Paging::Pages => {
                let first_after = w.good.len() + 1;
                let last = w.good.len() + w.after;
                for n in (first_after..=last).rev() {
                    let keep = w.torn_at.filter(|(t, end, _)| *t == n && *end > 0);
                    match keep {
                        Some((_, end, _)) => {
                            keys.truncate(&layout.page_key(n), end as usize)?;
                            pages.push(end);
                        }
                        None => keys.remove(&layout.page_key(n))?,
                    }
                }
            }
        }
        let j = Journal {
            layout,
            pages,
            snapshot_bytes,
            head: w.head,
            horizon: snapshot.as_ref().map_or(0, |(l, _)| l.horizon()),
            named: snapshot.as_ref().and_then(|(l, _)| l.id()),
            due: false,
            written: 0,
        };
        let opened = Opened {
            snapshot: snapshot.map(|(l, _)| l),
            head: w.head,
            applied: w.applied,
            stale: w.stale,
            torn: w.torn_at.map(|(n, end, len)| (n, len - end)),
        };
        Ok((j, opened))
    }

    /// A log with nothing on `keys` yet, whose first write is its snapshot.
    pub fn fresh(layout: Layout) -> Journal {
        Journal {
            layout,
            pages: vec![],
            snapshot_bytes: 0,
            head: 0,
            horizon: 0,
            named: None,
            due: false,
            written: 0,
        }
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// The snapshot's size and the pages', as last written.
    pub fn sizes(&self) -> (u64, u64) {
        (self.snapshot_bytes, self.pages.iter().sum())
    }

    /// How many pages are on the storage.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// The head as last written.
    pub fn head(&self) -> Seq {
        self.head
    }

    /// Records above the head, already encoded ([`encode_record`]),
    /// reaching `to`: appended to the one page, or saved as a page of
    /// their own. Under [`Compaction::Merge`] the caller merges after
    /// ([`Journal::merge`]), whose failure leaves the records durable.
    /// `Ok` means every record is on the storage. Whether the page landed
    /// when this fails is not known: the next write under
    /// [`Paging::Append`] is a snapshot; under [`Paging::Pages`] the page
    /// is whole or absent, and the caller writes the records again.
    pub fn append<K: Keys + ?Sized>(&mut self, keys: &mut K, records: &[u8], to: Seq) -> Result<(), String> {
        if records.is_empty() {
            return Ok(());
        }
        match self.layout.paging {
            Paging::Append => {
                if let Err(e) = keys.append(&self.layout.page_key(1), records) {
                    // Some of it may be on the storage and not whole.
                    self.due = true;
                    return Err(e);
                }
                match self.pages.first_mut() {
                    Some(n) => *n += records.len() as u64,
                    None => self.pages.push(records.len() as u64),
                }
            }
            Paging::Pages => {
                keys.save(&self.layout.page_key(self.pages.len() + 1), records)?;
                self.pages.push(records.len() as u64);
            }
        }
        self.written += records.len() as u64;
        self.head = to;
        Ok(())
    }

    /// Merge the newest page into the one before it for as long as that one
    /// is no larger (the module docs): the merged page saved whole first,
    /// then the newer removed — so a stop between leaves a page every
    /// record of which the merged one holds, which a reader skips and
    /// [`Journal::open`] removes. Nothing under [`Compaction::Snapshot`].
    pub fn merge<K: Keys + ?Sized>(&mut self, keys: &mut K) -> Result<(), String> {
        if self.layout.compaction != Compaction::Merge {
            return Ok(());
        }
        while let [.., older, newer] = self.pages[..] {
            if older > newer {
                break;
            }
            let k = self.pages.len();
            let (a, b) = (self.layout.page_key(k - 1), self.layout.page_key(k));
            let mut both = keys.load(&a)?.ok_or_else(|| format!("{a} is gone"))?;
            both.extend(keys.load(&b)?.ok_or_else(|| format!("{b} is gone"))?);
            keys.save(&a, &both)?;
            self.written += both.len() as u64;
            keys.remove(&b)?;
            self.pages.pop();
            *self.pages.last_mut().expect("two pages") = both.len() as u64;
        }
        Ok(())
    }

    /// Write what moved in `log` since the last write, under
    /// [`Compaction::Snapshot`]: the entries appended, as records at the end
    /// of the page, synced once for all of them; or a snapshot, where the
    /// horizon moved, the page cannot be written on, or the log was renamed
    /// (a log whose name is not the snapshot's is written as a snapshot, so
    /// the name is on the storage from the first write; where there is no
    /// snapshot yet, the first append makes one). `Ok` means every entry of
    /// `log` is on the storage. A compaction that follows a good append and
    /// fails is returned as `Ok(Some(why))`: the entries are durable
    /// either way.
    pub fn write<K: Keys + ?Sized>(&mut self, keys: &mut K, log: &Log) -> Result<Option<String>, String> {
        let renamed = log.id() != self.named && self.snapshot_bytes > 0;
        if self.due || renamed || log.horizon() != self.horizon || log.head_seq() < self.head {
            return self.snapshot(keys, log).map(|_| None);
        }
        if log.head_seq() == self.head {
            return Ok(None);
        }
        let mut buf = vec![];
        for (n, (e, f)) in log.entries.range(self.head + 1..) {
            buf.extend_from_slice(&encode_record(*n, e, f));
        }
        self.append(keys, &buf, log.head_seq())?;
        let (snapshot, pages) = self.sizes();
        if pages > snapshot {
            if let Err(e) = self.snapshot(keys, log) {
                return Ok(Some(e));
            }
        }
        Ok(None)
    }

    /// Compact: the log whole as the snapshot, then the pages emptied —
    /// the one page cut to nothing, or every page removed, newest first —
    /// so a stop between leaves records the snapshot already holds, which a
    /// reader skips.
    pub fn snapshot<K: Keys + ?Sized>(&mut self, keys: &mut K, log: &Log) -> Result<(), String> {
        let bytes = canon::encode(&log_to_value(log));
        keys.save(&self.layout.snapshot, &bytes)?;
        self.written += bytes.len() as u64;
        self.snapshot_bytes = bytes.len() as u64;
        self.head = log.head_seq();
        self.horizon = log.horizon();
        self.named = log.id();
        self.empty_pages(keys)?;
        self.due = false;
        Ok(())
    }

    fn empty_pages<K: Keys + ?Sized>(&mut self, keys: &mut K) -> Result<(), String> {
        match self.layout.paging {
            Paging::Append => {
                if !self.pages.is_empty() {
                    keys.truncate(&self.layout.page_key(1), 0)?;
                    self.pages = vec![0];
                }
            }
            Paging::Pages => {
                while !self.pages.is_empty() {
                    keys.remove(&self.layout.page_key(self.pages.len()))?;
                    self.pages.pop();
                }
            }
        }
        Ok(())
    }

    /// Start a log on `keys` at `base` — a snapshot with nothing above it
    /// — clearing any pages a log that was there left: removed newest
    /// first, then the snapshot saved, so a stop leaves no snapshot, or
    /// this one with nothing after it.
    pub fn create<K: Keys + ?Sized>(keys: &mut K, layout: Layout, base: &Log) -> Result<Journal, String> {
        clear_pages(keys, &layout)?;
        let mut j = Journal::fresh(layout);
        j.snapshot(keys, base)?;
        Ok(j)
    }

    /// Remove the log from `keys`: its pages newest first, then its
    /// snapshot — so a stop leaves the snapshot with a prefix of its pages,
    /// which reads as a shorter log, and never pages with no snapshot.
    pub fn destroy<K: Keys + ?Sized>(keys: &mut K, layout: &Layout) -> Result<(), String> {
        clear_pages(keys, layout)?;
        keys.remove(&layout.snapshot)
    }
}

// Every page from 1 to the first absent, removed newest first.
fn clear_pages<K: Keys + ?Sized>(keys: &mut K, layout: &Layout) -> Result<(), String> {
    let mut n = 0;
    while keys.load(&layout.page_key(n + 1))?.is_some() {
        n += 1;
        if layout.paging == Paging::Append {
            break;
        }
    }
    for k in (1..=n).rev() {
        keys.remove(&layout.page_key(k))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::Args;
    use crate::schema::{Column, Table, Ty};
    use crate::store::Change;

    #[derive(Clone, Debug, Default)]
    struct Mem(BTreeMap<String, Vec<u8>>);

    impl Keys for Mem {
        fn load(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
            Ok(self.0.get(key).cloned())
        }
        fn save(&mut self, key: &str, bytes: &[u8]) -> Result<(), String> {
            self.0.insert(key.into(), bytes.to_vec());
            Ok(())
        }
        fn remove(&mut self, key: &str) -> Result<(), String> {
            self.0.remove(key);
            Ok(())
        }
    }

    fn schema() -> Schema {
        Schema {
            tables: vec![Table::new(
                "t",
                vec![Column {
                    name: "id".into(),
                    ty: Ty::Int,
                    nullable: false,
                }],
                vec!["id".into()],
                vec![],
                vec![],
            )],
        }
    }

    fn record(i: i64) -> Vec<u8> {
        let mut id = [0u8; 16];
        id[8..].copy_from_slice(&i.to_be_bytes());
        let e = Entry {
            id,
            actor: "a".into(),
            session: "s".into(),
            fn_hash: vec![1],
            args: Args::new(),
            autos: Args::new(),
        };
        let row = [("id".to_string(), Value::int(i))].into_iter().collect();
        encode_record(i, &e, &vec![Change::Add("t".into(), row)])
    }

    fn seqs(keys: &Mem) -> Vec<Seq> {
        let mut out = vec![];
        read(keys, &Layout::alone(), &schema(), |n, _, _| out.push(n)).unwrap();
        out
    }

    /// §2: a peer alone's pages merge while the older is no larger, so a
    /// thousand writes of one record are a handful of pages, every record
    /// is still there in order, and the snapshot — the fork — was written
    /// once. Falsified by `merge` returning at once: a thousand pages.
    /// `docs/plan-db.md` D3, Landed: a snapshot written before the state
    /// hash was a sum — its `hash` by the old construction, no `hashing` —
    /// opens, checked by the construction it was written by and held under
    /// this one; the same file claiming the new construction is refused,
    /// and so is one naming a construction this build does not know; and
    /// what is written now says `hashing: 2`. Falsified by checking every
    /// snapshot by this build's construction alone, as before: the old
    /// file is refused, "the snapshot's hash does not match its rows".
    #[test]
    fn a_snapshot_hashed_the_old_way_opens_and_is_held_hashed_the_new_way() {
        let mut st = MemoryStore::empty(schema());
        for i in 1..=3 {
            st.apply_change(&Change::Add("t".into(), [("id".to_string(), Value::int(i))].into_iter().collect()));
        }
        let log = Log {
            base: snapshot_of(3, st.clone()),
            entries: BTreeMap::new(),
            ids: BTreeMap::new(),
            below: Default::default(),
        };
        let written = log_to_value(&log);
        let base = |v: &Value| fields(v).unwrap()["base"].clone();
        assert_eq!(fields(&base(&written)).unwrap().get("hashing"), Some(&Value::int(HASH_VERSION)));
        let with = |hash: Vec<u8>, hashing: Option<i64>| {
            let mut v = written.clone();
            if let Value::Struct(m) = &mut v {
                if let Some(Value::Struct(b)) = m.get_mut("base") {
                    b.insert("hash".into(), Value::bytes(hash));
                    match hashing {
                        Some(n) => b.insert("hashing".into(), Value::int(n)),
                        None => b.remove("hashing"),
                    };
                }
            }
            v
        };
        let old = crate::hash::state_hash_v1(&st);
        assert_ne!(old, crate::hash::state_hash(&st));
        let opened = log_from_value(&schema(), &with(old.clone(), None)).expect("an old snapshot opens");
        assert_eq!(opened.base.hash, crate::hash::state_hash(&st), "held hashed the new way");
        assert_eq!(opened, log);
        assert!(log_from_value(&schema(), &with(old.clone(), Some(1))).is_ok());
        let claimed = log_from_value(&schema(), &with(old, Some(2))).unwrap_err();
        assert!(claimed.contains("hash"), "{claimed}");
        let newer = log_from_value(&schema(), &with(log.base.hash.clone(), Some(3))).unwrap_err();
        assert!(newer.contains("does not know"), "{newer}");
    }

    #[test]
    fn pages_merge_into_few_and_keep_every_record() {
        let mut keys = Mem::default();
        let mut j = Journal::create(&mut keys, Layout::alone(), &Log::empty(schema())).unwrap();
        let snapshot = keys.load("log").unwrap();
        for i in 1..=1000 {
            j.append(&mut keys, &record(i), i).unwrap();
            j.merge(&mut keys).unwrap();
        }
        assert!(j.page_count() <= 10, "{} pages", j.page_count());
        assert_eq!(seqs(&keys), (1..=1000).collect::<Vec<_>>());
        assert_eq!(keys.load("log").unwrap(), snapshot, "the base never moves");
        assert!(j.written < 12 * j.sizes().1, "{} bytes written for {}", j.written, j.sizes().1);
    }

    /// A page torn mid-write reads to its last whole record, and `open`
    /// cuts it there and drops what follows, so the next page follows on.
    /// Falsified by `open` leaving the torn page as it was: the record
    /// appended after reopening is not read back.
    #[test]
    fn a_torn_page_opens_to_its_last_whole_record() {
        let mut keys = Mem::default();
        let mut j = Journal::create(&mut keys, Layout::alone(), &Log::empty(schema())).unwrap();
        for i in 1..=6 {
            j.append(&mut keys, &record(i), i).unwrap();
        }
        let last = format!("log.{}", j.page_count());
        let mut bytes = keys.load(&last).unwrap().unwrap();
        bytes.truncate(bytes.len() - 3);
        keys.save(&last, &bytes).unwrap();
        let (mut j, o) = Journal::open(&mut keys, Layout::alone(), &schema(), |_, _, _| {}).unwrap();
        assert_eq!(o.head, 5);
        assert!(o.torn.is_some());
        j.append(&mut keys, &record(6), 6).unwrap();
        assert_eq!(seqs(&keys), (1..=6).collect::<Vec<_>>());
    }

    /// A merge that stopped between saving the merged page and removing the
    /// newer one leaves a page the merged one holds whole: it is skipped,
    /// and `open` removes it, so the next write is the next page. Falsified
    /// by `open` keeping wholly stale pages: `log.2` is still there.
    #[test]
    fn a_merge_that_stopped_halfway_reads_once() {
        let mut keys = Mem::default();
        let mut j = Journal::create(&mut keys, Layout::alone(), &Log::empty(schema())).unwrap();
        j.append(&mut keys, &[record(1), record(2)].concat(), 2).unwrap();
        j.append(&mut keys, &record(3), 3).unwrap();
        assert_eq!(j.page_count(), 2, "the older page is larger: no merge");
        // What merging page 2 into page 1 had written before stopping.
        let merged = [record(1), record(2), record(3)].concat();
        keys.save("log.1", &merged).unwrap();
        assert_eq!(seqs(&keys), vec![1, 2, 3]);
        let (mut j, o) = Journal::open(&mut keys, Layout::alone(), &schema(), |_, _, _| {}).unwrap();
        assert_eq!((o.head, o.stale), (3, 1));
        assert!(keys.load("log.2").unwrap().is_none(), "the leftover is removed");
        j.append(&mut keys, &record(4), 4).unwrap();
        assert_eq!(seqs(&keys), vec![1, 2, 3, 4]);
    }

    /// `destroy` takes the pages first and the snapshot last, and `create`
    /// over what a stopped `destroy` left starts clean. Falsified by
    /// `create` not clearing pages: the old page reads on top.
    #[test]
    fn create_clears_what_was_there() {
        let mut keys = Mem::default();
        let mut j = Journal::create(&mut keys, Layout::alone(), &Log::empty(schema())).unwrap();
        j.append(&mut keys, &record(1), 1).unwrap();
        keys.remove("log").unwrap();
        Journal::create(&mut keys, Layout::alone(), &Log::empty(schema())).unwrap();
        assert_eq!(seqs(&keys), Vec::<Seq>::new());
        Journal::destroy(&mut keys, &Layout::alone()).unwrap();
        assert!(keys.0.is_empty());
    }
}
