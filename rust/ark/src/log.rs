//! §1.6 The log, chained: each entry an intent, logged with the facts it
//! had, the state hash after it and the hash of the chain to it.
//!
//! ```text
//! Entry  { id, actor, session, origin, local, roles, func, input, now }
//! Logged { seq, entry, facts, state, hash }
//! hash = sha256(prev.hash ‖ enc([seq, entry, facts, state]))
//! ```
//!
//! **The log is a table.** Its entries are the rows of the store's `_log`
//! table ([`crate::store::LOG`]), keyed by sequence, each row a
//! [`Logged`]'s encoding; the chain is that table's invariant, judged at
//! every write to it ([`Logged::links`]), and its first row is the one the
//! log stands on (the horizon, or a snapshot's head), not checked. What a
//! peer makes durable is the store's rows whole, the log's table's among
//! them, and the rows appended to the log's table since ([`Record`]): an
//! entry's facts are in its row, so the rows they write to the schema's
//! tables are not journaled beside it but written again as a peer opens
//! ([`Durable`]): one format, each fact on disk once.
//!
//! **The chain is the log's identity.** Two peers at one sequence with one
//! [`EntryHash`] hold one history; a peer that says `(cursor, hash)` is
//! paged from there by an authority that has that hash at that sequence
//! and is sent a snapshot by one that does not.
//!
//! **Deterministic ids.** An entry's id is `sha256(len(origin) ‖ origin ‖
//! local)[..16]`, the length and `local` eight bytes big-endian: `origin` is sixteen bytes a replica draws once,
//! at its first open, and keeps — the one other non-determinism beside
//! `now` — and `local` the replica's own count of the intents it authored,
//! from 1. The `n`-th row id an entry draws is `sha256(id ‖ n)[..16]`, `n`
//! eight bytes big-endian ([`EntryId::draw`]). In JSON an id is its hex. Neither depends on who is signed in, so
//! what a device made signed out keeps its ids, and its rows theirs, when
//! it signs in. Dedupe is `(origin, local)`, which also shows a gap in a
//! replica's pushes; `actor`, `session` and `roles` are what the authority
//! stamps from the connection it judged the entry on.
//!
//! **Snapshots and the horizon.** A [`Snapshot`] is `(seq, hash, state,
//! rows in key order)`, what an authority sends; a log stands on it as a
//! first row of its table with no entry ([`Snapshot::base`]). The horizon
//! is a delete of the rows below it.

use serde::{Deserialize, Serialize};

use crate::format::Encode;
use crate::hash::{sha256, EntryHash, StateHash};
use crate::store::Change;
use crate::Refusal;

/// A position in the log. The first entry is 1; 0 is the empty chain.
pub type Seq = i64;

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EntryId(#[serde(with = "crate::format::hex_bytes")] pub [u8; 16]);

impl std::fmt::Debug for EntryId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "EntryId({})", crate::hash::hex(&self.0[..6]))
    }
}

fn first16(h: [u8; 32]) -> [u8; 16] {
    h[..16].try_into().unwrap()
}

/// Where a replica's intents come from: drawn once, kept (§1.6).
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Origin(#[serde(with = "crate::format::hex_bytes")] pub [u8; 16]);

impl std::fmt::Debug for Origin {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Origin({})", crate::hash::hex(&self.0[..6]))
    }
}

impl AsRef<[u8]> for Origin {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Origin {
    /// The origin named by these bytes, `sha256(bytes)[..16]`: how a seeded
    /// run (a simulator, a test, an authority's own) has one without
    /// drawing it.
    pub fn named(bytes: impl AsRef<[u8]>) -> Origin {
        Origin(first16(sha256(&[bytes.as_ref()])))
    }
}

/// What a trace is named by: everything one tap, one scan, one job caused.
/// A trace begun by an entry is named by that entry's id.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TraceId(#[serde(with = "crate::format::hex_bytes")] pub [u8; 16]);

impl std::fmt::Debug for TraceId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "TraceId({})", crate::hash::hex(&self.0[..6]))
    }
}

/// One step of a trace: an entry, whose span is named by its id, or a span
/// a peer or an app opened around what it did (a tap, a job's run).
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Span {
    pub trace: TraceId,
    #[serde(with = "crate::format::hex_bytes")]
    pub id: [u8; 16],
    /// When its trace began, on the clock entries are dated by.
    pub began: i64,
}

