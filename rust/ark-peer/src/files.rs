//! Files beside the log (docs/plan-files.md): the bytes of what rows name
//! by [`FileRef`], kept by hash in a [`Store`] and carried by frames no
//! machine reads (`Offer`, `Want`, `Chunk`, `Have`, `Absent`).
//!
//! **A device** makes a file ([`Peer::put_file`]) from a reader — a file on
//! disk, a buffer — streamed into its store and hashed as it goes: it is
//! there at once and marked unsent, and its reference goes in the app's
//! mutation, pending as any intent is. On every connection each unsent file
//! is offered and sent as the server wants it, from where the server's copy
//! stops. A file a row names is read from the store ([`Peer::file`]), as a
//! reader of its bytes, never whole; one not there is wanted, and the views
//! are rung when it is whole (`crate::link::Pumped::shown`).
//!
//! **A server** takes what is offered, up to [`crate::Config::max_file`],
//! into its store and checks it against its name, and gives a file to a
//! connection that may read a row naming it ([`ark::file::held`]); one
//! wanted before it has arrived is answered `Absent` and, when it does,
//! `Have`.
//!
//! Whoever takes a file asks for it a chunk at a time, [`WINDOW`] chunks
//! ahead of what it holds, so a transfer goes at the link's speed, not a
//! chunk a round trip; a part of a file nothing was written to for
//! [`STALE`] is dropped. The rows never wait for their files: an upload
//! goes beside them, so a large one never holds back a device's sync.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ark::file::{is_hash, FileRef};
use ark::frames::Frame;

use crate::{ConnId, Deliver, Peer};

/// The most of a file one `Chunk` carries.
pub const CHUNK: usize = 1 << 20;

/// How many chunks of a file its taker asks for ahead of what it holds.
pub const WINDOW: u64 = 8;

/// The largest file a server takes unless told ([`crate::Config::max_file`]).
pub const MAX_FILE: u64 = 16 << 30;

/// How long a part of a file is kept with nothing more of it written.
pub const STALE: Duration = Duration::from_secs(3 * 24 * 3600);

/// A device's transfers are on its one connection, the server's.
const SERVER: ConnId = 0;

/// A file's bytes, held whole: read from the start, or a seek's range.
pub trait Bytes: Read + Seek + Send {}
impl<T: Read + Seek + Send> Bytes for T {}

/// Where a peer's files are, by their hash: `<hash>` whole, `<hash>.part`
/// arriving, and an empty `<hash>.unsent` beside one made here and not yet
/// sent. A whole one is renamed from a part, so it is whole or not there.
/// A directory ([`crate::Disk::files`]: a device's `files/` beside its
/// disk, a server's `log/files`), or memory — a browser's, a test's.
pub enum Store {
    Dir(PathBuf),
    Memory(BTreeMap<String, Arc<Vec<u8>>>),
}

/// A blob in memory, read while it is kept.
struct Shared(Arc<Vec<u8>>);

impl AsRef<[u8]> for Shared {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Default for Store {
    fn default() -> Store {
        Store::Memory(BTreeMap::new())
    }
}

impl Store {
    pub fn dir(dir: impl Into<PathBuf>) -> io::Result<Store> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        Ok(Store::Dir(dir))
    }

    fn len(&self, name: &str) -> Option<u64> {
        match self {
            Store::Dir(d) => fs::metadata(d.join(name)).ok().map(|m| m.len()),
            Store::Memory(m) => m.get(name).map(|b| b.len() as u64),
        }
    }

    fn open(&self, name: &str) -> io::Result<Box<dyn Bytes>> {
        match self {
            Store::Dir(d) => Ok(Box::new(File::open(d.join(name))?)),
            Store::Memory(m) => Ok(Box::new(io::Cursor::new(Shared(m.get(name).ok_or(io::ErrorKind::NotFound)?.clone())))),
        }
    }

    fn append(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        match self {
            Store::Dir(d) => OpenOptions::new().create(true).append(true).open(d.join(name))?.write_all(bytes),
            Store::Memory(m) => Arc::make_mut(m.entry(name.into()).or_default()).write_all(bytes),
        }
    }

    /// `from` named `to`, durably.
    fn rename(&mut self, from: &str, to: &str) -> io::Result<()> {
        match self {
            Store::Dir(d) => File::open(d.join(from))?.sync_all().and_then(|_| fs::rename(d.join(from), d.join(to))),
            Store::Memory(m) => m.remove(from).map(|b| drop(m.insert(to.into(), b))).ok_or(io::ErrorKind::NotFound.into()),
        }
    }

