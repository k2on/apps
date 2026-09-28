//! The media directory, as a peer.
//!
//! A server that owns a folder of files has to get them into the log
//! somehow, and there are only two honest ways: write the rows directly, or
//! be a peer and author mutations like everything else. This is the second,
//! and it is not a preference — writing rows would be a second definition of
//! what "add a song" means, and the first time it disagreed with the domain's
//! procedure the replicas would diverge with nothing to say so.
//!
//! So the scanner is an ordinary [`ark_client::Peer`], with its own replica
//! and its own pending queue on disk, dialling the hub in this process
//! (`HubHandle::dial`) instead of a socket. It gets acks, it gets the
//! rebase, and a file it adds fans out to every connected peer without
//! anyone asking. It signs in like everyone else — the engine holds every
//! entry to a session, and "the server wrote it" is not an exemption — as
//! the account [`ACCOUNT`].
//!
//! **Idempotency is not this file's doing.** `add_song` does nothing for a
//! file the library already has, inside the procedure, so a rescan is a
//! no-op for every peer replaying the log rather than only for the scanner
//! that happened to author it. What this file adds is not asking: the
//! replica says which files are already songs, and those are not offered —
//! so a restart over ten thousand files authors nothing at all.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use ark::store::Store;
use ark::value::Value;
use ark_auth::Login;
use ark_client::{args, Domain, Options, Peer};
use ark_server::HubHandle;
use notify::{RecursiveMode, Watcher};

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
    /// Look at one path the watcher saw appear or change.
    One(PathBuf),
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
    authored: Arc<AtomicUsize>,
    thread: Option<JoinHandle<()>>,
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
    /// hub as `login`, with the peer's replica kept in `dir`.
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
        domain: Domain,
        hub: HubHandle,
        dir: PathBuf,
        login: Login,
    ) -> Result<Scanner> {
        let opts = Options::server(
            login.user.id.clone(),
            login.session.clone(),
            Some(login.token.clone()),
        );
        let peer = Peer::open_path(domain, &dir, opts)
            .map_err(|e| anyhow::anyhow!("the scanner's replica in {}: {e}", dir.display()))?;
        let (tx, rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let authored = Arc::new(AtomicUsize::new(0));
        let thread = {
            let (root, stop, authored) = (root.clone(), stop.clone(), authored.clone());
            std::thread::Builder::new()
                .name("harken-scanner".into())
                .spawn(move || run(root, peer, hub, rx, &stop, &authored))
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
                // Creates and writes only. A removal is not handled on purpose:
                // the log is permanent, and a file disappearing is not evidence
                // that somebody meant to delete the song — an unplugged disk
                // looks exactly the same.
                if !matches!(
                    event.kind,
                    notify::EventKind::Create(_) | notify::EventKind::Modify(_)
                ) {
                    return;
                }
                for path in event.paths {
                    let _ = events.send(Nudge::One(path));
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
            authored,
            thread: Some(thread),
        })
    }

    /// Walk the whole directory again. Safe at any time: nothing already a
    /// song is offered, and `add_song` would do nothing with it if it were.
    pub fn rescan(&self) {
        let _ = self.tx.send(Nudge::All);
    }

    /// How many songs this scanner has authored since it started.
    pub fn authored(&self) -> usize {
        self.authored.load(Ordering::Relaxed)
    }
}

/// The scanner's whole life: a peer, and a loop over what it is told to
/// look at.
fn run(
    root: PathBuf,
    mut peer: Peer,
    hub: HubHandle,
    rx: Receiver<Nudge>,
    stop: &AtomicBool,
    authored: &AtomicUsize,
) {
    peer.connect_with("local", hub.dial());
    catch_up(&mut peer, &hub, stop);
    let music = root.join(MUSIC);

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
            let added = look(&mut peer, &root, &music, nudges);
            if added > 0 {
                authored.fetch_add(added, Ordering::Relaxed);
                println!("harken-server: library +{added}");
            }
        }
        // Whatever that produced, and whatever the log has for us. Not
        // conditional on a nudge: a quiet scanner still has to take the
        // entries other peers wrote.
        let pumped = peer.pump();
        if let Some(why) = pumped.denied {
            eprintln!("harken-server: library: the hub turned the scanner away: {why}");
        }
        for r in peer.take_rejections() {
            eprintln!("harken-server: library: an entry was refused: {}", r.reason);
        }
    }
    // Off the hub, with what is pending written down for the next start.
    peer.pump();
    peer.disconnect();
}

