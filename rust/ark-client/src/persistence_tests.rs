//! The replica on a storage, as a snapshot and a journal (the `storage`
//! module docs): what a write costs, what a reopen reads back, and what a
//! stop at the wrong moment leaves. Each test says what falsified it.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ark::live::{ConnId, Silent};
use ark::peer::Authority;
use ark::protocol::{open_access, trusting, Server};
use ark::store::Store;
use ark::value::Value;

use crate::storage::{
    decode_page, decode_pending_snapshot, encode_page, encode_pending, encode_pending_page, encode_pending_snapshot, encode_replica, Memory,
    ReplicaFile, Storage,
};
use crate::{args, demo, Autos, Error, Options, Peer};

/// A storage that counts what is written to it: the bytes, and the keys in
/// order. Clones share the counts and the records.
#[derive(Clone, Default)]
struct Counting {
    inner: Memory,
    bytes: Arc<AtomicUsize>,
    keys: Arc<Mutex<Vec<String>>>,
}

impl Counting {
    fn reset(&self) {
        self.bytes.store(0, Ordering::SeqCst);
        self.keys.lock().unwrap().clear();
    }
    fn written(&self) -> (usize, Vec<String>) {
        (self.bytes.load(Ordering::SeqCst), self.keys.lock().unwrap().clone())
    }
}

impl Storage for Counting {
    fn load(&self, key: &str) -> Result<Option<Vec<u8>>, Error> {
        self.inner.load(key)
    }
    fn save(&mut self, key: &str, bytes: &[u8]) -> Result<(), Error> {
        self.bytes.fetch_add(bytes.len(), Ordering::SeqCst);
        self.keys.lock().unwrap().push(key.to_string());
        self.inner.save(key, bytes)
    }
    fn remove(&mut self, key: &str) -> Result<(), Error> {
        self.inner.remove(key)
    }
}

/// What a storage holds now, as a storage of its own: a reopen from it
/// cannot write into the one the live peer is using.
fn copy(disk: &Memory) -> Memory {
    let mut out = Memory::new();
    for k in disk.keys() {
        out.save(&k, &disk.load(&k).unwrap().unwrap()).unwrap();
    }
    out
}

fn pages(disk: &Memory) -> Vec<String> {
    disk.keys().into_iter().filter(|k| k.starts_with("facts.")).collect()
}

fn alone() -> Options {
    Options::alone("me").with_autos(Autos::seeded(7))
}

fn create(p: &mut Peer, name: &str) {
    p.mutate("create_playlist", args([("name", Value::text(name))])).unwrap();
}

/// A peer reopened from what `disk` holds is the live one: the confirmed
/// store, the cursor, the intents and the view.
fn reopens_as(live: &Peer, disk: &Memory, opts: Options, what: &str) {
    let back = Peer::open(demo::domain(), Box::new(copy(disk)), opts).unwrap();
    let (a, b) = (live.replica(), back.replica());
    assert_eq!(b.cursor, a.cursor, "{what}: cursor");
    assert!(b.confirmed == a.confirmed, "{what}: the confirmed store");
    assert_eq!(b.pending, a.pending, "{what}: pending");
    assert!(back.store() == live.store(), "{what}: the view");
}

/// A server to sync with, sans-io: trusting, so a token is the user and
/// every login is `dev`.
fn server() -> Server<Silent> {
    let d = demo::domain();
    let mut a = Authority::new(d.module().schema.clone(), d.closures().clone());
    a.hold(d.native_list());
    Server::open(trusting(), open_access(), Silent, a)
}

/// Move frames between the server and the peers until nothing moves.
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
            return;
        }
    }
}

/// A peer alone with `n` playlists, opened over a storage holding them as
/// one snapshot — written by hand, so that no test's setup leans on the
/// write path it is testing.
fn seeded(disk: &Memory, n: usize) -> Peer {
    let mut author = Peer::open_memory(demo::domain(), Options::alone("me").with_autos(Autos::seeded(99))).unwrap();
    for i in 0..n {
        create(&mut author, &format!("p{i}"));
    }
    let r = author.replica();
    let snapshot = encode_replica("alone", r.cursor, &r.confirmed, "me", "local");
    disk.clone().save(ReplicaFile::KEY, &snapshot).unwrap();
    Peer::open(demo::domain(), Box::new(disk.clone()), alone()).unwrap()
}

