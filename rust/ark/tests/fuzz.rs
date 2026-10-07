//! The fuzzer held to itself (`arkc fuzz`, `docs/plan-db.md` D2): its
//! generator makes modules that verify, its sessions run and say what they
//! found, the ops a vector is written in read back as themselves — and every
//! finding still open, kept under `tests/fuzz-findings/` because fixing it
//! would change what the spec says, still fails the way it was found to.
//! A plain bug whose fix is in another's hands waits here too, its vector
//! as the fuzzer wrote it. A finding that starts to pass here has been
//! decided or fixed: its vector moves, unchanged but for its name, to
//! `spec/vectors/`, or is deleted with the decision that made it moot.

#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../src/bin/fuzz/mod.rs"]
mod fuzz;

use std::path::{Path, PathBuf};

use ark::eval::Ctx;
use ark::hash::closures;
use ark::ir::module_from_value;
use ark::protocol::{change_from_value, Mode};
use ark::sim::{run_script, Op, Sim};
use ark::store::{MemoryStore, Store};
use ark::value::Value;
use ark::view;

/// Nine in ten generated modules verify, or the fuzzer is fuzzing the
/// verifier's first rule. Falsified by writing every query's result type
/// as the placeholder (no `verify_patching_queries`): none verify.
#[test]
fn the_generator_mostly_verifies() {
    let mut rng = fuzz::churn_rng(1);
    let (mut made, mut good) = (0, 0);
    for _ in 0..300 {
        let sch = fuzz::gen::schema(&mut rng);
        made += 1;
        if fuzz::gen::module(&mut rng, &sch).is_ok() {
            good += 1;
        }
    }
    assert!(good * 10 >= made * 9, "{good} of {made} modules verified");
}

/// Every op a script may hold reads back from its value as itself.
#[test]
fn an_op_is_its_value() {
    let mut id = [0u8; 16];
    id[3] = 7;
    let ops = vec![
        Op::Mutate {
            peer: 2,
            function: "f".into(),
            eid: id,
            autos: [("now".to_string(), Value::Int(5))].into_iter().collect(),
            args: [("a".to_string(), Value::Null)].into_iter().collect(),
        },
        Op::Partition(1),
        Op::Heal(1),
        Op::Step,
        Op::Settle,
        Op::Restart,
        Op::Wipe,
        Op::Compact(4),
        Op::Join {
            mode: Mode::ByFacts,
            closures: false,
            nobody: true,
        },
        Op::SignIn(3),
        Op::Reopen(0),
        Op::Verify(2),
        Op::Roles {
            peer: 1,
            roles: ["r0".to_string()].into_iter().collect(),
            believes: ["r0".to_string(), "r1".to_string()].into_iter().collect(),
        },
    ];
    for op in ops {
        assert_eq!(Op::from_value(&op.value()), Ok(op.clone()));
    }
    assert!(Op::from_value(&Value::record(vec![("t", Value::text("dance"))])).is_err());
}

/// A server restarted from its journal is the server it was, and a client
/// re-opened from what it made durable is the client it was: the demo, a
/// mutation each, a restart, a re-open, and the fleet converges with
/// nothing found wrong. Falsified by opening the restarted authority's
/// store empty rather than at the head: `converged` names the client.
#[test]
fn a_restart_and_a_reopen_are_the_same_fleet() {
    let (m, _) = fuzz::demo_module();
    let mut sim = Sim::new(m.schema.clone(), closures(&m), 2, 3).durable();
    let mut pid = [0u8; 16];
    pid[0] = 0xf0;
    let mut eid = pid;
    eid[15] = 1;
    let args = |pairs: Vec<(&str, Value)>| pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    sim.run(&Op::Mutate {
        peer: 0,
        function: "create_playlist".into(),
        eid,
        autos: args(vec![("id", Value::Id(pid))]),
        args: args(vec![("name", Value::text("Road"))]),
    })
    .unwrap();
    sim.settle();
    eid[15] = 2;
    sim.run(&Op::Mutate {
        peer: 1,
        function: "add_to_playlist".into(),
        eid,
        autos: args(vec![]),
        args: args(vec![("playlist_id", Value::Id(pid)), ("track_id", Value::text("t1"))]),
    })
    .unwrap();
    for op in [Op::Restart, Op::Reopen(1), Op::Step, Op::Reopen(0)] {
        sim.run(&op).unwrap();
    }
    sim.try_settle().unwrap();
    sim.converged().unwrap();
    sim.replays().unwrap();
    assert_eq!(sim.server.authority.log.head_seq(), 2);
    assert!(sim.faults.is_empty(), "{:?}", sim.faults);
    let _ = Ctx::nobody();
}

fn findings() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fuzz-findings");
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|d| {
            d.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "json"))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

// How a `views/` finding fails: the first batch after which the maintained
// view is not a fresh hydrate, or a push that refuses where a fresh read
// does not (or the other way).
fn view_fails(v: &Value) -> Option<String> {
    let m = module_from_value(&v.field("module")).ok()?;
    let sch = &m.schema;
    let name = v.field("query").as_text().to_string();
    let f = m.lookup_function(&name)?;
    let plan = f.plan.clone()?;
    let c = ark::hash::closure(&m, f);
    let cx = v.field("ctx");
    let ctx = Ctx::new(cx.field("user").as_text(), cx.field("session").as_text());
    let mut st = MemoryStore::from_value(sch.clone(), &v.field("store_before"));
    let (a, provided) = ark::eval::middleware(sch, &c, &ctx, v.field("args").as_struct(), &st).ok()?;
    let env = view::Env {
        helpers: c.helpers.clone(),
        ctx,
        args: a,
        provided,
    };
    let mut vw = view::hydrate(sch, &plan, env.clone(), &st).ok()?;
    for (i, b) in v.field("batches").as_list().iter().enumerate() {
        let batch: Vec<_> = b.as_list().iter().map(|c| change_from_value(c).expect("a change")).collect();
        st.apply_changes(&batch);
        let pushed = view::push_all(sch, &st, &batch, &mut vw);
        let fresh = view::read(sch, &plan, &env.scope(sch), &st);
        match (pushed, fresh) {
            (Ok(_), Ok(rows)) if rows == vw.rows() && view::contract(sch, &st, &vw) => {}
            (Err(_), Err(_)) => return None,
            (p, r) => {
                return Some(format!(
                    "batch {i}: pushed {}, a fresh read {}",
                    if p.is_ok() { "answers" } else { "refuses" },
                    if r.is_ok() { "answers" } else { "refuses" }
                ))
            }
        }
    }
    None
}

