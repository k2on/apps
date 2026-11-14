//! Files beside the log (docs/plan-files.md): the bytes of what rows name
//! by [`FileRef`], kept by hash in a [`Store`] and carried by frames no
//! machine reads (`Offer`, `Want`, `Chunk`, `Have`, `Absent`).
//!
//! **A device** makes a file ([`Peer::put_file`]): it is in its store at
//! once and marked unsent, and its reference goes in the app's mutation,
//! pending as any intent is. On every connection each unsent file is
//! offered and sent a chunk at a time as the server wants it, from where
//! the server's copy stops. A file a row names is read from the store
//! ([`Peer::file`]); one not there is wanted, a chunk at a time, and the
//! views are rung when it is whole ([`Peer::files_arrived`]).
//!
//! **A server** takes what is offered into its store and checks it against
//! its name, and gives a file to a connection that may read a row naming
//! it ([`ark::file::held`]); one wanted before it has arrived is answered
//! `Absent` and, when it does, `Have`.
//!
//! The rows never wait for their files: an upload goes beside them, so a
//! large one never holds back a device's sync.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use ark::file::{is_hash, FileRef};
use ark::frames::Frame;
use ark::peer::Feed;

use crate::{ConnId, Deliver, Peer};

/// The most of a file one `Chunk` carries.
pub const CHUNK: usize = 1 << 20;

/// Files by hash: whole ones, ones arriving, and the ones made here not
/// yet sent.
pub trait Store: Send {
    /// A whole file's size.
    fn len(&self, hash: &str) -> Option<u64>;
    /// At most `n` of a whole file's bytes from `at`.
    fn read(&self, hash: &str, at: u64, n: usize) -> io::Result<Vec<u8>>;
    /// How much of a file arriving is here.
    fn partial(&self, hash: &str) -> u64;
    /// Bytes of a file arriving, at `at`, where what is here ends.
    fn append(&mut self, hash: &str, at: u64, bytes: &[u8]) -> io::Result<()>;
    /// A file arrived whole, if its bytes are its name's; otherwise it is
    /// dropped, to be sent again.
    fn finish(&mut self, hash: &str) -> io::Result<()>;
    /// A whole file, its name its bytes'.
    fn put(&mut self, bytes: &[u8]) -> io::Result<String>;
    /// The files made here not yet sent.
    fn unsent(&self) -> Vec<String>;
    /// Marked made here and not yet sent, or not.
    fn mark(&mut self, hash: &str, unsent: bool) -> io::Result<()>;
    /// Where a whole file is on the disk, for what reads it there.
    fn path(&self, _hash: &str) -> Option<PathBuf> {
        None
    }
}

fn not_a_name(hash: &str) -> io::Error {
    io::Error::other(format!("{hash:.80} is not a file's name"))
}

fn not_its_bytes() -> io::Error {
    io::Error::other("the bytes are not the file they were sent as")
}

/// Files in memory: a simulated device's, and the web build's until a page
/// closes.
#[derive(Default)]
pub struct Memory {
    whole: BTreeMap<String, Vec<u8>>,
    arriving: BTreeMap<String, Vec<u8>>,
    unsent: BTreeSet<String>,
}

impl Store for Memory {
    fn len(&self, hash: &str) -> Option<u64> {
        self.whole.get(hash).map(|b| b.len() as u64)
    }

    fn read(&self, hash: &str, at: u64, n: usize) -> io::Result<Vec<u8>> {
        let b = self.whole.get(hash).ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        let at = (at as usize).min(b.len());
        Ok(b[at..(at + n).min(b.len())].to_vec())
    }

    fn partial(&self, hash: &str) -> u64 {
        self.arriving.get(hash).map_or(0, |b| b.len() as u64)
    }

    fn append(&mut self, hash: &str, at: u64, bytes: &[u8]) -> io::Result<()> {
        if self.partial(hash) != at {
            return Err(io::Error::other("not where the file ends"));
        }
        self.arriving.entry(hash.into()).or_default().extend_from_slice(bytes);
        Ok(())
    }

    fn finish(&mut self, hash: &str) -> io::Result<()> {
        let b = self.arriving.remove(hash).unwrap_or_default();
        if ark::file::hash(&b) != hash {
            return Err(not_its_bytes());
        }
        self.whole.insert(hash.into(), b);
        Ok(())
    }

    fn put(&mut self, bytes: &[u8]) -> io::Result<String> {
        let hash = ark::file::hash(bytes);
        self.whole.insert(hash.clone(), bytes.to_vec());
        Ok(hash)
    }

    fn unsent(&self) -> Vec<String> {
        self.unsent.iter().cloned().collect()
    }

    fn mark(&mut self, hash: &str, unsent: bool) -> io::Result<()> {
        match unsent {
            true => self.unsent.insert(hash.into()),
            false => self.unsent.remove(hash),
        };
        Ok(())
    }
}

