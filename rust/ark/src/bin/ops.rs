//! `arkc backup`, `arkc restore` and `arkc verify-log`: a server's data
//! directory, copied while it runs, put back, and read (`docs/plan-db.md`
//! D6).
//!
//! A server's data directory (`ark-server`'s `persist`, `retain`,
//! `modules`, and `ark-auth`'s session store) is:
//!
//! ```text
//! log.ark-log      the snapshot, replaced whole by a rename
//! log.ark-journal  records appended after it, synced once a batch, and cut
//!                  to nothing once a compaction has renamed a new snapshot
//!                  into place
//! cursors.cbor     every session's place in the log and when it was heard
//! sessions.json    the sign-in sessions (ark-auth)
//! live.cbor        the rooms' kept snapshots
//! modules.cbor     every module the server has run, and its closures
//! ```
//!
//! **A backup is the log at one moment, then everything else.** The
//! snapshot is read, then the journal up to the length it had *when the
//! snapshot had been read* — so what a server appends while the copy is
//! being made is not in it, and the copy is the log as it stood at a
//! moment — then every other file. The journal is cut to its last whole
//! record that follows on from the snapshot (§10, `ark::journal`): an
//! append in flight when the length was taken is a torn tail the copy does
//! not carry. A compaction between the two reads is seen — the snapshot's
//! file is a different one afterwards (a rename gives it a new inode) —
//! and the pair is read again; without that the copy would still be a
//! consistent prefix (a journal of the next snapshot does not follow on
//! from the old one and is cut away), but an older one than the log on the
//! disk when the backup started, which is the property
//! `tests::a_compaction_mid_backup_is_read_again` holds. The files after
//! the log may be newer than it — a cursor ahead of the head is a session
//! the restored server serves a snapshot, which is the answer below or
//! past a head anyway (§12.4); a sign-in session is a sign-in session.
//! Directories (a scanner's replica, under `library/`) are a peer's, not
//! the server's, and are not copied: a peer of a restored server rebases
//! onto it like any other.
//!
//! **A restore is a new log**, unless told otherwise. The backup is a
//! prefix of a history some peers have seen more of; a peer whose cursor
//! is past the restored head is sent the snapshot (R6), but one that dials
//! only after the restored server has sequenced past its cursor would be
//! handed entries of the new history on top of a store holding the old one
//! — under the same log id nothing tells the two apart (Round 4, §12.4).
//! So `restore` writes the snapshot unnamed, and the server names it afresh
//! at start (`ark-server`'s hub; the `persist` module docs): every peer is
//! sent the snapshot once and rebases its pending onto it. `--same-log`
//! keeps the name, for a backup taken of a stopped server and moved
//! elsewhere, where nobody has seen past it.
//!
//! **verify-log reads, never writes**, and without the module it can say
//! everything but the state's hash: the snapshot and journal records are
//! read schema-free. With the module the snapshot's rows are checked
//! against its hash, the journal's facts are replayed to the head, and the
//! state hash there is printed — what every peer's `Verify` is compared
//! with.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use ark::canon;
use ark::ir::Module;
use ark::journal::{self, Keys, Layout};
use ark::log::Seq;
use ark::value::{hex, Id, Value};

/// The snapshot's and the journal's names, as the server lays them out.
fn names() -> (String, String) {
    let l = Layout::server();
    (l.snapshot, l.page)
}

/// D1's file of modules run (`ark-server`'s `modules`).
const MODULES: &str = "modules.cbor";

// -- reading a log without its schema -------------------------------------

/// What a snapshot says about the log, read without the schema.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Head {
    pub horizon: Seq,
    /// The last sequence the snapshot holds: its horizon, or its last entry.
    pub head: Seq,
    pub log_id: Option<Id>,
    /// The hash the snapshot carries, of the state at its horizon.
    pub hash: Vec<u8>,
    pub entries: usize,
    pub ids: usize,
}

fn field<'a>(m: &'a BTreeMap<String, Value>, k: &str) -> Result<&'a Value, String> {
    m.get(k).ok_or_else(|| format!("missing field {k}"))
}

fn record(v: &Value) -> Result<&BTreeMap<String, Value>, String> {
    match v {
        Value::Struct(m) => Ok(m),
        _ => Err("expected a record".into()),
    }
}

