//! `arkc fuzz`: differential fuzzing (`docs/plan-db.md` D2, the `--fuzz`
//! of §3.16 that was never built).
//!
//! ```text
//! arkc fuzz [--seed N] [--seconds S] [--cases K] [--out DIR] [--without OP,…]
//! arkc fuzz --replay FILE
//! ```
//!
//! Each case is a module — random ([`gen`]) and held to the verifier, or,
//! one case in five, the demo with its natives — and two random sessions
//! over it through `ark::sim` ([`session`]): partitions, duplicates, drops,
//! rebases, late joiners by facts and without closures, a client used by
//! nobody and then signed in, a client re-opened from what it made
//! durable, the server restarted from its journal or over an emptied one,
//! the horizon moved under a peer that comes back below it. After every op
//! every frame it put on the wire must come back from its bytes as itself,
//! every maintained view of every query on every client must be a fresh
//! hydrate, and the changes a view was told must reach the store; on the
//! demo every mutation is run native and interpreted and the two must
//! agree. After every session the fleet must have converged on the
//! authority's head and hash, the log must replay from its facts and from
//! its intents to the same state, no `Verify` may have been answered
//! "disagreed" — one below the horizon, or naming another log than the
//! authority's, is answered "unknown", which is not a finding
//! (`docs/plan-db.md` D3, D2) — and every query is driven once more under
//! raw changes by the churn generator (`tests/support/churn.rs`).
//!
//! Case `k` of a run from seed `N` is the case of seed `N + k`, so
//! `--seed N+k --cases 1` runs it alone. A finding is written under `--out`
//! as a vector in the suite's format ([`write`]), with a line in
//! `--out/README` saying where it belongs; the run goes on, and exits 1
//! if it found anything.

#[path = "../../../tests/support/churn.rs"]
mod churn;
// The demo and the JSON dialect, as `arkc vectors` has them: loaded a
// second time here because the vectors module keeps its own private.
#[allow(clippy::duplicate_mod)]
#[path = "../vectors/demo.rs"]
mod demo;
#[allow(clippy::duplicate_mod)]
#[path = "../vectors/json.rs"]
mod json;

pub mod gen;
pub mod session;
pub mod write;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use ark::authoring::Procedure;
use ark::hash::FnHash;
use ark::ir::Module;
use ark::store::MemoryStore;

use churn::Rng;

const USAGE: &str = "usage: arkc fuzz [--seed N] [--seconds S] [--cases K] [--out DIR] [--without OP,…] | --replay FILE";

/// The generator's randomness from a seed, for a caller outside this module.
pub fn churn_rng(seed: u64) -> Rng {
    Rng::new(seed)
}

/// What a run did, printed at the end and every half minute.
#[derive(Default)]
pub struct Stats {
    pub cases: u64,
    pub demo_cases: u64,
    /// Modules the generator produced, and how many of them verified.
    pub generated: u64,
    pub verified: u64,
    /// Modules that did not verify, by the first complaint.
    pub misses: BTreeMap<String, u64>,
    pub sessions: u64,
    pub churn_pushes: u64,
    pub tally: session::Tally,
    pub findings: Vec<(u64, String, String, String)>,
    /// Every finding by its signature ([`signature`]), and how often.
    pub seen: BTreeMap<String, u64>,
}

/// A finding with its particulars taken out — numbers, ids, names — so
/// that one bug met a hundred times is written once and counted.
fn signature(f: &session::Finding) -> String {
    format!("{}: {}", f.check(), kind(f.why()))
}

// A failure's text with every word that carries a number made `#`, up to
// the first `;`: what two failures of one bug share.
fn kind(why: &str) -> String {
    let first = why.split(';').next().unwrap_or("");
    // `op 12: ` is where the vector's runner was when it failed.
    let first = match first.strip_prefix("op ") {
        Some(rest) => rest.split_once(": ").map_or(first, |(_, r)| r),
        None => first,
    };
    let mut words: Vec<&str> = first
        .split_whitespace()
        .map(|w| if w.chars().any(|c| c.is_ascii_digit()) { "#" } else { w })
        .collect();
    // A list of numbers is one `#`, however long it is.
    words.dedup_by(|a, b| *a == "#" && *b == "#");
    words.join(" ").chars().take(140).collect()
}