/// Files in a directory: `<hash>` whole, `<hash>.part` arriving,
/// `<hash>.unsent` beside one made here and not yet sent. A whole one is
/// written to its part and renamed, so it is whole or not there.
#[derive(Clone, Debug)]
pub struct Dir(pub PathBuf);

impl Dir {
    pub fn new(dir: impl Into<PathBuf>) -> io::Result<Dir> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        Ok(Dir(dir))
    }

    /// A server's files, under its data directory (`Serve::data`).
    pub fn of_server(data: impl Into<PathBuf>) -> io::Result<Dir> {
        Dir::new(data.into().join("log").join("files"))
    }

    fn at(&self, hash: &str, ext: &str) -> io::Result<PathBuf> {
        if !is_hash(hash) {
            return Err(not_a_name(hash));
        }
        Ok(self.0.join(format!("{hash}{ext}")))
    }
}

impl Store for Dir {
    fn len(&self, hash: &str) -> Option<u64> {
        fs::metadata(self.at(hash, "").ok()?).ok().map(|m| m.len())
    }

    fn read(&self, hash: &str, at: u64, n: usize) -> io::Result<Vec<u8>> {
        let mut f = fs::File::open(self.at(hash, "")?)?;
        f.seek(SeekFrom::Start(at))?;
        let mut b = Vec::with_capacity(n);
        f.take(n as u64).read_to_end(&mut b)?;
        Ok(b)
    }

    fn partial(&self, hash: &str) -> u64 {
        self.at(hash, ".part").ok().and_then(|p| fs::metadata(p).ok()).map_or(0, |m| m.len())
    }

    fn append(&mut self, hash: &str, at: u64, bytes: &[u8]) -> io::Result<()> {
        let path = self.at(hash, ".part")?;
        let mut f = fs::OpenOptions::new().create(true).append(true).open(&path)?;
        if f.metadata()?.len() != at {
            return Err(io::Error::other("not where the file ends"));
        }
        f.write_all(bytes)
    }

    fn finish(&mut self, hash: &str) -> io::Result<()> {
        let part = self.at(hash, ".part")?;
        let f = fs::File::open(&part)?;
        if ark::file::hash_reader(f)? != hash {
            let _ = fs::remove_file(&part);
            return Err(not_its_bytes());
        }
        fs::File::open(&part)?.sync_all()?;
        fs::rename(part, self.at(hash, "")?)
    }

    fn put(&mut self, bytes: &[u8]) -> io::Result<String> {
        let hash = ark::file::hash(bytes);
        if self.len(&hash).is_none() {
            let part = self.at(&hash, ".part")?;
            let mut f = fs::File::create(&part)?;
            f.write_all(bytes)?;
            f.sync_all()?;
            fs::rename(part, self.at(&hash, "")?)?;
        }
        Ok(hash)
    }

    fn unsent(&self) -> Vec<String> {
        let Ok(rd) = fs::read_dir(&self.0) else { return vec![] };
        let names = rd.filter_map(|e| e.ok()?.file_name().into_string().ok());
        names
            .filter_map(|n| n.strip_suffix(".unsent").map(String::from))
            .filter(|h| is_hash(h))
            .collect()
    }

    fn mark(&mut self, hash: &str, unsent: bool) -> io::Result<()> {
        let path = self.at(hash, ".unsent")?;
        match unsent {
            true => fs::write(path, []),
            false => match fs::remove_file(path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            },
        }
    }

    fn path(&self, hash: &str) -> Option<PathBuf> {
        self.len(hash).and_then(|_| self.at(hash, "").ok())
    }
}

/// What a peer does with files: its store, and where each transfer is.
pub(crate) struct Files {
    pub store: Box<dyn Store>,
    /// A device's: the files it was asked for and does not hold whole, and
    /// those of them wanted on this connection.
    wanted: BTreeSet<String>,
    asked: BTreeSet<String>,
    /// How many files have arrived whole: what rings a device's views.
    pub arrived: u64,
    /// Why the server would not give one, by file, until it does.
    pub absent: BTreeMap<String, String>,
    /// A server's: each file arriving and its size, and the connections
    /// that wanted one before it arrived.
    uploads: BTreeMap<String, u64>,
    waiting: BTreeMap<String, BTreeSet<ConnId>>,
    /// The files each connection was let read, so a file is judged once per
    /// connection, not once a chunk.
    let_read: BTreeMap<ConnId, BTreeSet<String>>,
}

impl Files {
    pub fn new(store: Box<dyn Store>) -> Files {
        Files {
            store,
            wanted: BTreeSet::new(),
            asked: BTreeSet::new(),
            arrived: 0,
            absent: BTreeMap::new(),
            uploads: BTreeMap::new(),
            waiting: BTreeMap::new(),
            let_read: BTreeMap::new(),
        }
    }
}

