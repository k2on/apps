//! §10 The log, as `Ark.Log` defines it: the module's history, an
//! append-only sequence of entries, each an intent with what applying it
//! changed kept beside it, standing on a snapshot.
//!
//! An entry is an intent: who authored it, the function by hash, the
//! arguments, the autos. The facts beside it are derived, never
//! authoritative, and are what a peer takes for an entry it cannot replay.
//! The horizon is the snapshot the log stands on; entry ids are kept below
//! it too, so a re-pushed intent older than the horizon is recognised.
//!
//! A log has an identity, drawn once when it is created and kept on its
//! snapshot for as long as the log lives (`docs/plan-perf.md` Round 4). A
//! sequence number says where in *a* log a peer is and nothing about which
//! log: a server that lost its log and went on sequencing numbers its new
//! entries from 1 again, and a peer confirmed to 30 of the old one would be
//! handed 31 onwards of the new one on top of a store that never held its
//! first 30. The id is what tells the two apart (§12.4).

use std::collections::{BTreeMap, BTreeSet};

use crate::eval::Args;
use crate::hash::{leaf, state_hash, state_hash_of, Digest, FnHash};
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

/// The state at a sequence, and its hash — and which log it is a state
/// of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub seq: Seq,
    pub store: MemoryStore,
    pub hash: Vec<u8>,
    /// The identity of the log this is a snapshot of; `None` for a log
    /// nobody named — a peer alone's, a test's, one written before logs had
    /// names. Not in the hash: the hash is of the state, and two logs can
    /// reach the same state.
    pub log_id: Option<Id>,
}

/// A snapshot of a store at a sequence, hashed, of no named log.
pub fn snapshot_of(n: Seq, store: MemoryStore) -> Snapshot {
    let hash = state_hash(&store);
    Snapshot {
        seq: n,
        store,
        hash,
        log_id: None,
    }
}

impl Snapshot {
    /// The same snapshot, as one of the log `log_id`.
    pub fn of_log(self, log_id: Option<Id>) -> Snapshot {
        Snapshot { log_id, ..self }
    }
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

    /// The log's identity, which its snapshot carries (the module docs).
    pub fn id(&self) -> Option<Id> {
        self.base.log_id
    }

