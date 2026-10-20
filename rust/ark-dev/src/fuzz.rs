//! The fuzzer: random modules held to the checker, random scripts over
//! the simulator, and every check a session must pass.
//!
//! A [`Gen`] module is one to three tables keyed by an `Int`, with `Int`
//! and `Text` columns, a reference to the table before (so a delete can be
//! refused as still named) and sometimes a unique index; per table, a put,
//! a delete, a bump by checked arithmetic, a guarded insert, a touch by
//! `now` and a delete for admins only; per table a sorted query and a
//! filtered one; and a scope holding every row of a table another names
//! and the rows of the rest whose first column is positive, so that a
//! union holds what its rows name. Every function goes through the
//! checker, and one it refuses is the generator's bug.
//!
//! A session ([`session`]) is a fleet of two to four clients — one fed by
//! the scope, some holding the admin role, sometimes the live room — and a
//! script of authoring, partitions and heals, the server restarted, a
//! client reopened, a sign-in, going alone and joining, waits and
//! deliveries; sometimes the hub keeps only a few entries above its
//! horizon. After every op every view kept is its patches and a fresh
//! evaluation; once settled, [`check`]: every whole client converged, the
//! partial one exactly its union, the log replayed from its facts and each
//! entry run again to them, every entry's roles its login's, every frame
//! read back as sent, and every client's trace replayed to the same.

use std::sync::Arc;

use ark::codec::{bytes_enc, int_enc};
use ark::hash::FnHash;
use ark::ir::{CtxField, Effect, Expr, Function, Op as IrOp, Std};
use ark::log::EntryId;
use ark::module::Lib;
use ark::peer::{Cas, Ctx, Feed, Who};
use ark::schema::{Schema, Space, Table};
use ark::store::{Dir, Store};
use ark::ty::Ty;
use ark_peer::{Config, Live, Module};

use crate::sim::{lcg, Net, Op};

pub struct Rng(pub u64);

impl Rng {
    pub fn below(&mut self, n: u64) -> u64 {
        self.0 = lcg(self.0);
        (self.0 >> 11) % n.max(1)
    }
    pub fn one_in(&mut self, n: u64) -> bool {
        self.below(n) == 0
    }
}

/// A column: `Int`, or `Text`.
#[derive(Clone, Copy, PartialEq)]
enum Col {
    Int,
    Text,
}

/// A random module and what a script can do with it.
pub struct Gen {
    pub module: Module,
    /// Each mutator, and its parameters' kinds; the same for queries.
    mutators: Vec<(FnHash, Vec<Col>)>,
    queries: Vec<(FnHash, Vec<Col>)>,
    pub scope: FnHash,
}

fn lit_int(n: i64) -> Expr {
    Expr::Lit(Ty::Int, int_enc(n).to_vec())
}

fn lit_text(s: &str) -> Expr {
    Expr::Lit(Ty::Named("Text".into()), bytes_enc(s.as_bytes()))
}

fn yes() -> Expr {
    Expr::Variant(Ty::bool(), 1, vec![])
}

fn op(o: IrOp, a: Expr, b: Expr) -> Expr {
    Expr::Op(o, Box::new(a), Box::new(b))
}

fn field(e: Expr, i: usize) -> Expr {
    Expr::Field(Box::new(e), i as u32)
}

fn lam(e: Expr) -> Box<Expr> {
    Box::new(Expr::Lambda(1, Box::new(e)))
}

fn ty(c: Col) -> Ty {
    match c {
        Col::Int => Ty::Int,
        Col::Text => Ty::Named("Text".into()),
    }
}

fn default(c: Col) -> Expr {
    match c {
        Col::Int => lit_int(0),
        Col::Text => lit_text(""),
    }
}

