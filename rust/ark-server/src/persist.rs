//! The log on disk, as a snapshot and a journal (`docs/plan-perf.md` §R3):
//!
//! ```text
//! DATA/log.ark-log      the snapshot: canonical CBOR of `log_to_value` —
//!                       written to `.log.ark-log.tmp`, synced, renamed,
//!                       and the directory synced
//! DATA/log.ark-journal  append-only, one record per entry appended since:
//!                       a 4-byte big-endian length, then the canonical
//!                       CBOR of { seq, entry, facts }; synced once a batch
//! ```
//!
//! The snapshot's shape reuses the protocol's own encoders for an entry and
//! a change, so a log file carries exactly what a `Batch` frame would, and a
//! journal record is one item of its `entries`:
//!
//! ```text
//! { t: "log",
//!   base: { seq: Int, hash: Bytes, rows: { table: [row…] }, log: Id },
//!   entries: [ { seq: Int, entry: Entry, facts: [Change…] } … ],
//!   ids: [ { id: Id, seq: Int } … ] }
//! ```
//!
//! **Why two files.** The log used to be rewritten whole after every batch
//! of appends — 12.8 ms a push at two thousand entries, and growing with
//! every one (§R3) — and in place, so a kill mid-write was a torn file the
//! server then refused to start on. Now what a push costs is its own
//! records: [`LogFile::write`] appends the entries since the last write and
//! syncs the journal once, however many there were.
//!
//! **Compaction** rewrites the snapshot at the head and empties the journal
//! once the journal is larger than the snapshot — the client's rule
//! (`ark_client::storage`): a snapshot of *S* bytes is written only after
//! at least *S* bytes of journal, so the bytes written per append stay
//! within a small multiple of the record's whatever the log's length. A
//! log whose horizon moved (`Log::compact_to`) is written as a snapshot:
//! a journal only ever extends the snapshot it follows.
//!
//! **What a stop leaves behind is safe, by the order of the writes.** A
//! record is synced before the hub says anything about the entry in it
//! (`hub.rs`), so a torn tail holds only entries nobody was told about. A
//! compaction renames the new snapshot into place before it empties the
//! journal, so a stop between leaves records the snapshot already holds.
//! [`load`] therefore reads the snapshot, then the journal: records at or
//! below the snapshot's head, before any that extend it, are ones a
//! compaction already holds and are skipped; the first record that is
//! short, does not decode, or does not carry the next sequence is where
//! the journal ends. [`LogFile::open`] — the server starting — truncates
//! the file there and, if it skipped anything, compacts, so what it appends
//! next follows on from what it read.
//!
//! **The logic is `ark::journal`'s** (`docs/plan-alone.md` §2), in its
//! server layout (`Layout::server`): one page appended to and a snapshot
//! compaction. A peer alone keeps its local history with the same records
//! in the other layout, through its own storage. What is here is the
//! directory — files written whole by a rename, the one page appended to
//! and synced — and the names and messages this server has always had.
//!
//! A data directory written before the journal existed — `log.ark-log`
//! alone — is a snapshot with nothing after it, and opens unchanged.
//!
//! **The log's identity is on the snapshot** (`base.log`,
//! `docs/plan-perf.md` Round 4): drawn when the log is created — by the
//! hub, since this crate's core has no randomness — and kept through every
//! compaction and restart, because a peer's `Hello` names it and a server
//! whose log has another name answers with its snapshot (§12.4). A journal
//! record does not carry it: a journal only ever extends the snapshot it
//! follows. A snapshot written before logs had names has no `log`, loads
//! unnamed, and the hub names it; the next write is then a snapshot, so
//! the name is on the disk before a restart could draw another, and —
//! since the hub writes before it delivers (`hub.rs`) — before any peer is
//! told it. A directory with no snapshot at all, new or a journal alone,
//! writes its name with its first append, which compacts at once because
//! any journal outgrows no snapshot; it is not written sooner, so opening
//! one writes nothing. Two gaps are left, both costing a re-base and never
//! a wrong state: a new log's name told in a snapshot at 0 before anything
//! was appended, and a compaction that fails after its append (said, not
//! returned). A server restarted in either draws another name, and its
//! peers are sent its snapshot once.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use ark::canon;
use ark::journal::{self, Journal, Keys, Layout};
use ark::log::{Entry, Facts, Log, Seq};
use ark::schema::Schema;
use ark::value::Value;

