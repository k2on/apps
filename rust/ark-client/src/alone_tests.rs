//! A peer without a server is a peer (`docs/plan-alone.md`): its local
//! history kept on the storage and not in memory, and the two transitions
//! — alone → server, server → alone — including a stop in the middle of
//! one. Each test says what falsified it.

use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use ark::hash::state_hash;
use ark::journal::{self, Layout};
use ark::live::{ConnId, Silent};
use ark::log::Seq;
use ark::peer::{Authority, Changes};
use ark::protocol::{open_access, trusting, Server};
use ark::store::Store;
use ark::value::{Id, Value};

use crate::storage::{Memory, ReplicaFile, Storage};
use crate::{args, demo, Autos, Error, Fork, Login, Options, Peer};

fn alone(seed: u64) -> Options {
    Options::alone("me").with_autos(Autos::seeded(seed))
}

fn dev(user: &str) -> Options {
    Options::dev(user).with_autos(Autos::seeded(user.as_bytes()[0] as u64))
}

fn create(p: &mut Peer, name: &str) -> Id {
    p.mutate("create_playlist", args([("name", Value::text(name))])).unwrap()
}

fn add(p: &mut Peer, list: &Value, track: &str) {
    p.mutate("add_to_playlist", args([("playlist_id", list.clone()), ("track_id", Value::text(track))]))
        .unwrap();
}

fn hub() -> Server<Silent> {
    let d = demo::domain();
    let mut a = Authority::new(d.module().schema.clone(), d.closures().clone());
    a.hold(d.native_list());
    a.log.name_if_unnamed([9; 16]);
    Server::open(trusting(), open_access(), Silent, a)
}

/// Move frames between the hub and the peers until nothing moves.
fn settle(s: &mut Server<Silent>, peers: &mut [(ConnId, &mut Peer)]) {
    loop {
        let mut moved = false;
        for (c, p) in peers.iter_mut() {
            for m in p.take_outgoing() {
                moved = true;
                s.recv(*c, m);
            }
        }
        for (c, m) in s.take_outgoing() {
            if let Some((_, p)) = peers.iter_mut().find(|(pc, _)| *pc == c) {
                moved = true;
                p.recv(m);
            }
        }
        if !moved {
            for (_, p) in peers.iter_mut() {
                p.persist().unwrap();
            }
            return;
        }
    }
}

/// The local history on a storage, read back: every sequence and id, in
/// order, above the fork.
fn history(disk: &Memory) -> (Seq, Vec<(Seq, Id)>) {
    let mut out = vec![];
    let schema = demo::domain().module().schema.clone();
    let o = journal::read(disk, &Layout::alone(), &schema, |n, e, _| out.push((n, e.id))).unwrap();
    (o.snapshot.map_or(-1, |l| l.horizon()), out)
}

/// Whether the local history is on the storage at all.
fn has_log(disk: &Memory) -> bool {
    disk.load("log").unwrap().is_some()
}

fn hash(p: &Peer) -> Vec<u8> {
    state_hash(&p.replica().confirmed)
}

// -- §2: the alone log ---------------------------------------------------------

/// §2: what a peer alone sequences survives it — every entry, in order,
/// on the storage above the fork, and read back into the authority's ids
/// on reopen, so a re-sequenced id is a duplicate. Falsified by
/// `persist_log` writing nothing: the history read back is empty.
#[test]
fn an_alone_peers_log_survives_reopen() {
    let disk = Memory::new();
    let mut p = Peer::open(demo::domain(), Box::new(disk.clone()), alone(1)).unwrap();
    let ids: Vec<Id> = (0..40).map(|i| create(&mut p, &format!("p{i}"))).collect();
    let live = hash(&p);
    drop(p);
    let (fork, got) = history(&disk);
    assert_eq!(fork, 0, "a peer that never had a server forks at nothing");
    assert_eq!(got, ids.iter().enumerate().map(|(i, id)| (i as Seq + 1, *id)).collect::<Vec<_>>());
    let p = Peer::open(demo::domain(), Box::new(disk.clone()), alone(2)).unwrap();
    assert_eq!((p.cursor(), hash(&p)), (40, live));
    let a = p.authority_log().expect("alone");
    assert_eq!((a.head_seq(), a.ids.len()), (40, 40), "the head and every id");
    assert_eq!(p.status().link, "alone");
}

