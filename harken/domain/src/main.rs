//! Writes harken's module: `harken-domain [PATH]`, `harken.ark` by default.
//! The bytes are `module().emit()` — the verified module in canonical CBOR,
//! which `arkc verify` checks and every runtime hashes.

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "harken.ark".to_string());
    let module = harken_domain::module();
    let bytes = module.emit();
    if let Err(e) = std::fs::write(&out, &bytes) {
        eprintln!("harken-domain: writing {out}: {e}");
        std::process::exit(1);
    }
    println!("{out}  {} functions  {}", module.build().functions.len(), ark::value::hex(&module.hash()));
}
