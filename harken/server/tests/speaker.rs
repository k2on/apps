//! What a speaker is handed of a file more than it plays, through the whole
//! server: a 96 kHz FLAC is fetched from `/media/.speaker/` at 48 kHz, made
//! by ffmpeg on the fetch, while the file itself is still served whole at
//! `/media/`; and a file a speaker plays as it is has no copy at all.
//!
//! The files are made here by ffmpeg, the one the server runs
//! (`HARKEN_FFMPEG`, or `ffmpeg` on the path).

mod common;

use std::io::Read;
use std::sync::Arc;

use common::*;
use harken_server::speaker::{Lower, MAX_RATE};
use lofty::file::AudioFile;

fn ffmpeg() -> String {
    std::env::var("HARKEN_FFMPEG").unwrap_or_else(|_| "ffmpeg".into())
}

/// A second of a tone at `rate`, 24 bits, as FLAC at `path`.
fn tone(path: &std::path::Path, rate: u32) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let ok = std::process::Command::new(ffmpeg())
        .args(["-nostdin", "-loglevel", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("sine=frequency=440:duration=1:sample_rate={rate}"))
        .args([
            "-ac",
            "2",
            "-sample_fmt",
            "s32",
            "-bits_per_raw_sample",
            "24",
        ])
        .arg(path)
        .status()
        .expect("ffmpeg runs");
    assert!(ok.success(), "ffmpeg made {}", path.display());
}

/// The status and body of a GET of `path` on `server`.
fn get(base: &str, path: &str) -> (u16, Vec<u8>) {
    match ureq::get(&format!("{base}{path}")).call() {
        Ok(r) => {
            let status = r.status();
            let mut body = vec![];
            r.into_reader().read_to_end(&mut body).unwrap();
            (status, body)
        }
        Err(ureq::Error::Status(code, _)) => (code, vec![]),
        Err(e) => panic!("GET {path}: {e}"),
    }
}

fn rate_of(bytes: &[u8]) -> u32 {
    let file = lofty::probe::Probe::new(std::io::Cursor::new(bytes))
        .guess_file_type()
        .unwrap()
        .read()
        .unwrap();
    file.properties().sample_rate().unwrap()
}

#[test]
fn a_speaker_is_handed_a_copy_of_what_is_more_than_it_plays() {
    let media = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    tone(&media.path().join("music/hi.flac"), 96_000);
    tone(&media.path().join("music/cd.flac"), 44_100);

    let lower = Arc::new(Lower::new(media.path(), data.path(), ffmpeg().into()));
    assert_eq!(lower.file("music/hi.flac"), ".speaker/music/hi.flac.flac");
    assert_eq!(lower.file("music/cd.flac"), "music/cd.flac");

    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = serve(&rt, data.path(), |c| {
        c.media = Some(media.path().to_path_buf());
        c.ffmpeg = ffmpeg().into();
    });
    let base = server.url();

    let (status, copy) = get(&base, "/media/.speaker/music/hi.flac.flac");
    assert_eq!(status, 200);
    assert_eq!(
        rate_of(&copy),
        MAX_RATE,
        "the speaker's copy is at what it plays"
    );

    let (status, whole) = get(&base, "/media/music/hi.flac");
    assert_eq!(status, 200);
    assert_eq!(rate_of(&whole), 96_000, "the file itself is kept as it is");

    assert_eq!(get(&base, "/media/.speaker/music/cd.flac.flac").0, 404);
    assert_eq!(get(&base, "/media/.speaker/music/missing.flac.flac").0, 404);
    assert_eq!(
        get(&base, "/media/.speaker/..%2f..%2fetc/passwd.flac").0,
        404
    );
}
