//! Closure provenance: every module this server has started with, and the
//! closures each shipped (`docs/plan-db.md` D1).
//!
//! ```text
//! DATA/modules.cbor   canonical CBOR of
//!                     { t: "modules",
//!                       modules: [ { module: Bytes (its hash),
//!                                    private: Bytes (absent unless it has one),
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
//!
//! **A module with a server half is recorded by two hashes**
//! (`docs/plan-guards.md` D3). `module` is the public one — of the module a
//! client loads, every private block stripped, which is what a server says on
//! every page and what a client compares its own with — and `private`, beside
//! it, is the hash of the module the server ran, blocks and all; the closures
//! filed are the server's, blocks and all, under the public function hashes
//! the log names. Two starts whose public modules are one and whose private
//! halves differ are two records, so the file says which private bodies this
//! server has run, in the order it first ran them. A module with no private
//! block has no `private`, and its record is the bytes it was.

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

/// One module this server has run: its hash, the hash of its private half
/// where it has one (`docs/plan-guards.md` D3; the module docs), and its
/// closures by hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ran {
    pub module: Vec<u8>,
    pub private: Option<Vec<u8>>,
    pub closures: Vec<(FnHash, Closure)>,
}

impl Ran {
    /// A module with no private half.
    pub fn new(module: Vec<u8>, closures: Vec<(FnHash, Closure)>) -> Ran {
        Ran {
            module,
            private: None,
            closures,
        }
    }

    // The record's key: two starts with one public module and two private
    // halves are two records.
    fn same(&self, module: &[u8], private: &Option<Vec<u8>>) -> bool {
        self.module == module && self.private == *private
    }
}

/// The file's bytes.
pub fn encode(modules: &[Ran]) -> Vec<u8> {
    let modules = modules
        .iter()
        .map(|r| {
            let mut fields = vec![
                ("module", Value::Bytes(r.module[..].into())),
                (
                    "closures",
                    Value::List(
                        r.closures
                            .iter()
                            .map(|(h, c)| Value::record(vec![("hash", Value::Bytes(h[..].into())), ("closure", closure_value(c))]))
                            .collect(),
                    ),
                ),
            ];
            if let Some(p) = &r.private {
                fields.push(("private", Value::Bytes(p[..].into())));
            }
            Value::record(fields)
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
        Value::Bytes(b) => Ok(b.to_vec()),
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
        let private = match m {
            Value::Struct(fs) => fs.get("private").map(bytes).transpose()?,
            _ => None,
        };
        out.push(Ran {
            module: bytes(field(m, "module")?)?,
            private,
            closures: cs,
        });
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
/// Whether it is new — the first start with it — is the second half of
/// the answer: what decides whether the log on the disk may be of another
/// schema (`crate::persist::rehome`).
///
/// A record is new when no earlier one has both its hashes
/// (`docs/plan-guards.md` D3); whether the *log* may be of another schema
/// is asked of the public hash alone, since a private half moves no table.
pub fn start_with(dir: &Path, this: Ran) -> Result<(Vec<Ran>, bool)> {
    let mut ran = load(dir)?;
    let fresh = !ran.iter().any(|r| r.module == this.module);
    if !ran.iter().any(|r| r.same(&this.module, &this.private)) {
        ran.push(this);
        crate::persist::write_whole(dir, FILE, &encode(&ran))?;
    }
    Ok((ran, fresh))
}

/// The hashes of every module run, in the order first run.
pub fn hashes(ran: &[Ran]) -> Vec<Vec<u8>> {
    ran.iter().map(|r| r.module.clone()).collect()
}

/// Every function hash any of them shipped.
pub fn functions(ran: &[Ran]) -> BTreeSet<FnHash> {
    ran.iter().flat_map(|r| r.closures.iter().map(|(h, _)| h.clone())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark::hash::function_hash;

    /// The file keeps every module in the order first run, a module started
    /// with again adds nothing, and a closure read back is the closure
    /// written — the hash it was filed under is its function hash.
    ///
    /// Falsified once: with `start_with` not writing the file, the second
    /// start listed only itself.
    #[test]
    fn every_module_run_is_kept_and_read_back() {
        let dir = std::env::temp_dir().join(format!("ark-modules-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let d = ark_client::demo::domain();
        let cs: Vec<(FnHash, Closure)> = d.closures().iter().map(|(h, c)| (h.clone(), c.clone())).collect();
        let (first, fresh) = start_with(&dir, Ran::new(b"first".to_vec(), cs.clone())).unwrap();
        assert!(fresh);
        assert_eq!(hashes(&first), vec![b"first".to_vec()]);
        let (second, _) = start_with(&dir, Ran::new(b"second".to_vec(), vec![])).unwrap();
        assert_eq!(hashes(&second), vec![b"first".to_vec(), b"second".to_vec()]);
        let (again, fresh) = start_with(&dir, Ran::new(b"first".to_vec(), vec![])).unwrap();
        assert_eq!(hashes(&again), hashes(&second), "a module run before is not added twice");
        assert!(!fresh, "and is not new");
        assert_eq!(functions(&again), cs.iter().map(|(h, _)| h.clone()).collect());
        for (h, c) in &again[0].closures {
            assert_eq!(&function_hash(c), h, "a closure read back hashes as it was filed");
        }
        // A record with no private half is the bytes a record was before
        // there were private halves (`docs/plan-guards.md` D3).
        let before = canon::encode(&Value::record(vec![
            ("t", Value::text("modules")),
            (
                "modules",
                Value::List(
                    vec![Value::record(vec![
                        ("module", Value::Bytes(b"second"[..].into())),
                        ("closures", Value::List(vec![].into())),
                    ])]
                    .into(),
                ),
            ),
        ]));
        assert_eq!(encode(&[Ran::new(b"second".to_vec(), vec![])]), before);
        std::fs::write(dir.join(FILE), b"not cbor").unwrap();
        assert!(load(&dir).is_err(), "a damaged file is refused, not started over");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `docs/plan-guards.md` D3 A module with a private half is recorded by
    /// both hashes, and two starts whose public modules are one and whose
    /// private halves differ are two records — which private bodies this
    /// server has run — while the second is not a new *module*, since the
    /// log's schema is the public one's. Read back as written. Falsified by
    /// keying a record by its public hash alone: the second private half was
    /// not recorded.
    #[test]
    fn a_private_half_is_recorded_beside_the_public_hash() {
        let dir = std::env::temp_dir().join(format!("ark-modules-private-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let with = |p: &[u8]| Ran {
            module: b"public".to_vec(),
            private: Some(p.to_vec()),
            closures: vec![],
        };
        let (ran, fresh) = start_with(&dir, with(b"one")).unwrap();
        assert!(fresh);
        assert_eq!(ran.len(), 1);
        let (ran, fresh) = start_with(&dir, with(b"two")).unwrap();
        assert!(!fresh, "one public module, so one schema");
        assert_eq!(
            ran.iter().map(|r| r.private.clone()).collect::<Vec<_>>(),
            vec![Some(b"one".to_vec()), Some(b"two".to_vec())],
            "both private halves, in the order first run"
        );
        let (again, _) = start_with(&dir, with(b"one")).unwrap();
        assert_eq!(again, ran, "a private half run before is not added twice");
        assert_eq!(load(&dir).unwrap(), ran, "read back as written");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
