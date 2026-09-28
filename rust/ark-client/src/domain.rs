//! A module and what a peer needs of it: every function's closure by hash,
//! the hash and function by name, and the procedures run natively.

use std::collections::BTreeMap;
use std::sync::Arc;

use ark::authoring::{self, Procedure};
use ark::canon;
use ark::hash::{closures, Closure, FnHash};
use ark::ir::{module_from_value, FnKind, Function, Module};

use crate::Error;

struct Inner {
    module: Module,
    closures: BTreeMap<FnHash, Closure>,
    by_name: BTreeMap<String, FnHash>,
    natives: BTreeMap<FnHash, Procedure>,
}

/// An app's module, shared cheaply: clone it into every peer, the server and
/// the tests. Generic over the app — nothing here names a domain.
#[derive(Clone)]
pub struct Domain(Arc<Inner>);

impl std::fmt::Debug for Domain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Domain")
            .field("functions", &self.0.by_name.keys().collect::<Vec<_>>())
            .field("natives", &self.0.natives.len())
            .finish()
    }
}

impl Domain {
    /// An authored module (`spec/AUTHORING.md`): its emitted IR, and every
    /// procedure native. What an app passes: `Domain::new(&my_domain::module())`.
    pub fn new(m: &authoring::Module) -> Domain {
        Domain::of(m.build().clone(), m.procedures())
    }

    /// A module and the procedures this build holds; a procedure whose hash
    /// the module does not name is dropped, and a function with no procedure
    /// runs through the interpreter.
    pub fn of(module: Module, procedures: Vec<(FnHash, Procedure)>) -> Domain {
        let closures = closures(&module);
        let by_name = closures.iter().map(|(h, c)| (c.function.name.clone(), h.clone())).collect();
        let natives = procedures.into_iter().filter(|(h, _)| closures.contains_key(h)).collect();
        Domain(Arc::new(Inner {
            module,
            closures,
            by_name,
            natives,
        }))
    }

    /// The canonical bytes of an `.ark` file, verified; `procedures` as in
    /// [`Domain::of`].
    pub fn from_bytes(bytes: &[u8], procedures: Vec<(FnHash, Procedure)>) -> Result<Domain, Error> {
        let v = canon::decode(bytes).map_err(|e| Error::Corrupt(format!("the module is not canonical CBOR: {e}")))?;
        let m = module_from_value(&v).map_err(|e| Error::Corrupt(format!("the module does not decode: {e}")))?;
        let m = ark::verify::verify(&m).map_err(|es| Error::Corrupt(format!("the module does not verify: {es:?}")))?;
        Ok(Domain::of(m, procedures))
    }

    pub fn module(&self) -> &Module {
        &self.0.module
    }

    pub fn closures(&self) -> &BTreeMap<FnHash, Closure> {
        &self.0.closures
    }

    /// Every procedure held natively, by hash.
    pub fn natives(&self) -> &BTreeMap<FnHash, Procedure> {
        &self.0.natives
    }

    /// The natives as the engine's `hold` takes them.
    pub fn native_list(&self) -> Vec<(FnHash, Procedure)> {
        self.0.natives.iter().map(|(h, p)| (h.clone(), p.clone())).collect()
    }

    /// A function by name: its hash and its declaration.
    pub fn function(&self, name: &str) -> Option<(&FnHash, &Function)> {
        let h = self.0.by_name.get(name)?;
        Some((h, &self.0.closures[h].function))
    }

    /// A mutator by name, or why it is not one.
    pub fn mutator(&self, name: &str) -> Result<(&FnHash, &Function), Error> {
        let (h, f) = self.function(name).ok_or_else(|| Error::UnknownFunction(name.into()))?;
        match f.kind {
            FnKind::Mutator => Ok((h, f)),
            _ => Err(Error::NotA(name.into(), "mutator")),
        }
    }

    /// A query by name, or why it is not one.
    pub fn query(&self, name: &str) -> Result<(&FnHash, &Function), Error> {
        let (h, f) = self.function(name).ok_or_else(|| Error::UnknownFunction(name.into()))?;
        match f.kind {
            FnKind::Query => Ok((h, f)),
            _ => Err(Error::NotA(name.into(), "query")),
        }
    }

    /// The module hash.
    pub fn hash(&self) -> Vec<u8> {
        ark::hash::module_hash(&self.0.module)
    }
}
