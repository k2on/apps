//! The store on disk (§1.6, §1.7's `Send(To::Disk)`): what a peer made durable,
//! read back on open. The log is a table of the store, so the store's rows
//! and the log's rows appended since are the one format; an entry's facts
//! are in its row, and are on disk once.
//!
//! A [`Disk`] is named blobs — a file each in a directory ([`Files`]), a
//! buffer each ([`Memory`], for tests and the simulator), or a key each of
//! a page's `localStorage` (`Local`, in a browser, for what is small) — and the format is
//! written once, over them ([`Disk::load`], [`Disk::record`]). The files
//! rows name are blobs too, of a disk of their own ([`Disk::files`]: a
//! directory's `files/`), read and written in parts (`crate::files`):
//!
//! - `kv`: the store's rows whole ([`Record::Rows`]), the log's table's
//!   among them, written (to a temporary file, then renamed) when a
//!   `Send(To::Disk)` carries them, which also empties `log`: every entry before
//!   is folded into them.
//! - `log`: every [`Record::Logged`] a `Send(To::Disk)` carried — a row of the
//!   log's table, one logged entry with its facts — appended, each as a
//!   four-byte big-endian length and the record's bytes. On open they are
//!   read up to the first record that does not read, and the blob is cut
//!   there (made, empty, when there is none): a crash mid-write leaves a torn tail, never a wrong entry. The
//!   rows of the schema's tables are not journaled: the peer writes them
//!   again as it opens, from each entry's facts, each entry linking onto
//!   the one before it, and writes `kv` whole when one does not.
//!
//! A server keeps its schema beside them, as JSON ([`SCHEMA`]): what reads
//! the rows without its domain compiled in reads them under it.
//!
//! What a peer keeps beside its log is records under the names `local.*`
//! (`local::Shared`): a device's replica (`local.replica`) and its pending
//! intents, records framed the same way and appended (`local.pending`,
//! `local::Kept`); a server's journal and volatile space (`crate::hub`).
//!
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use ark::format::Encode;
use std::sync::Arc;

use ark::dynamic::Tables;
use ark::log::Record;
use ark::schema::{compat, Schema, Space};

/// The blob a disk's schema is in, beside its log: what its rows are, to
/// what reads them without the domain compiled in.
pub const SCHEMA: &str = "schema.json";

/// The schema `disk` says its rows are, if it says.
fn stored(disk: &mut dyn Disk) -> io::Result<Option<Schema>> {
    let Some(b) = disk.get(SCHEMA)? else { return Ok(None) };
    serde_json::from_slice(&b)
        .map(Some)
        .map_err(|e| io::Error::other(format!("{SCHEMA}: {e}")))
}

/// `schema` beside `disk`'s rows, as what they are.
pub fn put_schema(disk: &mut dyn Disk, schema: &Schema) -> io::Result<()> {
    disk.put(SCHEMA, &serde_json::to_vec_pretty(schema).map_err(io::Error::other)?)
}

/// Whether `disk`'s rows read under `schema`: it says none, or one
/// `schema` grows from (§1.2). A schema only grows, so every device syncs
/// with every server; data moves within it by an [`export`] and an
/// [`import`].
pub fn own(disk: &mut dyn Disk, schema: &Schema) -> io::Result<bool> {
    Ok(stored(disk)?.is_none_or(|was| compat(&was, schema).is_ok()))
}

/// Every row `disk` holds, in its human form ([`ark::dynamic::Tables`]),
/// read under the schema it says, and that schema. The disk is read from a
/// copy, so a peer writing it meanwhile is undisturbed.
pub fn export(disk: &mut dyn Disk) -> io::Result<(Schema, Tables)> {
    let schema = stored(disk)?.ok_or_else(|| io::Error::other(format!("no {SCHEMA}: nothing says what the rows are")))?;
    let mut copy = Memory::default();
    for name in ["kv", "log"] {
        disk.get(name)?.map_or(Ok(()), |b| copy.put(name, &b))?;
    }
    let read = crate::Instance::open_under(Arc::new(schema.clone()), Box::new(copy));
    if let Some(why) = read.failure {
        return Err(io::Error::other(why));
    }
    let tables = ark::dynamic::tables(&read.store).map_err(io::Error::other)?;
    Ok((schema, tables))
}

