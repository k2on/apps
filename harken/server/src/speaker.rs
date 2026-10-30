//! What a speaker is handed, where the file itself is more than it plays.
//!
//! A Sonos plays FLAC up to 48 kHz and 24 bits and no further: handed a
//! recording at 96 kHz — the classical catalogue's are — it takes the URL,
//! says nothing, and never starts, and the bridge reads it as stalled. The
//! file is kept as it is, since a phone and the browser play it whole; a
//! speaker is handed `/media/.speaker/<file>.flac` instead ([`Lower::file`]),
//! the same recording at [`MAX_RATE`], made by ffmpeg the first time it is
//! asked for and kept in the data directory.
//!
//! Made as soon as the track is handed, on a thread of its own, so by the
//! time the speaker fetches it the copy is usually there; when it is not,
//! the fetch waits for it. One at a time, as the transcriber is: a few
//! seconds of a host's core per recording.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::Router;
use tower_http::services::ServeDir;

/// Where under `/media` the speakers' copies are served.
pub const DIR: &str = ".speaker";

/// The most a speaker plays: what Sonos (S2) says of FLAC.
pub const MAX_RATE: u32 = 48_000;
pub const MAX_BITS: u8 = 24;

pub fn dir(data: &Path) -> PathBuf {
    data.join("speaker")
}

/// The speakers' copies: which files want one, and making them.
pub struct Lower {
    /// The media root, and the catalogue's and feeds' downloads, which are
    /// served under their own prefixes (`covers::routes`).
    media: PathBuf,
    catalogue: PathBuf,
    feeds: PathBuf,
    /// Where the copies are kept, each at its file's path with `.flac` after.
    out: PathBuf,
    ffmpeg: PathBuf,
    /// Whether each file asked about is more than a speaker plays, read once.
    known: Mutex<HashMap<String, bool>>,
    /// Held while ffmpeg runs: one copy at a time, and a fetch waiting on a
    /// copy being made waits here rather than making it again.
    making: Mutex<()>,
}

impl std::fmt::Debug for Lower {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Lower({} into {})",
            self.media.display(),
            self.out.display()
        )
    }
}

impl Lower {
    pub fn new(media: &Path, data: &Path, ffmpeg: PathBuf) -> Lower {
        Lower {
            media: media.to_path_buf(),
            catalogue: crate::catalogue::dir(data),
            feeds: crate::feeds::dir(data),
            out: dir(data),
            ffmpeg,
            known: Mutex::new(HashMap::new()),
            making: Mutex::new(()),
        }
    }

    /// What a speaker is handed for `file`: `file` itself, or its copy under
    /// [`DIR`] — whose making is started here.
    pub fn file(self: &Arc<Self>, file: &str) -> String {
        if !self.needs(file) {
            return file.to_string();
        }
        let (me, f) = (self.clone(), file.to_string());
        let _ = std::thread::Builder::new()
            .name("harken-speaker".into())
            .spawn(move || me.make(&f));
        format!("{DIR}/{file}.flac")
    }

    /// Whether `file` is more than a speaker plays. A file that cannot be
    /// read is not: it is handed as it is, and fails or not as it would.
    fn needs(&self, file: &str) -> bool {
        if let Some(&n) = self.known.lock().unwrap().get(file) {
            return n;
        }
        let n = self.source(file).is_some_and(|p| too_much(&p));
        self.known.lock().unwrap().insert(file.to_string(), n);
        n
    }

    /// Where `file` is on disk, by the prefix it is served under.
    fn source(&self, file: &str) -> Option<PathBuf> {
        if file.is_empty() || file.contains("://") || !safe(Path::new(file)) {
            return None;
        }
        let under = |prefix: &str| file.strip_prefix(prefix).and_then(|r| r.strip_prefix('/'));
        Some(if let Some(rest) = under(crate::catalogue::DIR) {
            self.catalogue.join(rest)
        } else if let Some(rest) = under(crate::feeds::DIR) {
            self.feeds.join(rest)
        } else {
            self.media.join(file)
        })
    }

