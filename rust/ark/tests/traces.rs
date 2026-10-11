//! `docs/plan-phones.md` S2 The trace runner, Rust's: every trace of a
//! corpus directory when `ARK_TRACE_CORPUS` names one
//! (`packages.trace-corpus`), and of a fuzz run of its own, replayed into a
//! fresh client from its inputs alone, every output held to the recorded
//! one, every frame byte for byte (`ark::trace::replay`). The client the
//! fuzzer recorded is this crate's, so this is the recording held to being
//! complete: an output that depended on something no input carries would
//! come out otherwise here. `spec/RUNNERS.md` § trace is what a runner in
//! another language reads.
//!
//! Falsified by dropping the frozen autos from what a mutation is recorded
//! with: the first trace that authors with a `now` or a `new_id` replays
//! its push with other bytes and fails, naming the trace, the step and the
//! field (`frames`).

use std::fs;
use std::path::{Path, PathBuf};

fn jsons(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    out.sort();
    out
}

// One trace file replayed; `Err` names what differs.
fn check(path: &Path) -> Result<usize, String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let (m, steps) = ark::trace::read(&text)?;
    ark::trace::replay(&m, &steps)?;
    Ok(steps.len())
}

// Every trace under `dir`, each replayed; the names it ran, and the first
// failure of each that failed.
fn run_dir(dir: &Path) -> (Vec<String>, Vec<String>) {
    let (mut ran, mut failed) = (vec![], vec![]);
    for p in jsons(dir) {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        match check(&p) {
            Ok(_) => ran.push(name),
            Err(e) => failed.push(format!("{name}: {e}")),
        }
    }
    (ran, failed)
}

/// `ARK_TRACE_CORPUS`: every trace of the generated corpus replays. Says
/// so and does nothing when the variable is unset.
#[test]
fn the_corpus_replays() {
    let Some(dir) = std::env::var_os("ARK_TRACE_CORPUS") else {
        println!("ARK_TRACE_CORPUS is unset: no corpus replayed");
        return;
    };
    let (ran, failed) = run_dir(Path::new(&dir));
    println!(
        "corpus {}: {} traces replayed, {} failed",
        Path::new(&dir).display(),
        ran.len(),
        failed.len()
    );
    assert!(failed.is_empty(), "traces that do not replay:\n{}", failed.join("\n"));
    assert!(!ran.is_empty(), "the corpus at {} holds no trace", Path::new(&dir).display());
}

/// `arkc fuzz --traces DIR` is a function of the seed and the case count:
/// two runs write the same files, byte for byte, and every one of them
/// replays. Falsified by naming the files after the wall clock's
/// nanoseconds — the two directories then share no name.
#[test]
fn the_fuzzer_writes_the_same_traces_twice() {
    let arkc = env!("CARGO_BIN_EXE_arkc");
    let base = std::env::temp_dir().join(format!("ark-traces-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let run = |name: &str| {
        let dir = base.join(name);
        let out = std::process::Command::new(arkc)
            .args(["fuzz", "--seed", "40", "--cases", "4", "--out"])
            .arg(base.join("findings"))
            .arg("--traces")
            .arg(&dir)
            .output()
            .expect("arkc runs");
        assert!(out.status.success(), "arkc fuzz: {}", String::from_utf8_lossy(&out.stdout));
        dir
    };
    let (a, b) = (run("a"), run("b"));
    let (fa, fb) = (jsons(&a), jsons(&b));
    assert!(fa.len() >= 8, "too few traces written: {}", fa.len());
    let names = |fs: &[PathBuf]| fs.iter().map(|p| p.file_name().unwrap().to_owned()).collect::<Vec<_>>();
    assert_eq!(names(&fa), names(&fb), "the two runs wrote other files");
    for (x, y) in fa.iter().zip(&fb) {
        assert!(fs::read(x).unwrap() == fs::read(y).unwrap(), "{} differs between two runs", x.display());
    }
    let (ran, failed) = run_dir(&a);
    assert!(failed.is_empty(), "traces that do not replay:\n{}", failed.join("\n"));
    println!("{} traces written twice, the same bytes, every one replayed", ran.len());
    let _ = fs::remove_dir_all(&base);
}
