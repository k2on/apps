//! §1.6 The log, chained: each entry an intent, logged with the facts it
//! had and the hash of the chain to it.
//!
//! ```text
//! Ctx    { id, actor, session, origin, local, roles, func, input, now, cause }
//! Logged { seq, entry, facts, hash }
//! hash = sha256(prev.hash ‖ enc([seq, entry, facts]))
//! ```
//!
//! **The log is a table.** Its entries are the rows of the store's `_log`
//! table ([`crate::store::LOG`]), keyed by sequence, each row a
//! [`Logged`]'s encoding; the chain is that table's invariant, judged at
//! every write to it ([`Logged::links`]), and its first row is the one the
//! log stands on (the horizon, or a fill's head), not checked. What a
//! peer makes durable is the store's rows whole, the log's table's among
//! them, and the rows appended to the log's table since ([`Record`]): an
//! entry's facts are in its row, so the rows they write to the schema's
//! tables are not journaled beside it but written again as a peer opens
//! from its records: one format, each fact on disk once.
//!
//! **The chain is the log's identity.** Two peers at one sequence with one
//! [`EntryHash`] hold one history; a peer that says `(cursor, hash)` is
//! paged from there by an authority that has that hash at that sequence
//! and is filled by one that does not.
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
//! **The horizon.** A log stands on a first row of its table with no entry
//! ([`Logged::base`]): where it began, a fill's head, or the horizon, which
//! is a delete of the rows below it.

use serde::{Deserialize, Serialize};

use crate::format::Encode;
use crate::hash::{sha256, EntryHash};
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

/// An entry: an intent, as recorded, and all a function knows of the one
/// it runs for — all frozen in it, so every run of it, anywhere, is the
/// same. `actor` and `roles` are what the authority judged it under: it
/// stamps them before sequencing (§1.7), and a replay anywhere reads what
/// the log says. The default is no intent, what the first row of a log
/// standing on a snapshot carries. With no intent in it, who someone is
/// ([`Ctx::who`]): who a peer authors as, who a connection was verified to
/// be; at a `now`, a query's or a view's context.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ctx {
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
    /// How many row ids a run of it has drawn ([`Ctx::new_id`]): no part of
    /// the entry.
    #[serde(skip)]
    pub draws: Draws,
}

/// A count a run moves through a shared reference: a `Cell`'s, but one a
/// context held across threads (who a connection is) may carry. Not what
/// an entry is: any two are equal.
#[derive(Debug, Default)]
pub struct Draws(std::sync::atomic::AtomicI64);

impl Draws {
    pub fn get(&self) -> i64 {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
    pub fn set(&self, n: i64) {
        self.0.store(n, std::sync::atomic::Ordering::Relaxed)
    }
}

impl Clone for Draws {
    fn clone(&self) -> Draws {
        Draws(self.get().into())
    }
}

impl PartialEq for Draws {
    fn eq(&self, _: &Draws) -> bool {
        true
    }
}
impl Eq for Draws {}

impl Default for Ctx {
    fn default() -> Ctx {
        Ctx {
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
            draws: Draws::default(),
        }
    }
}

impl Ctx {
    /// The `local`-th intent from `origin`, as `who` makes it: calling
    /// `func` on `input` (an input is its encoding; no bytes are `()`'s) at
    /// `now`.
    pub fn new(who: &Ctx, origin: Origin, local: i64, func: impl Into<String>, input: Vec<u8>, now: i64) -> Ctx {
        Ctx {
            id: EntryId::of(origin, local),
            origin,
            local,
            func: func.into(),
            input: if input.is_empty() { ().to_bytes() } else { input },
            now,
            cause: None,
            ..who.clone()
        }
    }

    /// Who someone is, in no entry.
    pub fn who(actor: impl Into<String>, session: impl Into<String>, roles: Vec<String>) -> Ctx {
        Ctx {
            actor: actor.into(),
            session: session.into(),
            roles,
            ..Ctx::default()
        }
    }