/// Pump until the replica has what the log had when it started, so that
/// "already a song" means the library and not an empty replica. Bounded: a
/// scanner that cannot catch up still scans, and the procedure keeps the
/// library right.
fn catch_up(peer: &mut Peer, hub: &HubHandle, stop: &AtomicBool) {
    let head = hub
        .read_blocking(|h| h.authority().log.head_seq())
        .unwrap_or(0);
    let deadline = Instant::now() + Duration::from_secs(30);
    while !stop.load(Ordering::Relaxed) && Instant::now() < deadline {
        peer.pump();
        if peer.linked() && peer.cursor() >= head && peer.pending_len() == 0 {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    if !stop.load(Ordering::Relaxed) {
        eprintln!("harken-server: library: not caught up with the log after 30s; scanning anyway");
    }
}

/// One look at whatever the nudges name. Says how many songs it authored.
fn look(peer: &mut Peer, root: &Path, music: &Path, nudges: Vec<Nudge>) -> usize {
    let mut paths = BTreeSet::new();
    for n in nudges {
        match n {
            Nudge::All => walk(music, &mut paths),
            // A *directory* appearing is the case that matters, and it is not
            // the same as a file appearing. Dropping an album folder in
            // creates the directory and its tracks in the same breath, and a
            // recursive watch has to add a watch for the new directory before
            // it can report anything inside it — so the tracks land in the gap
            // and are never announced. What is announced is the directory, so
            // that is what gets walked.
            Nudge::One(p) if p.is_dir() => walk(&p, &mut paths),
            Nudge::One(p) => {
                paths.insert(p);
            }
        }
    }
    // What the replica already has, once per look rather than once per file.
    let mut known = known_files(peer);
    let mut added = 0;
    for p in &paths {
        if offer(peer, &known, root, music, p) {
            added += 1;
            if let Some(rel) = relative(root, p) {
                known.insert(rel);
            }
        }
    }
    added
}

/// Every file the library already knows about.
fn known_files(peer: &Peer) -> BTreeSet<String> {
    peer.store()
        .scan("media")
        .into_iter()
        .filter_map(|row| match row.get("file") {
            Some(Value::Text(f)) if !f.is_empty() => Some(f.clone()),
            _ => None,
        })
        .collect()
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

/// Offer one path to the log. Says whether it authored anything.
fn offer(
    peer: &mut Peer,
    known: &BTreeSet<String>,
    root: &Path,
    music: &Path,
    path: &Path,
) -> bool {
    if !is_track(music, path) {
        return false;
    }
    let Some(rel) = relative(root, path) else {
        return false;
    };
    if known.contains(&rel) {
        return false;
    }
    let Some(track) = read(path) else {
        return false;
    };
    let call = args([
        ("title", Value::text(track.title)),
        ("artist", Value::text(track.artist)),
        ("album", Value::text(track.album)),
        ("duration_ms", Value::int(track.duration_ms)),
        ("file", Value::text(rel)),
        ("track", Value::int(track.number)),
        // Neither is in a standard tag: a part is a division of a work and a
        // catalogue number is a scholarly index, and no file carries either.
        // Left empty rather than guessed out of the folder names.
        ("part", Value::text("")),
        ("catalogue", Value::text("")),
        ("performer", Value::text(track.performer)),
        ("bpm", Value::int(track.bpm)),
        // No cover, from a scanner that cannot yet find one. Empty rather
        // than absent, and the procedure reads empty as "this entry has no
        // picture to offer" and leaves whatever is already on the row — so a
        // rescan of a library somebody has given covers to does not wipe them.
        ("album_art", Value::text("")),
        ("artist_art", Value::text("")),
        // No disc, no work, no movement number — none of which a plain tag
        // carries. 0 and empty are "nobody said": the track becomes a
        // recording of its own, which is what a pop track is.
        ("disc", Value::int(0)),
        ("work_title", Value::text("")),
        ("movement_no", Value::int(0)),
    ]);
    match peer.mutate("add_song", call) {
        Ok(_) => true,
        Err(e) => {
            eprintln!(
                "harken-server: library: {} was refused: {e}",
                path.display()
            );
            false
        }
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
    Some(Track {
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
    })
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
