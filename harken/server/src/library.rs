//! The media directory, and everything else the library comes from, as a
//! worker of the server's.
//!
//! A server that owns a folder of files has to get them into the log
//! somehow, and there are only two honest ways: write the rows directly, or
//! be a peer and author mutations like everything else. This is the second,
//! and it is not a preference — writing rows would be a second definition of
//! what "add a song" means, and the first time it disagreed with the domain's
//! procedure the replicas would diverge with nothing to say so.
//!
//! So the scanner is a worker (`Handle::<Authority>::worker`): a device of
//! the server's own, signed in as the account [`ACCOUNT`], with its own
//! replica and its own pending intents on disk, dialling the hub in this
//! process. It gets its confirmations, it gets the rebase, and a file it
//! adds fans out to every connected peer without anyone asking.
//!
//! **A file is one song, and that is not this file's doing.** `add_song` is
//! an upsert by `file`, inside the procedure: a file the library already has
//! is that song said again, retagged or with a cover it did not have, and
//! the same file saying nothing new writes nothing — for every peer
//! replaying the log, not only for the scanner that authored it.
//!
//! **Its work is jobs** (`ark::effect`), each kind a handler of its
//! worker's, run on a thread of its own:
//! - a [`Look`] at the media directory (`library.look`): everything at boot
//!   or on a rescan, the paths a watch saw settle;
//! - a [`Source`] read (`source.read`): a followed feed, every hour and
//!   whenever it is followed or more of it is asked for, or the catalogue —
//!   each, with every feed, at every full look ([`crate::sync`]);
//! - a [`Fetch`] of a file a source's rows name (`library.queue_download`),
//!   one at a time;
//! - a [`Hearing`] of an episode here without captions (`hearing.hear`), on
//!   a server with a transcriber ([`crate::transcribe`]).
//!
//! A job asked and not done before a stop is done after the next start,
//! once its claim's lease has run out; one that fails is tried again later.
//!
//! What this file adds is not asking. It keeps, beside its replica, what
//! each file was when it was last offered — its size, when it was modified,
//! and the same of the covers it was offered with ([`Prints`]). A file whose
//! print has not moved, and which the replica has as a song, is not read at
//! all; one whose print has moved is read, and authored only if the entry
//! would change something the replica holds (`Peer::mutate_new`). So a
//! restart over ten thousand untouched files reads no tags and authors
//! nothing, a retagged file is one entry, and the first start of a server
//! that never kept prints reads every file once — which is what gives a
//! library scanned before covers were found its covers. The prints are this
//! server's, never the log's — rows of a local instance of its own
//! ([`HarkenLocal`]), beside the pictures it took out of files — and
//! missing or unreadable they are a full read, and nothing worse.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::domain::{
    AddEpisode, AddShow, AddSong, DownloadProgress, Downloaded, Fetch, Harken, Hear, Hearing, Look,
    LookAt, Looked, Read, SetChapters, SetTranscript, Source,
};
use anyhow::Result;
use ark::format::Encode;
use ark::prelude::{Deserialize, Row, Schema, Serialize};
use ark_auth::server::Auth;
use ark_auth::Account;
use ark_peer::{Authority, Files, Handle, Instance, Peer, Work};
use notify::{RecursiveMode, Watcher};

use crate::covers::{self, Covers};
use crate::sync::Plan;

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

/// Where podcasts and sermons live under the media root, beside `music/`:
/// a folder per show (`podcasts/Full Time Nix/…`), the show its name, an
/// episode each audio file in it. A show followed by its feed is
/// downloaded into the data directory instead (`feeds.rs`).
pub const SHOWS: &[(&str, &str)] = &[("podcasts", "podcast"), ("sermons", "sermon")];

/// The account a scanned track is authored as. A real name would be a lie —
/// nobody added these by hand — and it shows up as the `user_id` on every
/// scanned row, which is the truth: the library put it there.
pub const ACCOUNT: &str = "library";

/// What the scanner's sign-in says it is.
pub const DEVICE: &str = "the scanner";

/// The mutation a file is offered as.
const ADD_SONG: &str = "library.add_song";

/// The mutations a show and an episode are offered as.
const ADD_SHOW: &str = "library.add_show";
const ADD_EPISODE: &str = "library.add_episode";

/// The mutations a download is said with.
const QUEUE_DOWNLOAD: &str = "library.queue_download";
const DOWNLOAD_PROGRESS: &str = "library.download_progress";
const DOWNLOADED: &str = "library.downloaded";

/// The mutations a look, a source's read and an episode's hearing are
/// asked with.
const LOOK: &str = "library.look";
const READ: &str = "source.read";
const HEAR: &str = "hearing.hear";

/// How long a watched path is left to settle before it is read: a file is
/// usually still being written when its create arrives, so the tags are not
/// there yet. Every event inside the window is one look.
const SETTLE: Duration = Duration::from_millis(400);

/// Where the scanner keeps what is its own under the server's data: its
/// replica, the prints of the files it has offered, and the pictures it
/// took out of them.
pub fn replica_dir(data: &Path) -> PathBuf {
    data.join("library")
}

/// The prints and the pictures' index ([`HarkenLocal`]): a local instance
/// of this server's, not of the log's.
pub fn prints_dir(data: &Path) -> PathBuf {
    data.join("library-files")
}

