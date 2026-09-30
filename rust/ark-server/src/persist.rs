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
//!   base: { seq: Int, hash: Bytes, rows: { table: [row…] } },
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
//! A data directory written before the journal existed — `log.ark-log`
//! alone — is a snapshot with nothing after it, and opens unchanged.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use ark::canon;
use ark::log::{snapshot_of, Entry, Facts, Log, Seq};
use ark::protocol::{change_from_value, change_value, entry_from_value, entry_value};
use ark::schema::Schema;
use ark::store::{MemoryStore, Store};
use ark::value::{FieldName, Value};

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

/// One entry as the log file and the journal both carry it.
pub fn record_value(n: Seq, e: &Entry, facts: &Facts) -> Value {
    Value::record(vec![
        ("seq", Value::int(n)),
        ("entry", entry_value(e)),
        ("facts", Value::list(facts.iter().map(change_value).collect())),
    ])
}

/// A journal record's bytes: the length, then the record.
pub fn encode_record(n: Seq, e: &Entry, facts: &Facts) -> Vec<u8> {
    let body = canon::encode(&record_value(n, e, facts));
    let mut out = Vec::with_capacity(body.len() + 4);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(&body);
    out
}

/// A log as a value; its file form is `canon::encode` of this.
pub fn log_to_value(log: &Log) -> Value {
    let store = &log.base.store;
    let rows: Vec<(String, Value)> = store
        .table_names()
        .into_iter()
        .map(|t| {
            let rs = store.scan(&t).into_iter().map(Value::Struct).collect();
            (t, Value::list(rs))
        })
        .collect();
    let entries: Vec<Value> = log.entries.iter().map(|(n, (e, f))| record_value(*n, e, f)).collect();
    let ids: Vec<Value> = log
        .ids
        .iter()
        .map(|(id, n)| Value::record(vec![("id", Value::Id(*id)), ("seq", Value::int(*n))]))
        .collect();
    Value::record(vec![
        ("t", Value::text("log")),
        (
            "base",
            Value::record(vec![
                ("seq", Value::int(log.base.seq)),
                ("hash", Value::bytes(log.base.hash.clone())),
                ("rows", Value::record(rows)),
            ]),
        ),
        ("entries", Value::list(entries)),
        ("ids", Value::list(ids)),
    ])
}

fn fields(v: &Value) -> Result<&BTreeMap<FieldName, Value>> {
    match v {
        Value::Struct(m) => Ok(m),
        other => bail!("expected a struct, found {other:?}"),
    }
}

fn need<'a>(m: &'a BTreeMap<FieldName, Value>, k: &str) -> Result<&'a Value> {
    m.get(k).with_context(|| format!("missing field {k}"))
}

fn int(v: &Value) -> Result<i64> {
    match v {
        Value::Int(n) => Ok(*n),
        other => bail!("expected an int, found {other:?}"),
    }
}

fn items(v: &Value) -> Result<&[Value]> {
    match v {
        Value::List(xs) => Ok(xs),
        other => bail!("expected a list, found {other:?}"),
    }
}

/// One entry back from its value.
pub fn record_from_value(v: &Value) -> Result<(Seq, Entry, Facts)> {
    let im = fields(v)?;
    let n = int(need(im, "seq")?)?;
    let e = entry_from_value(need(im, "entry")?).with_context(|| format!("entry {n}"))?;
    let facts = items(need(im, "facts")?)?
        .iter()
        .map(change_from_value)
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("facts of {n}"))?;
    Ok((n, e, facts))
}

/// A log from its value, over the module's schema.
pub fn log_from_value(schema: &Schema, v: &Value) -> Result<Log> {
    let m = fields(v)?;
    match need(m, "t")? {
        Value::Text(t) if t == "log" => {}
        other => bail!("not a log file: t = {other:?}"),
    }
    let base = fields(need(m, "base")?)?;
    let store = MemoryStore::from_value(schema.clone(), need(base, "rows")?);
    let snapshot = snapshot_of(int(need(base, "seq")?)?, store);
    match need(base, "hash")? {
        Value::Bytes(h) if *h == snapshot.hash => {}
        _ => bail!("the snapshot's hash does not match its rows"),
    }
    let mut log = Log {
        base: snapshot,
        entries: BTreeMap::new(),
        ids: BTreeMap::new(),
    };
    for item in items(need(m, "entries")?)? {
        let (n, e, facts) = record_from_value(item)?;
        log.entries.insert(n, (e, facts));
    }
    for item in items(need(m, "ids")?)? {
        let im = fields(item)?;
        let id = match need(im, "id")? {
            Value::Id(i) => *i,
            other => bail!("expected an id, found {other:?}"),
        };
        log.ids.insert(id, int(need(im, "seq")?)?);
    }
    if !log.contiguous() {
        bail!("the entries do not run without a gap from {} to {}", log.horizon() + 1, log.head_seq());
    }
    Ok(log)
}