    fn remove(&mut self, name: &str) {
        match self {
            Store::Dir(d) => drop(fs::remove_file(d.join(name))),
            Store::Memory(m) => drop(m.remove(name)),
        }
    }

    /// The names that end `suffix`.
    fn names(&self, suffix: &str) -> Vec<String> {
        let names: Vec<String> = match self {
            Store::Dir(d) => fs::read_dir(d)
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok()?.file_name().into_string().ok())
                .collect(),
            Store::Memory(m) => m.keys().cloned().collect(),
        };
        names.into_iter().filter(|n| n.ends_with(suffix)).collect()
    }

    /// Every part nothing was written to for [`STALE`], gone — an upload
    /// or a download given up, a put cut off — and when (`now_ms`).
    fn sweep(&self) -> i64 {
        if let Store::Dir(d) = self {
            for p in self.names(".part").into_iter().map(|n| d.join(n)) {
                let at = fs::metadata(&p).and_then(|m| m.modified());
                if at.is_ok_and(|t| t.elapsed().is_ok_and(|a| a > STALE)) {
                    let _ = fs::remove_file(p);
                }
            }
        }
        crate::now_ms()
    }
}

/// The blob of the file `hash` (`ext` "" whole, `.part`, `.unsent`).
fn blob(hash: &str, ext: &str) -> Result<String, String> {
    is_hash(hash)
        .then(|| format!("{hash}{ext}"))
        .ok_or_else(|| format!("{hash:.80} is not a file's name"))
}

/// A whole file's size.
fn whole(s: &Store, hash: &str) -> Option<u64> {
    s.len(&blob(hash, "").ok()?)
}

/// How much of a file arriving is here.
fn partial(s: &Store, hash: &str) -> u64 {
    blob(hash, ".part").ok().and_then(|b| s.len(&b)).unwrap_or(0)
}

/// The bytes read through it, appended to a blob as they go.
struct Keeping<'a, R>(R, &'a mut Store, &'a str);

impl<R: Read> Read for Keeping<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.0.read(buf)?;
        self.1.append(self.2, &buf[..n]).map(|_| n)
    }
}

/// A file made here, streamed from `from` into the store and hashed as it
/// goes, once, and marked unsent: its name and size.
fn put(s: &mut Store, from: impl Read) -> io::Result<(String, u64)> {
    let tmp = format!("{}.put.part", ark::hash::hex(&crate::draw_origin().0));
    let hash = ark::file::hash_reader(BufReader::with_capacity(CHUNK, Keeping(from, s, &tmp)));
    let size = s.len(&tmp).unwrap_or(0);
    let kept = hash.and_then(|h| {
        s.append(&format!("{h}.unsent"), &[])?;
        if whole(s, &h).is_none() {
            s.rename(&tmp, &h)?;
        }
        Ok(h)
    });
    s.remove(&tmp);
    Ok((kept?, size))
}

/// The file's bytes from `at`, as one chunk.
fn chunk(s: &Store, hash: &str, at: u64) -> Option<Frame> {
    let (size, hash, mut bytes) = (whole(s, hash)?, hash.to_string(), vec![]);
    let mut r = s.open(&hash).ok()?;
    r.seek(SeekFrom::Start(at)).ok()?;
    r.take(CHUNK as u64).read_to_end(&mut bytes).ok()?;
    Some(Frame::Chunk { hash, at, size, bytes })
}

/// The asks a transfer of a file of `size` makes: from `next`, where the
/// last one ended, up to [`WINDOW`] chunks past what is here — and one at
/// least, from where it stops, of a file whose size is not known yet
/// (none), or whose part is all here.
fn window(s: &Store, hash: &str, size: u64, next: &mut u64) -> Vec<Frame> {
    let (have, from) = (partial(s, hash), *next);
    let from = from.max(have);
    *next = size.max(have + 1).min(have + WINDOW * CHUNK as u64).max(from);
    (from..*next).step_by(CHUNK).map(|at| Frame::Want { hash: hash.into(), at }).collect()
}