/// `tables` — an [`export`] taken under `from`, transformed — made the
/// rows of an empty `disk` under `schema`, which grows from `from`, every
/// row judged, its log starting afresh.
pub fn import(disk: &mut dyn Disk, from: &Schema, schema: Arc<Schema>, tables: &Tables) -> io::Result<()> {
    compat(from, &schema).map_err(|e| io::Error::other(format!("the schema does not grow from the export's: {}", e)))?;
    if disk.get("kv")?.is_some() || disk.get("log")?.is_some_and(|l| !l.is_empty()) {
        return Err(io::Error::other("an import is into a disk that holds no rows"));
    }
    let store = ark::dynamic::store(schema.clone(), Space::Durable, tables).map_err(io::Error::other)?;
    let rows = ark::log::standing(store.snapshot_rows(), 0, Default::default());
    disk.record(&Record::Rows(rows).to_bytes())?;
    put_schema(disk, &schema)
}

/// Where a peer's durable records go: named blobs, and the format over them.
pub trait Disk: Send {
    fn get(&mut self, name: &str) -> io::Result<Option<Vec<u8>>>;
    fn put(&mut self, name: &str, bytes: &[u8]) -> io::Result<()>;
    /// Bytes added at the end of a blob.
    fn append(&mut self, name: &str, bytes: &[u8]) -> io::Result<()>;

    /// A blob's size, when there is one.
    fn len(&self, _name: &str) -> Option<u64> {
        None
    }

    /// At most `n` of a blob's bytes from `at`: what a file is read in
    /// parts by.
    fn read(&self, name: &str, _at: u64, _n: usize) -> io::Result<Vec<u8>> {
        Err(io::Error::other(format!("{name} is not read in parts here")))
    }

    /// A blob given another name, durably; the one it had gone.
    fn rename(&mut self, from: &str, to: &str) -> io::Result<()> {
        let b = self.get(from)?.ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        self.put(to, &b)?;
        self.remove(from)
    }

    /// A blob gone; none there is no failure.
    fn remove(&mut self, name: &str) -> io::Result<()> {
        Err(io::Error::other(format!("{name} is not removed here")))
    }

    /// The names of its blobs that end `suffix`.
    fn names(&self, _suffix: &str) -> Vec<String> {
        vec![]
    }

    /// Where the files its rows name are kept (`crate::files`), as blobs
    /// named by their hash: in memory, unless the disk is a directory.
    fn files(&self) -> Box<dyn Disk> {
        Box::new(Memory::default())
    }

    /// What the store made durable is opened from: its rows as last written
    /// whole (none written, a log standing on the empty chain) and the log's
    /// rows appended since, read; a torn tail of `log` cut.
    fn load(&mut self) -> io::Result<Vec<Record>> {
        let kv = self.get("kv")?;
        let (log, had) = self.get("log")?.map_or((vec![], false), |l| (l, true));
        let rows = match kv.map(|b| Record::dec(&b)) {
            Some(Ok(Record::Rows(r))) => r,
            _ => ark::log::standing(vec![], 0, Default::default()),
        };
        let (records, at) = framed::<Record>(&log);
        let mut s = vec![Record::Rows(rows)];
        s.extend(records.into_iter().take_while(|r| matches!(r, Record::Logged(_))));
        if at < log.len() || !had {
            self.put("log", &log[..at])?;
        }
        Ok(s)
    }

    /// One `Send(To::Disk)`: the store's rows whole, or one of the log's appended.
    fn record(&mut self, bytes: &[u8]) -> io::Result<()> {
        if Record::is_rows(bytes) {
            self.put("kv", bytes)?;
            return self.put("log", &[]);
        }
        self.append("log", &frame(bytes))
    }
}

/// Length-framed records read up to the first that does not read; and the
/// bytes they take.
pub(crate) fn framed<T: Encode>(bytes: &[u8]) -> (Vec<T>, usize) {
    let (mut out, mut at) = (vec![], 0);
    while let Some(len) = bytes.get(at..at + 4).map(|b| u32::from_be_bytes(b.try_into().unwrap()) as usize) {
        let Some(Ok(x)) = bytes.get(at + 4..at + 4 + len).map(T::from_bytes) else {
            break;
        };
        out.push(x);
        at += 4 + len;
    }
    (out, at)
}

pub(crate) fn frame(bytes: &[u8]) -> Vec<u8> {
    [&(bytes.len() as u32).to_be_bytes()[..], bytes].concat()
}

/// The blobs in memory: what a simulated device keeps across a reopen.
#[derive(Clone, Debug, Default)]
pub struct Memory(pub BTreeMap<String, Vec<u8>>);