/// The pictures taken out of files, which `/media/.art/` serves
/// ([`covers::routes`]).
pub fn art_dir(data: &Path) -> PathBuf {
    data.join("art")
}

/// The scanner, running. Dropping it stops the watch and takes its worker
/// off the hub.
pub struct Scanner {
    /// Kept alive for as long as the scanner is: a watcher nobody holds
    /// stops at once.
    _watcher: Option<notify::RecommendedWatcher>,
    worker: Handle<Peer>,
    scan: Arc<Scan>,
}

/// What the scanner has done since it started, for what it says and for
/// the tests.
#[derive(Default)]
struct Counts {
    /// Entries authored.
    authored: AtomicUsize,
    /// Files whose tags were read.
    read: AtomicUsize,
}

impl Scanner {
    /// Scan `music/` and the shows' folders under `root` and watch `root`,
    /// authoring through `hub` as [`ACCOUNT`], signed in by `auth`, with the
    /// scanner's own state — the replica, the prints, the pictures taken
    /// out of files — under `data` ([`replica_dir`], [`prints_dir`],
    /// [`art_dir`]).
    ///
    /// The walk is narrow and the watch is wide, on purpose: only `music/`
    /// holds tracks, but watching the root is what notices that directory
    /// being created at all — on a fresh install it may not exist yet, and a
    /// watch on a path that is not there watches nothing forever.
    ///
    /// What is downloaded into the library comes from sources: with
    /// `catalogue`, the URL of one (`catalogue.rs`); and the feeds the
    /// library follows (`feeds.rs`). Captions beside an episode's file
    /// (`<file>.vtt`) are its transcript, read at every full look and as
    /// they are written, its chapters made from them by `chapters` when it
    /// has none; with `transcriber`, the episodes here without any are
    /// heard (`transcribe.rs`).
    ///
    /// Returns at once: everything is done by the worker's handlers.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        root: PathBuf,
        hub: &Handle<Authority>,
        auth: &Arc<Auth>,
        data: PathBuf,
        catalogue: Option<String>,
        transcriber: Option<PathBuf>,
        chapters: Vec<Box<dyn crate::chapters::Chapterer>>,
    ) -> Result<Scanner> {
        let account = Account {
            id: ACCOUNT.into(),
            name: "Library".into(),
            roles: vec![crate::domain::LIBRARY.into()],
            ..Account::default()
        };
        let dir = replica_dir(&data);
        let disk = Files::new(&dir)
            .map_err(|e| anyhow::anyhow!("the scanner's replica in {}: {e}", dir.display()))?;
        let worker = hub
            .worker(auth, &account, DEVICE, Box::new(disk))
            .map_err(|e| anyhow::anyhow!("signing the scanner in: {e}"))?;
        let feeds = crate::feeds::dir(&data);
        let _ = std::fs::create_dir_all(&feeds);
        let scan = Arc::new(Scan {
            music: root.join(MUSIC),
            root: root.clone(),
            art: art_dir(&data),
            prints: Mutex::new(Prints::load(prints_dir(&data))),
            feeds: feeds.clone(),
            catalogue: catalogue.map(|url| (url, crate::catalogue::dir(&data))),
            transcriber,
            chapters,
            counts: Counts::default(),
        });
        {
            let mut p = worker.lock();
            let s = scan.clone();
            p.handle::<Look>(move |w, job| s.look(w, job));
            let s = scan.clone();
            p.handle::<Source>(move |w, job| s.read(w, job));
            let s = scan.clone();
            p.handle::<Fetch>(move |w, job| s.fetch(w, job));
            if scan.transcriber.is_some() {
                let s = scan.clone();
                p.handle::<Hearing>(move |w, job| s.hear(w, job));
            }
        }
        let scanner = Scanner {
            _watcher: None,
            worker,
            scan,
        };
        scanner.rescan();
        Ok(Scanner {
            _watcher: scanner.watch(&feeds),
            ..scanner
        })
    }

    /// The watch, best-effort: a directory that cannot be watched is still
    /// scanned at boot, and saying so is better than refusing to start over
    /// it. What it sees settles for [`SETTLE`] on a thread of its own, and
    /// is one look.
    fn watch(&self, feeds: &Path) -> Option<notify::RecommendedWatcher> {
        let (events, seen) = channel::<PathBuf>();
        let (worker, scan) = (self.worker.clone(), self.scan.clone());
        let settle = std::thread::Builder::new().name("harken-watch".into());
        let _ = settle.spawn(move || {
            while let Ok(first) = seen.recv() {
                std::thread::sleep(SETTLE);
                let paths = std::iter::once(first).chain(seen.try_iter());
                let paths: BTreeSet<String> = paths.filter_map(|p| scan.file_of(&p)).collect();
                if !paths.is_empty() {
                    ask(&worker, paths.into_iter().collect());
                }
            }
        });
        let downloads = feeds.to_path_buf();
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            let Ok(event) = event else { return };
            // Creates and writes, and a picture going. A removed *track* is
            // not handled on purpose: the log is permanent, and a file
            // disappearing is not evidence that somebody meant to delete
            // the song — an unplugged disk looks exactly the same. A
            // removed cover is only a directory whose tracks are offered
            // again, with whatever cover they have now. Of the feeds'
            // downloads, only captions: a download is written as it comes.
            let gone = matches!(event.kind, notify::EventKind::Remove(_));
            let made = matches!(
                event.kind,
                notify::EventKind::Create(_) | notify::EventKind::Modify(_)
            );
            let wanted = |p: &PathBuf| {
                let captions = p.extension().is_some_and(|e| e == "vtt");
                (made || gone && covers::is_image(p)) && (captions || !p.starts_with(&downloads))
            };
            for path in event.paths.into_iter().filter(wanted) {
                let _ = events.send(path);
            }
        });
        let mut w = watcher
            .map_err(|e| tracing::warn!("no filesystem watch available: {e}"))
            .ok()?;
        if let Err(e) = w.watch(&self.scan.root, RecursiveMode::Recursive) {
            tracing::warn!("cannot watch {}: {e}", self.scan.root.display());
            return None;
        }
        // A feed's episodes are in the data directory, and so are their
        // captions: read as they are written.
        if let Err(e) = w.watch(feeds, RecursiveMode::NonRecursive) {
            tracing::warn!("cannot watch {}: {e}", feeds.display());
        }
        Some(w)
    }

    /// Walk the whole directory again, and read every source. Safe at any
    /// time: a file whose print has not moved is not read, and one that has
    /// is authored only if it says something new.
    pub fn rescan(&self) {
        ask(&self.worker, vec![]);
    }

    /// How many entries this scanner has authored since it started.
    pub fn authored(&self) -> usize {
        self.scan.counts.authored.load(Ordering::Relaxed)
    }

    /// How many files' tags this scanner has read since it started.
    pub fn read(&self) -> usize {
        self.scan.counts.read.load(Ordering::Relaxed)
    }
}

