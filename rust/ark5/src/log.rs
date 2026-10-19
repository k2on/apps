//! §1.6 The log, chained: each entry an intent, logged with the facts it
//! had, the state hash after it and the hash of the chain to it.
//!
//! ```text
//! Entry  { id, actor, session, local, roles, fn, input, now }
//! Logged { seq, entry, facts, state, hash }
//! hash = sha256(prev.hash ‖ enc(seq, entry, facts, state))
//! ```
//!
//! **The chain is the log's identity.** Two peers at one sequence with one
//! [`EntryHash`] hold one history; a peer that says `(cursor, hash)` is
//! paged from there by an authority that has that hash at that sequence
//! ([`Log::has`]) and is sent a snapshot by one that does not.
//!
//! **Deterministic ids.** An entry's id is `sha256(enc(Bytes session) ‖
//! enc(Int local))[..16]`, `local` being the session's own count of the
//! intents it authored, from 1; the `n`-th row id an entry draws is
//! `sha256(id ‖ enc(Int n))[..16]` ([`EntryId::draw`]). `now` is the only
//! non-determinism, drawn once at origin and frozen in the entry. Dedupe is
//! `(session, local)`, which also shows a gap in a session's pushes.
//!
//! **Snapshots and the horizon.** A [`Snapshot`] is `(seq, hash, state,
//! rows in key order)` and a log stands on one ([`Log::base`]); entries at
//! or below it are dropped.

use crate::codec::{put_bytes, put_int, put_list, put_variant, Reader};
use crate::hash::{sha256, EntryHash, FnHash, StateHash};
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

impl EntryId {
    /// The id of a session's `local`-th intent.
    pub fn of(session: &str, local: i64) -> EntryId {
        let mut b = vec![];
        put_bytes(&mut b, session.as_bytes());
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
/// anywhere reads what the log says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: EntryId,
    pub actor: String,
    pub session: String,
    pub local: i64,
    pub roles: Vec<String>,
    pub func: FnHash,
    pub input: Vec<u8>,
    pub now: i64,
}

fn put_hash(out: &mut Vec<u8>, h: &[u8; 32]) {
    put_bytes(out, h);
}

impl Entry {
    /// `(Bytes id, Bytes actor, Bytes session, Int local, List Bytes roles,
    /// Bytes fn, Bytes input, Int now)`.
    pub fn enc(&self, out: &mut Vec<u8>) {
        put_bytes(out, &self.id.0);
        put_bytes(out, self.actor.as_bytes());
        put_bytes(out, self.session.as_bytes());
        put_int(out, self.local);
        put_list(out, &self.roles, |o, r| put_bytes(o, r.as_bytes()));
        put_hash(out, &self.func.0);
        put_bytes(out, &self.input);
        put_int(out, self.now);
    }

    pub fn dec(r: &mut Reader) -> Result<Entry, Refusal> {
        Ok(Entry {
            id: EntryId(r.fixed()?),
            actor: r.text()?,
            session: r.text()?,
            local: r.int()?,
            roles: r.list(|r| r.text())?,
            func: FnHash(r.fixed()?),
            input: r.bytes()?,
            now: r.int()?,
        })
    }

    /// Whether the id is the one `(session, local)` names: an id is never
    /// taken on trust.
    pub fn id_checks(&self) -> bool {
        self.id == EntryId::of(&self.session, self.local)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
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
        entry.enc(&mut b);
        put_list(&mut b, facts, |o, c| c.enc(o));
        put_hash(&mut b, &state.0);
        EntryHash(sha256(&[&prev.0, &b]))
    }

    /// Whether this links onto `prev`.
    pub fn links(&self, prev: &EntryHash) -> bool {
        self.hash == Logged::chain(prev, self.seq, &self.entry, &self.facts, &self.state)
    }

    /// `(Int seq, Entry, List Change, Bytes state, Bytes hash)`.
    pub fn enc(&self, out: &mut Vec<u8>) {
        put_int(out, self.seq);
        self.entry.enc(out);
        put_list(out, &self.facts, |o, c| c.enc(o));
        put_hash(out, &self.state.0);
        put_hash(out, &self.hash.0);
    }

    pub fn dec(r: &mut Reader) -> Result<Logged, Refusal> {
        Ok(Logged {
            seq: r.int()?,
            entry: Entry::dec(r)?,
            facts: r.list(Change::dec)?,
            state: StateHash(r.fixed()?),
            hash: EntryHash(r.fixed()?),
        })
    }
}

/// The state at a sequence: the chain's hash and the state hash there, and
/// the durable rows, table by table in schema order, each in key order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub seq: Seq,
    pub hash: EntryHash,
    pub state: StateHash,
    pub rows: Vec<(String, Vec<Vec<u8>>)>,
}

