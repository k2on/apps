//! A random session over a module: a seeded fleet through `ark::sim`, ops
//! drawn against what the clients hold, and every check of D2 held after
//! each op and at the end. What it finds is a [`Finding`], carrying what
//! the writer needs to make a vector of it.

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

use ark::authoring::Procedure;
use ark::canon::{decode, encode};
use ark::eval::{self, Args, Ctx, EvalFault};
use ark::hash::{closure, closures, FnHash};
use ark::ir::{FnKind, Function, Module};
use ark::peer::Changes;
use ark::protocol::{ClientMsg, Mode, Received, ServerMsg, Snapshot};
use ark::schema::Ty;
use ark::sim::{Frame, Op, Sim};
use ark::store::{Change, MemoryStore, Store};
use ark::value::{Id, Value};
use ark::view::{self, Env, View};

use super::churn::{Churn, Rng};
use super::gen::{ENUM, TEXTS};

/// What broke, with what a vector of it needs.
pub enum Finding {
    /// The fleet: not converged, a log that does not replay, a restart or
    /// a re-open that found its storage wrong, the engine panicking.
    Session {
        check: String,
        why: String,
        clients: i64,
        sim_seed: u64,
        script: Vec<Op>,
    },
    /// A maintained view that is not a fresh hydrate: over the store it was
    /// hydrated on, the batches it was pushed.
    View {
        why: String,
        query: String,
        ctx: Ctx,
        args: Args,
        base: MemoryStore,
        batches: Vec<Vec<Change>>,
    },
    /// A frame that does not come back from its bytes as itself.
    Frame { client: bool, value: Value, why: String },
    /// A procedure run natively and interpreted, disagreeing; or a bug
    /// (`EvalError`) from a verified module on well-typed input.
    Verdict {
        check: String,
        why: String,
        function: String,
        ctx: Ctx,
        autos: Args,
        args: Args,
        store: MemoryStore,
    },
}

impl Finding {
    pub fn check(&self) -> &str {
        match self {
            Finding::Session { check, .. } | Finding::Verdict { check, .. } => check,
            Finding::View { .. } => "view",
            Finding::Frame { .. } => "frame",
        }
    }

    pub fn why(&self) -> &str {
        match self {
            Finding::Session { why, .. } | Finding::View { why, .. } | Finding::Frame { why, .. } | Finding::Verdict { why, .. } => why,
        }
    }
}

/// What a session did, for the run's totals.
#[derive(Default, Clone, Debug)]
pub struct Tally {
    pub ops: u64,
    pub mutations: u64,
    pub refused_locally: u64,
    pub entries: u64,
    pub restarts: u64,
    pub compactions: u64,
    pub below_horizon: u64,
    pub reopens: u64,
    pub sign_ins: u64,
    pub frames: u64,
    pub view_pushes: u64,
    pub natives_checked: u64,
    /// Answers to a `Verify` at the end of a session, by kind: agreed or
    /// not, and `unknown` (below the horizon, D3, or of another log, D2).
    pub verifies_answered: u64,
    pub verifies_unknown: u64,
}

struct Held {
    query: String,
    args: Args,
    ctx: Ctx,
    env: Env,
    view: View,
    base: MemoryStore,
    batches: Vec<Vec<Change>>,
    /// The plan refused over the store at some push: nothing is kept.
    dead: bool,
}

/// Values for a module's inputs, drawn against a store.
pub struct Draw<'a> {
    sch: &'a ark::schema::Schema,
    rng: Rng,
    count: u64,
    minted: Vec<Id>,
}

struct S<'a> {
    m: &'a Module,
    natives: &'a [(FnHash, Procedure)],
    rng: Rng,
    draw: Draw<'a>,
    sim: Sim,
    script: Vec<Op>,
    clients: i64,
    sim_seed: u64,
    views: BTreeMap<i64, Vec<Held>>,
    without: Vec<String>,
    mutators: Vec<Function>,
    queries: Vec<Function>,
    tally: Tally,
}