    /// The copy of `file`, made if it is not there yet; `None` if it is not
    /// one to make, or ffmpeg failed (said on standard error).
    pub fn make(&self, file: &str) -> Option<PathBuf> {
        if !self.needs(file) {
            return None;
        }
        let source = self.source(file)?;
        let out = self.out.join(format!("{file}.flac"));
        let _one = self.making.lock().unwrap();
        if out.exists() {
            return Some(out);
        }
        std::fs::create_dir_all(out.parent()?).ok()?;
        let part = out.with_extension("flac.part");
        let status = Command::new(&self.ffmpeg)
            .args(["-nostdin", "-loglevel", "error", "-y", "-i"])
            .arg(&source)
            // The audio only, a cover as a picture block would be refused as
            // a video stream by the flac muxer's defaults.
            .args(["-map", "0:a:0", "-map_metadata", "0", "-c:a", "flac"])
            .args(["-ar", &MAX_RATE.to_string()])
            .args([
                "-sample_fmt",
                "s32",
                "-bits_per_raw_sample",
                &MAX_BITS.to_string(),
            ])
            .args(["-f", "flac"])
            .arg(&part)
            .stdin(Stdio::null())
            .status();
        match status {
            Ok(s) if s.success() => {
                std::fs::rename(&part, &out).ok()?;
                println!("harken-server: speaker: {file} made at {MAX_RATE} Hz");
                Some(out)
            }
            other => {
                let _ = std::fs::remove_file(&part);
                eprintln!(
                    "harken-server: speaker: {file}: {} {}: {other:?}",
                    self.ffmpeg.display(),
                    source.display()
                );
                None
            }
        }
    }
}

/// More than [`MAX_RATE`] or [`MAX_BITS`], by what the file's own header
/// says; tags are not read.
fn too_much(path: &Path) -> bool {
    use lofty::config::ParseOptions;
    use lofty::file::AudioFile;
    use lofty::probe::Probe;

    let Some(file) = Probe::open(path)
        .ok()
        .map(|p| p.options(ParseOptions::new().read_tags(false)))
        .and_then(|p| p.read().ok())
    else {
        return false;
    };
    let p = file.properties();
    p.sample_rate().is_some_and(|r| r > MAX_RATE) || p.bit_depth().is_some_and(|b| b > MAX_BITS)
}

/// No `..`, no root: a path that stays under whatever it is joined to.
fn safe(path: &Path) -> bool {
    path.components().all(|c| matches!(c, Component::Normal(_)))
}

/// `/media/.speaker`: the copies, each made on its first fetch if the
/// hand-off's own making has not finished, then served as `/media` is.
pub fn routes(lower: Arc<Lower>) -> Router {
    Router::new().nest_service(
        &format!("/media/{DIR}"),
        Router::new()
            .fallback_service(ServeDir::new(&lower.out))
            .layer(middleware::from_fn_with_state(lower, ensure)),
    )
}

async fn ensure(State(lower): State<Arc<Lower>>, req: Request, next: Next) -> Response {
    let path = crate::assistant::address(req.uri().path());
    let Some(file) = path
        .trim_start_matches('/')
        .strip_suffix(".flac")
        .map(str::to_string)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let made = tokio::task::spawn_blocking(move || lower.make(&file)).await;
    match made {
        Ok(Some(_)) => next.run(req).await,
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lower() -> Lower {
        Lower::new(Path::new("/m"), Path::new("/d"), "ffmpeg".into())
    }

    #[test]
    fn a_file_is_found_under_the_prefix_it_is_served_at() {
        let l = lower();
        assert_eq!(l.source("music/a.flac"), Some("/m/music/a.flac".into()));
        assert_eq!(
            l.source(".catalogue/x.flac"),
            Some("/d/catalogue/x.flac".into())
        );
        assert_eq!(l.source(".feeds/f/e.mp3"), Some("/d/feeds/f/e.mp3".into()));
    }

    #[test]
    fn nothing_outside_is_reached() {
        let l = lower();
        for bad in [
            "../etc/passwd",
            "/etc/passwd",
            "music/../../x",
            "",
            "http://h/a.flac",
        ] {
            assert_eq!(l.source(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_file_that_cannot_be_read_is_handed_as_it_is() {
        let l = Arc::new(lower());
        assert_eq!(l.file("music/missing.flac"), "music/missing.flac");
    }
}