impl Default for Files {
    fn default() -> Files {
        Files::new(Box::new(Memory::default()))
    }
}

/// The file's bytes from `at`, as one chunk.
fn chunk(store: &dyn Store, hash: &str, at: u64) -> Option<Frame> {
    let size = store.len(hash)?;
    let bytes = store.read(hash, at, CHUNK).ok()?;
    Some(Frame::Chunk {
        hash: hash.into(),
        at,
        size,
        bytes,
    })
}

impl Peer {
    /// The store files are kept in: a device's is `files/` beside its disk
    /// when it has a directory ([`crate::Disk::files`]).
    pub fn keep_files(&mut self, store: Box<dyn Store>) {
        self.files.store = store;
    }

    /// A file made here: kept, marked unsent and offered to the server when
    /// there is one; its reference, for a row to name.
    pub fn put_file(&mut self, bytes: &[u8], media: &str) -> Result<FileRef, String> {
        let hash = self.files.store.put(bytes).map_err(|e| e.to_string())?;
        self.files.store.mark(&hash, true).map_err(|e| e.to_string())?;
        if self.linked {
            let size = bytes.len() as u64;
            self.frames.push(Frame::Offer { hash, size }.enc());
            self.flush();
        }
        Ok(FileRef::of(bytes, media))
    }

    /// A file's bytes, when they are here; when not, wanted from the
    /// server, and the views rung when they arrive.
    pub fn file(&self, f: &FileRef) -> Option<Vec<u8>> {
        let size = self.files.store.len(&f.hash)?;
        self.files.store.read(&f.hash, 0, size as usize).ok()
    }

    /// [`Peer::file`], wanted from the server when it is not here.
    pub fn open_file(&mut self, f: &FileRef) -> Option<Vec<u8>> {
        if let Some(b) = self.file(f) {
            return Some(b);
        }
        if is_hash(&f.hash) && self.files.wanted.insert(f.hash.clone()) {
            self.want(&f.hash);
            self.flush();
        }
        None
    }

    /// Where a whole file is on the disk, for what reads it there.
    pub fn file_path(&self, f: &FileRef) -> Option<PathBuf> {
        self.files.store.path(&f.hash)
    }

    /// Where a screen reads a file it shows while it is here
    /// (`ark::views::Views::file_url`): its `file:` URL on a disk, its
    /// bytes as a `data:` URL in memory; none while it is not here.
    pub fn file_url(&self, f: &FileRef) -> Option<String> {
        match self.file_path(f) {
            Some(p) => Some(ark::file::path_url(&p.to_string_lossy())),
            None => self.file(f).map(|b| ark::file::data_url(&f.media, &b)),
        }
    }

    /// Whether the server said why it would not give a file.
    pub fn file_absent(&self, f: &FileRef) -> Option<&str> {
        self.files.absent.get(&f.hash).map(String::as_str)
    }

    /// The files this device made that the server does not hold yet.
    pub fn files_unsent(&self) -> Vec<String> {
        self.files.store.unsent()
    }

    /// How many wanted files have arrived whole: what a UI redraws on.
    pub fn files_arrived(&self) -> u64 {
        self.files.arrived
    }

    fn want(&mut self, hash: &str) {
        // Here already: a store shared with the server's, which has it.
        if self.files.store.len(hash).is_some() {
            if self.files.wanted.remove(hash) {
                self.files.absent.remove(hash);
                self.files.arrived += 1;
            }
            return;
        }
        if self.linked && self.files.asked.insert(hash.into()) {
            let at = self.files.store.partial(hash);
            self.frames.push(Frame::Want { hash: hash.into(), at }.enc());
        }
    }

    /// A connection up: every unsent file offered, every wanted one asked.
    pub(crate) fn files_connected(&mut self) {
        self.files.asked.clear();
        for hash in self.files.store.unsent() {
            match self.files.store.len(&hash) {
                Some(size) => self.frames.push(Frame::Offer { hash, size }.enc()),
                None => drop(self.files.store.mark(&hash, false)),
            }
        }
        let wanted: Vec<String> = self.files.wanted.iter().cloned().collect();
        wanted.iter().for_each(|h| self.want(h));
    }

