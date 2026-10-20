//! The module the hub runs (§1.5): harken's own — the domain's directory as
//! this build compiled it (`crate::domain::MODULE`) — or one `HARKEN_MODULE`
//! names: a domain's directory of ArkLang, a file of it, or a module file
//! (`(Module, [body])` under §1.1's codec), read by `ark::sexp::load` — what
//! `harken-peer --module` runs, so a fleet can run "the next release"
//! (`tests/versions.rs`). A module loaded must be one this build's bindings
//! may call ([`crate::domain::fits`]: its schema grown only by appending).
//! An old module (`HARKEN_OLD_MODULES`, read the same ways) only adds its
//! bodies to the library: "did this authority run this function" is "is it
//! in the store". It must be one this module may follow ([`compat`]):
//! §1.2's prefix check on the schema, plus one verify per new hash.

use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use ark::hash::{FnHash, ModuleHash};
use ark::module::{Lib, Module};
use ark::peer::Ctx;
use ark::store::{Change, Read};
use ark::{Domain, Refusal};

/// A module assembled, and the same as `ark_client`'s per-space module.
#[derive(Clone)]
pub struct Hosted {
    pub domain: Arc<Domain>,
    pub module: ark_client::Module,
    /// The modules held as run before ([`Hosted::hold`]), by hash.
    pub held: Vec<ModuleHash>,
}

/// A module read from a path, as [`ark::sexp::load`] reads one.
fn load(path: &Path, what: &str) -> Result<Domain> {
    ark::sexp::load(path).map_err(|e| anyhow!("{what} {}: {}", path.display(), e.0))
}

impl Hosted {
    /// harken's own domain, as this build compiled it.
    pub fn harken() -> Hosted {
        Hosted::of(crate::domain::host::load())
    }

    pub fn of(domain: Domain) -> Hosted {
        let module = ark_client::Module {
            schema: domain.schema.clone(),
            cas: domain.cas(),
            lib: domain.lib.clone(),
        };
        Hosted {
            domain: Arc::new(domain),
            module,
            held: vec![],
        }
    }

    /// A module read — every body installed and the module verified — that
    /// this build's bindings may call.
    pub fn read(path: &Path) -> Result<Hosted> {
        let d = load(path, "the module")?;
        crate::domain::fits(&d).map_err(|e| {
            anyhow!(
                "the module {} is not one this build calls: {}",
                path.display(),
                e.0
            )
        })?;
        Ok(Hosted::of(d))
    }

    /// Another module's bodies, held beside these: how many.
    pub fn hold(self, path: &Path) -> Result<(Hosted, usize)> {
        let old = load(path, "the old module")?;
        let d = Arc::try_unwrap(self.domain).unwrap_or_else(|d| Domain {
            schema: d.schema.clone(),
            module: d.module.clone(),
            lib: d.lib.clone(),
            bodies: d.bodies.clone(),
        });
        compat(&old.module, &d.module, &d.lib).map_err(|e| {
            anyhow!(
                "this module may not follow the old module {}: {}",
                path.display(),
                e.0
            )
        })?;
        let n = old.bodies.len();
        let d = d
            .with(old.bodies.values().cloned().collect())
            .map_err(|e| anyhow!("the old module {}: {}", path.display(), e.0))?;
        let held = self.held.into_iter().chain([old.module.hash()]).collect();
        Ok((
            Hosted {
                held,
                ..Hosted::of(d)
            },
            n,
        ))
    }

    /// A function's hash by its binding, `library.add_song`.
    pub fn get(&self, name: &str) -> Option<FnHash> {
        self.domain.module.get(name)
    }

    /// A function's hash by its binding, which must be there.
    pub fn func(&self, name: &str) -> FnHash {
        self.domain.get(name)
    }

    /// A mutator's changes over `store`, as the kernel runs it.
    pub fn run(
        &self,
        store: &dyn Read,
        ctx: &Ctx,
        name: &str,
        input: &[u8],
    ) -> Result<Vec<Change>, Refusal> {
        self.domain.run(store, ctx, name, input)
    }

    /// A query's answer over `store`.
    pub fn ask(
        &self,
        store: &dyn Read,
        ctx: &Ctx,
        name: &str,
        input: &[u8],
    ) -> Result<Vec<u8>, Refusal> {
        self.domain.ask(store, ctx, name, input)
    }

    /// The module file: the binding map and every body.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.domain.to_file()
    }
}

/// §1.2, §1.5 Whether `new` may follow `old` ([`ark::module::compat`]).
pub fn compat(old: &Module, new: &Module, lib: &Lib) -> Result<usize, Refusal> {
    ark::module::compat(old, new, lib)
}

/// A module file of `domain`, at `path`.
pub fn write(domain: Domain, path: &Path) -> Result<()> {
    std::fs::write(path, domain.to_file()).with_context(|| format!("writing {}", path.display()))
}