struct Opts {
    seed: u64,
    seconds: Option<u64>,
    cases: Option<u64>,
    out: PathBuf,
    without: Vec<String>,
}

fn opts(args: &[&str]) -> Result<Opts, String> {
    let mut o = Opts {
        seed: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64),
        seconds: None,
        cases: None,
        out: PathBuf::from("fuzz-out"),
        without: vec![],
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().copied().ok_or_else(|| format!("{a} wants a value"));
        match *a {
            "--seed" => o.seed = val()?.parse().map_err(|e| format!("--seed: {e}"))?,
            "--seconds" => o.seconds = Some(val()?.parse().map_err(|e| format!("--seconds: {e}"))?),
            "--cases" => o.cases = Some(val()?.parse().map_err(|e| format!("--cases: {e}"))?),
            "--out" => o.out = PathBuf::from(val()?),
            "--without" => o.without = val()?.split(',').map(str::to_string).collect(),
            other => return Err(format!("unknown argument {other}\n{USAGE}")),
        }
    }
    if o.seconds.is_none() && o.cases.is_none() {
        o.seconds = Some(60);
    }
    Ok(o)
}

/// `arkc fuzz …`: the exit code.
pub fn main(args: &[&str]) -> i32 {
    if let ["--replay", path] = args {
        return replay(path);
    }
    let o = match opts(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("arkc fuzz: {e}");
            return 2;
        }
    };
    // The engine's panics are findings, caught and written; the default
    // hook would print each one over the run's own report.
    std::panic::set_hook(Box::new(|_| {}));
    println!("arkc fuzz: seed {}, {}", o.seed, limits(&o));
    let start = Instant::now();
    let mut last = Instant::now();
    let mut stats = Stats::default();
    let mut k = 0u64;
    while o.cases.is_none_or(|n| k < n) && o.seconds.is_none_or(|s| start.elapsed() < Duration::from_secs(s)) {
        case(o.seed.wrapping_add(k), &o, &mut stats);
        k += 1;
        if last.elapsed() > Duration::from_secs(30) {
            last = Instant::now();
            println!("  {:>6.0}s  {}", start.elapsed().as_secs_f64(), line(&stats));
        }
    }
    report(&stats, start.elapsed());
    if stats.findings.is_empty() {
        0
    } else {
        1
    }
}

fn limits(o: &Opts) -> String {
    match (o.cases, o.seconds) {
        (Some(c), Some(s)) => format!("{c} cases or {s}s"),
        (Some(c), None) => format!("{c} cases"),
        (None, Some(s)) => format!("{s}s"),
        (None, None) => "unbounded".into(),
    }
}

fn line(s: &Stats) -> String {
    format!(
        "{} cases ({} demo), {} sessions, {}/{} modules verified, {} ops, {} entries, {} verifies answered and {} unknown, {} findings",
        s.cases,
        s.demo_cases,
        s.sessions,
        s.verified,
        s.generated,
        s.tally.ops,
        s.tally.entries,
        s.tally.verifies_answered,
        s.tally.verifies_unknown,
        s.findings.len()
    )
}

fn report(s: &Stats, took: Duration) {
    let t = &s.tally;
    println!("arkc fuzz: {:.0}s", took.as_secs_f64());
    println!("  {}", line(s));
    println!(
        "  modules: {} generated, {} verified ({:.1}%)",
        s.generated,
        s.verified,
        if s.generated == 0 {
            0.0
        } else {
            100.0 * s.verified as f64 / s.generated as f64
        }
    );
    for (why, n) in &s.misses {
        println!("    missed {n:>6}  {why}");
    }
    println!(
        "  sessions: {} ops, {} mutations ({} refused by the author's own view), {} entries sequenced",
        t.ops, t.mutations, t.refused_locally, t.entries
    );
    println!(
        "    {} restarts, {} compactions, {} snapshots sent (below the horizon or another log), {} re-opens, {} sign-ins",
        t.restarts, t.compactions, t.below_horizon, t.reopens, t.sign_ins
    );
    println!(
        "    {} frames round-tripped, {} view pushes in sessions, {} under churn, {} native agreements",
        t.frames, t.view_pushes, s.churn_pushes, t.natives_checked
    );
    println!(
        "    rules (docs/plan-auth.md): {} peers ended partial and {} whole, {} role changes, {} writes refused as forbidden on the device and {} by the authority",
        t.partial_peers, t.whole_peers, t.role_changes, t.forbidden_local, t.forbidden_remote
    );
    for (seed, check, path, why) in &s.findings {
        println!("  finding: seed {seed}, {check}: {why}\n    written to {path}");
    }
    for (sig, n) in &s.seen {
        println!("  {n:>6} × {sig}");
    }
}

