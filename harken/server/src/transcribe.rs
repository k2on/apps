//! What is said in each episode, heard here: a program (`HARKEN_TRANSCRIBER`,
//! whisper.cpp as the NixOS module wraps it) run on the very file the server
//! serves, its captions written beside it as WebVTT (`<file>.vtt`).
//!
//! Heard from the server's own copy, a transcript keeps time with what
//! plays: a feed's captions are timed to the host's file, and a host that
//! stitches an advertisement in front of each download moves every line of
//! them. So a feed's transcript is not read at all.
//!
//! This only writes the captions. Captions beside an episode's file are
//! its transcript, whoever wrote them — this, another machine that heard
//! it faster (`hear-here`), a hand — and the scanner reads them as they
//! appear (`library.rs`), so nothing here says anything to the library.
//!
//! The scanner says which episodes are here without captions, newest first
//! ([`Transcriber::want`]), and a thread of its own hears them one at a
//! time — an hour of speech is a while of a host's every core, so the
//! program is the one to be gentle.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Where an episode's captions are: beside its file, `.vtt` after its name.
pub fn captions_of(audio: &Path) -> PathBuf {
    let mut name = audio.as_os_str().to_owned();
    name.push(".vtt");
    PathBuf::from(name)
}

/// An episode here to be heard: where its file is, what to call it in
/// what the server prints, and whether it is one voice throughout — a
/// sermon — so there are no voices to tell apart in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub path: PathBuf,
    pub title: String,
    pub one_voice: bool,
}

/// The transcriber's thread, as the scanner holds it.
pub struct Transcriber {
    wanted: Arc<Mutex<Vec<Job>>>,
    wake: Sender<()>,
}

impl Transcriber {
    /// `program` run as `program <audio> <captions.vtt>` on each episode
    /// wanted, its captions written beside it; `HARKEN_ONE_VOICE=1` in its
    /// environment for one that is one voice throughout.
    pub fn start(program: PathBuf) -> Transcriber {
        let wanted = Arc::new(Mutex::new(Vec::new()));
        let (wake, woken) = channel();
        let list = wanted.clone();
        let _ = std::thread::Builder::new()
            .name("harken-transcribe".into())
            .spawn(move || run(&program, &list, &woken));
        Transcriber { wanted, wake }
    }

    /// The episodes here without captions, in the order to hear them: the
    /// thread takes the first it has not tried, now or once the one it is
    /// on is done.
    pub fn want(&self, jobs: Vec<Job>) {
        let Ok(mut wanted) = self.wanted.lock() else {
            return;
        };
        if *wanted != jobs {
            *wanted = jobs;
            let _ = self.wake.send(());
        }
    }
}

fn run(program: &Path, wanted: &Mutex<Vec<Job>>, woken: &Receiver<()>) {
    // Each file once a run: one the program fails on is left for the next
    // start.
    let mut tried: HashSet<PathBuf> = HashSet::new();
    loop {
        let next = wanted.lock().ok().and_then(|w| {
            w.iter()
                .find(|j| !tried.contains(&j.path) && !captions_of(&j.path).exists())
                .cloned()
        });
        let Some(job) = next else {
            if woken.recv().is_err() {
                return;
            }
            continue;
        };
        tried.insert(job.path.clone());
        if let Err(e) = hear(program, &job) {
            eprintln!("harken-server: transcribe: {}: {e}", job.title);
        }
    }
}

/// An episode heard, its captions written beside it — unless some came
/// while it was heard, which are kept.
fn hear(program: &Path, job: &Job) -> Result<(), String> {
    let captions = captions_of(&job.path);
    let mut part = captions.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    let started = Instant::now();
    let status = Command::new(program)
        .arg(&job.path)
        .arg(&part)
        .env("HARKEN_ONE_VOICE", if job.one_voice { "1" } else { "0" })
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
    eprintln!(
        "harken-server: transcribe: {} heard in {} minutes",
        job.title,
        started.elapsed().as_secs() / 60
    );
    Ok(())
}
