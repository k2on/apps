//! The media directory as a peer: walk `MEDIA/music`, and author
//! `add_track` for every audio file the library does not yet hold.
//!
//! The scanner is an ordinary `ark::protocol::Client` holding the whole
//! `library` scope, exchanged with the server machine directly — no socket,
//! the shape of Petros's `Hub::exchange`. Every entry it authors therefore
//! carries an actor and a session like anyone else's, is applied by the
//! same closure, and is refused by the same verdicts.
//!
//! This is the one place non-determinism enters: a fresh id and the clock,
//! drawn here for the autos and frozen in the entry. A file already in the
//! library (by its path in `file`) is not authored again, and `add_track`
//! refuses one that slipped through anyway.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use ark::eval::{Args, Ctx};
use ark::ir::{Auto, FnKind};
use ark::peer::Replica;
use ark::protocol::{Client, Mode};
use ark::store::MemoryStore;
use ark::value::Value;

use crate::hub::HubHandle;
use crate::Domain;

/// The mutator the scanner authors, and the user it authors as.
pub const MUTATOR: &str = "add_track";
pub const USER: &str = "library";
/// The directory under the media root that holds music.
pub const MUSIC: &str = "music";
/// The table `file` is a column of, for the not-yet-held check.
pub const TABLE: &str = "track";
/// What counts as audio, by extension.
pub const EXTENSIONS: [&str; 6] = ["mp3", "flac", "ogg", "m4a", "wav", "opus"];
/// Files authored per exchange with the hub.
const BATCH: usize = 64;

/// What one file is authored as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    /// Relative to the media root, with `/` separators: what `file` holds
    /// and what `/media/` serves back.
    pub file: String,
}