impl Gen {
    pub fn new(r: &mut Rng) -> Gen {
        let mut tables: Vec<(Table, Vec<Col>)> = vec![];
        for t in 0..1 + r.below(3) as usize {
            let mut tab = Table::new(&format!("t{t}")).col("k", "Int", Ty::Int);
            let mut cols = vec![Col::Int];
            for c in 0..1 + r.below(3) {
                let col = if r.one_in(3) { Col::Text } else { Col::Int };
                tab = match col {
                    Col::Int => tab.col(&format!("c{c}"), "Int", Ty::Int),
                    Col::Text => tab.col(&format!("c{c}"), "Text", Ty::Bytes),
                };
                cols.push(col);
            }
            if t > 0 && r.one_in(2) {
                tab = tab.col("p", "Int", Ty::Int).refs("p", &format!("t{}", t - 1));
                cols.push(Col::Int);
            }
            if cols[1] == Col::Int && r.one_in(4) {
                tab = tab.index(&["c0"], true);
            }
            tables.push((tab, cols));
        }
        let named: Vec<bool> = (0..tables.len())
            .map(|t| tables.iter().any(|(x, _)| x.refs.iter().any(|f| f.table == format!("t{t}"))))
            .collect();
        let schema = Arc::new(Schema {
            tables: tables.iter().map(|t| t.0.clone()).collect(),
        });
        let lib = Lib::default();
        let mut cas = Cas::default().with_loader(lib.loader(schema.clone()));
        let mut install = |f: Function| -> FnHash {
            let h = lib
                .install(&schema, f.clone())
                .unwrap_or_else(|e| panic!("the checker refused a generated function: {e}\n{f:?}"));
            cas.add_body(ark::ir::Node::to_bytes(&f));
            h
        };
        let (mut mutators, mut queries, mut arms) = (vec![], vec![], vec![]);
        for (t, (tab, cols)) in tables.iter().enumerate() {
            let name = tab.name.clone();
            let n = cols.len();
            let var = |shift: usize, i: usize| Expr::Var((n - 1 - i + shift) as u32);
            let key = |shift| Box::new(Expr::Tuple(vec![var(shift, 0)]));
            let exists = Expr::Std(Std::IsSome, vec![Expr::Lookup(name.clone(), key(0))]);
            let mutator = |params: &[Col], body: Expr| Function {
                types: vec![],
                params: params.iter().map(|c| ty(*c)).collect(),
                ret: Ty::unit(),
                effect: Effect::Write,
                body,
                names: vec![],
            };
            let all: Vec<Expr> = (0..n).map(|i| var(0, i)).collect();
            let put = Expr::If(
                Box::new(exists.clone()),
                Box::new(Expr::Update(
                    name.clone(),
                    key(0),
                    lam(Expr::Tuple(
                        (0..n).map(|i| if i == 0 { field(Expr::Var(0), 0) } else { var(1, i) }).collect(),
                    )),
                )),
                Box::new(Expr::Insert(name.clone(), Box::new(Expr::Tuple(all)))),
            );
            mutators.push((install(mutator(cols, put)), cols.clone()));
            let one = |body: Expr| mutator(&[Col::Int], body);
            let k1 = || Box::new(Expr::Tuple(vec![Expr::Var(0)]));
            mutators.push((install(one(Expr::Delete(name.clone(), k1()))), vec![Col::Int]));
            let defaults: Vec<Expr> = cols
                .iter()
                .enumerate()
                .map(|(i, c)| if i == 0 { Expr::Var(0) } else { default(*c) })
                .collect();
            let guarded = Expr::If(
                Box::new(op(IrOp::Ge, Expr::Std(Std::Len, vec![Expr::Scan(name.clone())]), lit_int(6))),
                Box::new(Expr::Refuse(Box::new(lit_text("full")))),
                Box::new(Expr::Insert(name.clone(), Box::new(Expr::Tuple(defaults)))),
            );
            mutators.push((install(one(guarded)), vec![Col::Int]));
            let admins = Expr::If(
                Box::new(Expr::Std(Std::Contains, vec![Expr::Ctx(CtxField::Roles), lit_text("admin")])),
                Box::new(Expr::Delete(name.clone(), k1())),
                Box::new(Expr::Refuse(Box::new(lit_text("admins only")))),
            );
            mutators.push((install(one(admins)), vec![Col::Int]));
            if let Some(i) = (1..n).find(|i| cols[*i] == Col::Int) {
                let k2 = || Box::new(Expr::Tuple(vec![Expr::Var(1)]));
                let lookup = Expr::Std(Std::IsSome, vec![Expr::Lookup(name.clone(), k2())]);
                let bumped = Expr::Tuple(
                    (0..n)
                        .map(|j| {
                            if j == i {
                                op(IrOp::Add, field(Expr::Var(0), j), Expr::Var(1))
                            } else {
                                field(Expr::Var(0), j)
                            }
                        })
                        .collect(),
                );
                let bump = Expr::If(
                    Box::new(lookup),
                    Box::new(Expr::Update(name.clone(), k2(), lam(bumped))),
                    Box::new(Expr::Refuse(Box::new(lit_text("no such row")))),
                );
                mutators.push((install(mutator(&[Col::Int, Col::Int], bump)), vec![Col::Int, Col::Int]));
                let touch = Expr::If(
                    Box::new(Expr::Std(Std::IsSome, vec![Expr::Lookup(name.clone(), k1())])),
                    Box::new(set_one(&name, n, i)),
                    Box::new(Expr::Seq(vec![])),
                );
                mutators.push((install(one(touch)), vec![Col::Int]));
                let over = Expr::Filter(
                    Box::new(Expr::Scan(name.clone())),
                    lam(op(IrOp::Gt, field(Expr::Var(0), i), Expr::Var(1))),
                );
                queries.push((install(query(cols, &[Col::Int], over)), vec![Col::Int]));
            }
            let by = if r.one_in(2) { Dir::Asc } else { Dir::Desc };
            let sorted = Expr::Sort(Box::new(Expr::Scan(name.clone())), lam(field(Expr::Var(0), 1)), vec![by]);
            let sorted = if r.one_in(2) {
                Expr::Take(Box::new(sorted), Box::new(lit_int(2)))
            } else {
                sorted
            };
            queries.push((install(query(cols, &[], sorted)), vec![]));
            let holds = if named[t] || cols[1] != Col::Int {
                yes()
            } else {
                op(IrOp::Gt, field(Expr::Var(0), 1), lit_int(0))
            };
            arms.push(holds);
        }
        let rows = tables
            .iter()
            .map(|(_, cols)| vec![Ty::Tuple(cols.iter().map(|c| ty(*c)).collect())])
            .collect();
        let scope = Function {
            types: vec![],
            params: vec![Ty::Sum(rows)],
            ret: Ty::bool(),
            effect: Effect::Pure,
            body: Expr::Match(Box::new(Expr::Var(0)), arms),
            names: vec![],
        };
        let scope = install(scope);
        Gen {
            module: Module { schema, cas, lib },
            mutators,
            queries,
            scope,
        }
    }
}

