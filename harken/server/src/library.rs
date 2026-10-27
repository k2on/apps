//! The media directory, as a peer.
//!
//! A server that owns a folder of files has to get them into the log
//! somehow, and there are only two honest ways: write the rows directly, or
//! be a peer and author mutations like everything else. This is the second,
//! and it is not a preference — writing rows would be a second definition of
//! what "add a song" means, and the first time it disagreed with the domain's
//! procedure the replicas would diverge with nothing to say so.
//!
//! So the scanner is an ordinary [`ark_peer::Peer`], with its own
//! replica and its own pending intents on disk, its link dialling the hub
//! in this process (`Handle::dialer`) instead of a socket. It gets its
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
//! **A look is a job** (`ark::effect`, `docs/plan-v5.md` S3a). What the
//! scanner is told to look at — everything at boot or on a rescan, the
//! paths a watch saw settle — is asked for as a [`Look`] row
//! (`library.look`), and the scanner's handler, registered on its own peer
//! and on no other, claims it, does it, and finishes it with what it read
//! and authored. A look asked and not done before a stop is done after the
//! next start, once its claim's lease has run out.
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
//! were found its covers. The prints are this server's, never the log's —
//! rows of a local instance of its own ([`HarkenLocal`]), beside the
//! pictures it took out of files — and missing or unreadable they are a
//! full read, and nothing worse.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::domain::{
    AddEpisode, AddShow, AddSong, DownloadProgress, Downloaded, Harken, Look, LookAt, Looked,
    QueueDownload, Subscription,
};
use anyhow::{Context, Result};
use ark::format::Encode;
use ark::peer::{Ctx, Who};
use ark::prelude::{Deserialize, Row, Schema, Serialize};
use ark::store::{Dir, Read, Scan as Range, Store};
use ark_auth::Login;
use ark_peer::{Config, Files, Handle, Instance};
use notify::{RecursiveMode, Watcher};

use crate::covers::{self, Covers};
use crate::hosted::Hosted;
use crate::sync::{Reader, Said};

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

/// The mutation a file is offered as.
const ADD_SONG: &str = "library.add_song";

/// The mutations a show and an episode are offered as.
const ADD_SHOW: &str = "library.add_show";
const ADD_EPISODE: &str = "library.add_episode";

/// The mutations a download is said with.
const QUEUE_DOWNLOAD: &str = "library.queue_download";
const DOWNLOAD_PROGRESS: &str = "library.download_progress";
const DOWNLOADED: &str = "library.downloaded";

