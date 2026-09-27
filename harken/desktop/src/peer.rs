//! The peer, headless: the engine's [`Client`] holding one replica per
//! scope, the [`Authority`] it is for each of them when there is no server,
//! the files they are kept in, and the one way a mutation is authored — the
//! generated function, run through `Client::mutate_with`. No terminal and
//! no socket: the screens draw what is here and the transport moves the
//! frames `take_outgoing` hands out and `recv_frame` takes in.
//!
//! What path a mutation takes: **authoring runs the generated code**
//! (`gen::create_playlist` and friends, as one transaction over the
//! optimistic store, through `ark::gen::run_mutator`); the entry it records
//! names the closure's hash, so **every replay — the rebase after a
//! confirmed entry lands, an authority sequencing it, a peer receiving it —
//! runs the interpreter** over the closure the module carries. In a debug
//! build every authoring call also runs the interpreter beside the generated
//! body and asserts the two produce the same changes and the same store.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ark::canon;
use ark::db::run_mutator;
use ark::eval::{apply_closure, Args, Ctx};
use ark::gen::Db;
use ark::hash::{closures, module_hash, Closure, FnHash};
use ark::ir::decode::module_from_value;
use ark::ir::{Auto, Function, Module};
use ark::log::{snapshot_of, Log, Seq};
use ark::peer::{local_commit, Authority, Changes, Replica};
use ark::protocol::{Client, Mode, ServerMsg};
use ark::schema::{Schema, ScopeName};
use ark::value::{decode_hex, hex, Id, Value};

use crate::domain::{self, gen, Call};
use crate::storage::{self, Durable};

/// How the peer is opened.
#[derive(Clone, Debug)]
pub struct Config {
    /// `ws://host:port/sync`; `None` is a peer alone, its own authority.
    pub server: Option<String>,
    pub user: String,
    pub data: PathBuf,
    /// A module file to load instead of the one the generated code embeds.
    pub module: Option<PathBuf>,
}

/// What the status line shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub user: String,
    pub server: Option<String>,
    pub linked: bool,
    pub denied: Option<String>,
    pub cursors: Vec<(ScopeName, Seq)>,
    pub pending: usize,
    pub diverged: usize,
    pub last_refusal: Option<String>,
}

pub struct Peer {
    pub module: Module,
    schema: Schema,
    bodies: BTreeMap<FnHash, Closure>,
    pub client: Client,
    /// One per scope when alone; empty when a server sequences for us.
    authorities: BTreeMap<ScopeName, Authority>,
    data: PathBuf,
    ctx: Ctx,
    server: Option<String>,
    /// What each scope's file last held: the cursor and the pending ids.
    written: BTreeMap<ScopeName, (Seq, Vec<Id>)>,
    rejections_seen: usize,
    pub last_refusal: Option<String>,
    /// How many times the generated body and the interpreter were compared.
    pub agreement_checks: u64,
}