/// A row of `n` columns whose `i`-th is `now`, under the key `Var(0)`.
fn set_one(name: &str, n: usize, i: usize) -> Expr {
    let row = Expr::Tuple(
        (0..n)
            .map(|j| if j == i { Expr::Ctx(CtxField::Now) } else { field(Expr::Var(0), j) })
            .collect(),
    );
    Expr::Update(name.into(), Box::new(Expr::Tuple(vec![Expr::Var(0)])), lam(row))
}

fn query(cols: &[Col], params: &[Col], body: Expr) -> Function {
    Function {
        types: vec![],
        params: params.iter().map(|c| ty(*c)).collect(),
        ret: Ty::list(Ty::Tuple(cols.iter().map(|c| ty(*c)).collect())),
        effect: Effect::Read,
        body,
        names: vec![],
    }
}

impl Gen {
    fn value(r: &mut Rng, c: Col) -> Vec<u8> {
        match c {
            Col::Int if r.one_in(40) => int_enc(i64::MAX).to_vec(),
            Col::Int => int_enc(r.below(6) as i64 - 1).to_vec(),
            Col::Text => bytes_enc(["", "a", "b", " "][r.below(4) as usize].as_bytes()),
        }
    }

    fn input(r: &mut Rng, cols: &[Col]) -> Vec<u8> {
        cols.iter().flat_map(|c| Gen::value(r, *c)).collect()
    }