    /// A file's frame from the server.
    pub(crate) fn file_told(&mut self, f: Frame) {
        match f {
            // The server taking an upload: the next chunk of it.
            Frame::Want { hash, at } => {
                if self.files.store.unsent().contains(&hash) {
                    if let Some(c) = chunk(&*self.files.store, &hash, at) {
                        self.frames.push(c.enc());
                    }
                }
            }
            Frame::Chunk { hash, at, size, bytes } => {
                if !self.files.wanted.contains(&hash) {
                    return;
                }
                let end = at + bytes.len() as u64;
                if self.files.store.append(&hash, at, &bytes).is_err() || bytes.is_empty() && end < size {
                    // Out of step (a chunk again after a reconnect): from
                    // where the file stops.
                    self.files.asked.remove(&hash);
                    return self.want(&hash);
                }
                if end < size {
                    self.frames.push(Frame::Want { hash, at: end }.enc());
                    return;
                }
                self.files.asked.remove(&hash);
                match self.files.store.finish(&hash) {
                    Ok(()) => {
                        self.files.wanted.remove(&hash);
                        self.files.absent.remove(&hash);
                        self.files.arrived += 1;
                    }
                    Err(e) => {
                        self.files.absent.insert(hash, e.to_string());
                    }
                }
            }
            Frame::Have { hash } => {
                let _ = self.files.store.mark(&hash, false);
                if self.files.wanted.contains(&hash) {
                    self.files.asked.remove(&hash);
                    self.want(&hash);
                }
            }
            // Wanted again when the server says it has it.
            Frame::Absent { hash, why } => {
                self.files.absent.insert(hash, why);
            }
            _ => {}
        }
    }

    /// A file's frame from a connection, as a server.
    pub(crate) fn file_frame(&mut self, c: ConnId, f: Frame) -> Vec<Deliver> {
        let to = |f: Frame| vec![(c, f.enc())];
        let absent = |hash: String, why: &str| to(Frame::Absent { hash, why: why.into() });
        let files = &mut self.files;
        match f {
            Frame::Offer { hash, size } => {
                if !is_hash(&hash) {
                    return absent(hash, "not a file's name");
                }
                if files.store.len(&hash).is_some() {
                    return to(Frame::Have { hash });
                }
                files.uploads.insert(hash.clone(), size);
                let at = files.store.partial(&hash);
                to(Frame::Want { hash, at })
            }
            Frame::Chunk { hash, at, bytes, .. } => {
                let Some(&size) = files.uploads.get(&hash) else { return vec![] };
                let end = at + bytes.len() as u64;
                if files.store.append(&hash, at, &bytes).is_err() || bytes.is_empty() && end < size {
                    let at = files.store.partial(&hash);
                    return to(Frame::Want { hash, at });
                }
                if end < size {
                    return to(Frame::Want { hash, at: end });
                }
                files.uploads.remove(&hash);
                if let Err(e) = files.store.finish(&hash) {
                    return absent(hash, &e.to_string());
                }
                let waiting = files.waiting.remove(&hash).unwrap_or_default();
                let have = Frame::Have { hash }.enc();
                std::iter::once(c).chain(waiting).map(|w| (w, have.clone())).collect()
            }
            Frame::Want { hash, at } => {
                if !self.may_read(c, &hash) {
                    return absent(hash, "no row this connection reads names it");
                }
                let files = &mut self.files;
                match chunk(&*files.store, &hash, at) {
                    Some(f) => to(f),
                    None => {
                        files.waiting.entry(hash.clone()).or_default().insert(c);
                        absent(hash, "not here yet")
                    }
                }
            }
            _ => vec![],
        }
    }

    /// Whether a connection may read a file: a row it reads names it.
    fn may_read(&mut self, c: ConnId, hash: &str) -> bool {
        if self.files.let_read.get(&c).is_some_and(|s| s.contains(hash)) {
            return true;
        }
        let Some(who) = self.who(c) else { return false };
        let store = match (self.config.feed)(&who) {
            Feed::Whole => Some(&self.log.store),
            Feed::Scoped(_) => self.log.served.get(&who.session).map(|s| &s.union),
        };
        let may = store.is_some_and(|s| ark::file::held(s, hash));
        if may {
            self.files.let_read.entry(c).or_default().insert(hash.into());
        }
        may
    }

    /// A connection gone: what it waited for, forgotten.
    pub(crate) fn files_left(&mut self, c: ConnId) {
        self.files.let_read.remove(&c);
        self.files.waiting.values_mut().for_each(|w| {
            w.remove(&c);
        });
    }
}

/// The files a row names.
pub(crate) fn named<R: ark::row::Row>(r: &R) -> Vec<String> {
    let t = R::table();
    let ty = t.row_type();
    if !ark::file::mentions(&ty) {
        return vec![];
    }
    let mut out = vec![];
    if let Ok(v) = ark::dynamic::row(&t, &ark::format::Encode::to_bytes(r)) {
        ark::file::named(&ty, &v, &mut out);
    }
    out
}

impl Peer {
    /// Whether every one of these files is here; those that are not,
    /// wanted.
    pub(crate) fn have_files(&mut self, hashes: &[String]) -> bool {
        let mut all = true;
        for h in hashes {
            if self.files.store.len(h).is_none() {
                all = false;
                if is_hash(h) && self.files.wanted.insert(h.clone()) {
                    self.want(h);
                }
            }
        }
        all
    }
}
