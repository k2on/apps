//! A catalogue imported (`HARKEN_CATALOGUE`, a URL of the JSON
//! github.com/k2on/classical publishes): a source of the library
//! (`sync.rs`), read as a job (`library.rs`) at every full look. Its
//! pictures are downloaded into the data directory as it is read; its
//! library is authored — the mutations `harken_domain::catalogue` says it
//! is, each only if it would change something — with each recording's
//! file still to come, then downloaded one at a time and served at
//! `/media/.catalogue/`. A recording that cannot be had stays in the
//! library marked as failed, and is tried again later, and at the next
//! read.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::domain::catalogue::Catalogue;
use crate::sync::{self, Plan, Want};

pub use crate::sync::name_of;

/// Where under `/media` the downloads are served: the folder the domain
/// says a catalogue's albums are in.
pub const DIR: &str = harken_domain::catalogue::DIR;

pub fn dir(data: &Path) -> PathBuf {
    data.join("catalogue")
}

/// The catalogue at `url` read as one plan: the people's and the albums'
/// pictures downloaded into `dir` first, then every row it makes, every
/// recording's file the one it will be; and how many pictures could not
/// be had.
pub fn read(agent: &ureq::Agent, url: &str, dir: &Path) -> (Vec<Plan>, usize) {
    let catalogue = match sync::text(agent, url).and_then(|json| Catalogue::parse(&json)) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("{e}");
            return (vec![], 1);
        }
    };
    let tracks: BTreeSet<&str> = catalogue
        .albums
        .iter()
        .flat_map(|a| a.tracks.iter().map(|t| t.file.as_str()))
        .filter(|u| !u.is_empty())
        .collect();
    let mut failed = 0;
    let pictures: Vec<&str> = catalogue
        .urls()
        .into_iter()
        .filter(|u| !tracks.contains(u))
        .collect();
    let mut here = std::collections::BTreeMap::new();
    for u in pictures {
        if let Some(file) = sync::picture(agent, u, DIR, dir, &mut failed) {
            here.insert(u.to_string(), file);
        }
    }
    let files: Vec<Want> = tracks.iter().map(|u| Want::of(u, DIR, dir)).collect();
    let calls = catalogue.calls(|u| match tracks.contains(u) {
        true => Some(Want::of(u, DIR, dir).file),
        false => here.get(u).cloned(),
    });
    let plan = Plan {
        what: "the catalogue".into(),
        calls,
        gate: false,
        files,
    };
    (vec![plan], failed)
}