/// §2: the alone authority holds no entries — after every one of five
/// hundred appends, the log in memory and the unwritten buffer are empty;
/// the entries are on the storage. Falsified by `commit_alone` leaving the
/// entries in the authority's log (no `take_entries`): 500 held.
#[test]
fn memory_holds_no_entries_after_n_appends() {
    let disk = Memory::new();
    let mut p = Peer::open(demo::domain(), Box::new(disk.clone()), alone(3)).unwrap();
    let mut most = 0;
    for i in 0..500 {
        create(&mut p, &format!("p{i}"));
        most = most.max(p.entries_held());
    }
    assert_eq!(most, 0, "entries held in memory after an append");
    assert_eq!(history(&disk).1.len(), 500, "…and every one on the storage");
    let pages = disk.keys().iter().filter(|k| k.starts_with("log.")).count();
    assert!(pages <= 10, "{pages} pages for 500 appends");
}

/// §2 and §4: a peer alone killed in the middle of writing a page of its
/// history reopens to the last whole entry, and carries on from there:
/// the torn intent's `mutate` had not returned, so nothing that was
/// acknowledged is lost. And a store written ahead of the history — a
/// storage that lost a page after the store landed — is rebuilt from the
/// fork and the facts to the history's head rather than trusted.
/// Falsified by `resume_alone` skipping the rebuild when the store is
/// ahead: the peer reopens at 12 over a history that reaches 11.
#[test]
fn a_page_torn_mid_write_reopens_to_the_last_whole_entry() {
    let disk = Memory::new();
    let mut p = Peer::open(demo::domain(), Box::new(disk.clone()), alone(4)).unwrap();
    for i in 0..10 {
        create(&mut p, &format!("p{i}"));
    }
    p.pump();
    let at_ten = hash(&p);
    // The eleventh intent's page, torn: what a kill during its write leaves
    // on a storage that can tear (a `Dir` renames, and cannot).
    create(&mut p, "torn");
    let torn = disk
        .keys()
        .into_iter()
        .filter(|k| k.starts_with("log."))
        .max_by_key(|k| k[4..].parse::<usize>().unwrap())
        .unwrap();
    let mut bytes = disk.load(&torn).unwrap().unwrap();
    bytes.truncate(bytes.len() - 5);
    let mut cut = disk.clone();
    std::mem::forget(p);
    cut.save(&torn, &bytes).unwrap();

    let mut back = Peer::open(demo::domain(), Box::new(disk.clone()), alone(5)).unwrap();
    assert_eq!((back.cursor(), hash(&back)), (10, at_ten), "to the last whole entry");
    assert_eq!(history(&disk).1.len(), 10);
    create(&mut back, "next");
    assert_eq!(back.cursor(), 11);
    assert_eq!(history(&disk).1.len(), 11, "and on from it");
    drop(back);

    // The store written ahead of what the history kept.
    let mut p = Peer::open(demo::domain(), Box::new(disk.clone()), alone(6)).unwrap();
    create(&mut p, "ahead");
    p.pump();
    assert_eq!(ReplicaFile::load(&disk, &demo::domain().module().schema).unwrap().unwrap().cursor, 12);
    std::mem::forget(p);
    let last = disk
        .keys()
        .into_iter()
        .filter(|k| k.starts_with("log."))
        .max_by_key(|k| k[4..].parse::<usize>().unwrap())
        .unwrap();
    let mut bytes = disk.load(&last).unwrap().unwrap();
    bytes.truncate(bytes.len() - 5);
    disk.clone().save(&last, &bytes).unwrap();
    let mut back = Peer::open(demo::domain(), Box::new(disk.clone()), alone(7)).unwrap();
    assert_eq!(back.cursor(), 11, "the store follows the history, not the other way");
    assert_eq!(back.store().scan("playlist").len(), 11);
    create(&mut back, "then");
    assert_eq!(back.cursor(), 12);
}

