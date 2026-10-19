//! The chain on disk (§1.6, §1.7's `Persist`): what a peer made durable,
//! read back on open.
//!
//! Three things, each its own file in a directory ([`Files`]) or its own
//! buffer ([`Memory`], for tests and the simulator):
//!
//! - `snapshot`: the [`Snapshot`] the chain stands on — its rows in key
//!   order — written whole (to a temporary file, then renamed) when a
//!   `Persist` carries one, which also empties the chain.
//! - `chain`: every `Logged` a `Persist` carried, appended, each as a
//!   four-byte big-endian length and the record's bytes. On open the chain
//!   is read up to the first record that does not read or whose hash does
//!   not check against the one before it, and the file is cut there: a
//!   crash mid-write leaves a torn tail, never a wrong entry.
//! - named blobs beside them, each written whole: a client's `pending`
//!   (its own intents not yet confirmed and the last `local` its session
//!   drew, `(Int, List Entry)`); a hub's `journal` and `live` (`ark5-server`).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read as _, Write};
use std::path::PathBuf;

use std::collections::BTreeMap;

use ark5::log::{Logged, Record, Snapshot};

/// The chain made durable, read back.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stored {
    pub snapshot: Option<Snapshot>,
    pub chain: Vec<Logged>,
}

/// Where a peer's durable records go.
pub trait Disk: Send {
    fn load(&mut self) -> io::Result<Stored>;
    /// One `Persist`: a record's bytes.
    fn record(&mut self, bytes: &[u8]) -> io::Result<()>;
    fn put(&mut self, name: &str, bytes: &[u8]) -> io::Result<()>;
    fn get(&mut self, name: &str) -> io::Result<Option<Vec<u8>>>;
}

/// The records of a chain read in order up to the first that does not read
/// or link; and how many of them that is.
fn read(snapshot: Option<&[u8]>, records: &[&[u8]]) -> (Stored, usize) {
    let snapshot = snapshot.and_then(|b| match Record::dec(b) {
        Ok(Record::Snapshot(x)) => Some(x),
        _ => None,
    });
    let mut s = Stored { snapshot, chain: vec![] };
    let (mut seq, mut prev) = s.snapshot.as_ref().map_or((0, Default::default()), |x| (x.seq, x.hash));
    for b in records {
        match Record::dec(b) {
            Ok(Record::Logged(l)) if l.seq == seq + 1 && l.links(&prev) => {
                (seq, prev) = (l.seq, l.hash);
                s.chain.push(l);
            }
            _ => break,
        }
    }
    let n = s.chain.len();
    (s, n)
}

fn snapshot_of(bytes: &[u8]) -> bool {
    matches!(bytes.first(), Some(1))
}

/// The records in memory: what a simulated device keeps across a reopen.
#[derive(Clone, Debug, Default)]
pub struct Memory {
    pub snapshot: Option<Vec<u8>>,
    pub chain: Vec<Vec<u8>>,
    pub blobs: BTreeMap<String, Vec<u8>>,
}

impl Disk for Memory {
    fn load(&mut self) -> io::Result<Stored> {
        let records: Vec<&[u8]> = self.chain.iter().map(|b| &b[..]).collect();
        let (s, n) = read(self.snapshot.as_deref(), &records);
        self.chain.truncate(n);
        Ok(s)
    }

    fn record(&mut self, bytes: &[u8]) -> io::Result<()> {
        if snapshot_of(bytes) {
            self.snapshot = Some(bytes.to_vec());
            self.chain.clear();
        } else {
            self.chain.push(bytes.to_vec());
        }
        Ok(())
    }

    fn put(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        self.blobs.insert(name.into(), bytes.to_vec());
        Ok(())
    }

    fn get(&mut self, name: &str) -> io::Result<Option<Vec<u8>>> {
        Ok(self.blobs.get(name).cloned())
    }
}

/// The records in a directory: `snapshot`, `chain`, and a file per blob.
#[derive(Debug)]
pub struct Files {
    dir: PathBuf,
    chain: Option<File>,
    /// Whether every append is synced before it returns: a server's
    /// acknowledgement follows the write.
    pub sync: bool,
}

impl Files {
    pub fn new(dir: impl Into<PathBuf>) -> io::Result<Files> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        Ok(Files {
            dir,
            chain: None,
            sync: false,
        })
    }

    fn whole(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let tmp = self.dir.join(format!("{name}.tmp"));
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(tmp, self.dir.join(name))
    }

    fn opened(&mut self) -> io::Result<&mut File> {
        if self.chain.is_none() {
            self.chain = Some(OpenOptions::new().create(true).append(true).open(self.dir.join("chain"))?);
        }
        Ok(self.chain.as_mut().unwrap())
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
        let chain = file(self.dir.join("chain"))?.unwrap_or_default();
        let (mut records, mut ends, mut at) = (vec![], vec![0], 0);
        while let Some(len) = chain.get(at..at + 4).map(|b| u32::from_be_bytes(b.try_into().unwrap()) as usize) {
            let Some(b) = chain.get(at + 4..at + 4 + len) else { break };
            records.push(b);
            at += 4 + len;
            ends.push(at);
        }
        let snapshot = file(self.dir.join("snapshot"))?;
        let (s, n) = read(snapshot.as_deref(), &records);
        self.chain = None;
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(self.dir.join("chain"))?
            .set_len(ends[n] as u64)?;
        Ok(s)
    }

    fn record(&mut self, bytes: &[u8]) -> io::Result<()> {
        if snapshot_of(bytes) {
            self.whole("snapshot", bytes)?;
            self.chain = None;
            return File::create(self.dir.join("chain"))?.sync_all();
        }
        let sync = self.sync;
        let f = self.opened()?;
        f.write_all(&[&(bytes.len() as u32).to_be_bytes()[..], bytes].concat())?;
        if sync {
            f.sync_data()?;
        }
        Ok(())
    }

    fn put(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        self.whole(name, bytes)
    }

    fn get(&mut self, name: &str) -> io::Result<Option<Vec<u8>>> {
        file(self.dir.join(name))
    }
}