/// The mutation a look is asked with.
const LOOK: &str = "library.look";

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
    /// ([`replica_dir`], [`prints_dir`], [`art_dir`]).
    ///
    /// The walk is narrow and the watch is wide, on purpose: only `music/`
    /// holds tracks, but watching the root is what notices that directory
    /// being created at all — on a fresh install it may not exist yet, and a
    /// watch on a path that is not there watches nothing forever.
    ///
    /// What is downloaded into the library comes from sources, each read
    /// on a thread of its own (`sync.rs`) and authored here, its files
    /// queued and then said as they download: with `catalogue`, the URL
    /// of one (`catalogue.rs`), read at every full look (at boot, and
    /// whenever a rescan is asked for); and the feeds the library follows
    /// (`feeds.rs`), read at every full look, every hour, and whenever
    /// somebody follows one or asks for more of it.
    ///
    /// Returns at once: the walk happens on a thread of its own, so a
    /// library of ten thousand files does not hold up anything.
    pub fn start(
        root: PathBuf,
        hosted: Hosted,
        hub: Handle,
        data: PathBuf,
        login: Login,
        catalogue: Option<String>,
        transcriber: Option<crate::transcribe::Hearing>,
    ) -> Result<Scanner> {
        // `docs/plan-auth.md` The scanner is the library, by construction: it
        // holds the library's role whatever else its login holds, and its
        // entries carry it. The server grants the account the same role
        // (`start`), so the device and the authority agree on it — which is
        // what `is_library` asks, on the device and, of the roles the hub
        // stamps, at the authority.
        let mut roles = login.user.roles.clone();
        roles.push(crate::domain::LIBRARY.to_string());
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
        let config = Config::of(hosted.domain.clone());
        let mut client = ark_peer::Peer::open(config, who.clone(), Box::new(disk), false);
        // Each start is a login of its own: what the replica kept pending
        // is authored again under it, from its session's first `local`,
        // or the hub would hold the first entry for the ones before it.
        client.sign_in(who.clone());
        // In this process, the token is the scanner's and proves its login.
        let token = login.token.clone();
        let dial = hub.dialer(Arc::new(move |t: &str| (t == token).then(|| who.clone())));
        client.set_token(Some(login.token.clone()));
        client.connect_with("", dial);
        let (tx, rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let counts = Arc::new(Counts::default());
        // The sources' threads: a read asked is however long its index,
        // its pictures and then its files take, so it is not the scanner's
        // to wait on; what it read and how its downloads go come back as
        // [`Said`], authored here.
        let (said, heard) = channel::<Said>();
        let sync = catalogue.map(|url| {
            let dir = crate::catalogue::dir(&data);
            crate::sync::start(crate::catalogue::Import { url, dir }, said.clone())
        });
        let subscriptions = Arc::new(Mutex::new(Vec::new()));
        let transcribe = transcriber.map(|hearing| {
            crate::transcribe::Transcriber::start(
                hearing,
                crate::transcribe::dir(&data),
                said.clone(),
            )
        });
        let follow = crate::sync::start(
            crate::feeds::Follow {
                subscriptions: subscriptions.clone(),
                dir: crate::feeds::dir(&data),
            },
            said,
        );
        let thread = {
            let (stop, counts) = (stop.clone(), counts.clone());
            let scan = Scan {
                music: root.join(MUSIC),
                root: root.clone(),
                art: art_dir(&data),
                prints: Prints::load(prints_dir(&data)),
                sync,
                follow,
                subscriptions,
                followed: None,
                feeds: crate::feeds::dir(&data),
                transcribe,
                listed: None,
            };
            std::thread::Builder::new()
                .name("harken-scanner".into())
                .spawn(move || run(scan, client, hub, rx, heard, &stop, counts))
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
                eprintln!(
                    "harken-server: library: cannot keep prints in {}: {e}",
                    dir.display()
                );
                Box::new(ark_peer::Memory::default())
            }
        };
        let local = Instance::open::<HarkenLocal>(disk);
        if let Some(e) = &local.failure {
            eprintln!(
                "harken-server: library: {} is not readable ({e}); every file is read again",
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
        if let Some(e) = kept.err().map(|r| r.0).or(self.local.failure.clone()) {
            eprintln!("harken-server: library: cannot keep the prints: {e}");
        }
    }
}

/// The scanner's own state on its thread: where things are, and the prints.
struct Scan {
    root: PathBuf,
    music: PathBuf,
    art: PathBuf,
    prints: Prints,
    /// The catalogue's reader, when there is a catalogue.
    sync: Option<Reader>,
    /// The followed feeds' reader.
    follow: Reader,
    /// The feeds followed, as the feeds' reader reads them: the replica's
    /// `subscription` rows, kept here as they move.
    subscriptions: Arc<Mutex<Vec<Subscription>>>,
    /// What of them the reader was last asked to read with.
    followed: Option<Vec<(String, String, i64)>>,
    /// Where a feed's episodes are on disk.
    feeds: PathBuf,
    /// Who hears the episodes here, when anybody does, and the replica's
    /// head when it was last told which they are.
    transcribe: Option<crate::transcribe::Transcriber>,
    listed: Option<ark::log::Seq>,
}

impl Scan {
    /// The replica's followed feeds handed to the feeds' reader, and a
    /// read asked of it, when they moved since it last was: a feed
    /// followed, its kind changed, or more of it asked for. Its title and
    /// what the last read said are the reader's own writes, and ask
    /// nothing.
    fn follow(&mut self, peer: &ark_peer::Peer) {
        let now: Vec<Subscription> =
            ark::db::read::<Harken, _>(&peer.log.store, |db| db.subscription.scan());
        let moved: Vec<(String, String, i64)> = now
            .iter()
            .map(|s| (s.url.clone(), s.kind.clone(), s.keep))
            .collect();
        if self.followed.as_ref() == Some(&moved) {
            return;
        }
        let first = self.followed.is_none();
        self.followed = Some(moved);
        if let Ok(mut s) = self.subscriptions.lock() {
            *s = now;
        }
        // The first is the boot's, whose full look asks for a read itself.
        if !first {
            self.follow.read();
        }
    }
}

impl Scan {
    /// The episodes whose files are here, newest first, told to the
    /// transcriber whenever the replica moved: a download finished is an
    /// entry, and one not finished is a `.part` beside its name, not yet
    /// here.
    fn transcribe(&mut self, peer: &ark_peer::Peer) {
        let Some(transcribe) = &self.transcribe else {
            return;
        };
        let head = peer.log.store.log().head().0;
        if self.listed == Some(head) {
            return;
        }
        self.listed = Some(head);
        let mut here: Vec<(i64, i64, crate::transcribe::Job)> =
            ark::db::read::<Harken, _>(&peer.log.store, |db| {
                let chaptered: std::collections::HashSet<_> =
                    db.chapter.scan().into_iter().map(|c| c.media_id).collect();
                db.episode
                    .scan()
                    .into_iter()
                    .filter_map(|e| {
                        let m = db.media.get(e.media_id)?;
                        let chaptered = chaptered.contains(&e.media_id);
                        let path = match m.file.strip_prefix(&format!("{}/", crate::feeds::DIR)) {
                            Some(name) => self.feeds.join(name),
                            None => self.root.join(&m.file),
                        };
                        let job = crate::transcribe::Job {
                            file: m.file.clone(),
                            path,
                            title: m.title.clone(),
                            description: e.description.clone(),
                            chaptered,
                        };
                        Some((e.published_ms, m.pos, job))
                    })
                    .collect()
            });
        here.retain(|(_, _, j)| j.path.is_file());
        here.sort_by_key(|(published, pos, _)| std::cmp::Reverse((*published, *pos)));
        transcribe.want(here.into_iter().map(|(_, _, j)| j).collect());
    }
}

/// The scanner's peer pumped — what the link said given to it, what it
/// says back, and the looks its handler does — and a refused entry said
/// out loud.
fn pump(peer: &mut ark_peer::Peer) {
    peer.pump();
    for (_, why) in peer.take_rejections() {
        eprintln!("harken-server: library: an entry was refused: {why}");
    }
}

/// The scanner's whole life: a peer whose handler does each look asked of
/// the library (a [`Look`] row, `docs/plan-v5.md` S3a), and a loop turning
/// what it is told to look at into looks asked.
fn run(
    mut scan: Scan,
    mut peer: ark_peer::Peer,
    hub: Handle,
    rx: Receiver<Nudge>,
    heard: Receiver<Said>,
    stop: &AtomicBool,
    counts: Arc<Counts>,
) {
    catch_up(&mut peer, &hub, stop);
    let root = scan.root.clone();
    let synced = counts.clone();
    scan.follow(&peer);
    // The look's handler and the loop both have the scan: the handler runs
    // inside the loop's pump, one at a time.
    let watch = Arc::new(Mutex::new(scan));
    let looks = watch.clone();
    peer.handle::<Look>(move |p, job| {
        let Ok(mut scan) = looks.lock() else {
            return Ok(Looked {
                read: 0,
                authored: 0,
            });
        };
        let scan = &mut *scan;
        let nudges = match job.paths.is_empty() {
            true => vec![Nudge::All],
            false => job
                .paths
                .iter()
                .map(|r| Nudge::One(scan.root.join(r)))
                .collect(),
        };
        // A full look is every source read too: at boot, and the rescan a
        // person asks for.
        if job.paths.is_empty() {
            if let Some(sync) = &scan.sync {
                sync.read();
            }
            scan.follow.read();
        }
        let (authored, read) = look(p, scan, nudges);
        counts.read.fetch_add(read, Ordering::Relaxed);
        if authored > 0 {
            counts.authored.fetch_add(authored, Ordering::Relaxed);
            println!("harken-server: library +{authored}");
        }
        let (read, authored) = (read as i64, authored as i64);
        Ok(Looked { read, authored })
    });
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
            ask(&mut peer, &root, nudges);
        }
        // What the sources read and how their downloads go, authored.
        for said in heard.try_iter() {
            let authored = hear(&mut peer, said);
            synced.authored.fetch_add(authored, Ordering::Relaxed);
        }
        // Whatever that produced, and whatever the log has for us — the
        // looks asked among it. Not conditional on a nudge: a quiet
        // scanner still has to take the entries other peers wrote.
        pump(&mut peer);
        if let Ok(mut scan) = watch.lock() {
            scan.follow(&peer);
            scan.transcribe(&peer);
        }
    }
    // Off the hub, with what is pending written down for the next start (the
    // client writes its pending intents as they change).
    pump(&mut peer);
}