/// The demo, built, and its procedures run native.
pub fn demo_module() -> (Module, Vec<(FnHash, Procedure)>) {
    let m = ark::authoring::Module::new((demo::demo(),));
    (m.build().clone(), m.procedures())
}

// One case: a module, two sessions over it, the churn over what the second
// left.
fn case(seed: u64, o: &Opts, stats: &mut Stats) {
    stats.cases += 1;
    let mut rng = churn_rng(seed);
    let (m, natives) = if rng.chance(20) {
        stats.demo_cases += 1;
        demo_module()
    } else {
        let mut got = None;
        for _ in 0..20 {
            let sch = gen::schema(&mut rng);
            stats.generated += 1;
            match gen::module(&mut rng, &sch) {
                Ok(m) => {
                    stats.verified += 1;
                    got = Some(m);
                    break;
                }
                Err(why) => *stats.misses.entry(why).or_default() += 1,
            }
        }
        match got {
            Some(m) => (m, vec![]),
            None => return,
        }
    };
    let found = |f: session::Finding, stats: &mut Stats| {
        // One vector per kind of failure: the rest are counted.
        let sig = signature(&f);
        let seen = stats.seen.entry(sig).or_default();
        *seen += 1;
        if *seen > 1 {
            return;
        }
        let (f, small) = shrink(&m, f);
        let path = match write::write(&o.out, seed, &small, &f) {
            Ok(p) => o.out.join(p).display().to_string(),
            Err(e) => format!("(not written: {e})"),
        };
        let why: String = f.why().chars().take(300).collect();
        println!("  finding: seed {seed}, {}: {why}\n    written to {path}", f.check());
        stats.findings.push((seed, f.check().to_string(), path, why));
    };
    let mut last = MemoryStore::empty(m.schema.clone());
    for _ in 0..2 {
        stats.sessions += 1;
        let (f, st) = session::run(&m, &natives, rng.next(), &o.without, &mut stats.tally);
        if let Some(f) = f {
            found(f, stats);
            return;
        }
        last = st;
    }
    if let Some(f) = session::churn(&m, &last, rng.next(), 25, &mut stats.churn_pushes) {
        found(f, stats);
    }
}

/// `arkc fuzz --replay FILE`: a `rebase/fleet-fuzz-*` vector run again,
/// op by op, and what the fleet holds when it does not converge — what a
/// finding is read with.
pub fn replay(path: &str) -> i32 {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("arkc fuzz: {path}: {e}");
            return 2;
        }
    };
    let v = match ark::json::decode(&text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("arkc fuzz: {path}: {e:?}");
            return 2;
        }
    };
    let m = match ark::ir::module_from_value(&v.field("module")) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("arkc fuzz: {path}: not a module: {e}");
            return 2;
        }
    };
    if v.as_struct().contains_key("query") {
        return replay_view(&m, &v);
    }
    let script = v.field("script").as_list();
    let mut sim = ark::sim::Sim::new(
        m.schema.clone(),
        ark::hash::closures(&m),
        v.field("clients").as_int(),
        v.field("seed").as_int() as u64,
    )
    .durable();
    for (k, o) in script.iter().enumerate() {
        let op = match ark::sim::Op::from_value(o) {
            Ok(op) => op,
            Err(e) => {
                eprintln!("op {k}: {e}");
                return 2;
            }
        };
        if matches!(op, ark::sim::Op::Settle) {
            println!("op {k}: settle");
        }
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sim.run(&op)));
        if r.is_err() {
            println!("op {k} ({op:?}) panicked");
            describe(&sim);
            return 1;
        }
        // `docs/plan-auth.md` As `run_script` holds it after every op.
        if let Err(e) = sim.partitions_hold() {
            println!("op {k} ({op:?}): {e}");
            describe(&sim);
            return 1;
        }
    }
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sim.settle()));
    if r.is_err() {
        println!("the final settle panicked");
        describe(&sim);
        return 1;
    }
    match sim.converged().and_then(|_| sim.replays()) {
        Ok(()) => {
            println!("converged at {:?}", sim.server_hash().0);
            0
        }
        Err(e) => {
            println!("{e}");
            describe(&sim);
            1
        }
    }
}

