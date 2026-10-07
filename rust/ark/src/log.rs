//! §10 The log, as `Ark.Log` defines it: the module's history, an
//! append-only sequence of entries, each an intent with what applying it
//! changed kept beside it, standing on a snapshot.
//!
//! An entry is an intent: who authored it, the function by hash, the
//! arguments, the autos. The facts beside it are derived, never
//! authoritative, and are what a peer takes for an entry it cannot replay.
//! The horizon is the snapshot the log stands on; entry ids are kept below
//! it too — by an 8-byte key rather than whole ([`Below`]) — so a re-pushed intent older than the horizon is recognised.
//!
//! A log has an identity, drawn once when it is created and kept on its
//! snapshot for as long as the log lives (`docs/plan-perf.md` Round 4). A
//! sequence number says where in *a* log a peer is and nothing about which
//! log: a server that lost its log and went on sequencing numbers its new
//! entries from 1 again, and a peer confirmed to 30 of the old one would be
//! handed 31 onwards of the new one on top of a store that never held its
//! first 30. The id is what tells the two apart (§12.4).

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use crate::eval::Args;
use crate::hash::{leaf, state_hash, state_hash_of, Digest, FnHash};
use crate::schema::Schema;
use crate::store::{project_row, Change, MemoryStore, Row, Store};
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
    /// The roles the author held, frozen with the entry as the actor and
    /// the session are (`docs/plan-guards.md` D1): a body that reads one
    /// (`Expr::HasRole`) replays with what the authority judged it under,
    /// on every peer and at every sequence. The device authors with the
    /// roles it believes; the authority **stamps** the entry with the
    /// connection's at sequencing ([`crate::protocol::Server`]), so what
    /// the log holds is the truth and never a device's claim. On the wire
    /// and on disk `roles`, a list of texts in ascending order, present
    /// only when not empty — so every entry from before roles is the bytes
    /// it was (`crate::protocol::entry_value`).
    pub roles: BTreeSet<String>,
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
            roles: self.roles.clone(),
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
    /// Every entry id sequenced above the horizon, exactly — and below it,
    /// those not yet folded into `below` ([`Log::fold_ids`]).
    pub ids: BTreeMap<Id, Seq>,
    /// The ids below the horizon, kept as 8-byte keys rather than whole
    /// (`docs/plan-db.md` D6; [`Below`]).
    pub below: Below,
}

/// The ids of the entries at or below the horizon, each as an 8-byte key
/// and the sequence it was given (`docs/plan-db.md` D6).
///
/// **Why.** A log keeps every id it ever sequenced, below the horizon too,
/// so that a re-push of an intent older than the snapshot is answered
/// `Duplicate` rather than applied twice (§10.3). Above the horizon the
/// ids are what a page is checked against and are kept exactly; below it
/// the only question asked of one is "was this sequenced, and where" — and
/// the answer may be wrong with probability 2^-64 per id held, which for a
/// random id is never. A `BTreeMap<Id, Seq>` costs about 40 bytes an id
/// (24 of payload and the tree's nodes around it); a sorted vector of
/// `(u64, Seq)` costs 16, and a server that has run for years holds
/// millions (`rust/ark/tests/perf.rs`, `perf_ids_below_the_horizon`).
///
/// **The key is the first 8 bytes of the id's SHA-256, not of the id.** An
/// id is chosen by the peer that authored the intent, and nothing makes its
/// first half random: a UUID's version nibble is there, and an id built
/// from a counter — every test's, and any peer's that numbers its own —
/// differs only in its last bytes, so its prefix is every other one's. A
/// digest's prefix is uniform whatever the id looks like.
///
/// **A false positive** — a fresh intent whose key one below the horizon
/// already has — is answered `Duplicate` at that sequence and never
/// applied. That is the price, stated: 2^-64 per pair. Two ids below the
/// horizon with one key are both kept, and the lower sequence is the
/// answer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Below {
    /// `(key, seq)`, sorted.
    keys: Vec<(u64, Seq)>,
}

impl Below {
    /// The key an id is kept under.
    pub fn key(id: &Id) -> u64 {
        let h = crate::sha256::sha256(id);
        u64::from_be_bytes([h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]])
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// The sequence an id below the horizon was given, if its key is held.
    pub fn get(&self, id: &Id) -> Option<Seq> {
        self.get_key(Below::key(id))
    }

