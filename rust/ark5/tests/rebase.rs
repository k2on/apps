//! `rebase/three-peers`, steps 1–4 of `spec/RUNNERS.md` § rebase, re-derived
//! for v5: every confirmed entry is applied by its facts, ids are drawn from
//! the entry, the chain is the log's identity. The states it reaches are
//! pinned here; the interpreter must reach them byte for byte.

#[path = "demo.rs"]
mod demo;
mod support {
    pub mod wire;
}

use ark5::frames::Frame;
use ark5::hash::EntryHash;
use ark5::log::{Durable, EntryId, Logged};
use ark5::peer::{Cas, Input, Output, Peer};
use ark5::store::Change;
use demo::{create_input, pair_input, pos_of, ADD, CREATE, REMOVE, RENAME};
use support::wire::*;

/// The state after the scenario's four entries, on every peer: alice's
/// playlist "Favorites" with t1, t2 and t9 at 1, 2 and 3.
const FINAL: &str = "483c666c329e3d348e2445c3edea9e60aba9554cca7fb2b41b3aeffd2328e2af";

/// The playlist alice's first intent creates: its first draw.
fn pid() -> [u8; 16] {
    EntryId::of("alice-session", 1).draw(0)
}

struct Scenario {
    server: Peer,
    alice: Peer,
    bob: Peer,
    batches: Vec<Frame>,
}

/// Step 4 up to three replicas at `FINAL`, each claim asserted on the way.
fn scenario() -> Scenario {
    let (mut server, mut alice, mut bob) = (authority(demo::cas()), replica("alice", demo::cas()), replica("bob", demo::cas()));
    let mut batches = vec![];
    // step 1: alice creates the playlist; everybody sees it
    let o = alice.step(mutate(CREATE, create_input(" Favorites "), 1));
    let (_, f) = deliver_up(&mut server, &who("alice"), &o);
    batches.extend(f.clone());
    for p in [&mut alice, &mut bob] {
        p.step(down(&f[0]));
    }
    let name = |p: &Peer| p.store.get_row("playlist", &pid());
    assert_eq!(name(&alice), name(&bob));
    assert_eq!(name(&alice), Some(demo::playlist_row(&pid(), "Favorites", "alice")), "the name was trimmed");
    assert!(alice.pending.is_empty() && alice.confirmed_hash() == bob.confirmed_hash());
    // step 2: alice goes dark; bob adds two tracks, alice one alone
    for t in ["t1", "t2"] {
        let o = bob.step(mutate(ADD, pair_input(&pid(), t), 2));
        let (_, f) = deliver_up(&mut server, &who("bob"), &o);
        bob.step(down(&f[0]));
        batches.extend(f);
    }
    let o = alice.step(mutate(ADD, pair_input(&pid(), "t9"), 3));
    assert_eq!(changes(&o).len(), 1, "a local mutation reports its one change");
    assert_eq!(pos_of(&alice.store, &pid(), "t9"), Some(1), "alone, alice's track is first on her view");
    assert_eq!((pos_of(&bob.store, &pid(), "t1"), pos_of(&bob.store, &pid(), "t2")), (Some(1), Some(2)));
    // step 3: bob's entries land on alice, one frame each; hers rebases on top
    let told: usize = batches[1..].iter().map(|b| changes(&alice.step(down(b))).len()).sum();
    assert_eq!(told, 6, "each landing undoes hers, lands bob's and puts hers back");
    assert_eq!(pos_of(&alice.store, &pid(), "t9"), Some(3), "after the rebase alice's track is third");
    assert_eq!(alice.confirmed_hash(), bob.confirmed_hash());
    // alice comes back: hello, and her intent pushed again
    let o = alice.step(Input::Connected);
    assert!(matches!(sends(&o)[..], [Frame::Hello { cursor: 3, .. }, Frame::Push { .. }]));
    let (s, f) = deliver_up(&mut server, &who("alice"), &o);
    assert!(matches!(s[..], [Frame::Ack { seq: 4, .. }]), "{s:?}");
    let l4 = logged(&f[0]);
    assert!(matches!(&l4.facts[..], [Change::Add { row, .. }] if demo::item_of(row).2 == 3), "the authority's facts say pos 3 too");
    let o = alice.step(down(&f[0]));
    assert!(changes(&o).is_empty() && o.contains(&Output::Confirmed(l4.entry.id)), "her own entry, as previewed: nothing moves");
    assert!(alice.step(down(&s[0])).is_empty(), "the ack of what landed is nothing more");
    bob.step(down(&f[0]));
    batches.extend(f);
    assert!(alice.pending.is_empty() && alice.store == bob.store && bob.store == server.store);
    let rows = vec![
        ("playlist".into(), vec![demo::playlist_row(&pid(), "Favorites", "alice")]),
        ("item".into(), [("t1", 1), ("t2", 2), ("t9", 3)].iter().map(|(t, n)| demo::item_row(&pid(), t, *n)).collect()),
    ];
    assert_eq!(server.store, ark5::store::Store::from_rows(demo::shared(), &rows).unwrap(), "the rows the scenario says");
    for p in [&server, &alice, &bob] {
        assert_eq!(p.store.state_hash().hex(), FINAL, "three replicas, one hash");
        assert_eq!(p.log.head(), server.log.head(), "one chain");
    }
    Scenario { server, alice, bob, batches }
}