impl std::fmt::Debug for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Span({}/{})", crate::hash::hex(&self.trace.0[..6]), crate::hash::hex(&self.id[..6]))
    }
}

impl Span {
    /// A span of its own inside this one's trace, named by `bytes` (a run
    /// of a job, a call through the bridge).
    pub fn child(&self, bytes: impl AsRef<[u8]>) -> Span {
        Span {
            trace: self.trace,
            id: first16(sha256(&[&self.id, bytes.as_ref()])),
            began: self.began,
        }
    }
}

impl EntryId {
    /// The id of the `local`-th intent from `origin` (an [`Origin`]; any
    /// bytes name an id outside the log, as a query's context does).
    pub fn of(origin: impl AsRef<[u8]>, local: i64) -> EntryId {
        let o = origin.as_ref();
        EntryId(first16(sha256(&[&(o.len() as u64).to_be_bytes(), o, &local.to_be_bytes()])))
    }

    /// The `n`-th id drawn inside this entry.
    pub fn draw(&self, n: i64) -> [u8; 16] {
        first16(sha256(&[&self.0, &n.to_be_bytes()]))
    }
}

/// An intent, as recorded. `actor` and `roles` are what the authority
/// judged it under: it stamps them before sequencing (§1.7), and a replay
/// anywhere reads what the log says. The default is no intent, what the
/// first row of a log standing on a snapshot carries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: EntryId,
    pub actor: String,
    pub session: String,
    pub origin: Origin,
    pub local: i64,
    pub roles: Vec<String>,
    pub func: String,
    /// The input's encoding, carried as it is.
    #[serde(with = "crate::format::raw")]
    pub input: Vec<u8>,
    pub now: i64,
    /// The span it was authored under: a tap, a job's run, the entry whose
    /// change woke what authored it. None begins a trace of its own.
    pub cause: Option<Span>,
}

impl Default for Entry {
    fn default() -> Entry {
        Entry {
            id: EntryId::default(),
            actor: String::new(),
            session: String::new(),
            origin: Origin::default(),
            local: 0,
            roles: vec![],
            func: String::new(),
            input: ().to_bytes(),
            now: 0,
            cause: None,
        }
    }
}

impl Entry {
    /// The `local`-th intent from `origin`, as `who` makes it: calling
    /// `func` on `input` (an input is its encoding; no bytes are `()`'s) at
    /// `now`.
    pub fn new(who: &crate::peer::Who, origin: Origin, local: i64, func: impl Into<String>, input: Vec<u8>, now: i64) -> Entry {
        Entry {
            id: EntryId::of(origin, local),
            actor: who.actor.clone(),
            session: who.session.clone(),
            origin,
            local,
            roles: who.roles.clone(),
            func: func.into(),
            input: if input.is_empty() { ().to_bytes() } else { input },
            now,
            cause: None,
        }
    }

    /// Whether the id is the one `(origin, local)` names: an id is never
    /// taken on trust.
    pub fn id_checks(&self) -> bool {
        self.id == EntryId::of(self.origin, self.local)
    }