fn ctx_of(sim: &Sim, i: i64) -> Ctx {
    if sim.nobody.contains(&i) {
        Ctx::nobody()
    } else {
        Ctx::new(format!("peer-{i}"), "dev")
    }
}

// The message a panic left, for the finding.
fn panic_text(p: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = p.downcast_ref::<&str>() {
        s.to_string()
    } else {
        "a panic".into()
    }
}

/// One session of `len` ops over `m` from `seed`; natives, when given, are
/// held by the server and by every even client, the rest interpreting.
/// The finding, if any, and the authority's store at the end.
///
/// `without` names ops never drawn (`--without`): how a finding is told
/// apart from the ops it does not need.
pub fn run(m: &Module, natives: &[(FnHash, Procedure)], seed: u64, without: &[String], tally: &mut Tally) -> (Option<Finding>, MemoryStore) {
    let mut rng = Rng::new(seed);
    let clients = 2 + rng.below(3) as i64;
    let sim_seed = rng.next();
    let len = 30 + rng.below(120);
    let mut sim = Sim::new(m.schema.clone(), closures(m), clients, sim_seed).tapped().durable();
    if !natives.is_empty() {
        sim.hold(natives.to_vec(), true, (0..clients).filter(|i| i % 2 == 0));
    }
    let draw = Draw::new(&m.schema, rng.next());
    let mut s = S {
        m,
        natives,
        rng,
        draw,
        sim,
        script: vec![],
        clients,
        sim_seed,
        views: BTreeMap::new(),
        without: without.to_vec(),
        mutators: m.functions.iter().filter(|f| f.kind == FnKind::Mutator).cloned().collect(),
        queries: m.functions.iter().filter(|f| f.kind == FnKind::Query).cloned().collect(),
        tally: Tally::default(),
    };
    let out = s.session(len);
    let t = &s.tally;
    tally.ops += t.ops;
    tally.mutations += t.mutations;
    tally.refused_locally += t.refused_locally;
    tally.entries += s.sim.server.authority.log.head_seq() as u64;
    tally.restarts += t.restarts;
    tally.compactions += t.compactions;
    tally.below_horizon += t.below_horizon;
    tally.reopens += t.reopens;
    tally.sign_ins += t.sign_ins;
    tally.frames += t.frames;
    tally.view_pushes += t.view_pushes;
    tally.natives_checked += t.natives_checked;
    tally.verifies_answered += t.verifies_answered;
    tally.verifies_unknown += t.verifies_unknown;
    (out, s.sim.server.authority.store.clone())
}

