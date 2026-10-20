//! The module the hub runs (§1.5): harken's own (`harken_domain5::domain`),
//! or one read from a file — what `HARKEN_MODULE` names and `harken-peer
//! --module` runs, so a fleet can run "the next release"
//! (`tests/versions.rs`).
//!
//! A module file is `(Module, [body])` under §1.1's codec: the binding map
//! and every function's bytes, since a module binds hashes and a runtime
//! needs the bodies. Reading one installs each body once every function it
//! calls is installed, and holds the module to [`ark5::module::verify`].
//! An old module's file (`HARKEN_OLD_MODULES`) only adds its bodies to the
//! library: "did this authority run this function" is "is it in the store".
//! It must be one this module may follow ([`compat`]): §1.2's prefix check
//! on the schema, plus one verify per new hash.

use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use ark5::author::Domain;
use ark5::hash::{FnHash, ModuleHash};
use ark5::ir::{Function, Node};
use ark5::module::{verify, Lib, Module};
use ark5::peer::Ctx;
use ark5::store::{Change, Read};
use ark5::Refusal;

/// A module assembled, and the same as `ark5_client`'s per-space module.
#[derive(Clone)]
pub struct Hosted {
    pub domain: Arc<Domain>,
    pub module: ark5_client::Module,
    /// The modules held as run before ([`Hosted::hold`]), by hash.
    pub held: Vec<ModuleHash>,
}

impl Hosted {
    /// harken's own domain.
    pub fn harken() -> Hosted {
        Hosted::of(harken_domain5::domain())
    }

    pub fn of(domain: Domain) -> Hosted {
        let module = ark5_client::Module {
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

    /// A module file read, every body installed and the module verified.
    pub fn read(path: &Path) -> Result<Hosted> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("reading the module {}", path.display()))?;
        let (module, bodies) = <(Module, Vec<Vec<u8>>)>::from_bytes(&bytes)
            .map_err(|e| anyhow!("the module {}: {}", path.display(), e.0))?;
        let mut d = Domain {
            schema: Arc::new(module.schema.clone()),
            module,
            lib: Lib::default(),
            bodies: Default::default(),
        };
        install(&mut d, bodies).map_err(|e| anyhow!("the module {}: {}", path.display(), e.0))?;
        verify(&d.module, &d.lib).map_err(|e| anyhow!("the module {}: {}", path.display(), e.0))?;
        Ok(Hosted::of(d))
    }

    /// Another module file's bodies, held beside these: how many.
    pub fn hold(self, path: &Path) -> Result<(Hosted, usize)> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("reading the old module {}", path.display()))?;
        let (old, bodies) = <(Module, Vec<Vec<u8>>)>::from_bytes(&bytes)
            .map_err(|e| anyhow!("the old module {}: {}", path.display(), e.0))?;
        let mut d = Arc::try_unwrap(self.domain).unwrap_or_else(|d| Domain {
            schema: d.schema.clone(),
            module: d.module.clone(),
            lib: d.lib.clone(),
            bodies: d.bodies.clone(),
        });
        let n = bodies.len();
        compat(&old, &d.module, &d.lib).map_err(|e| {
            anyhow!(
                "this module may not follow the old module {}: {}",
                path.display(),
                e.0
            )
        })?;
        install(&mut d, bodies)
            .map_err(|e| anyhow!("the old module {}: {}", path.display(), e.0))?;
        let held = self.held.into_iter().chain([old.hash()]).collect();
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
        (
            self.domain.module.clone(),
            self.domain.bodies.values().cloned().collect::<Vec<_>>(),
        )
            .to_bytes()
    }
}

/// §1.2, §1.5 Whether `new` may follow `old`: its schema a prefix-extension
/// of old's, and every hash `new` binds that `old` did not checked over the
/// new schema, its callees from `lib`. How many hashes are new.
pub fn compat(old: &Module, new: &Module, lib: &Lib) -> Result<usize, Refusal> {
    ark5::schema::compat(&old.schema, &new.schema)?;
    let mut n = 0;
    for (name, h) in new
        .bindings
        .iter()
        .filter(|b| !old.bindings.iter().any(|o| o.1 == b.1))
    {
        let f = lib
            .get(h)
            .ok_or_else(|| Refusal(format!("{name}: no function {h}")))?;
        lib.install(&new.schema, f.f.clone())
            .map_err(|e| Refusal(format!("{name}: {}", e.0)))?;
        n += 1;
    }
    Ok(n)
}

/// `bodies` checked into `d`'s library, each once what it calls is there.
fn install(d: &mut Domain, bodies: Vec<Vec<u8>>) -> Result<(), Refusal> {
    let mut todo: Vec<(Function, Vec<u8>)> = bodies
        .into_iter()
        .map(|b| Ok((Function::from_bytes(&b)?, b)))
        .collect::<Result<_, Refusal>>()?;
    while !todo.is_empty() {
        let before = todo.len();
        let mut last = None;
        for (f, b) in std::mem::take(&mut todo) {
            if d.lib.get(&f.hash()).is_some() {
                continue;
            }
            match d.lib.install(&d.schema, f.clone()) {
                Ok(h) => {
                    d.bodies.insert(h, b);
                }
                Err(e) => {
                    last = Some(e);
                    todo.push((f, b));
                }
            }
        }
        if todo.len() == before {
            return Err(
                last.unwrap_or_else(|| Refusal::new("a function no other can be installed before"))
            );
        }
    }
    Ok(())
}

/// A module file of `domain`, at `path`.
pub fn write(domain: Domain, path: &Path) -> Result<()> {
    std::fs::write(path, Hosted::of(domain).to_bytes())
        .with_context(|| format!("writing {}", path.display()))
}
