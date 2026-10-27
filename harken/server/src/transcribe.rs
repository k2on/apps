//! What is said in each episode, heard here: a program (`HARKEN_TRANSCRIBER`,
//! whisper.cpp as the NixOS module wraps it) run on the very file the server
//! serves, writing its captions as WebVTT, read into the episode's
//! transcript (`library.set_transcript`).
//!
//! Heard from the server's own copy, a transcript keeps time with what
//! plays: a feed's captions are timed to the host's file, and a host that
//! stitches an advertisement in front of each download moves every line of
//! them. So a feed's transcript is not read at all.
//!
//! The scanner says which episodes are here, newest first ([`Transcriber::want`]),
//! and a thread of its own hears them one at a time — an hour of speech is
//! minutes of a host's every core, so the program is the one to be gentle.
//! What it wrote is kept in the data directory by the file's hash, so an
//! episode is heard once whatever restarts, and a restart says it again
//! (which writes nothing when the log already has it).
//!
//! An episode its feed gave no chapters is given some from what was heard
//! in it, by whichever chapterers the server names (`chapters.rs`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use ark::format::Encode;

use crate::captions;
use crate::chapters::{self, About, Chapterer};
use crate::domain::{CueAt, SetChapters, SetTranscript};
use crate::sync::{name_of, Call, Plan, Said};

pub fn dir(data: &Path) -> PathBuf {
    data.join("transcripts")
}

/// An episode here to be heard: its file as its row says it, where that
/// is on disk, what it is, and whether it has chapters already.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub file: String,
    pub path: PathBuf,
    pub title: String,
    pub description: String,
    pub chaptered: bool,
}

/// How the server hears: the program, and who makes chapters of what it
/// heard, in the order to try them.
pub struct Hearing {
    pub program: PathBuf,
    pub chapters: Vec<Box<dyn Chapterer>>,
}

/// The transcriber's thread, as the scanner holds it.
pub struct Transcriber {
    wanted: Arc<Mutex<Vec<Job>>>,
    wake: Sender<()>,
}

impl Transcriber {
    /// `hearing.program` run as `program <audio> <captions.vtt>` on each
    /// episode wanted, its captions kept in `dir`, each transcript (and the
    /// chapters of an episode with none) said to `said`.
    pub fn start(hearing: Hearing, dir: PathBuf, said: Sender<Said>) -> Transcriber {
        let wanted = Arc::new(Mutex::new(Vec::new()));
        let (wake, woken) = channel();
        let list = wanted.clone();
        let _ = std::thread::Builder::new()
            .name("harken-transcribe".into())
            .spawn(move || run(&hearing, &dir, &list, &woken, &said));
        Transcriber { wanted, wake }
    }

    /// The episodes here, in the order to hear them: the thread takes the
    /// first it has not yet, now or once the one it is on is done.
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

fn run(
    hearing: &Hearing,
    dir: &Path,
    wanted: &Mutex<Vec<Job>>,
    woken: &Receiver<()>,
    said: &Sender<Said>,
) {
    // Each file once a run: heard or kept and said, or failed and left for
    // the next start.
    let mut tried: HashSet<String> = HashSet::new();
    loop {
        let next = wanted
            .lock()
            .ok()
            .and_then(|w| w.iter().find(|j| !tried.contains(&j.file)).cloned());
        let Some(job) = next else {
            if woken.recv().is_err() {
                return;
            }
            continue;
        };
        tried.insert(job.file.clone());
        let cues = match heard(&hearing.program, dir, &job) {
            Ok(cues) => cues,
            Err(e) => {
                eprintln!("harken-server: transcribe: {}: {e}", job.title);
                continue;
            }
        };
        if cues.is_empty() {
            continue;
        }
        let call = |name: &'static str, input: Vec<u8>| Call {
            name,
            what: job.title.clone(),
            input,
            if_there: false,
        };
        let mut calls = vec![];
        if !job.chaptered {
            let about = About {
                title: &job.title,
                description: &job.description,
            };
            let made = chapters::first(&hearing.chapters, &about, &cues);
            if !made.is_empty() {
                let set = SetChapters {
                    file: job.file.clone(),
                    chapters: made,
                };
                calls.push(call("library.set_chapters", set.to_bytes()));
            }
        }
        let set = SetTranscript {
            file: job.file.clone(),
            cues,
        };
        calls.insert(0, call("library.set_transcript", set.to_bytes()));
        let plan = Plan {
            what: job.title.clone(),
            calls,
            gate: false,
            files: vec![],
        };
        let (yes, answer) = channel();
        if said.send(Said::Plan(Box::new(plan), yes)).is_err() || answer.recv().is_err() {
            return;
        }
    }
}

/// An episode's lines: as kept, or heard now and kept.
fn heard(program: &Path, dir: &Path, job: &Job) -> Result<Vec<CueAt>, String> {
    let kept = dir.join(format!("{}.vtt", name_of(&job.file)));
    if !kept.exists() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let part = kept.with_extension("vtt.part");
        let started = std::time::Instant::now();
        let status = Command::new(program)
            .arg(&job.path)
            .arg(&part)
            .stdin(Stdio::null())
            .status()
            .map_err(|e| format!("{}: {e}", program.display()))?;
        if !status.success() {
            let _ = std::fs::remove_file(&part);
            return Err(format!("{} said {status}", program.display()));
        }
        std::fs::rename(&part, &kept).map_err(|e| format!("{}: {e}", kept.display()))?;
        eprintln!(
            "harken-server: transcribe: {} heard in {} minutes",
            job.title,
            started.elapsed().as_secs() / 60
        );
    }
    let text = std::fs::read_to_string(&kept).map_err(|e| format!("{}: {e}", kept.display()))?;
    Ok(captions::captions(&text))
}
