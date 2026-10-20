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
//! [`Lib::loader`] is how the kernel's content store runs the bodies of the
//! modules it was given (§1.7): decode, check, keep, and hand back an
//! [`Interp`]. No body comes over the wire.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use ark_macros::Node;

use crate::check::{check, Env};
use crate::hash::{FnHash, ModuleHash};
use crate::interp::{decode, parse, Machine, V};
use crate::interp::{Interp, Loaded};
use crate::ir::{Decl, Function, Node};
use crate::peer::{Cas, Ctx, Loader, Procedure};
use crate::schema::Schema;
use crate::store::{Change, Read};
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

/// A domain assembled: the schema, the module binding names to hashes,
/// every function checked into a library, and their bodies.
pub struct Domain {
    pub schema: Arc<Schema>,
    pub module: Module,
    pub lib: Lib,
    pub bodies: BTreeMap<FnHash, Vec<u8>>,
}

impl Domain {
    /// The module file: `(Module, [body])` under §1.1's codec.
    pub fn to_file(&self) -> Vec<u8> {
        (self.module.clone(), self.bodies.values().cloned().collect::<Vec<_>>()).to_bytes()
    }

    /// A module file read: every body installed once what it calls is,
    /// and the module verified (§1.5).
    pub fn from_file(bytes: &[u8]) -> Result<Domain, Refusal> {
        let (module, bodies) = <(Module, Vec<Vec<u8>>)>::from_bytes(bytes)?;
        let d = Domain {
            schema: Arc::new(module.schema.clone()),
            module,
            lib: Lib::default(),
            bodies: BTreeMap::new(),
        };
        d.with(bodies)
    }

    /// `bodies` installed beside this domain's, each once what it calls is.
    pub fn with(mut self, bodies: Vec<Vec<u8>>) -> Result<Domain, Refusal> {
        let mut todo = bodies
            .into_iter()
            .map(|b| Ok((Function::from_bytes(&b)?, b)))
            .collect::<Result<Vec<_>, Refusal>>()?;
        while !todo.is_empty() {
            let (before, mut last) = (todo.len(), None);
            for (f, b) in std::mem::take(&mut todo) {
                match self.lib.install(&self.schema, f.clone()) {
                    Ok(h) => _ = self.bodies.insert(h, b),
                    Err(e) => (last, _) = (Some(e), todo.push((f, b))),
                }
            }
            if todo.len() == before {
                return Err(last.expect("a refusal"));
            }
        }
        verify(&self.module, &self.lib)?;
        Ok(self)
    }

    /// Every function the domain holds, decoded.
    pub fn functions(&self) -> Vec<Function> {
        self.bodies.values().map(|b| Function::from_bytes(b).expect("a body installed")).collect()
    }

    pub fn get(&self, name: &str) -> FnHash {
        self.module.get(name).unwrap_or_else(|| panic!("no function {name}"))
    }

    /// A content store holding every function, loading what it is sent.
    pub fn cas(&self) -> Cas {
        let mut c = Cas::default().with_loader(self.lib.loader(self.schema.clone()));
        for b in self.bodies.values() {
            c.add_body(b.clone());
        }
        c
    }

    /// A function's answer over a store: its input parsed under its
    /// parameters, its body's value — what a query is asked.
    pub fn ask(&self, store: &dyn Read, ctx: &Ctx, name: &str, input: &[u8]) -> Result<Vec<u8>, Refusal> {
        let f = self.lib.get(&self.get(name)).expect("an installed function");
        let ty = Interp::input_ty(&f.f);
        let v = decode(&f.cx, &ty, parse(&f.cx, &ty, input)?.bytes())?;
        let callee = |h: &FnHash| self.lib.get(h);
        let mut m = Machine::new(store, ctx, &callee, &f.cx);
        m.stack = match (v, f.f.params.len()) {
            (V::Tup(vs), n) if n != 1 => vs,
            (v, _) => vec![v],
        };
        Ok(m.eval(&f.f.body)?.to_bytes())
    }

    /// A mutator's changes over a store, as the kernel runs it.
    pub fn run(&self, store: &dyn Read, ctx: &Ctx, name: &str, input: &[u8]) -> Result<Vec<Change>, Refusal> {
        self.lib.procedure(&self.get(name)).expect("an installed function").run(store, ctx, input)
    }
}

/// §1.2, §1.5 Whether `new` may follow `old`: its schema a prefix-extension
/// of old's, and every function it binds that `old` did not, checked over
/// the new schema. How many are new.
pub fn compat(old: &Module, new: &Domain) -> Result<usize, Refusal> {
    crate::schema::compat(&old.schema, &new.module.schema)?;
    let mut n = 0;
    for (name, h) in new.module.bindings.iter().filter(|b| !old.bindings.iter().any(|o| o.1 == b.1)) {
        let f = new.lib.get(h).ok_or_else(|| Refusal(format!("{name}: no function {h}")))?;
        new.lib
            .install(&new.module.schema, f.f.clone())
            .map_err(|e| Refusal(format!("{name}: {}", e.0)))?;
        n += 1;
    }
    Ok(n)
}
