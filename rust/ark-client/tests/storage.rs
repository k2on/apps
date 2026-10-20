//! The store on disk held: what was persisted reads back; a torn tail is
//! cut on open, in the file too; the rows written whole fold every write
//! before them; blobs are written whole; a directory of the format before
//! the log was a table is refused.

use ark::log::Record;
use ark::store::Change;
use ark_client::storage::Stored;
use ark_client::{Disk, Files, Memory};

/// The `k`-th write: a row added.
fn write(k: i64) -> Vec<Change> {
    vec![Change::Add {
        table: "t".into(),
        row: ark::codec::int_enc(k).to_vec(),
    }]
}

fn persist(d: &mut dyn Disk, ks: std::ops::Range<i64>) {
    for k in ks {
        d.record(&Record::Writes(write(k)).enc()).unwrap();
    }
}

/// Every write persisted reads back; a torn tail is cut at the last whole
/// record, and the file with it, so a write after the open is read after
/// the others. Falsified by `load` not cutting the file: the record
/// appended after the open sat behind the torn bytes and the second open
/// lost it.
#[test]
fn a_torn_tail_is_cut() {
    let dir = tempfile::tempdir().unwrap();
    let mut f = Files::new(dir.path()).unwrap();
    persist(&mut f, 1..3);
    let two: Vec<Vec<Change>> = (1..3).map(write).collect();
    assert_eq!(f.load().unwrap().writes, two);
    let path = dir.path().join("writes");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.extend_from_slice(&[0, 0, 0, 200, 1, 2, 3]);
    std::fs::write(&path, &bytes).unwrap();
    let mut f = Files::new(dir.path()).unwrap();
    assert_eq!(f.load().unwrap().writes, two, "the torn record is not read");
    persist(&mut f, 3..4);
    assert_eq!(
        Files::new(dir.path()).unwrap().load().unwrap().writes,
        (1..4).map(write).collect::<Vec<_>>(),
        "and what follows it is read"
    );
}

/// The store's rows written whole are the new start: the writes before
/// them are gone and those after are read on them. Blobs read back whole.
/// Falsified by `record` keeping the writes under the rows: the old writes
/// were read again on rows that already held them.
#[test]
fn the_rows_fold_every_write_before() {
    for d in [
        &mut Memory::default() as &mut dyn Disk,
        &mut Files::new(tempfile::tempdir().unwrap().keep()).unwrap(),
    ] {
        persist(d, 1..3);
        let rows = vec![("t".to_string(), vec![ark::codec::int_enc(1).to_vec()])];
        d.record(&Record::Rows(rows.clone()).enc()).unwrap();
        persist(d, 3..5);
        let want = Stored {
            rows: Some(rows),
            writes: (3..5).map(write).collect(),
        };
        assert_eq!(d.load().unwrap(), want);
        d.put("pending", b"\x00\x01").unwrap();
        assert_eq!(d.get("pending").unwrap().as_deref(), Some(&b"\x00\x01"[..]));
        assert_eq!(d.get("nothing").unwrap(), None);
    }
}

/// A directory holding a log of the format before the log was a table
/// (`snapshot`, `chain`) does not open as an empty one. Falsified by
/// `load` reading only `kv` and `writes`: it opened empty.
#[test]
fn the_format_before_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("chain"), [0, 0, 0, 1, 0]).unwrap();
    let err = Files::new(dir.path()).unwrap().load().unwrap_err();
    assert!(err.to_string().contains("before the log was a table"), "{err}");
}
