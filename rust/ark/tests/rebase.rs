//! `rebase/three-peers`, steps 1–4 of `spec/RUNNERS.md` § rebase, re-derived
//! for v5: every confirmed entry is applied by its facts, ids are drawn from
//! the entry, the chain is the log's identity. The states it reaches are
//! pinned here; the domain in Rust must reach them byte for byte.

#[path = "demo.rs"]
mod demo;
mod support {
    pub mod wire;
}

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use ark::format::Encode;
use ark::frames::{Frame, Verdict};
use ark::hash::EntryHash;
use ark::log::{EntryId, Logged, Origin, Record};
use ark::peer::{Ctx, Input, Output, Peer, To};
use ark::store::{Change, LOG};
use demo::{create_input, pair_input, pos_of, rename_input, ADD, CREATE, REMOVE, RENAME};
use support::wire::*;

/// The state after the scenario's four entries, on every peer: alice's
/// playlist "Favorites" with t1, t2 and t9 at 1, 2 and 3. N1 moved it
/// (`bb0abc1a…` before): rows are their JSON, and an id is drawn from
/// `(origin, local)` without the codec; the scenario is as it was. N4
/// made the value format postcard (`dbd4f870…` as JSON).
const FINAL: &str = "bde6d903ffaacfaf561e66bd8cfd41a15b3efa5c56a55f80c749eac0bc78a87e";

/// The playlist alice's first intent creates: its first draw.
fn pid() -> [u8; 16] {
    EntryId::of(Origin::named("alice-session"), 1).draw(0)
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
    assert_eq!(
        name(&alice),
        Some(demo::playlist_row(&pid(), "Favorites", "alice")),
        "the name was trimmed"
    );
    assert!(alice.pending.is_empty() && alice.log().head() == bob.log().head());
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
    assert_eq!(alice.log().head(), bob.log().head());
    // alice comes back: hello, and her intent pushed again
    let o = alice.step(Input::Connected);
    assert!(matches!(sends(&o)[..], [Frame::Hello { cursor: 3, .. }, Frame::Page { .. }]));
    let (s, f) = deliver_up(&mut server, &who("alice"), &o);
    assert!(matches!(verdicts(&s)[..], [Some(Verdict::At { seq: 4, .. })]), "{s:?}");
    let l4 = logged(&f[0]);
    assert!(
        matches!(&l4.facts[..], [Change::Add { row, .. }] if demo::item_of(row).2 == 3),
        "the authority's facts say pos 3 too"
    );
    let o = alice.step(down(&f[0]));
    assert!(
        changes(&o).is_empty() && o.contains(&Output::Settled(l4.entry.id, Ok(()))),
        "her own entry, as previewed: nothing moves"
    );
    assert!(alice.step(down(&s[0])).is_empty(), "the ack of what landed is nothing more");
    bob.step(down(&f[0]));
    batches.extend(f);
    assert!(alice.pending.is_empty() && alice.store == bob.store && bob.store == server.store);
    let rows = vec![
        ("playlist".into(), vec![demo::playlist_row(&pid(), "Favorites", "alice")]),
        (
            "item".into(),
            [("t1", 1), ("t2", 2), ("t9", 3)]
                .iter()
                .map(|(t, n)| demo::item_row(&pid(), t, *n))
                .collect(),
        ),
    ];
    assert_eq!(
        server.store,
        ark::store::Store::from_rows(demo::shared(), ark::schema::Space::Durable, &rows).unwrap(),
        "the rows the scenario says"
    );
    for p in [&server, &alice, &bob] {
        assert_eq!(p.store.state_hash().hex(), FINAL, "three replicas, one hash");
        assert_eq!(p.log().head(), server.log().head(), "one chain");
    }
    Scenario { server, alice, bob, batches }
}

trait Row {
    fn get_row(&self, table: &str, id: &[u8]) -> Option<Vec<u8>>;
}
impl Row for ark::store::Store {
    fn get_row(&self, table: &str, id: &[u8]) -> Option<Vec<u8>> {
        use ark::store::Read;
        let mut k = vec![];
        ark::format::put_key(&ark::row::Id::<()>::new(id.try_into().unwrap()), &mut k);
        self.get(table, &k)
    }
}

