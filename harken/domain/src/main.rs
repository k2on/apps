//! Writes harken's module file: `harken-domain [PATH]`, `harken.ark` by
//! default — `(Module, [body])` under §1.1's codec, the binding map and
//! every function's bytes, each checked: the file the server's
//! `HARKEN_MODULE` and `HARKEN_OLD_MODULES` read (`harken_server::hosted`)
//! and `arkc verify` holds together.

use ark::author::Node;

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "harken.ark".to_string());
    let d = harken_domain::domain();
    let file = (d.module.clone(), d.bodies.values().cloned().collect::<Vec<_>>());
    if let Err(e) = std::fs::write(&out, file.to_bytes()) {
        eprintln!("harken-domain: writing {out}: {e}");
        std::process::exit(1);
    }
    println!("{out}  {} functions  {}", d.bodies.len(), d.module.hash());
}
