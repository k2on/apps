//! The store on disk (§1.6, §1.7's `Persist`): what a peer made durable,
//! read back on open. The log is a table of the store, so the store's rows
//! and the log's rows appended since are the one format; an entry's facts
//! are in its row, and are on disk once.
//!
//! Two things, each its own file in a directory ([`Files`]), its own
//! buffer ([`Memory`], for tests and the simulator) or its own key of a
//! page's `localStorage` (`Local`, in a browser), and blobs beside them:
//!
//! - `kv`: the store's rows whole ([`Record::Rows`]), the log's table's
//!   among them, written (to a temporary file, then renamed) when a
//!   `Persist` carries them, which also empties `log`: every entry before
//!   is folded into them.
//! - `log`: every [`Record::Logged`] a `Persist` carried — a row of the
//!   log's table, one logged entry with its facts — appended, each as a
//!   four-byte big-endian length and the record's bytes. On open they are
//!   read up to the first record that does not read, and the file is cut
//!   there: a crash mid-write leaves a torn tail, never a wrong entry. The
//!   rows of the schema's tables are not journaled: the peer writes them
//!   again as it opens, from each entry's facts, each entry linking onto
//!   the one before it, and writes `kv` whole when one does not.
//! - named blobs, each written whole or appended to: a server's `journal`
//!   and `live`, written whole; a client's `pending` (its own
//!   intents not yet confirmed and the last `local` its session drew), a
//!   journal of records, each a four-byte big-endian length and `((Int
//!   local, Int gone), List Entry)`: the intents left are those before with
//!   the first `gone` taken off and these added. It is read up to the first
//!   record that does not read, as `log` is.
//!
//! A directory holding the files of a format before — the log not yet a
//! table (`snapshot`, `chain`), or every write journaled (`writes`) — is
//! refused, never read as empty.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read as _, Write};
use std::path::PathBuf;

use std::collections::BTreeMap;

use ark::ir::Node;
use ark::log::{Entry, Logged, Record, Rows};

/// The store made durable, read back: its rows as last written whole, and
/// the log's rows appended since.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stored {
    pub rows: Option<Rows>,
    pub logged: Vec<Logged>,
}

/// Where a peer's durable records go.
pub trait Disk: Send {
    fn load(&mut self) -> io::Result<Stored>;
    /// One `Persist`: a record's bytes.
    fn record(&mut self, bytes: &[u8]) -> io::Result<()>;
    fn put(&mut self, name: &str, bytes: &[u8]) -> io::Result<()>;
    /// Bytes added at the end of a blob; by default, the blob read and
    /// written whole.
    fn append(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let mut b = self.get(name)?.unwrap_or_default();
        b.extend_from_slice(bytes);
        self.put(name, &b)
    }
    fn get(&mut self, name: &str) -> io::Result<Option<Vec<u8>>>;
}

/// One record of the `pending` journal.
pub fn pending_record(local: i64, gone: usize, new: Vec<Entry>) -> Vec<u8> {
    let b = ((local, gone as i64), new).to_bytes();
    [&(b.len() as u32).to_be_bytes()[..], &b].concat()
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

/// The store's rows and the log's rows after them, read; and the bytes of
/// `log` that read.
fn read(kv: Option<&[u8]>, log: &[u8]) -> (Stored, usize) {
    let rows = kv.and_then(|b| match Record::dec(b) {
        Ok(Record::Rows(r)) => Some(r),
        _ => None,
    });
    let (records, at) = framed::<Record>(log);
    let mut s = Stored { rows, logged: vec![] };
    for r in records {
        match r {
            Record::Logged(l) => s.logged.push(l),
            Record::Rows(_) => break,
        }
    }
    (s, at)
}

fn rows_of(bytes: &[u8]) -> bool {
    matches!(bytes.first(), Some(1))
}

/// The records in memory: what a simulated device keeps across a reopen.
#[derive(Clone, Debug, Default)]
pub struct Memory {
    pub kv: Option<Vec<u8>>,
    pub log: Vec<u8>,
    pub blobs: BTreeMap<String, Vec<u8>>,
}

impl Disk for Memory {
    fn load(&mut self) -> io::Result<Stored> {
        let (s, at) = read(self.kv.as_deref(), &self.log);
        self.log.truncate(at);
        Ok(s)
    }

    fn record(&mut self, bytes: &[u8]) -> io::Result<()> {
        if rows_of(bytes) {
            self.kv = Some(bytes.to_vec());
            self.log.clear();
        } else {
            self.log.extend(frame(bytes));
        }
        Ok(())
    }

    fn put(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        self.blobs.insert(name.into(), bytes.to_vec());
        Ok(())
    }

    fn append(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        self.blobs.entry(name.into()).or_default().extend_from_slice(bytes);
        Ok(())
    }

    fn get(&mut self, name: &str) -> io::Result<Option<Vec<u8>>> {
        Ok(self.blobs.get(name).cloned())
    }
}

/// The records in a directory: `kv`, `log`, and a file per blob.
#[derive(Debug)]
pub struct Files {
    dir: PathBuf,
    log: Option<File>,
    /// Whether every append is synced before it returns: a server's
    /// acknowledgement follows the write.
    pub sync: bool,
}

impl Files {
    pub fn new(dir: impl Into<PathBuf>) -> io::Result<Files> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        Ok(Files { dir, log: None, sync: false })
    }

    fn whole(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let tmp = self.dir.join(format!("{name}.tmp"));
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(tmp, self.dir.join(name))
    }

    fn opened(&mut self) -> io::Result<&mut File> {
        if self.log.is_none() {
            self.log = Some(OpenOptions::new().create(true).append(true).open(self.dir.join("log"))?);
        }
        Ok(self.log.as_mut().unwrap())
    }
}