trait Row {
    fn get_row(&self, table: &str, id: &[u8]) -> Option<Vec<u8>>;
}
impl Row for ark5::store::Store {
    fn get_row(&self, table: &str, id: &[u8]) -> Option<Vec<u8>> {
        use ark5::store::Read;
        self.get(table, &ark5::codec::bytes_enc(id))
    }
}

/// Step 4, the scenario. Falsified by taking the fast path for an own entry
/// whatever its facts (the `changes == facts` test dropped): dave's own
/// entry, confirmed with facts other than his preview's, failed here.
#[test]
fn step_4_the_scenario() {
    let Scenario { mut server, bob, batches, .. } = scenario();
    // a duplicate delivery changes nothing
    let mut b = bob.clone();
    assert!(b.step(down(&batches[1])).is_empty());
    assert!(b.log == bob.log && b.store == bob.store);
    // dave's build of add_to_playlist steps by two. Confirmed entries are
    // facts, so it never runs for anyone else's; his own preview is wrong
    // and the authority's answer puts it right, which is not a divergence.
    let mut dave = replica("dave", demo::daves_cas());
    for f in &batches {
        dave.step(down(f));
    }
    assert_eq!(dave.store.state_hash().hex(), FINAL);
    let o = dave.step(mutate(ADD, pair_input(&pid(), "t3"), 4));
    assert_eq!(pos_of(&dave.store, &pid(), "t3"), Some(5));
    let (_, f) = deliver_up(&mut server, &who("dave"), &o);
    dave.step(down(&f[0]));
    assert_eq!(pos_of(&dave.store, &pid(), "t3"), Some(4));
    assert!(dave.store == server.store && dave.diverged.is_empty() && dave.pending.is_empty());
}

/// Step 1: an authority given the transcript's entries sequences each at its
/// sequence with its facts, and the chain it builds is the chain. Falsified
/// by stamping the entry with the session for the actor: this failed, as
/// did the scenario every other test here starts from.
#[test]
fn step_1_an_authority_sequences_the_transcript() {
    let s = scenario();
    let mut fresh = authority(demo::cas());
    for l in &s.server.log.entries {
        let (acks, f) = deliver_up(&mut fresh, &who(&l.entry.actor), &[Output::Send(
            Frame::Push { space: ark5::schema::Space::Durable, entry: l.entry.clone(), body: None }.enc(),
        )]);
        assert!(matches!(acks[..], [Frame::Ack { seq, .. }] if seq == l.seq));
        assert_eq!(&logged(&f[0]), l, "entry {}: its facts, state and hash", l.seq);
    }
    assert_eq!(fresh.store, s.server.store);
    assert_eq!(fresh.store.state_hash().hex(), FINAL);
}