pub use ark::journal::{encode_record, log_to_value, record_value, Replayed};

/// The snapshot's file.
pub fn path_of(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

/// The journal's file.
pub fn journal_path_of(dir: &Path) -> PathBuf {
    dir.join(JOURNAL)
}

/// The snapshot's name.
pub const FILE: &str = "log.ark-log";

/// The journal's name.
pub const JOURNAL: &str = "log.ark-journal";

/// One entry back from its value.
pub fn record_from_value(v: &Value) -> Result<(Seq, Entry, Facts)> {
    journal::record_from_value(v).map_err(|e| anyhow!(e))
}

/// A log from its value, over the module's schema.
pub fn log_from_value(schema: &Schema, v: &Value) -> Result<Log> {
    journal::log_from_value(schema, v).map_err(|e| anyhow!(e))
}

/// Append a journal's records to `log`, in order, stopping at the first
/// that is short, does not decode, or does not carry the next sequence
/// (the module docs).
pub fn replay_journal(log: &mut Log, bytes: &[u8]) -> Replayed {
    journal::replay_into(log, bytes)
}

/// A data directory as `ark::journal` keeps a log in it: a record is a file,
/// saved whole by a rename, and the journal is appended to through a handle
/// kept open once it exists.
#[derive(Debug)]
struct Files {
    dir: PathBuf,
    open: HashMap<String, File>,
}

impl Files {
    fn new(dir: &Path) -> Files {
        Files {
            dir: dir.to_path_buf(),
            open: HashMap::new(),
        }
    }

    fn handle(&mut self, key: &str, create: bool) -> std::io::Result<&mut File> {
        if !self.open.contains_key(key) {
            let path = self.dir.join(key);
            let existed = path.exists();
            let f = OpenOptions::new().create(create).append(true).open(&path)?;
            if !existed {
                // The file's name is durable only once its directory is.
                sync_dir(&self.dir)?;
            }
            self.open.insert(key.to_string(), f);
        }
        Ok(self.open.get_mut(key).expect("inserted above"))
    }
}

fn say(e: impl std::fmt::Display, what: &str, path: &Path) -> String {
    format!("{what} {}: {e}", path.display())
}

impl Keys for Files {
    fn load(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        let path = self.dir.join(key);
        match fs::read(&path) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(say(e, "reading", &path)),
        }
    }

    fn save(&mut self, key: &str, bytes: &[u8]) -> Result<(), String> {
        write_whole(&self.dir, key, bytes).map_err(|e| format!("{e:#}"))
    }

    fn remove(&mut self, key: &str) -> Result<(), String> {
        self.open.remove(key);
        let path = self.dir.join(key);
        match fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(say(e, "removing", &path)),
            _ => Ok(()),
        }
    }

    fn append(&mut self, key: &str, bytes: &[u8]) -> Result<(), String> {
        let path = self.dir.join(key);
        let f = self.handle(key, true).map_err(|e| say(e, "opening", &path))?;
        f.write_all(bytes).map_err(|e| say(e, "appending to", &path))?;
        f.sync_data().map_err(|e| say(e, "syncing", &path))
    }

    fn truncate(&mut self, key: &str, len: usize) -> Result<(), String> {
        let path = self.dir.join(key);
        let f = self.handle(key, false).map_err(|e| say(e, "opening", &path))?;
        f.set_len(len as u64).map_err(|e| say(e, "truncating", &path))?;
        f.sync_all().map_err(|e| say(e, "syncing", &path))
    }
}

/// Read the log back, if there is one: the snapshot, then the journal (the
/// module docs). Reads, never writes — a test, or a tool beside a running
/// server, sees a whole prefix of what the server has written.
pub fn load(dir: &Path, schema: &Schema) -> Result<Option<Log>> {
    journal::load(&Files::new(dir), &Layout::server(), schema).map_err(|e| anyhow!(e))
}

/// Write `bytes` as `dir/name`: to a temporary name, synced, renamed over
/// the old one, and the directory synced — so a stop leaves the old file
/// or the new one, never part of either.
pub(crate) fn write_whole(dir: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let tmp = dir.join(format!(".{name}.tmp"));
    {
        let mut f = File::create(&tmp).with_context(|| format!("writing {}", tmp.display()))?;
        f.write_all(bytes).with_context(|| format!("writing {}", tmp.display()))?;
        f.sync_all().with_context(|| format!("syncing {}", tmp.display()))?;
    }
    fs::rename(&tmp, dir.join(name)).with_context(|| format!("moving {} into place", tmp.display()))?;
    sync_dir(dir).with_context(|| format!("syncing {}", dir.display()))
}