/// §11.9 A peer alone writes, per pump after one mutation, one page of that
/// mutation's changes — the same bytes over three hundred playlists as over
/// two thousand four hundred, and a small fraction of the snapshot. Falsified by writing a
/// snapshot on every pump that moved the cursor (the layout before the
/// journal): 6617 bytes against 104983.
#[test]
fn a_write_costs_the_mutation_not_the_store() {
    let mut cost = vec![];
    // Sizes whose sequences take the same bytes to write (a CBOR integer
    // from 256 to 65535 is three): the page says where it starts and ends.
    for n in [300, 2400] {
        let disk = Counting::default();
        let mut p = Peer::open(demo::domain(), Box::new(disk.clone()), alone()).unwrap();
        for i in 0..n {
            create(&mut p, &format!("p{i}"));
        }
        p.pump();
        let snapshot = disk.inner.load(ReplicaFile::KEY).unwrap().unwrap().len();
        disk.reset();
        create(&mut p, "New");
        p.pump();
        let (bytes, keys) = disk.written();
        assert_eq!(keys, vec![ReplicaFile::page_key(1)], "{n}: one page and nothing else");
        assert!(bytes * 20 < snapshot, "{n}: {bytes} bytes against a snapshot of {snapshot}");
        cost.push(bytes);
    }
    assert_eq!(cost[0], cost[1], "the same mutation costs the same bytes at any size");
}

/// Reopened after every write, a peer is the one that wrote: alone, through
/// pages and the compactions between them; and with a server, with its own
/// intents pending while another peer's land, and after they are answered.
/// Falsified by `open` reading the snapshot and not the pages after it:
/// the first reopen is at 50, not 51.
#[test]
fn reopening_after_every_write_is_the_live_replica() {
    let disk = Memory::new();
    let mut p = seeded(&disk, 50);
    for i in 0..120 {
        create(&mut p, &format!("q{i}"));
        p.pump();
        reopens_as(&p, &disk, alone(), &format!("alone, after {i}"));
    }

    let mut s = server();
    let (da, db) = (Memory::new(), Memory::new());
    let dev = |u: &str| Options::dev(u).with_autos(Autos::seeded(u.as_bytes()[0] as u64));
    let mut alice = Peer::open(demo::domain(), Box::new(da.clone()), dev("alice")).unwrap();
    let mut bob = Peer::open(demo::domain(), Box::new(db.clone()), dev("bobby")).unwrap();
    alice.connected();
    bob.connected();
    let step = |s: &mut Server<Silent>, alice: &mut Peer, bob: &mut Peer, what: &str, linked: bool| {
        if linked {
            settle(s, &mut [(1, alice), (2, bob)]);
        } else {
            settle(s, &mut [(2, bob)]);
        }
        alice.persist().unwrap();
        bob.persist().unwrap();
        reopens_as(alice, &da, dev("alice"), what);
        reopens_as(bob, &db, dev("bobby"), what);
    };
    for i in 0..30 {
        create(&mut bob, &format!("b{i}"));
        step(&mut s, &mut alice, &mut bob, &format!("bob's {i}"), true);
    }
    create(&mut alice, "a0");
    step(&mut s, &mut alice, &mut bob, "alice's, answered", true);
    alice.disconnected();
    s.disconnect(1);
    for i in 1..4 {
        create(&mut alice, &format!("a{i}"));
        create(&mut bob, &format!("c{i}"));
        step(&mut s, &mut alice, &mut bob, &format!("alice offline, {i} pending"), false);
        assert_eq!(alice.pending_len(), i);
    }
    alice.connected();
    step(&mut s, &mut alice, &mut bob, "alice back", true);
    assert_eq!(alice.pending_len(), 0);
    assert!(alice.replica().confirmed == s.authority.store);
    assert!(bob.replica().confirmed == s.authority.store);
}