/// The snapshot's head, horizon, name and counts, from its bytes.
pub fn head_of(bytes: &[u8]) -> Result<Head, String> {
    let v = canon::decode(bytes).map_err(|e| format!("decoding the snapshot: {e}"))?;
    let m = record(&v)?;
    if field(m, "t")? != &Value::text("log") {
        return Err("not a log file".into());
    }
    let base = record(field(m, "base")?)?;
    let horizon = match field(base, "seq")? {
        Value::Int(n) => *n,
        _ => return Err("base.seq is not an int".into()),
    };
    let log_id = match base.get("log") {
        Some(Value::Id(i)) => Some(*i),
        _ => None,
    };
    let hash = match base.get("hash") {
        Some(Value::Bytes(b)) => b.to_vec(),
        _ => vec![],
    };
    let (entries, ids) = match (field(m, "entries")?, field(m, "ids")?) {
        (Value::List(e), Value::List(i)) => (e, i.len()),
        _ => return Err("entries or ids is not a list".into()),
    };
    let last = entries.last().map(|e| record(e).and_then(|r| field(r, "seq").cloned())).transpose()?;
    let head = match last {
        Some(Value::Int(n)) => n,
        None => horizon,
        Some(_) => return Err("an entry's seq is not an int".into()),
    };
    Ok(Head {
        horizon,
        head,
        log_id,
        hash,
        entries: entries.len(),
        ids,
    })
}

/// A journal's whole records that follow on from `head`: how many, where
/// they end, the head they reach, and how many were stale.
pub fn whole_records(head: Seq, journal: &[u8]) -> journal::Replayed {
    let mut h = head;
    journal::replay_page(&mut h, journal, |_, _, _| {})
}

// -- the directory, read only ---------------------------------------------

/// A data directory as `ark::journal` reads one: a file per key.
pub struct ReadDir(pub PathBuf);

impl Keys for ReadDir {
    fn load(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        match fs::read(self.0.join(key)) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("reading {}: {e}", self.0.join(key).display())),
        }
    }
    fn save(&mut self, _: &str, _: &[u8]) -> Result<(), String> {
        Err("read only".into())
    }
    fn remove(&mut self, _: &str) -> Result<(), String> {
        Err("read only".into())
    }
}

/// Write `bytes` as `dir/name` by a rename, synced; the directory is synced
/// by the caller once every file is in.
pub fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    let tmp = dir.join(format!(".{name}.tmp"));
    let say = |e: std::io::Error| format!("writing {}: {e}", dir.join(name).display());
    let mut f = File::create(&tmp).map_err(say)?;
    f.write_all(bytes).map_err(say)?;
    f.sync_all().map_err(say)?;
    fs::rename(&tmp, dir.join(name)).map_err(say)
}

fn sync_dir(dir: &Path) -> Result<(), String> {
    File::open(dir)
        .and_then(|f| f.sync_all())
        .map_err(|e| format!("syncing {}: {e}", dir.display()))
}

/// A directory that does not exist, or is empty: created.
fn empty_dir(dir: &Path) -> Result<(), String> {
    if let Ok(mut it) = fs::read_dir(dir) {
        if it.next().is_some() {
            return Err(format!("{} is not empty; refusing to write over it", dir.display()));
        }
    }
    fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))
}

/// What identifies a file's contents across a rename: its inode, length and
/// modification time.
#[cfg(unix)]
fn stamp(path: &Path) -> Option<(u64, u64, i64, i64)> {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).ok().map(|m| (m.ino(), m.len(), m.mtime(), m.mtime_nsec()))
}
#[cfg(not(unix))]
fn stamp(path: &Path) -> Option<(u64, u64, i64, i64)> {
    fs::metadata(path).ok().map(|m| (0, m.len(), 0, 0))
}