/// Step 4, the scenario. Falsified by taking the fast path for an own entry
/// whatever its facts (the `changes == facts` test dropped): dave's own
/// entry, confirmed with facts other than his preview's, failed here.
#[test]
fn step_4_the_scenario() {
    let Scenario {
        mut server, bob, batches, ..
    } = scenario();
    // a duplicate delivery changes nothing
    let mut b = bob.clone();
    assert!(b.step(down(&batches[1])).is_empty());
    assert!(b.log() == bob.log() && b.store == bob.store);
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
    for l in &s.server.log().entries() {
        let (acks, f) = deliver_up(
            &mut fresh,
            &who(&l.entry.actor),
            &[Output::Send(
                To::Back,
                Frame::push(ark::schema::Space::Durable, 0, [l.entry.clone()]).enc(),
            )],
        );
        assert!(matches!(verdicts(&acks)[..], [Some(Verdict::At { seq, .. })] if seq == l.seq));
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
    assert!(
        matches!(asked[..], [Frame::Hello { cursor: 0, .. }]),
        "one hello, at the first gap: {asked:?}"
    );
    assert_eq!(r.log().head().0, 1);
    let mut server = s.server.clone();
    for f in sends(&server.step(up(&who("rita"), &asked[0]))) {
        r.step(down(&f));
    }
    assert_eq!((r.log().head(), r.store.state_hash().hex()), (s.server.log().head(), FINAL.into()));
    assert!(r.diverged.is_empty());
}

/// Step 3: a replica with no functions at all reaches the state by facts.
/// Falsified by `confirm` running the entry's function when it holds one
/// and skipping it otherwise: this replica stayed empty.
#[test]
fn step_3_by_facts_alone() {
    let s = scenario();
    let mut r = replica("nobody", demo::only(&[]));
    for f in &s.batches {
        r.step(down(f));
    }
    assert_eq!(r.store.state_hash().hex(), FINAL);
    assert_eq!(r.log(), s.server.log());
}

/// An entry whose facts do not apply to what this peer holds is a
/// divergence at that sequence: the peer starts again from nothing and the
/// authority's fill heals it. Falsified by `confirm` putting facts as they
/// are outside a fill: the forged entry was kept, and the replica's chain
/// was not the authority's.
#[test]
fn a_divergence_is_found_at_its_entry_and_healed() {
    let s = scenario();
    let (l1, mut l2) = (logged(&s.batches[0]), logged(&s.batches[1]));
    l2.facts = vec![Change::Remove {
        table: "item".into(),
        row: demo::item_row(&pid(), "t1", 7),
    }];
    l2.hash = Logged::chain(&l1.hash, 2, &l2.entry, &l2.facts);
    let mut carol = replica("carol", demo::cas());
    carol.step(down(&s.batches[0]));
    let o = carol.step(down(&Frame::Page {
        space: ark::schema::Space::Durable,
        from: 1,
        logged: vec![l2],
    }));
    assert_eq!(carol.diverged, vec![2]);
    let hello = sends(&o);
    // The fill begun again: its first table asked for from its start.
    assert!(matches!(&hello[..], [Frame::Hello { cursor: 2, fill: Some(from), .. }] if from.key.is_empty()));
    let mut server = s.server.clone();
    let mut up_frames = hello;
    while !up_frames.is_empty() {
        let back: Vec<Frame> = up_frames.drain(..).flat_map(|f| sends(&server.step(up(&who("carol"), &f)))).collect();
        up_frames = back.iter().flat_map(|f| sends(&carol.step(down(f)))).collect();
    }
    let healed = (carol.log().head(), carol.store.state_hash().hex());
    assert_eq!(healed, (s.server.log().head(), FINAL.into()), "filled at the head");
}

/// Compaction: the horizon at 2; a hello at 0 is answered with a fill a
/// replica stands on — its first part, where it stands, and then each table
/// it asks — one at 2 with the tail. Falsified by `Log::has`
/// answering for any sequence below the base: the hello at 0 was paged from
/// an entry the log no longer has.
#[test]
fn compaction_and_the_horizon() {
    let mut server = scenario().server;
    let h2 = server.log().at(2).unwrap().hash;
    let o = server.compact(2);
    assert!(o.iter().all(|x| matches!(x, Output::Send(To::Disk, _))));
    assert_eq!(server.log().base_seq(), 2);
    let hello = |cursor, hash| Frame::Hello {
        space: ark::schema::Space::Durable,
        cursor,
        hash,
        fill: None,
        most: ark::peer::PART as u64,
    };
    let at0 = sends(&server.step(up(&who("x"), &hello(0, EntryHash::default()))));
    let mut r = replica("x", demo::only(&[]));
    assert!(matches!(&at0[..], [Frame::Part { from: None, next: None, .. }]));
    let mut said = at0;
    while !said.is_empty() {
        let asks: Vec<Frame> = said.drain(..).flat_map(|f| sends(&r.step(down(&f)))).collect();
        said = asks.iter().flat_map(|f| sends(&server.step(up(&who("x"), f)))).collect();
    }
    assert_eq!((r.log().head(), r.store.state_hash().hex()), (server.log().head(), FINAL.into()));
    let at2 = sends(&server.step(up(&who("x"), &hello(2, h2))));
    assert!(matches!(&at2[..], [Frame::Page { from: 2, logged, .. }] if logged.iter().map(|l| l.seq).collect::<Vec<_>>() == [3, 4]));
}

/// Every change kind rebases: bob renames the playlist and removes a track
/// while alice, dark, adds one and renames it twice; landing his under her
/// pending intents undoes them newest-first, applies the edit and the
/// removal, and runs hers again. The stores agree. Falsified by undoing
/// pending changes oldest-first: the first rename's undo was refused.
#[test]
fn edits_and_removes_rebase_too() {
    let Scenario {
        mut server,
        mut alice,
        mut bob,
        ..
    } = scenario();
    let o1 = alice.step(mutate(ADD, pair_input(&pid(), "t4"), 5));
    let o2 = alice.step(mutate(RENAME, rename_input(&pid(), "Old"), 6));
    let o3 = alice.step(mutate(RENAME, rename_input(&pid(), "Older"), 6));
    for (name, input) in [(RENAME, rename_input(&pid(), "Best")), (REMOVE, pair_input(&pid(), "t2"))] {
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

/// An intent naming a function the authority does not run is not judged:
/// answered where its origin stands (`At`, its `last` below it), it stays
/// pending on the device, which sends nothing back and pushes it again on
/// its next connection; not judged again until the authority has the
/// function, then sequenced. Falsified by `push` answering an unknown
/// function with a refusal: the first push came back refused.
#[test]
fn held_until_the_authority_has_it() {
    let mut server = authority(demo::only(&[CREATE]));
    let mut alice = replica("alice", demo::cas());
    let o = alice.step(mutate(CREATE, create_input("Road"), 1));
    deliver_up(&mut server, &who("alice"), &o);
    let o = alice.step(mutate(ADD, pair_input(&pid(), "t1"), 2));
    let (s, f) = deliver_up(&mut server, &who("alice"), &o);
    assert!(matches!(verdicts(&s)[..], [Some(Verdict::At { last: 1, .. })]) && f.is_empty(), "{s:?}");
    assert!(sends(&alice.step(down(&s[0]))).is_empty(), "a held intent sends nothing");
    assert_eq!(alice.pending.len(), 2, "…and stays pending");
    let (s, _) = deliver_up(&mut server, &who("alice"), &alice.step(Input::Connected));
    assert!(
        matches!(
            verdicts(&s)[..],
            [.., Some(Verdict::At { seq: 1, .. }), Some(Verdict::At { last: 1, .. })]
        ),
        "held again: {s:?}"
    );
    demo::give(&mut server, ADD);
    let (s, f) = deliver_up(&mut server, &who("alice"), &alice.step(Input::Connected));
    assert!(
        matches!(verdicts(&s)[..], [.., Some(Verdict::At { seq: 1, .. }), Some(Verdict::At { seq: 2, .. })]),
        "{s:?}"
    );
    assert_eq!(f.len(), 1);
}

/// What a peer persisted opens it again: the same log and store, its
/// pending intents run on top. Each entry is persisted as its row of the
/// log's table alone (K3c: its facts are in it, and the rows they write
/// are written again on open, so they are on disk once), and the rows are
/// appended up to the first that does not link onto the head — here a
/// stray at sequence 9, with no facts to refuse it — the store then made
/// durable whole, without it. Falsified by `Store::append` taking a row
/// at any sequence (the stray was taken, and the store was not written
/// whole again), and by `Peer::open` not applying an entry's facts (bob's
/// log over a store without bob's tracks).
#[test]
fn a_peer_opens_from_what_it_persisted() {
    let s = scenario();
    let mut records: Vec<Vec<u8>> = vec![];
    let mut r = replica("bob", demo::cas());
    for f in &s.batches {
        for o in r.step(down(f)) {
            if let Output::Send(To::Disk, b) = o {
                records.push(b);
            }
        }
    }
    let mut stray = logged(&s.batches[3]);
    (stray.seq, stray.facts) = (9, vec![]);
    records.push(Record::Logged(stray).enc());
    let rows = records.iter().map(|b| match Record::dec(b) {
        Ok(Record::Logged(l)) => {
            assert_eq!(*b, Record::logged(&l.to_bytes()), "a record is its row of the log's table wrapped");
            l
        }
        other => panic!("{other:?}"),
    });
    let mut d = vec![Record::Rows(replica("alice", demo::cas()).confirmed_rows())];
    d.extend(rows.map(Record::Logged));
    let pending = s.alice.clone().step(mutate(ADD, pair_input(&pid(), "t5"), 9));
    let pending = sends(&pending)
        .into_iter()
        .map(|f| match f {
            Frame::Page { mut logged, .. } => ark::log::Logged::intent(logged.remove(0).entry, vec![]),
            _ => unreachable!(),
        })
        .collect();
    d.push(Record::Pending { pending, at: None, local: 0 });
    let mut opened = replica("alice", demo::cas());
    let out = opened.step(Input::Open(d));
    let folded: Vec<Record> = out
        .iter()
        .filter_map(|o| match o {
            Output::Send(To::Disk, b) => Some(Record::dec(b).unwrap()),
            _ => None,
        })
        .collect();
    assert!(
        matches!(&folded[..], [Record::Rows(rows)] if rows.last().is_some_and(|(t, rs)| t == LOG && rs.len() == 5)),
        "the rows stop where they stop linking, and the store is written whole"
    );
    assert_eq!(opened.log(), s.bob.log());
    assert_eq!(pos_of(&opened.store, &pid(), "t5"), Some(4));
}

/// A session's pushes are judged in its order: one that arrives ahead of
/// the one before it is answered with the gap, waits, and is pushed again
/// after the next hello. Falsified by dropping the gap check in `push`: the
/// add was judged before the playlist it adds to existed, and refused.
#[test]
fn a_sessions_pushes_are_judged_in_order() {
    let (mut server, mut alice) = (authority(demo::cas()), replica("alice", demo::cas()));
    let mut pushes = sends(&alice.step(mutate(CREATE, create_input("Road"), 1)));
    pushes.extend(sends(&alice.step(mutate(ADD, pair_input(&pid(), "t1"), 2))));
    let mut answers = vec![];
    for f in pushes.iter().rev() {
        let o = server.step(up(&who("alice"), f));
        answers.extend(sends(&o));
        for b in fanouts(&o) {
            alice.step(down(&b));
        }
    }
    assert!(
        matches!(
            verdicts(&answers)[..],
            [Some(Verdict::At { last: 0, .. }), Some(Verdict::At { seq: 1, .. })]
        ),
        "the add waits: {answers:?}"
    );
    let (s, f) = deliver_up(&mut server, &who("alice"), &alice.step(Input::Connected));
    assert!(matches!(verdicts(&s)[..], [Some(Verdict::At { seq: 2, .. })]), "{s:?}");
    alice.step(down(&f[0]));
    assert!(alice.pending.is_empty() && alice.store == server.store);
}

/// A page is one rebase: bob's N = 4 entries land on alice in one `Batch`
/// under her K = 3 pending intents, and her intents run K times — not K
/// for every entry — and she is told her changes undone once, bob's facts,
/// and hers again: 3 + 4 + 3. Her tracks land
/// after bob's, the confirmed state is bob's. Falsified by undoing and
/// running the pending intents again per entry, as before: 12 runs.
#[test]
fn a_page_rebases_once() {
    let runs = Arc::new(AtomicUsize::new(0));
    // Each run counted.
    let mut cas = demo::demo();
    for n in [CREATE, ADD] {
        let (p, runs) = (cas.fns[n].proc.clone().unwrap(), runs.clone());
        cas = cas.run(
            n,
            Arc::new(move |store, ctx: &Ctx, input| {
                runs.fetch_add(1, Ordering::Relaxed);
                p(store, ctx, input)
            }),
        );
    }
    let cas = Arc::new(cas);
    let (mut server, mut alice, mut bob) = (authority(demo::cas()), replica("alice", cas), replica("bob", demo::cas()));
    let o = alice.step(mutate(CREATE, create_input("Favorites"), 1));
    let (_, f) = deliver_up(&mut server, &who("alice"), &o);
    for p in [&mut alice, &mut bob] {
        p.step(down(&f[0]));
    }
    for t in ["a1", "a2", "a3"] {
        alice.step(mutate(ADD, pair_input(&pid(), t), 2));
    }
    let mut page = vec![];
    for t in ["b1", "b2", "b3", "b4"] {
        let (_, f) = deliver_up(&mut server, &who("bob"), &bob.step(mutate(ADD, pair_input(&pid(), t), 3)));
        bob.step(down(&f[0]));
        page.push(logged(&f[0]));
    }
    let batch = |logged: Vec<Logged>| Frame::Page {
        space: ark::schema::Space::Durable,
        from: 1,
        logged,
    };
    runs.store(0, Ordering::Relaxed);
    let o = alice.step(down(&batch(page)));
    assert_eq!(runs.load(Ordering::Relaxed), 3, "the pending intents ran once for the page");
    assert_eq!(changes(&o).len(), 3 + 4 + 3);
    assert_eq!((alice.pending.len(), alice.log().head()), (3, bob.log().head()));
    assert_eq!(pos_of(&alice.store, &pid(), "a1"), Some(5), "hers after bob's four");
}

/// What a session pushes behind a held intent is held too: the authority
/// keeps nothing of the held one and answers what comes after it with the
/// same `last`. The device holds the held one, so it waits, its numbers as they
/// are; once the authority runs the function the next connection's pushes
/// are judged in order. Falsified by one not judged passing its `local`: the
/// playlist after it was sequenced at 2, before the track. And by
/// a push saying its pending begin after the one before it rather than
/// before the first: the held one was passed.
#[test]
fn what_comes_behind_a_held_intent_is_a_gap() {
    let mut server = authority(demo::only(&[CREATE]));
    let mut alice = replica("alice", demo::cas());
    let o = alice.step(mutate(CREATE, create_input("Road"), 1));
    deliver_up(&mut server, &who("alice"), &o);
    let o = alice.step(mutate(ADD, pair_input(&pid(), "t1"), 2));
    let (s, _) = deliver_up(&mut server, &who("alice"), &o);
    assert!(matches!(verdicts(&s)[..], [Some(Verdict::At { last: 1, .. })]), "{s:?}");
    let o = alice.step(mutate(CREATE, create_input("Sea"), 3));
    let (s, f) = deliver_up(&mut server, &who("alice"), &o);
    assert!(
        matches!(verdicts(&s)[..], [Some(Verdict::At { last: 1, .. })]) && f.is_empty(),
        "a gap: {s:?}"
    );
    assert_eq!(server.log().head().0, 1);
    let ids: Vec<_> = alice.pending.iter().map(|p| p.entry.id).collect();
    assert!(sends(&alice.step(down(&s[0]))).is_empty(), "behind its own held one, the device waits");
    assert_eq!(alice.pending.iter().map(|p| p.entry.id).collect::<Vec<_>>(), ids);
    demo::give(&mut server, ADD);
    let (s, _) = deliver_up(&mut server, &who("alice"), &alice.step(Input::Connected));
    assert!(
        matches!(
            verdicts(&s)[..],
            [
                ..,
                Some(Verdict::At { seq: 1, .. }),
                Some(Verdict::At { seq: 2, .. }),
                Some(Verdict::At { seq: 3, .. })
            ]
        ),
        "{s:?}"
    );
}