    /// Every function the module was generated with, bound by a name of
    /// its kind and place: what `ark::sexp` prints the module as.
    pub fn bindings(&self) -> Vec<(String, FnHash)> {
        let kinds = [("m", &self.mutators), ("q", &self.queries)];
        let each = kinds
            .into_iter()
            .flat_map(|(k, fs)| fs.iter().enumerate().map(move |(i, f)| (format!("fuzz.{k}{i:02}"), f.0)));
        let mut out: Vec<_> = each.chain([("fuzz.scope".to_string(), self.scope)]).collect();
        out.sort();
        out
    }

    /// A random mutation for client `i`.
    pub fn mutate(&self, r: &mut Rng, i: i64) -> Op {
        let (f, cols) = &self.mutators[r.below(self.mutators.len() as u64) as usize];
        Op::Mutate(i, *f, Gen::input(r, cols))
    }

    /// A random query and its input.
    pub fn query(&self, r: &mut Rng) -> (FnHash, Vec<u8>) {
        let (f, cols) = &self.queries[r.below(self.queries.len() as u64) as usize];
        (*f, Gen::input(r, cols))
    }
}

/// What a session reached, for a report.
#[derive(Debug, Default)]
pub struct Found {
    pub entries: usize,
    pub refused: usize,
    pub ops: usize,
}

fn login(i: i64, n: u64, admin: bool) -> Who {
    let roles = if admin { vec!["admin".to_string()] } else { vec![] };
    Who {
        actor: format!("u{}", i % 2),
        session: if i == 0 { format!("partial-{n}") } else { format!("s{i}-{n}") },
        roles,
    }
}

fn ctx_of(w: &Who) -> Ctx {
    Ctx {
        id: EntryId::default(),
        actor: w.actor.clone(),
        session: w.session.clone(),
        roles: w.roles.clone(),
        now: 0,
    }
}

/// One session from its seed: the net it left, or the first check it failed.
pub fn session(seed: u64, traces: bool) -> Result<(Net, Found), String> {
    let mut r = Rng(seed);
    let g = Gen::new(&mut r);
    let room = crate::demo::room();
    let live = r.one_in(2);
    let mut hub = Config::new(g.module.clone());
    let scope = g.scope;
    hub.feed = Arc::new(move |w: &Who| {
        if w.session.starts_with("partial") {
            Feed::Scoped(scope)
        } else {
            Feed::Whole
        }
    });
    hub.keep = r.one_in(3).then(|| 2 + r.below(4) as usize);
    if live {
        hub.live = Some(Live {
            on_join: Some(room.get("room.on_join")),
            on_leave: Some(room.get("room.on_leave")),
            arrive: Arc::new(crate::demo::arrival),
            keep: Some(4),
            ..Live::new(Module::of(&room))
        });
    }
    let n = 2 + r.below(3) as i64;
    let partial = r.one_in(2);
    let config = |i: i64| Config {
        live: live.then(|| crate::demo::live(&format!("d{i}"), 50)),
        ..Config::new(g.module.clone())
    };
    let logins: Vec<(Who, Config)> = (0..n).map(|i| (login(i + !partial as i64, 0, r.one_in(3)), config(i))).collect();
    let mut net = Net::of(hub, logins, seed);
    if let Some(c) = net.clients.get_mut(&0).filter(|_| partial) {
        c.log.feed = Feed::Scoped(scope);
    }
    let watch = |net: &mut Net, r: &mut Rng, i: i64| -> Result<(), String> {
        let (f, input) = g.query(r);
        let ctx = ctx_of(&net.clients[&i].log.me);
        net.watch(i, Space::Durable, f, input, ctx).map(|_| ())
    };
    for i in 0..n {
        watch(&mut net, &mut r, i)?;
    }
    let mut found = Found::default();
    for k in 0..30 + r.below(90) {
        let i = r.below(n as u64) as i64;
        let whole = !(partial && i == 0);
        let op = match r.below(40) {
            0..=11 => g.mutate(&mut r, i),
            12..=13 if live => Op::Call(i, room.get("room.point"), int_enc(r.below(100) as i64).to_vec()),
            14 if live => Op::Call(i, room.get("room.rename"), bytes_enc(b"x")),
            15 => Op::Partition(i),
            16 => Op::Heal(i),
            17 if r.one_in(3) => Op::Restart,
            18 if r.one_in(2) => Op::Reopen(i),
            19 if whole && r.one_in(3) => Op::SignIn(i, login(i + !partial as i64, k + 1, r.one_in(3))),
            20 if whole && r.one_in(2) => Op::Alone(i),
            21 => Op::Join(i),
            22 => Op::Wait(r.below(80) as i64),
            _ => Op::Step,
        };
        net.run(&op);
        found.ops += 1;
        if let Some(c) = net.clients.get_mut(&0).filter(|_| partial) {
            c.log.feed = Feed::Scoped(scope);
        }
        if matches!(op, Op::Reopen(_)) {
            watch(&mut net, &mut r, i)?;
        }
        net.views_hold().map_err(|e| format!("after op {k} ({op:?}): {e}"))?;
    }
    net.settle()?;
    check(&net, &g, partial, traces)?;
    found.entries = net.hub.log.log().head().0 as usize;
    found.refused = net.refused.len();
    Ok((net, found))
}