/// §2: compaction merges pages and never moves the base past the fork —
/// a peer that left a server at 50 and then sequenced two hundred alone
/// has a handful of pages holding 51 to 250, every one, over a snapshot
/// at 50 that was written once. Falsified by `Journal::merge` merging
/// nothing: 200 pages.
#[test]
fn compaction_keeps_every_entry_since_the_fork() {
    let mut s = hub();
    let disk = Memory::new();
    let mut p = Peer::open(demo::domain(), Box::new(disk.clone()), dev("alice")).unwrap();
    p.connected();
    for i in 0..50 {
        create(&mut p, &format!("s{i}"));
        settle(&mut s, &mut [(1, &mut p)]);
    }
    assert_eq!(p.cursor(), 50);
    p.leave().unwrap();
    let fork_snapshot = disk.load("log").unwrap();
    for i in 0..200 {
        create(&mut p, &format!("a{i}"));
        if i % 7 == 0 {
            p.pump();
        }
    }
    let (fork, got) = history(&disk);
    assert_eq!(fork, 50);
    assert_eq!(got.iter().map(|(n, _)| *n).collect::<Vec<_>>(), (51..=250).collect::<Vec<_>>());
    assert_eq!(disk.load("log").unwrap(), fork_snapshot, "the base is the fork, written once");
    let pages = disk.keys().iter().filter(|k| k.starts_with("log.")).count();
    assert!(pages <= 10, "{pages} pages");
    assert_eq!(
        p.status().fork,
        Fork {
            log_id: Some([9; 16]),
            cursor: 50
        }
    );
}

// -- §1: the transitions -----------------------------------------------------------

/// §1, §4: alone for N, then joined to a fresh hub — the hub ends with the
/// N entries, in the order they were sequenced alone, and the peer's
/// confirmed store is the hub's. While the hub has yet to answer, the
/// status says how many local intents are pending. Falsified by
/// `fork_back_to` re-queuing nothing: the hub ends with 0.
#[test]
fn alone_for_n_then_a_fresh_hub_has_n() {
    let mut s = hub();
    let disk = Memory::new();
    let mut p = Peer::open(demo::domain(), Box::new(disk.clone()), alone(8)).unwrap();
    let ids: Vec<Id> = (0..30).map(|i| create(&mut p, &format!("p{i}"))).collect();
    assert_eq!((p.status().link.as_str(), p.status().joining), ("alone", 0));
    p.join("ws://hub/sync", Some(Login::dev("alice"))).unwrap();
    assert!(!has_log(&disk), "the local history is pending now, and gone from the storage");
    assert_eq!((p.cursor(), p.pending_len(), p.status().joining), (0, 30, 30));
    assert!(p.replica().pending.iter().all(|e| e.actor == "alice" && e.session == "dev"));
    p.connected();
    settle(&mut s, &mut [(1, &mut p)]);
    assert_eq!(s.authority.log.head_seq(), 30);
    let order: Vec<Id> = (1..=30).map(|n| s.authority.log.entries[&n].0.id).collect();
    assert_eq!(order, ids, "in the order they were sequenced alone");
    assert_eq!(hash(&p), state_hash(&s.authority.store));
    assert_eq!((p.pending_len(), p.status().joining), (0, 0));
    assert_eq!(p.replica().log_id, Some([9; 16]));
    assert!(p.store().scan("playlist").iter().all(|r| r["user_id"] == Value::text("alice")));
}

/// §1, §4: alone for N while another peer used the hub — the local history
/// lands after theirs, and everyone converges. Opened for a server over the
/// alone storage, the peer transitions rather than refusing. Falsified by
/// `Peer::open` not joining when the storage holds a local history: the
/// peer opens at its local cursor 20 and the hub never hears of them.
#[test]
fn alone_beside_another_lands_after_theirs() {
    let mut s = hub();
    let mut bob = Peer::open_memory(demo::domain(), dev("bob")).unwrap();
    bob.connected();
    for i in 0..15 {
        create(&mut bob, &format!("b{i}"));
    }
    settle(&mut s, &mut [(2, &mut bob)]);
    let disk = Memory::new();
    let mut me = Peer::open(demo::domain(), Box::new(disk.clone()), alone(9)).unwrap();
    let mine: Vec<Id> = (0..20).map(|i| create(&mut me, &format!("m{i}"))).collect();
    drop(me);
    let mut me = Peer::open(demo::domain(), Box::new(disk.clone()), dev("alice")).unwrap();
    assert_eq!((me.cursor(), me.pending_len()), (0, 20), "opened for a server, it joins");
    me.connected();
    settle(&mut s, &mut [(1, &mut me), (2, &mut bob)]);
    assert_eq!(s.authority.log.head_seq(), 35);
    let after: Vec<Id> = (16..=35).map(|n| s.authority.log.entries[&n].0.id).collect();
    assert_eq!(after, mine, "after theirs");
    assert_eq!(hash(&me), state_hash(&s.authority.store));
    assert_eq!(hash(&bob), state_hash(&s.authority.store));
}

