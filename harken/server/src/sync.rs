//! What the server downloads into its library, from wherever it comes: one
//! engine under every source of it — the catalogue (`catalogue.rs`), the
//! followed feeds (`feeds.rs`) — so each says only what it is and where its
//! files are, and none of them downloads on its own.
//!
//! A source is read on a thread of its own ([`start`]) and says what it is
//! as [`Plan`]s: the mutations that put it in the library, and the files
//! those rows name. The scanner (`library.rs`) authors a plan before any of
//! its files is downloaded — each row as it is, and each file not here yet
//! queued (`library.queue_download`) — so a client lists a track or an
//! episode at once, marked as not on the server yet. Then the thread
//! downloads them one at a time, and says how each goes: `downloading`,
//! how far now and then ([`EVERY_SAID`]), and `library.downloaded` once it
//! is here, or `failed` and why. The next read tries a failed one again.
//!
//! A file is named by its URL's hash ([`name_of`]), so a read downloads
//! only what it has not got, and one interrupted leaves a `.part` that the
//! next starts over. Politely, as Wikimedia Commons — where the
//! catalogue's recordings are — asks: saying who is asking, one file at a
//! time with a pause between, and when told to slow down (429 or 503),
//! waiting as long as it is told and asking again. Nothing is deleted: a
//! row gone from its source stays.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

pub use crate::domain::catalogue::Call;

/// The most of one file downloaded: a long FLAC movement is a few hundred
/// megabytes, a sermon's video a few gigabytes.
const MAX: u64 = 4 << 30;

/// Who is asking, as Wikimedia's User-Agent policy asks a client to say.
const AGENT: &str = concat!(
    "harken-server/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/k2on/apps)"
);

/// The pause after each file downloaded.
const PAUSE: Duration = Duration::from_millis(500);

/// How many times a file told to wait is asked for, and the longest wait
/// taken: a host that says longer is tried at the next read.
const TRIES: u32 = 5;
const LONGEST: Duration = Duration::from_secs(300);

/// How many redirects a download follows. A podcast's episode is routinely
/// a chain of them: each analytics service a show uses (op3, Podtrac,
/// Chartable…) wraps the next one's link before the host's CDN — seven
/// hops for Full Time Nix's, where ureq's own five gave up on every
/// episode.
pub const REDIRECTS: u32 = 20;

/// How often a download says how far it has got: each saying is an entry
/// every client is sent, so a file is a handful of them, not one a block.
pub const EVERY_SAID: Duration = Duration::from_secs(2);

/// A read that could not have everything is tried again by itself: this
/// long after, and twice as long each time after, to [`RETRY_LONGEST`].
const RETRY_FIRST: Duration = Duration::from_secs(10 * 60);
const RETRY_LONGEST: Duration = Duration::from_secs(6 * 60 * 60);

/// Somewhere the library comes from.
pub trait Source: Send + 'static {
    /// What the server calls it when it says what it did.
    fn name(&self) -> &str;

    /// Read it: what it is, as plans, and how many of what a plan needs
    /// before it is authored (its pictures) could not be had. Blocking, and
    /// as long as its index and its pictures take; its files are the
    /// engine's to download.
    fn read(&mut self) -> (Vec<Plan>, usize);

    /// How long after a read it is read again by itself, when it is; a
    /// source with none is read when asked, and after a read that could not
    /// have everything.
    fn every(&self) -> Option<Duration> {
        None
    }
}

/// Part of a source, as the library has it: the mutations, in order, each
/// authored only if it would change something; then its files, each
/// queued if it is not here and downloaded.
#[derive(Clone, Debug, Default)]
pub struct Plan {
    /// What it is, for what the server prints.
    pub what: String,
    pub calls: Vec<Call>,
    /// The first call refused is the whole plan not authored, and none of
    /// its files downloaded: a feed nobody follows any more.
    pub gate: bool,
    pub files: Vec<Want>,
}

/// A file a plan's rows name: where its row says it is under the media
/// root, where it is downloaded from, and where it is kept on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Want {
    pub file: String,
    pub url: String,
    pub path: PathBuf,
}

