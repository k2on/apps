//! §1.5 Functions by hash; a module is a binding map.
//!
//! ```text
//! Module { spec: Int, schema: Schema, types: [(name, Decl)], bindings: [(name, FnHash)] }
//! ModuleHash = sha256(enc(Module))
//! ```
//!
//! A function is kept in a [`Lib`] by `FnHash = sha256(enc(function))`,
//! and only once [`check`] has passed it against the schema and the
//! functions it calls, which must be there first: so what a `Lib` holds is
//! a DAG of checked functions, and "did this authority run this function"
//! is "is it in the store". A binding's name is a path, `playlists.create`,
//! which is what a router is. Two modules binding the same names to the
//! same hashes over the same schema and types are one module.
//!
//! [`Lib::loader`] is how the kernel's content store runs a body it is sent
//! (§1.7): decode, check, keep, and hand back an [`Interp`].

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use ark5_macros::Node;

use crate::check::{check, Env};
use crate::hash::{FnHash, ModuleHash};
use crate::interp::{Interp, Loaded};
use crate::ir::{Decl, Function, Node};
use crate::peer::{Loader, Procedure};
use crate::schema::Schema;
use crate::Refusal;

/// The version of this specification a module is written against.
pub const SPEC: i64 = 5;

#[derive(Clone, Debug, PartialEq, Eq, Node)]
pub struct Module {
    pub spec: i64,
    pub schema: Schema,
    pub types: Vec<(String, Decl)>,
    pub bindings: Vec<(String, FnHash)>,
}

impl Module {
    pub fn hash(&self) -> ModuleHash {
        ModuleHash::of(&self.to_bytes())
    }

    pub fn get(&self, name: &str) -> Option<FnHash> {
        self.bindings.iter().find(|(n, _)| n == name).map(|(_, h)| *h)
    }
}

/// Checked functions by hash, shared by every run that may call them.
#[derive(Clone, Debug, Default)]
pub struct Lib(Arc<RwLock<BTreeMap<FnHash, Arc<Loaded>>>>);

impl Lib {
    pub fn get(&self, h: &FnHash) -> Option<Arc<Loaded>> {
        self.0.read().unwrap().get(h).cloned()
    }

    /// Checks `f` over `schema` and keeps it under its hash.
    pub fn install(&self, schema: &Schema, f: Function) -> Result<FnHash, Refusal> {
        let callee = |h: &FnHash| self.get(h);
        let cx = check(&f, &Env { schema, callee: &callee }).map_err(|r| Refusal(format!("{}: {r}", f.hash())))?;
        let hash = f.hash();
        self.0.write().unwrap().insert(hash, Arc::new(Loaded { hash, f, cx }));
        Ok(hash)
    }

    /// A function from its bytes, checked and kept.
    pub fn load(&self, schema: &Schema, bytes: &[u8]) -> Result<FnHash, Refusal> {
        self.install(schema, Function::from_bytes(bytes)?)
    }

    /// The function as the kernel runs it.
    pub fn procedure(&self, h: &FnHash) -> Option<Arc<dyn Procedure>> {
        Some(Arc::new(Interp {
            f: self.get(h)?,
            lib: self.clone(),
        }))
    }

    /// What the content store loads a body it is sent with.
    pub fn loader(&self, schema: Arc<Schema>) -> Loader {
        let lib = self.clone();
        Arc::new(move |b| lib.procedure(&lib.load(&schema, b).ok()?))
    }
}

/// §1.5 Whether a module holds together: its bindings sorted by name, each
/// once, each a function the library checked over this schema again; its
/// types sorted, each once, and every declaration a function makes the
/// module's own.
pub fn verify(m: &Module, lib: &Lib) -> Result<(), Refusal> {
    let sorted = |ns: Vec<&String>| ns.windows(2).all(|w| w[0] < w[1]);
    if m.spec != SPEC || !sorted(m.bindings.iter().map(|b| &b.0).collect()) || !sorted(m.types.iter().map(|t| &t.0).collect()) {
        return Err(Refusal::new("a module is of this spec, its bindings and types sorted by name, each once"));
    }
    for (name, h) in &m.bindings {
        let at = |why: String| Refusal(format!("{name}: {why}"));
        let f = lib.get(h).ok_or_else(|| at(format!("no function {h}")))?;
        let callee = |h: &FnHash| lib.get(h);
        check(
            &f.f,
            &Env {
                schema: &m.schema,
                callee: &callee,
            },
        )
        .map_err(|r| at(r.0))?;
        if let Some((n, _)) = f.f.types.iter().find(|t| !m.types.contains(t)) {
            return Err(at(format!("{n} is not the module's")));
        }
    }
    Ok(())
}