/// §1, §4: leave, more intents, join again — and the items put on a shared
/// playlist while away sit after the ones others put there meanwhile. The
/// view is told a rebase by changes at the join, never `Rebuilt`.
/// Falsified by `Replica::fork_back` undoing only the confirmed store and
/// not the view: the debug assertion that the view reached the fork
/// fails.
#[test]
fn leave_more_intents_and_join_again() {
    let mut s = hub();
    let mut bob = Peer::open_memory(demo::domain(), dev("bob")).unwrap();
    let disk = Memory::new();
    let mut alice = Peer::open(demo::domain(), Box::new(disk.clone()), dev("alice")).unwrap();
    alice.connected();
    bob.connected();
    create(&mut bob, "Shared");
    settle(&mut s, &mut [(1, &mut alice), (2, &mut bob)]);
    let list = alice.store().scan("playlist")[0]["id"].clone();
    add(&mut alice, &list, "a-before");
    settle(&mut s, &mut [(1, &mut alice), (2, &mut bob)]);
    let at_leave = alice.cursor();

    alice.leave().unwrap();
    assert_eq!(alice.status().link, "alone");
    assert_eq!(alice.status().fork.cursor, at_leave);
    for i in 0..5 {
        add(&mut alice, &list, &format!("a{i}"));
    }
    assert_eq!(alice.cursor(), at_leave + 5, "sequenced alone");
    for i in 0..3 {
        add(&mut bob, &list, &format!("b{i}"));
    }
    settle(&mut s, &mut [(2, &mut bob)]);
    let _ = alice.take_changes();

    alice.join("ws://hub/sync", Some(Login::dev("alice"))).unwrap();
    assert!(matches!(alice.take_changes(), Changes::Applied(_)), "patched, not rebuilt");
    alice.connected();
    settle(&mut s, &mut [(1, &mut alice), (2, &mut bob)]);
    assert_eq!(hash(&alice), state_hash(&s.authority.store));
    assert_eq!(hash(&bob), state_hash(&s.authority.store));
    let mut items = alice.store().scan("item");
    items.sort_by_key(|r| r["pos"].clone());
    let order: Vec<String> = items
        .iter()
        .map(|r| match &r["track_id"] {
            Value::Text(t) => t.to_string(),
            v => panic!("{v:?}"),
        })
        .collect();
    assert_eq!(order, ["a-before", "b0", "b1", "b2", "a0", "a1", "a2", "a3", "a4"]);

    // And away again, and back: a second fork.
    alice.leave().unwrap();
    add(&mut alice, &list, "again");
    alice.join("ws://hub/sync", Some(Login::dev("alice"))).unwrap();
    alice.connected();
    settle(&mut s, &mut [(1, &mut alice), (2, &mut bob)]);
    assert_eq!(s.authority.store.scan("item").len(), 10);
    assert_eq!(hash(&alice), state_hash(&s.authority.store));
}

/// A storage that stops writing after `left` more writes — what a kill
/// leaves: everything before it on the storage, nothing after. Loads
/// always answer.
#[derive(Clone)]
struct Stopping {
    inner: Memory,
    left: Arc<AtomicIsize>,
}

impl Storage for Stopping {
    fn load(&self, key: &str) -> Result<Option<Vec<u8>>, Error> {
        self.inner.load(key)
    }
    fn save(&mut self, key: &str, bytes: &[u8]) -> Result<(), Error> {
        if self.left.fetch_sub(1, Ordering::SeqCst) <= 0 {
            return Err(Error::Storage("stopped".into()));
        }
        self.inner.save(key, bytes)
    }
    fn remove(&mut self, key: &str) -> Result<(), Error> {
        if self.left.fetch_sub(1, Ordering::SeqCst) <= 0 {
            return Err(Error::Storage("stopped".into()));
        }
        self.inner.remove(key)
    }
}

