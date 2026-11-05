//! Files beside the log (docs/plan-files.md): the bytes of what rows name
//! by [`FileRef`], kept by hash as blobs of a [`Disk`] and carried by
//! frames no machine reads (`Offer`, `Want`, `Chunk`, `Have`, `Absent`).
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
use std::io::{self, Read};

use ark::file::{is_hash, FileRef};
use ark::frames::Frame;

use crate::{ConnId, Deliver, Disk, Memory, Peer};

/// The most of a file one `Chunk` carries.
pub const CHUNK: usize = 1 << 20;

// Files are blobs of a [`Disk`] of their own ([`Disk::files`]), named by
// their hash: `<hash>` whole, `<hash>.part` arriving, and an empty
// `<hash>.unsent` beside one made here and not yet sent. A whole one is
// written in full or renamed from its part, so it is whole or not there.

fn not_a_name(hash: &str) -> io::Error {
    io::Error::other(format!("{hash:.80} is not a file's name"))
}

/// The blob of the file `hash` (`ext` "" whole, `.part`, `.unsent`).
fn blob(hash: &str, ext: &str) -> io::Result<String> {
    match is_hash(hash) {
        true => Ok(format!("{hash}{ext}")),
        false => Err(not_a_name(hash)),
    }
}

/// A whole file's size.
fn whole(d: &dyn Disk, hash: &str) -> Option<u64> {
    d.len(&blob(hash, "").ok()?)
}

/// At most `n` of a whole file's bytes from `at`.
fn bytes(d: &dyn Disk, hash: &str, at: u64, n: usize) -> io::Result<Vec<u8>> {
    d.read(&blob(hash, "")?, at, n)
}

/// How much of a file arriving is here.
fn partial(d: &dyn Disk, hash: &str) -> u64 {
    blob(hash, ".part").ok().and_then(|b| d.len(&b)).unwrap_or(0)
}

/// Bytes of a file arriving, at `at`, where what is here ends.
fn append(d: &mut dyn Disk, hash: &str, at: u64, bytes: &[u8]) -> io::Result<()> {
    if partial(d, hash) != at {
        return Err(io::Error::other("not where the file ends"));
    }
    d.append(&blob(hash, ".part")?, bytes)
}

/// A file arrived whole, if its bytes are its name's; otherwise it is
/// dropped, to be sent again.
fn finish(d: &mut dyn Disk, hash: &str) -> io::Result<()> {
    let part = blob(hash, ".part")?;
    if ark::file::hash_reader(Reading(d, &part, 0))? != hash {
        let _ = d.remove(&part);
        return Err(io::Error::other("the bytes are not the file they were sent as"));
    }
    d.rename(&part, &blob(hash, "")?)
}

/// A blob read in parts, as a reader.
struct Reading<'a>(&'a dyn Disk, &'a str, u64);

impl Read for Reading<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let b = self.0.read(self.1, self.2, buf.len())?;
        buf[..b.len()].copy_from_slice(&b);
        self.2 += b.len() as u64;
        Ok(b.len())
    }
}

/// A whole file, its name its bytes'.
fn put(d: &mut dyn Disk, bytes: &[u8]) -> io::Result<String> {
    let hash = ark::file::hash(bytes);
    if whole(d, &hash).is_none() {
        d.put(&blob(&hash, "")?, bytes)?;
    }
    Ok(hash)
}

/// The files made here not yet sent.
fn unsent(d: &dyn Disk) -> Vec<String> {
    let names = d.names(".unsent").into_iter();
    names
        .filter_map(|n| n.strip_suffix(".unsent").map(String::from))
        .filter(|h| is_hash(h))
        .collect()
}

/// Marked made here and not yet sent, or not.
fn mark(d: &mut dyn Disk, hash: &str, unsent: bool) -> io::Result<()> {
    let b = blob(hash, ".unsent")?;
    match unsent {
        true => d.put(&b, &[]),
        false => d.remove(&b),
    }
}

/// What a peer does with files: its store, and where each transfer is.
pub(crate) struct Files {
    pub store: Box<dyn Disk>,
    /// A device's: the files it was asked for and does not hold whole, and
    /// those of them wanted on this connection.
    wanted: BTreeSet<String>,
    asked: BTreeSet<String>,
    /// How many files have arrived whole: what rings a device's views.
    pub arrived: u64,
    /// Why the server would not give one, by file, until it does.
    pub absent: BTreeMap<String, String>,
    /// A server's: the connections that wanted a file before it arrived.
    waiting: BTreeMap<String, BTreeSet<ConnId>>,
    /// The files each connection was let read, so a file is judged once per
    /// connection, not once a chunk.
    let_read: BTreeMap<ConnId, BTreeSet<String>>,
}

