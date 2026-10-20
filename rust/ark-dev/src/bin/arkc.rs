//! `arkc`: modules checked, printed and compared (§1.5, §1.15). A module is
//! a domain's directory of ArkLang, one file of it, or a module file
//! (`(Module, [body])` under §1.1's codec), each read by `ark::sexp::load`.
//!
//! ```text
//! arkc check MODULE...            built, every function checked, the module verified; its hash
//! arkc print MODULE [-o DIR]      as ArkLang: to stdout, or under DIR one file per dotted
//!                                 prefix, beside schema.ark and types.ark
//! arkc compat OLD NEW             whether NEW may follow OLD (§1.2's prefix rule, every new
//!                                 function checked over NEW's schema)
//! arkc build MODULE -o FILE       the module file
//! ```

use std::collections::BTreeMap;
use std::path::Path;

use ark::sexp::{form, forms, layout, print, Sx, K};
use ark::Domain;

fn load(p: &str) -> Result<Domain, String> {
    ark::sexp::load(Path::new(p)).map_err(|r| r.0)
}

/// What a command says, or why it could not.
fn run(args: &[&str]) -> Result<String, String> {
    let said = |m: &str, d: &Domain| format!("{m}  {} functions  {}\n", d.bodies.len(), d.module.hash());
    match args {
        ["check", ms @ ..] if !ms.is_empty() => ms.iter().map(|m| Ok(said(m, &load(m)?))).collect(),
        ["print", m] => {
            let d = load(m)?;
            print(&d.module, &d.functions()).map_err(|r| r.0)
        }
        ["print", m, "-o", dir] => {
            let d = load(m)?;
            let (mut files, mut schema) = (
                BTreeMap::<String, Vec<String>>::new(),
                vec![Sx {
                    at: (0, 0),
                    k: K::Sym("schema".into()),
                }],
            );
            for (name, x) in forms(&d.module, &d.functions()).map_err(|r| r.0)? {
                let file = match form(&x).map(|f| f.0) {
                    Some("table") => {
                        schema.push(x);
                        continue;
                    }
                    Some("type") => "types",
                    _ => name.rsplit_once('.').map_or("functions", |p| p.0),
                };
                files.entry(file.into()).or_default().push(layout(&x, 0));
            }
            files.insert(
                "schema".into(),
                vec![layout(
                    &Sx {
                        at: (0, 0),
                        k: K::List(schema),
                    },
                    0,
                )],
            );
            std::fs::create_dir_all(dir).map_err(|e| format!("{dir}: {e}"))?;
            for (f, xs) in &files {
                let path = Path::new(dir).join(format!("{f}.ark"));
                std::fs::write(&path, xs.join("\n\n") + "\n").map_err(|e| format!("{}: {e}", path.display()))?;
            }
            Ok(format!("{dir}: {} files\n", files.len()))
        }
        ["compat", old, new] => match load(new).and_then(|n| Ok(ark::module::compat(&load(old)?.module, &n.module, &n.lib)))? {
            Ok(fresh) => Ok(format!("{new} may follow {old}: {fresh} functions new\n")),
            Err(r) => Err(format!("{new} may not follow {old}: {}", r.0)),
        },
        ["build", m, "-o", out] => {
            let d = load(m)?;
            std::fs::write(out, d.to_file()).map_err(|e| format!("{out}: {e}"))?;
            Ok(said(out, &d))
        }
        _ => Err("usage: arkc check MODULE... | print MODULE [-o DIR] | compat OLD NEW | build MODULE -o FILE".into()),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args.iter().map(String::as_str).collect::<Vec<_>>()) {
        Ok(said) => print!("{said}"),
        Err(e) => {
            eprintln!("arkc: {e}");
            std::process::exit(1);
        }
    }
}
