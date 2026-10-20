//! The media directory, as a peer.
//!
//! A server that owns a folder of files has to get them into the log
//! somehow, and there are only two honest ways: write the rows directly, or
//! be a peer and author mutations like everything else. This is the second,
//! and it is not a preference — writing rows would be a second definition of
//! what "add a song" means, and the first time it disagreed with the domain's
//! procedure the replicas would diverge with nothing to say so.
//!
//! So the scanner is an ordinary [`ark5_client::Client`], with its own
//! replica and its own pending intents on disk, its link dialling the hub
//! in this process (`Handle::dial`) instead of a socket. It gets its
//! confirmations, it gets the rebase, and a file it adds fans out to every
//! connected peer without anyone asking. It signs in like everyone else — the engine holds every
//! entry to a session, and "the server wrote it" is not an exemption — as
//! the account [`ACCOUNT`].
//!
//! **A file is one song, and that is not this file's doing.** `add_song` is
//! an upsert by `file`, inside the procedure: a file the library already has
//! is that song said again, retagged or with a cover it did not have, and
//! the same file saying nothing new writes nothing — for every peer
//! replaying the log, not only for the scanner that authored it.
//!
//! What this file adds is not asking. It keeps, beside its replica, what
//! each file was when it was last offered — its size, when it was modified,
//! and the same of the covers it was offered with ([`Prints`]). A file whose
//! print has not moved, and which the replica has as a song, is not read at
//! all; one whose print has moved is read, and authored only if the entry
//! would change something the replica holds. So a restart over ten thousand
//! untouched files reads no tags and authors nothing, a retagged file is one
//! entry, and the first start of a server that never kept prints reads
//! every file once — which is what gives a library scanned before covers
//! were found its covers. The prints are this server's, never the log's: a
//! missing or unreadable file of them is a full read, and nothing worse.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use ark5::ir::Node;
use ark5::log::EntryId;
use ark5::peer::{Ctx, Who};
use ark5::store::{Dir, Read, Scan as Range, Store};
use ark5_client::link::{Backoff, Link};
use ark5_client::{Client, Event, Files};
use ark5_server::handle::now;
use ark5_server::Handle;
use ark_auth::Login;
use harken_domain5::library::AddSong;
use notify::{RecursiveMode, Watcher};

use crate::covers::{self, Covers};
use crate::hosted::Hosted;

/// What the scanner will look at. Everything else in the directory —
/// artwork, playlists, the stray `.DS_Store` — is not a track and is skipped
/// in silence.
pub const AUDIO: &[&str] = &[
    "mp3", "flac", "ogg", "oga", "opus", "m4a", "m4b", "aac", "wav", "wv", "aiff", "aif",
];

/// Where music lives under the media root.
///
/// The root is kind-neutral because `file` is: one directory holds every
/// kind, `/media/` serves all of it, and an episode or a sermon gets a
/// sibling of this rather than a second root and a second URL prefix. So a
/// song's path reads `music/Bach/air.wav`, and a file outside this
/// subdirectory is not a track however much it sounds like one.
pub const MUSIC: &str = "music";

/// The account a scanned track is authored as. A real name would be a lie —
/// nobody added these by hand — and it shows up as the `user_id` on every
/// scanned row, which is the truth: the library put it there.
pub const ACCOUNT: &str = "library";

/// The mutation a file is offered as.
const ADD_SONG: &str = "library.add_song";

/// How long a watched path is left to settle before it is read: a file is
/// usually still being written when its create arrives, so the tags are not
/// there yet. Every event inside the window is one look.
const SETTLE: Duration = Duration::from_millis(400);

/// How often the peer is pumped when nothing is happening, so the log other
/// peers write does not pile up in its channel and "already a song" stays
/// true.
const IDLE: Duration = Duration::from_millis(250);

/// Why the scanner woke up.
enum Nudge {
    /// Look at everything. Sent once at boot, because whatever happened
    /// while the server was down produced no events to watch.
    All,
    /// Look at one path the watcher saw appear, change or — for a picture
    /// — go.
    One(PathBuf),
}

