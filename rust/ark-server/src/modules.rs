//! Closure provenance: every module this server has started with, and the
//! closures each shipped (`docs/plan-db.md` D1).
//!
//! ```text
//! DATA/modules.cbor   canonical CBOR of
//!                     { t: "modules",
//!                       modules: [ { module: Bytes (its hash),
//!                                    closures: [ { hash: Bytes, closure } … ] } … ] }
//!                     — beside `live.cbor` and `cursors.cbor`, in the order
//!                     the modules were first started with
//! ```
//!
//! **Why it exists.** A peer authors at the hashes of the module it was
//! built with, and the frozen phones are built once. A server restarted with
//! a newer module used to hold only that module's closures (and the log's
//! facts, which rebuild the state but run nothing), so an older client's
//! `create_playlist` at the hash this same server had accepted yesterday
//! became an unknown function. Here the server keeps the closures of every
//! module it has ever run, written at the first start with each module and
//! read back at every start, and `ark::peer::Authority::retire` keeps them;
//! a hash from a module this server never ran is still unknown, and the
//! client is held until it is told otherwise (§12, `held`).
//!
//! **Why it is synced and refused when damaged**, where `cursors.cbor` is
//! neither: it is written once per module rather than once a page, and what
//! rests on it is whether an old client's intents are applied or held. A
//! file that does not decode stops the server at start, with its path; the
//! fix is a person deciding, not a guess.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use ark::canon;
use ark::hash::{Closure, FnHash};
use ark::ir::decode::closure_from_value;
use ark::ir::encode::closure_value;
use ark::value::Value;

/// The file, beside `live.cbor`.
pub const FILE: &str = "modules.cbor";

/// One module this server has run: its hash, and its closures by hash.
pub type Ran = (Vec<u8>, Vec<(FnHash, Closure)>);

/// The file's bytes.
pub fn encode(modules: &[Ran]) -> Vec<u8> {
    let modules = modules
        .iter()
        .map(|(m, cs)| {
            Value::record(vec![
                ("module", Value::Bytes(m.clone())),
                (
                    "closures",
                    Value::List(
                        cs.iter()
                            .map(|(h, c)| Value::record(vec![("hash", Value::Bytes(h.clone())), ("closure", closure_value(c))]))
                            .collect(),
                    ),
                ),
            ])
        })
        .collect();
    canon::encode(&Value::record(vec![("t", Value::text("modules")), ("modules", Value::List(modules))]))
}

fn field<'a>(v: &'a Value, k: &str) -> Result<&'a Value> {
    match v {
        Value::Struct(m) => m.get(k).ok_or_else(|| anyhow!("missing {k}")),
        _ => bail!("expected a struct"),
    }
}

fn list(v: &Value) -> Result<&[Value]> {
    match v {
        Value::List(xs) => Ok(xs),
        _ => bail!("expected a list"),
    }
}

fn bytes(v: &Value) -> Result<Vec<u8>> {
    match v {
        Value::Bytes(b) => Ok(b.clone()),
        _ => bail!("expected bytes"),
    }
}

/// The file's bytes, read back.
pub fn decode(b: &[u8]) -> Result<Vec<Ran>> {
    let v = canon::decode(b).map_err(|e| anyhow!("not canonical CBOR: {e}"))?;
    if field(&v, "t")? != &Value::text("modules") {
        bail!("not a modules file");
    }
    let mut out = vec![];
    for m in list(field(&v, "modules")?)? {
        let mut cs = vec![];
        for c in list(field(m, "closures")?)? {
            let closure = closure_from_value(field(c, "closure")?).map_err(|e| anyhow!("a closure does not decode: {e}"))?;
            cs.push((bytes(field(c, "hash")?)?, closure));
        }
        out.push((bytes(field(m, "module")?)?, cs));
    }
    Ok(out)
}

/// Every module run before, as the file holds them, in the order first
/// run; nothing when there is no file yet.
pub fn load(dir: &Path) -> Result<Vec<Ran>> {
    let path = dir.join(FILE);
    match std::fs::read(&path) {
        Ok(b) => decode(&b).with_context(|| format!("{}: the modules this server has run", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Start with `module`: every module run before (`dir`'s file), and this
/// one added and the file rewritten if it is new. What the hub holds and
/// `/healthz` lists.
pub fn start_with(dir: &Path, module: Vec<u8>, closures: impl IntoIterator<Item = (FnHash, Closure)>) -> Result<Vec<Ran>> {
    let mut ran = load(dir)?;
    if !ran.iter().any(|(m, _)| *m == module) {
        ran.push((module, closures.into_iter().collect()));
        crate::persist::write_whole(dir, FILE, &encode(&ran))?;
    }
    Ok(ran)
}

/// The hashes of every module run, in the order first run.
pub fn hashes(ran: &[Ran]) -> Vec<Vec<u8>> {
    ran.iter().map(|(m, _)| m.clone()).collect()
}

/// Every function hash any of them shipped.
pub fn functions(ran: &[Ran]) -> BTreeSet<FnHash> {
    ran.iter().flat_map(|(_, cs)| cs.iter().map(|(h, _)| h.clone())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark::hash::function_hash;

    /// The file keeps every module in the order first run, a module started
    /// with again adds nothing, and a closure read back is the closure
    /// written — the hash it was filed under is its function hash.
    #[test]
    fn every_module_run_is_kept_and_read_back() {
        let dir = std::env::temp_dir().join(format!("ark-modules-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let d = ark_client::demo::domain();
        let cs: Vec<(FnHash, Closure)> = d.closures().iter().map(|(h, c)| (h.clone(), c.clone())).collect();
        let first = start_with(&dir, b"first".to_vec(), cs.clone()).unwrap();
        assert_eq!(hashes(&first), vec![b"first".to_vec()]);
        let second = start_with(&dir, b"second".to_vec(), vec![]).unwrap();
        assert_eq!(hashes(&second), vec![b"first".to_vec(), b"second".to_vec()]);
        let again = start_with(&dir, b"first".to_vec(), vec![]).unwrap();
        assert_eq!(hashes(&again), hashes(&second), "a module run before is not added twice");
        assert_eq!(functions(&again), cs.iter().map(|(h, _)| h.clone()).collect());
        for (h, c) in &again[0].1 {
            assert_eq!(&function_hash(c), h, "a closure read back hashes as it was filed");
        }
        std::fs::write(dir.join(FILE), b"not cbor").unwrap();
        assert!(load(&dir).is_err(), "a damaged file is refused, not started over");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