/// The module: from a file, or the bytes the generated code embeds.
pub fn load_module(path: Option<&Path>) -> Result<Module, String> {
    let bytes = match path {
        Some(p) => std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?,
        None => decode_hex(gen::MODULE_BYTES).ok_or("MODULE_BYTES is not hex")?,
    };
    let v = canon::decode(&bytes).map_err(|e| format!("module: {e}"))?;
    module_from_value(&v).map_err(|e| format!("module: {e}"))
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn fresh_id() -> Id {
    rand::random::<[u8; 16]>()
}

/// The hash the generated code records for a function, by name.
fn generated_hash(name: &str) -> Result<FnHash, String> {
    let (_, h) = gen::FUNCTIONS
        .iter()
        .find(|(n, _)| *n == name)
        .ok_or_else(|| format!("no generated function {name}"))?;
    decode_hex(h).ok_or_else(|| format!("{name}: hash is not hex"))
}

impl Peer {
    pub fn open(cfg: Config) -> Result<Peer, String> {
        let module = load_module(cfg.module.as_deref())?;
        if cfg.module.is_some() && hex(&module_hash(&module)) != gen::MODULE_HASH {
            return Err(format!(
                "the module at {} is {}, but this build's generated code is from {}",
                cfg.module.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
                hex(&module_hash(&module)),
                gen::MODULE_HASH
            ));
        }
        let schema = module.schema.clone();
        let bodies = closures(&module);
        for (name, _) in gen::FUNCTIONS {
            let h = generated_hash(name)?;
            if !bodies.contains_key(&h) {
                return Err(format!("generated {name} names a closure the module does not carry"));
            }
        }
        let ctx = Ctx::new(cfg.user.clone(), "dev");
        let mut client = Client::open(schema.clone(), Some(cfg.user.clone()));
        let mut authorities = BTreeMap::new();
        let mut written = BTreeMap::new();
        for sc in &schema.scopes {
            let d = storage::load(&cfg.data, schema.clone(), &sc.name)?;
            written.insert(sc.name.clone(), (d.cursor, d.pending.iter().map(|e| e.id).collect()));
            if cfg.server.is_none() {
                // The authority a peer alone is for its own scope: its log
                // is the confirmed store as a snapshot at the cursor — the
                // horizon — with nothing above it yet (docs §3.9, §3.10).
                let Durable { confirmed, cursor, .. } = &d;
                authorities.insert(
                    sc.name.clone(),
                    Authority {
                        scope: sc.name.clone(),
                        schema: schema.clone(),
                        bodies: bodies.clone(),
                        log: Log {
                            base: snapshot_of(*cursor, confirmed.clone()),
                            entries: BTreeMap::new(),
                            ids: BTreeMap::new(),
                        },
                        store: confirmed.clone(),
                    },
                );
            }
            client.subscribe(
                Mode::Whole,
                Replica::open(schema.clone(), &sc.name, bodies.clone(), d.confirmed, d.cursor, d.pending),
            );
        }
        let mut peer = Peer {
            module,
            schema,
            bodies,
            client,
            authorities,
            data: cfg.data,
            ctx,
            server: cfg.server,
            written,
            rejections_seen: 0,
            last_refusal: None,
            agreement_checks: 0,
        };
        // Alone, nothing stays pending: whatever a previous run left is
        // sequenced now.
        if peer.alone() {
            let scopes: Vec<ScopeName> = peer.client.scopes.keys().cloned().collect();
            for s in &scopes {
                peer.commit_alone(s);
            }
            peer.persist()?;
        }
        Ok(peer)
    }

    /// No server: this peer sequences its own scopes.
    pub fn alone(&self) -> bool {
        !self.authorities.is_empty()
    }

    pub fn ctx(&self) -> &Ctx {
        &self.ctx
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    pub fn scopes(&self) -> Vec<ScopeName> {
        self.client.scopes.keys().cloned().collect()
    }

    fn replica(&self, scope: &str) -> Option<&Replica> {
        self.client.scopes.get(scope).map(|(r, _)| r)
    }

    /// The optimistic store of a scope, as generated queries read it.
    pub fn db(&self, scope: &str) -> Option<Db<'_>> {
        self.replica(scope).map(|r| Db::new(&r.view))
    }

    /// The cursor and confirmed-state hash of a scope, as `Verify` claims it.
    pub fn verify_at(&self, scope: &str) -> Option<(Seq, Vec<u8>)> {
        self.replica(scope).map(|r| r.verify_at())
    }

    /// The autos a function declares, drawn now: a fresh id per `NewId`,
    /// the clock per `Now`. Frozen in the entry from here on.
    pub fn autos_for(f: &Function) -> Args {
        f.autos
            .iter()
            .map(|(name, auto)| {
                let v = match auto {
                    Auto::NewId(_) => Value::Id(fresh_id()),
                    Auto::Now => Value::Int(now_ms()),
                };
                (name.clone(), v)
            })
            .collect()
    }

    /// Author a mutation through its generated body.
    pub fn call(&mut self, c: Call) -> Result<(), String> {
        let f = self
            .module
            .lookup_function(c.name)
            .ok_or_else(|| format!("the module has no function {}", c.name))?;
        if f.scope.as_deref() != Some(c.scope) {
            return Err(format!("{} is not a mutator of scope {}", c.name, c.scope));
        }
        let fh = generated_hash(c.name)?;
        let autos = Self::autos_for(f);
        let ctx = self.ctx.clone();
        let Call { scope, args, body, .. } = c;
        #[cfg(debug_assertions)]
        self.check_agreement(scope, &fh, body, &ctx, &autos, &args);
        let id = fresh_id();
        let outcome = self
            .client
            .mutate_with(scope, id, &ctx, &fh, &autos, &args, |db| body(db, &ctx, &autos, &args));
        match outcome {
            Ok(_) => {
                self.commit_alone(scope);
                self.persist()?;
                self.note_rejections();
                Ok(())
            }
            Err(why) => {
                let text = why.to_string();
                self.last_refusal = Some(text.clone());
                Err(text)
            }
        }
    }

    /// Author a mutation the generated code does not contain — one the
    /// module still carries, such as harken's `add_track`, which only a
    /// scanner authors — through the interpreter. What a test or a peer
    /// alone uses to put something in the library.
    pub fn author_by_intent(&mut self, name: &str, args: Args) -> Result<(), String> {
        let f = self
            .module
            .lookup_function(name)
            .ok_or_else(|| format!("the module has no function {name}"))?;
        let scope = f.scope.clone().ok_or_else(|| format!("{name} is not a mutator"))?;
        let fh = self
            .bodies
            .iter()
            .find(|(_, c)| c.function.name == name)
            .map(|(h, _)| h.clone())
            .ok_or_else(|| format!("no closure for {name}"))?;
        let autos = Self::autos_for(f);
        let ctx = self.ctx.clone();
        match self.client.mutate(&scope, fresh_id(), &ctx, &fh, &autos, &args) {
            Ok(_) => {
                self.commit_alone(&scope);
                self.persist()?;
                self.note_rejections();
                Ok(())
            }
            Err(why) => {
                let text = why.to_string();
                self.last_refusal = Some(text.clone());
                Err(text)
            }
        }
    }

    // The generated body and the interpreter, over copies of the optimistic
    // store: same verdict, same changes, same store afterwards.
    #[cfg(debug_assertions)]
    fn check_agreement(&mut self, scope: &str, fh: &FnHash, body: domain::Body, ctx: &Ctx, autos: &Args, args: &Args) {
        let Some(r) = self.replica(scope) else { return };
        let Some(closure) = self.bodies.get(fh) else { return };
        let mut slow_store = r.view.clone();
        let mut fast_store = r.view.clone();
        let slow = apply_closure(&self.schema, closure, ctx, autos, args, &mut slow_store).map_err(|e| e.to_string());
        let fast = run_mutator(&mut fast_store, |db| body(db, ctx, autos, args));
        assert_eq!(slow, fast, "the generated body and the interpreter disagree on a mutation");
        assert_eq!(slow_store, fast_store, "the generated body and the interpreter leave different stores");
        self.agreement_checks += 1;
    }

    // Alone: sequence everything pending in a scope and take the answers.
    fn commit_alone(&mut self, scope: &str) {
        if let Some(a) = self.authorities.get_mut(scope) {
            if let Some((r, _)) = self.client.scopes.get_mut(scope) {
                local_commit(a, r);
            }
        }
    }

    fn note_rejections(&mut self) {
        let all: Vec<String> = self
            .client
            .scopes
            .values()
            .flat_map(|(r, _)| r.rejections.iter().map(|(_, why)| why.to_string()))
            .collect();
        if all.len() > self.rejections_seen {
            self.last_refusal = all.last().cloned();
            self.rejections_seen = all.len();
        }
    }

    // -- the transport's side ----------------------------------------------

    pub fn connected(&mut self) {
        self.client.connected();
    }

    pub fn disconnected(&mut self) {
        self.client.disconnected();
    }

    /// A frame from the server, as canonical CBOR.
    pub fn recv_frame(&mut self, bytes: &[u8]) -> Result<(), String> {
        let v = canon::decode(bytes).map_err(|e| format!("frame: {e}"))?;
        let msg = ServerMsg::from_value(&v).map_err(|e| format!("frame: {e}"))?;
        self.client.recv(msg);
        self.note_rejections();
        self.persist()
    }

    /// What is to be sent, as canonical CBOR, oldest first.
    pub fn take_outgoing(&mut self) -> Vec<Vec<u8>> {
        self.client.take_outgoing().iter().map(|m| canon::encode(&m.to_value())).collect()
    }

    /// Whether any scope's optimistic store moved since this was last asked.
    pub fn take_changes(&mut self) -> bool {
        let mut moved = false;
        for (r, _) in self.client.scopes.values_mut() {
            match r.take_changes() {
                Changes::Rebuilt => moved = true,
                Changes::Applied(chs) => moved = moved || !chs.is_empty(),
            }
        }
        moved
    }

    /// Write every scope whose cursor or pending moved since its file was.
    pub fn persist(&mut self) -> Result<(), String> {
        for (name, (r, _)) in &self.client.scopes {
            let now = (r.cursor, r.pending.iter().map(|e| e.id).collect::<Vec<Id>>());
            if self.written.get(name) == Some(&now) {
                continue;
            }
            let d = Durable {
                confirmed: r.confirmed.clone(),
                cursor: r.cursor,
                pending: r.pending.clone(),
            };
            storage::save(&self.data, name, &d)?;
            self.written.insert(name.clone(), now);
        }
        Ok(())
    }

    pub fn status(&self) -> Status {
        Status {
            user: self.ctx.user.clone(),
            server: self.server.clone(),
            linked: self.client.linked,
            denied: self.client.denied.clone(),
            cursors: self.client.scopes.iter().map(|(s, (r, _))| (s.clone(), r.cursor)).collect(),
            pending: self.client.scopes.values().map(|(r, _)| r.pending.len()).sum(),
            diverged: self.client.scopes.values().map(|(r, _)| r.diverged.len()).sum(),
            last_refusal: self.last_refusal.clone(),
        }
    }
}