/// Everything a settled session must be.
pub fn check(net: &Net, g: &Gen, partial: bool, traces: bool) -> Result<(), String> {
    net.converged()?;
    if let Some(b) = net.misread.first() {
        return Err(format!("a frame did not read back as sent: {}", ark::hash::hex(b)));
    }
    net.views_hold()?;
    let hub = &net.hub.log;
    let schema = hub.store.schema_arc().clone();
    let entries = hub.log().entries();
    let mut s = hub.store.clone();
    for c in entries.iter().rev().flat_map(|l| l.facts.iter().rev()) {
        s.apply(&c.inverse()).map_err(|r| format!("the log's facts do not undo: {r}"))?;
    }
    if s.state_hash() != hub.log().base().state {
        return Err("the store at the horizon is not the state the log stands on".into());
    }
    for l in &entries {
        let p = g.module.cas.get(&l.entry.func).ok_or("an entry of no function")?;
        let ran = p
            .run(&s, &Ctx::of(&l.entry), &l.entry.input)
            .map_err(|e| format!("entry {} refuses when run again: {e}", l.seq))?;
        if ran != l.facts {
            return Err(format!("entry {} runs again to other facts", l.seq));
        }
        s.apply_all(&l.facts).map_err(|r| format!("entry {}'s facts do not apply: {r}", l.seq))?;
        if s.state_hash() != l.state {
            return Err(format!("entry {}'s state is not its facts'", l.seq));
        }
        let w = net.logins.get(&l.entry.session).ok_or(format!("entry {} of no login", l.seq))?;
        if (&w.actor, &w.roles) != (&l.entry.actor, &l.entry.roles) {
            return Err(format!("entry {} is stamped other than its login", l.seq));
        }
    }
    if s != hub.store {
        return Err("the log replayed from its facts is not the store".into());
    }
    if let Some(c) = net.clients.get(&0).filter(|_| partial) {
        let holds = g.module.cas.get(&g.scope).expect("the scope");
        let ctx = ctx_of(&c.log.me);
        let mut rows = vec![];
        for (t, (name, rs)) in hub.store.snapshot_rows().into_iter().enumerate() {
            let held = rs
                .into_iter()
                .filter(|row| holds.holds(&ctx, &[&[t as u8][..], row].concat()) == Ok(true));
            rows.push((name, held.collect()));
        }
        let union = Store::from_rows(schema, &rows).map_err(|r| r.0)?;
        if c.log.store != union || !c.log.pending.is_empty() {
            return Err("the partial client is not exactly its union".into());
        }
    }
    if traces {
        for (i, c) in &net.clients {
            let t = crate::trace::record(&format!("client {i}"), c);
            crate::trace::replay(&t).map_err(|e| format!("client {i}'s trace: {e}"))?;
        }
    }
    Ok(())
}