/// A look asked for: at these files, or at everything.
fn ask(worker: &Handle<Peer>, paths: Vec<String>) {
    if let Err(why) = worker.lock().mutate(LOOK, &LookAt { paths }) {
        tracing::warn!("a look was refused: {why}");
    }
}

/// What one file was when it was last offered: its size and modification
/// time, and [`covers::Found::print`] of the pictures beside it. Any of the
/// three moving is a reason to read it again. The artist its tags named is
/// kept too, because whose a picture in the directory above is depends on
/// it — so the pictures are looked at as that artist's until the tags are
/// read again, and a file whose tags moved has moved anyway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Row)]
pub struct Print {
    /// The path the log carries.
    #[ark(key)]
    pub path: String,
    pub size: i64,
    pub mtime: i64,
    pub artist: String,
    pub covers: String,
}

/// A picture taken out of a file and written under `art/` (the picture
/// stays a file), by its name there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Row)]
pub struct Art {
    #[ark(key)]
    pub name: String,
}

/// What the scanner keeps of its own: a local schema of the server's, not
/// the domain's — the domain is what every peer links, phones too, and
/// none of these rows is ever in a log any of them syncs.
#[derive(Schema)]
pub struct HarkenLocal {
    pub print: Print,
    pub art: Art,
}

/// Every file's [`Print`] and every picture taken out of one, as rows of
/// the local instance; what a look moved is written in one entry after
/// it. Lost or unreadable, they are empty: every file is read once more,
/// and the procedure keeps the library right.
pub struct Prints {
    local: Instance,
    moved: BTreeMap<String, Print>,
}

impl Prints {
    pub fn load(dir: PathBuf) -> Prints {
        let disk: Box<dyn ark_peer::Disk> = match Files::new(&dir) {
            Ok(f) => Box::new(f),
            Err(e) => {
                tracing::warn!("cannot keep prints in {}: {e}", dir.display());
                Box::new(ark_peer::Memory::default())
            }
        };
        let local = Instance::open::<HarkenLocal>(disk);
        if let Some(e) = &local.failure {
            tracing::warn!(
                "{} is not readable ({e}); every file is read again",
                dir.display()
            );
        }
        Prints {
            local,
            moved: BTreeMap::new(),
        }
    }

    pub fn get(&self, file: &str) -> Option<Print> {
        let moved = self.moved.get(file).cloned();
        moved.or_else(|| self.local.read::<HarkenLocal, _>(|db| db.print.get(file)))
    }

    fn set(&mut self, print: Print) {
        if self.get(&print.path).as_ref() != Some(&print) {
            self.moved.insert(print.path.clone(), print);
        }
    }

    /// The pictures already taken out of files.
    fn art(&self) -> BTreeSet<String> {
        self.local
            .read::<HarkenLocal, _>(|db| db.art.scan())
            .into_iter()
            .map(|a| a.name)
            .collect()
    }

    /// What moved, and the pictures `written` the index does not have yet.
    fn save(&mut self, written: &BTreeSet<String>) {
        let moved = std::mem::take(&mut self.moved);
        let kept = self.local.write::<HarkenLocal, ark::db::Durable, _>(|db| {
            for p in moved.into_values() {
                db.print.put(p)?;
            }
            for name in written {
                db.art.put(Art { name: name.clone() })?;
            }
            Ok(())
        });
        if let Some(e) = kept.err().or(self.local.failure.clone()) {
            tracing::warn!("cannot keep the prints: {e}");
        }
    }
}