/// Where the scanner keeps what is its own under the server's data: its
/// replica, the prints of the files it has offered, and the pictures it
/// took out of them.
pub fn replica_dir(data: &Path) -> PathBuf {
    data.join("library")
}

/// The prints ([`Prints`]): a file of this server's, not of the log's.
pub fn prints_file(data: &Path) -> PathBuf {
    data.join("library-files.json")
}

/// The pictures taken out of files, which `/media/.art/` serves
/// ([`covers::routes`]).
pub fn art_dir(data: &Path) -> PathBuf {
    data.join("art")
}

/// The scanner, running. Dropping it stops the watch and the thread, and
/// takes its peer off the hub.
pub struct Scanner {
    /// Kept alive for as long as the scanner is: a watcher nobody holds
    /// stops at once. Declared first, so it is dropped first and its clone
    /// of the channel goes with it.
    _watcher: Option<notify::RecommendedWatcher>,
    tx: Sender<Nudge>,
    stop: Arc<AtomicBool>,
    counts: Arc<Counts>,
    thread: Option<JoinHandle<()>>,
}

/// What the scanner has done since it started, for what it prints and for
/// the tests.
#[derive(Default)]
struct Counts {
    /// Entries authored.
    authored: AtomicUsize,
    /// Files whose tags were read.
    read: AtomicUsize,
}

impl Drop for Scanner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Scanner {
    /// Scan `music/` under `root` and watch `root`, authoring through the
    /// hub as `login`, with the scanner's own state — the replica, the
    /// prints, the pictures taken out of files — under `data`
    /// ([`replica_dir`], [`prints_file`], [`art_dir`]).
    ///
    /// The walk is narrow and the watch is wide, on purpose: only `music/`
    /// holds tracks, but watching the root is what notices that directory
    /// being created at all — on a fresh install it may not exist yet, and a
    /// watch on a path that is not there watches nothing forever.
    ///
    /// Returns at once: the walk happens on a thread of its own, so a
    /// library of ten thousand files does not hold up anything.
    pub fn start(
        root: PathBuf,
        hosted: Hosted,
        hub: Handle,
        data: PathBuf,
        login: Login,
    ) -> Result<Scanner> {
        // `docs/plan-auth.md` The scanner is the library, by construction: it
        // holds the library's role whatever else its login holds, and its
        // entries carry it. The server grants the account the same role
        // (`start`), so the device and the authority agree on it — which is
        // what `is_library` asks, on the device and, of the roles the hub
        // stamps, at the authority.
        let mut roles = login.user.roles.clone();
        roles.push(harken_domain5::schema::LIBRARY.to_string());
        roles.sort();
        roles.dedup();
        let who = Who {
            actor: login.user.id.clone(),
            session: login.session.clone(),
            roles,
        };
        let dir = replica_dir(&data);
        let disk = Files::new(&dir)
            .map_err(|e| anyhow::anyhow!("the scanner's replica in {}: {e}", dir.display()))?;
        let config = ark5_client::Config {
            log: hosted.module.clone(),
            live: None,
        };
        let mut client = Client::open(config, who.clone(), Box::new(disk), false);
        // Each start is a login of its own: what the replica kept pending
        // is authored again under it, from its session's first `local`,
        // or the hub would hold the first entry for the ones before it.
        client.sign_in(who.clone());
        // In this process, the token is the scanner's and proves its login.
        let token = login.token.clone();
        let link = Link::start(
            Box::new(hub.dial(std::sync::Arc::new(move |t: &str| {
                (t == token).then(|| who.clone())
            }))),
            login.token.clone(),
            Backoff::default(),
        );
        let peer = Peer {
            client,
            link,
            hosted,
        };
        let (tx, rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let counts = Arc::new(Counts::default());
        let thread = {
            let (stop, counts) = (stop.clone(), counts.clone());
            let scan = Scan {
                music: root.join(MUSIC),
                root: root.clone(),
                art: art_dir(&data),
                prints: Prints::load(prints_file(&data)),
            };
            std::thread::Builder::new()
                .name("harken-scanner".into())
                .spawn(move || run(scan, peer, hub, rx, &stop, &counts))
                .context("starting the scanner")?
        };
        tx.send(Nudge::All)
            .context("the scanner stopped before it started")?;

        // The watch is best-effort: a directory that cannot be watched is
        // still scanned at boot, and saying so is better than refusing to
        // start over it.
        let events = tx.clone();
        let watcher =
            match notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                let Ok(event) = event else { return };
                // Creates and writes, and a picture going. A removed *track* is
                // not handled on purpose: the log is permanent, and a file
                // disappearing is not evidence that somebody meant to delete
                // the song — an unplugged disk looks exactly the same. A
                // removed cover is only a directory whose tracks are offered
                // again, with whatever cover they have now.
                let gone = matches!(event.kind, notify::EventKind::Remove(_));
                if !gone
                    && !matches!(
                        event.kind,
                        notify::EventKind::Create(_) | notify::EventKind::Modify(_)
                    )
                {
                    return;
                }
                for path in event.paths {
                    if !gone || covers::is_image(&path) {
                        let _ = events.send(Nudge::One(path));
                    }
                }
            }) {
                Ok(mut w) => match w.watch(&root, RecursiveMode::Recursive) {
                    Ok(()) => Some(w),
                    Err(e) => {
                        eprintln!(
                            "harken-server: library: cannot watch {}: {e}",
                            root.display()
                        );
                        None
                    }
                },
                Err(e) => {
                    eprintln!("harken-server: library: no filesystem watch available: {e}");
                    None
                }
            };

