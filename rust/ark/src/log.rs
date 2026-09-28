//! §10 The log, as `Ark.Log` defines it: the module's history, an
//! append-only sequence of entries, each an intent with what applying it
//! changed kept beside it, standing on a snapshot.
//!
//! An entry is an intent: who authored it, the function by hash, the
//! arguments, the autos. The facts beside it are derived, never
//! authoritative, and are what a peer takes for an entry it cannot replay.
//! The horizon is the snapshot the log stands on; entry ids are kept below
//! it too, so a re-pushed intent older than the horizon is recognised.

use std::collections::{BTreeMap, BTreeSet};

use crate::eval::Args;
use crate::hash::{state_hash, FnHash};
use crate::schema::Schema;
use crate::store::{Change, MemoryStore, Store};
use crate::value::Id;

/// A position in the log. The first entry is 1; 0 is "nothing".
pub type Seq = i64;

/// An intent, as recorded (`Ark.Log.Entry`). The sequence is not a field:
/// it is the key an entry is stored under once the authority assigned it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Chosen by the originating peer; the dedupe key.
    pub id: Id,
    /// The user the authority verified for the connection.
    pub actor: String,
    /// The login it was authored under.
    pub session: String,
    /// The closure that authored it (§8.3).
    pub fn_hash: FnHash,
    pub args: Args,
    pub autos: Args,
}

/// What applying an entry changed, in order.
pub type Facts = Vec<Change>;

/// The state at a sequence, and its hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub seq: Seq,
    pub store: MemoryStore,
    pub hash: Vec<u8>,
}

/// A snapshot of a store at a sequence, hashed.
pub fn snapshot_of(n: Seq, store: MemoryStore) -> Snapshot {
    let hash = state_hash(&store);
    Snapshot { seq: n, store, hash }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Log {
    pub base: Snapshot,
    /// Every sequence above `base`, contiguous.
    pub entries: BTreeMap<Seq, (Entry, Facts)>,
    /// Every entry id ever sequenced, kept below the horizon too.
    pub ids: BTreeMap<Id, Seq>,
}

/// What a peer at a cursor is sent next (`Ark.Log.Page`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Page {
    /// The entries after the cursor, at most a batch, and whether more
    /// follow.
    Entries(Vec<(Seq, Entry, Facts)>, bool),
    /// The cursor is below the horizon: start again from the snapshot.
    BelowHorizon(Snapshot),
}

impl Log {
    /// A log with nothing in it, standing on the empty store at 0.
    pub fn empty(sch: Schema) -> Log {
        Log {
            base: snapshot_of(0, MemoryStore::empty(sch)),
            entries: BTreeMap::new(),
            ids: BTreeMap::new(),
        }
    }

    /// The last sequence assigned.
    pub fn head_seq(&self) -> Seq {
        self.entries.keys().next_back().copied().unwrap_or(self.base.seq)
    }

    /// The sequence the log stands on: entries at or below it are gone.
    pub fn horizon(&self) -> Seq {
        self.base.seq
    }

    /// §10.1 Append an entry the authority has applied, with what it
    /// changed; the sequence is `head + 1`. The caller has already checked
    /// `seq_of` for a duplicate.
    pub fn append(&mut self, e: Entry, facts: Facts) -> Seq {
        let n = self.head_seq() + 1;
        self.ids.insert(e.id, n);
        self.entries.insert(n, (e, facts));
        n
    }

    /// The sequence an entry id was given, if it ever was.
    pub fn seq_of(&self, id: &Id) -> Option<Seq> {
        self.ids.get(id).copied()
    }

    /// What a peer at a cursor is sent next: a page, or the snapshot if the
    /// cursor is below the horizon.
    pub fn entries_after(&self, cursor: Seq, limit: usize) -> Page {
        if cursor < self.horizon() {
            return Page::BelowHorizon(self.base.clone());
        }
        let after: Vec<(Seq, Entry, Facts)> = self.entries.range(cursor + 1..).map(|(s, (e, f))| (*s, e.clone(), f.clone())).collect();
        let more = after.len() > limit;
        Page::Entries(after.into_iter().take(limit).collect(), more)
    }

    /// §10.2 The state at any retained sequence, from the snapshot and the
    /// facts alone — no function is run.
    pub fn state_at(&self, n: Seq) -> Option<MemoryStore> {
        if n < self.horizon() || n > self.head_seq() {
            return None;
        }
        let mut st = self.base.store.clone();
        for (_, (_, f)) in self.entries.range(..=n) {
            st.apply_changes(f);
        }
        Some(st)
    }

    /// §10.3 Move the horizon up to a sequence: snapshot the state there and
    /// drop everything at or under it. Ids are kept. `None` if the sequence
    /// is not retained.
    pub fn compact_to(&self, n: Seq) -> Option<Log> {
        let st = self.state_at(n)?;
        Some(Log {
            base: snapshot_of(n, st),
            entries: self.entries.range(n + 1..).map(|(k, v)| (*k, v.clone())).collect(),
            ids: self.ids.clone(),
        })
    }

    /// The function hashes the retained entries name.
    pub fn named_hashes(&self) -> BTreeSet<FnHash> {
        self.entries.values().map(|(e, _)| e.fn_hash.clone()).collect()
    }

    /// Whether the entries run without a gap from the sequence after the
    /// snapshot to the head.
    pub fn contiguous(&self) -> bool {
        let want: Vec<Seq> = (self.horizon() + 1..=self.head_seq()).collect();
        self.entries.keys().copied().collect::<Vec<_>>() == want
    }
}