impl Disk for Memory {
    fn get(&mut self, name: &str) -> io::Result<Option<Vec<u8>>> {
        Ok(self.0.get(name).cloned())
    }

    fn put(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        self.0.insert(name.into(), bytes.to_vec());
        Ok(())
    }

    fn append(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        self.0.entry(name.into()).or_default().extend_from_slice(bytes);
        Ok(())
    }

    fn len(&self, name: &str) -> Option<u64> {
        self.0.get(name).map(|b| b.len() as u64)
    }

    fn read(&self, name: &str, at: u64, n: usize) -> io::Result<Vec<u8>> {
        let b = self.0.get(name).ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        let at = (at as usize).min(b.len());
        Ok(b[at..(at + n).min(b.len())].to_vec())
    }

    fn remove(&mut self, name: &str) -> io::Result<()> {
        self.0.remove(name);
        Ok(())
    }

    fn names(&self, suffix: &str) -> Vec<String> {
        self.0.keys().filter(|n| n.ends_with(suffix)).cloned().collect()
    }
}

/// The blobs in a directory, a file each; one written whole goes to a
/// temporary file, synced, then renamed.
#[derive(Debug)]
pub struct Files {
    dir: PathBuf,
    /// Whether every append to `log` is synced before it returns, as every
    /// other is: a server's acknowledgement follows the write.
    pub sync: bool,
}

impl Files {
    pub fn new(dir: impl Into<PathBuf>) -> io::Result<Files> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        Ok(Files { dir, sync: false })
    }
}

impl Disk for Files {
    fn files(&self) -> Box<dyn Disk> {
        match Files::new(self.dir.join("files")) {
            Ok(d) => Box::new(d),
            Err(_) => Box::new(Memory::default()),
        }
    }

    fn len(&self, name: &str) -> Option<u64> {
        fs::metadata(self.dir.join(name)).ok().map(|m| m.len())
    }

    fn read(&self, name: &str, at: u64, n: usize) -> io::Result<Vec<u8>> {
        let mut f = File::open(self.dir.join(name))?;
        f.seek(SeekFrom::Start(at))?;
        let mut b = Vec::with_capacity(n);
        f.take(n as u64).read_to_end(&mut b)?;
        Ok(b)
    }

    fn rename(&mut self, from: &str, to: &str) -> io::Result<()> {
        File::open(self.dir.join(from))?.sync_all()?;
        fs::rename(self.dir.join(from), self.dir.join(to))
    }

    fn remove(&mut self, name: &str) -> io::Result<()> {
        match fs::remove_file(self.dir.join(name)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    fn names(&self, suffix: &str) -> Vec<String> {
        let Ok(rd) = fs::read_dir(&self.dir) else { return vec![] };
        let names = rd.filter_map(|e| e.ok()?.file_name().into_string().ok());
        names.filter(|n| n.ends_with(suffix)).collect()
    }

    fn get(&mut self, name: &str) -> io::Result<Option<Vec<u8>>> {
        match fs::read(self.dir.join(name)) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            r => r.map(Some),
        }
    }

    fn put(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let tmp = self.dir.join(format!("{name}.tmp"));
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(tmp, self.dir.join(name))
    }

    fn append(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let mut f = OpenOptions::new().create(true).append(true).open(self.dir.join(name))?;
        f.write_all(bytes)?;
        // A file arriving is synced as it is renamed whole.
        match self.sync || name != "log" && !name.ends_with(".part") {
            true => f.sync_data(),
            false => Ok(()),
        }
    }
}

/// The blobs in the page's `localStorage`, under one key's prefix, each as
/// hex.
#[cfg(target_arch = "wasm32")]
pub struct Local(pub String);

#[cfg(target_arch = "wasm32")]
impl Disk for Local {
    fn get(&mut self, name: &str) -> io::Result<Option<Vec<u8>>> {
        let s = ark_auth::web::storage().ok_or_else(|| io::Error::other("no localStorage"))?;
        Ok(s.get_item(&format!("{}:{name}", self.0))
            .ok()
            .flatten()
            .and_then(|h| ark::hash::unhex(&h)))
    }

    fn put(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let s = ark_auth::web::storage().ok_or_else(|| io::Error::other("no localStorage"))?;
        let key = format!("{}:{name}", self.0);
        s.set_item(&key, &ark::hash::hex(bytes)).map_err(|e| io::Error::other(format!("{e:?}")))
    }

    fn append(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let b = [self.get(name)?.unwrap_or_default(), bytes.to_vec()].concat();
        self.put(name, &b)
    }
}
