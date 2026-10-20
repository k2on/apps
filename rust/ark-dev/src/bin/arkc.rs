//! `arkc`: module files (`(Module, [body])` under §1.1's codec, what
//! `harken-domain` writes and a server's `HARKEN_MODULE` reads), checked.
//!
//! ```text
//! arkc verify FILE...     every body checked, the module verified (§1.5); its hash
//! arkc compat OLD NEW     whether NEW may follow OLD: the schema a prefix-extension (§1.2),
//!                         every function NEW binds and OLD did not checked over NEW's schema
//! arkc build FILE... -o OUT  Ark text (§1.15), the files one module, compiled to a module file
//! arkc print FILE         a module file as Ark text
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
        [cmd, rest @ ..] if cmd == "build" && rest.len() >= 3 && rest[rest.len() - 2] == "-o" => {
            let (files, out) = (&rest[..rest.len() - 2], &rest[rest.len() - 1]);
            let mut src = String::new();
            for f in files {
                src += &std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?;
                src += "\n";
            }
            let d = ark::text::build(&src).map_err(|e| e.0)?;
            let file = (d.module.clone(), d.bodies.values().cloned().collect::<Vec<_>>());
            std::fs::write(out, file.to_bytes()).map_err(|e| format!("{out}: {e}"))?;
            println!("{out}  {} functions  {}", d.bodies.len(), d.module.hash());
            Ok(())
        }
        [cmd, file] if cmd == "print" => {
            let bytes = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
            let (m, bodies) = <(Module, Vec<Vec<u8>>)>::from_bytes(&bytes).map_err(|e| format!("{file}: {}", e.0))?;
            let fns = bodies
                .iter()
                .map(|b| Function::from_bytes(b))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.0)?;
            print!("{}", ark::text::print(&m, &fns).map_err(|e| format!("{file}: {}", e.0))?);
            Ok(())
        }
        _ => Err("usage: arkc verify FILE... | arkc compat OLD NEW | arkc build FILE... -o OUT | arkc print FILE".into()),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&args) {
        eprintln!("arkc: {e}");
        std::process::exit(1);
    }
}