        Ok(Scanner {
            _watcher: watcher,
            tx,
            stop,
            counts,
            thread: Some(thread),
        })
    }

    /// Walk the whole directory again. Safe at any time: a file whose print
    /// has not moved is not read, and one that has is authored only if it
    /// says something new.
    pub fn rescan(&self) {
        let _ = self.tx.send(Nudge::All);
    }

    /// How many entries this scanner has authored since it started.
    pub fn authored(&self) -> usize {
        self.counts.authored.load(Ordering::Relaxed)
    }

    /// How many files' tags this scanner has read since it started.
    pub fn read(&self) -> usize {
        self.counts.read.load(Ordering::Relaxed)
    }
}

/// What one file was when it was last offered: its size and modification
/// time, and [`covers::Found::print`] of the pictures beside it. Any of the
/// three moving is a reason to read it again. The artist its tags named is
/// kept too, because whose a picture in the directory above is depends on
/// it — so the pictures are looked at as that artist's until the tags are
/// read again, and a file whose tags moved has moved anyway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Print {
    pub size: u64,
    pub mtime: u128,
    pub artist: String,
    pub covers: String,
}

/// Every file's [`Print`], by the path the log carries, kept in one JSON
/// file and written whole — beside and renamed in — after any look that
/// moved one. Lost or unreadable, it is empty: every file is read once
/// more, and the procedure keeps the library right.
pub struct Prints {
    path: PathBuf,
    files: BTreeMap<String, Print>,
    dirty: bool,
}

impl Prints {
    pub fn load(path: PathBuf) -> Prints {
        let files = match std::fs::read(&path) {
            Ok(bytes) => Prints::decode(&bytes).unwrap_or_else(|| {
                eprintln!(
                    "harken-server: library: {} is not readable; every file is read again",
                    path.display()
                );
                BTreeMap::new()
            }),
            Err(_) => BTreeMap::new(),
        };
        Prints {
            path,
            files,
            dirty: false,
        }
    }

    fn decode(bytes: &[u8]) -> Option<BTreeMap<String, Print>> {
        let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
        let mut out = BTreeMap::new();
        for (file, p) in v.get("files")?.as_object()? {
            out.insert(
                file.clone(),
                Print {
                    size: p.get("size")?.as_u64()?,
                    mtime: p.get("mtime")?.as_str()?.parse().ok()?,
                    artist: p.get("artist")?.as_str()?.to_string(),
                    covers: p.get("covers")?.as_str()?.to_string(),
                },
            );
        }
        Some(out)
    }

