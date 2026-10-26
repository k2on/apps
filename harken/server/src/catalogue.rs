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
//! not authored, and its picture is no picture. Nothing is deleted, as the
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

pub fn dir(data: &Path) -> PathBuf {
    data.join("catalogue")
}

/// A catalogue, and for each URL of it that is here, what a row holds for
/// it: its path under the media root (`.catalogue/<hash>.<ext>`).
pub struct Fetched {
    pub catalogue: Catalogue,
    pub here: BTreeMap<String, String>,
}

/// The catalogue at `url`, and everything it names downloaded into `dir`.
/// Blocking, and as long as the downloads take.
pub fn fetch(url: &str, dir: &Path) -> Result<Fetched, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(120))
        .build();
    let json = agent
        .get(url)
        .call()
        .map_err(|e| format!("{url}: {e}"))?
        .into_string()
        .map_err(|e| format!("{url}: {e}"))?;
    let catalogue = Catalogue::parse(&json)?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let (mut here, mut fetched, mut failed) = (BTreeMap::new(), 0, 0);
    for u in catalogue.urls() {
        let name = name_of(u);
        let path = dir.join(&name);
        if !path.exists() {
            match download(&agent, u, &path) {
                Ok(()) => fetched += 1,
                Err(e) => {
                    failed += 1;
                    eprintln!("harken-server: catalogue: {u}: {e}");
                    continue;
                }
            }
        }
        here.insert(u.to_string(), format!("{DIR}/{name}"));
    }
    eprintln!(
        "harken-server: catalogue: {} files here, {fetched} downloaded now, {failed} failed",
        here.len()
    );
    Ok(Fetched { catalogue, here })
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
    let response = agent.get(url).call().map_err(|e| e.to_string())?;
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
