//! A catalogue imported (`HARKEN_CATALOGUE`, a URL of the JSON
//! github.com/k2on/classical publishes): its recordings and pictures
//! downloaded into the data directory and served at `/media/.catalogue/`,
//! and its library authored by the scanner — the mutations
//! `harken_domain::catalogue` says it is, each only if it would change
//! something (`library.rs`).
//!
//! A file is named by its URL's hash, so a sync downloads only what it has
//! not got, and one interrupted leaves a `.part` that the next starts over.
//! A file that fails to download is left out until it does: its track is
//! not authored, and its picture is no picture, and the sync is tried again
//! later (`library.rs`).
//!
//! Politely, as Wikimedia Commons — where its recordings are — asks: saying
//! who is asking, one file at a time with a pause between, and when told to
//! slow down (429 or 503), waiting as long as it is told and asking again.
//! Ark's first sync asked 248 files in a second as `ureq/2`, and all but
//! four were 429. Nothing is deleted, as the
//! scanner deletes nothing: a recording gone from the catalogue stays.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::domain::catalogue::Catalogue;

/// Where under `/media` the downloads are served.
pub const DIR: &str = ".catalogue";

/// The most of one file it downloads: a long FLAC movement is a few hundred
/// megabytes.
const MAX: u64 = 2 << 30;

/// Who is asking, as Wikimedia's User-Agent policy asks a client to say.
const AGENT: &str = concat!(
    "harken-server/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/k2on/apps)"
);

/// The pause after each file downloaded.
const PAUSE: Duration = Duration::from_millis(500);

/// How many times a file told to wait is asked for, and the longest wait
/// taken: a host that says longer is tried at the next sync.
const TRIES: u32 = 5;
const LONGEST: Duration = Duration::from_secs(300);

pub fn dir(data: &Path) -> PathBuf {
    data.join("catalogue")
}

/// A catalogue, and for each URL of it that is here, what a row holds for
/// it: its path under the media root (`.catalogue/<hash>.<ext>`).
pub struct Fetched {
    pub catalogue: Catalogue,
    pub here: BTreeMap<String, String>,
    /// How many of its files could not be had: the next sync tries them.
    pub failed: usize,
}

/// The catalogue at `url`, and everything it names downloaded into `dir`.
/// Blocking, and as long as the downloads take: the people's pictures
/// first, then album by album, and after each that downloaded anything,
/// what is here so far given to `so_far`, so a library fills as it goes
/// rather than all at once at the end.
pub fn fetch(url: &str, dir: &Path, mut so_far: impl FnMut(Fetched)) -> Result<Fetched, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(120))
        .user_agent(AGENT)
        .build();
    let json = agent
        .get(url)
        .call()
        .map_err(|e| format!("{url}: {e}"))?
        .into_string()
        .map_err(|e| format!("{url}: {e}"))?;
    let catalogue = Catalogue::parse(&json)?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let people = catalogue
        .people
        .iter()
        .flat_map(|p| [p.art.as_str(), p.banner.as_str()]);
    let mut groups = vec![("people".to_string(), people.collect::<Vec<_>>())];
    for a in &catalogue.albums {
        let files = a.tracks.iter().map(|t| t.file.as_str());
        groups.push((
            a.name.clone(),
            std::iter::once(a.art.as_str()).chain(files).collect(),
        ));
    }
    let (mut here, mut fetched, mut failed) = (BTreeMap::new(), 0, 0);
    for (group, urls) in &groups {
        let (was, mut new) = (failed, 0);
        for &u in urls.iter().filter(|u| !u.is_empty()) {
            if here.contains_key(u) {
                continue;
            }
            let name = name_of(u);
            let path = dir.join(&name);
            if !path.exists() {
                match download(&agent, u, &path) {
                    Ok(()) => {
                        new += 1;
                        std::thread::sleep(PAUSE);
                    }
                    Err(e) => {
                        failed += 1;
                        eprintln!("harken-server: catalogue: {u}: {e}");
                        continue;
                    }
                }
            }
            here.insert(u.to_string(), format!("{DIR}/{name}"));
        }
        fetched += new;
        if new > 0 {
            eprintln!(
                "harken-server: catalogue: {group}: {new} downloaded, {} failed ({} files here)",
                failed - was,
                here.len()
            );
            so_far(Fetched {
                catalogue: catalogue.clone(),
                here: here.clone(),
                failed,
            });
        }
    }
    eprintln!(
        "harken-server: catalogue: {} files here, {fetched} downloaded now, {failed} failed",
        here.len()
    );
    Ok(Fetched {
        catalogue,
        here,
        failed,
    })
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

/// One file into `path`, by way of `<path>.part`, so a file there is whole.
fn download(agent: &ureq::Agent, url: &str, path: &Path) -> Result<(), String> {
    let response = get(agent, url)?;
    let part = path.with_extension("part");
    let mut file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let copied = std::io::copy(&mut response.into_reader().take(MAX + 1), &mut file)
        .map_err(|e| e.to_string())?;
    if copied > MAX {
        let _ = std::fs::remove_file(&part);
        return Err(format!("more than {MAX} bytes"));
    }
    std::fs::rename(&part, path).map_err(|e| e.to_string())
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
            Err(e) => return Err(e.to_string()),
        }
    }
}