    pub fn get(&self, file: &str) -> Option<&Print> {
        self.files.get(file)
    }

    fn set(&mut self, file: String, print: Print) {
        if self.files.get(&file) != Some(&print) {
            self.files.insert(file, print);
            self.dirty = true;
        }
    }

    /// Written whole, beside and renamed in, so a crash leaves the old file
    /// or the new one and never half of either.
    fn save(&mut self) {
        if !self.dirty {
            return;
        }
        let files: serde_json::Map<String, serde_json::Value> = self
            .files
            .iter()
            .map(|(f, p)| {
                (
                    f.clone(),
                    serde_json::json!({
                        "size": p.size,
                        // A string: nanoseconds since 1970 are past what a
                        // JSON number promises to hold exactly.
                        "mtime": p.mtime.to_string(),
                        "artist": p.artist,
                        "covers": p.covers,
                    }),
                )
            })
            .collect();
        let bytes = serde_json::to_vec(&serde_json::json!({ "files": files }))
            .expect("a map of strings and numbers encodes");
        let tmp = self.path.with_extension("json.partial");
        let done = self
            .path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&tmp, bytes))
            .and_then(|()| std::fs::rename(&tmp, &self.path));
        match done {
            Ok(()) => self.dirty = false,
            Err(e) => eprintln!(
                "harken-server: library: cannot keep {}: {e}",
                self.path.display()
            ),
        }
    }
}

/// The scanner's own state on its thread: where things are, and the prints.
struct Scan {
    root: PathBuf,
    music: PathBuf,
    art: PathBuf,
    prints: Prints,
}

/// The scanner's peer: its client, the link to the hub, and the module.
struct Peer {
    client: Client,
    link: Link,
    hosted: Hosted,
}

impl Peer {
    /// What the link said, given to the client, and what it says back:
    /// a refused entry said out loud.
    fn pump(&mut self) {
        let events = self.link.events();
        self.client.pump(&self.link, events, now());
        for e in self.client.take_events() {
            if let Event::Refused { why, .. } = e {
                eprintln!("harken-server: library: an entry was refused: {why}");
            }
        }
    }
}

