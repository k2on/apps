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
mod demo_live;
mod eval;
mod fuzzed;
mod json;
mod live;
mod module;
mod protocol;
mod rebase;
mod trace;
mod views;

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};

/// Where the vectors go, and the one way a file is written there; and every
/// file written, for the manifest.
pub struct Out {
    root: PathBuf,
    written: RefCell<Vec<String>>,
    /// The live vectors (`docs/plan-live.md`), which the manifest marks
    /// `rust-for-now`: Kotlin and Swift skip them until they hold live state.
    live: RefCell<std::collections::BTreeSet<String>>,
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
        self.written.borrow_mut().push(rel.to_string());
    }

    /// Write one live vector: [`Out::write`], marked for the manifest.
    fn write_live(&self, rel: &str, text: &str) {
        self.write(rel, text);
        self.live.borrow_mut().insert(rel.to_string());
    }
}

/// The directories a run writes into, created before anything is written.
/// Each has a `falsify/` beside it: a vector with a wrong expectation that
/// a conformant runner must fail.
const DIRS: [&str; 10] = [
    "codec", "order", "hash", "eval", "verify", "rebase", "protocol", "module", "views", "trace",
];

/// Write every vector under `root`. Panics, naming the claim, when the
/// reference disagrees with what a vector would say.
pub fn write_all(root: &Path) {
    for d in DIRS {
        let p = root.join(d).join("falsify");
        fs::create_dir_all(&p).unwrap_or_else(|e| panic!("creating {}: {e}", p.display()));
    }
    let out = Out {
        root: root.to_path_buf(),
        written: RefCell::new(vec![]),
        live: RefCell::new(Default::default()),
    };
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
    trace::traces(&out);
    live::live(&out);
    manifest(&out);
    println!("vectors written");
}

/// `MANIFEST.json`: every vector this run wrote, by path, with its kind —
/// the directory it is in — its form within the kind, whether a runner
/// must pass it or fail it, and who must run it (`spec/RUNNERS.md`,
/// `docs/plan-phones.md` S1): `client`, every runtime that is a client; or
/// `rust-only`, a fleet run through Rust's simulator and its seeded
/// scheduler, which reaches the other runtimes through the traces of its
/// clients instead (`trace/`); or `rust-for-now`, a vector of live state
/// (`docs/plan-live.md`), which Rust runs and the client runtimes skip until
/// they hold live state. Written last, from the files themselves, so that a
/// vector added anywhere is listed without anyone listing it.
fn manifest(out: &Out) {
    let mut paths = out.written.borrow().clone();
    paths.sort();
    let lines: Vec<String> = paths
        .iter()
        .map(|rel| {
            let text = fs::read_to_string(out.root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
            let v = ark::json::decode(&text).unwrap_or_else(|e| panic!("{rel}: {e:?}"));
            let has = |k: &str| v.as_struct().contains_key(k);
            let kind = rel.split('/').next().unwrap_or("");
            let file = rel.rsplit('/').next().unwrap_or("");
            let form = match kind {
                "eval" if has("cases") => "cases",
                "eval" if has("raw") => "raw",
                "eval" if has("live") => "live",
                "eval" => "apply",
                "hash" if has("tables") => "parts",
                "module" if has("stripped") => "stripped",
                "protocol" if file.starts_with("client-") => "client-frame",
                "protocol" => "server-frame",
                "rebase" if has("fuzz") => "fleet-fuzz",
                "rebase" if has("script") => "fleet-seed",
                "rebase" => "three-peers",
                "views" if has("fuzz") => "fuzz",
                "trace" if file.starts_with("harken-") => "harken",
                "trace" if rel.starts_with("trace/live/") => "live",
                "trace" => "fuzz",
                k => k,
            };
            let side = if kind == "rebase" && has("script") {
                "rust-only"
            } else if out.live.borrow().contains(rel) {
                "rust-for-now"
            } else {
                "client"
            };
            let expect = if rel.contains("/falsify/") { "fail" } else { "pass" };
            json::json(&ark::value::Value::record(vec![
                ("path", ark::value::Value::text(rel.clone())),
                ("kind", ark::value::Value::text(kind)),
                ("form", ark::value::Value::text(form)),
                ("side", ark::value::Value::text(side)),
                ("expect", ark::value::Value::text(expect)),
            ]))
        })
        .collect();
    let text = json::obj(&[
        (
            "about",
            json::quoted(
                "every vector, by path: its kind (the directory), its form, whether a runner must pass or fail it, and who runs it — spec/RUNNERS.md",
            ),
        ),
        ("vectors", format!("[\n{}\n]", lines.join(",\n"))),
    ]);
    let path = out.root.join("MANIFEST.json");
    fs::write(&path, text).unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
    println!("  {}", path.display());
}

/// Stop the generator on a claim that does not hold.
fn claim(what: &str, ok: bool) {
    assert!(ok, "{what}");
}