/// What the scanner's handlers share: where things are, the sources, who
/// hears and who makes chapters, the prints, and what it has done.
struct Scan {
    root: PathBuf,
    music: PathBuf,
    art: PathBuf,
    prints: Mutex<Prints>,
    /// Where a feed's episodes are on disk, and the catalogue's URL and
    /// where its files are, when there is a catalogue.
    feeds: PathBuf,
    catalogue: Option<(String, PathBuf)>,
    /// Who hears the episodes here, when anybody does.
    transcriber: Option<PathBuf>,
    /// Who makes chapters of an episode's captions when it has none, in
    /// the order to try them.
    chapters: Vec<Box<dyn crate::chapters::Chapterer>>,
    counts: Counts,
}

/// What `f` reads of what a peer holds.
fn held<T>(p: &Peer, f: impl FnOnce(&ark::db::Db<Harken>) -> T) -> T {
    ark::db::read::<Harken, _>(&p.log.store, f)
}

impl Scan {
    /// Where a file the log names is on disk: a feed's or the catalogue's
    /// under the data directory, anything else under the media root.
    fn path_of(&self, file: &str) -> PathBuf {
        let under = |dir: &str| file.strip_prefix(dir).and_then(|f| f.strip_prefix('/'));
        if let Some(name) = under(crate::feeds::DIR) {
            return self.feeds.join(name);
        }
        match (under(crate::catalogue::DIR), &self.catalogue) {
            (Some(name), Some((_, dir))) => dir.join(name),
            _ => self.root.join(file),
        }
    }

    /// The file a path on disk is, as the log names it: a feed's, or one
    /// under the media root.
    fn file_of(&self, path: &Path) -> Option<String> {
        match path.strip_prefix(&self.feeds) {
            Ok(name) => Some(format!("{}/{}", crate::feeds::DIR, name.to_str()?)),
            Err(_) => relative(&self.root, path),
        }
    }

    /// A [`Look`]: at the files it names, or at everything — and then
    /// every source read again, every episode's captions read, and every
    /// episode here without any heard.
    fn look(&self, w: &mut Work, job: &Look) -> Result<Looked, String> {
        let all = job.paths.is_empty();
        let mut paths = BTreeSet::new();
        if all {
            walk(&self.music, &mut paths);
            for (dir, _) in SHOWS {
                walk(&self.root.join(dir), &mut paths);
            }
        }
        for p in job.paths.iter().map(|f| self.path_of(f)) {
            match p {
                // A *directory* appearing is the case that matters, and it
                // is not the same as a file appearing. Dropping an album
                // folder in creates the directory and its tracks in the same
                // breath, and a recursive watch has to add a watch for the
                // new directory before it can report anything inside it —
                // so the tracks land in the gap and are never announced.
                // What is announced is the directory, so that is what gets
                // walked.
                p if p.is_dir() => walk(&p, &mut paths),
                // A picture added, replaced or taken away is every track
                // below its directory offered again: an album's cover is its
                // own directory's, and an artist's picture is the albums'
                // under it.
                p if covers::is_image(&p) => p.parent().iter().for_each(|d| walk(d, &mut paths)),
                p => drop(paths.insert(p)),
            }
        }
        let mut prints = self.prints.lock().unwrap_or_else(|e| e.into_inner());
        let mut covers = Covers::new(&self.root, &self.music, &self.art, prints.art());
        let (mut authored, mut read) = (0, 0);
        for p in &paths {
            // Captions are an episode's transcript.
            if p.extension().is_some_and(|e| e == "vtt") {
                authored += w.with(|peer| self.captions(peer, p))?;
                continue;
            }
            match w.with(|peer| offer(peer, self, &mut prints, &mut covers, p))? {
                Offered::Authored => (authored, read) = (authored + 1, read + 1),
                Offered::Read => read += 1,
                Offered::Not => {}
            }
        }
        prints.save(covers.written());
        drop(prints);
        if all {
            authored += w.with(|p| self.everything(p))?;
        }
        self.counts.read.fetch_add(read, Ordering::Relaxed);
        self.said(authored, "library");
        let (read, authored) = (read as i64, authored as i64);
        Ok(Looked { read, authored })
    }

    /// What a full look asks besides the files: every source read, every
    /// episode's captions read — whatever was written while the server
    /// was down — and every episode here without any heard, newest first.
    /// How many entries the captions were.
    fn everything(&self, p: &mut Peer) -> usize {
        let (followed, mut episodes) = held(p, |db| {
            let episodes = db.episode.scan().into_iter().filter_map(|e| {
                let m = db.media.get(e.media_id)?;
                Some((std::cmp::Reverse((e.published_ms, m.pos)), m.file))
            });
            (db.subscription.scan(), episodes.collect::<Vec<_>>())
        });
        let sources = self.catalogue.iter().map(|c| c.0.clone());
        for url in sources.chain(followed.into_iter().map(|s| s.url)) {
            if let Err(why) = p.mutate_new(READ, &Read { url }) {
                tracing::warn!("a read was refused: {why}");
            }
        }
        episodes.sort();
        let mut authored = 0;
        for (_, file) in episodes {
            let captions = crate::transcribe::captions_of(&self.path_of(&file));
            match captions.is_file() {
                true => authored += self.captions(p, &captions),
                false => self.to_hear(p, &file),
            }
        }
        authored
    }