/// §4: a join stopped after any number of its writes — the re-queued
/// pending, the login, the fork's replica, the history's pages and then
/// its snapshot removed — reopens for the server and finishes: the hub
/// ends with every local entry and the peer converges with it. Walked at
/// every write the join makes. Falsified by removing the local history
/// before writing the re-queued intents (`destroy` first in
/// `fork_back_to`): stopped after one page of it is removed, the reopened
/// peer has 8 of its 12.
#[test]
fn a_join_killed_between_its_writes_finishes_on_reopen() {
    // How many writes a whole join makes, counted on a storage that never
    // stops.
    let writes = {
        let st = Stopping {
            inner: Memory::new(),
            left: Arc::new(AtomicIsize::new(isize::MAX)),
        };
        let mut p = Peer::open(demo::domain(), Box::new(st.clone()), alone(10)).unwrap();
        for i in 0..12 {
            create(&mut p, &format!("p{i}"));
        }
        p.pump();
        let before = st.left.load(Ordering::SeqCst);
        p.join("ws://hub/sync", Some(Login::dev("alice"))).unwrap();
        (before - st.left.load(Ordering::SeqCst)) as usize
    };
    assert!(writes >= 4, "{writes} writes");
    for stop in 0..=writes {
        let st = Stopping {
            inner: Memory::new(),
            left: Arc::new(AtomicIsize::new(isize::MAX)),
        };
        let mut p = Peer::open(demo::domain(), Box::new(st.clone()), alone(10)).unwrap();
        for i in 0..12 {
            create(&mut p, &format!("p{i}"));
        }
        p.pump();
        st.left.store(stop as isize, Ordering::SeqCst);
        let joined = p.join("ws://hub/sync", Some(Login::dev("alice")));
        assert_eq!(joined.is_ok(), stop >= writes, "stopped after {stop}");
        drop(p);
        let mut s = hub();
        let mut back = Peer::open(demo::domain(), Box::new(st.inner.clone()), dev("alice")).unwrap();
        assert!(!has_log(&st.inner), "stopped after {stop}: the join finished");
        assert_eq!(back.pending_len(), 12, "stopped after {stop}");
        back.connected();
        settle(&mut s, &mut [(1, &mut back)]);
        assert_eq!(s.authority.log.head_seq(), 12, "stopped after {stop}");
        assert_eq!(hash(&back), state_hash(&s.authority.store), "stopped after {stop}");
    }
}

/// §1: a storage last used with a server, opened alone, leaves: the fork is
/// where the replica stood and what was pending is sequenced locally.
/// Falsified by `start_alone` recording the fork at nothing: the status
/// says cursor 0 where the replica left the hub at 6.
#[test]
fn opening_alone_over_a_server_replica_leaves() {
    let mut s = hub();
    let disk = Memory::new();
    let mut p = Peer::open(demo::domain(), Box::new(disk.clone()), dev("alice")).unwrap();
    p.connected();
    for i in 0..6 {
        create(&mut p, &format!("p{i}"));
    }
    settle(&mut s, &mut [(1, &mut p)]);
    p.disconnected();
    create(&mut p, "offline");
    drop(p);
    let p = Peer::open(demo::domain(), Box::new(disk.clone()), alone(11)).unwrap();
    assert_eq!(
        p.status().fork,
        Fork {
            log_id: Some([9; 16]),
            cursor: 6
        }
    );
    assert_eq!((p.cursor(), p.pending_len()), (7, 0), "the pending intent, sequenced here");
    assert_eq!(p.replica().log_id, None, "alone, the sequences are nobody's log");
    assert_eq!(history(&disk).0, 6);
}

/// §4: a peer that never had a server joins with 2,000 local intents: the
/// hub ends with all of them and the time it took is said. Falsified by
/// `Replica::fork_back` queuing the first thousand only: the hub has 1000.
#[test]
fn two_thousand_local_intents_join() {
    let disk = Memory::new();
    let mut p = Peer::open(demo::domain(), Box::new(disk.clone()), alone(12)).unwrap();
    for i in 0..2_000 {
        create(&mut p, &format!("p{i:04}"));
    }
    p.pump();
    let mut s = hub();
    let t = Instant::now();
    p.join("ws://hub/sync", Some(Login::dev("alice"))).unwrap();
    let requeued = t.elapsed();
    p.connected();
    settle(&mut s, &mut [(1, &mut p)]);
    let total = t.elapsed();
    eprintln!("2,000 local intents: re-queued in {requeued:?}, on the hub and confirmed in {total:?}");
    assert_eq!(s.authority.log.head_seq(), 2_000);
    assert_eq!(hash(&p), state_hash(&s.authority.store));
    assert_eq!(p.pending_len(), 0);
}