impl Snapshot {
    /// `(Int seq, Bytes hash, Bytes state, List (Bytes table, List Bytes))`.
    pub fn enc(&self, out: &mut Vec<u8>) {
        put_int(out, self.seq);
        put_hash(out, &self.hash.0);
        put_hash(out, &self.state.0);
        put_list(out, &self.rows, |o, (t, rs)| {
            put_bytes(o, t.as_bytes());
            put_list(o, rs, |o, r| put_bytes(o, r));
        });
    }

    pub fn dec(r: &mut Reader) -> Result<Snapshot, Refusal> {
        Ok(Snapshot {
            seq: r.int()?,
            hash: EntryHash(r.fixed()?),
            state: StateHash(r.fixed()?),
            rows: r.list(|r| Ok((r.text()?, r.list(|r| r.bytes())?)))?,
        })
    }
}

/// The log: a snapshot, and the chain above it in sequence order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Log {
    pub base: Snapshot,
    pub entries: Vec<Logged>,
}

impl Log {
    pub fn new(base: Snapshot) -> Log {
        Log { base, entries: vec![] }
    }

    /// The last sequence and the chain's hash there.
    pub fn head(&self) -> (Seq, EntryHash) {
        self.entries.last().map_or((self.base.seq, self.base.hash), |l| (l.seq, l.hash))
    }

    pub fn head_state(&self) -> StateHash {
        self.entries.last().map_or(self.base.state, |l| l.state)
    }

    pub fn at(&self, seq: Seq) -> Option<&Logged> {
        let i = seq - self.base.seq - 1;
        (i >= 0).then(|| self.entries.get(i as usize)).flatten()
    }

    /// Whether this log has `hash` at `seq`: one history up to there.
    pub fn has(&self, seq: Seq, hash: &EntryHash) -> bool {
        if seq == self.base.seq {
            return self.base.hash == *hash;
        }
        self.at(seq).is_some_and(|l| l.hash == *hash)
    }

    /// The entries after `seq`, which must be at or above the base.
    pub fn after(&self, seq: Seq) -> &[Logged] {
        &self.entries[((seq - self.base.seq).max(0) as usize).min(self.entries.len())..]
    }

    /// Appends an entry with its facts and the state after it; the next
    /// sequence and the chain's hash are the log's to give.
    pub fn append(&mut self, entry: Entry, facts: Vec<Change>, state: StateHash) -> &Logged {
        let (n, prev) = self.head();
        let hash = Logged::chain(&prev, n + 1, &entry, &facts, &state);
        self.entries.push(Logged {
            seq: n + 1,
            entry,
            facts,
            state,
            hash,
        });
        self.entries.last().unwrap()
    }

    /// Appends a logged entry from elsewhere, which must link onto the head.
    pub fn push(&mut self, l: Logged) -> Result<(), Refusal> {
        let (n, prev) = self.head();
        if l.seq != n + 1 || !l.links(&prev) {
            return Err(Refusal(format!("entry {} does not link onto {n}", l.seq)));
        }
        self.entries.push(l);
        Ok(())
    }

    /// Drops every entry above `seq`.
    pub fn truncate(&mut self, seq: Seq) {
        self.entries.truncate((seq - self.base.seq).max(0) as usize);
    }
}

/// What a peer makes durable, one record per `Persist` (§1.7): a logged
/// entry appended to the chain, or a snapshot the chain now stands on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Record {
    Logged(Logged),
    Snapshot(Snapshot),
}

impl Record {
    pub fn enc(&self) -> Vec<u8> {
        let mut out = vec![];
        match self {
            Record::Logged(l) => {
                put_variant(&mut out, 0);
                l.enc(&mut out);
            }
            Record::Snapshot(s) => {
                put_variant(&mut out, 1);
                s.enc(&mut out);
            }
        }
        out
    }

    pub fn dec(b: &[u8]) -> Result<Record, Refusal> {
        let mut r = Reader::new(b);
        let rec = match r.variant(2)? {
            0 => Record::Logged(Logged::dec(&mut r)?),
            _ => Record::Snapshot(Snapshot::dec(&mut r)?),
        };
        r.end()?;
        Ok(rec)
    }
}

/// What a peer is opened from: the snapshot and the chain on it as they
/// were made durable, its own intents still pending, and the last `local`
/// its session drew.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Durable {
    pub snapshot: Snapshot,
    pub chain: Vec<Logged>,
    pub pending: Vec<Entry>,
    pub local: i64,
}

impl Durable {
    /// The records, in the order persisted, read back: a snapshot starts
    /// the chain again; the chain stops at the first record that does not
    /// read or does not link, which is where a crash cut the journal.
    pub fn from_records(base: Snapshot, records: &[Vec<u8>]) -> Durable {
        let mut log = Log::new(base);
        for b in records {
            let read = match Record::dec(b) {
                Ok(Record::Snapshot(s)) => {
                    log = Log::new(s);
                    true
                }
                Ok(Record::Logged(l)) => log.push(l).is_ok(),
                Err(_) => false,
            };
            if !read {
                break;
            }
        }
        Durable {
            snapshot: log.base,
            chain: log.entries,
            pending: vec![],
            local: 0,
        }
    }
}