    /// An episode here without captions asked to be heard, on a server
    /// with a transcriber.
    fn to_hear(&self, p: &mut Peer, file: &str) {
        let path = self.path_of(file);
        if self.transcriber.is_none()
            || !path.is_file()
            || crate::transcribe::captions_of(&path).exists()
        {
            return;
        }
        if let Err(why) = p.mutate_new(HEAR, &Hear { file: file.into() }) {
            tracing::warn!(file, "a hearing was refused: {why}");
        }
    }

    /// A [`Source`] read: the catalogue, or a followed feed — read again in
    /// an hour — each said as plans and authored. A read that could not
    /// have everything fails, and is read again sooner.
    fn read(&self, w: &mut Work, job: &Source) -> Result<i64, String> {
        let agent = crate::sync::agent();
        let (plans, failed, every) = match &self.catalogue {
            Some((url, dir)) if *url == job.url => {
                let (plans, failed) = crate::catalogue::read(&agent, url, dir);
                (plans, failed, None)
            }
            _ => {
                // A feed nobody follows any more is read no more.
                let Some(s) = w.with(|p| held(p, |db| db.subscription.get(&job.url)))? else {
                    return Ok(0);
                };
                let mut failed = 0;
                let plan = crate::feeds::read(&agent, &s, &self.feeds, &mut failed);
                (vec![plan], failed, Some(crate::feeds::EVERY))
            }
        };
        let mut authored = 0;
        for plan in &plans {
            let n = w.with(|p| author(p, plan))?;
            self.said(n, &plan.what);
            authored += n;
        }
        if failed > 0 {
            return Err(format!("{failed} files could not be had"));
        }
        if let Some(every) = every {
            w.again(ark_peer::now_ms() + every.as_millis() as i64);
        }
        Ok(authored as i64)
    }

    /// A [`Fetch`]: a file still to come downloaded, saying how it goes,
    /// and heard once it is here when it is an episode. Its download gone
    /// — the file here, or its row taken out — is nothing to do.
    fn fetch(&self, w: &mut Work, job: &Fetch) -> Result<i64, String> {
        let id = job.media_id;
        let row = w.with(|p| held(p, |db| Some((db.media.get(id)?, db.download.get(id)?))))?;
        let Some((media, download)) = row else {
            return Ok(0);
        };
        let (path, file) = (self.path_of(&media.file), media.file.clone());
        let say = |w: &mut Work, state: &str, bytes: u64, total: u64, error: &str| {
            let (state, error) = (state.to_string(), error.to_string());
            let (bytes, total) = (bytes as i64, total as i64);
            let input = DownloadProgress {
                file: file.clone(),
                state,
                bytes,
                total,
                error,
            };
            w.with(|p| p.mutate_new(DOWNLOAD_PROGRESS, &input))?
        };
        let mut bytes = 0;
        if !path.exists() {
            // Refused, its row is gone: not downloaded.
            if say(w, "downloading", 0, 0, "").is_err() {
                return Ok(0);
            }
            let mut last = std::time::Instant::now();
            let got = crate::sync::download(
                &crate::sync::agent(),
                &download.source,
                &path,
                |n, total| {
                    bytes = n;
                    if last.elapsed() >= crate::sync::EVERY_SAID {
                        last = std::time::Instant::now();
                        let _ = say(w, "downloading", n, total, "");
                    }
                },
            );
            if let Err(e) = got {
                tracing::warn!(file, "{e}");
                let _ = say(w, "failed", 0, 0, &e);
                return Err(e);
            }
            std::thread::sleep(crate::sync::PAUSE);
        }
        w.mutate(DOWNLOADED, &Downloaded { file: file.clone() })?;
        if crate::domain::shows::KINDS.contains(&media.kind.as_str()) {
            w.with(|p| self.to_hear(p, &file))?;
        }
        Ok(bytes as i64)
    }

    /// A [`Hearing`]: an episode here heard, unless its captions came
    /// meanwhile, and its captions read.
    fn hear(&self, w: &mut Work, job: &Hearing) -> Result<i64, String> {
        let id = job.media_id;
        let Some(m) = w.with(|p| held(p, |db| db.media.get(id)))? else {
            return Ok(0);
        };
        let path = self.path_of(&m.file);
        let captions = crate::transcribe::captions_of(&path);
        if let (false, Some(program)) = (captions.exists(), &self.transcriber) {
            // A sermon is its preacher's: one voice throughout.
            crate::transcribe::hear(program, &path, &m.title, m.kind == "sermon")?;
        }
        Ok(w.with(|p| self.captions(p, &captions))? as i64)
    }

    /// Entries authored, counted and said.
    fn said(&self, authored: usize, what: &str) {
        if authored > 0 {
            self.counts.authored.fetch_add(authored, Ordering::Relaxed);
            tracing::info!(authored, "{what}");
        }
    }