    /// The sequence held under a key: the lowest, where two share it.
    pub fn get_key(&self, key: u64) -> Option<Seq> {
        let at = self.keys.partition_point(|(k, _)| *k < key);
        self.keys.get(at).filter(|(k, _)| *k == key).map(|(_, n)| *n)
    }

    /// Every key and its sequence, in key order.
    pub fn iter(&self) -> impl Iterator<Item = (u64, Seq)> + '_ {
        self.keys.iter().copied()
    }

    /// Add keys: sorted, and merged with those held in one pass, so a fold
    /// costs the ids held and not their square.
    pub fn extend(&mut self, more: impl IntoIterator<Item = (u64, Seq)>) {
        let mut more: Vec<(u64, Seq)> = more.into_iter().collect();
        if more.is_empty() {
            return;
        }
        more.sort_unstable();
        let old = std::mem::take(&mut self.keys);
        let mut out = Vec::with_capacity(old.len() + more.len());
        let (mut a, mut b) = (old.into_iter().peekable(), more.into_iter().peekable());
        loop {
            let next = match (a.peek(), b.peek()) {
                (Some(x), Some(y)) if x <= y => a.next(),
                (Some(_), Some(_)) => b.next(),
                (Some(_), None) => a.next(),
                (None, Some(_)) => b.next(),
                (None, None) => break,
            };
            out.extend(next);
        }
        self.keys = out;
    }

    /// The bytes the keys take on the heap.
    pub fn heap_bytes(&self) -> usize {
        self.keys.capacity() * std::mem::size_of::<(u64, Seq)>()
    }
}