/// The other files of a data directory, in name order: every regular file
/// that is not the log and not a temporary (`.name.tmp`); and the
/// directories left out.
fn others(dir: &Path) -> Result<(Vec<String>, Vec<String>), String> {
    let (snap, page) = names();
    let mut files = vec![];
    let mut dirs = vec![];
    for e in fs::read_dir(dir).map_err(|e| format!("reading {}: {e}", dir.display()))? {
        let e = e.map_err(|e| e.to_string())?;
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == snap || name == page {
            continue;
        }
        match e.file_type() {
            Ok(t) if t.is_dir() => dirs.push(name),
            Ok(t) if t.is_file() => files.push(name),
            _ => {}
        }
    }
    files.sort();
    dirs.sort();
    Ok((files, dirs))
}

// -- backup ---------------------------------------------------------------

/// What a backup copied.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Backup {
    /// The snapshot's bytes, if there was one, and what it says.
    pub snapshot: Option<(u64, Head)>,
    /// The journal's bytes as copied (whole records only), and as long as
    /// it was when the snapshot had been read.
    pub journal: u64,
    pub journal_was: u64,
    pub records: usize,
    /// The head the copy reaches.
    pub head: Seq,
    /// How many times the pair was read: more than once where a compaction
    /// moved the snapshot between the two reads.
    pub reads: usize,
    pub files: Vec<String>,
    pub skipped: Vec<String>,
}

impl std::fmt::Display for Backup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.snapshot {
            Some((n, h)) => writeln!(f, "snapshot  {n} bytes, horizon {}, head {}", h.horizon, h.head)?,
            None => writeln!(f, "snapshot  none")?,
        }
        writeln!(
            f,
            "journal   {} bytes of {} when the snapshot was read, {} records",
            self.journal, self.journal_was, self.records
        )?;
        writeln!(f, "head      {}", self.head)?;
        if self.reads > 1 {
            writeln!(f, "read      {} times: a compaction moved the snapshot meanwhile", self.reads)?;
        }
        for n in &self.files {
            writeln!(f, "copied    {n}")?;
        }
        for n in &self.skipped {
            writeln!(f, "skipped   {n}/ (a peer's, not the server's)")?;
        }
        Ok(())
    }
}

/// How many times the snapshot and journal are read before a backup gives
/// up on a server compacting faster than it can copy: each compaction
/// writes a whole snapshot, so this many in the time of one read is a
/// server doing nothing else.
const READS: usize = 20;

/// `arkc backup DIR OUT` (the module docs).
pub fn backup(dir: &Path, out: &Path) -> Result<Backup, String> {
    backup_with(dir, out, || {})
}

/// [`backup`], with `between` run after the snapshot is read and before the
/// journal is: where a test puts the server's writes.
pub fn backup_with(dir: &Path, out: &Path, mut between: impl FnMut()) -> Result<Backup, String> {
    let (snap_name, page_name) = names();
    let (snap_path, page_path) = (dir.join(&snap_name), dir.join(&page_name));
    if !dir.is_dir() {
        return Err(format!("{} is not a directory", dir.display()));
    }
    let mut reads = 0;
    let (snapshot, journal, journal_was) = loop {
        reads += 1;
        let before = stamp(&snap_path);
        let snapshot = match fs::read(&snap_path) {
            Ok(b) => Some(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("reading {}: {e}", snap_path.display())),
        };
        let was = fs::metadata(&page_path).map_or(0, |m| m.len());
        between();
        let mut journal = match fs::read(&page_path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
            Err(e) => return Err(format!("reading {}: {e}", page_path.display())),
        };
        journal.truncate(was as usize);
        if stamp(&snap_path) == before && snapshot.as_ref().map(|b| b.len() as u64) == before.map(|s| s.1) {
            break (snapshot, journal, was);
        }
        if reads >= READS {
            return Err(format!(
                "the snapshot moved under every one of {READS} reads; is the server compacting in a loop?"
            ));
        }
    };
    let head = match &snapshot {
        Some(b) => Some(head_of(b).map_err(|e| format!("{}: {e}", snap_path.display()))?),
        None => None,
    };
    let base = head.as_ref().map_or(0, |h| h.head);
    let r = whole_records(base, &journal);
    let journal = &journal[..r.end as usize];

    empty_dir(out)?;
    if let Some(b) = &snapshot {
        write_file(out, &snap_name, b)?;
    }
    if !journal.is_empty() || snapshot.is_none() {
        write_file(out, &page_name, journal)?;
    }
    let (files, skipped) = others(dir)?;
    let mut copied = vec![];
    for name in files {
        match fs::read(dir.join(&name)) {
            Ok(b) => {
                write_file(out, &name, &b)?;
                copied.push(name);
            }
            // Gone between the listing and the read: a file the server
            // removed, which a backup of a moment later would not hold.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("reading {}: {e}", dir.join(&name).display())),
        }
    }
    sync_dir(out)?;
    Ok(Backup {
        snapshot: snapshot.as_ref().map(|b| b.len() as u64).zip(head),
        journal: journal.len() as u64,
        journal_was,
        records: r.applied,
        head: base + r.applied as Seq,
        reads,
        files: copied,
        skipped,
    })
}