/// The pages are folded into a snapshot once they outgrow it, and the
/// snapshot is written before the pages go: what a reopen reads is the
/// same before and after. Falsified by never compacting: the pages
/// outgrow the snapshot and stay.
#[test]
fn the_journal_is_compacted_once_it_outgrows_the_snapshot() {
    let disk = Memory::new();
    let mut p = seeded(&disk, 30);
    let (mut most, mut compacted) = (0, false);
    for i in 0..200 {
        let before = disk.load(ReplicaFile::KEY).unwrap();
        create(&mut p, &format!("q{i}"));
        p.pump();
        let n = pages(&disk).len();
        if n == 0 {
            assert_ne!(disk.load(ReplicaFile::KEY).unwrap(), before, "a compaction writes a snapshot");
            compacted = true;
            break;
        }
        most = most.max(n);
        let journal: usize = pages(&disk).iter().map(|k| disk.load(k).unwrap().unwrap().len()).sum();
        let snapshot = disk.load(ReplicaFile::KEY).unwrap().unwrap().len();
        assert!(journal <= snapshot, "{journal} bytes of pages over a snapshot of {snapshot}");
    }
    assert!(compacted && most > 1, "compacted after {most} pages");
    reopens_as(&p, &disk, alone(), "after a compaction");
    let f = ReplicaFile::load(&disk, &demo::domain().module().schema).unwrap().unwrap();
    assert_eq!(f.cursor, p.cursor(), "the snapshot is at the cursor");
}

/// What a stop leaves behind: a page torn in the writing, or a page that
/// does not follow on. `open` applies what is whole and in order, drops the
/// rest — never out of order — and compacts, and the server sends the
/// dropped sequences again. Falsified by `open` applying any page that
/// decodes wherever it starts: the skipping one lands over a gap and the
/// peer reopens at 44 with sequence 42 never applied, where the pages that
/// follow on reach 41.
#[test]
fn a_torn_tail_is_dropped_and_fetched_again() {
    let mut s = server();
    let dev = || Options::dev("alice").with_autos(Autos::seeded(3));
    let disk = Memory::new();
    let mut alice = Peer::open(demo::domain(), Box::new(disk.clone()), dev()).unwrap();
    let mut bob = Peer::open_memory(demo::domain(), Options::dev("bob").with_autos(Autos::seeded(4))).unwrap();
    alice.connected();
    bob.connected();
    for i in 0..40 {
        create(&mut bob, &format!("b{i}"));
    }
    settle(&mut s, &mut [(1, &mut alice), (2, &mut bob)]);
    alice.persist().unwrap();
    assert!(pages(&disk).is_empty(), "forty sequences, one snapshot");
    for i in 0..4 {
        create(&mut alice, &format!("a{i}"));
        settle(&mut s, &mut [(1, &mut alice), (2, &mut bob)]);
        alice.persist().unwrap();
    }
    assert_eq!(pages(&disk).len(), 4);
    assert_eq!(alice.cursor(), 44);
    let schema = demo::domain().module().schema.clone();

    // Torn: the last page cut short, as a write that did not finish.
    let torn = copy(&disk);
    let last = torn.load(&ReplicaFile::page_key(4)).unwrap().unwrap();
    torn.clone().save(&ReplicaFile::page_key(4), &last[..last.len() / 2]).unwrap();
    // Out of order: the second page's slot holding a run that starts one
    // sequence late — so it, and the good page after it, are dropped.
    let skipped = copy(&disk);
    let third = decode_page(&skipped.load(&ReplicaFile::page_key(3)).unwrap().unwrap()).unwrap();
    let run: Vec<_> = (third.from..=third.to).zip(third.facts).collect();
    skipped.clone().save(&ReplicaFile::page_key(2), &encode_page(&run)).unwrap();

    for (what, broken, good) in [("torn", torn, 43), ("skipping", skipped, 41)] {
        let mut back = Peer::open(demo::domain(), Box::new(broken.clone()), dev()).unwrap();
        assert_eq!(back.cursor(), good, "{what}: reopened at the last good sequence");
        assert!(pages(&broken).is_empty(), "{what}: and compacted");
        assert_eq!(ReplicaFile::load(&broken, &schema).unwrap().unwrap().cursor, good);
        back.connected();
        settle(&mut s, &mut [(3, &mut back)]);
        assert_eq!(back.cursor(), 44, "{what}: fetched again");
        assert!(back.replica().confirmed == s.authority.store, "{what}: the server's store");
        assert_eq!(back.store().scan("playlist").len(), 44);
        back.persist().unwrap();
        reopens_as(&back, &broken, dev(), what);
        s.disconnect(3);
    }
}