fn describe(sim: &ark::sim::Sim) {
    let a = &sim.server.authority;
    println!(
        "server: head {} horizon {} log {:?}",
        a.log.head_seq(),
        a.log.horizon(),
        a.log.id().map(|i| ark::value::hex(&i))
    );
    for (i, c) in &sim.clients {
        let r = &c.replica;
        println!(
            "client {i}: cursor {} log {:?} linked {} mode {:?} pending {} inbox {:?} needs {:?} nobody {} denied {:?} to_server {} to_client {}",
            r.cursor,
            r.log_id.map(|i| ark::value::hex(&i)),
            c.linked,
            c.mode,
            r.pending.len(),
            r.inbox.keys().collect::<Vec<_>>(),
            r.needs(),
            sim.nobody.contains(i),
            c.denied,
            sim.to_server.get(i).map_or(0, |q| q.len()),
            sim.to_client.get(i).map_or(0, |q| q.len()),
        );
        for e in &r.pending {
            println!("    pending {} by {:?}/{:?}", ark::value::hex(&e.id), e.actor, e.session);
        }
        println!("    confirmed {}", ark::json::json(&ark::store::Store::store_value(&r.confirmed)));
    }
    println!("server store {}", ark::json::json(&ark::store::Store::store_value(&a.store)));
    for i in sim.clients.keys() {
        let who = sim.identity_of(*i);
        let rows = ark::rules::visible_rows(&a.store, who.who());
        println!(
            "  {} {:?} may see {:?}",
            who.user,
            who.roles,
            rows.iter().map(|(t, rs)| (t, rs.len())).collect::<Vec<_>>()
        );
        println!(
            "    served {:?} partial {}",
            sim.served.get(i).map(|w| (&w.user, &w.roles)),
            sim.clients[i].replica.partial
        );
    }
    for (n, (e, f)) in &a.log.entries {
        println!("  {n}: {} {} {:?}", ark::value::hex(&e.id), e.actor, f.len());
    }
}

// Whether a scripted session fails as `ark::sim::run_script` runs it — the
// vector's own runner — and how.
fn fails(m: &Module, clients: i64, seed: u64, script: &[ark::sim::Op]) -> Option<String> {
    let vals: Vec<ark::value::Value> = script.iter().map(|o| o.value()).collect();
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ark::sim::run_script(m.schema.clone(), ark::hash::closures(m), clients, seed, &vals)
    }));
    match run {
        Ok(Ok(_)) => None,
        Ok(Err(e)) => Some(e),
        Err(p) => Some(
            p.downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "a panic".into()),
        ),
    }
}