impl Want {
    /// The file a URL is downloaded to: `<served>/<name_of(url)>` under the
    /// media root, kept as `<dir>/<name_of(url)>`.
    pub fn of(url: &str, served: &str, dir: &Path) -> Want {
        let name = name_of(url);
        Want {
            file: format!("{served}/{name}"),
            url: url.to_string(),
            path: dir.join(name),
        }
    }

    pub fn here(&self) -> bool {
        self.path.exists()
    }
}

/// What a source's thread tells the scanner, which authors it.
pub enum Said {
    /// A plan read: authored, and whether it was is answered — a gated
    /// plan refused is none of its files downloaded.
    Plan(Box<Plan>, Sender<bool>),
    /// A file about to be downloaded: `downloading`, and whether it still
    /// is to be — refused, its row is gone.
    Start(String, Sender<bool>),
    /// How far a download has got: bytes here, and of how many (0 for not
    /// said).
    Progress(String, u64, u64),
    /// A file here.
    Here(String),
    /// A file that could not be had, and why.
    Failed(String, String),
}

/// A source's thread, running: [`Reader::read`] asks it to read now.
/// Dropped, the thread ends after whatever it is downloading.
pub struct Reader {
    ask: Sender<()>,
}

impl Reader {
    pub fn read(&self) {
        let _ = self.ask.send(());
    }
}

/// `source` read on a thread of its own whenever it is asked, as often as
/// it says, and again after one that could not have everything; what it
/// read and how its downloads go said to `said`. A read asked while
/// another's files download stops after the file it is on, and reads
/// again: a feed just followed does not wait behind a catalogue's hundred
/// files, and what was left is still queued and planned again.
pub fn start<S: Source>(mut source: S, said: Sender<Said>) -> Reader {
    let (ask, asked) = channel::<()>();
    let name = format!("harken-{}", source.name());
    let _ = std::thread::Builder::new().name(name).spawn(move || {
        let (mut again, mut now): (Option<Duration>, bool) = (None, false);
        loop {
            if !now {
                let wait = [again, source.every()].into_iter().flatten().min();
                let gone = match wait {
                    None => asked.recv().is_err(),
                    Some(after) => matches!(asked.recv_timeout(after), Err(RecvTimeoutError::Disconnected)),
                };
                if gone {
                    return;
                }
            }
            while asked.try_recv().is_ok() {}
            let (plans, mut failed) = source.read();
            let Some((f, asked_again)) = fetch(&source, plans, &said, &asked) else {
                return;
            };
            (failed, now) = (failed + f, asked_again);
            again = (failed > 0).then(|| again.map_or(RETRY_FIRST, |a| (a * 2).min(RETRY_LONGEST)));
            if let (Some(after), false) = (again, now) {
                eprintln!(
                    "harken-server: {}: {failed} files could not be had; reading again in {} minutes",
                    source.name(),
                    after.as_secs() / 60
                );
            }
        }
    });
    Reader { ask }
}

/// Every plan said and, of those authored, every file not here downloaded:
/// how many could not be had, and whether a read was asked meanwhile — or
/// `None` when the scanner is gone.
fn fetch<S: Source>(
    source: &S,
    plans: Vec<Plan>,
    said: &Sender<Said>,
    asked: &Receiver<()>,
) -> Option<(usize, bool)> {
    let mut wanted = vec![];
    for plan in plans {
        let (yes, answer) = channel();
        let files = plan.files.clone();
        said.send(Said::Plan(Box::new(plan), yes)).ok()?;
        if answer.recv().ok()? {
            wanted.extend(files.into_iter().filter(|w| !w.here()));
        }
    }
    let agent = agent();
    let (mut failed, mut fetched) = (0, 0);
    for want in wanted {
        // Asked to read again: what is left is queued, and planned again.
        if asked.try_recv().is_ok() {
            return Some((failed, true));
        }
        if want.here() {
            said.send(Said::Here(want.file.clone())).ok()?;
            continue;
        }
        let (yes, answer) = channel();
        said.send(Said::Start(want.file.clone(), yes)).ok()?;
        if !answer.recv().ok()? {
            continue;
        }
        let mut last = Instant::now();
        let progress = |bytes: u64, total: u64| {
            if last.elapsed() >= EVERY_SAID {
                last = Instant::now();
                let _ = said.send(Said::Progress(want.file.clone(), bytes, total));
            }
        };
        match download(&agent, &want.url, &want.path, progress) {
            Ok(()) => {
                fetched += 1;
                said.send(Said::Here(want.file.clone())).ok()?;
                std::thread::sleep(PAUSE);
            }
            Err(e) => {
                failed += 1;
                eprintln!("harken-server: {}: {e}", source.name());
                said.send(Said::Failed(want.file.clone(), e)).ok()?;
            }
        }
    }
    if fetched > 0 || failed > 0 {
        eprintln!(
            "harken-server: {}: {fetched} downloaded, {failed} failed",
            source.name()
        );
    }
    Some((failed, false))
}