/// Every audio file under `root/music`, deepest last within a directory,
/// sorted by path so a scan authors in one order.
pub fn walk(root: &Path) -> Result<Vec<PathBuf>> {
    let music = root.join(MUSIC);
    let mut out = Vec::new();
    if !music.is_dir() {
        return Ok(out);
    }
    let mut todo = vec![music];
    while let Some(dir) = todo.pop() {
        for entry in
            std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))?
        {
            let path = entry?.path();
            if path.is_dir() {
                todo.push(path);
            } else if is_audio(&path) {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// What a file at a path under the media root is authored as: the title is
/// the file stem, the artist its directory, the album the directory above
/// that when there are two under `music`.
pub fn describe(root: &Path, path: &Path) -> Result<Found> {
    let rel = path
        .strip_prefix(root)
        .with_context(|| format!("{} is not under {}", path.display(), root.display()))?;
    let parts: Vec<String> = rel
        .components()
        .map(|c| match c {
            Component::Normal(s) => Ok(s.to_string_lossy().into_owned()),
            other => Err(anyhow!(
                "unexpected path component {other:?} in {}",
                rel.display()
            )),
        })
        .collect::<Result<_>>()?;
    let [first, rest @ ..] = parts.as_slice() else {
        bail!("{} has no name", path.display());
    };
    if first != MUSIC || rest.is_empty() {
        bail!("{} is not under {MUSIC}/", rel.display());
    }
    let title = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .with_context(|| format!("{} has no stem", path.display()))?;
    // The common layout: music/Artist/Album/track. One level is an artist
    // with no album; none is an unknown artist.
    let dirs = &rest[..rest.len() - 1];
    let artist = dirs
        .first()
        .cloned()
        .unwrap_or_else(|| "Unknown Artist".to_string());
    let album = if dirs.len() >= 2 {
        Some(dirs[dirs.len() - 1].clone())
    } else {
        None
    };
    Ok(Found {
        title,
        artist,
        album,
        file: parts.join("/"),
    })
}

/// A fresh random id, laid out as a version-4 UUID.
pub fn fresh_id() -> [u8; 16] {
    let mut b: [u8; 16] = rand::random();
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    b
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// How a scan went.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub found: usize,
    pub already: usize,
    pub authored: usize,
    pub refused: Vec<(String, String)>,
}

/// Scan `media` and author what is new, through the hub. Says so and does
/// nothing when the module has no `add_track`.
pub async fn scan(hub: &HubHandle, domain: &Domain, media: &Path) -> Result<Report> {
    let Some(fh) = domain.by_name.get(MUTATOR) else {
        eprintln!("harken-server: scanner: the module has no {MUTATOR}; nothing to do");
        return Ok(Report::default());
    };
    let f = &domain.closures[fh].function;
    if f.kind != FnKind::Mutator {
        bail!("{MUTATOR} is not a mutator");
    }
    let scope = f
        .scope
        .clone()
        .ok_or_else(|| anyhow!("{MUTATOR} names no scope"))?;
    let wanted: BTreeSet<&str> = ["title", "artist", "album", "duration_ms", "file"].into();
    let declared: BTreeSet<&str> = f.input.iter().map(|(n, _)| n.as_str()).collect();
    if declared != wanted {
        bail!("{MUTATOR} takes {declared:?}; the scanner knows how to fill {wanted:?}");
    }

    let files = walk(media)?;
    let mut report = Report {
        found: files.len(),
        ..Report::default()
    };
    let held: BTreeSet<String> = hub
        .rows(&scope, TABLE)
        .await?
        .iter()
        .filter_map(|r| match r.get("file") {
            Some(Value::Text(t)) => Some(t.clone()),
            _ => None,
        })
        .collect();
    let mut new = Vec::new();
    for p in &files {
        let found = describe(media, p)?;
        if held.contains(&found.file) {
            report.already += 1;
        } else {
            new.push(found);
        }
    }
    if new.is_empty() {
        eprintln!(
            "harken-server: scanner: {} files under {}, all in the library",
            files.len(),
            media.join(MUSIC).display()
        );
        return Ok(report);
    }

    let schema = domain.module.schema.clone();
    let mut client = Client::open(schema.clone(), Some(USER.into()));
    client.subscribe(
        Mode::Whole,
        Replica::open(
            schema.clone(),
            &scope,
            domain.closures.clone(),
            MemoryStore::empty(schema),
            0,
            vec![],
        ),
    );
    let conn = hub.connect_local().await?;
    client.connected();
    pump(hub, conn, &mut client).await?;
    let who = hub
        .identity(conn)
        .await?
        .ok_or_else(|| anyhow!("the hub did not identify the scanner"))?;
    let ctx = Ctx::new(who.user, who.session);

    for batch in new.chunks(BATCH) {
        for found in batch {
            let autos: Args = f
                .autos
                .iter()
                .map(|(name, kind)| {
                    let v = match kind {
                        Auto::NewId(_) => Value::Id(fresh_id()),
                        Auto::Now => Value::int(now_ms()),
                    };
                    (name.clone(), v)
                })
                .collect();
            let args: Args = [
                ("title".to_string(), Value::text(found.title.clone())),
                ("artist".to_string(), Value::text(found.artist.clone())),
                (
                    "album".to_string(),
                    Value::opt(found.album.clone().map(Value::text)),
                ),
                ("duration_ms".to_string(), Value::int(0)),
                ("file".to_string(), Value::text(found.file.clone())),
            ]
            .into();
            match client.mutate(&scope, fresh_id(), &ctx, fh, &autos, &args) {
                Ok(_) => report.authored += 1,
                Err(why) => report.refused.push((found.file.clone(), why.to_string())),
            }
        }
        pump(hub, conn, &mut client).await?;
    }
    let replica = &client.scopes[&scope].0;
    for (id, why) in &replica.rejections {
        report.authored -= 1;
        report.refused.push((ark::value::hex(id), why.to_string()));
    }
    if !replica.pending.is_empty() {
        bail!(
            "{} entries the scanner authored were never answered",
            replica.pending.len()
        );
    }
    hub.disconnect(conn)?;
    eprintln!(
        "harken-server: scanner: {} files under {}, {} already in the library, {} authored, {} refused",
        report.found,
        media.join(MUSIC).display(),
        report.already,
        report.authored,
        report.refused.len()
    );
    for (what, why) in &report.refused {
        eprintln!("harken-server: scanner: {what}: {why}");
    }
    Ok(report)
}

/// Exchange with the hub until neither side has anything more to say.
async fn pump(hub: &HubHandle, conn: i64, client: &mut Client) -> Result<()> {
    loop {
        let out = client.take_outgoing();
        let back = hub.exchange(conn, out).await?;
        if back.is_empty() {
            return Ok(());
        }
        for m in back {
            client.recv(m);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(title: &str, artist: &str, album: Option<&str>, file: &str) -> Found {
        Found {
            title: title.into(),
            artist: artist.into(),
            album: album.map(Into::into),
            file: file.into(),
        }
    }

    #[test]
    fn a_path_under_music_is_a_title_an_artist_and_perhaps_an_album() {
        let root = Path::new("/srv/media");
        assert_eq!(
            describe(root, &root.join("music/Bach/Goldberg/01 Aria.flac")).unwrap(),
            found(
                "01 Aria",
                "Bach",
                Some("Goldberg"),
                "music/Bach/Goldberg/01 Aria.flac"
            )
        );
        assert_eq!(
            describe(root, &root.join("music/Bach/air.mp3")).unwrap(),
            found("air", "Bach", None, "music/Bach/air.mp3")
        );
        assert_eq!(
            describe(root, &root.join("music/loose.ogg")).unwrap(),
            found("loose", "Unknown Artist", None, "music/loose.ogg")
        );
        assert!(describe(root, &root.join("podcasts/ep1.mp3")).is_err());
        assert!(describe(Path::new("/elsewhere"), &root.join("music/x.mp3")).is_err());
    }

    #[test]
    fn the_walk_finds_audio_and_only_audio_in_path_order() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for p in [
            "music/b/2.MP3",
            "music/a/1.flac",
            "music/a/cover.jpg",
            "music/zed.opus",
            "podcasts/no.mp3",
        ] {
            let full = root.join(p);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(&full, b"").unwrap();
        }
        let rel: Vec<String> = walk(root)
            .unwrap()
            .iter()
            .map(|p| describe(root, p).unwrap().file)
            .collect();
        assert_eq!(
            rel,
            vec!["music/a/1.flac", "music/b/2.MP3", "music/zed.opus"]
        );
        assert!(walk(&root.join("nowhere")).unwrap().is_empty());
    }

    #[test]
    fn a_fresh_id_is_laid_out_as_a_v4_uuid_and_two_differ() {
        let a = fresh_id();
        let b = fresh_id();
        assert_eq!(a[6] >> 4, 4);
        assert_eq!(a[8] >> 6, 0b10);
        assert_ne!(a, b);
    }
}
