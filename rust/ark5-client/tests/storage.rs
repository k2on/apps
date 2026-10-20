//! The chain on disk held: what was persisted reads back; a torn tail and
//! an entry whose hash does not check are cut on open, in the file too; a
//! snapshot starts the chain again; blobs are written whole.

use std::sync::Arc;

use ark5::hash::{EntryHash, FnHash, StateHash};
use ark5::log::{Entry, EntryId, Log, Origin, Record, Snapshot};
use ark5::schema::{Schema, Table};
use ark5::store::{Change, Store};
use ark5::ty::Ty;
use ark5_client::storage::Stored;
use ark5_client::{Disk, Files, Memory};

fn store() -> Store {
    Store::new(Arc::new(Schema {
        tables: vec![Table::new("t").col("k", "Int", Ty::Int)],
    }))
}

/// A log of `n` entries, each adding a row, standing on the empty store.
fn log(n: i64) -> Log {
    let mut s = store();
    let mut log = Log::new(Snapshot {
        seq: 0,
        hash: EntryHash::default(),
        state: s.state_hash(),
        rows: s.snapshot_rows(),
    });
    for k in 1..=n {
        let c = Change::Add {
            table: "t".into(),
            row: ark5::codec::int_enc(k).to_vec(),
        };
        s.apply(&c).unwrap();
        let e = Entry {
            id: EntryId::of(Origin::named("s"), k),
            actor: "a".into(),
            session: "s".into(),
            origin: Origin::named("s"),
            local: k,
            roles: vec![],
            func: FnHash::default(),
            input: vec![],
            now: k,
        };
        log.append(e, vec![c], s.state_hash());
    }
    log
}

fn persist(d: &mut dyn Disk, log: &Log) {
    for l in &log.entries {
        d.record(&Record::Logged(l.clone()).enc()).unwrap();
    }
}

/// Every entry persisted reads back; a torn tail is cut at the last whole
/// record, and the file with it, so an append after the open links. Falsified
/// by `load` not cutting the file: the record appended after the open sat
/// behind the torn bytes and the second open lost it.
#[test]
fn a_torn_tail_is_cut() {
    let dir = tempfile::tempdir().unwrap();
    let l = log(3);
    let mut f = Files::new(dir.path()).unwrap();
    persist(
        &mut f,
        &Log {
            entries: l.entries[..2].to_vec(),
            ..l.clone()
        },
    );
    assert_eq!(f.load().unwrap().chain, l.entries[..2]);
    let path = dir.path().join("chain");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.extend_from_slice(&[0, 0, 0, 200, 1, 2, 3]);
    std::fs::write(&path, &bytes).unwrap();
    let mut f = Files::new(dir.path()).unwrap();
    assert_eq!(f.load().unwrap().chain, l.entries[..2], "the torn record is not read");
    f.record(&Record::Logged(l.entries[2].clone()).enc()).unwrap();
    assert_eq!(
        Files::new(dir.path()).unwrap().load().unwrap().chain,
        l.entries,
        "and what follows it links"
    );
}

/// A record whose hash does not check against the one before it ends the
/// chain there, on disk and in memory alike. Falsified by `read` taking any
/// record that decodes: the tampered entry and the one after were read.
#[test]
fn an_entry_whose_hash_does_not_check_ends_the_chain() {
    let l = log(3);
    let mut bad = l.clone();
    bad.entries[1].state = StateHash([9; 32]);
    let mut m = Memory::default();
    persist(&mut m, &bad);
    assert_eq!(m.load().unwrap().chain, l.entries[..1]);
    assert_eq!(m.chain.len(), 1, "cut");
    let dir = tempfile::tempdir().unwrap();
    let mut f = Files::new(dir.path()).unwrap();
    persist(&mut f, &bad);
    assert_eq!(f.load().unwrap().chain, l.entries[..1]);
    assert_eq!(
        std::fs::metadata(dir.path().join("chain")).unwrap().len() as usize,
        Record::Logged(l.entries[0].clone()).enc().len() + 4
    );
}

/// A snapshot is the chain's new start: the entries before it are gone and
/// those after link onto it. Blobs read back whole. Falsified by `record`
/// keeping the chain under a snapshot: the old entries were read on it and
/// did not link, and nothing after the snapshot was read.
#[test]
fn a_snapshot_starts_the_chain_again() {
    let l = log(4);
    for d in [
        &mut Memory::default() as &mut dyn Disk,
        &mut Files::new(tempfile::tempdir().unwrap().keep()).unwrap(),
    ] {
        persist(
            d,
            &Log {
                entries: l.entries[..2].to_vec(),
                ..l.clone()
            },
        );
        let at = &l.entries[1];
        let base = Snapshot {
            seq: 2,
            hash: at.hash,
            state: at.state,
            rows: vec![],
        };
        d.record(&Record::Snapshot(base.clone()).enc()).unwrap();
        persist(
            d,
            &Log {
                entries: l.entries[2..].to_vec(),
                ..l.clone()
            },
        );
        let want = Stored {
            snapshot: Some(base),
            chain: l.entries[2..].to_vec(),
        };
        assert_eq!(d.load().unwrap(), want);
        d.put("pending", b"\x00\x01").unwrap();
        assert_eq!(d.get("pending").unwrap().as_deref(), Some(&b"\x00\x01"[..]));
        assert_eq!(d.get("nothing").unwrap(), None);
    }
}