    /// The next row id this run draws: the entry's `n`-th, from 0 (§1.6).
    /// With `now`, a function's only non-determinism, frozen in the entry.
    pub fn new_id<R>(&self) -> crate::row::Id<R> {
        let n = self.draws.get();
        self.draws.set(n + 1);
        crate::row::Id::new(self.id.draw(n))
    }

    /// Who the intent is from.
    pub fn user(&self) -> &str {
        &self.actor
    }

    /// The device the intent came from: its session.
    pub fn device(&self) -> &str {
        &self.session
    }

    /// Whether the intent was judged under `role`.
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
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
    pub entry: Ctx,
    pub facts: Vec<Change>,
    pub hash: EntryHash,
}

impl Logged {
    /// The hash of the chain to an entry, from the one before it.
    pub fn chain(prev: &EntryHash, seq: Seq, entry: &Ctx, facts: &[Change]) -> EntryHash {
        let b = crate::format::encode(&(seq, entry, facts)).expect("an entry serializes");
        EntryHash(sha256(&[&prev.0, &b]))
    }

    /// An intent not yet sequenced: at sequence 0 and a zero hash, with
    /// what its run changed as its facts — a peer's own pending, or, with
    /// none, as a peer pages it (§1.11).
    pub fn intent(entry: Ctx, facts: Vec<Change>) -> Logged {
        Logged {
            facts,
            ..Logged::base(0, EntryHash::default()).with(entry)
        }
    }

    /// The row a log stands on at `(seq, hash)`: no entry, no facts.
    pub fn base(seq: Seq, hash: EntryHash) -> Logged {
        Logged {
            seq,
            entry: Ctx::default(),
            facts: vec![],
            hash,
        }
    }

    fn with(self, entry: Ctx) -> Logged {
        Logged { entry, ..self }
    }

    /// About how many bytes it is in a frame: what a page is cut by.
    pub fn weight(&self) -> usize {
        let e = &self.entry;
        let rows = |c: &Change| match c {
            Change::Add { row, .. } | Change::Remove { row, .. } => row.len(),
            Change::Edit { old, new, .. } => old.len() + new.len(),
        };
        let facts: usize = self.facts.iter().map(|c| c.table().len() + rows(c)).sum();
        96 + e.func.len() + e.actor.len() + e.session.len() + e.input.len() + facts
    }

    /// Whether this links onto `prev`.
    pub fn links(&self, prev: &EntryHash) -> bool {
        self.hash == Logged::chain(prev, self.seq, &self.entry, &self.facts)
    }
}

/// A store's rows, by table.
pub type Rows = Vec<(String, Vec<Vec<u8>>)>;

/// A store's rows and a log standing on `(seq, hash)` beside them: what a
/// store made durable whole at that head holds, every entry before it
/// folded in.
pub fn standing(mut rows: Rows, seq: Seq, hash: EntryHash) -> Rows {
    rows.push((crate::store::LOG.into(), vec![Logged::base(seq, hash).to_bytes()]));
    rows
}

/// Where a fill stands: at `table`'s first row whose key is `key` or past
/// it, every row before it sent.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub table: String,
    #[serde(with = "crate::format::raw")]
    pub key: Vec<u8>,
}

/// What a peer makes durable, one record per `Send(To::Disk)` (§1.7): a logged
/// entry, appended, as its row of the log's table — its facts are in it —
/// or the store's rows whole, which every entry before them is folded
/// into. A peer opens from its records in order (`Input::Open`): the
/// store's rows as last written whole, the rows appended to the log's
/// table since, whose facts write the rest again, and what a device's
/// driver keeps of it beside them (`Pending`, never a `Send(To::Disk)`): its own
/// intents still pending, each with its preview's changes as its facts,
/// the head of the log those were made at (none: they are run again), and
/// the last `local` it drew.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)]
pub enum Record {
    Logged(Logged),
    Rows(#[serde(with = "crate::format::raw_tables")] Rows),
    Pending {
        pending: Vec<Logged>,
        at: Option<(Seq, EntryHash)>,
        local: i64,
    },
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