    /// Name a log nobody has named yet; a log already named keeps its name
    /// — a log's identity is drawn once, when it is created, and moving it
    /// would re-base every peer of it onto a snapshot for nothing. What
    /// the server does with a log it creates or loads from a directory
    /// written before logs had names; `id` is the caller's randomness, since
    /// this crate has none.
    pub fn name_if_unnamed(&mut self, id: Id) {
        self.base.log_id.get_or_insert(id);
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
    /// the answer is that store's hash. What a `Verify` asks, and asked at
    /// the head it used to replay the whole log (`docs/plan-perf.md` R4).
    /// `head` must be the state at [`Log::head_seq`], as `Authority::store`
    /// is.
    ///
    /// Below the head nothing is replayed either (`docs/plan-db.md` D3):
    /// the head's digests (§8.1) with every fact above `n` taken back — an
    /// `add`'s leaf subtracted, a `remove`'s added, an `edit`'s new row
    /// swapped for its old — which is [`Log::state_at`]'s hash at the cost
    /// of the facts since `n` rather than a copy of the store. A peer that
    /// verifies after every settle lands a little below a busy head, and
    /// that is what this answers.
    pub fn hash_at(&self, n: Seq, head: &MemoryStore) -> Option<Vec<u8>> {
        if n == self.head_seq() {
            return Some(state_hash(head));
        }
        if n < self.horizon() || n > self.head_seq() {
            return None;
        }
        let names: Vec<&str> = head.schema().tables().map(|t| t.name.as_str()).collect();
        let mut ds: BTreeMap<&str, Digest> = names.iter().map(|t| (*t, head.digest(t).unwrap_or_default())).collect();
        for (_, (_, facts)) in self.entries.range(n + 1..) {
            for c in facts {
                let Some(d) = ds.get_mut(c.table()) else { continue };
                match c {
                    Change::Add(t, r) => d.sub(&leaf(t, r)),
                    Change::Remove(t, r) => d.add(&leaf(t, r)),
                    Change::Edit(t, old, new) => {
                        d.sub(&leaf(t, new));
                        d.add(&leaf(t, old));
                    }
                }
            }
        }
        Some(state_hash_of(names.into_iter().map(|t| (t, ds[t]))))
    }

    /// §10.3 Move the horizon up to a sequence: snapshot the state there and
    /// drop everything at or under it. Ids are kept, entry ids and the
    /// log's own: it is the same log with less of its past. `None` if the
    /// sequence is not retained.
    pub fn compact_to(&self, n: Seq) -> Option<Log> {
        let st = self.state_at(n)?;
        Some(Log {
            base: snapshot_of(n, st).of_log(self.id()),
            entries: self.entries.range(n + 1..).map(|(k, v)| (*k, v.clone())).collect(),
            ids: self.ids.clone(),
        })
    }

    /// Every entry taken out, oldest first, and the log left standing at
    /// its head with the ids kept: what a peer alone's authority does after
    /// each append, since its entries are kept in its journal and not in
    /// memory (`docs/plan-alone.md` §2). What is left says nothing about the
    /// state below the head — its base's store is whatever it was, and a
    /// peer alone gives it an empty one, since the authority holds the state
    /// at the head — so it is asked only [`Log::head_seq`], [`Log::seq_of`],
    /// [`Log::append`] and [`Log::hash_at`] the head.
    pub fn take_entries(&mut self) -> Vec<(Seq, Entry, Facts)> {
        let head = self.head_seq();
        self.base.seq = head;
        std::mem::take(&mut self.entries).into_iter().map(|(n, (e, f))| (n, e, f)).collect()
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
    /// and nothing replayed; below it (D3), the head's digests with the
    /// facts above taken back, again copying nothing. Both agree with
    /// hashing `state_at`. Falsified by answering the head through
    /// `state_at` again: one store copied; and by answering below it so
    /// (the body this replaced): one store copied there too.
    #[test]
    fn the_hash_at_the_head_replays_nothing() {
        let l = log(50);
        let head = l.state_at(50).unwrap();
        let before = crate::store::clones();
        assert_eq!(l.hash_at(50, &head), Some(state_hash(&head)));
        assert_eq!(crate::store::clones() - before, 0);
        let at_20 = state_hash(&l.state_at(20).unwrap());
        let before = crate::store::clones();
        assert_eq!(l.hash_at(20, &head), Some(at_20));
        assert_eq!(crate::store::clones() - before, 0, "below the head, nothing copied");
        assert_eq!(l.hash_at(0, &head), Some(state_hash(&MemoryStore::empty(schema()))));
        assert_ne!(l.hash_at(20, &head), l.hash_at(50, &head));
        assert_eq!(l.hash_at(51, &head), None);
    }

    /// Round 4: a log's name survives a compaction, a page below the
    /// horizon carries it, and naming a named log changes nothing.
    /// Falsified by `compact_to` building its base with `snapshot_of`
    /// alone: the compacted log is unnamed.
    #[test]
    fn a_log_keeps_its_name() {
        let mut l = log(20);
        assert_eq!(l.id(), None);
        l.name_if_unnamed([7; 16]);
        l.name_if_unnamed([8; 16]);
        assert_eq!(l.id(), Some([7; 16]));
        let c = l.compact_to(10).unwrap();
        assert_eq!(c.id(), Some([7; 16]));
        let Page::BelowHorizon(sn) = c.entries_after(3, 10) else {
            panic!("below the horizon")
        };
        assert_eq!((sn.seq, sn.log_id), (10, Some([7; 16])));
    }
}