    /// The captions beside an episode's file (`<file>.vtt`, whoever wrote
    /// them: the transcriber, another machine that heard it, a hand)
    /// authored as its transcript, and its chapters made from them when it
    /// has none (`chapters.rs`). Said again the same, it writes nothing.
    fn captions(&self, peer: &mut Peer, path: &Path) -> usize {
        let Some(file) = path
            .to_str()
            .and_then(|p| p.strip_suffix(".vtt"))
            .and_then(|audio| self.file_of(Path::new(audio)))
        else {
            return 0;
        };
        let episode = held(peer, |db| {
            let m = db
                .media
                .by(crate::domain::Media::FILE)
                .from(&file)
                .first()
                .filter(|m| m.file == file)?;
            let e = db.episode.get(m.id)?;
            let chaptered = db.chapter.scan().iter().any(|c| c.media_id == m.id);
            Some((m.title, e.description, chaptered))
        });
        let Some((title, description, chaptered)) = episode else {
            return 0;
        };
        let crate::captions::Captions { cues, voices } = match std::fs::read_to_string(path) {
            Ok(text) => crate::captions::captions(&text),
            Err(e) => {
                tracing::warn!("{}: {e}", path.display());
                return 0;
            }
        };
        if cues.is_empty() {
            return 0;
        }
        let call = |name: &'static str, input: Vec<u8>| crate::sync::Call {
            name,
            what: title.clone(),
            input,
            if_there: false,
        };
        let about = crate::chapters::About {
            title: &title,
            description: &description,
        };
        let made = match chaptered {
            true => vec![],
            false => crate::chapters::first(&self.chapters, &about, &cues),
        };
        let transcript = SetTranscript {
            file: file.clone(),
            cues,
            voices,
        };
        let mut calls = vec![call("library.set_transcript", transcript.to_bytes())];
        if !made.is_empty() {
            let chapters = SetChapters {
                file,
                chapters: made,
            };
            calls.push(call("library.set_chapters", chapters.to_bytes()));
        }
        let plan = Plan {
            what: title.clone(),
            calls,
            gate: false,
            files: vec![],
        };
        author(peer, &plan)
    }
}