/// Step 2: a replica given the entries in reverse order, a frame each, asks
/// from where it stands at the first gap and is paged from there. Falsified
/// by applying a batch whatever its sequence (the gap test dropped): the
/// fourth entry's facts went onto an empty store and were refused, a
/// divergence.
#[test]
fn step_2_a_replica_given_the_entries_backwards() {
    let s = scenario();
    let mut r = replica("rita", demo::cas());
    let mut asked = vec![];
    for f in s.batches.iter().rev() {
        asked.extend(sends(&r.step(down(f))));
    }
    assert!(matches!(asked[..], [Frame::Hello { cursor: 0, .. }]), "one hello, at the first gap: {asked:?}");
    assert_eq!(r.log.head().0, 1);
    let mut server = s.server.clone();
    for f in sends(&server.step(up(&who("rita"), &asked[0]))) {
        r.step(down(&f));
    }
    assert_eq!((r.log.head(), r.store.state_hash().hex()), (s.server.log.head(), FINAL.into()));
    assert!(r.diverged.is_empty());
}

/// Step 3: a replica with no functions at all reaches the state by facts.
/// Falsified by `confirm` running the entry's function when it holds one
/// and skipping it otherwise: this replica stayed empty.
#[test]
fn step_3_by_facts_alone() {
    let s = scenario();
    let mut r = replica("nobody", Cas::default());
    for f in &s.batches {
        r.step(down(f));
    }
    assert_eq!(r.store.state_hash().hex(), FINAL);
    assert_eq!(r.log, s.server.log);
}

/// An entry whose state is not what its facts give this peer is a
/// divergence at that sequence: the peer starts again from nothing and the
/// authority's pages heal it. Falsified by not comparing the state after
/// the facts in `confirm`: the forged entry was kept, and the replica's
/// chain was not the authority's.
#[test]
fn a_divergence_is_found_at_its_entry_and_healed() {
    let s = scenario();
    let (l1, mut l2) = (logged(&s.batches[0]), logged(&s.batches[1]));
    if let Change::Add { row, .. } = &mut l2.facts[0] {
        *row = demo::item_row(&pid(), "t1", 7);
    }
    l2.hash = Logged::chain(&l1.hash, 2, &l2.entry, &l2.facts, &l2.state);
    let mut carol = replica("carol", demo::cas());
    carol.step(down(&s.batches[0]));
    let o = carol.step(down(&Frame::Batch { space: ark5::schema::Space::Durable, from: 1, logged: vec![l2] }));
    assert_eq!(carol.diverged, vec![2]);
    let hello = sends(&o);
    assert!(matches!(hello[..], [Frame::Hello { cursor: 0, hash: None, .. }]));
    let mut server = s.server.clone();
    for f in sends(&server.step(up(&who("carol"), &hello[0]))) {
        carol.step(down(&f));
    }
    assert_eq!((carol.log.clone(), carol.store.state_hash().hex()), (s.server.log.clone(), FINAL.into()));
}

/// Compaction: the horizon at 2; a hello at 0 is answered with a snapshot a
/// replica stands on, one at 2 with the tail. Falsified by `Log::has`
/// answering for any sequence below the base: the hello at 0 was paged from
/// an entry the log no longer has.
#[test]
fn compaction_and_the_horizon() {
    let mut server = scenario().server;
    let h2 = server.log.at(2).unwrap().hash;
    let o = server.step(Input::Compact(2));
    assert!(o.iter().all(|x| matches!(x, Output::Persist(_))));
    assert_eq!(server.log.base.seq, 2);
    let hello = |cursor, hash| Frame::Hello { space: ark5::schema::Space::Durable, cursor, hash: Some(hash), device: None };
    let at0 = sends(&server.step(up(&who("x"), &hello(0, EntryHash::default()))));
    let mut r = replica("x", Cas::default());
    assert!(matches!(at0[..], [Frame::Snapshot { .. }]));
    r.step(down(&at0[0]));
    assert_eq!((r.log.head(), r.store.state_hash().hex()), (server.log.head(), FINAL.into()));
    let at2 = sends(&server.step(up(&who("x"), &hello(2, h2))));
    assert!(matches!(&at2[..], [Frame::Batch { from: 2, logged, .. }] if logged.iter().map(|l| l.seq).collect::<Vec<_>>() == [3, 4]));
}

