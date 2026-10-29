//! The store on disk held: what was persisted reads back; a torn tail is
//! cut on open, in the file too; the rows written whole fold every entry
//! before them; blobs are written whole; a directory of a format before —
//! the log not a table, every write journaled, or a peer's own state in
//! blobs of its own — is refused.

use ark::log::{Entry, Logged, Record};
use ark_peer::disk::Stored;
use ark_peer::{Disk, Files, Memory};

/// The `k`-th entry of the log (the disk does not link them: opening is
/// the peer's).
fn row(k: i64) -> Logged {
    let mut l = Logged::intent(Entry::default());
    l.seq = k;
    l
}

fn persist(d: &mut dyn Disk, ks: std::ops::Range<i64>) {
    for k in ks {
        d.record(&Record::Logged(row(k)).enc()).unwrap();
    }
}

/// Every entry persisted reads back; a torn tail is cut at the last whole
/// record, and the file with it, so a write after the open is read after
/// the others. Falsified by `load` not cutting the file: the record
/// appended after the open sat behind the torn bytes and the second open
/// lost it.
#[test]
fn a_torn_tail_is_cut() {
    let dir = tempfile::tempdir().unwrap();
    let mut f = Files::new(dir.path()).unwrap();
    persist(&mut f, 1..3);
    let two: Vec<Logged> = (1..3).map(row).collect();
    assert_eq!(f.load().unwrap().logged, two);
    let path = dir.path().join("log");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.extend_from_slice(&[0, 0, 0, 200, 1, 2, 3]);
    std::fs::write(&path, &bytes).unwrap();
    let mut f = Files::new(dir.path()).unwrap();
    assert_eq!(f.load().unwrap().logged, two, "the torn record is not read");
    persist(&mut f, 3..4);
    assert_eq!(
        Files::new(dir.path()).unwrap().load().unwrap().logged,
        (1..4).map(row).collect::<Vec<_>>(),
        "and what follows it is read"
    );
}

/// The store's rows written whole are the new start: the entries before
/// them are gone and those after are read on them. Blobs read back whole.
/// Falsified by `record` keeping the entries under the rows: the old ones
/// were read again on rows that already held them.
#[test]
fn the_rows_fold_every_write_before() {
    for d in [
        &mut Memory::default() as &mut dyn Disk,
        &mut Files::new(tempfile::tempdir().unwrap().keep()).unwrap(),
    ] {
        persist(d, 1..3);
        let rows = vec![("t".to_string(), vec![br#"{"k":1}"#.to_vec()])];
        d.record(&Record::Rows(rows.clone()).enc()).unwrap();
        persist(d, 3..5);
        let want = Stored {
            rows: Some(rows),
            logged: (3..5).map(row).collect(),
            parts: vec![],
        };
        assert_eq!(d.load().unwrap(), want);
        d.put("blob", b"\x00\x01").unwrap();
        assert_eq!(d.get("blob").unwrap().as_deref(), Some(&b"\x00\x01"[..]));
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

/// A directory of the format that journaled every write — each entry's
/// facts, the rows they wrote and its row of the log's table, in `writes`
/// — is refused with a message, never read as one with no entries since
/// its rows. Falsified by `load` not looking for `writes`: it opened on
/// `kv` alone and lost the entry.
#[test]
fn the_format_that_journaled_every_write_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("kv"), Record::Rows(vec![]).enc()).unwrap();
    std::fs::write(dir.path().join("writes"), [0, 0, 0, 2, 0, 0]).unwrap();
    let err = Files::new(dir.path()).unwrap().load().unwrap_err();
    assert!(err.to_string().contains("journaled every write"), "{err}");
}

/// A directory that kept a peer's own state in blobs of its own (here a
/// device's `pending` journal) is refused, not opened as one with nothing
/// pending. Falsified by `load` not looking for them: it opened, and the
/// pending intents were lost.
#[test]
fn the_format_of_side_blobs_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("pending"), [0, 0, 0, 1, 0]).unwrap();
    let err = Files::new(dir.path()).unwrap().load().unwrap_err();
    assert!(err.to_string().contains("in blobs (pending)"), "{err}");
}