/// A look asked for: at the paths the nudges name, under the media root,
/// or at everything.
fn ask(peer: &mut ark_peer::Peer, root: &Path, nudges: Vec<Nudge>) {
    let all = nudges.iter().any(|n| matches!(n, Nudge::All));
    let paths = nudges.into_iter().filter_map(|n| match n {
        Nudge::One(p) if !all => relative(root, &p),
        _ => None,
    });
    let paths: Vec<String> = paths.collect::<BTreeSet<_>>().into_iter().collect();
    if all || !paths.is_empty() {
        if let Err(why) = peer.mutate(LOOK, &LookAt { paths }) {
            eprintln!("harken-server: library: a look was refused: {why}");
        }
    }
}

/// Pump until the replica has what the log had when it started, so that
/// "already a song" means the library and not an empty replica. Bounded: a
/// scanner that cannot catch up still scans, and the procedure keeps the
/// library right.
fn catch_up(peer: &mut ark_peer::Peer, hub: &Handle, stop: &AtomicBool) {
    let head = hub.with(|h| h.log.log().head().0).unwrap_or(0);
    let deadline = Instant::now() + Duration::from_secs(30);
    while !stop.load(Ordering::Relaxed) && Instant::now() < deadline {
        pump(peer);
        if peer.linked && peer.log.log().head().0 >= head && peer.log.pending.is_empty() {
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
fn look(peer: &mut ark_peer::Peer, scan: &mut Scan, nudges: Vec<Nudge>) -> (usize, usize) {
    let mut paths = BTreeSet::new();
    for n in nudges {
        match n {
            Nudge::All => {
                walk(&scan.music, &mut paths);
                for (dir, _) in SHOWS {
                    walk(&scan.root.join(dir), &mut paths);
                }
            }
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
    let mut covers = Covers::new(&scan.root, &scan.music, &scan.art, scan.prints.art());
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
    scan.prints.save(covers.written());
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
    let mut key = vec![];
    ark::format::put_key(file, &mut key);
    let range = Range::prefix(index, &key, Dir::Asc, 1);
    !store.scan("media", &range).is_empty()
}

/// What a source said, authored: a plan's rows and its files queued, or
/// how one of its downloads goes. How many entries it authored.
fn hear(peer: &mut ark_peer::Peer, said: Said) -> usize {
    match said {
        Said::Plan(plan, answer) => {
            let (authored, yes) = author(peer, &plan);
            let _ = answer.send(yes);
            if authored > 0 {
                println!("harken-server: {} +{authored}", plan.what);
            }
            authored
        }
        // Refused, its row is gone: not downloaded.
        Said::Start(file, answer) => {
            let going = progress(&file, "downloading", 0, 0, "");
            let yes = !matches!(would(peer, DOWNLOAD_PROGRESS, &going), Would::Refuse);
            let authored = yes && author_one(peer, DOWNLOAD_PROGRESS, &file, going);
            let _ = answer.send(yes);
            authored as usize
        }
        Said::Progress(file, bytes, total) => {
            let going = progress(&file, "downloading", bytes as i64, total as i64, "");
            author_one(peer, DOWNLOAD_PROGRESS, &file, going) as usize
        }
        Said::Failed(file, why) => {
            let failed = progress(&file, "failed", 0, 0, &why);
            author_one(peer, DOWNLOAD_PROGRESS, &file, failed) as usize
        }
        Said::Here(file) => {
            let here = Downloaded { file: file.clone() }.to_bytes();
            author_one(peer, DOWNLOADED, &file, here) as usize
        }
    }
}

fn progress(file: &str, state: &str, bytes: i64, total: i64, error: &str) -> Vec<u8> {
    DownloadProgress {
        file: file.to_string(),
        state: state.to_string(),
        bytes,
        total,
        error: error.to_string(),
    }
    .to_bytes()
}

/// An entry authored if it would change something, a refusal said: whether
/// one was.
fn author_one(peer: &mut ark_peer::Peer, name: &str, what: &str, input: Vec<u8>) -> bool {
    if !would_change(peer, name, &input) {
        return false;
    }
    match peer.mutate_bytes(name, input) {
        Ok(_) => true,
        Err(why) => {
            eprintln!("harken-server: library: {what} ({name}) was refused: {why}");
            false
        }
    }
}

/// A plan authored: each of its mutations that would change something,
/// then each of its files queued, or said to be here when it already is.
/// How many entries it authored, and whether its files are to be
/// downloaded — not when it is gated and its first mutation is refused.
fn author(peer: &mut ark_peer::Peer, plan: &crate::sync::Plan) -> (usize, bool) {
    let mut authored = 0;
    for (n, call) in plan.calls.iter().enumerate() {
        match would(peer, call.name, &call.input) {
            Would::Nothing => continue,
            Would::Refuse if plan.gate && n == 0 => return (0, false),
            // A describe of a work this library has not got.
            Would::Refuse if call.if_there => continue,
            Would::Change | Would::Refuse => {}
        }
        match peer.mutate_bytes(call.name, call.input.clone()) {
            Ok(_) => authored += 1,
            Err(why) => eprintln!(
                "harken-server: {}: {} ({}) was refused: {why}",
                plan.what, call.what, call.name
            ),
        }
    }
    for want in &plan.files {
        let input = match want.here() {
            true => (
                DOWNLOADED,
                Downloaded {
                    file: want.file.clone(),
                }
                .to_bytes(),
            ),
            // Queued again only from nothing or a failure: a file queued
            // or downloading already is as it was.
            false if queued(peer, &want.file) => continue,
            false => (
                QUEUE_DOWNLOAD,
                QueueDownload {
                    file: want.file.clone(),
                    source: want.url.clone(),
                }
                .to_bytes(),
            ),
        };
        authored += author_one(peer, input.0, &want.file, input.1) as usize;
    }
    (authored, true)
}

/// Whether a file is queued or downloading already, as the replica has it.
fn queued(peer: &ark_peer::Peer, file: &str) -> bool {
    ark::db::read::<Harken, _>(&peer.log.store, |db| {
        let media = db
            .media
            .by(crate::domain::Media::FILE)
            .from(&file.to_string())
            .first();
        media
            .filter(|m| m.file == file)
            .and_then(|m| db.download.get(m.id))
            .is_some_and(|d| d.state != "failed")
    })
}

/// Whether an entry would write anything over what the replica holds: the
/// mutation run over it, its changes looked at and thrown away. What the
/// procedure does with an entry saying nothing new is write nothing, so
/// not authoring one is the same library with one entry fewer in the log.
fn would_change(peer: &ark_peer::Peer, name: &str, input: &[u8]) -> bool {
    !matches!(would(peer, name, input), Would::Nothing)
}

/// What an entry would do over what the replica holds.
enum Would {
    Change,
    Nothing,
    Refuse,
}

fn would(peer: &ark_peer::Peer, name: &str, input: &[u8]) -> Would {
    let me = &peer.log.me;
    let ctx = Ctx {
        id: peer.log.next_id(),
        ..Ctx::new(me, ark_peer::now_ms())
    };
    let Some(f) = peer.domain().get(name).and_then(|f| f.proc.clone()) else {
        return Would::Change;
    };
    let mut store = peer.log.store.clone();
    match f.apply(&mut store, &ctx, input) {
        Ok(changes) if changes.is_empty() => Would::Nothing,
        Ok(_) => Would::Change,
        Err(_) => Would::Refuse,
    }
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

/// Offer one path to the log.
fn offer(peer: &mut ark_peer::Peer, scan: &mut Scan, covers: &mut Covers, path: &Path) -> Offered {
    if let Some((kind, show)) = is_episode(&scan.root, path) {
        return offer_episode(peer, scan, covers, path, kind, &show);
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
    if let Some(was) = scan.prints.get(&rel) {
        let now = Print {
            path: rel.clone(),
            size,
            mtime,
            artist: was.artist.clone(),
            covers: covers.beside(dir, &was.artist).print(),
        };
        if was == now && known_file(&peer.log.store, &rel) {
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
    }
    .to_bytes();
    if !would_change(peer, ADD_SONG, &call) {
        scan.prints.set(print);
        return Offered::Read;
    }
    match peer.mutate_bytes(ADD_SONG, call) {
        Ok(_) => {
            scan.prints.set(print);
            Offered::Authored
        }
        Err(why) => {
            eprintln!(
                "harken-server: library: {} was refused: {why}",
                path.display()
            );
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
    peer: &mut ark_peer::Peer,
    scan: &mut Scan,
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
    if let Some(was) = scan.prints.get(&rel) {
        if was == print(was.artist.clone()) && known_file(&peer.log.store, &rel) {
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
        if !would_change(peer, name, &input) {
            continue;
        }
        match peer.mutate_bytes(name, input) {
            Ok(_) => authored = true,
            Err(why) => {
                eprintln!(
                    "harken-server: library: {} was refused: {why}",
                    path.display()
                );
                return Offered::Read;
            }
        }
    }
    scan.prints.set(print(track.artist));
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