/// Every change kind rebases: bob renames the playlist and removes a track
/// while alice, dark, adds one and renames it twice; landing his under her
/// pending intents undoes them newest-first, applies the edit and the
/// removal, and runs hers again. The stores agree. Falsified by undoing
/// pending changes oldest-first: the first rename's undo was refused.
#[test]
fn edits_and_removes_rebase_too() {
    let Scenario { mut server, mut alice, mut bob, .. } = scenario();
    let o1 = alice.step(mutate(ADD, pair_input(&pid(), "t4"), 5));
    let o2 = alice.step(mutate(RENAME, pair_input(&pid(), "Old"), 6));
    let o3 = alice.step(mutate(RENAME, pair_input(&pid(), "Older"), 6));
    for (name, input) in [(RENAME, pair_input(&pid(), "Best")), (REMOVE, pair_input(&pid(), "t2"))] {
        let o = bob.step(mutate(name, input, 7));
        let (_, f) = deliver_up(&mut server, &who("bob"), &o);
        bob.step(down(&f[0]));
        alice.step(down(&f[0]));
    }
    assert_eq!(pos_of(&alice.store, &pid(), "t2"), None);
    for o in [o1, o2, o3] {
        let (_, f) = deliver_up(&mut server, &who("alice"), &o);
        bob.step(down(&f[0]));
        alice.step(down(&f[0]));
    }
    assert!(alice.pending.is_empty() && alice.store == server.store && bob.store == server.store);
    assert_eq!(pos_of(&alice.store, &pid(), "t4"), Some(4));
}

/// A function the authority lacks is held until its body comes, then run.
/// Falsified by the client answering `Held` without the body: the
/// authority held it again and nothing was sequenced.
#[test]
fn held_until_the_body_comes() {
    let loader: ark5::peer::Loader = std::sync::Arc::new(|b: &[u8]| demo::procedure(std::str::from_utf8(b).ok()?));
    let mut lacking = Cas::default().with_loader(loader.clone());
    lacking.add_body(CREATE.as_bytes().to_vec());
    let mut server = authority(lacking);
    let mut knows = Cas::default().with_loader(loader);
    knows.add_body(CREATE.as_bytes().to_vec());
    knows.add_body(ADD.as_bytes().to_vec());
    let mut alice = replica("alice", knows);
    let o = alice.step(mutate(CREATE, create_input("Road"), 1));
    deliver_up(&mut server, &who("alice"), &o);
    let o = alice.step(mutate(ADD, pair_input(&pid(), "t1"), 2));
    let (s, _) = deliver_up(&mut server, &who("alice"), &o);
    assert!(matches!(s[..], [Frame::Held { .. }]));
    let o = alice.step(down(&s[0]));
    let (s, f) = deliver_up(&mut server, &who("alice"), &o);
    assert!(matches!(s[..], [Frame::Ack { seq: 2, .. }]), "{s:?}");
    assert_eq!(f.len(), 1);
}

/// What a peer persisted opens it again: the same log and store, its
/// pending intents run on top. Falsified by `Durable::from_records` keeping
/// a record that does not link: the cut journal's stray entry was taken.
#[test]
fn a_peer_opens_from_what_it_persisted() {
    let s = scenario();
    let mut records: Vec<Vec<u8>> = vec![];
    let mut r = replica("bob", demo::cas());
    for f in &s.batches {
        for o in r.step(down(f)) {
            if let Output::Persist(b) = o {
                records.push(b);
            }
        }
    }
    let mut stray = logged(&s.batches[3]);
    stray.seq = 9;
    records.push(ark5::log::Record::Logged(stray).enc());
    let base = s.bob.log.base.clone();
    let mut d = Durable::from_records(base, &records);
    assert_eq!(d.chain.len(), 4, "the journal stops where it stops linking");
    let pending = s.alice.clone().step(mutate(ADD, pair_input(&pid(), "t5"), 9));
    d.pending = sends(&pending)
        .into_iter()
        .map(|f| match f {
            Frame::Push { entry, .. } => entry,
            _ => unreachable!(),
        })
        .collect();
    let mut opened = replica("alice", demo::cas());
    opened.step(Input::Open(d));
    assert_eq!(opened.log, s.bob.log);
    assert_eq!(opened.confirmed_hash().hex(), FINAL);
    assert_eq!(pos_of(&opened.store, &pid(), "t5"), Some(4));
}