/// The scanner's whole life: a peer, and a loop over what it is told to
/// look at.
fn run(
    mut scan: Scan,
    mut peer: Peer,
    hub: Handle,
    rx: Receiver<Nudge>,
    stop: &AtomicBool,
    counts: &Counts,
) {
    catch_up(&mut peer, &hub, stop);

    while !stop.load(Ordering::Relaxed) {
        let first = match rx.recv_timeout(IDLE) {
            Ok(n) => Some(n),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        if let Some(first) = first {
            let mut nudges = vec![first];
            // A watched path settles first; everything that arrives in the
            // meantime is part of the same look.
            if matches!(nudges[0], Nudge::One(_)) {
                std::thread::sleep(SETTLE);
            }
            nudges.extend(rx.try_iter());
            let (authored, read) = look(&mut peer, &mut scan, nudges);
            counts.read.fetch_add(read, Ordering::Relaxed);
            if authored > 0 {
                counts.authored.fetch_add(authored, Ordering::Relaxed);
                println!("harken-server: library +{authored}");
            }
        }
        // Whatever that produced, and whatever the log has for us. Not
        // conditional on a nudge: a quiet scanner still has to take the
        // entries other peers wrote.
        peer.pump();
    }
    // Off the hub, with what is pending written down for the next start (the
    // client writes its pending intents as they change).
    peer.pump();
}

/// Pump until the replica has what the log had when it started, so that
/// "already a song" means the library and not an empty replica. Bounded: a
/// scanner that cannot catch up still scans, and the procedure keeps the
/// library right.
fn catch_up(peer: &mut Peer, hub: &Handle, stop: &AtomicBool) {
    let head = hub.with(|h| h.log.log.head().0).unwrap_or(0);
    let deadline = Instant::now() + Duration::from_secs(30);
    while !stop.load(Ordering::Relaxed) && Instant::now() < deadline {
        peer.pump();
        let c = &peer.client;
        if c.linked && c.log.log.head().0 >= head && c.log.pending.is_empty() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    if !stop.load(Ordering::Relaxed) {
        eprintln!("harken-server: library: not caught up with the log after 30s; scanning anyway");
    }
}

/// One look at whatever the nudges name. Says how many entries it authored
/// and how many files it read.
fn look(peer: &mut Peer, scan: &mut Scan, nudges: Vec<Nudge>) -> (usize, usize) {
    let mut paths = BTreeSet::new();
    for n in nudges {
        match n {
            Nudge::All => walk(&scan.music, &mut paths),
            // A *directory* appearing is the case that matters, and it is not
            // the same as a file appearing. Dropping an album folder in
            // creates the directory and its tracks in the same breath, and a
            // recursive watch has to add a watch for the new directory before
            // it can report anything inside it — so the tracks land in the gap
            // and are never announced. What is announced is the directory, so
            // that is what gets walked.
            Nudge::One(p) if p.is_dir() => walk(&p, &mut paths),
            // A picture added, replaced or taken away is every track below
            // its directory offered again: an album's cover is its own
            // directory's, and an artist's picture is the albums' under it.
            Nudge::One(p) if covers::is_image(&p) => {
                if let Some(dir) = p.parent() {
                    walk(dir, &mut paths)
                }
            }
            Nudge::One(p) => {
                paths.insert(p);
            }
        }
    }
    let mut covers = Covers::new(&scan.root, &scan.music, &scan.art);
    let (mut authored, mut read) = (0, 0);
    for p in &paths {
        match offer(peer, scan, &mut covers, p) {
            Offered::Authored => {
                authored += 1;
                read += 1;
            }
            Offered::Read => read += 1,
            Offered::Not => {}
        }
    }
    scan.prints.save();
    (authored, read)
}

/// Whether the library already has a file: asked of the replica's
/// optimistic store through the `media (file)` index — one row looked at
/// (`docs/plan-perf.md` R4). The store holds what this look has already
/// authored, so a file authored a moment ago is known without a set to
/// remember it in.
fn known_file(store: &Store, file: &str) -> bool {
    let schema = store.schema_arc().clone();
    let Some((_, media)) = schema.table("media") else {
        return false;
    };
    let col = media.columns.iter().position(|c| c.name == "file");
    let index = media
        .indexes
        .iter()
        .position(|i| Some(i.columns.as_slice()) == col.as_ref().map(std::slice::from_ref));
    let range = Range::prefix(index, &file.to_string().to_bytes(), Dir::Asc, 1);
    !store.scan("media", &range).is_empty()
}

/// Whether an entry would write anything over what the replica holds: the
/// mutation run over it, its changes looked at and thrown away. What the
/// procedure does with an entry saying nothing new is write nothing, so
/// not authoring one is the same library with one entry fewer in the log.
fn would_change(peer: &Peer, name: &str, input: &[u8]) -> bool {
    let me = &peer.client.log.me;
    let ctx = Ctx {
        id: EntryId::of(&me.session, peer.client.log.local + 1),
        actor: me.actor.clone(),
        session: me.session.clone(),
        roles: me.roles.clone(),
        now: now(),
    };
    !matches!(peer.hosted.run(&peer.client.log.store, &ctx, name, input), Ok(changes) if changes.is_empty())
}

/// Every file under `dir`. A directory that cannot be read is skipped rather
/// than fatal — a permission on one folder is not a reason to index none of
/// the others.
pub fn walk(dir: &Path, out: &mut BTreeSet<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => walk(&path, out),
            Ok(t) if t.is_file() => {
                out.insert(path);
            }
            _ => {}
        }
    }
}

