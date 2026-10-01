//! `arkc`: everything that is about a module rather than about running one.
//!
//! ```text
//! arkc verify  M          verify, and print the module hash
//! arkc hash    M          the module hash and every function's hash
//! arkc check   OLD NEW    log compatibility (§17): every break, exit 1; or nothing
//! arkc vectors OUTDIR     write the conformance vectors, as ark-vectors does
//!
//! arkc backup DIR OUT                 a running server's data directory, copied consistently
//! arkc restore BACKUP DIR [--same-log] put a backup into an empty directory, as a new log
//! arkc verify-log DIR [M]             replay a data directory: head, hash (with M), horizon,
//!                                     log id, modules run
//!
//! arkc fuzz [--seed N] [--seconds S] [--cases K] [--out DIR] [--without OP,…]
//!                                     differential fuzzing (D2): random modules and
//!                                     fleet sessions, every finding a vector
//! arkc fuzz --replay FILE             a finding run again, saying where it parts
//! ```
//!
//! The last three are about a server's data directory rather than a module
//! (`docs/plan-db.md` D6); what they read and why is in `ops.rs`.
//!
//! A `.ark` file is a module's canonical CBOR (`ark::ir::module_value`
//! through `ark::canon::encode`). `arkc` verifies before it does anything
//! else, and what it hashes or compares is the verified module: the hash an
//! entry names is of that form.

#[path = "fuzz/mod.rs"]
mod fuzz;
#[path = "vectors/mod.rs"]
mod vectors;

// A server's data directory: backup, restore, verify-log (D6).
mod ops;
#[cfg(test)]
mod ops_tests;

use std::process::exit;

use ark::compat;
use ark::hash::{closure, function_hash, module_hash};
use ark::ir::{module_from_value, Module};
use ark::value::hex;

const USAGE: &str = "usage: arkc verify M | hash M | check OLD NEW | vectors OUTDIR\n       arkc backup DIR OUT | restore BACKUP DIR [--same-log] | verify-log DIR [M]\n       arkc fuzz [--seed N] [--seconds S] [--cases K] [--out DIR] [--without OP,…] | fuzz --replay FILE";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["verify", path] => println!("{}", hex(&module_hash(&load(path)))),
        ["hash", path] => {
            let m = load(path);
            println!("module   {}", hex(&module_hash(&m)));
            for f in &m.functions {
                println!("{}  {}", hex(&function_hash(&closure(&m, f))), f.name);
            }
        }
        ["check", old, new] => {
            let breaks = compat::check(&load(old), &load(new));
            if breaks.is_empty() {
                println!("additive: every retained entry still applies");
            } else {
                for b in breaks {
                    eprintln!("{b:?}");
                }
                exit(1);
            }
        }
        ["vectors", out] => vectors::write_all(std::path::Path::new(out)),
        ["fuzz", rest @ ..] => exit(fuzz::main(rest)),
        // A server's data directory (D6, `ops.rs`).
        ["backup", dir, out] => print!("{}", ops::backup(dir.as_ref(), out.as_ref()).unwrap_or_else(|e| die(&e))),
        ["restore", from, dir] => print!("{}", ops::restore(from.as_ref(), dir.as_ref(), false).unwrap_or_else(|e| die(&e))),
        ["restore", from, dir, "--same-log"] => print!("{}", ops::restore(from.as_ref(), dir.as_ref(), true).unwrap_or_else(|e| die(&e))),
        ["verify-log", dir] => print!("{}", ops::verify_log(dir.as_ref(), None).unwrap_or_else(|e| die(&e))),
        ["verify-log", dir, m] => print!("{}", ops::verify_log(dir.as_ref(), Some(&load(m))).unwrap_or_else(|e| die(&e))),
        _ => die(USAGE),
    }
}

/// Read, decode and verify a module; anything wrong is fatal and named.
fn load(path: &str) -> Module {
    let bytes = std::fs::read(path).unwrap_or_else(|e| die(&format!("{path}: {e}")));
    let v = ark::canon::decode(&bytes).unwrap_or_else(|e| die(&format!("not canonical CBOR: {e}")));
    let m = module_from_value(&v).unwrap_or_else(|e| die(&format!("not a module: {e}")));
    ark::verify::verify(&m).unwrap_or_else(|es| die(&es.iter().map(|e| format!("{e}\n")).collect::<String>()))
}

fn die(s: &str) -> ! {
    eprintln!("arkc: {s}");
    exit(1)
}
