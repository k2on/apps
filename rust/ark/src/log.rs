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
#[derive(Debug, PartialEq, Eq)]
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

/// Written out rather than derived so that this crate's tests can count
/// the copies: a page of the log is held to cloning the entries it sends
/// and no others (`docs/plan-perf.md` R4, `tests::a_page_clones_itself`).
impl Clone for Entry {
    fn clone(&self) -> Entry {
        #[cfg(test)]
        CLONES.with(|n| n.set(n.get() + 1));
        Entry {
            id: self.id,
            actor: self.actor.clone(),
            session: self.session.clone(),
            fn_hash: self.fn_hash.clone(),
            args: self.args.clone(),
            autos: self.autos.clone(),
        }
    }
}

// Per thread, as the store's count is.
#[cfg(test)]
thread_local! {
    static CLONES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
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
    /// cursor is below the horizon. Only the page is copied, and whether
    /// more follow is whether the range has one more: a connection at 0 of
    /// a log of ten thousand used to cost ten thousand entries cloned per
    /// page of a hundred (`docs/plan-perf.md` R4).
    pub fn entries_after(&self, cursor: Seq, limit: usize) -> Page {
        if cursor < self.horizon() {
            return Page::BelowHorizon(self.base.clone());
        }
        let mut after = self.entries.range(cursor + 1..);
        let page: Vec<(Seq, Entry, Facts)> = after.by_ref().take(limit).map(|(s, (e, f))| (*s, e.clone(), f.clone())).collect();
        Page::Entries(page, after.next().is_some())
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

    /// The state hash at a retained sequence, given the store at the head
    /// — which the authority holds, so at the head nothing is replayed and
    /// the answer is that store's hash; below it, [`Log::state_at`]'s.
    /// What a `Verify` asks, and asked at the head it used to replay the
    /// whole log (`docs/plan-perf.md` R4). `head` must be the state at
    /// [`Log::head_seq`], as `Authority::store` is.
    pub fn hash_at(&self, n: Seq, head: &MemoryStore) -> Option<Vec<u8>> {
        if n == self.head_seq() {
            return Some(state_hash(head));
        }
        self.state_at(n).map(|st| state_hash(&st))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Column, Table, Ty};
    use crate::value::Value;

    fn clones() -> usize {
        CLONES.with(|n| n.get())
    }

    fn schema() -> Schema {
        Schema {
            tables: vec![Table {
                name: "t".into(),
                columns: vec![Column {
                    name: "id".into(),
                    ty: Ty::Int,
                    nullable: false,
                }],
                key: vec!["id".into()],
                indexes: vec![],
                refs: vec![],
            }],
        }
    }

    // A log of `n` entries, each adding one row.
    fn log(n: i64) -> Log {
        let mut l = Log::empty(schema());
        for i in 1..=n {
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
            l.append(e, vec![Change::Add("t".into(), row)]);
        }
        l
    }

    /// R4: a page clones the entries it holds and no others, wherever the
    /// cursor is, and says whether more follow. Falsified by the old body
    /// (clone every entry after the cursor, then take a page): 1,000
    /// clones for the page of ten at 0.
    #[test]
    fn a_page_clones_itself() {
        let l = log(1000);
        for (cursor, limit, len, more) in [
            (0, 10, 10, true),
            (500, 10, 10, true),
            (990, 10, 10, false),
            (995, 10, 5, false),
            (1000, 10, 0, false),
        ] {
            let before = clones();
            let Page::Entries(page, m) = l.entries_after(cursor, limit) else {
                panic!("above the horizon")
            };
            assert_eq!(clones() - before, len, "cursor {cursor}");
            assert_eq!((page.len(), m), (len, more), "cursor {cursor}");
            assert_eq!(page.first().map(|(s, _, _)| *s), (len > 0).then_some(cursor + 1));
        }
    }

    /// R4: the hash at the head is the head store's, with no store copied
    /// and nothing replayed; below it, the replay's. Both agree with
    /// hashing `state_at`. Falsified by answering the head through
    /// `state_at` again: one store copied.
    #[test]
    fn the_hash_at_the_head_replays_nothing() {
        let l = log(50);
        let head = l.state_at(50).unwrap();
        let before = crate::store::clones();
        assert_eq!(l.hash_at(50, &head), Some(state_hash(&head)));
        assert_eq!(crate::store::clones() - before, 0);
        let at_20 = state_hash(&l.state_at(20).unwrap());
        assert_eq!(l.hash_at(20, &head), Some(at_20));
        assert_ne!(l.hash_at(20, &head), l.hash_at(50, &head));
        assert_eq!(l.hash_at(51, &head), None);
    }
}
