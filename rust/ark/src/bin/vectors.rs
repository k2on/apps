//! `ark-vectors OUTDIR`: write the conformance vectors (`spec/README.md`,
//! "The vectors") into OUTDIR, `vectors` when none is given.

#[path = "vectors/mod.rs"]
mod vectors;

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "vectors".into());
    vectors::write_all(std::path::Path::new(&out));
}
