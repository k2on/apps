//! The conformance vectors, written from the reference: every case runs
//! the specification end to end on the demo domain — build the module,
//! verify it, apply, rebase, sequence, maintain — and asserts what it
//! claims as it goes, so a change that breaks a claim fails the generator
//! rather than emitting a wrong vector (`spec/README.md`, "The vectors").
//!
//! One module per directory, each a function from the output directory to
//! its files; [`json`] is the dialect they are written in and [`demo`] the
//! domain they are over. The `ark-vectors` binary and `arkc vectors` are
//! both [`write_all`].

mod codec;
mod demo;
mod eval;
mod fuzzed;
mod json;
mod module;
mod protocol;
mod rebase;
mod views;

use std::fs;
use std::path::{Path, PathBuf};

/// Where the vectors go, and the one way a file is written there.
pub struct Out {
    root: PathBuf,
}

impl Out {
    /// A heading on stdout for the directory about to be written.
    fn dir(&self, name: &str) {
        println!("{name}");
    }

    /// Write one vector, as UTF-8 whatever the locale says: a vector must
    /// be the same bytes on every machine that emits it.
    fn write(&self, rel: &str, text: &str) {
        let path = self.root.join(rel);
        fs::write(&path, text).unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
        println!("  {}", path.display());
    }
}

/// The directories a run writes into, created before anything is written.
/// Each has a `falsify/` beside it: a vector with a wrong expectation that
/// a conformant runner must fail.
const DIRS: [&str; 9] = ["codec", "order", "hash", "eval", "verify", "rebase", "protocol", "module", "views"];

/// Write every vector under `root`. Panics, naming the claim, when the
/// reference disagrees with what a vector would say.
pub fn write_all(root: &Path) {
    for d in DIRS {
        let p = root.join(d).join("falsify");
        fs::create_dir_all(&p).unwrap_or_else(|e| panic!("creating {}: {e}", p.display()));
    }
    let out = Out { root: root.to_path_buf() };
    codec::codec(&out);
    codec::order(&out);
    eval::demo(&out);
    eval::has_role(&out);
    eval::raw(&out);
    rebase::three_peers(&out);
    module::module(&out);
    protocol::protocol(&out);
    protocol::scoped(&out);
    protocol::private(&out);
    protocol::raw(&out);
    rebase::fleet(&out);
    fuzzed::fuzzed(&out);
    views::views(&out);
    println!("vectors written");
}

/// Stop the generator on a claim that does not hold.
fn claim(what: &str, ok: bool) {
    assert!(ok, "{what}");
}
