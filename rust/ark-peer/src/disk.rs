//! The store on disk (§1.6, §1.7's `Persist`): what a peer made durable,
//! read back on open. The log is a table of the store, so the store's rows
//! and the log's rows appended since are the one format; an entry's facts
//! are in its row, and are on disk once.
//!
//! A [`Disk`] is named blobs — a file each in a directory ([`Files`]), a
//! buffer each ([`Memory`], for tests and the simulator), or a record or a few each
//! of a page's IndexedDB (`Browser`, in a browser) — and the format is
//! written once, over them ([`Disk::load`], [`Disk::record`]):
//!
//! - `kv`: the store's rows whole ([`Record::Rows`]), the log's table's
//!   among them, written (to a temporary file, then renamed) when a
//!   `Persist` carries them, which also empties `log`: every entry before
//!   is folded into them.
//! - `log`: every [`Record::Logged`] a `Persist` carried — a row of the
//!   log's table, one logged entry with its facts — appended, each as a
//!   four-byte big-endian length and the record's bytes. On open they are
//!   read up to the first record that does not read, and the blob is cut
//!   there (made, empty, when there is none): a crash mid-write leaves a torn tail, never a wrong entry. The
//!   rows of the schema's tables are not journaled: the peer writes them
//!   again as it opens, from each entry's facts, each entry linking onto
//!   the one before it, and writes `kv` whole when one does not. A replica
//!   filling appends each part of its fill there too ([`Record::Part`]),
//!   read back beside the entries.
//!
//! A server keeps its schema beside them, as JSON ([`SCHEMA`]): what reads
//! the rows without its domain compiled in reads them under it.
//!
//! What a peer keeps beside its log is the rows of its local instance
//! (`local`), in this same format under the names `local.kv` and
//! `local.log` ([`crate::Shared`]).
//!
//! A disk holding the blobs of a format before — the log not yet a table
//! (`snapshot`, `chain`), every write journaled (`writes`), or a peer's own
//! state in blobs of their own (`journal`, `live`, `pending`, `origin`,
//! `fork`, `joining`, `who`) — is refused, never read as empty.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;

use ark::format::Encode;
use ark::log::{Logged, Part, Record, Rows};

/// The blob a server's schema is in, beside its log.
pub const SCHEMA: &str = "schema.json";

/// The store made durable, read back: its rows as last written whole, and
/// the log's rows appended since.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stored {
    pub rows: Option<Rows>,
    pub logged: Vec<Logged>,
    pub parts: Vec<Part>,
}

/// Where a peer's durable records go: named blobs, and the format over them.
pub trait Disk: Send {
    fn get(&mut self, name: &str) -> io::Result<Option<Vec<u8>>>;
    fn put(&mut self, name: &str, bytes: &[u8]) -> io::Result<()>;
    /// Bytes added at the end of a blob.
    fn append(&mut self, name: &str, bytes: &[u8]) -> io::Result<()>;

    /// The store's rows and the log's rows after them, read; a torn tail
    /// of `log` cut.
    fn load(&mut self) -> io::Result<Stored> {
        let kv = self.get("kv")?;
        if kv.is_none() && (self.get("snapshot")?.is_some() || self.get("chain")?.is_some()) {
            let why = "a log of the format before the log was a table (snapshot, chain): not read";
            return Err(io::Error::other(why));
        }
        if self.get("writes")?.is_some() {
            let why = "a log of the format that journaled every write (writes), not the log's rows alone: not read";
            return Err(io::Error::other(why));
        }
        for old in ["journal", "live", "pending", "origin", "fork", "joining", "who"] {
            if self.get(old)?.is_some() {
                let why = format!("a disk of the format that kept a peer's own state in blobs ({old}), not rows: not read");
                return Err(io::Error::other(why));
            }
        }
        let (log, had) = self.get("log")?.map_or((vec![], false), |l| (l, true));
        let rows = kv.and_then(|b| match Record::dec(&b) {
            Ok(Record::Rows(r)) => Some(r),
            _ => None,
        });
        let (records, at) = framed::<Record>(&log);
        let (mut logged, mut parts) = (vec![], vec![]);
        for r in records {
            match r {
                Record::Logged(l) => logged.push(l),
                Record::Part(p) => parts.push(p),
                Record::Rows(_) => break,
            }
        }
        let s = Stored { rows, logged, parts };
        if at < log.len() || !had {
            self.put("log", &log[..at])?;
        }
        Ok(s)
    }

    /// One `Persist`: the store's rows whole, or one of the log's appended.
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
fn framed<T: Encode>(bytes: &[u8]) -> (Vec<T>, usize) {
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

fn frame(bytes: &[u8]) -> Vec<u8> {
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
        match self.sync || name != "log" {
            true => f.sync_data(),
            false => Ok(()),
        }
    }
}

/// The page's blobs, in its IndexedDB (`crate::browser`).
#[cfg(target_arch = "wasm32")]
pub use crate::browser::{open as open_browser, Browser};