/// The client every download is made with, saying who is asking.
pub fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(120))
        .redirects(REDIRECTS)
        .user_agent(AGENT)
        .build()
}

/// A URL's file name: its hash, with the extension its path ends in, which
/// is what `/media` takes its type from.
pub fn name_of(url: &str) -> String {
    let hash = format!("{:x}", Sha256::digest(url.as_bytes()));
    let path = url.split(['?', '#']).next().unwrap_or("");
    let last = path.rsplit('/').next().unwrap_or("");
    let ext = last
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .filter(|e| (1..=5).contains(&e.len()) && e.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or_else(|| "bin".into());
    format!("{}.{ext}", &hash[..32])
}

/// A text at `url`: a catalogue's JSON, a feed's RSS.
pub fn text(agent: &ureq::Agent, url: &str) -> Result<String, String> {
    get(agent, url)?
        .into_string()
        .map_err(|e| format!("{url}: {e}"))
}

/// One file into `path`, by way of `<path>.part`, so a file there is whole;
/// `progress` told how many bytes are here and of how many (0 when the
/// host does not say) as they come.
pub fn download(
    agent: &ureq::Agent,
    url: &str,
    path: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<(), String> {
    let response = get(agent, url)?;
    let total = response
        .header("content-length")
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or(0);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let part = path.with_extension("part");
    let mut file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut reader = response.into_reader().take(MAX + 1);
    let mut buf = vec![0u8; 64 * 1024];
    let mut copied = 0u64;
    let copy = loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break Ok(()),
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => break Err(e.to_string()),
        };
        if let Err(e) = std::io::Write::write_all(&mut file, &buf[..n]) {
            break Err(e.to_string());
        }
        copied += n as u64;
        if copied > MAX {
            break Err(format!("more than {MAX} bytes"));
        }
        progress(copied, total);
    };
    if let Err(e) = copy {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    std::fs::rename(&part, path).map_err(|e| e.to_string())
}

/// A picture a plan's rows name, downloaded before they are authored: its
/// file under the media root, or `None` when there is none or it could
/// not be had (counted in `failed`).
pub fn picture(
    agent: &ureq::Agent,
    url: &str,
    served: &str,
    dir: &Path,
    failed: &mut usize,
) -> Option<String> {
    if url.is_empty() {
        return None;
    }
    let want = Want::of(url, served, dir);
    if !want.here() {
        if let Err(e) = download(agent, url, &want.path, |_, _| {}) {
            *failed += 1;
            eprintln!("harken-server: {e}");
            return None;
        }
    }
    Some(want.file)
}

/// A response to `url`, asked again — up to [`TRIES`] times — after as long
/// as a 429 or 503 says to wait (`Retry-After`, in seconds; 30 when it does
/// not say), unless that is longer than [`LONGEST`].
fn get(agent: &ureq::Agent, url: &str) -> Result<ureq::Response, String> {
    let mut tries = 0;
    loop {
        tries += 1;
        match agent.get(url).call() {
            Ok(response) => return Ok(response),
            Err(ureq::Error::Status(code @ (429 | 503), response)) if tries < TRIES => {
                let wait = response
                    .header("retry-after")
                    .and_then(|s| s.trim().parse().ok())
                    .map_or(Duration::from_secs(30), Duration::from_secs);
                if wait > LONGEST {
                    return Err(format!(
                        "status code {code}, and to wait {}s",
                        wait.as_secs()
                    ));
                }
                std::thread::sleep(wait);
            }
            // ureq says the URL itself.
            Err(e) => return Err(e.to_string()),
        }
    }
}