/// A fact as a state of this schema holds it (`docs/plan-db.md` D1): a row
/// that names nothing the table lacks but leaves out a nullable column — a
/// fact sequenced before the module grew that column — laid out with the
/// column `Null`, as the server widens its head (`ark_server::persist::
/// widen`) and as a peer of this module that runs the entry again writes
/// it; any other change as it is. So the state below the head, and its
/// hash, agree with the head about rows that predate a column — without it
/// a `Verify` below the head over such facts took a raw row's leaf off a
/// widened row's digest and disagreed with every peer. A fact sequenced
/// under this schema has every column and is returned as it is.
/// Borrowed where nothing is filled, which is every fact of a log written
/// under one schema.
pub fn widened<'c>(sch: &Schema, c: &'c Change) -> Cow<'c, Change> {
    let Some(tbl) = sch.lookup_table(c.table()) else {
        return Cow::Borrowed(c);
    };
    let short = |r: &Row| r.len() < tbl.columns.len() && r.keys().all(|k| tbl.column(k).is_some());
    let fill = |r: &Row| {
        if short(r) {
            project_row(tbl, r).unwrap_or_else(|_| r.clone())
        } else {
            r.clone()
        }
    };
    Cow::Owned(match c {
        Change::Add(t, r) if short(r) => Change::Add(t.clone(), fill(r)),
        Change::Remove(t, r) if short(r) => Change::Remove(t.clone(), fill(r)),
        Change::Edit(t, o, r) if short(o) || short(r) => Change::Edit(t.clone(), fill(o), fill(r)),
        _ => return Cow::Borrowed(c),
    })
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
            below: Below::default(),
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

    /// The sequence an entry id was given, if it ever was: exactly above
    /// the horizon, and by its key below it ([`Below`]).
    pub fn seq_of(&self, id: &Id) -> Option<Seq> {
        self.ids.get(id).copied().or_else(|| self.below.get(id))
    }

    /// How many ids the log holds, exact and keyed.
    pub fn id_count(&self) -> usize {
        self.ids.len() + self.below.len()
    }

    /// Fold every id at or below the horizon into [`Below`]: what the
    /// server does after each compaction and when it opens its log
    /// (`ark-server`'s hub). Costs the ids held, once per move of the
    /// horizon — which the retention rule makes rare (`ark::retention`).
    /// Not done by [`Log::compact_to`] or by reading a log back, so that a
    /// log written and read is the log it was; a log is folded where memory
    /// is the point. A peer alone's log, whose horizon is its head after
    /// every append ([`Log::take_entries`]), is never folded: its ids are
    /// its local history's, and it keeps them exactly.
    pub fn fold_ids(&mut self) {
        let horizon = self.horizon();
        if !self.ids.values().any(|n| *n <= horizon) {
            return;
        }
        let mut folded = vec![];
        let kept: BTreeMap<Id, Seq> = std::mem::take(&mut self.ids)
            .into_iter()
            .filter(|(id, n)| {
                if *n <= horizon {
                    folded.push((Below::key(id), *n));
                    false
                } else {
                    true
                }
            })
            .collect();
        self.ids = kept;
        self.below.extend(folded);
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
            for c in f {
                st.apply_change(&widened(st.schema(), c));
            }
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
                let c = &*widened(head.schema(), c);
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
    /// log's own: it is the same log with less of its past. They are kept
    /// as they were — whole — as `Authority::compact` keeps them; folding
    /// the ones now below the horizon into their keys is
    /// [`Log::fold_ids`], which the server does after a compaction. `None`
    /// if the sequence is not retained.
    pub fn compact_to(&self, n: Seq) -> Option<Log> {
        let st = self.state_at(n)?;
        Some(Log {
            base: snapshot_of(n, st).of_log(self.id()),
            entries: self.entries.range(n + 1..).map(|(k, v)| (*k, v.clone())).collect(),
            ids: self.ids.clone(),
            below: self.below.clone(),
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
                roles: BTreeSet::new(),
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

    /// D6: an id below the horizon is held by its key, and a re-push of it
    /// is still answered `Duplicate` at the sequence it was given — through
    /// the authority, as the server asks. Above the horizon ids stay exact.
    /// Falsified by forgetting the keys (`seq_of` reading `ids` alone): the
    /// eighty below the horizon are unknown — id 1 first — and the authority
    /// would run the re-push rather than answer the duplicate it is.
    #[test]
    fn a_duplicate_below_the_horizon_is_still_refused() {
        let mut l = log(100).compact_to(80).unwrap();
        l.fold_ids();
        assert_eq!((l.ids.len(), l.below.len(), l.id_count()), (20, 80, 100));
        for n in 1..=100i64 {
            let mut id = [0u8; 16];
            id[8..].copy_from_slice(&n.to_be_bytes());
            assert_eq!(l.seq_of(&id), Some(n), "id {n}");
        }
        let mut fresh = [0u8; 16];
        fresh[8..].copy_from_slice(&101i64.to_be_bytes());
        assert_eq!(l.seq_of(&fresh), None);

        let mut a = crate::peer::Authority::new(schema(), BTreeMap::new());
        a.store = l.state_at(100).unwrap();
        a.log = l;
        let mut id = [0u8; 16];
        id[8..].copy_from_slice(&7i64.to_be_bytes());
        let again = Entry {
            id,
            actor: "a".into(),
            session: "s".into(),
            roles: BTreeSet::new(),
            fn_hash: vec![1],
            args: Args::new(),
            autos: Args::new(),
        };
        assert!(matches!(a.sequence_entry(&again), crate::peer::Sequenced::Duplicate(7)));
        // Folding twice changes nothing, and two ids of one key are both
        // kept, the lower sequence the answer.
        let mut twice = a.log.clone();
        twice.fold_ids();
        assert_eq!(twice, a.log);
        let mut b = Below::default();
        b.extend([(5, 9), (5, 3), (1, 4)]);
        assert_eq!((b.get_key(5), b.get_key(1), b.get_key(2), b.len()), (Some(3), Some(4), None, 3));
    }

    /// D6: a folded log is written with its keys and read back with them,
    /// so a server restarted still answers a re-push below its horizon.
    /// Falsified by writing the exact ids alone (`journal::log_to_value`
    /// without the keys): the log read back is not the one written — it
    /// holds nothing below the horizon, so not id 7.
    #[test]
    fn a_folded_log_survives_its_file() {
        let mut l = log(50).compact_to(30).unwrap();
        l.fold_ids();
        let back = crate::journal::log_from_value(&schema(), &crate::journal::log_to_value(&l)).unwrap();
        assert_eq!(back, l);
        let mut id = [0u8; 16];
        id[8..].copy_from_slice(&7i64.to_be_bytes());
        assert_eq!(back.seq_of(&id), Some(7));
    }
}
