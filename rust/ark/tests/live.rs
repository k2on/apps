//! Live state (`docs/plan-live.md`): the demo's room, built, verified,
//! classified, run natively and interpreted, and served — the server's
//! sequencer and relay, and the client's confirmed-plus-pending view —
//! over the machines alone. The fleet over a network is `tests/live_fleet.rs`
//! in `ark-server`, and the seeded one is `Sim`'s.

#[path = "../src/bin/vectors/demo.rs"]
mod demo;
#[path = "../src/bin/vectors/demo_live.rs"]
mod demo_live;

use std::collections::BTreeMap;

use ark::eval::{Args, Ctx, EvalFault, LiveCtx};
use ark::hash::live_closure;
use ark::ir::ordering::Kind;
use ark::live::state::{Call, Desk, DeviceDecl, Instance, Order, Runner, Said, Watch, Who};
use ark::store::Refusal;
use ark::value::Value;

fn module() -> ark::ir::Module {
    demo::live_module()
}

fn runner(native: bool) -> Runner {
    let mut r = Runner::of_module(&module());
    if native {
        r.hold(demo::live_natives());
    }
    r
}

fn args<const N: usize>(pairs: [(&str, Value); N]) -> Args {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn lctx(device: &str, now: i64) -> LiveCtx {
    LiveCtx {
        user: "alice".into(),
        session: format!("{device}-login"),
        roles: Default::default(),
        device: device.into(),
        now,
    }
}

fn room() -> Instance {
    Instance::new("room", "alice")
}

fn alice(device: &str) -> Who {
    Who {
        user: "alice".into(),
        session: format!("{device}-login"),
        roles: Default::default(),
    }
}

fn decl(device: &str) -> DeviceDecl {
    DeviceDecl::new(device, "phone", device, true)
}

/// The room builds into a module that verifies, with a type section of
/// two names and the state after the demo's functions; the demo's own
/// module, which has none, is the bytes it was.
#[test]
fn the_room_is_a_module_that_verifies() {
    let m = module();
    assert_eq!(m.types.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["mode", "room"]);
    assert_eq!(m.states.len(), 1);
    assert_eq!(ark::verify::verify(&m).map(|_| ()), Ok(()));
    let bytes = ark::canon::encode(&ark::ir::module_value(&m));
    let back = ark::ir::module_from_value(&ark::canon::decode(&bytes).unwrap()).unwrap();
    assert_eq!(
        ark::canon::encode(&ark::ir::module_value(&back)),
        bytes,
        "decode of encode is the identity"
    );
    assert!(ark::canon::encode(&ark::ir::module_value(&demo::module())).len() < bytes.len());
}

/// What the plan names: `point` writes only its own ephemeral slot and is
/// ephemeral; `drop_at` writes it too but reads its input into an ordered
/// field, and `bump` reads the state: both ordered. Falsified by letting an
/// ephemeral transition write any field its input alone decides: `drop_at`
/// came out ephemeral. (A write of another device's slot is
/// `ark::ir::ordering`'s own test.)
#[test]
fn the_rooms_transitions_are_classified() {
    let m = module();
    let c = live_closure(&m, &m.states[0]);
    assert_eq!(c.kind("point"), Kind::Ephemeral);
    assert_eq!(c.kind("drop_at"), Kind::Ordered);
    assert_eq!(c.kind("bump"), Kind::Ordered);
    assert_eq!(c.kind("start"), Kind::Ordered);
    assert_eq!(c.kind("on_join"), Kind::Ordered);
}

/// Every transition, native against the interpreter, over one evolving
/// state: the same new states and the same verdicts. Falsified by a native
/// `PerDevice::set` that leaves a slot it is told to empty: the two parted
/// at `drop_at`.
#[test]
fn native_agrees_with_the_interpreter_on_every_transition() {
    let natives = demo::live_natives();
    let n = &natives[0];
    let m = module();
    let mut s = ark::eval::constant(&m.states[0].init).unwrap();
    let phone = decl("phone").record("alice");
    let steps: Vec<(&str, Args)> = vec![
        ("on_join", args([("device", phone.clone())])),
        ("bump", args([("by", Value::Int(3))])),
        ("bump", args([("by", Value::Int(30))])),
        ("stop", Args::new()),
        ("start", Args::new()),
        ("point", args([("at", Value::Int(7))])),
        ("drop_at", args([("at", Value::Int(2))])),
        ("stop", Args::new()),
        ("nonesuch", Args::new()),
        ("bump", args([("by", Value::text("three"))])),
        ("on_leave", args([("device", phone)])),
        ("on_restore", Args::new()),
    ];
    let mut verdicts = vec![];
    for (k, (t, input)) in steps.into_iter().enumerate() {
        match n.agrees(t, &s, &lctx("phone", 1000 + k as i64), &input) {
            Err(e) => panic!("{e}"),
            Ok(Ok(next)) => s = next,
            Ok(Err(EvalFault::Verdict(Refusal::Refused(why)))) => verdicts.push(why),
            Ok(Err(other)) => panic!("{t}: {other:?}"),
        }
    }
    assert_eq!(
        verdicts,
        [
            "by: between 1 and 10",
            "not counting",
            "room: no transition nonesuch",
            "by: expected an int, found text"
        ]
    );
    assert_eq!(s.field("count"), Value::Int(2));
    assert_eq!(s.field("devices"), Value::from(BTreeMap::<String, Value>::new()));
}

/// The state's hash leaves the ephemeral fields out, and a server keeps
/// only the ordered part: two rooms that differ only in a pointer hash
/// alike and are kept alike. Falsified by hashing the whole value: the
/// two hashes differed.
#[test]
fn the_hash_and_the_kept_snapshot_ignore_ephemeral_fields() {
    let r = runner(false);
    let s = r.init("room").unwrap();
    let pointed = r.run("room", "point", &s, &lctx("phone", 1), &args([("at", Value::Int(4))])).unwrap();
    assert_ne!(s, pointed);
    assert_eq!(r.hash("room", &s), r.hash("room", &pointed));
    let mut desk = Desk::new(r);
    desk.restore(room(), 3, pointed.clone(), 0);
    let kept = desk.kept();
    assert_eq!(kept.len(), 1);
    assert!(!matches!(&kept[0].2, Value::Struct(m) if m.contains_key("pointer")), "{:?}", kept[0].2);
}

// A desk and the frames it said, by connection.
fn said_to(out: &[(i64, Said)], c: i64) -> Vec<Said> {
    out.iter().filter(|(to, _)| *to == c).map(|(_, s)| s.clone()).collect()
}

fn call(id: u8, transition: &str, input: Args, counter: Option<i64>) -> Call {
    let mut i = [0u8; 16];
    i[15] = id;
    Call {
        instance: room(),
        transition: transition.into(),
        input,
        id: i,
        counter,
    }
}

/// Two devices of one account stand in the room: each is told the other
/// arriving, an ordered call reaches both with its sequence and the
/// server's time, an ephemeral one reaches the other and not its author,
/// and leaving clears the leaver's pointer. Falsified by echoing an
/// ephemeral call to its author: the phone heard its own pointer; and by
/// not emptying the leaver's slots before `on_leave`: its pointer stayed.
#[test]
fn two_devices_see_each_other_and_leaving_clears_a_pointer() {
    let mut desk = Desk::new(runner(true));
    let out = desk.hello(1, alice("phone"), decl("phone"), 10);
    assert!(matches!(said_to(&out, 1).as_slice(), [Said::Snapshot(s)] if s.seq == 1), "{out:?}");
    let out = desk.hello(2, alice("laptop"), decl("laptop"), 20);
    assert!(matches!(said_to(&out, 1).as_slice(), [Said::Applied(a)] if a.transition == "on_join" && a.seq() == Some(2)));
    assert!(matches!(said_to(&out, 2).as_slice(), [Said::Snapshot(s)] if s.seq == 2));
    let out = desk.call(1, call(1, "bump", args([("by", Value::Int(2))]), None), 30);
    for c in [1, 2] {
        assert!(
            matches!(said_to(&out, c).as_slice(), [Said::Applied(a)] if matches!(a.order, Order::Ordered { seq: 3, at: 30, .. })),
            "{out:?}"
        );
    }
    let out = desk.call(1, call(2, "point", args([("at", Value::Int(9))]), Some(1)), 40);
    assert!(said_to(&out, 1).is_empty(), "not echoed to its author: {out:?}");
    assert!(matches!(said_to(&out, 2).as_slice(), [Said::Applied(a)] if a.order == Order::Ephemeral { counter: 1 }));
    let held = &desk.instances[&room()];
    assert_eq!(held.value.field("pointer").field("phone"), Value::Int(9));
    let out = desk.part(1, 50);
    assert!(matches!(said_to(&out, 2).as_slice(), [Said::Applied(a)] if a.transition == "on_leave"));
    let held = &desk.instances[&room()];
    assert_eq!(
        held.value.field("pointer"),
        Value::from(BTreeMap::<String, Value>::new()),
        "its pointer is gone with it"
    );
    assert!(!matches!(held.value.field("devices"), Value::Struct(m) if m.contains_key("phone")));
}

/// An ephemeral call whose counter is not above the last its device's
/// took is nothing, at the server and at a replica. Falsified by taking
/// any counter: the stale pointer won, at the server and — falsified on its
/// own — at the replica.
#[test]
fn a_stale_ephemeral_counter_is_ignored() {
    let mut desk = Desk::new(runner(false));
    desk.hello(1, alice("phone"), decl("phone"), 0);
    let first = desk.hello(2, alice("laptop"), decl("laptop"), 0);
    let mut laptop = Watch::new(runner(false), Some(decl("laptop")));
    for s in said_to(&first, 2) {
        laptop.heard(s);
    }
    let fresh = desk.call(1, call(1, "point", args([("at", Value::Int(5))]), Some(10)), 100);
    let stale = desk.call(1, call(2, "point", args([("at", Value::Int(1))]), Some(9)), 200);
    assert!(stale.is_empty(), "the server drops it: {stale:?}");
    assert_eq!(desk.instances[&room()].value.field("pointer").field("phone"), Value::Int(5));
    let mut late = said_to(&fresh, 2);
    let mut old = late[0].clone();
    if let Said::Applied(a) = &mut old {
        a.order = Order::Ephemeral { counter: 9 };
        a.input = args([("at", Value::Int(1))]);
    }
    late.push(old);
    for s in late {
        laptop.heard(s);
    }
    assert_eq!(laptop.view(&room()).unwrap().field("pointer").field("phone"), Value::Int(5));
}

/// A client's own call is on its view at once, pending, and replaced by
/// the server's confirmed result, which the other device reaches through
/// the same frames; a call the server refuses leaves it. Falsified by
/// keeping a confirmed call pending: it was still pending under its own
/// confirmed result.
#[test]
fn a_pending_call_is_replaced_by_the_confirmed_one() {
    let who = Ctx::new("alice", "phone-login");
    let mut desk = Desk::new(runner(true));
    let mut phone = Watch::new(runner(true), Some(decl("phone")));
    let mut laptop = Watch::new(runner(false), Some(decl("laptop")));
    let deliver = |w: &mut Watch, out: &[(i64, Said)], c: i64| {
        for s in said_to(out, c) {
            assert!(w.heard(s).is_empty(), "no resync asked");
        }
    };
    let out = desk.hello(1, alice("phone"), decl("phone"), 0);
    deliver(&mut phone, &out, 1);
    let out = desk.hello(2, alice("laptop"), decl("laptop"), 0);
    deliver(&mut phone, &out, 1);
    deliver(&mut laptop, &out, 2);
    let called = phone.call(&who, &room(), "bump", &args([("by", Value::Int(4))]), [1; 16], 5, 0).unwrap();
    assert_eq!(called.kind, Kind::Ordered);
    assert_eq!(phone.view(&room()).unwrap().field("count"), Value::Int(4), "at once");
    assert_eq!(phone.instances[&room()].pending.len(), 1);
    let out = desk.call(1, called.call, 7);
    deliver(&mut phone, &out, 1);
    deliver(&mut laptop, &out, 2);
    for w in [&phone, &laptop] {
        let i = &w.instances[&room()];
        assert!(i.pending.is_empty());
        assert_eq!((i.seq, i.view.field("count")), (3, Value::Int(4)));
        assert_eq!(i.view, desk.instances[&room()].value);
    }
    // Refused by the server, though the preview took it: stopped twice.
    let a = phone.call(&who, &room(), "start", &Args::new(), [2; 16], 8, 0).unwrap();
    let b = phone.call(&who, &room(), "stop", &Args::new(), [3; 16], 9, 0).unwrap();
    let c = laptop
        .call(&Ctx::new("alice", "laptop-login"), &room(), "start", &Args::new(), [4; 16], 9, 0)
        .unwrap();
    let out = desk.call(2, c.call, 10);
    deliver(&mut phone, &out, 1);
    let out = desk.call(1, a.call, 11);
    deliver(&mut phone, &out, 1);
    let out = desk.call(1, b.call, 12);
    deliver(&mut phone, &out, 1);
    let stop_again = phone.call(&who, &room(), "stop", &Args::new(), [5; 16], 13, 0);
    assert!(
        matches!(stop_again, Err(Refusal::Refused(ref w)) if w == "not counting"),
        "the preview refuses: {stop_again:?}"
    );
    assert!(phone.take_refused().is_empty());
    assert_eq!(phone.view(&room()), Some(&desk.instances[&room()].value));
}

/// A gap in the sequence, or a state that does not hash to the server's,
/// asks for a snapshot, once. The frame missed is a `drop_at` that leaves
/// the ordered part as it was, so that applying the next one past the gap
/// lands on the server's hash anyway and only the sequence can tell.
/// Falsified by applying a frame past a gap: no resync was asked. (With a
/// missed `bump` instead, the hash asked for one and the test passed with
/// the gap check gone — it was written that way first.)
#[test]
fn a_gap_or_a_wrong_hash_asks_for_a_snapshot() {
    let mut desk = Desk::new(runner(false));
    let out = desk.hello(1, alice("phone"), decl("phone"), 0);
    let mut phone = Watch::new(runner(false), Some(decl("phone")));
    for s in said_to(&out, 1) {
        phone.heard(s);
    }
    let a = desk.call(1, call(1, "drop_at", args([("at", Value::Int(0))]), None), 1);
    let b = desk.call(1, call(2, "bump", args([("by", Value::Int(1))]), None), 2);
    let (Said::Applied(_), Said::Applied(second)) = (said_to(&a, 1)[0].clone(), said_to(&b, 1)[0].clone()) else {
        panic!("two applied")
    };
    assert_eq!(phone.heard(Said::Applied(second.clone())), vec![room()], "a gap");
    assert!(phone.heard(Said::Applied(second.clone())).is_empty(), "asked once");
    let snap = desk.resync(1, &room());
    for s in said_to(&snap, 1) {
        phone.heard(s);
    }
    assert_eq!(phone.instances[&room()].seq, 3);
    let mut wrong = second;
    if let Order::Ordered { seq, hash, .. } = &mut wrong.order {
        *seq = 4;
        hash[0] ^= 1;
    }
    assert_eq!(phone.heard(Said::Applied(wrong)), vec![room()], "a hash that is not the server's");
}

/// An ephemeral relay never overtakes an ordered frame: a pointer coalesced
/// behind the rate limit goes out before the next ordered call on the
/// instance is told. And faster than thirty a second, only the newest
/// pointer goes. Falsified by not flushing before sequencing: the bump went
/// out alone, the pointer left waiting for a tick after it.
#[test]
fn an_ephemeral_frame_never_reorders_around_an_ordered_one() {
    let mut desk = Desk::new(runner(false));
    desk.hello(1, alice("phone"), decl("phone"), 0);
    desk.hello(2, alice("laptop"), decl("laptop"), 0);
    let first = desk.call(1, call(1, "point", args([("at", Value::Int(1))]), Some(1)), 1000);
    assert_eq!(said_to(&first, 2).len(), 1);
    // Within the window: held, and the second replaces the first.
    assert!(desk.call(1, call(2, "point", args([("at", Value::Int(2))]), Some(2)), 1005).is_empty());
    assert!(desk.call(1, call(3, "point", args([("at", Value::Int(3))]), Some(3)), 1010).is_empty());
    assert_eq!(desk.deadline(), Some(1000 + ark::live::state::RELAY_EVERY));
    let out = desk.call(1, call(4, "bump", args([("by", Value::Int(1))]), None), 1012);
    let kinds: Vec<String> = said_to(&out, 2)
        .iter()
        .map(|s| match s {
            Said::Applied(a) => format!("{} {}", a.transition, a.input.get("at").map_or(String::new(), |v| format!("{v:?}"))),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(kinds, ["point Int(3)", "bump "]);
    assert_eq!(desk.deadline(), None);
    // And a pointer due by the clock goes on its tick.
    desk.call(1, call(5, "point", args([("at", Value::Int(4))]), Some(4)), 1020);
    assert!(desk.tick(1030).is_empty());
    assert_eq!(said_to(&desk.tick(1012 + 40), 2).len(), 1);
}

/// An ill-typed input to an ephemeral transition, and an ephemeral call
/// without its counter, are refused to their author. (A slot over its size
/// is `eval/live-room` and `ark-client`'s test.) Falsified by taking a call
/// with no counter as counter 0: it was relayed rather than refused.
#[test]
fn an_ill_typed_ephemeral_call_is_refused() {
    let mut desk = Desk::new(runner(false));
    desk.hello(1, alice("phone"), decl("phone"), 0);
    let out = desk.call(1, call(1, "point", args([("at", Value::text("here"))]), Some(1)), 1);
    assert!(
        matches!(said_to(&out, 1).as_slice(), [Said::Refused(r)] if r.reason == "at: expected an int, found text"),
        "{out:?}"
    );
    let out = desk.call(1, call(2, "point", args([("at", Value::Int(1))]), None), 1);
    assert!(matches!(said_to(&out, 1).as_slice(), [Said::Refused(r)] if r.reason.contains("counter")));
}

/// A server restarted from what it kept: the ordered part at its sequence,
/// every pointer empty, and `on_restore` run — the room no longer counting.
/// Falsified by not running the hook: the restored room was still counting.
#[test]
fn a_restored_instance_runs_its_hook() {
    let r = runner(false);
    let s = r.init("room").unwrap();
    let s = r.run("room", "start", &s, &lctx("phone", 1), &Args::new()).unwrap();
    assert_eq!(s.field("mode").field("t"), Value::text("counting"));
    let mut desk = Desk::new(r);
    desk.restore(room(), 7, s, 99);
    let h = &desk.instances[&room()];
    assert_eq!((h.seq, h.value.field("mode").field("t")), (8, Value::text("idle")));
}

/// §15 The seeded fleet over the room: three devices of one account call
/// every transition — ordered and ephemeral — through a network that
/// reorders, duplicates and drops, partitioned and healed at random; after
/// a settle every client holds every instance at the server's sequence and
/// state, ephemeral slots included, with nothing pending and every preview
/// replaced by what was confirmed. And every client's life replays from its
/// trace into a client that interprets what the fleet ran natively.
/// Falsified by not saying pending calls again on a connection
/// (`Watch::connected` returning nothing): seed 0 never settled.
#[test]
fn a_seeded_fleet_converges_on_every_instance() {
    use ark::sim::{lcg, Sim};
    let m = module();
    let bodies = ark::hash::closures(&m);
    type Draw = fn(u64) -> Args;
    let calls: [(&str, Draw); 5] = [
        ("bump", |r| args([("by", Value::Int((r % 12) as i64))])),
        ("start", |_| Args::new()),
        ("stop", |_| Args::new()),
        ("point", |r| args([("at", Value::Int((r % 50) as i64))])),
        ("drop_at", |r| args([("at", Value::Int((r % 7) as i64))])),
    ];
    for seed in 0..12u64 {
        let mut sim = Sim::live_account(m.schema.clone(), bodies.clone(), runner(seed % 2 == 0), "alice", 3, seed, true);
        let mut r = lcg(seed ^ 0x5eed);
        let mut counters = [0i64; 3];
        for k in 0..160u64 {
            r = lcg(r);
            let peer = ((r >> 33) % 3) as i64;
            match (r >> 40) % 10 {
                0 => sim.partition(peer),
                1 => sim.heal(peer),
                2..=5 => {
                    let (t, input) = calls[((r >> 20) % 5) as usize];
                    counters[peer as usize] += 1;
                    let mut id = [0u8; 16];
                    id[..8].copy_from_slice(&k.to_be_bytes());
                    id[15] = peer as u8;
                    sim.call(peer, "room", t, &input(r >> 8), id, counters[peer as usize]);
                }
                _ => {
                    for _ in 0..3 {
                        sim.step();
                    }
                }
            }
        }
        sim.try_settle().unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        sim.live_converged().unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        assert!(sim.server.desk.instances[&room()].seq > 3, "seed {seed}: the room moved");
        let traces = &sim.recording.as_ref().expect("recorded").traces;
        for (i, t) in traces {
            ark::trace::replay(&m, &t.steps).unwrap_or_else(|e| panic!("seed {seed}, client {i}: {e}"));
        }
    }
}