/// Whether a path is a track at all: under `music/`, and audio by its
/// extension. The watch covers the whole media root, so this is where
/// everything that is not music is dropped — without it a podcast appearing
/// in a sibling directory would be indexed as a song, because it has exactly
/// the extension and exactly the tags of one.
pub fn is_track(music: &Path, path: &Path) -> bool {
    if !path.starts_with(music) {
        return false;
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    ext.is_some_and(|e| AUDIO.contains(&e.as_str()))
}

/// What offering one path came to.
enum Offered {
    /// Not a track, not readable, or unchanged since it was last offered:
    /// its tags were not read.
    Not,
    /// Read, and the library already says what it says.
    Read,
    /// Read, and authored.
    Authored,
}

/// Offer one path to the log.
fn offer(peer: &mut Peer, scan: &mut Scan, covers: &mut Covers, path: &Path) -> Offered {
    if !is_track(&scan.music, path) {
        return Offered::Not;
    }
    let Some(rel) = relative(&scan.root, path) else {
        return Offered::Not;
    };
    // Gone since it was announced, or never there: nothing to say about it.
    let Ok(meta) = std::fs::metadata(path) else {
        return Offered::Not;
    };
    let Some(dir) = path.parent() else {
        return Offered::Not;
    };
    let (size, mtime) = (meta.len(), covers::mtime(&meta));
    if let Some(was) = scan.prints.get(&rel) {
        let now = Print {
            size,
            mtime,
            artist: was.artist.clone(),
            covers: covers.beside(dir, &was.artist).print(),
        };
        if *was == now && known_file(&peer.client.log.store, &rel) {
            return Offered::Not;
        }
    }
    let Some((track, embedded)) = read_with_picture(path) else {
        return Offered::Not;
    };
    let found = covers.beside(dir, &track.artist);
    let print = Print {
        size,
        mtime,
        artist: track.artist.clone(),
        covers: found.print(),
    };
    // A cover beside the track first, then the picture inside it — taken
    // out only for a track on a record, since a cover is an album's.
    let album_art = match &found.album {
        Some(i) => i.value(),
        None => embedded
            .filter(|_| !track.album.is_empty())
            .and_then(|bytes| covers.extract(&bytes))
            .unwrap_or_default(),
    };
    let artist_art = found.artist.map(|i| i.value()).unwrap_or_default();
    let call = AddSong {
        title: track.title,
        artist: track.artist,
        album: track.album,
        duration_ms: track.duration_ms,
        file: rel.clone(),
        track: track.number,
        // Neither is in a standard tag: a part is a division of a work and a
        // catalogue number is a scholarly index, and no file carries either.
        // Left empty rather than guessed out of the folder names.
        part: String::new(),
        catalogue: String::new(),
        performer: track.performer,
        bpm: track.bpm,
        // The covers found for it, or empty: the procedure reads empty as
        // "this entry has no picture to offer" and leaves whatever is
        // already on the row, so a track with none does not wipe the one its
        // album already has.
        album_art,
        artist_art,
        // No disc, no work, no movement number — none of which a plain tag
        // carries. 0 and empty are "nobody said": the track becomes a
        // recording of its own, which is what a pop track is.
        disc: 0,
        work_title: String::new(),
        movement_no: 0,
    }
    .to_bytes();
    if !would_change(peer, ADD_SONG, &call) {
        scan.prints.set(rel, print);
        return Offered::Read;
    }
    let before = peer.client.log.pending.len();
    peer.client.mutate(peer.hosted.func(ADD_SONG), call, now());
    if peer.client.log.pending.len() > before {
        scan.prints.set(rel, print);
        return Offered::Authored;
    }
    for e in peer.client.take_events() {
        if let Event::Refused { why, .. } = e {
            eprintln!(
                "harken-server: library: {} was refused: {why}",
                path.display()
            );
        }
    }
    Offered::Read
}

/// The path as the log should carry it: relative to the media root, with `/`
/// between the parts. A track therefore reads `music/Bach/air.wav`.
///
/// **Not the absolute path.** The log is permanent, so an absolute one would
/// freeze this machine's layout into it forever and break the day the
/// directory moves. Relative is also exactly what `/media/` serves, so the
/// column a client plays from and the column the scanner writes are the same
/// string.
pub fn relative(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let mut parts = Vec::new();
    for part in rel.components() {
        match part {
            std::path::Component::Normal(p) => parts.push(p.to_str()?.to_string()),
            // Anything that is not a plain name — a `..`, a root — is not
            // something under the media directory, whatever the path says.
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// What a file's tags say, as `add_song` takes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: i64,
    /// The tag's own numbering, 0 when the file has none.
    pub number: i64,
    /// `ALBUMARTIST` where there is one. On a classical recording the artist
    /// tag is usually the composer and this is the performer, which is
    /// exactly the pair the schema keeps apart.
    pub performer: String,
    /// `TBPM`. Rare, and the reason the column is worth storing rather than
    /// deriving: when a file states its tempo, that is a fact.
    pub bpm: i64,
}

/// What the tags say, with the file name as the fallback for a title; `None`
/// for a file that is not audio lofty can read (or not yet: one still being
/// written is read again on its next event).
///
/// A scanner that guessed the artist and album from the directory layout
/// would be wrong for every library not arranged the way it expected, so an
/// untagged file gets a title and nothing else rather than a confident
/// mistake.
pub fn read(path: &Path) -> Option<Track> {
    read_with_picture(path).map(|(track, _)| track)
}

/// [`read`], and the picture the tags carry ([`covers::embedded`]), from
/// the one reading.
pub fn read_with_picture(path: &Path) -> Option<(Track, Option<Vec<u8>>)> {
    use lofty::file::{AudioFile, TaggedFileExt};
    use lofty::probe::Probe;
    use lofty::tag::{Accessor, ItemKey};

    let tagged = Probe::open(path).ok()?.read().ok()?;
    let duration_ms = i64::try_from(tagged.properties().duration().as_millis()).unwrap_or(0);
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let text =
        |v: Option<std::borrow::Cow<'_, str>>| v.map(|s| s.trim().to_string()).unwrap_or_default();
    let title = text(tag.and_then(|t| t.title()));
    let title = if title.is_empty() {
        path.file_stem()?.to_str()?.trim().to_string()
    } else {
        title
    };
    if title.is_empty() {
        return None;
    }
    // A tag that is there but unparseable is the same as no tag: a number
    // nobody can read is not a number.
    let bpm = tag
        .and_then(|t| t.get_string(ItemKey::Bpm))
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(0);
    let track = Track {
        title,
        artist: text(tag.and_then(|t| t.artist())),
        album: text(tag.and_then(|t| t.album())),
        duration_ms,
        // The tag's own reading of it, which is where `3/12` becomes 3.
        number: tag
            .and_then(|t| t.track())
            .map_or(0, i64::from)
            .clamp(0, 999),
        performer: tag
            .and_then(|t| t.get_string(ItemKey::AlbumArtist))
            .unwrap_or_default()
            .trim()
            .to_string(),
        bpm: bpm.max(0),
    };
    Some((track, covers::embedded(&tagged)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_track_is_audio_under_music_and_nothing_else() {
        let root = Path::new("/srv/media");
        let music = root.join(MUSIC);
        assert!(is_track(&music, &root.join("music/Bach/air.FLAC")));
        assert!(is_track(&music, &root.join("music/loose.opus")));
        assert!(!is_track(&music, &root.join("music/Bach/cover.jpg")));
        assert!(!is_track(&music, &root.join("music/Bach/noext")));
        assert!(
            !is_track(&music, &root.join("podcasts/ep1.mp3")),
            "audio, and not a track"
        );
        assert!(
            !is_track(&music, &root.join("musicals/x.mp3")),
            "a component, not a prefix"
        );
    }

    #[test]
    fn the_log_carries_the_path_under_the_media_root() {
        let root = Path::new("/srv/media");
        assert_eq!(
            relative(root, &root.join("music/Bach/air.flac")).as_deref(),
            Some("music/Bach/air.flac")
        );
        assert_eq!(relative(root, Path::new("/elsewhere/music/a.mp3")), None);
        assert_eq!(relative(root, root), None, "the root itself is not a file");
        assert_eq!(relative(root, &root.join("music/../../etc/passwd")), None);
    }
}
