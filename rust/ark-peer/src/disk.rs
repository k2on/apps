//! The store on disk (§1.6, §1.7's `Persist`): what a peer made durable,
//! read back on open. The log is a table of the store, so the store's rows
//! and the log's rows appended since are the one format; an entry's facts
//! are in its row, and are on disk once.
//!
//! A [`Disk`] is named blobs — a file each in a directory ([`Files`]), a
//! buffer each ([`Memory`], for tests and the simulator), or a key each of
//! a page's `localStorage` (`Local`, in a browser) — and the format is
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
//!   the one before it, and writes `kv` whole when one does not.
//! - the rest, each written whole or appended to: a server's `journal` and
//!   `live`, written whole; a device's `pending` (its own intents not yet
//!   confirmed and the last `local` its session drew), a journal of
//!   records, each a four-byte big-endian length and `((Int local, Int
//!   gone), List Entry)`: the intents left are those before with the first
//!   `gone` taken off and these added. It is read up to the first record
//!   that does not read, as `log` is.
//!
//! A disk holding the blobs of a format before — the log not yet a table
//! (`snapshot`, `chain`), or every write journaled (`writes`) — is refused,
//! never read as empty.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;

use ark::codec::Node;
use ark::log::{Entry, Logged, Record, Rows};

/// The store made durable, read back: its rows as last written whole, and
/// the log's rows appended since.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stored {
    pub rows: Option<Rows>,
    pub logged: Vec<Logged>,
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
        let (log, had) = self.get("log")?.map_or((vec![], false), |l| (l, true));
        let rows = kv.and_then(|b| match Record::dec(&b) {
            Ok(Record::Rows(r)) => Some(r),
            _ => None,
        });
        let (records, at) = framed::<Record>(&log);
        let logged = records.into_iter().map_while(|r| match r {
            Record::Logged(l) => Some(l),
            Record::Rows(_) => None,
        });
        let s = Stored {
            rows,
            logged: logged.collect(),
        };
        if at < log.len() || !had {
            self.put("log", &log[..at])?;
        }
        Ok(s)
    }

    /// One `Persist`: the store's rows whole, or one of the log's appended.
    fn record(&mut self, bytes: &[u8]) -> io::Result<()> {
        if matches!(bytes.first(), Some(1)) {
            self.put("kv", bytes)?;
            return self.put("log", &[]);
        }
        self.append("log", &frame(bytes))
    }
}

/// One record of the `pending` journal.
pub fn pending_record(local: i64, gone: usize, new: Vec<Entry>) -> Vec<u8> {
    frame(&((local, gone as i64), new).to_bytes())
}

/// The `pending` journal read: the last `local` and the intents left.
pub fn pending(bytes: &[u8]) -> (i64, Vec<Entry>) {
    let (mut local, mut left) = (0, vec![]);
    for ((l, gone), new) in framed::<((i64, i64), Vec<Entry>)>(bytes).0 {
        left.drain(..(gone.max(0) as usize).min(left.len()));
        left.extend(new);
        local = l;
    }
    (local, left)
}

/// Length-framed records read up to the first that does not read; and the
/// bytes they take.
fn framed<T: Node>(bytes: &[u8]) -> (Vec<T>, usize) {
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