impl S<'_> {
    fn finding(&self, check: &str, why: String) -> Finding {
        Finding::Session {
            check: check.into(),
            why,
            clients: self.clients,
            sim_seed: self.sim_seed,
            script: self.script.clone(),
        }
    }

    fn session(&mut self, len: usize) -> Option<Finding> {
        if let Some(f) = self.observe() {
            return Some(f);
        }
        for _ in 0..len {
            let mut op = self.draw();
            if let ark::value::Value::Text(t) = op.value().field("t") {
                if self.without.iter().any(|w| w.as_str() == &*t) {
                    op = Op::Step;
                }
            }
            if let Op::Mutate {
                peer, function, autos, args, ..
            } = &op
            {
                if let Some(f) = self.natives_agree(*peer, function, autos, args) {
                    return Some(f);
                }
            }
            if let Some(f) = self.apply(op) {
                return Some(f);
            }
        }
        // The end: everyone back and everything delivered.
        if let Some(f) = self.apply(Op::Settle) {
            return Some(f);
        }
        if let Err(why) = self.sim.converged() {
            return Some(self.finding("converged", why));
        }
        if let Err(why) = self.sim.replays() {
            return Some(self.finding("replays", why));
        }
        // Every answer a `Verify` had, whatever the script did: a sequence
        // below the horizon or past the head, or of another log than the
        // authority's — a client that has not yet had the snapshot of a
        // server that came back emptied — is answered `unknown`
        // (`docs/plan-db.md` D3, D2), recorded as `None`: the authority
        // saying it cannot say, never a finding. Only an answer that it
        // disagreed is one. Both kinds are counted, so a run shows the
        // check exercised both ways.
        let mut disagreed = None;
        for (i, c) in &self.sim.clients {
            for (n, ok) in &c.agreed {
                match ok {
                    Some(agreed) => {
                        self.tally.verifies_answered += 1;
                        if !agreed && disagreed.is_none() {
                            disagreed = Some((*i, *n));
                        }
                    }
                    None => self.tally.verifies_unknown += 1,
                }
            }
        }
        if let Some((i, n)) = disagreed {
            return Some(self.finding("verify", format!("client {i}: the authority disagreed at {n}")));
        }
        None
    }

    // One op, run under a net for the engine's panics, then everything the
    // op may have broken looked at.
    fn apply(&mut self, op: Op) -> Option<Finding> {
        self.tally.ops += 1;
        match &op {
            Op::Restart | Op::Wipe => self.tally.restarts += 1,
            Op::Compact(_) => self.tally.compactions += 1,
            Op::Reopen(_) => self.tally.reopens += 1,
            Op::SignIn(_) => self.tally.sign_ins += 1,
            Op::Mutate { .. } => self.tally.mutations += 1,
            _ => {}
        }
        self.script.push(op.clone());
        let pending_before: usize = self.sim.clients.values().map(|c| c.replica.pending.len()).sum();
        let sim = &mut self.sim;
        match catch_unwind(AssertUnwindSafe(|| sim.run(&op))) {
            Ok(Ok(())) => {}
            Ok(Err(why)) => return Some(self.finding("settle", why)),
            Err(p) => return Some(self.finding("panic", panic_text(p))),
        }
        if let Op::Mutate { .. } = op {
            let after: usize = self.sim.clients.values().map(|c| c.replica.pending.len()).sum();
            if after <= pending_before {
                self.tally.refused_locally += 1;
            }
        }
        self.observe()
    }

    // After every op: the frames it put on the wire, the views it moved,
    // and anything the fleet noticed.
    fn observe(&mut self) -> Option<Finding> {
        if let Some(tap) = &mut self.sim.tap {
            let frames = std::mem::take(tap);
            for f in frames {
                self.tally.frames += 1;
                if let Some(x) = round_trip(&f, &self.m.schema) {
                    return Some(x);
                }
                if let Frame::ToClient(ServerMsg::SnapshotOf { .. }) = f {
                    self.tally.below_horizon += 1;
                }
            }
        }
        if let Some(why) = self.sim.faults.first() {
            return Some(self.finding("faults", why.clone()));
        }
        // Everything a client's view was told since the last look, as one:
        // the store it is pushed against is the view as it stands now,
        // after all of it — so a rebuild anywhere in it is a rebuild, and
        // otherwise the changes are one batch.
        let told = std::mem::take(&mut self.sim.told);
        for (i, chs) in told {
            let ch = if chs.iter().any(|c| matches!(c, Changes::Rebuilt)) {
                Changes::Rebuilt
            } else {
                Changes::Applied(
                    chs.into_iter()
                        .flat_map(|c| match c {
                            Changes::Applied(cs) => cs,
                            Changes::Rebuilt => vec![],
                        })
                        .collect(),
                )
            };
            if let Some(f) = self.push_views(i, ch) {
                return Some(f);
            }
        }
        None
    }

    // A client's views told what its replica said: hydrated again on a
    // rebuild, pushed the changes otherwise, and held to the contract.
    fn push_views(&mut self, i: i64, ch: Changes) -> Option<Finding> {
        let m = self.m;
        let sch = &m.schema;
        let c = self.sim.clients.get(&i)?;
        let st = c.replica.view.clone();
        match ch {
            Changes::Rebuilt => {
                let ctx = ctx_of(&self.sim, i);
                let mut held = vec![];
                for q in self.queries.clone() {
                    let args = self.draw.input(&q, &st);
                    let cl = closure(m, &q);
                    let (args_in, provided) = match eval::middleware(sch, &cl, &ctx, &args, &st) {
                        Ok(x) => x,
                        Err(EvalFault::Verdict(_)) => continue,
                        Err(EvalFault::Bug(b)) => {
                            return Some(Finding::Verdict {
                                check: "bug".into(),
                                why: format!("{}: middleware: {b:?}", q.name),
                                function: q.name.clone(),
                                ctx,
                                autos: Args::new(),
                                args,
                                store: st,
                            })
                        }
                    };
                    let env = Env {
                        helpers: cl.helpers.clone(),
                        ctx: ctx.clone(),
                        args: args_in,
                        provided,
                    };
                    let plan = q.plan.clone().expect("a query is a plan");
                    let v = match view::hydrate(sch, &plan, env.clone(), &st) {
                        Ok(v) => v,
                        // A plan may refuse — arithmetic in a projection —
                        // and then there is no view to keep.
                        Err(EvalFault::Verdict(_)) => continue,
                        Err(EvalFault::Bug(b)) => {
                            return Some(Finding::View {
                                why: format!("hydrate: {b:?}"),
                                query: q.name.clone(),
                                ctx,
                                args,
                                base: st,
                                batches: vec![],
                            })
                        }
                    };
                    held.push(Held {
                        query: q.name.clone(),
                        args,
                        ctx: ctx.clone(),
                        env,
                        view: v,
                        base: st.clone(),
                        batches: vec![],
                        dead: false,
                    });
                }
                self.views.insert(i, held);
            }
            Changes::Applied(cs) => {
                if cs.is_empty() {
                    return None;
                }
                let held = self.views.get_mut(&i)?;
                for h in held.iter_mut().filter(|h| !h.dead) {
                    self.tally.view_pushes += 1;
                    h.batches.push(cs.clone());
                    let before = h.view.rows();
                    let plan = m.lookup_function(&h.query).and_then(|f| f.plan.clone()).expect("the query");
                    let bad = |why: String, h: &Held| Finding::View {
                        why,
                        query: h.query.clone(),
                        ctx: h.ctx.clone(),
                        args: h.args.clone(),
                        base: h.base.clone(),
                        batches: h.batches.clone(),
                    };
                    let pushed = catch_unwind(AssertUnwindSafe(|| view::push_all(sch, &st, &cs, &mut h.view)));
                    let ps = match pushed {
                        Ok(Ok(ps)) => ps,
                        // A verdict pushing is a verdict hydrating: the
                        // fresh read must refuse too.
                        Ok(Err(EvalFault::Verdict(r))) => match view::read(sch, &plan, &h.env.scope(sch), &st) {
                            Err(EvalFault::Verdict(_)) => {
                                // The plan refuses over this store (checked
                                // arithmetic): there is no view to keep, and
                                // this one is let go until the next rebuild.
                                h.dead = true;
                                continue;
                            }
                            _ => return Some(bad(format!("push refused {r:?}, a fresh read does not"), h)),
                        },
                        Ok(Err(e)) => return Some(bad(format!("push: {e:?}"), h)),
                        Err(p) => return Some(bad(format!("push panicked: {}", panic_text(p)), h)),
                    };
                    if !view::contract(sch, &st, &h.view) {
                        return Some(bad("the view is not a fresh hydrate".into(), h));
                    }
                    let fresh = match view::read(sch, &plan, &h.env.scope(sch), &st) {
                        Ok(r) => r,
                        Err(e) => return Some(bad(format!("a fresh read faults where the push did not: {e:?}"), h)),
                    };
                    if h.view.rows() != fresh {
                        return Some(bad("the answer is not a fresh read".into(), h));
                    }
                    if view::splice(&ps, &before) != fresh {
                        return Some(bad("the patches do not splice the answer before into the answer after".into(), h));
                    }
                }
            }
        }
        None
    }

    // The demo's procedures, native and interpreted, over the authoring
    // client's view: the same verdict, changes and store.
    fn natives_agree(&mut self, peer: i64, function: &str, autos: &Args, args: &Args) -> Option<Finding> {
        let p = self.natives.iter().find(|(_, p)| p.name() == function).map(|(_, p)| p.clone())?;
        let c = self.sim.clients.get(&peer)?;
        let ctx = ctx_of(&self.sim, peer);
        self.tally.natives_checked += 1;
        match p.agrees(&ctx, autos, args, &c.replica.view) {
            Ok(_) => None,
            Err(why) => Some(Finding::Verdict {
                check: "natives".into(),
                why,
                function: function.into(),
                ctx,
                autos: autos.clone(),
                args: args.clone(),
                store: c.replica.view.clone(),
            }),
        }
    }

    // ------------------------------------------------------------------
    // Drawing ops

    fn peer(&mut self) -> i64 {
        let ids: Vec<i64> = self.sim.clients.keys().copied().collect();
        *self.rng.pick(&ids).expect("a client")
    }

    fn draw(&mut self) -> Op {
        let r = self.rng.below(1000);
        match r {
            0..=419 if !self.mutators.is_empty() => self.mutation(),
            0..=749 => Op::Step,
            750..=799 => Op::Partition(self.peer()),
            800..=869 => Op::Heal(self.peer()),
            870..=884 => Op::Settle,
            885..=904 => Op::Restart,
            905..=909 => Op::Wipe,
            910..=929 => {
                let log = &self.sim.server.authority.log;
                let (lo, hi) = (log.horizon(), log.head_seq());
                Op::Compact(lo + self.rng.below((hi - lo + 1) as usize) as i64)
            }
            930..=944 if self.sim.clients.len() < 7 => Op::Join {
                mode: if self.rng.chance(50) { Mode::Whole } else { Mode::ByFacts },
                closures: self.rng.chance(75),
                nobody: self.rng.chance(30),
            },
            945..=959 => match self.sim.nobody.iter().next().copied() {
                Some(i) => Op::SignIn(i),
                None => Op::Step,
            },
            960..=984 => Op::Reopen(self.peer()),
            985..=999 => Op::Verify(self.peer()),
            _ => Op::Step,
        }
    }

    fn mutation(&mut self) -> Op {
        let peer = self.peer();
        let f = self.rng.pick(&self.mutators).cloned().expect("a mutator");
        let st = self.sim.clients[&peer].replica.view.clone();
        let args = self.draw.input(&f, &st);
        let mut autos = Args::new();
        for (n, a) in &f.autos {
            let v = match a {
                ark::ir::Auto::NewId(_) => Value::Id(self.draw.mint()),
                ark::ir::Auto::Now => Value::Int(1_700_000_000_000 + self.draw.count as i64 * 1000),
            };
            autos.insert(n.clone(), v);
        }
        let eid = self.draw.mint();
        Op::Mutate {
            peer,
            function: f.name.clone(),
            eid,
            autos,
            args,
        }
    }
}

