//! `ark-vectors OUTDIR`: write the conformance vectors (`spec/README.md`,
//! "The vectors") into OUTDIR, `vectors` when none is given.

// The fuzzer's sessions, which `trace/` is chosen from (`docs/plan-phones.md`
// S2); `arkc` has them as its own `fuzz`, and so does this binary.
#[allow(dead_code)]
#[path = "fuzz/mod.rs"]
mod fuzz;
#[path = "vectors/mod.rs"]
mod vectors;

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "vectors".into());
    vectors::write_all(std::path::Path::new(&out));
}