/// Read a file, or `None` where there is none.
fn read_opt(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// The snapshot, if there is one.
fn read_snapshot(dir: &Path, schema: &Schema) -> Result<Option<(Log, usize)>> {
    let path = path_of(dir);
    let Some(bytes) = read_opt(&path)? else { return Ok(None) };
    let v = canon::decode(&bytes).with_context(|| format!("decoding {}", path.display()))?;
    let log = log_from_value(schema, &v).with_context(|| format!("reading {}", path.display()))?;
    Ok(Some((log, bytes.len())))
}

/// What reading a journal over a log found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Replayed {
    /// Records appended to the log.
    pub applied: usize,
    /// Records at or below the snapshot's head, ahead of the rest: what a
    /// compaction that stopped before emptying the journal left.
    pub stale: usize,
    /// Where the good records end: the file's length, or the offset of the
    /// first record that is short, does not decode, or does not follow on.
    pub end: u64,
    /// The file's length as read.
    pub len: u64,
}

impl Replayed {
    /// Whether everything after `end` is a torn tail.
    pub fn torn(&self) -> bool {
        self.end < self.len
    }
}

/// Append a journal's records to `log`, in order, stopping at the first
/// that is short, does not decode, or does not carry the next sequence
/// (the module docs).
pub fn replay_journal(log: &mut Log, bytes: &[u8]) -> Replayed {
    let mut out = Replayed {
        len: bytes.len() as u64,
        ..Replayed::default()
    };
    let mut at = 0usize;
    while at < bytes.len() {
        let Some(head) = bytes.get(at..at + 4) else { break };
        let n = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
        let Some(body) = bytes.get(at + 4..at + 4 + n) else { break };
        let Ok((seq, e, facts)) = canon::decode(body).map_err(anyhow::Error::from).and_then(|v| record_from_value(&v)) else {
            break;
        };
        if seq == log.head_seq() + 1 {
            log.append(e, facts);
            out.applied += 1;
        } else if seq <= log.head_seq() && out.applied == 0 {
            out.stale += 1;
        } else {
            break;
        }
        at += 4 + n;
    }
    out.end = at as u64;
    out
}

/// Read the log back, if there is one: the snapshot, then the journal (the
/// module docs). Reads, never writes — a test, or a tool beside a running
/// server, sees a whole prefix of what the server has written.
pub fn load(dir: &Path, schema: &Schema) -> Result<Option<Log>> {
    let snapshot = read_snapshot(dir, schema)?;
    let journal = read_opt(&journal_path_of(dir))?;
    if snapshot.is_none() && journal.as_ref().is_none_or(|j| j.is_empty()) {
        return Ok(None);
    }
    let mut log = snapshot.map_or_else(|| Log::empty(schema.clone()), |(l, _)| l);
    if let Some(bytes) = journal {
        replay_journal(&mut log, &bytes);
    }
    Ok(Some(log))
}

/// Write `bytes` as `dir/name`: to a temporary name, synced, renamed over
/// the old one, and the directory synced — so a stop leaves the old file
/// or the new one, never part of either.
fn write_whole(dir: &Path, name: &str, bytes: &[u8]) -> Result<()> {
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
    match fs::remove_file(journal_path_of(dir)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e).with_context(|| format!("removing {}", journal_path_of(dir).display())),
        _ => Ok(()),
    }
}

