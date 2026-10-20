//! `arkc`: module files (`(Module, [body])` under §1.1's codec, what
//! `harken-domain` writes and a server's `HARKEN_MODULE` reads), checked.
//!
//! ```text
//! arkc verify FILE...     every body checked, the module verified (§1.5); its hash
//! arkc compat OLD NEW     whether NEW may follow OLD: the schema a prefix-extension (§1.2),
//!                         every function NEW binds and OLD did not checked over NEW's schema
//! ```

use ark::author::Node;
use ark::ir::Function;
use ark::module::{verify, Lib, Module};
use ark::Refusal;

/// A module file read: every body installed once what it calls is, and
/// the module verified.
fn read(path: &str) -> Result<(Module, Lib), String> {
    let at = |e: Refusal| format!("{path}: {}", e.0);
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let (m, bodies) = <(Module, Vec<Vec<u8>>)>::from_bytes(&bytes).map_err(at)?;
    let lib = Lib::default();
    let mut todo: Vec<Function> = bodies.iter().map(|b| Function::from_bytes(b)).collect::<Result<_, _>>().map_err(at)?;
    while !todo.is_empty() {
        let before = todo.len();
        let mut last = None;
        for f in std::mem::take(&mut todo) {
            if let Err(e) = lib.install(&m.schema, f.clone()) {
                last = Some(e);
                todo.push(f);
            }
        }
        if todo.len() == before {
            return Err(at(last.expect("a refusal")));
        }
    }
    verify(&m, &lib).map_err(at)?;
    Ok((m, lib))
}

fn run(args: &[String]) -> Result<(), String> {
    match args {
        [cmd, files @ ..] if cmd == "verify" && !files.is_empty() => {
            for f in files {
                let (m, _) = read(f)?;
                println!("{f}  {} bindings  {}", m.bindings.len(), m.hash());
            }
            Ok(())
        }
        [cmd, old, new] if cmd == "compat" => {
            let (o, _) = read(old)?;
            let (n, _) = read(new)?;
            ark::schema::compat(&o.schema, &n.schema).map_err(|e| format!("{new} may not follow {old}: {}", e.0))?;
            let fresh = n.bindings.iter().filter(|b| !o.bindings.iter().any(|x| x.1 == b.1)).count();
            println!("{new} may follow {old}: {fresh} functions new");
            Ok(())
        }
        _ => Err("usage: arkc verify FILE... | arkc compat OLD NEW".into()),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&args) {
        eprintln!("arkc: {e}");
        std::process::exit(1);
    }
}
