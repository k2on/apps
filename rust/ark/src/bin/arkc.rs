//! `arkc`: everything that is about a module rather than about running one.
//!
//! ```text
//! arkc verify  M          verify, and print the module hash
//! arkc hash    M          the module hash and every function's hash
//! arkc check   OLD NEW    log compatibility (§17): every break, exit 1; or nothing
//! arkc vectors OUTDIR     write the conformance vectors, as ark-vectors does
//! ```
//!
//! A `.ark` file is a module's canonical CBOR (`ark::ir::module_value`
//! through `ark::canon::encode`). `arkc` verifies before it does anything
//! else, and what it hashes or compares is the verified module: the hash an
//! entry names is of that form.

#[path = "arkc/compat.rs"]
mod compat;
#[path = "vectors/mod.rs"]
mod vectors;

use std::process::exit;

use ark::hash::{closure, function_hash, module_hash};
use ark::ir::{module_from_value, Module};
use ark::value::hex;

const USAGE: &str = "usage: arkc verify M | hash M | check OLD NEW | vectors OUTDIR";

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
