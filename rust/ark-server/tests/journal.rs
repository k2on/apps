//! The server's log on disk as a snapshot and a journal
//! (`docs/plan-perf.md` §R3; the `persist` module docs): what an append
//! costs, what a stop at every point of a write leaves, and that a
//! directory written before the journal opens as it did. Each test says
//! what falsified it.

use std::fs;
use std::path::Path;

use ark::eval::Ctx;
use ark::log::{Entry, Log};
use ark::peer::{Authority, Sequenced};
use ark::value::Value;
use ark_client::{demo, Domain};
use ark_server::persist::{self, encode_record, journal_path_of, path_of, LogFile};

/// An authority over the demo module, and the domain it came from.
fn authority() -> (Authority, Domain) {
    let d = demo::domain();
    let mut a = Authority::new(d.module().schema.clone(), d.closures().clone());
    a.hold(d.native_list());
    (a, d)
}

/// Sequence the `i`th playlist: every one the same size on the disk — a
/// fixed-width name, an id of the same length — so that bytes compare.
fn author(a: &mut Authority, d: &Domain, i: u32) {
    let (fh, _) = d.mutator("create_playlist").unwrap();
    let mut id = [7u8; 16];
    id[..4].copy_from_slice(&i.to_be_bytes());
    let ctx = Ctx::new("alice", "dev");
    let e = Entry {
        id,
        actor: ctx.user.clone(),
        session: ctx.session.clone(),
        fn_hash: fh.clone(),
        args: [("name".to_string(), Value::text(format!("p{i:05}")))].into(),
        autos: [("id".to_string(), Value::Id(id))].into(),
    };
    assert!(matches!(a.sequence_entry(&e), Sequenced::Appended(..)), "{i}");
}

fn open(dir: &Path) -> (LogFile, Option<Log>) {
    LogFile::open(dir, &demo::domain().module().schema).unwrap()
}

fn load(dir: &Path) -> Option<Log> {
    persist::load(dir, &demo::domain().module().schema).unwrap()
}

/// `n` entries, each written as the hub writes them: one `write` per
/// append.
fn written(dir: &Path, n: u32) -> (Authority, Domain, LogFile) {
    let (mut a, d) = authority();
    let (mut f, _) = open(dir);
    for i in 0..n {
        author(&mut a, &d, i);
        f.write(&a.log).unwrap();
    }
    (a, d, f)
}

/// A copy of what `from` holds, the journal cut to `cut` bytes.
fn copy_cut(from: &Path, to: &Path, cut: usize) {
    fs::copy(path_of(from), path_of(to)).unwrap();
    let j = fs::read(journal_path_of(from)).unwrap();
    fs::write(journal_path_of(to), &j[..cut]).unwrap();
}

/// §R3 An append writes its own record and not the log: the same bytes at
/// three hundred entries as at two thousand four hundred, a small fraction
/// of the snapshot, and — compactions included — a bounded multiple of the
/// records over the whole run. Falsified by `write` writing a snapshot
/// every time (the layout before the journal): 11,530,570 bytes written
/// for 69,022 bytes of records at three hundred entries.
#[test]
fn bytes_written_per_append_do_not_grow_with_the_log() {
    let mut cost = vec![];
    // Sizes whose sequences and positions take the same bytes to write (a
    // CBOR integer from 256 to 65535 is three).
    for n in [300u32, 2400] {
        let dir = tempfile::tempdir().unwrap();
        let (mut a, d, mut f) = written(dir.path(), n);
        let records: u64 = a.log.entries.iter().map(|(s, (e, fs))| encode_record(*s, e, fs).len() as u64).sum();
        assert!(
            f.written <= 5 * records,
            "{n}: {} bytes written for {records} bytes of records",
            f.written
        );
        // One more, and not one that lands on a compaction (the one after a
        // compaction never is: the journal is empty and the snapshot whole).
        let mut i = n;
        let bytes = loop {
            assert!(i < n + 3, "{n}: three appends in a row compacted");
            let before = f.written;
            author(&mut a, &d, i);
            f.write(&a.log).unwrap();
            i += 1;
            if f.sizes().1 > 0 {
                break f.written - before;
            }
        };
        let (snapshot, _) = f.sizes();
        assert!(bytes * 20 < snapshot, "{n}: {bytes} bytes against a snapshot of {snapshot}");
        assert_eq!(
            fs::metadata(journal_path_of(dir.path())).unwrap().len(),
            f.sizes().1,
            "{n}: the file is what was counted"
        );
        cost.push(bytes);
    }
    assert_eq!(cost[0], cost[1], "the same append costs the same bytes at any length");
}