#[cfg(unix)]
fn sync_dir(dir: &Path) -> std::io::Result<()> {
    File::open(dir)?.sync_all()
}
#[cfg(not(unix))]
fn sync_dir(_: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Write a log whole, as a snapshot with no journal after it: what a tool
/// seeding a data directory does. The snapshot goes into place first, then
/// the journal is removed — a journal left beside a snapshot it did not
/// follow would be read on top of it.
pub fn save(dir: &Path, log: &Log) -> Result<()> {
    write_whole(dir, FILE, &canon::encode(&log_to_value(log)))?;
    Files::new(dir).remove(JOURNAL).map_err(|e| anyhow!(e))
}

/// Every row of `st` laid out as its table's under `st`'s schema where it
/// names only columns the table has — a nullable column it lacks `Null` —
/// and any other row as it is (`docs/plan-db.md` D1). A log written under
/// an older module holds rows without a column the current one added;
/// a peer of the current module that runs one of those entries again
/// writes the column `Null`, so the server must hold it so too, or the two
/// hash apart over rows that say the same thing.
pub fn widen(st: &ark::store::MemoryStore) -> ark::store::MemoryStore {
    use ark::store::{project_row, Change, MemoryStore, Store};
    let schema = st.schema().clone();
    let mut out = MemoryStore::empty(schema.clone());
    for tbl in schema.tables() {
        for r in st.scan(&tbl.name) {
            let fits = r.keys().all(|k| tbl.column(k).is_some());
            let r = if fits { project_row(tbl, &r).unwrap_or(r) } else { r };
            out.apply_change(&Change::Add(tbl.name.clone(), r));
        }
    }
    out
}

/// The snapshot rewritten under `schema` (`docs/plan-db.md` D1): read
/// without its hash checked, every row widened ([`widen`]), hashed again,
/// and renamed into place. For the first start with a new module only,
/// whose schema may have grown — a table more is a pair more in the state
/// hash — or whose state hash was defined otherwise when the file was
/// written (`docs/plan-db.md` D3): the file is this server's own, synced
/// and renamed into place by it, so what the hash would catch, a torn or
/// foreign file, is not what a new module brings. Every other start checks
/// the hash as it always has. The journal after the snapshot is left as
/// it is: its records extend the same head.
pub fn rehome(dir: &Path, schema: &Schema) -> Result<bool> {
    use ark::store::MemoryStore;
    let path = path_of(dir);
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let v = canon::decode(&bytes).map_err(|e| anyhow!("{}: {e}", path.display()))?;
    let field = |v: &Value, k: &str| -> Result<Value> {
        match v {
            Value::Struct(m) => m.get(k).cloned().ok_or_else(|| anyhow!("{}: no {k}", path.display())),
            _ => Err(anyhow!("{}: not a struct", path.display())),
        }
    };
    let list = |v: Value| -> Result<Vec<Value>> {
        match v {
            Value::List(xs) => Ok(xs),
            _ => Err(anyhow!("{}: not a list", path.display())),
        }
    };
    let base = field(&v, "base")?;
    let Value::Int(seq) = field(&base, "seq")? else {
        return Err(anyhow!("{}: the snapshot's seq is not an int", path.display()));
    };
    let log_id = match field(&base, "log") {
        Ok(Value::Id(i)) => Some(i),
        _ => None,
    };
    let store = widen(&MemoryStore::from_value(schema.clone(), &field(&base, "rows")?));
    let mut log = Log {
        base: ark::log::snapshot_of(seq, store).of_log(log_id),
        entries: Default::default(),
        ids: Default::default(),
        below: Default::default(),
    };
    for item in list(field(&v, "entries")?)? {
        let (n, e, f) = journal::record_from_value(&item).map_err(|e| anyhow!("{}: {e}", path.display()))?;
        log.entries.insert(n, (e, f));
    }
    // An id below the horizon is kept as its key (`ark::log::Below`,
    // `docs/plan-db.md` D6), and must be carried as one: dropping it would
    // let a re-push of that intent be applied again.
    let mut keys = vec![];
    for item in list(field(&v, "ids")?)? {
        match (field(&item, "id"), field(&item, "key"), field(&item, "seq")) {
            (Ok(Value::Id(id)), _, Ok(Value::Int(n))) => {
                log.ids.insert(id, n);
            }
            (_, Ok(Value::Bytes(k)), Ok(Value::Int(n))) if k.len() == 8 => {
                keys.push((u64::from_be_bytes(k[..].try_into().expect("eight bytes")), n));
            }
            _ => {}
        }
    }
    log.below.extend(keys);
    write_whole(dir, FILE, &canon::encode(&journal::log_to_value(&log)))?;
    Ok(true)
}

/// A data directory's log, open for writing: what the disk holds, so that
/// [`LogFile::write`] appends what moved since and nothing else.
#[derive(Debug)]
pub struct LogFile {
    files: Files,
    journal: Journal,
    /// Every byte written, snapshots and journal both: what a test counts.
    pub written: u64,
}

impl LogFile {
    /// Open `dir`'s log: the snapshot and the journal after it, as
    /// [`load`] reads them — then the journal truncated where its good
    /// records end, and compacted if anything was skipped, so that what is
    /// appended next follows on. The log, or `None` where the directory
    /// holds none; nothing is written for a directory whose files are
    /// whole.
    pub fn open(dir: &Path, schema: &Schema) -> Result<(LogFile, Option<Log>)> {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let mut files = Files::new(dir);
        let mut got = vec![];
        let (journal, opened) = Journal::open(&mut files, Layout::server(), schema, |_, e, f| got.push((e, f))).map_err(|e| anyhow!(e))?;
        if let Some((_, dropped)) = opened.torn {
            eprintln!(
                "ark-server: {} ends in {dropped} bytes that are not a whole record; dropped",
                journal_path_of(dir).display()
            );
        }
        let applied = !got.is_empty();
        let mut log = opened.snapshot.clone();
        if applied {
            let l = log.get_or_insert_with(|| Log::empty(schema.clone()));
            for (e, f) in got {
                l.append(e, f);
            }
        }
        let mut file = LogFile { files, journal, written: 0 };
        if opened.stale > 0 {
            if let Some(l) = &log {
                file.snapshot(l)?;
            }
        }
        if opened.snapshot.is_none() && !applied {
            log = None;
        }
        Ok((file, log))
    }

    /// Write what moved since the last write: the entries appended, as
    /// records at the end of the journal, synced once for all of them; or
    /// a snapshot, where the horizon moved or the journal cannot be
    /// written on. `Ok` means every entry of `log` is on the disk. A
    /// compaction that follows a good append and fails is said, not
    /// returned: the entries are durable either way.
    ///
    /// A log whose name is not the one on its snapshot — a directory
    /// written before logs had names, named since it was opened — is
    /// written as a snapshot, whether or not it moved, so the name is on
    /// the disk from the first write (the module docs). Where there is no
    /// snapshot yet, the first append is what writes one.
    pub fn write(&mut self, log: &Log) -> Result<()> {
        let out = self.journal.write(&mut self.files, log);
        self.written = self.journal.written;
        match out {
            Ok(None) => Ok(()),
            Ok(Some(e)) => {
                eprintln!("ark-server: could not compact the log: {e}");
                Ok(())
            }
            Err(e) => Err(anyhow!(e)),
        }
    }

    /// Compact: the log whole as the snapshot, renamed into place, then
    /// the journal emptied — so a stop between leaves records the snapshot
    /// already holds, which a reader skips.
    pub fn snapshot(&mut self, log: &Log) -> Result<()> {
        let out = self.journal.snapshot(&mut self.files, log);
        self.written = self.journal.written;
        out.map_err(|e| anyhow!(e))
    }

    /// The snapshot's size and the journal's, as last written.
    pub fn sizes(&self) -> (u64, u64) {
        self.journal.sizes()
    }

    /// The head as last written.
    pub fn head(&self) -> Seq {
        self.journal.head()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark::eval::Ctx;
    use ark::peer::{Authority, Sequenced};
    use ark_client::{demo, Domain};

    fn author(a: &mut Authority, d: &Domain, id: [u8; 16], name: &str, ctx: &Ctx) {
        let (fh, _) = d.mutator("create_playlist").unwrap();
        let e = ark::log::Entry {
            id,
            actor: ctx.user.clone(),
            session: ctx.session.clone(),
            fn_hash: fh.clone(),
            args: [("name".to_string(), Value::text(name))].into(),
            autos: [("id".to_string(), Value::Id(id))].into(),
        };
        assert!(matches!(a.sequence_entry(&e), Sequenced::Appended(..)), "{name}");
    }

    #[test]
    fn a_log_survives_the_file_and_a_damaged_one_is_refused() {
        let d = demo::domain();
        let schema = d.module().schema.clone();
        let mut a = Authority::new(schema.clone(), d.closures().clone());
        a.hold(d.native_list());
        let ctx = Ctx::new("alice", "dev");
        author(&mut a, &d, [1; 16], "Road trip", &ctx);
        author(&mut a, &d, [2; 16], "Focus", &ctx);
        // Compact once so the base carries rows, then append above it.
        assert!(a.compact(1));
        author(&mut a, &d, [3; 16], "Sleep", &ctx);
        assert_eq!(a.log.head_seq(), 3);

        let dir = tempfile::tempdir().unwrap();
        let empty = tempfile::tempdir().unwrap();
        assert!(load(empty.path(), &schema).unwrap().is_none());
        save(dir.path(), &a.log).unwrap();
        let back = load(dir.path(), &schema).unwrap().expect("a file was written");
        assert_eq!(back, a.log);

        // A row changed under the snapshot's hash is a file that is not served.
        let mut v = log_to_value(&a.log);
        if let Value::Struct(m) = &mut v {
            if let Some(Value::Struct(base)) = m.get_mut("base") {
                base.insert("rows".into(), Value::record::<String>(vec![]));
            }
        }
        let err = log_from_value(&schema, &v).unwrap_err();
        assert!(err.to_string().contains("hash"), "{err}");
    }

    /// Round 4: a log's name is on its snapshot and survives what the disk
    /// does to a log — a save and a load, appends, a compaction, the
    /// horizon moving, a reopen. Falsified by `log_to_value` leaving the
    /// name out: the reopened log is unnamed.
    #[test]
    fn a_log_keeps_its_name_on_the_disk() {
        let d = demo::domain();
        let schema = d.module().schema.clone();
        let mut a = Authority::new(schema.clone(), d.closures().clone());
        a.hold(d.native_list());
        a.log.name_if_unnamed([5; 16]);
        let ctx = Ctx::new("alice", "dev");
        let dir = tempfile::tempdir().unwrap();
        let (mut f, none) = LogFile::open(dir.path(), &schema).unwrap();
        assert!(none.is_none());
        for i in 0..40u8 {
            author(&mut a, &d, [i + 1; 16], &format!("p{i}"), &ctx);
            f.write(&a.log).unwrap();
            let back = load(dir.path(), &schema).unwrap().unwrap();
            assert_eq!(back.id(), Some([5; 16]), "after {i}");
        }
        assert!(a.compact(30));
        f.write(&a.log).unwrap();
        let (_, back) = LogFile::open(dir.path(), &schema).unwrap();
        assert_eq!(back.as_ref(), Some(&a.log));
        assert_eq!(back.unwrap().id(), Some([5; 16]));
    }

    /// Round 4: a snapshot written before logs had names loads unnamed and
    /// opens writing nothing; named after it is opened, as the hub names
    /// it, the next write is a snapshot with the name on it — not an
    /// append the old snapshot would stand under, unnamed, until a
    /// compaction — and the name is what a reopen reads. Falsified by
    /// `write` not treating a renamed log as due a snapshot: the reopened
    /// log is unnamed.
    #[test]
    fn an_unnamed_log_is_named_at_its_next_write() {
        let d = demo::domain();
        let schema = d.module().schema.clone();
        let mut a = Authority::new(schema.clone(), d.closures().clone());
        a.hold(d.native_list());
        let ctx = Ctx::new("alice", "dev");
        for i in 0..5u8 {
            author(&mut a, &d, [i + 1; 16], &format!("p{i}"), &ctx);
        }
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &a.log).unwrap();
        let (mut f, log) = LogFile::open(dir.path(), &schema).unwrap();
        let mut log = log.unwrap();
        assert_eq!((log.id(), f.written), (None, 0));
        log.name_if_unnamed([6; 16]);
        a.log = log;
        author(&mut a, &d, [9; 16], "p9", &ctx);
        f.write(&a.log).unwrap();
        assert_eq!(f.sizes().1, 0, "written as a snapshot");
        let (_, back) = LogFile::open(dir.path(), &schema).unwrap();
        assert_eq!(back.unwrap().id(), Some([6; 16]));
    }
}
