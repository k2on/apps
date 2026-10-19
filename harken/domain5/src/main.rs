//! Writes harken's module: `harken-domain5 [PATH]`, `harken.ark5` by
//! default — the `Module`'s bytes, every function checked, which a runtime
//! verifies and hashes; the bodies are in the content store it binds.

use ark5::author::Node;

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "harken.ark5".to_string());
    let d = harken_domain5::domain();
    if let Err(e) = std::fs::write(&out, d.module.to_bytes()) {
        eprintln!("harken-domain5: writing {out}: {e}");
        std::process::exit(1);
    }
    println!("{out}  {} functions  {}", d.bodies.len(), d.module.hash());
}