/// A confirmed store replaced wholesale — a snapshot from below the
/// server's horizon — is written as a snapshot, not a page. Falsified by
/// taking `Journal::Replaced` as an empty journal: nothing is written, and
/// the `replica` record is still the empty one the open wrote.
#[test]
fn a_replaced_store_is_written_as_a_snapshot() {
    let mut s = server();
    let mut bob = Peer::open_memory(demo::domain(), Options::dev("bob").with_autos(Autos::seeded(4))).unwrap();
    bob.connected();
    for i in 0..6 {
        create(&mut bob, &format!("b{i}"));
    }
    settle(&mut s, &mut [(2, &mut bob)]);
    assert!(s.authority.compact(6));
    let disk = Memory::new();
    let mut alice = Peer::open(demo::domain(), Box::new(disk.clone()), Options::dev("alice")).unwrap();
    let at_open = disk.load(ReplicaFile::KEY).unwrap();
    alice.connected();
    settle(&mut s, &mut [(1, &mut alice)]);
    assert_eq!(alice.cursor(), 6);
    alice.persist().unwrap();
    assert_ne!(disk.load(ReplicaFile::KEY).unwrap(), at_open, "a new snapshot");
    assert!(pages(&disk).is_empty(), "and no page");
    reopens_as(&alice, &disk, Options::dev("alice"), "after a snapshot from the server");
}

/// A peer dropped between a mutation and the next pump writes its journal
/// on the way out. Falsified by `Drop` doing nothing: no page.
#[test]
fn dropping_the_peer_writes_the_journal() {
    let disk = Memory::new();
    let mut p = seeded(&disk, 30);
    create(&mut p, "Last");
    assert!(pages(&disk).is_empty(), "the journal waits for a pump");
    drop(p);
    assert_eq!(pages(&disk).len(), 1, "or a drop");
    let p = Peer::open(demo::domain(), Box::new(disk.clone()), alone()).unwrap();
    assert_eq!(p.cursor(), 31);
    assert!(p.store().scan("playlist").iter().any(|r| r["name"] == Value::text("Last")));
}

/// A storage written before the journal — a `replica` record with the
/// login inside and a `pending` record — opens as it did and writes
/// nothing, then journals on from it. Falsified by `open` compacting any
/// storage it has not seen a page of: the open writes the 585-byte
/// snapshot again.
#[test]
fn a_storage_in_the_old_layout_opens_unchanged() {
    let mut author = Peer::open_memory(demo::domain(), alone()).unwrap();
    for i in 0..12 {
        create(&mut author, &format!("p{i}"));
    }
    let r = author.replica();
    let disk = Counting::default();
    disk.clone()
        .save(ReplicaFile::KEY, &encode_replica("alone", r.cursor, &r.confirmed, "me", "local"))
        .unwrap();
    disk.clone().save(ReplicaFile::PENDING, &encode_pending(&[])).unwrap();
    disk.reset();
    let mut p = Peer::open(demo::domain(), Box::new(disk.clone()), alone()).unwrap();
    assert_eq!(disk.written(), (0, vec![]), "opening writes nothing");
    assert_eq!(p.cursor(), 12);
    assert!(p.replica().confirmed == author.replica().confirmed);
    create(&mut p, "New");
    p.pump();
    assert_eq!(disk.written().1, vec![ReplicaFile::page_key(1)]);
    reopens_as(&p, &disk.inner, alone(), "journalled on from the old layout");
}