/// §R3 A kill at every byte of the last record's write: the journal cut
/// at each offset inside it reopens to the entry before, whole, with the
/// file truncated where the good records end — and the next append then
/// follows on. Uncut, it reopens to the last. Falsified by `open` leaving
/// the torn tail in place: one byte into the record, the file is 921
/// bytes where its good records end at 920, and an append after that byte
/// would be one no reader reaches.
#[test]
fn a_journal_cut_anywhere_in_its_last_record_reopens_to_the_one_before() {
    let dir = tempfile::tempdir().unwrap();
    let (mut a, d, f) = written(dir.path(), 39);
    let before = a.log.clone();
    author(&mut a, &d, 39);
    let mut f = f;
    f.write(&a.log).unwrap();
    let (_, journal) = f.sizes();
    let last = {
        let (e, fs) = &a.log.entries[&40];
        encode_record(40, e, fs).len() as u64
    };
    assert!(journal >= last, "the last record is in the journal ({journal} bytes, the record {last})");
    let start = (journal - last) as usize;
    drop(f);

    for cut in start..journal as usize {
        let to = tempfile::tempdir().unwrap();
        copy_cut(dir.path(), to.path(), cut);
        assert_eq!(load(to.path()).as_ref(), Some(&before), "cut at {cut}: read");
        let (mut g, log) = open(to.path());
        assert_eq!(log.as_ref(), Some(&before), "cut at {cut}: opened");
        assert_eq!(
            fs::metadata(journal_path_of(to.path())).unwrap().len(),
            start as u64,
            "cut at {cut}: truncated"
        );
        g.write(&a.log).unwrap();
        drop(g);
        assert_eq!(open(to.path()).1.as_ref(), Some(&a.log), "cut at {cut}: appended after");
    }
    let to = tempfile::tempdir().unwrap();
    copy_cut(dir.path(), to.path(), journal as usize);
    assert_eq!(open(to.path()).1.as_ref(), Some(&a.log), "whole");
}

/// §R3 The snapshot's rename leaves the old file or the new one. A stop
/// before it — a temporary file half written beside the old — reopens as
/// the old snapshot and its journal; a stop after it and before the
/// journal is emptied reopens as the new, skipping the records it holds
/// — and so does a journal that failed to empty and was appended on.
/// Falsified by reading a record at or below the snapshot's head as the
/// end of the journal: the last case reopens at 30, not 32.
#[test]
fn a_stop_either_side_of_the_snapshots_rename_reopens_whole() {
    let dir = tempfile::tempdir().unwrap();
    let (mut a, d, f) = written(dir.path(), 30);
    assert!(f.sizes().1 > 0, "a journal to stop beside");
    drop(f);

    // Before the rename: the new snapshot half written to the temporary name.
    let before = tempfile::tempdir().unwrap();
    copy_cut(
        dir.path(),
        before.path(),
        fs::metadata(journal_path_of(dir.path())).unwrap().len() as usize,
    );
    let whole = ark::canon::encode(&persist::log_to_value(&a.log));
    fs::write(before.path().join(".log.ark-log.tmp"), &whole[..whole.len() / 2]).unwrap();
    assert_eq!(open(before.path()).1.as_ref(), Some(&a.log), "the old snapshot and its journal");

    // After the rename, before the journal is emptied.
    let after = tempfile::tempdir().unwrap();
    copy_cut(
        dir.path(),
        after.path(),
        fs::metadata(journal_path_of(dir.path())).unwrap().len() as usize,
    );
    fs::write(path_of(after.path()), &whole).unwrap();
    assert_eq!(load(after.path()).as_ref(), Some(&a.log), "read: the records it holds skipped");
    let (_, log) = open(after.path());
    assert_eq!(log.as_ref(), Some(&a.log), "opened");
    assert_eq!(fs::metadata(journal_path_of(after.path())).unwrap().len(), 0, "and compacted");

    // …and a journal that could not be emptied, appended on after them.
    let stuck = tempfile::tempdir().unwrap();
    copy_cut(
        dir.path(),
        stuck.path(),
        fs::metadata(journal_path_of(dir.path())).unwrap().len() as usize,
    );
    fs::write(path_of(stuck.path()), &whole).unwrap();
    let mut tail = vec![];
    for i in 30..32 {
        author(&mut a, &d, i);
        let n = a.log.head_seq();
        let (e, fs_) = &a.log.entries[&n];
        tail.extend(encode_record(n, e, fs_));
    }
    let mut j = fs::read(journal_path_of(stuck.path())).unwrap();
    j.extend(tail);
    fs::write(journal_path_of(stuck.path()), j).unwrap();
    assert_eq!(open(stuck.path()).1.map(|l| l.head_seq()), Some(32));
    assert_eq!(load(stuck.path()).as_ref(), Some(&a.log));
}

/// §R3 The journal is folded into the snapshot once it is larger, and a
/// horizon moved by `compact_to` is written as a snapshot: either way a
/// reopen is the live log, and the journal never outgrows the snapshot.
/// Falsified by never compacting: the journal outgrows the snapshot and
/// stays.
#[test]
fn a_compaction_reopens_identically() {
    let dir = tempfile::tempdir().unwrap();
    let (mut a, d) = authority();
    let (mut f, _) = open(dir.path());
    let mut compactions = 0;
    for i in 0..200 {
        author(&mut a, &d, i);
        let before = f.sizes();
        f.write(&a.log).unwrap();
        let (snapshot, journal) = f.sizes();
        assert!(journal <= snapshot, "{i}: a journal of {journal} over a snapshot of {snapshot}");
        if journal == 0 && snapshot > before.0 {
            compactions += 1;
            assert_eq!(open(dir.path()).1.as_ref(), Some(&a.log), "{i}: after a compaction");
        }
    }
    assert!(compactions > 2, "{compactions} compactions");
    assert_eq!(open(dir.path()).1.as_ref(), Some(&a.log));

    // The horizon moved: a snapshot at it, and the entries above it.
    assert!(a.compact(150));
    f.write(&a.log).unwrap();
    assert_eq!(f.sizes().1, 0, "written as a snapshot");
    author(&mut a, &d, 200);
    f.write(&a.log).unwrap();
    let (_, back) = open(dir.path());
    let back = back.unwrap();
    assert_eq!((back.horizon(), back.head_seq()), (150, 201));
    assert_eq!(back, a.log);
}