/// A session's script cut down to what still fails as the vector's runner
/// runs it: chunks of ops removed, halving, while the failure stands — a
/// vector a person can read. A finding the runner does not reproduce (one
/// that needs natives, or a view) is kept whole, and says so.
fn shrink(m: &Module, f: session::Finding) -> (session::Finding, Module) {
    let session::Finding::Session {
        check,
        why,
        clients,
        sim_seed,
        script,
    } = f
    else {
        return (f, m.clone());
    };
    let Some(first) = fails(m, clients, sim_seed, &script) else {
        return (
            session::Finding::Session {
                check,
                why: format!("{why} (not reproduced by run_script alone)"),
                clients,
                sim_seed,
                script,
            },
            m.clone(),
        );
    };
    // A cut that fails some other way is not this finding.
    let want = kind(&first);
    let same = |e: &str| kind(e) == want;
    let mut cur = script;
    let mut last = first;
    let mut budget = 600;
    let mut chunk = cur.len().div_ceil(2);
    while chunk >= 1 && budget > 0 {
        let mut i = 0;
        while i < cur.len() && budget > 0 {
            budget -= 1;
            let mut cand = cur.clone();
            cand.drain(i..(i + chunk).min(cur.len()));
            match fails(m, clients, sim_seed, &cand) {
                Some(e) if same(&e) => {
                    cur = cand;
                    last = e;
                }
                _ => i += chunk,
            }
        }
        if chunk == 1 {
            break;
        }
        chunk = chunk.div_ceil(2).min(chunk - 1).max(1);
    }
    // Then the module: every function the script does not name, taken
    // out one at a time while the module still verifies and the script
    // still fails the same way.
    let named: std::collections::BTreeSet<String> = cur
        .iter()
        .filter_map(|o| match o {
            ark::sim::Op::Mutate { function, .. } => Some(function.clone()),
            _ => None,
        })
        .collect();
    let mut small = m.clone();
    for name in m.functions.iter().rev().map(|f| f.name.clone()) {
        if named.contains(&name) || budget == 0 {
            continue;
        }
        budget -= 1;
        let mut cand = small.clone();
        cand.functions.retain(|f| f.name != name);
        cand.routers.iter_mut().for_each(|r| r.uses.retain(|u| *u != name));
        let Ok(cand) = ark::verify::verify(&cand) else { continue };
        if let Some(e) = fails(&cand, clients, sim_seed, &cur) {
            if same(&e) {
                small = cand;
                last = e;
            }
        }
    }
    (
        session::Finding::Session {
            check,
            why: format!(
                "{why}; shrunk to {} ops and {} functions, which fail: {last}",
                cur.len(),
                small.functions.len()
            ),
            clients,
            sim_seed,
            script: cur,
        },
        small,
    )
}

use ark::store::Store as _;

// A `views/` finding run again, batch by batch, saying where the view and
// a fresh hydrate first part.
fn replay_view(m: &Module, v: &ark::value::Value) -> i32 {
    use ark::view;
    let sch = &m.schema;
    let name = v.field("query").as_text().to_string();
    let f = m.lookup_function(&name).expect("the query");
    let plan = f.plan.clone().expect("a plan");
    println!("plan {plan:#?}");
    let c = ark::hash::closure(m, f);
    let cx = v.field("ctx");
    let ctx = ark::eval::Ctx::new(cx.field("user").as_text(), cx.field("session").as_text());
    let mut st = MemoryStore::from_value(sch.clone(), &v.field("store_before"));
    let args = v.field("args").as_struct().clone();
    let (a, provided) = ark::eval::middleware(sch, &c, &ctx, &args, &st).expect("middleware");
    let env = view::Env {
        helpers: c.helpers.clone(),
        ctx,
        args: a,
        provided,
    };
    let mut vw = view::hydrate(sch, &plan, env.clone(), &st).expect("hydrate");
    println!("store before {}", ark::json::json(&ark::store::Store::store_value(&st)));
    println!("rows before {:?}", vw.rows());
    for (i, b) in v.field("batches").as_list().iter().enumerate() {
        let batch: Vec<ark::store::Change> = b
            .as_list()
            .iter()
            .map(|c| ark::protocol::change_from_value(c).expect("a change"))
            .collect();
        st.apply_changes(&batch);
        let r = view::push_all(sch, &st, &batch, &mut vw);
        let fresh = view::hydrate(sch, &plan, env.clone(), &st);
        let ok = view::contract(sch, &st, &vw);
        if !ok {
            println!("batch {i}: {batch:?}");
            println!("  pushed {r:?}");
            println!("  view  {:?}", vw.rows());
            println!("  fresh {:?}", fresh.as_ref().map(|f| f.rows()));
            println!("  view entries  {:?}", vw.entries);
            if let Ok(f) = &fresh {
                println!("  fresh entries {:?}", f.entries);
                for (k, e) in &vw.by_key {
                    if f.by_key.get(k) != Some(e) {
                        println!("  differs at {k:?}:\n    view  {e:?}\n    fresh {:?}", f.by_key.get(k));
                    }
                }
                for k in f.by_key.keys() {
                    if !vw.by_key.contains_key(k) {
                        println!("  only fresh has {k:?}");
                    }
                }
            }
            return 1;
        }
    }
    println!("every batch keeps the contract");
    0
}
