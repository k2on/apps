//! What the server downloads into its library, from wherever it comes:
//! what every source of it — the catalogue (`catalogue.rs`), the followed
//! feeds (`feeds.rs`) — is read as, and how a file is downloaded, so each
//! says only what it is and where its files are.
//!
//! A source is read as a job (a `Source` row, `library.rs`) and says what
//! it is as [`Plan`]s: the mutations that put it in the library, and the
//! files those rows name. The scanner authors a plan before any of its
//! files is downloaded — each row as it is, and each file not here yet
//! queued (`library.queue_download`) — so a client lists a track or an
//! episode at once, marked as not on the server yet. Each file queued is a
//! job of its own (`Fetch`), downloaded one at a time: `downloading`, how
//! far now and then ([`EVERY_SAID`]), and `library.downloaded` once it is
//! here, or `failed` and why, and tried again later.
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
use std::time::Duration;

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
pub const PAUSE: Duration = Duration::from_millis(500);

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
            tracing::warn!(target: "harken", url, "a picture could not be had: {e}");
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