/// A chunk of a file arriving, on a transfer whose asks reached `next`:
/// appended where what is here ends, and the asks that makes room for — or
/// none, the file whole; or why its bytes are not its name's. One out of
/// step (again, or after a gap) is dropped, and the file asked for again
/// from where it stops once the window's last ask is answered; a part
/// longer than the file is not of it, and goes.
fn arriving(s: &mut Store, hash: &str, at: u64, size: u64, bytes: &[u8], next: &mut u64) -> io::Result<Option<Vec<Frame>>> {
    let part = blob(hash, ".part").map_err(io::Error::other)?;
    if partial(s, hash) > size {
        s.remove(&part);
    }
    let (held, end) = (partial(s, hash), at + bytes.len() as u64);
    if at != held || end > size || bytes.is_empty() && end < size {
        *next = if at + CHUNK as u64 >= *next { held } else { *next };
        return Ok(Some(window(s, hash, size, next)));
    }
    s.append(&part, bytes)?;
    if end < size {
        return Ok(Some(window(s, hash, size, next)));
    }
    match ark::file::hash_reader(s.open(&part)?)? == hash {
        true => s.rename(&part, hash).map(|_| None),
        false => {
            s.remove(&part);
            Err(io::Error::other("the bytes are not the file they were sent as"))
        }
    }
}

/// What a peer does with files: its store, and where each transfer is.
#[derive(Default)]
pub(crate) struct Files {
    pub store: Store,
    /// A device's: the files it was asked for and does not hold whole.
    wanted: BTreeSet<String>,
    /// Each file coming in, by its connection (a device's, [`SERVER`]):
    /// where its next ask begins.
    asked: BTreeMap<(ConnId, String), u64>,
    /// Whether a wanted file arrived whole since the last pump: what rings
    /// a device's views.
    pub arrived: bool,
    /// Why the server would not give one, by file, until it does.
    pub absent: BTreeMap<String, String>,
    /// A server's: the connections that wanted a file before it arrived.
    waiting: BTreeMap<String, BTreeSet<ConnId>>,
    /// The files each connection was let read, so a file is judged once per
    /// connection, not once a chunk.
    let_read: BTreeMap<ConnId, BTreeSet<String>>,
    /// When parts were last swept (`now_ms`).
    swept: i64,
}

impl Peer {
    /// The store files are kept in: a device's is `files/` beside its disk
    /// when that is a directory ([`crate::Disk::files`]). Parts left
    /// [`STALE`] are dropped.
    pub fn keep_files(&mut self, store: Store) {
        self.files.swept = store.sweep();
        self.files.store = store;
    }

    /// A file made here, read from `from` (a file, a buffer) into the store
    /// as it is hashed: marked unsent and offered to the server when there
    /// is one; its reference, for a row to name.
    pub fn put_file(&mut self, from: impl Read, media: &str) -> Result<FileRef, String> {
        let (hash, size) = put(&mut self.files.store, from).map_err(|e| e.to_string())?;
        if self.linked {
            self.frames.push(Frame::Offer { hash: hash.clone(), size }.enc());
            self.flush();
        }
        let (size, media) = (size as i64, media.into());
        Ok(FileRef { hash, size, media })
    }

    /// A file's bytes when they are here whole, to read in order or from a
    /// seek; when not, wanted from the server (the views rung when they
    /// arrive) — or why the server would not give it, when it said.
    pub fn file(&mut self, f: &FileRef) -> Result<Option<Box<dyn Bytes>>, String> {
        if !self.have_files(std::slice::from_ref(&f.hash)) {
            self.flush();
            return self.files.absent.get(&f.hash).map_or(Ok(None), |why| Err(why.clone()));
        }
        self.files.store.open(&blob(&f.hash, "")?).map(Some).map_err(|e| e.to_string())
    }

    fn want(&mut self, hash: &str) {
        let files = &mut self.files;
        // Here already: a store shared with the server's, which has it.
        if whole(&files.store, hash).is_some() {
            if files.wanted.remove(hash) {
                files.absent.remove(hash);
                files.arrived = true;
            }
            return;
        }
        // Asked once a connection; how big it is is known from its first chunk.
        if let (true, Entry::Vacant(next)) = (self.linked, files.asked.entry((SERVER, hash.into()))) {
            self.frames.extend(window(&files.store, hash, 0, next.insert(0)).iter().map(Frame::enc));
        }
    }

    /// A connection up: every unsent file offered, every wanted one asked.
    pub(crate) fn files_connected(&mut self) {
        self.files.asked.clear();
        for hash in self.files.store.names(".unsent").iter().filter_map(|n| n.strip_suffix(".unsent")) {
            match whole(&self.files.store, hash) {
                Some(size) => self.frames.push(Frame::Offer { hash: hash.into(), size }.enc()),
                None => self.files.store.remove(&format!("{hash}.unsent")),
            }
        }
        let wanted: Vec<String> = self.files.wanted.iter().cloned().collect();
        wanted.iter().for_each(|h| self.want(h));
    }

