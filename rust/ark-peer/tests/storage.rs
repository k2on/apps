//! The store on disk held: what was persisted reads back; a torn tail is
//! cut on open, in the file too; the rows written whole fold every entry
//! before them; blobs are written whole; a directory of a format before —
//! the log not a table, every write journaled, or a peer's own state in
//! blobs of its own — is refused.

use ark::log::{Ctx, Logged, Record};
use ark_peer::{Disk, Files, Memory};

/// The `k`-th entry of the log (the disk does not link them: opening is
/// the peer's).
fn row(k: i64) -> Logged {
    let mut l = Logged::intent(Ctx::default(), vec![]);
    l.seq = k;
    l
}

/// The entries a disk's records hold.
fn logged(records: Vec<Record>) -> Vec<Logged> {
    records
        .into_iter()
        .filter_map(|r| match r {
            Record::Logged(l) => Some(l),
            _ => None,
        })
        .collect()
}

fn persist(d: &mut dyn Disk, ks: std::ops::Range<i64>) {
    for k in ks {
        d.record(&Record::Logged(row(k))).unwrap();
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
    assert_eq!(logged(f.load().unwrap()), two);
    let path = dir.path().join("log");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.extend_from_slice(&[0, 0, 0, 200, 1, 2, 3]);
    std::fs::write(&path, &bytes).unwrap();
    let mut f = Files::new(dir.path()).unwrap();
    assert_eq!(logged(f.load().unwrap()), two, "the torn record is not read");
    persist(&mut f, 3..4);
    assert_eq!(
        logged(Files::new(dir.path()).unwrap().load().unwrap()),
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
        d.record(&Record::Rows(rows.clone())).unwrap();
        persist(d, 3..5);
        let mut want = vec![Record::Rows(rows)];
        want.extend((3..5).map(row).map(Record::Logged));
        assert_eq!(d.load().unwrap(), want);
        d.put("blob", b"\x00\x01").unwrap();
        assert_eq!(d.get("blob").unwrap().as_deref(), Some(&b"\x00\x01"[..]));
        assert_eq!(d.get("nothing").unwrap(), None);
    }
}