/// Each open finding still fails. A finding written as a session is run
/// as `rebase/` runs one ([`run_script`]); one written as a view is run as
/// `views/` runs one, but for the answers, which are the fresh read's.
#[test]
fn every_open_finding_still_fails() {
    for p in findings() {
        let v = ark::json::decode(&std::fs::read_to_string(&p).unwrap()).unwrap_or_else(|e| panic!("{}: {e:?}", p.display()));
        let fails = if v.as_struct().contains_key("script") {
            let m = module_from_value(&v.field("module")).unwrap();
            let script = v.field("script").as_list();
            let run = std::panic::catch_unwind(|| {
                run_script(
                    m.schema.clone(),
                    closures(&m),
                    v.field("clients").as_int(),
                    v.field("seed").as_int() as u64,
                    &script,
                )
            });
            !matches!(run, Ok(Ok(_)))
        } else {
            view_fails(&v).is_some()
        };
        assert!(
            fails,
            "{} passes: the finding it records has been decided — move or delete it",
            p.display()
        );
    }
}

/// An ack names the log, and a peer that never knew it learns it there:
/// a client whose push is acknowledged and whose page is lost still says
/// the log in its next `hello`. Falsified by not reading `log` off the ack
/// in `Client::recv`: the client knows no log.
#[test]
fn an_ack_names_the_log() {
    use ark::live::Silent;
    use ark::peer::{Authority, Replica};
    use ark::protocol::{open_access, trusting, Client, ServerMsg};
    let (m, _) = fuzz::demo_module();
    let mut a = Authority::new(m.schema.clone(), closures(&m));
    let name = [7u8; 16];
    a.log.name_if_unnamed(name);
    let mut server = ark::protocol::Server::open(trusting(), open_access(), Silent, a);
    let r = Replica::open(m.schema.clone(), closures(&m), MemoryStore::empty(m.schema.clone()), 0, vec![]);
    let mut c = Client::open(r, Mode::Whole, Some("alice".into()));
    c.connected();
    let fh = closures(&m)
        .into_iter()
        .find(|(_, c)| c.function.name == "create_playlist")
        .map(|(h, _)| h)
        .unwrap();
    let mut pid = [0u8; 16];
    pid[15] = 1;
    c.mutate(
        [9u8; 16],
        &Ctx::new("alice", "dev"),
        &fh,
        &[("id".to_string(), Value::Id(pid))].into_iter().collect(),
        &[("name".to_string(), Value::text("Road"))].into_iter().collect(),
    )
    .unwrap();
    for f in c.take_outgoing() {
        server.recv(1, f);
    }
    // Only the ack arrives: every page is lost.
    for (_, f) in server.take_outgoing() {
        if matches!(f, ServerMsg::Ack { .. }) {
            c.recv(f);
        }
    }
    c.settle();
    assert_eq!(c.replica.cursor, 1, "the ack confirmed the intent");
    assert_eq!(c.replica.log_id, Some(name), "and named the log it is at");
}

/// A replica fed by replay whose replay of a confirmed entry refuses asks
/// for that entry's facts, and takes them, rather than waiting for facts
/// nobody will send it. Falsified by not recording the refusal in
/// `advance`: `needs` is empty and the cursor never moves.
#[test]
fn a_replay_that_refuses_asks_for_facts() {
    use ark::log::Entry;
    use ark::peer::Replica;
    use ark::store::{Change, Row};
    let (m, _) = fuzz::demo_module();
    let bodies = closures(&m);
    let fh = bodies
        .iter()
        .find(|(_, c)| c.function.name == "add_to_playlist")
        .map(|(h, _)| h.clone())
        .unwrap();
    let mut pid = [0u8; 16];
    pid[15] = 1;
    // The authority holds a playlist this replica's state does not.
    let e = Entry {
        id: [3u8; 16],
        actor: "bob".into(),
        session: "dev".into(),
        roles: Default::default(),
        fn_hash: fh,
        args: [("playlist_id".to_string(), Value::Id(pid)), ("track_id".to_string(), Value::text("t1"))]
            .into_iter()
            .collect(),
        autos: Default::default(),
    };
    let mut r = Replica::open(m.schema.clone(), bodies, MemoryStore::empty(m.schema.clone()), 0, vec![]);
    r.receive(1, e);
    r.settle();
    assert_eq!(r.cursor, 0, "the replay refused");
    assert_eq!(r.needs(), vec![1], "so the replica asks for the facts");
    let item = m.schema.lookup_table("item").unwrap();
    let row = Row::of(
        item,
        [
            ("playlist_id".to_string(), Value::Id(pid)),
            ("track_id".to_string(), Value::text("t1")),
            ("pos".to_string(), Value::Int(1)),
        ],
    );
    r.receive_facts(1, vec![Change::Add("item".into(), row)]);
    r.settle();
    assert_eq!(r.cursor, 1, "and takes them");
    assert_eq!(r.diverged, vec![1], "a divergence, recorded");
}