/// A plan authored: each of its mutations that would change something,
/// then each of its files queued, or said to be here when it already is.
/// How many entries it authored: none when it is gated and its first
/// mutation is refused.
fn author(p: &mut Peer, plan: &Plan) -> usize {
    let mut authored = 0;
    for (n, call) in plan.calls.iter().enumerate() {
        match p.mutate_new(call.name, call.input.clone()) {
            Ok(n) => authored += n.is_some() as usize,
            Err(_) if plan.gate && n == 0 => return 0,
            // A describe of a work this library has not got.
            Err(_) if call.if_there => {}
            Err(why) => {
                tracing::warn!(what = %plan.what, "{} ({}) was refused: {why}", call.what, call.name)
            }
        }
    }
    for want in &plan.files {
        let file = want.file.clone();
        let input = match want.here() {
            true => (DOWNLOADED, Downloaded { file }.to_bytes()),
            false => (
                QUEUE_DOWNLOAD,
                crate::domain::QueueDownload {
                    file,
                    source: want.url.clone(),
                }
                .to_bytes(),
            ),
        };
        match p.mutate_new(input.0, input.1) {
            Ok(n) => authored += n.is_some() as usize,
            Err(why) => tracing::warn!(file = %want.file, "{} was refused: {why}", input.0),
        }
    }
    authored
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

/// The kind of show a path is an episode of, and the show's folder: audio
/// under `podcasts/<show>/` or `sermons/<show>/`. A file loose in
/// `podcasts/` is nobody's show.
pub fn is_episode(root: &Path, path: &Path) -> Option<(&'static str, PathBuf)> {
    let audio = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO.contains(&e.to_ascii_lowercase().as_str()));
    if !audio {
        return None;
    }
    SHOWS.iter().find_map(|(dir, kind)| {
        let base = root.join(dir);
        let rest = path.strip_prefix(&base).ok()?;
        let show = rest.components().next()?;
        (rest.components().count() > 1).then(|| (*kind, base.join(show)))
    })
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

/// Whether the library already has a file: asked of the replica's
/// optimistic store through the `media (file)` index — one row looked at
/// (`docs/plan-perf.md` R4). The store holds what this look has already
/// authored, so a file authored a moment ago is known without a set to
/// remember it in.
fn known_file(p: &Peer, file: &str) -> bool {
    let media = held(p, |db| {
        db.media
            .by(crate::domain::Media::FILE)
            .from(&file.to_string())
            .first()
    });
    media.is_some_and(|m| m.file == file)
}

/// Offer one path to the log.
fn offer(
    peer: &mut Peer,
    scan: &Scan,
    prints: &mut Prints,
    covers: &mut Covers,
    path: &Path,
) -> Offered {
    if let Some((kind, show)) = is_episode(&scan.root, path) {
        return offer_episode(peer, scan, prints, covers, path, kind, &show);
    }
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
    let (size, mtime) = (meta.len() as i64, covers::mtime(&meta) as i64);
    if let Some(was) = prints.get(&rel) {
        let now = Print {
            path: rel.clone(),
            size,
            mtime,
            artist: was.artist.clone(),
            covers: covers.beside(dir, &was.artist).print(),
        };
        if was == now && known_file(peer, &rel) {
            return Offered::Not;
        }
    }
    let Some((track, embedded)) = read_with_picture(path) else {
        return Offered::Not;
    };
    let found = covers.beside(dir, &track.artist);
    let print = Print {
        path: rel.clone(),
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
    let artist_banner = found.backdrop.map(|i| i.value()).unwrap_or_default();
    let call = AddSong {
        title: track.title,
        artist: track.artist,
        album: track.album,
        duration_ms: track.duration_ms,
        file: rel.clone(),
        track: track.number,
        // A part, a division of a work, is in no standard tag; left empty
        // rather than guessed out of the folder names. The catalogue number
        // is the work title's own (`catalogue`).
        part: String::new(),
        catalogue: track.catalogue,
        performer: track.performer,
        bpm: track.bpm,
        // The covers found for it, or empty: the procedure reads empty as
        // "this entry has no picture to offer" and leaves whatever is
        // already on the row, so a track with none does not wipe the one its
        // album already has.
        album_art,
        artist_art,
        // 0 and empty are "nobody said": a file naming no work becomes a
        // recording of its own, which is what a pop track is.
        disc: track.disc,
        work_title: track.work,
        movement_no: track.movement_no,
        artist_banner,
        released: track.year,
        // Its album is the folder it is in, as a library's files are.
        folder: String::new(),
    }
    .to_bytes();
    match peer.mutate_new(ADD_SONG, call) {
        Ok(authored) => {
            prints.set(print);
            match authored {
                Some(_) => Offered::Authored,
                None => Offered::Read,
            }
        }
        Err(why) => {
            tracing::warn!("{} was refused: {why}", path.display());
            Offered::Read
        }
    }
}

/// Offer one episode of a folder's show: the show, named for its folder
/// and pictured by the cover in it, and then the file — each authored only
/// if it would change something. Its tags say its title (else its file's
/// name), who made it and its number; a folder has no dates, so it says
/// none, and the show lists its episodes as they were added.
fn offer_episode(
    peer: &mut Peer,
    scan: &Scan,
    prints: &mut Prints,
    covers: &mut Covers,
    path: &Path,
    kind: &str,
    show: &Path,
) -> Offered {
    let Some(rel) = relative(&scan.root, path) else {
        return Offered::Not;
    };
    let Ok(meta) = std::fs::metadata(path) else {
        return Offered::Not;
    };
    let art = covers.show_cover(show);
    let print = |artist: String| Print {
        path: rel.clone(),
        size: meta.len() as i64,
        mtime: covers::mtime(&meta) as i64,
        artist,
        covers: art
            .as_ref()
            .map_or(String::new(), |i| format!("{}@{}", i.rel, i.print)),
    };
    if let Some(was) = prints.get(&rel) {
        if was == print(was.artist.clone()) && known_file(peer, &rel) {
            return Offered::Not;
        }
    }
    let Some(track) = read(path) else {
        return Offered::Not;
    };
    let title = show
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();
    let add_show = AddShow {
        kind: kind.into(),
        title: title.clone(),
        author: track.artist.clone(),
        art: art.as_ref().map(|i| i.value()).unwrap_or_default(),
        ..AddShow::default()
    }
    .to_bytes();
    let stem = path
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();
    let add_episode = AddEpisode {
        show: crate::domain::keys::show_key(kind, &title),
        title: if track.title.trim().is_empty() {
            stem
        } else {
            track.title.clone()
        },
        duration_ms: track.duration_ms,
        file: rel.clone(),
        number: track.number,
        ..AddEpisode::default()
    }
    .to_bytes();
    let mut authored = false;
    for (name, input) in [(ADD_SHOW, add_show), (ADD_EPISODE, add_episode)] {
        match peer.mutate_new(name, input) {
            Ok(n) => authored |= n.is_some(),
            Err(why) => {
                tracing::warn!("{} was refused: {why}", path.display());
                return Offered::Read;
            }
        }
    }
    prints.set(print(track.artist));
    scan.to_hear(peer, &rel);
    match authored {
        true => Offered::Authored,
        false => Offered::Read,
    }
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
    /// The movement's name where the file names one ("I. Allegro"), the
    /// title tag otherwise.
    pub title: String,
    /// Who made it: the composer where the file names one, which on a
    /// classical recording is who the music is by; the artist tag otherwise.
    pub artist: String,
    pub album: String,
    pub duration_ms: i64,
    /// The tag's own numbering, 0 when the file has none.
    pub number: i64,
    /// Who played it: `ALBUMARTIST` where there is one, and where there is
    /// not and a composer is named, the artist tag — which on a classical
    /// file is then the performer. The pair the schema keeps apart.
    pub performer: String,
    /// `TBPM`. Rare, and the reason the column is worth storing rather than
    /// deriving: when a file states its tempo, that is a fact.
    pub bpm: i64,
    /// The disc, 0 when the file has none.
    pub disc: i64,
    /// The work it is a movement of (`WORK`, `©wrk`), empty for none.
    pub work: String,
    /// The work's catalogue number, read off the end of its title
    /// ("Sonata in F major, RV 68"): no tag carries one.
    pub catalogue: String,
    /// Where it sits in its work (`MVIN`, `©mvi`), 0 when nobody said.
    pub movement_no: i64,
    /// The year its tag says (`TDRC`, `DATE`, `©day`), 0 when it says none.
    pub year: i64,
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
///
/// **A classical file says so in standard tags**, and they are read: the
/// composer (`TCOM`, `©wrt`), the work (`TXXX:WORK`, `©wrk`), the movement
/// (`MVNM` and `MVIN`, `©mvn` and `©mvi`) and the disc. Named, the composer
/// is the artist and whoever the artist tag named played it; the movement's
/// name, numbered, is the title, so a work's page reads "I. Allegro" under
/// "Sonata in F major, RV 68" while the title tag can say both for players
/// that know no works.
pub fn read_with_picture(path: &Path) -> Option<(Track, Option<Vec<u8>>)> {
    use lofty::file::{AudioFile, TaggedFileExt};
    use lofty::probe::Probe;
    use lofty::tag::{Accessor, ItemKey};

    let tagged = Probe::open(path).ok()?.read().ok()?;
    let duration_ms = i64::try_from(tagged.properties().duration().as_millis()).unwrap_or(0);
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let text =
        |v: Option<std::borrow::Cow<'_, str>>| v.map(|s| s.trim().to_string()).unwrap_or_default();
    let item = |key: ItemKey| {
        tag.and_then(|t| t.get_string(key))
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    // A tag that is there but unparseable is the same as no tag: a number
    // nobody can read is not a number. `3/12` is 3.
    let number = |key: ItemKey| {
        let v = item(key);
        v.split('/')
            .next()
            .and_then(|n| n.trim().parse::<i64>().ok())
            .unwrap_or(0)
            .clamp(0, 999)
    };
    let movement_no = number(ItemKey::MovementNumber);
    let movement = item(ItemKey::Movement);
    let title = match (movement.is_empty(), movement_no) {
        (false, 0) => movement,
        (false, n) => format!("{}. {movement}", roman(n)),
        (true, _) => text(tag.and_then(|t| t.title())),
    };
    let title = if title.is_empty() {
        path.file_stem()?.to_str()?.trim().to_string()
    } else {
        title
    };
    if title.is_empty() {
        return None;
    }
    let (composer, artist) = (item(ItemKey::Composer), text(tag.and_then(|t| t.artist())));
    let performer = match item(ItemKey::AlbumArtist) {
        p if !p.is_empty() => p,
        _ if !composer.is_empty() && artist != composer => artist.clone(),
        _ => String::new(),
    };
    let work = item(ItemKey::Work);
    let track = Track {
        title,
        artist: if composer.is_empty() {
            artist
        } else {
            composer
        },
        album: text(tag.and_then(|t| t.album())),
        duration_ms,
        // The tag's own reading of it, which is where `3/12` becomes 3.
        number: tag
            .and_then(|t| t.track())
            .map_or(0, i64::from)
            .clamp(0, 999),
        performer,
        bpm: number(ItemKey::Bpm),
        disc: tag
            .and_then(|t| t.disk())
            .map_or(0, i64::from)
            .clamp(0, 999),
        catalogue: catalogue(&work),
        work,
        movement_no,
        year: tag.and_then(|t| t.date()).map_or(0, |d| i64::from(d.year)),
    };
    Some((track, covers::embedded(&tagged)))
}

/// The catalogue number a work's title ends with — "RV 68" of "Sonata in F
/// major, RV 68", "TWV 55:e1", "Op. 23", "BWV 988" — or empty: the last
/// comma's part, when it starts with an index scholars keep.
pub fn catalogue(work: &str) -> String {
    const INDEXES: &[&str] = &[
        "RV ", "BWV ", "HWV ", "TWV ", "K. ", "KV ", "Op. ", "D. ", "Hob. ", "WoO ", "L. ", "S. ",
    ];
    match work.rsplit_once(", ") {
        Some((_, last)) if INDEXES.iter().any(|i| last.starts_with(i)) => last.trim().to_string(),
        _ => String::new(),
    }
}

/// `n` as a movement is numbered: I, II, … XX, and plainly past that.
fn roman(n: i64) -> String {
    const R: [&str; 20] = [
        "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI", "XII", "XIII", "XIV",
        "XV", "XVI", "XVII", "XVIII", "XIX", "XX",
    ];
    match usize::try_from(n - 1).ok().and_then(|i| R.get(i)) {
        Some(r) => r.to_string(),
        None => n.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The prints and the pictures' index are rows: what a look moved
    /// reads back after the scanner's next start. Falsified by `save`
    /// putting nothing (its writes dropped): the next start had no print.
    #[test]
    fn the_prints_and_the_art_read_back_after_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let print = Print {
            path: "music/Bach/air.wav".into(),
            size: 44,
            mtime: 1_700_000_000_000_000_000,
            artist: "Bach".into(),
            covers: "|".into(),
        };
        let mut p = Prints::load(dir.path().join("prints"));
        p.set(print.clone());
        assert_eq!(
            p.get(&print.path),
            Some(print.clone()),
            "before it is saved"
        );
        p.save(&["abc.png".to_string()].into());
        let p = Prints::load(dir.path().join("prints"));
        assert_eq!(p.get(&print.path), Some(print));
        assert_eq!(p.art(), ["abc.png".to_string()].into());
    }

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