/// §R3 A data directory written before the journal — `log.ark-log`
/// alone, as the old `save` wrote it — opens as the log it holds, writes
/// nothing, and is journalled on from. Falsified by `open` compacting
/// whatever it opens: 4,663 bytes written before anything moved.
#[test]
fn an_old_data_directory_opens_unchanged() {
    let (mut a, d) = authority();
    for i in 0..25 {
        author(&mut a, &d, i);
    }
    assert!(a.compact(10));
    let dir = tempfile::tempdir().unwrap();
    let bytes = ark::canon::encode(&persist::log_to_value(&a.log));
    fs::write(path_of(dir.path()), &bytes).unwrap();

    let (mut f, log) = open(dir.path());
    assert_eq!(log.as_ref(), Some(&a.log));
    assert_eq!(f.written, 0, "opening writes nothing");
    assert!(!journal_path_of(dir.path()).exists(), "…not even an empty journal");
    assert_eq!(fs::read(path_of(dir.path())).unwrap(), bytes);

    author(&mut a, &d, 25);
    f.write(&a.log).unwrap();
    assert!(f.sizes().1 > 0, "journalled on");
    assert_eq!(fs::read(path_of(dir.path())).unwrap(), bytes, "the snapshot untouched");
    assert_eq!(open(dir.path()).1.as_ref(), Some(&a.log));
}

/// A whole server over a directory: every push reaches the journal, and
/// a server started again over it hands the peers back the same log.
/// Falsified by `open_hub` ignoring the journal (reading `log.ark-log`
/// alone): the restarted head is behind the one the peers confirmed.
#[test]
fn a_server_restarted_over_its_directory_has_every_entry() {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let data = tempfile::tempdir().unwrap();
    let build = || {
        ark_server::builder(demo::domain())
            .name("journal")
            .trusting()
            .data(data.path())
            .build()
            .unwrap()
    };
    let app = build();
    let mut p = ark_client::Peer::open_memory(demo::domain(), ark_client::Options::dev("alice")).unwrap();
    p.connect_with("local", app.hub.dial());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let pump_until = |p: &mut ark_client::Peer, done: &dyn Fn(&ark_client::Peer) -> bool| {
        while !done(p) {
            p.pump();
            assert!(std::time::Instant::now() < deadline, "{:?}", p.status());
            std::thread::yield_now();
        }
    };
    for i in 0..60 {
        p.mutate("create_playlist", ark_client::args([("name", Value::text(format!("p{i}")))]))
            .unwrap();
        pump_until(&mut p, &|p| p.pending_len() == 0);
    }
    let head = p.cursor();
    // Every wait is bounded: a hub that stopped answering is a failure, not
    // a test that runs until somebody kills it.
    let read = |hub: &ark_server::HubHandle| {
        rt.block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(20), hub.read(|h| h.authority().log.clone()))
                .await
                .expect("the hub answers")
                .unwrap()
        })
    };
    let log = read(&app.hub);
    assert_eq!(log.head_seq(), head);
    assert!(
        fs::metadata(journal_path_of(data.path())).unwrap().len() > 0,
        "the last pushes are in the journal"
    );
    drop(p);
    drop(app);
    let again = build();
    let back = read(&again.hub);
    assert_eq!(back.head_seq(), head, "every entry a peer was told of");
    assert_eq!(back, log);
}

/// R10: `Authority::compact` moves the entries it keeps rather than
/// cloning them, and takes the head's state from its own store; what it
/// leaves is exactly the log `Log::compact_to` builds, at the horizon, in
/// the middle, at the head, and not below the horizon at all. Falsified by
/// splitting the entries at `n` rather than `n + 1`: the entry at the new
/// horizon is kept above it.
#[test]
fn compacting_in_place_is_compacting() {
    let (mut a, d) = authority();
    for i in 0..200 {
        author(&mut a, &d, i);
    }
    assert!(a.compact(20));
    for n in [20, 21, 150, 200] {
        let mut b = a.clone();
        assert!(b.compact(n), "{n}");
        assert_eq!(Some(&b.log), a.log.compact_to(n).as_ref(), "at {n}");
        assert_eq!(b.store, a.store, "the head's state does not move");
    }
    let mut b = a.clone();
    assert!(!b.compact(19), "below the horizon");
    assert!(!b.compact(201), "past the head");
    assert_eq!(b, a);
}