// -- restore --------------------------------------------------------------

/// What a restore wrote.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Restored {
    pub head: Seq,
    pub horizon: Seq,
    /// The name the backup's log had, and whether it was kept.
    pub log_id: Option<Id>,
    pub kept_name: bool,
    pub files: Vec<String>,
}

impl std::fmt::Display for Restored {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "head      {}", self.head)?;
        writeln!(f, "horizon   {}", self.horizon)?;
        match (self.log_id, self.kept_name) {
            (Some(id), true) => writeln!(f, "log       {} (kept)", hex(&id))?,
            (Some(id), false) => writeln!(f, "log       {} dropped: the server names the restored log at start", hex(&id))?,
            (None, _) => writeln!(f, "log       unnamed: the server names it at start")?,
        }
        for n in &self.files {
            writeln!(f, "restored  {n}")?;
        }
        Ok(())
    }
}

/// The snapshot's bytes with its name taken off (the module docs). The hash
/// is of the state, not the name, so the file is otherwise the same.
fn unnamed(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut v = canon::decode(bytes).map_err(|e| format!("decoding the snapshot: {e}"))?;
    if let Value::Struct(m) = &mut v {
        if let Some(Value::Struct(base)) = m.get_mut("base") {
            base.remove("log");
        }
    }
    Ok(canon::encode(&v))
}

/// `arkc restore BACKUP DIR [--same-log]` (the module docs): the backup's
/// files into a directory that is empty or absent, the log first.
pub fn restore(from: &Path, dir: &Path, same_log: bool) -> Result<Restored, String> {
    let (snap_name, page_name) = names();
    if !from.is_dir() {
        return Err(format!("{} is not a directory", from.display()));
    }
    // Read whole before anything is written, so a backup that is not one
    // leaves nothing behind.
    let snapshot = match fs::read(from.join(&snap_name)) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("reading {}: {e}", from.join(&snap_name).display())),
    };
    let journal = fs::read(from.join(&page_name)).unwrap_or_default();
    if snapshot.is_none() && journal.is_empty() {
        return Err(format!("{} holds no log ({snap_name}, {page_name})", from.display()));
    }
    let head = snapshot.as_deref().map(head_of).transpose()?.unwrap_or_default();
    let r = whole_records(head.head, &journal);
    if r.torn() {
        return Err(format!(
            "{}: the journal does not follow on from the snapshot after {} records",
            from.display(),
            r.applied
        ));
    }
    let (files, _) = others(from)?;
    empty_dir(dir)?;
    let mut written = vec![];
    if let Some(b) = &snapshot {
        let b = if same_log { b.clone() } else { unnamed(b)? };
        write_file(dir, &snap_name, &b)?;
        written.push(snap_name.clone());
    }
    if !journal.is_empty() {
        write_file(dir, &page_name, &journal)?;
        written.push(page_name.clone());
    }
    for name in files {
        let b = fs::read(from.join(&name)).map_err(|e| format!("reading {}: {e}", from.join(&name).display()))?;
        write_file(dir, &name, &b)?;
        written.push(name);
    }
    sync_dir(dir)?;
    Ok(Restored {
        head: head.head + r.applied as Seq,
        horizon: head.horizon,
        log_id: head.log_id,
        kept_name: same_log,
        files: written,
    })
}

// -- verify-log -----------------------------------------------------------

