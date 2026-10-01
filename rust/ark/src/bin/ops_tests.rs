//! `ops.rs`'s tests, in a file of their own so that a test elsewhere that
//! includes `ops.rs` by path — the process fleet, which drives a real
//! server — does not run them a second time.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use ark::eval::Args;
use ark::journal::{self, Journal, Keys, Layout};
use ark::log::{Entry, Log};
use ark::schema::{Column, Schema, Table, Ty};
use ark::store::Change;
use ark::value::Value;

use crate::ops::*;

/// A directory of its own under the system's temporary one, removed
/// when dropped: this crate has no `tempfile`.
struct Temp(PathBuf);

impl Temp {
    fn new() -> Temp {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("arkc-ops-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        Temp(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
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

/// A data directory written the way `ark-server`'s `persist` writes
/// one: the snapshot by a rename, the journal appended to and cut.
struct Files(PathBuf);

impl Keys for Files {
    fn load(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        ReadDir(self.0.clone()).load(key)
    }
    fn save(&mut self, key: &str, bytes: &[u8]) -> Result<(), String> {
        write_file(&self.0, key, bytes)
    }
    fn remove(&mut self, key: &str) -> Result<(), String> {
        let _ = fs::remove_file(self.0.join(key));
        Ok(())
    }
    fn append(&mut self, key: &str, bytes: &[u8]) -> Result<(), String> {
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.0.join(key))
            .map_err(|e| e.to_string())?;
        f.write_all(bytes).map_err(|e| e.to_string())
    }
    fn truncate(&mut self, key: &str, len: usize) -> Result<(), String> {
        let f = fs::OpenOptions::new().write(true).open(self.0.join(key)).map_err(|e| e.to_string())?;
        f.set_len(len as u64).map_err(|e| e.to_string())
    }
}

/// A server's log on a directory, appended to one entry at a time.
struct Writer {
    log: Log,
    journal: Journal,
    files: Files,
}

impl Writer {
    fn new(dir: &Path) -> Writer {
        let mut log = Log::empty(schema());
        log.name_if_unnamed([3; 16]);
        let mut files = Files(dir.to_path_buf());
        let journal = Journal::create(&mut files, Layout::server(), &log).unwrap();
        Writer { log, journal, files }
    }

    fn append(&mut self) {
        let n = self.log.head_seq() + 1;
        let mut id = [0u8; 16];
        id[8..].copy_from_slice(&n.to_be_bytes());
        let e = Entry {
            id,
            actor: "a".into(),
            session: "s".into(),
            fn_hash: vec![1],
            args: Args::new(),
            autos: Args::new(),
        };
        let row = [("id".to_string(), Value::int(n))].into_iter().collect();
        let facts = vec![Change::Add("t".into(), row)];
        let record = journal::encode_record(n, &e, &facts);
        self.log.append(e, facts);
        // Appended and never compacted on its own, so that a test says
        // when a compaction happens.
        self.journal.append(&mut self.files, &record, n).unwrap();
    }

    /// The horizon moved to the head: a snapshot renamed into place and
    /// the journal cut, as a compaction does.
    fn compact(&mut self) {
        self.log = self.log.compact_to(self.log.head_seq()).unwrap();
        self.journal.snapshot(&mut self.files, &self.log).unwrap();
    }
}

fn loaded(dir: &Path) -> Log {
    journal::load(&ReadDir(dir.to_path_buf()), &Layout::server(), &schema()).unwrap().unwrap()
}

/// D6: what a server appends while it is backed up is not in the copy,
/// and the copy is the log at the moment its snapshot had been read —
/// whole, with an append caught half-written cut away. The appends are
/// made between the two reads, so the length the journal had then is
/// what decides. Falsified by reading the journal to its end rather
/// than to the length it had: the copy holds the entries appended
/// meanwhile, and its head is 13 rather than 10.
#[test]
fn a_backup_is_the_log_at_one_moment() {
    let dir = Temp::new();
    let out = dir.path().join("out");
    let data = dir.path().join("data");
    fs::create_dir(&data).unwrap();
    let mut w = Writer::new(&data);
    for _ in 0..10 {
        w.append();
    }
    fs::write(data.join("cursors.cbor"), b"c").unwrap();
    fs::create_dir(data.join("library")).unwrap();
    let at = loaded(&data);
    let b = backup_with(&data, &out, || {
        for _ in 0..3 {
            w.append();
        }
        // And half of another record, as an append in flight is.
        let mut f = fs::OpenOptions::new().append(true).open(data.join("log.ark-journal")).unwrap();
        f.write_all(&[0, 0, 0, 40, 1, 2]).unwrap();
    })
    .unwrap();
    assert_eq!((b.head, b.reads), (10, 1));
    assert_eq!(b.files, vec!["cursors.cbor".to_string()]);
    assert_eq!(b.skipped, vec!["library".to_string()]);
    assert_eq!(loaded(&out), at, "the log as it stood");
    let v = verify_log(&out, None).unwrap();
    assert_eq!((v.head, v.torn, v.log_id), (10, 0, Some([3; 16])));
    assert_eq!(fs::read(out.join("cursors.cbor")).unwrap(), b"c");

    // Torn by the server itself before the backup: still cut.
    let torn = dir.path().join("torn");
    fs::create_dir(&torn).unwrap();
    for f in ["log.ark-log", "log.ark-journal"] {
        fs::copy(data.join(f), torn.join(f)).unwrap();
    }
    let b = backup(&torn, &dir.path().join("out2")).unwrap();
    assert_eq!(b.head, 13);
    assert_eq!(verify_log(&dir.path().join("out2"), None).unwrap().torn, 0);
}

/// D6: a compaction between the two reads — the snapshot renamed into
/// place, the journal cut — is seen, and the pair read again, so the
/// copy is not older than the log on the disk when the backup began.
/// Falsified by not comparing the snapshot's file before and after: the
/// old snapshot is copied with a journal that follows the new one, the
/// journal is cut to nothing, and the copy's head is 5 where the disk
/// held 8 when the backup started.
#[test]
fn a_compaction_mid_backup_is_read_again() {
    let dir = Temp::new();
    let data = dir.path().join("data");
    fs::create_dir(&data).unwrap();
    let mut w = Writer::new(&data);
    for _ in 0..5 {
        w.append();
    }
    w.compact();
    for _ in 0..3 {
        w.append();
    }
    assert_eq!(loaded(&data).head_seq(), 8);
    let mut once = true;
    let b = backup_with(&data, &dir.path().join("out"), || {
        if std::mem::take(&mut once) {
            w.compact();
            w.append();
        }
    })
    .unwrap();
    assert!(b.head >= 8, "the copy is at {} and the disk was at 8", b.head);
    assert_eq!(b.reads, 2);
    assert_eq!(loaded(&dir.path().join("out")).head_seq(), b.head);
}

/// D6: a backup taken while another thread appends and compacts as
/// fast as it can is, every time, a log that loads, is a prefix of the
/// writer's, and is at least as long as the disk's when it began. Not
/// falsified on its own — it is the race the two tests above pin down
/// deterministically; it is here to say the real interleaving agrees.
#[test]
fn backups_while_a_server_appends() {
    let dir = Temp::new();
    let data = dir.path().join("data");
    fs::create_dir(&data).unwrap();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let writer = {
        let (data, stop) = (data.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut w = Writer::new(&data);
            let mut n = 0u64;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) || n < 50 {
                w.append();
                n += 1;
                if n.is_multiple_of(37) {
                    w.compact();
                }
            }
            w.log
        })
    };
    let mut heads = vec![];
    for i in 0..40 {
        while !data.join("log.ark-log").exists() {
            std::thread::yield_now();
        }
        let start = loaded(&data).head_seq();
        let out = dir.path().join(format!("b{i}"));
        let b = backup(&data, &out).unwrap();
        assert!(b.head >= start, "backup {i} at {} began at {start}", b.head);
        let got = loaded(&out);
        assert_eq!(got.head_seq(), b.head);
        heads.push(got);
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let full = writer.join().unwrap();
    for got in heads {
        // A prefix: its state at its head is the writer's replayed to
        // there, as far as the writer still holds entries for it.
        if got.head_seq() >= full.horizon() {
            assert_eq!(got.state_at(got.head_seq()), full.state_at(got.head_seq()));
        }
        assert!(got.head_seq() <= full.head_seq());
    }
}

/// D6: a restore refuses a directory with anything in it, writes the
/// log unnamed (so the server names it afresh) unless told to keep the
/// name, and what it writes reads back as the backup's log. Falsified
/// by restoring the snapshot as it is: the name survives without
/// `--same-log`.
#[test]
fn a_restore_is_a_new_log_unless_told() {
    let dir = Temp::new();
    let data = dir.path().join("data");
    fs::create_dir(&data).unwrap();
    let mut w = Writer::new(&data);
    for _ in 0..4 {
        w.append();
    }
    w.compact();
    w.append();
    fs::write(data.join("sessions.json"), b"{}").unwrap();
    let out = dir.path().join("b");
    backup(&data, &out).unwrap();

    let full = dir.path().join("full");
    fs::create_dir(&full).unwrap();
    fs::write(full.join("x"), b"").unwrap();
    assert!(restore(&out, &full, false).unwrap_err().contains("not empty"));

    let fresh = dir.path().join("fresh");
    let r = restore(&out, &fresh, false).unwrap();
    assert_eq!((r.head, r.horizon, r.log_id, r.kept_name), (5, 4, Some([3; 16]), false));
    let back = loaded(&fresh);
    assert_eq!(back.id(), None, "unnamed");
    assert_eq!(back.state_at(5), w.log.state_at(5));
    assert_eq!(fs::read(fresh.join("sessions.json")).unwrap(), b"{}");

    let same = dir.path().join("same");
    restore(&out, &same, true).unwrap();
    assert_eq!(loaded(&same), w.log);
    let v = verify_log(&same, None).unwrap();
    assert_eq!((v.head, v.horizon, v.log_id, v.journal_records), (5, 4, Some([3; 16]), 1));
}
