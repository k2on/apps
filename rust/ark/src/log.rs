//! §1.6 The log, chained: each entry an intent, logged with the facts it
//! had, the state hash after it and the hash of the chain to it.
//!
//! ```text
//! Entry  { id, actor, session, origin, local, roles, fn, input, now }
//! Logged { seq, entry, facts, state, hash }
//! hash = sha256(prev.hash ‖ enc(seq, entry, facts, state))
//! ```
//!
//! **The log is a table.** Its entries are the rows of the store's `_log`
//! table ([`crate::store::LOG`]), keyed by sequence, each row a
//! [`Logged`]'s bytes; the chain is that table's invariant, judged at
//! every write to it ([`Logged::links`]), and its first row is the one the
//! log stands on (the horizon, or a snapshot's head), not checked. What a
//! peer makes durable is writes to that store, the log's table's among
//! them ([`Record`]), and it opens from the store's rows and the writes
//! since ([`Durable`]): one format.
//!
//! **The chain is the log's identity.** Two peers at one sequence with one
//! [`EntryHash`] hold one history; a peer that says `(cursor, hash)` is
//! paged from there by an authority that has that hash at that sequence
//! and is sent a snapshot by one that does not.
//!
//! **Deterministic ids.** An entry's id is `sha256(enc(Bytes origin) ‖
//! enc(Int local))[..16]`: `origin` is sixteen bytes a replica draws once,
//! at its first open, and keeps — the one other non-determinism beside
//! `now` — and `local` the replica's own count of the intents it authored,
//! from 1. The `n`-th row id an entry draws is `sha256(id ‖ enc(Int
//! n))[..16]` ([`EntryId::draw`]). Neither depends on who is signed in, so
//! what a device made signed out keeps its ids, and its rows theirs, when
//! it signs in. Dedupe is `(origin, local)`, which also shows a gap in a
//! replica's pushes; `actor`, `session` and `roles` are what the authority
//! stamps from the connection it judged the entry on.
//!
//! **Snapshots and the horizon.** A [`Snapshot`] is `(seq, hash, state,
//! rows in key order)`, what an authority sends; a log stands on it as a
//! first row of its table with no entry ([`Snapshot::base`]). The horizon
//! is a delete of the rows below it.

use ark_macros::Node;

use crate::codec::{put_bytes, put_int, Reader};
use crate::hash::{sha256, EntryHash, FnHash, StateHash};
use crate::ir::Node;
use crate::store::Change;
use crate::Refusal;

/// A position in the log. The first entry is 1; 0 is the empty chain.
pub type Seq = i64;

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntryId(pub [u8; 16]);

impl std::fmt::Debug for EntryId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "EntryId({})", crate::hash::hex(&self.0[..6]))
    }
}

fn first16(h: [u8; 32]) -> [u8; 16] {
    h[..16].try_into().unwrap()
}

/// Where a replica's intents come from: drawn once, kept (§1.6).
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Origin(pub [u8; 16]);

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

impl EntryId {
    /// The id of the `local`-th intent from `origin` (an [`Origin`]; any
    /// bytes name an id outside the log, as a query's context does).
    pub fn of(origin: impl AsRef<[u8]>, local: i64) -> EntryId {
        let mut b = vec![];
        put_bytes(&mut b, origin.as_ref());
        put_int(&mut b, local);
        EntryId(first16(sha256(&[&b])))
    }

    /// The `n`-th id drawn inside this entry.
    pub fn draw(&self, n: i64) -> [u8; 16] {
        let mut b = self.0.to_vec();
        put_int(&mut b, n);
        first16(sha256(&[&b]))
    }
}

/// An intent, as recorded. `actor` and `roles` are what the authority
/// judged it under: it stamps them before sequencing (§1.7), and a replay
/// anywhere reads what the log says. The default is no intent, what the
/// first row of a log standing on a snapshot carries.
#[derive(Clone, Debug, Default, PartialEq, Eq, Node)]
pub struct Entry {
    pub id: EntryId,
    pub actor: String,
    pub session: String,
    pub origin: Origin,
    pub local: i64,
    pub roles: Vec<String>,
    pub func: FnHash,
    pub input: Vec<u8>,
    pub now: i64,
}

impl Entry {
    /// `(Bytes id, Bytes actor, Bytes session, Bytes origin, Int local, List
    /// Bytes roles, Bytes fn, Bytes input, Int now)`, derived.
    pub fn enc(&self, out: &mut Vec<u8>) {
        self.put(out);
    }

    pub fn dec(r: &mut Reader) -> Result<Entry, Refusal> {
        Entry::take(r)
    }

    /// Whether the id is the one `(origin, local)` names: an id is never
    /// taken on trust.
    pub fn id_checks(&self) -> bool {
        self.id == EntryId::of(self.origin, self.local)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Node)]
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
        let mut b = vec![];
        put_int(&mut b, seq);
        entry.put(&mut b);
        Change::put_list(facts, &mut b);
        state.put(&mut b);
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

    /// `(Int seq, Entry, List Change, Bytes state, Bytes hash)`, derived.
    pub fn enc(&self, out: &mut Vec<u8>) {
        self.put(out);
    }

    pub fn dec(r: &mut Reader) -> Result<Logged, Refusal> {
        Logged::take(r)
    }
}

/// The state at a sequence: the chain's hash and the state hash there, and
/// the durable rows, table by table in schema order, each in key order.
#[derive(Clone, Debug, PartialEq, Eq, Node)]
pub struct Snapshot {
    pub seq: Seq,
    pub hash: EntryHash,
    pub state: StateHash,
    pub rows: Vec<(String, Vec<Vec<u8>>)>,
}

impl Snapshot {
    /// `(Int seq, Bytes hash, Bytes state, List (Bytes table, List
    /// Bytes))`, derived.
    pub fn enc(&self, out: &mut Vec<u8>) {
        self.put(out);
    }

    pub fn dec(r: &mut Reader) -> Result<Snapshot, Refusal> {
        Snapshot::take(r)
    }

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

/// What a peer makes durable, one record per `Persist` (§1.7): writes to
/// its store, appended — a logged entry is its facts and its row of the
/// log's table — or the store's rows whole, which every write before them
/// is folded into.
#[derive(Clone, Debug, PartialEq, Eq, Node)]
pub enum Record {
    Writes(Vec<Change>),
    Rows(Rows),
}

impl Record {
    pub fn enc(&self) -> Vec<u8> {
        self.to_bytes()
    }

    pub fn dec(b: &[u8]) -> Result<Record, Refusal> {
        Record::from_bytes(b)
    }
}

/// What a peer is opened from: its store's rows as they were last written
/// whole — the log's table's among them — and the writes since, in order;
/// its own intents still pending; and the last `local` it drew.
#[derive(Clone, Debug, Default, PartialEq, Eq, Node)]
pub struct Durable {
    pub rows: Rows,
    pub writes: Vec<Vec<Change>>,
    pub pending: Vec<Entry>,
    pub local: i64,
}