    /// A file's frame from the server.
    pub(crate) fn file_told(&mut self, f: Frame) {
        let files = &mut self.files;
        match f {
            // The server taking an upload: the chunk it asks for.
            Frame::Want { hash, at } if blob(&hash, ".unsent").is_ok_and(|b| files.store.len(&b).is_some()) => {
                self.frames.extend(chunk(&files.store, &hash, at).map(|c| c.enc()));
            }
            Frame::Chunk { hash, at, size, bytes } if files.wanted.contains(&hash) => {
                let key = (SERVER, hash.clone());
                let next = files.asked.entry(key.clone()).or_default();
                match arriving(&mut files.store, &hash, at, size, &bytes, next) {
                    Ok(Some(asks)) => return self.frames.extend(asks.iter().map(Frame::enc)),
                    Ok(None) => {
                        files.wanted.remove(&hash);
                        files.absent.remove(&hash);
                        files.arrived = true;
                    }
                    Err(why) => drop(files.absent.insert(hash, why.to_string())),
                }
                files.asked.remove(&key);
            }
            Frame::Have { hash } if is_hash(&hash) => {
                files.store.remove(&format!("{hash}.unsent"));
                if files.wanted.contains(&hash) {
                    files.asked.remove(&(SERVER, hash.clone()));
                    self.want(&hash);
                }
            }
            // Wanted again when the server says it has it.
            Frame::Absent { hash, why } => {
                files.absent.insert(hash, why);
            }
            _ => {}
        }
    }

    /// A file's frame from a connection, as a server.
    pub(crate) fn file_frame(&mut self, c: ConnId, f: Frame) -> Vec<Deliver> {
        let to = |fs: Vec<Frame>| fs.iter().map(|f| (c, f.enc())).collect();
        let absent = |hash: String, why: &str| to(vec![Frame::Absent { hash, why: why.into() }]);
        let (max, files) = (self.config.max_file, &mut self.files);
        match f {
            Frame::Offer { hash, .. } | Frame::Chunk { hash, .. } if !is_hash(&hash) => absent(hash, "not a file's name"),
            Frame::Offer { hash, .. } | Frame::Chunk { hash, .. } if whole(&files.store, &hash).is_some() => to(vec![Frame::Have { hash }]),
            Frame::Offer { hash, size } | Frame::Chunk { hash, size, .. } if size > max => {
                files.store.remove(&format!("{hash}.part"));
                tracing::info!(target: "ark", file = %hash, size, max, "file refused");
                absent(hash, &format!("larger than the {max} bytes a file may be here"))
            }
            Frame::Offer { hash, size } => {
                // Swept once an hour, as uploads begin.
                if crate::now_ms() > files.swept + 3_600_000 {
                    files.swept = files.store.sweep();
                }
                let next = files.asked.entry((c, hash.clone())).or_default();
                *next = 0;
                to(window(&files.store, &hash, size, next))
            }
            Frame::Chunk { hash, at, size, bytes } => {
                let next = files.asked.entry((c, hash.clone())).or_default();
                match arriving(&mut files.store, &hash, at, size, &bytes, next) {
                    Ok(Some(asks)) => to(asks),
                    Ok(None) => {
                        let waiting = files.waiting.remove(&hash).unwrap_or_default();
                        let have = Frame::Have { hash }.enc();
                        std::iter::once(c).chain(waiting).map(|w| (w, have.clone())).collect()
                    }
                    Err(why) => absent(hash, &why.to_string()),
                }
            }
            Frame::Want { hash, at } => {
                if !self.may_read(c, &hash) {
                    return absent(hash, "no row this connection reads names it");
                }
                let files = &mut self.files;
                match chunk(&files.store, &hash, at) {
                    Some(f) => to(vec![f]),
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

    /// A connection gone: what it waited for, and took, forgotten.
    pub(crate) fn files_left(&mut self, c: ConnId) {
        self.files.let_read.remove(&c);
        self.files.asked.retain(|(k, _), _| *k != c);
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
            if whole(&self.files.store, h).is_none() {
                all = false;
                if is_hash(h) && self.files.wanted.insert(h.clone()) {
                    self.want(h);
                }
            }
        }
        all
    }
}