impl<'a> Draw<'a> {
    pub fn new(sch: &'a ark::schema::Schema, seed: u64) -> Draw<'a> {
        Draw {
            sch,
            rng: Rng::new(seed),
            count: 0,
            minted: vec![],
        }
    }

    // A fresh id: never drawn before in this session.
    fn mint(&mut self) -> Id {
        self.count += 1;
        let mut id = [0u8; 16];
        id[0] = 0xf0;
        id[8..].copy_from_slice(&self.count.to_be_bytes());
        self.minted.push(id);
        id
    }

    /// An input for `f` drawn against `st`: a row's whole key, now and
    /// then, where the input has every key column of a table; each other
    /// field a value of its type, mostly one the store already holds.
    fn input(&mut self, f: &Function, st: &MemoryStore) -> Args {
        let mut out = Args::new();
        let names: Vec<(String, Ty)> = f.input.iter().map(|(n, fd)| (n.clone(), fd.ty.clone())).collect();
        let sch = self.sch;
        let fits: Vec<&ark::schema::Table> = sch
            .tables()
            .filter(|t| {
                t.key
                    .iter()
                    .all(|k| names.iter().any(|(n, ty)| n == k && t.column(k).is_some_and(|c| c.column_ty() == *ty)))
            })
            .collect();
        if let Some(t) = self.rng.pick(&fits).copied() {
            let rows = st.scan(&t.name);
            if self.rng.chance(75) {
                if let Some(row) = self.rng.pick(&rows) {
                    for k in &t.key {
                        out.insert(k.clone(), row.get(k).cloned().unwrap_or(Value::Null));
                    }
                }
            }
        }
        for (n, ty) in names {
            if let std::collections::btree_map::Entry::Vacant(slot) = out.entry(n) {
                slot.insert(self.value(&ty, st));
            }
        }
        out
    }

    fn value(&mut self, ty: &Ty, st: &MemoryStore) -> Value {
        match ty {
            Ty::Option(t) => {
                if self.rng.chance(30) {
                    Value::Null
                } else {
                    self.value(t, st)
                }
            }
            Ty::Id(t) => {
                let key = self.sch.lookup_table(t).and_then(|tb| tb.key.first().cloned()).unwrap_or_default();
                let keys: Vec<Value> = st.scan(t).into_iter().filter_map(|r| r.get(&key).cloned()).collect();
                if !keys.is_empty() && self.rng.chance(75) {
                    return self.rng.pick(&keys).cloned().expect("a key");
                }
                if !self.minted.is_empty() && self.rng.chance(60) {
                    return Value::Id(*self.rng.pick(&self.minted).expect("an id"));
                }
                let mut id = [0u8; 16];
                id[15] = 1 + self.rng.below(3) as u8;
                Value::Id(id)
            }
            Ty::Int => match self.rng.below(20) {
                0 => Value::Int(i64::MAX),
                1 => Value::Int(i64::MIN),
                2 => Value::Int(1 << 40),
                _ => Value::Int(self.rng.below(12) as i64 - 3),
            },
            Ty::Text => Value::text(TEXTS[self.rng.below(TEXTS.len())]),
            Ty::Bool => Value::Bool(self.rng.chance(50)),
            Ty::Bytes => Value::Bytes((0..self.rng.below(3)).map(|i| i as u8 * 7).collect()),
            Ty::Enum(vs) => Value::text(vs.get(self.rng.below(vs.len().max(1))).cloned().unwrap_or_else(|| ENUM[0].into())),
            Ty::List(t) => Value::List((0..self.rng.below(3)).map(|_| self.value(t, st)).collect()),
            Ty::Struct(fs) => Value::Struct(Box::new(fs.iter().map(|(k, t)| (k.clone(), self.value(t, st))).collect())),
        }
    }
}

// A frame encoded, decoded and read back: itself. A server frame is held
// by its value, since a closure decodes with its symbols' names gone — and
// read again as a client of `sch` reads the socket (`docs/plan-db.md`
// D7.4, `ServerMsg::decode_for`), a snapshot's rows built as they are
// decoded: the rows its values make.
fn round_trip(f: &Frame, sch: &ark::schema::Schema) -> Option<Finding> {
    match f {
        Frame::ToServer(m) => {
            let v = m.to_value();
            let back = decode(&encode(&v))
                .map_err(|e| e.to_string())
                .and_then(|d| ClientMsg::from_value(&d).map_err(|e| e.to_string()));
            match back {
                Ok(b) if b == *m => None,
                other => Some(Finding::Frame {
                    client: true,
                    value: v,
                    why: format!("read back as {other:?}"),
                }),
            }
        }
        Frame::ToClient(m) => {
            let v = m.to_value();
            let back = decode(&encode(&v))
                .map_err(|e| e.to_string())
                .and_then(|d| ServerMsg::from_value(&d).map_err(|e| e.to_string()));
            match back {
                Ok(b) if b.to_value() == v => {}
                other => {
                    return Some(Finding::Frame {
                        client: false,
                        value: v,
                        why: format!("read back as {:?}", other.map(|b| b.to_value())),
                    })
                }
            }
            let read = ServerMsg::decode_for(&encode(&v), sch);
            let same = match (&read, m) {
                (
                    Ok(Received::Snapshot(s)),
                    ServerMsg::SnapshotOf {
                        seq, rows, log_id, module, ..
                    },
                ) => *s == Snapshot::of_values(sch, *seq, rows.clone(), *log_id, module.clone()),
                (Ok(Received::Msg(b)), m) => !matches!(m, ServerMsg::SnapshotOf { .. }) && b.to_value() == v,
                _ => false,
            };
            (!same).then(|| Finding::Frame {
                client: false,
                value: v,
                why: format!("read as a client reads it: {read:?}"),
            })
        }
    }
}

/// Every query of `m` held to the contract under raw changes (the churn
/// driver's generator): hydrated over `st`, `steps` batches pushed, each
/// checked as `support/churn.rs`'s `drive` checks it.
pub fn churn(m: &Module, st: &MemoryStore, seed: u64, steps: usize, pushes: &mut u64) -> Option<Finding> {
    let sch = &m.schema;
    let ctx = Ctx::new("peer-0", "dev");
    let mut rng = Rng::new(seed ^ 0x5eed);
    for q in m.functions.iter().filter(|f| f.kind == FnKind::Query) {
        let plan = q.plan.clone().expect("a query is a plan");
        // The input drawn as a session draws it, against the store.
        let args = Draw::new(sch, rng.next()).input(q, st);
        let cl = closure(m, q);
        let Ok((args_in, provided)) = eval::middleware(sch, &cl, &ctx, &args, st) else {
            continue;
        };
        let env = Env {
            helpers: cl.helpers.clone(),
            ctx: ctx.clone(),
            args: args_in,
            provided,
        };
        let mut store = st.clone();
        let Ok(mut v) = view::hydrate(sch, &plan, env.clone(), &store) else {
            continue;
        };
        let mut churn = Churn::new(sch, &plan, rng.next());
        let mut batches: Vec<Vec<Change>> = vec![];
        for _ in 0..steps {
            let batch = churn.batch(&mut store);
            batches.push(batch.clone());
            *pushes += 1;
            let before = v.rows();
            let bad = |why: String, batches: &Vec<Vec<Change>>| Finding::View {
                why,
                query: q.name.clone(),
                ctx: ctx.clone(),
                args: args.clone(),
                base: st.clone(),
                batches: batches.clone(),
            };
            let ps = match catch_unwind(AssertUnwindSafe(|| view::push_all(sch, &store, &batch, &mut v))) {
                Ok(Ok(ps)) => ps,
                Ok(Err(EvalFault::Verdict(_))) => break,
                Ok(Err(e)) => return Some(bad(format!("push: {e:?}"), &batches)),
                Err(p) => return Some(bad(format!("push panicked: {}", panic_text(p)), &batches)),
            };
            if !view::contract(sch, &store, &v) {
                return Some(bad("the view is not a fresh hydrate".into(), &batches));
            }
            let fresh = match view::read(sch, &plan, &env.scope(sch), &store) {
                Ok(r) => r,
                Err(_) => break,
            };
            if v.rows() != fresh {
                return Some(bad("the answer is not a fresh read".into(), &batches));
            }
            if view::splice(&ps, &before) != fresh {
                return Some(bad("the patches do not splice".into(), &batches));
            }
        }
    }
    None
}