impl Files {
    pub fn new(store: Box<dyn Disk>) -> Files {
        Files {
            store,
            wanted: BTreeSet::new(),
            asked: BTreeSet::new(),
            arrived: 0,
            absent: BTreeMap::new(),
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
fn chunk(store: &dyn Disk, hash: &str, at: u64) -> Option<Frame> {
    let size = whole(store, hash)?;
    let bytes = bytes(store, hash, at, CHUNK).ok()?;
    Some(Frame::Chunk {
        hash: hash.into(),
        at,
        size,
        bytes,
    })
}

/// A chunk of a file arriving, appended where what is here of it ends: where
/// the next is wanted from — after it, or, out of step (a chunk again after
/// a reconnect), from where the file stops — or none, the file whole; or why
/// its bytes are not its name's.
fn arriving(store: &mut dyn Disk, hash: &str, at: u64, size: u64, bytes: &[u8]) -> Result<Option<u64>, String> {
    let end = at + bytes.len() as u64;
    if append(store, hash, at, bytes).is_err() || bytes.is_empty() && end < size {
        return Ok(Some(partial(store, hash)));
    }
    match end < size {
        true => Ok(Some(end)),
        false => finish(store, hash).map(|_| None).map_err(|e| e.to_string()),
    }
}

impl Peer {
    /// The disk files are kept on: a device's is `files/` beside its own
    /// when that is a directory ([`crate::Disk::files`]).
    pub fn keep_files(&mut self, store: Box<dyn Disk>) {
        self.files.store = store;
    }

    /// A file made here: kept, marked unsent and offered to the server when
    /// there is one; its reference, for a row to name.
    pub fn put_file(&mut self, bytes: &[u8], media: &str) -> Result<FileRef, String> {
        let store = &mut *self.files.store;
        let hash = put(store, bytes)
            .and_then(|h| mark(store, &h, true).map(|_| h))
            .map_err(|e| e.to_string())?;
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
        let size = whole(&*self.files.store, &f.hash)?;
        bytes(&*self.files.store, &f.hash, 0, size as usize).ok()
    }

    /// [`Peer::file`], wanted from the server when it is not here.
    pub fn open_file(&mut self, f: &FileRef) -> Option<Vec<u8>> {
        if !self.have_files(std::slice::from_ref(&f.hash)) {
            self.flush();
            return None;
        }
        self.file(f)
    }

    /// Whether the server said why it would not give a file.
    pub fn file_absent(&self, f: &FileRef) -> Option<&str> {
        self.files.absent.get(&f.hash).map(String::as_str)
    }

    /// The files this device made that the server does not hold yet.
    pub fn files_unsent(&self) -> Vec<String> {
        unsent(&*self.files.store)
    }

    /// How many wanted files have arrived whole: what a UI redraws on.
    pub fn files_arrived(&self) -> u64 {
        self.files.arrived
    }

    fn want(&mut self, hash: &str) {
        // Here already: a store shared with the server's, which has it.
        if whole(&*self.files.store, hash).is_some() {
            if self.files.wanted.remove(hash) {
                self.files.absent.remove(hash);
                self.files.arrived += 1;
            }
            return;
        }
        if self.linked && self.files.asked.insert(hash.into()) {
            let at = partial(&*self.files.store, hash);
            self.frames.push(Frame::Want { hash: hash.into(), at }.enc());
        }
    }

    /// A connection up: every unsent file offered, every wanted one asked.
    pub(crate) fn files_connected(&mut self) {
        self.files.asked.clear();
        for hash in unsent(&*self.files.store) {
            match whole(&*self.files.store, &hash) {
                Some(size) => self.frames.push(Frame::Offer { hash, size }.enc()),
                None => drop(mark(&mut *self.files.store, &hash, false)),
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
                if unsent(&*self.files.store).contains(&hash) {
                    if let Some(c) = chunk(&*self.files.store, &hash, at) {
                        self.frames.push(c.enc());
                    }
                }
            }
            Frame::Chunk { hash, at, size, bytes } if self.files.wanted.contains(&hash) => {
                match arriving(&mut *self.files.store, &hash, at, size, &bytes) {
                    Ok(Some(at)) => return self.frames.push(Frame::Want { hash, at }.enc()),
                    Ok(None) => {
                        self.files.wanted.remove(&hash);
                        self.files.absent.remove(&hash);
                        self.files.arrived += 1;
                    }
                    Err(why) => drop(self.files.absent.insert(hash.clone(), why)),
                }
                self.files.asked.remove(&hash);
            }
            Frame::Have { hash } => {
                let _ = mark(&mut *self.files.store, &hash, false);
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
            Frame::Offer { hash, .. } => {
                if !is_hash(&hash) {
                    return absent(hash, "not a file's name");
                }
                if whole(&*files.store, &hash).is_some() {
                    return to(Frame::Have { hash });
                }
                let at = partial(&*files.store, &hash);
                to(Frame::Want { hash, at })
            }
            Frame::Chunk { hash, at, size, bytes } => match arriving(&mut *files.store, &hash, at, size, &bytes) {
                Ok(Some(at)) => to(Frame::Want { hash, at }),
                Ok(None) => {
                    let waiting = files.waiting.remove(&hash).unwrap_or_default();
                    let have = Frame::Have { hash }.enc();
                    std::iter::once(c).chain(waiting).map(|w| (w, have.clone())).collect()
                }
                Err(why) => absent(hash, &why),
            },
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
        let may = ark::file::held(&self.log.store, hash, self.log.reads(&who));
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
            if whole(&*self.files.store, h).is_none() {
                all = false;
                if is_hash(h) && self.files.wanted.insert(h.clone()) {
                    self.want(h);
                }
            }
        }
        all
    }
}