/// What `verify-log` found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub head: Seq,
    pub horizon: Seq,
    pub log_id: Option<Id>,
    /// The state hash at the head, where the module was given.
    pub hash: Option<Vec<u8>>,
    /// The hash the snapshot carries, of the state at its horizon.
    pub snapshot_hash: Vec<u8>,
    pub snapshot_entries: usize,
    pub journal_records: usize,
    pub stale: usize,
    /// Bytes at the end of the journal that are not a whole record.
    pub torn: u64,
    pub ids: usize,
    /// Every module the server has run, as `modules.cbor` lists them.
    pub modules: Vec<Vec<u8>>,
    /// The module given, and whether the server has run it.
    pub module: Option<(Vec<u8>, bool)>,
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "head      {}", self.head)?;
        match &self.hash {
            Some(h) => writeln!(f, "hash      {}", hex(h))?,
            None => writeln!(f, "hash      (give the module to compute it)")?,
        }
        writeln!(f, "horizon   {} (snapshot hash {})", self.horizon, hex(&self.snapshot_hash))?;
        match self.log_id {
            Some(id) => writeln!(f, "log       {}", hex(&id))?,
            None => writeln!(f, "log       unnamed")?,
        }
        writeln!(
            f,
            "entries   {} in the snapshot, {} in the journal; {} ids",
            self.snapshot_entries, self.journal_records, self.ids
        )?;
        if self.stale > 0 {
            writeln!(f, "stale     {} records a compaction already holds", self.stale)?;
        }
        if self.torn > 0 {
            writeln!(f, "torn      {} bytes at the journal's end, not a whole record", self.torn)?;
        }
        for m in &self.modules {
            writeln!(f, "module    {}", hex(m))?;
        }
        if let Some((m, ran)) = &self.module {
            let said = if *ran || self.modules.is_empty() {
                ""
            } else {
                " — not one this server has run"
            };
            writeln!(f, "given     {}{said}", hex(m))?;
        }
        Ok(())
    }
}

/// The module hashes `modules.cbor` lists, in its order; none where there
/// is no file.
fn modules_run(dir: &Path) -> Result<Vec<Vec<u8>>, String> {
    let path = dir.join(MODULES);
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(format!("reading {}: {e}", path.display())),
    };
    let v = canon::decode(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let Some(Value::List(ms)) = record(&v)?.get("modules") else {
        return Err(format!("{}: no modules", path.display()));
    };
    ms.iter()
        .map(|m| match record(m).and_then(|r| field(r, "module").cloned()) {
            Ok(Value::Bytes(h)) => Ok(h.into_vec()),
            _ => Err(format!("{}: a module with no hash", path.display())),
        })
        .collect()
}

/// `arkc verify-log DIR [M]` (the module docs).
pub fn verify_log(dir: &Path, module: Option<&Module>) -> Result<Report, String> {
    let keys = ReadDir(dir.to_path_buf());
    let layout = Layout::server();
    let snapshot = keys.load(&layout.snapshot)?;
    let journal = keys.load(&layout.page)?.unwrap_or_default();
    if snapshot.is_none() && journal.is_empty() {
        return Err(format!("{} holds no log", dir.display()));
    }
    let h = snapshot.as_deref().map(head_of).transpose()?.unwrap_or_default();
    let r = whole_records(h.head, &journal);
    let mut report = Report {
        head: h.head + r.applied as Seq,
        horizon: h.horizon,
        log_id: h.log_id,
        hash: None,
        snapshot_hash: h.hash.clone(),
        snapshot_entries: h.entries,
        journal_records: r.applied,
        stale: r.stale,
        torn: r.len - r.end,
        ids: h.ids + r.applied,
        modules: modules_run(dir)?,
        module: None,
    };
    if let Some(m) = module {
        // The snapshot's rows checked against its hash, as the server's
        // own open does, and the journal replayed onto it.
        let log = journal::load(&keys, &layout, &m.schema)?.ok_or("no log")?;
        if log.head_seq() != report.head {
            return Err(format!("read to {} with the schema and {} without", log.head_seq(), report.head));
        }
        let st = log.state_at(log.head_seq()).ok_or("no state at the head")?;
        report.hash = Some(ark::hash::state_hash(&st));
        let mh = ark::hash::module_hash(m);
        let ran = report.modules.contains(&mh);
        report.module = Some((mh, ran));
    }
    Ok(report)
}