fn file(path: PathBuf) -> io::Result<Option<Vec<u8>>> {
    match File::open(path) {
        Ok(mut f) => {
            let mut b = vec![];
            f.read_to_end(&mut b)?;
            Ok(Some(b))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

impl Disk for Files {
    fn load(&mut self) -> io::Result<Stored> {
        let kv = file(self.dir.join("kv"))?;
        if kv.is_none() && (self.dir.join("snapshot").exists() || self.dir.join("chain").exists()) {
            let why = "a log of the format before the log was a table (snapshot, chain): not read";
            return Err(io::Error::other(why));
        }
        if self.dir.join("writes").exists() {
            let why = "a log of the format that journaled every write (writes), not the log's rows alone: not read";
            return Err(io::Error::other(why));
        }
        let log = file(self.dir.join("log"))?.unwrap_or_default();
        let (s, at) = read(kv.as_deref(), &log);
        self.log = None;
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(self.dir.join("log"))?
            .set_len(at as u64)?;
        Ok(s)
    }

    fn record(&mut self, bytes: &[u8]) -> io::Result<()> {
        if rows_of(bytes) {
            self.whole("kv", bytes)?;
            self.log = None;
            return File::create(self.dir.join("log"))?.sync_all();
        }
        let sync = self.sync;
        let f = self.opened()?;
        f.write_all(&frame(bytes))?;
        if sync {
            f.sync_data()?;
        }
        Ok(())
    }

    fn put(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        self.whole(name, bytes)
    }

    /// Synced before it returns, as a blob written whole is.
    fn append(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let mut f = OpenOptions::new().create(true).append(true).open(self.dir.join(name))?;
        f.write_all(bytes)?;
        f.sync_data()
    }

    fn get(&mut self, name: &str) -> io::Result<Option<Vec<u8>>> {
        file(self.dir.join(name))
    }
}

/// The records in the page's `localStorage`, under one key's prefix: a
/// [`Memory`] read once and written through, each part as hex.
#[cfg(target_arch = "wasm32")]
pub struct Local {
    key: String,
    mem: Memory,
}

#[cfg(target_arch = "wasm32")]
impl Local {
    pub fn open(key: &str) -> Result<Local, String> {
        let s = ark_auth::web::storage().ok_or("this page has no localStorage")?;
        let get = |k: &str| s.get_item(&format!("{key}:{k}")).ok().flatten().and_then(|h| ark::hash::unhex(&h));
        let blobs = get("blobs")
            .map(|b| <Vec<(String, Vec<u8>)>>::from_bytes(&b))
            .transpose()
            .map_err(|e| e.0)?;
        let mem = Memory {
            kv: get("kv"),
            log: get("log").unwrap_or_default(),
            blobs: blobs.unwrap_or_default().into_iter().collect(),
        };
        Ok(Local { key: key.into(), mem })
    }

    fn save(&self) -> io::Result<()> {
        let s = ark_auth::web::storage().ok_or_else(|| io::Error::other("no localStorage"))?;
        let put = |k: &str, b: &[u8]| {
            s.set_item(&format!("{}:{k}", self.key), &ark::hash::hex(b))
                .map_err(|e| io::Error::other(format!("{e:?}")))
        };
        if let Some(b) = &self.mem.kv {
            put("kv", b)?;
        }
        put("log", &self.mem.log)?;
        put("blobs", &self.mem.blobs.clone().into_iter().collect::<Vec<_>>().to_bytes())
    }
}

#[cfg(target_arch = "wasm32")]
impl Disk for Local {
    fn load(&mut self) -> io::Result<Stored> {
        self.mem.load()
    }
    fn record(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.mem.record(bytes)?;
        self.save()
    }
    fn put(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        self.mem.put(name, bytes)?;
        self.save()
    }
    fn append(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        self.mem.append(name, bytes)?;
        self.save()
    }
    fn get(&mut self, name: &str) -> io::Result<Option<Vec<u8>>> {
        self.mem.get(name)
    }
}