/// All of it on a directory: the pages are files, a compaction removes
/// them, and a reopen reads back the live replica. Falsified by `Dir`
/// saving to the temporary name and never renaming it: the first reopen
/// finds no snapshot and is at 0, not 21.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_directory_round_trips_the_same_way() {
    let dir = std::env::temp_dir().join(format!("ark-client-journal-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut p = Peer::open_path(demo::domain(), &dir, alone()).unwrap();
    for i in 0..20 {
        create(&mut p, &format!("p{i}"));
    }
    p.pump();
    let mut saw_page = false;
    for i in 0..40 {
        create(&mut p, &format!("q{i}"));
        p.pump();
        saw_page |= dir.join(ReplicaFile::page_key(1)).exists();
        let back = Peer::open_path(demo::domain(), &dir, alone()).unwrap();
        assert_eq!(back.cursor(), p.cursor(), "after {i}");
        assert!(back.replica().confirmed == p.replica().confirmed, "after {i}");
    }
    assert!(saw_page, "pages are files");
    let names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(!names.iter().any(|n| n.ends_with(".tmp")), "{names:?}");
    let live = (p.cursor(), p.replica().confirmed.clone());
    drop(p);
    let back = Peer::open_path(demo::domain(), &dir, alone()).unwrap();
    assert_eq!((back.cursor(), back.replica().confirmed.clone()), live);
    drop(back);
    std::fs::remove_dir_all(&dir).unwrap();
}

// The pending intents: a snapshot and pages ----------------------------------------

fn pending_pages(disk: &Memory) -> Vec<String> {
    disk.keys().into_iter().filter(|k| k.starts_with("pending.")).collect()
}

/// A peer with a server it is not linked to: everything it authors stays
/// pending.
fn offline(user: &str) -> Options {
    Options::dev(user).with_autos(Autos::seeded(user.as_bytes()[0] as u64))
}

/// §R3 `mutate` writes one page holding its one intent, and nothing else —
/// the same bytes with three hundred intents pending as with two thousand
/// four hundred, and a small fraction of the snapshot a pump then folds
/// them into. Falsified by writing the whole `pending` record on every
/// mutate (the layout before the pages): the first mutate writes
/// `pending`, not `pending.1`.
#[test]
fn a_mutate_writes_its_intent_and_not_the_backlog() {
    let mut cost = vec![];
    for n in [300, 2400] {
        let disk = Counting::default();
        let mut p = Peer::open(demo::domain(), Box::new(disk.clone()), offline("alice")).unwrap();
        let mut each = std::collections::BTreeSet::new();
        for i in 0..n {
            disk.reset();
            create(&mut p, &format!("p{i:05}"));
            let (bytes, keys) = disk.written();
            assert_eq!(keys, vec![ReplicaFile::pending_page_key(i + 1)], "{n}: mutate {i} writes its page alone");
            each.insert(bytes);
        }
        assert_eq!(p.pending_len(), n);
        assert_eq!(each.len(), 1, "{n}: every mutate the same bytes: {each:?}");
        // A pump folds the pages into a snapshot; the next mutate is a page
        // again, the same size.
        p.pump();
        assert!(pending_pages(&disk.inner).is_empty(), "{n}: compacted");
        let snapshot = disk.inner.load(ReplicaFile::PENDING).unwrap().unwrap().len();
        disk.reset();
        create(&mut p, "p99999");
        let (bytes, keys) = disk.written();
        assert_eq!(keys, vec![ReplicaFile::pending_page_key(1)]);
        assert!(each.contains(&bytes), "{n}: {bytes}, not {each:?}");
        assert!(bytes * 100 < snapshot, "{n}: {bytes} bytes against a snapshot of {snapshot}");
        cost.push(bytes);
    }
    assert_eq!(cost[0], cost[1], "the same mutate costs the same bytes whatever is pending");
}

/// §R3 Reopened after every mutate and after every pump, a peer's pending
/// intents are the live ones: authored offline, answered a few at a time,
/// refused, compacted as the pages outgrow the snapshot and as the list
/// empties. Falsified by `open` reading the `pending` snapshot and not the
/// pages after it: the first reopen after a mutate has no intent.
#[test]
fn reopening_after_every_mutate_and_every_pump_is_the_live_pending() {
    let mut s = server();
    let (da, db) = (Memory::new(), Memory::new());
    let mut alice = Peer::open(demo::domain(), Box::new(da.clone()), offline("alice")).unwrap();
    let mut bob = Peer::open(demo::domain(), Box::new(db.clone()), offline("bobby")).unwrap();
    bob.connected();
    let (mut compacted, mut emptied) = (0, 0);
    for round in 0..8 {
        for i in 0..(3 + round * 4) {
            create(&mut alice, &format!("a{round}-{i}"));
            reopens_as(&alice, &da, offline("alice"), &format!("round {round}, mutate {i}"));
        }
        if round % 3 == 1 {
            // Answered: linked, every intent acknowledged at once.
            alice.connected();
            settle(&mut s, &mut [(1, &mut alice), (2, &mut bob)]);
            alice.disconnected();
            s.disconnect(1);
        } else if round % 3 == 2 {
            // Refused in part: bob took two of the names first.
            create(&mut bob, &format!("a{round}-0"));
            create(&mut bob, &format!("a{round}-1"));
            settle(&mut s, &mut [(2, &mut bob)]);
            alice.connected();
            settle(&mut s, &mut [(1, &mut alice), (2, &mut bob)]);
            alice.disconnected();
            s.disconnect(1);
        }
        let before = da.load(ReplicaFile::PENDING).unwrap();
        alice.persist().unwrap();
        if da.load(ReplicaFile::PENDING).unwrap() != before {
            compacted += 1;
            emptied += usize::from(alice.pending_len() == 0);
        }
        reopens_as(&alice, &da, offline("alice"), &format!("round {round}, pumped"));
    }
    assert!(compacted > emptied && emptied > 0, "{compacted} compactions, {emptied} of them emptied");
}

/// §R3 A page torn in the writing is dropped with everything after it —
/// never applied out of order — and the peer reopens to the last whole
/// op, compacted; a page of an older generation, which a compaction
/// stopped before removing, is skipped. Falsified by applying a page
/// whatever its generation: the stopped compaction reopens with the
/// acknowledged intent back, `[b, a]` where the live list is `[b]`.
#[test]
fn a_torn_page_is_dropped_and_the_peer_reopens_to_the_last_whole_op() {
    let disk = Memory::new();
    let mut p = Peer::open(demo::domain(), Box::new(disk.clone()), offline("alice")).unwrap();
    for i in 0..10 {
        create(&mut p, &format!("p{i}"));
    }
    p.pump();
    assert!(pending_pages(&disk).is_empty(), "ten intents, one snapshot");
    let ids: Vec<_> = (0..4)
        .map(|i| {
            create(&mut p, &format!("q{i}"));
            p.replica().pending.last().unwrap().id
        })
        .collect();
    assert_eq!(pending_pages(&disk).len(), 4);
    let live = p.replica().pending.clone();

    for (torn_at, keep) in [(4, 13), (2, 11)] {
        let broken = copy(&disk);
        let key = ReplicaFile::pending_page_key(torn_at);
        let page = broken.load(&key).unwrap().unwrap();
        broken.clone().save(&key, &page[..page.len() / 2]).unwrap();
        let back = Peer::open(demo::domain(), Box::new(broken.clone()), offline("alice")).unwrap();
        assert_eq!(back.replica().pending, live[..keep], "torn at {torn_at}: the last whole op");
        assert!(pending_pages(&broken).is_empty(), "torn at {torn_at}: and compacted");
        let (_, on_disk) = decode_pending_snapshot(&broken.load(ReplicaFile::PENDING).unwrap().unwrap()).unwrap();
        assert_eq!(on_disk, live[..keep]);
        assert!(!back.replica().pending.iter().any(|e| e.id == ids[torn_at - 1]), "torn at {torn_at}");
    }

    // A compaction stopped between its snapshot and removing the pages:
    // the pages say add a, add b, drop a (acknowledged); the snapshot of
    // the next generation already says [b].
    let mut author = Peer::open_memory(demo::domain(), offline("alice")).unwrap();
    create(&mut author, "a");
    create(&mut author, "b");
    let (a, b) = (author.replica().pending[0].clone(), author.replica().pending[1].clone());
    let mut stopped = Memory::new();
    stopped.save(ReplicaFile::PENDING, &encode_pending_snapshot(1, &[])).unwrap();
    stopped
        .save(&ReplicaFile::pending_page_key(1), &encode_pending_page(1, &[&a], &[]))
        .unwrap();
    stopped
        .save(&ReplicaFile::pending_page_key(2), &encode_pending_page(1, &[&b], &[]))
        .unwrap();
    stopped
        .save(&ReplicaFile::pending_page_key(3), &encode_pending_page(1, &[], &[a.id]))
        .unwrap();
    let whole = copy(&stopped);
    let back = Peer::open(demo::domain(), Box::new(whole), offline("alice")).unwrap();
    assert_eq!(
        back.replica().pending,
        std::slice::from_ref(&b),
        "before the compaction: the pages say [b]"
    );
    stopped
        .save(ReplicaFile::PENDING, &encode_pending_snapshot(2, std::slice::from_ref(&b)))
        .unwrap();
    let back = Peer::open(demo::domain(), Box::new(stopped.clone()), offline("alice")).unwrap();
    assert_eq!(back.replica().pending, [b], "after it: the older generation's pages skipped");
    assert!(pending_pages(&stopped).is_empty(), "and removed");
}

/// §R3 A storage in the layout before the pages — one `pending` record,
/// no generation — opens as it did and writes nothing, then pages on
/// from it. Falsified by `open` treating a record with no `gen` as
/// unclean: the open writes the snapshot again, 406 bytes to `pending`.
#[test]
fn an_old_pending_record_opens_unchanged() {
    let mut author = Peer::open_memory(demo::domain(), offline("alice")).unwrap();
    for i in 0..3 {
        create(&mut author, &format!("p{i}"));
    }
    let disk = Counting::default();
    let r = author.replica();
    disk.clone()
        .save(ReplicaFile::KEY, &encode_replica("server", r.cursor, &r.confirmed, "alice", "dev"))
        .unwrap();
    disk.clone().save(ReplicaFile::PENDING, &encode_pending(&r.pending)).unwrap();
    disk.reset();
    let fresh = Options::dev("alice").with_autos(Autos::seeded(1234));
    let mut p = Peer::open(demo::domain(), Box::new(disk.clone()), fresh).unwrap();
    assert_eq!(disk.written(), (0, vec![]), "opening writes nothing");
    assert_eq!(p.replica().pending, author.replica().pending);
    create(&mut p, "p3");
    assert_eq!(disk.written().1, vec![ReplicaFile::pending_page_key(1)]);
    reopens_as(&p, &disk.inner, offline("alice"), "paged on from the old layout");
    assert_eq!(p.pending_len(), 4);
}

/// §R3 The pages on a directory: files, one per mutate; folded and removed
/// on a pump; and a reopen — from a copy, so it cannot write into the live
/// peer's — is the live pending. Falsified by `compact_pending` never
/// running: the pages are still files after the pump.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_directory_round_trips_the_pending_pages() {
    let root = std::env::temp_dir().join(format!("ark-client-pending-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join("live");
    let snap = |n: usize| -> std::path::PathBuf {
        let to = root.join(format!("copy-{n}"));
        std::fs::create_dir_all(&to).unwrap();
        for e in std::fs::read_dir(&dir).unwrap() {
            let e = e.unwrap();
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
        to
    };
    let mut p = Peer::open_path(demo::domain(), &dir, offline("alice")).unwrap();
    for i in 0..30 {
        create(&mut p, &format!("p{i}"));
        assert!(dir.join(ReplicaFile::pending_page_key(i + 1)).is_file(), "{i}: a page, a file");
        let back = Peer::open_path(demo::domain(), snap(i), offline("alice")).unwrap();
        assert_eq!(back.replica().pending, p.replica().pending, "{i}");
    }
    p.pump();
    assert!(!dir.join(ReplicaFile::pending_page_key(1)).exists(), "folded on the pump");
    let mut s = server();
    p.connected();
    settle(&mut s, &mut [(1, &mut p)]);
    p.persist().unwrap();
    assert_eq!(p.pending_len(), 0);
    let back = Peer::open_path(demo::domain(), snap(99), offline("alice")).unwrap();
    assert_eq!((back.pending_len(), back.cursor()), (0, 30));
    let names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(!names.iter().any(|n| n.ends_with(".tmp") || n.starts_with("pending.")), "{names:?}");
    drop((p, back));
    std::fs::remove_dir_all(&root).unwrap();
}