/// A data directory's log, open for writing: what the disk holds, so that
/// [`LogFile::write`] appends what moved since and nothing else.
#[derive(Debug)]
pub struct LogFile {
    dir: PathBuf,
    /// Open for appending once there is anything to append.
    journal: Option<File>,
    snapshot_bytes: u64,
    journal_bytes: u64,
    /// The head and horizon of the log as last written.
    head: Seq,
    horizon: Seq,
    /// The journal has bytes in it that are not whole records — an append
    /// failed part-way — so the next write is a snapshot, which empties it.
    due: bool,
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
        let snapshot = read_snapshot(dir, schema)?;
        let jpath = journal_path_of(dir);
        let journal = read_opt(&jpath)?;
        let snapshot_bytes = snapshot.as_ref().map_or(0, |(_, n)| *n as u64);
        let mut log = snapshot.as_ref().map(|(l, _)| l.clone());
        let mut replayed = Replayed::default();
        if let Some(bytes) = &journal {
            let l = log.get_or_insert_with(|| Log::empty(schema.clone()));
            replayed = replay_journal(l, bytes);
        }
        let mut file = LogFile {
            dir: dir.to_path_buf(),
            journal: None,
            snapshot_bytes,
            journal_bytes: replayed.end,
            head: log.as_ref().map_or(0, Log::head_seq),
            horizon: log.as_ref().map_or(0, Log::horizon),
            due: false,
            written: 0,
        };
        if journal.is_some() {
            let f = OpenOptions::new()
                .append(true)
                .open(&jpath)
                .with_context(|| format!("opening {}", jpath.display()))?;
            if replayed.torn() {
                eprintln!(
                    "ark-server: {} ends in {} bytes that are not a whole record; dropped",
                    jpath.display(),
                    replayed.len - replayed.end
                );
                f.set_len(replayed.end).with_context(|| format!("truncating {}", jpath.display()))?;
                f.sync_all().with_context(|| format!("syncing {}", jpath.display()))?;
            }
            file.journal = Some(f);
        }
        if replayed.stale > 0 {
            if let Some(l) = &log {
                file.snapshot(l)?;
            }
        }
        if snapshot.is_none() && replayed.applied == 0 {
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
    pub fn write(&mut self, log: &Log) -> Result<()> {
        if self.due || log.horizon() != self.horizon || log.head_seq() < self.head {
            return self.snapshot(log);
        }
        if log.head_seq() == self.head {
            return Ok(());
        }
        let mut buf = vec![];
        for (n, (e, f)) in log.entries.range(self.head + 1..) {
            buf.extend_from_slice(&encode_record(*n, e, f));
        }
        if let Err(e) = self.append(&buf) {
            // Some of it may be on the disk and not whole.
            self.due = true;
            return Err(e);
        }
        self.written += buf.len() as u64;
        self.journal_bytes += buf.len() as u64;
        self.head = log.head_seq();
        if self.journal_bytes > self.snapshot_bytes {
            if let Err(e) = self.snapshot(log) {
                eprintln!("ark-server: could not compact the log: {e:#}");
            }
        }
        Ok(())
    }

    fn append(&mut self, buf: &[u8]) -> Result<()> {
        let path = journal_path_of(&self.dir);
        if self.journal.is_none() {
            let f = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .with_context(|| format!("opening {}", path.display()))?;
            // The file's name is durable only once its directory is.
            sync_dir(&self.dir).with_context(|| format!("syncing {}", self.dir.display()))?;
            self.journal = Some(f);
        }
        let f = self.journal.as_mut().expect("opened above");
        f.write_all(buf).with_context(|| format!("appending to {}", path.display()))?;
        f.sync_data().with_context(|| format!("syncing {}", path.display()))
    }

    /// Compact: the log whole as the snapshot, renamed into place, then
    /// the journal emptied — so a stop between leaves records the snapshot
    /// already holds, which a reader skips.
    pub fn snapshot(&mut self, log: &Log) -> Result<()> {
        let bytes = canon::encode(&log_to_value(log));
        write_whole(&self.dir, FILE, &bytes)?;
        self.written += bytes.len() as u64;
        self.snapshot_bytes = bytes.len() as u64;
        self.head = log.head_seq();
        self.horizon = log.horizon();
        if let Some(f) = &self.journal {
            let path = journal_path_of(&self.dir);
            f.set_len(0).with_context(|| format!("emptying {}", path.display()))?;
            f.sync_all().with_context(|| format!("syncing {}", path.display()))?;
        }
        self.journal_bytes = 0;
        self.due = false;
        Ok(())
    }

    /// The snapshot's size and the journal's, as last written.
    pub fn sizes(&self) -> (u64, u64) {
        (self.snapshot_bytes, self.journal_bytes)
    }

    /// The head as last written.
    pub fn head(&self) -> Seq {
        self.head
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
}