    /// Its own span: named by its id, in its cause's trace, or beginning one
    /// named by its id, at its `now`.
    pub fn span(&self) -> Span {
        Span {
            trace: self.cause.map_or(TraceId(self.id.0), |c| c.trace),
            id: self.id.0,
            began: self.cause.map_or(self.now, |c| c.began),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Logged {
    pub seq: Seq,
    pub entry: Entry,
    pub facts: Vec<Change>,
    pub state: StateHash,
    pub hash: EntryHash,
}

impl Logged {
    /// The hash of the chain to an entry, from the one before it.
    pub fn chain(prev: &EntryHash, seq: Seq, entry: &Entry, facts: &[Change], state: &StateHash) -> EntryHash {
        let b = crate::format::encode(&(seq, entry, facts, state)).expect("an entry serializes");
        EntryHash(sha256(&[&prev.0, &b]))
    }

    /// An intent not yet sequenced, as a peer pages it (§1.11): at
    /// sequence 0, with no facts and zero hashes.
    pub fn intent(entry: Entry) -> Logged {
        let (state, hash) = Default::default();
        Logged {
            seq: 0,
            entry,
            facts: vec![],
            state,
            hash,
        }
    }

    /// Whether this links onto `prev`.
    pub fn links(&self, prev: &EntryHash) -> bool {
        self.hash == Logged::chain(prev, self.seq, &self.entry, &self.facts, &self.state)
    }
}

/// The state at a sequence: the chain's hash and the state hash there, and
/// the durable rows, table by table in schema order, each in key order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub seq: Seq,
    pub hash: EntryHash,
    pub state: StateHash,
    #[serde(with = "crate::format::raw_tables")]
    pub rows: Vec<(String, Vec<Vec<u8>>)>,
}

impl Snapshot {
    /// The first row of a log standing on this snapshot: its sequence,
    /// state and hash, and no entry.
    pub fn base(&self) -> Logged {
        let mut l = Logged::intent(Entry::default());
        (l.seq, l.state, l.hash) = (self.seq, self.state, self.hash);
        l
    }

    /// The store's rows as a log standing on this snapshot holds them: the
    /// snapshot's, then the log's table with its first row.
    pub fn kv_rows(&self) -> Rows {
        let mut rows = self.rows.clone();
        rows.push((crate::store::LOG.into(), vec![self.base().to_bytes()]));
        rows
    }
}

/// A store's rows, by table.
pub type Rows = Vec<(String, Vec<Vec<u8>>)>;

/// Where a fill stands: at `table`'s first row whose key is `key` or past
/// it, every row before it sent.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub table: String,
    #[serde(with = "crate::format::raw")]
    pub key: Vec<u8>,
}

/// Every row of `table` whose key is `from` or past it and before `to`
/// (none: to the table's end), as the authority held them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub table: String,
    #[serde(with = "crate::format::raw")]
    pub from: Vec<u8>,
    pub to: Option<Vec<u8>>,
    #[serde(with = "crate::format::raw_list")]
    pub rows: Vec<Vec<u8>>,
}

/// A part of a fill (§1.7): ranges of the authority's rows as they stood at
/// `(seq, hash)`, with the state there, each range's rows replacing what a
/// replica held in it; the first part of a fill begins it from nothing,
/// and the last says no `next`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Part {
    pub seq: Seq,
    pub hash: EntryHash,
    pub state: StateHash,
    pub first: bool,
    pub ranges: Vec<Range>,
    pub next: Option<Cursor>,
}

impl Part {
    /// Where it begins: its first range's start.
    pub fn from(&self) -> Option<Cursor> {
        self.ranges.first().map(|r| Cursor {
            table: r.table.clone(),
            key: r.from.clone(),
        })
    }
}

/// What a peer makes durable, one record per `Persist` (§1.7): a logged
/// entry, appended, as its row of the log's table — its facts are in it —
/// or the store's rows whole, which every entry before them is folded
/// into; or a part of a fill, appended, its rows written again as a peer
/// opens beside the entries, by its sequence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)]
pub enum Record {
    Logged(Logged),
    Rows(#[serde(with = "crate::format::raw_tables")] Rows),
    Part(Part),
}

impl Record {
    /// A logged entry's record from its row of the log's table, which is
    /// the entry's encoding: the variant's index, 0, before the row as it is.
    pub fn logged(row: &[u8]) -> Vec<u8> {
        [&[0u8][..], row].concat()
    }

    pub fn enc(&self) -> Vec<u8> {
        self.to_bytes()
    }

    /// Whether these bytes are a record of the store's rows whole, told by
    /// their variant alone.
    pub fn is_rows(b: &[u8]) -> bool {
        b.first() == Some(&1)
    }

    pub fn dec(b: &[u8]) -> Result<Record, Refusal> {
        Record::from_bytes(b)
    }
}

/// What a peer is opened from: its store's rows as they were last written
/// whole — the log's table's among them — and the rows appended to the
/// log's table since, in order, whose facts write the rest again; its own
/// intents still pending; and the last `local` it drew. A replica filling
/// has the parts of the fill appended since too, each after the entries at
/// or below its sequence.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Durable {
    #[serde(with = "crate::format::raw_tables")]
    pub rows: Rows,
    pub logged: Vec<Logged>,
    pub pending: Vec<Entry>,
    pub local: i64,
    #[serde(default)]
    pub parts: Vec<Part>,
}
