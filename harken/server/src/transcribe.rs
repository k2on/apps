//! What is said in each episode, heard here: a program (`HARKEN_TRANSCRIBER`,
//! whisper.cpp as the NixOS module wraps it) run on the very file the server
//! serves, its captions written beside it as WebVTT (`<file>.vtt`).
//!
//! Heard from the server's own copy, a transcript keeps time with what
//! plays: a feed's captions are timed to the host's file, and a host that
//! stitches an advertisement in front of each download moves every line of
//! them. So a feed's transcript is not read at all.
//!
//! Captions beside an episode's file are its transcript, whoever wrote
//! them — this, another machine that heard it faster (`hear-here`), a
//! hand — and the scanner reads them as they appear (`library.rs`).
//!
//! Each episode here without captions is a job (a `Hearing` row), newest
//! first, heard one at a time — an hour of speech is a while of a host's
//! every core, so the program is the one to be gentle. One the program
//! fails on is tried again later.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

/// Where an episode's captions are: beside its file, `.vtt` after its name.
pub fn captions_of(audio: &Path) -> PathBuf {
    let mut name = audio.as_os_str().to_owned();
    name.push(".vtt");
    PathBuf::from(name)
}

/// The episode at `audio`, called `title`, heard by `program`, run as
/// `program <audio> <captions.vtt>`, with `HARKEN_ONE_VOICE=1` in its
/// environment when it is one voice throughout (a sermon): its captions
/// written beside it — unless some came while it was heard, which are
/// kept.
pub fn hear(program: &Path, audio: &Path, title: &str, one_voice: bool) -> Result<(), String> {
    let captions = captions_of(audio);
    let mut part = captions.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    let started = Instant::now();
    let status = Command::new(program)
        .arg(audio)
        .arg(&part)
        .env("HARKEN_ONE_VOICE", if one_voice { "1" } else { "0" })
        .stdin(Stdio::null())
        .status()
        .map_err(|e| format!("{}: {e}", program.display()))?;
    if !status.success() || captions.exists() {
        let _ = std::fs::remove_file(&part);
        return match status.success() {
            true => Ok(()),
            false => Err(format!("{} said {status}", program.display())),
        };
    }
    std::fs::rename(&part, &captions).map_err(|e| format!("{}: {e}", captions.display()))?;
    let minutes = started.elapsed().as_secs() / 60;
    tracing::info!(minutes, "{title} heard");
    Ok(())
}
