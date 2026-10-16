//! Live state (`docs/plan-live.md`): the demo's room as vectors, every one
//! marked `rust-for-now` in the manifest — Rust runs them, and Kotlin and
//! Swift skip them until they hold live state (phase 1b).
//!
//! - `module/live.json`: the demo with its room, its type section and its
//!   state on the wire;
//! - `verify/live-room.json`, and `verify/falsify/live-ordered-reads-a-pointer.json`,
//!   a room whose ordered transition reads an ephemeral field;
//! - `eval/live-room.json`: every transition of the room, its kind, and a
//!   life of it step by step — the state after each, its hash with the
//!   ephemeral fields left out, or the refusal — and its `falsify/`, whose
//!   hash is of the whole value;
//! - `protocol/`: every live frame — a `hello` declaring a device, a `call`
//!   ordered and ephemeral, `resync`, `applied` ordered, ephemeral and of a
//!   hook, `refused`, `live_snapshot` — and a `falsify/`;
//! - `trace/live/`: devices of one account through a scripted fleet, their
//!   inputs and outputs, and a `falsify/`. A directory of their own because
//!   the client runtimes' storage tests replay every file of `trace/` and
//!   `trace/falsify/` without reading the manifest, and these are not theirs
//!   yet.

use std::collections::BTreeMap;

use ark::canon::encode;
use ark::eval::{Args, EvalFault, LiveCtx};
use ark::hash::{live_closure, live_hash, module_hash, state_type_hash};
use ark::ir::{module_value, ON_JOIN, ON_LEAVE, ON_RESTORE};
use ark::live::state::{Applied, Call, DeviceDecl, Instance, Order, Refused, Runner, Snapshot};
use ark::protocol::{ClientMsg, Mode, ServerMsg, Subscription};
use ark::store::Refusal;
use ark::value::{hex, Value};

use super::demo::{self, id_n};
use super::json::{json, obj, quoted};
use super::{claim, Out};

fn args<const N: usize>(pairs: [(&str, Value); N]) -> Args {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn lctx_value(c: &LiveCtx) -> Value {
    Value::record(vec![
        ("user", Value::text(c.user.clone())),
        ("session", Value::text(c.session.clone())),
        ("roles", Value::from(c.roles.iter().map(|r| Value::text(r.clone())).collect::<Vec<_>>())),
        ("device", Value::text(c.device.clone())),
        ("now", Value::Int(c.now)),
    ])
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

pub fn live(out: &Out) {
    let m = demo::live_module();
    module(out, &m);
    verify(out, &m);
    eval(out, &m);
    protocol(out);
    traces(out, &m);
}

fn module(out: &Out, m: &ark::ir::Module) {
    let v = module_value(m);
    let bytes = encode(&v);
    claim("the live module verifies", ark::verify::verify(m).is_ok());
    claim(
        "the demo's module has no live state",
        demo::module().types.is_empty() && demo::module().states.is_empty(),
    );
    out.write_live(
        "module/live.json",
        &obj(&[
            ("module", json(&v)),
            ("bytes", quoted(&hex(&bytes))),
            ("hash", quoted(&hex(&module_hash(m)))),
        ]),
    );
}

fn verify(out: &Out, m: &ark::ir::Module) {
    out.write_live(
        "verify/live-room.json",
        &obj(&[("module", json(&module_value(m))), ("verifies", "true".into())]),
    );
    // The room with `bump` reading how many pointers there are: an ordered
    // transition reading an ephemeral field, which no replica can agree on.
    let mut broken = m.clone();
    let st = &mut broken.states[0];
    let bump = st.transitions.iter_mut().find(|t| t.name == "bump").expect("bump");
    let pointers = ark::ir::Expr::Std(
        ark::ir::StdFn::Len,
        vec![ark::ir::Expr::Slots(Box::new(ark::ir::Expr::Field(
            Box::new(ark::ir::Expr::LiveState),
            "pointer".into(),
        )))],
    );
    if let Some(ark::ir::Stmt::Return(Some(ark::ir::Expr::Struct(fs)))) = bump.body.last_mut() {
        fs.insert("count".into(), pointers);
    }
    let refused = ark::verify::verify(&broken).err().unwrap_or_default();
    claim(
        &format!("an ordered read of an ephemeral field is refused: {refused:?}"),
        refused.iter().any(|e| matches!(e, ark::verify::VerifyError::InTransition(_, t, ark::verify::Complaint::OrderedReadsEphemeral(f)) if t == "bump" && f == "pointer")),
    );
    out.write_live(
        "verify/falsify/live-ordered-reads-a-pointer.json",
        &obj(&[
            ("module", json(&module_value(&broken))),
            ("verifies", "true".into()),
            ("expect", quoted("fail")),
        ]),
    );
}

fn eval(out: &Out, m: &ark::ir::Module) {
    let st = &m.states[0];
    let c = live_closure(m, st);
    let r = Runner::of_module(m);
    let kinds: BTreeMap<String, Value> = st
        .transitions
        .iter()
        .map(|t| (t.name.clone(), Value::text(c.kind(&t.name).name())))
        .chain([ON_JOIN, ON_LEAVE, ON_RESTORE].map(|h| (h.to_string(), Value::text(c.kind(h).name()))))
        .collect();
    claim(
        "point and label are the room's ephemeral transitions",
        kinds
            .iter()
            .filter(|(_, k)| **k == Value::text("ephemeral"))
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>()
            == ["label", "point"],
    );
    let phone = DeviceDecl::new("phone", "phone", "Phone", true).record("alice");
    let laptop = DeviceDecl::new("laptop", "computer", "Laptop", false).record("alice");
    let long = "x".repeat(2000);
    let steps: Vec<(&str, LiveCtx, Args)> = vec![
        (ON_JOIN, lctx("phone", 1000), args([("device", phone.clone())])),
        (ON_JOIN, lctx("laptop", 1001), args([("device", laptop.clone())])),
        ("bump", lctx("phone", 1002), args([("by", Value::Int(3))])),
        ("bump", lctx("phone", 1003), args([("by", Value::Int(11))])),
        ("stop", lctx("laptop", 1004), Args::new()),
        ("start", lctx("laptop", 1005), Args::new()),
        ("point", lctx("phone", 1006), args([("at", Value::Int(7))])),
        ("point", lctx("laptop", 1007), args([("at", Value::Int(2))])),
        ("label", lctx("phone", 1008), args([("text", Value::text("here"))])),
        ("label", lctx("laptop", 1009), args([("text", Value::text(long))])),
        ("point", lctx("phone", 1010), args([("at", Value::text("seven"))])),
        ("drop_at", lctx("phone", 1011), args([("at", Value::Int(4))])),
        ("stop", lctx("phone", 1012), Args::new()),
        (ON_LEAVE, lctx("laptop", 1013), args([("device", laptop)])),
        ("nonesuch", lctx("phone", 1014), Args::new()),
        (
            ON_RESTORE,
            LiveCtx {
                now: 1015,
                ..LiveCtx::default()
            },
            Args::new(),
        ),
    ];
    let init = r.init("room").expect("an init");
    let mut s = init.clone();
    let mut written = vec![];
    let mut whole_hashes = vec![];
    for (t, ctx, input) in &steps {
        let mut fields = vec![
            ("transition", Value::text(*t)),
            ("ctx", lctx_value(ctx)),
            ("input", Value::from(input.clone())),
        ];
        match r.run("room", t, &s, ctx, input) {
            Ok(next) => {
                s = next;
                fields.push(("state", s.clone()));
                fields.push(("hash", Value::text(hex(&r.hash("room", &s)))));
                whole_hashes.push(Some(hex(&ark::sha256::sha256(&encode(&s)))));
            }
            Err(EvalFault::Verdict(Refusal::Refused(why))) => {
                fields.push(("refused", Value::text(why)));
                whole_hashes.push(None);
            }
            Err(other) => panic!("live eval {t}: {other:?}"),
        }
        written.push(Value::record(fields));
    }
    claim(
        "the room's life refuses five times",
        written.iter().filter(|w| w.as_struct().contains_key("refused")).count() == 5,
    );
    claim("the last state is restored idle", s.field("mode").field("t") == Value::text("idle"));
    let parts = |steps: &[Value]| -> Vec<(&str, String)> {
        vec![
            ("module", json(&module_value(m))),
            ("live", quoted("room")),
            ("live_hash", quoted(&hex(&live_hash(&c)))),
            ("type_hash", quoted(&hex(&state_type_hash(&c)))),
            ("kinds", json(&Value::from(kinds.clone()))),
            ("init", json(&init)),
            ("init_hash", quoted(&hex(&r.hash("room", &init)))),
            ("steps", format!("[\n{}\n]", steps.iter().map(json).collect::<Vec<_>>().join(",\n"))),
        ]
    };
    out.write_live("eval/live-room.json", &obj(&parts(&written)));
    // The same life with every hash taken of the whole value, pointers and
    // labels in it: a runner must fail it.
    let wrong: Vec<Value> = written
        .iter()
        .zip(&whole_hashes)
        .map(|(w, h)| match h {
            Some(h) => {
                let mut f = w.as_struct().clone();
                f.insert("hash".into(), Value::text(h.clone()));
                Value::from(f)
            }
            None => w.clone(),
        })
        .collect();
    claim("a whole-value hash differs somewhere", wrong != written);
    let mut p = parts(&wrong);
    p.push(("expect", quoted("fail")));
    out.write_live("eval/falsify/live-room-hashing-every-field.json", &obj(&p));
}

fn protocol(out: &Out) {
    let room = Instance::new("room", "alice");
    let hello = ClientMsg::Hello {
        sub: Subscription {
            partial: false,
            since: 4,
            mode: Mode::Whole,
            log_id: None,
        },
        token: Some("tok".into()),
        spec: ark::ir::SPEC_VERSION,
        device: Some(DeviceDecl::new("phone-login", "phone", "Phone", true)),
    };
    let input = args([("by", Value::Int(2))]);
    let client: Vec<(&str, ClientMsg)> = vec![
        ("hello-device", hello),
        (
            "call",
            ClientMsg::Call(Call {
                instance: room.clone(),
                transition: "bump".into(),
                input: input.clone(),
                id: id_n(7),
                counter: None,
            }),
        ),
        (
            "call-ephemeral",
            ClientMsg::Call(Call {
                instance: room.clone(),
                transition: "point".into(),
                input: args([("at", Value::Int(3))]),
                id: id_n(8),
                counter: Some(41),
            }),
        ),
        ("resync", ClientMsg::Resync(room.clone())),
    ];
    for (name, f) in client {
        let v = f.to_value();
        claim(&format!("client-{name} reads back"), ClientMsg::from_value(&v).as_ref() == Ok(&f));
        out.write_live(
            &format!("protocol/client-{name}.json"),
            &obj(&[("frame", json(&v)), ("bytes", quoted(&hex(&encode(&v))))]),
        );
    }
    let hash = ark::sha256::sha256(b"the room after");
    let ordered = Applied {
        instance: room.clone(),
        transition: "bump".into(),
        input,
        device: "phone-login".into(),
        order: Order::Ordered {
            seq: 5,
            id: Some(id_n(7)),
            user: "alice".into(),
            session: "phone-login".into(),
            roles: ["listener".to_string()].into(),
            at: 1_760_000_000_000,
            hash: hash.clone(),
        },
    };
    let joined = Applied {
        instance: room.clone(),
        transition: ON_JOIN.into(),
        input: args([("device", DeviceDecl::new("laptop-login", "computer", "Laptop", false).record("alice"))]),
        device: "laptop-login".into(),
        order: Order::Ordered {
            seq: 6,
            id: None,
            user: "alice".into(),
            session: "laptop-login".into(),
            roles: Default::default(),
            at: 1_760_000_000_500,
            hash,
        },
    };
    let ephemeral = Applied {
        instance: room.clone(),
        transition: "point".into(),
        input: args([("at", Value::Int(3))]),
        device: "phone-login".into(),
        order: Order::Ephemeral { counter: 41 },
    };
    let server: Vec<(&str, ServerMsg)> = vec![
        ("applied", ServerMsg::Applied(ordered.clone())),
        ("applied-hook", ServerMsg::Applied(joined)),
        ("applied-ephemeral", ServerMsg::Applied(ephemeral.clone())),
        (
            "refused",
            ServerMsg::Refused(Refused {
                instance: room.clone(),
                id: id_n(9),
                reason: "not counting".into(),
            }),
        ),
        (
            "live-snapshot",
            ServerMsg::LiveSnapshot(Snapshot {
                instance: room.clone(),
                seq: 6,
                value: Value::record(vec![
                    ("count", Value::Int(2)),
                    ("pointer", Value::record(vec![("phone-login", Value::Int(3))])),
                ]),
                counters: [("phone-login".to_string(), 41)].into(),
            }),
        ),
    ];
    for (name, f) in server {
        let v = f.to_value();
        claim(&format!("server-{name} reads back"), ServerMsg::from_value(&v).as_ref() == Ok(&f));
        out.write_live(
            &format!("protocol/server-{name}.json"),
            &obj(&[("frame", json(&v)), ("bytes", quoted(&hex(&encode(&v))))]),
        );
    }
    // An ephemeral applied whose bytes carry a sequence: no frame is both.
    let v = ServerMsg::Applied(ephemeral).to_value();
    let mut both = v.as_struct().clone();
    both.insert("seq".into(), Value::Int(5));
    let both = Value::from(both);
    claim("an applied with a seq and a counter is refused", ServerMsg::from_value(&both).is_err());
    out.write_live(
        "protocol/falsify/server-applied-ephemeral-and-ordered.json",
        &obj(&[("frame", json(&v)), ("bytes", quoted(&hex(&encode(&both)))), ("expect", quoted("fail"))]),
    );
}

fn traces(out: &Out, m: &ark::ir::Module) {
    use ark::sim::Sim;
    let bodies = ark::hash::closures(m);
    let mut r = Runner::of_module(m);
    r.hold(demo::live_natives());
    let mut sim = Sim::live_account(m.schema.clone(), bodies, r, "alice", 2, 7, true);
    let id = |k: u8| id_n(0x40 + k);
    // Two devices: the first bumps, points, and labels; the second, dark,
    // calls while partitioned, and its calls are sequenced on the heal —
    // one of them refused there, the room being stopped by then.
    sim.call(0, "room", "start", &Args::new(), id(1), 0);
    sim.settle();
    sim.call(0, "room", "point", &args([("at", Value::Int(5))]), id(2), 1);
    sim.call(0, "room", "label", &args([("text", Value::text("hi"))]), id(3), 2);
    sim.partition(1);
    sim.call(1, "room", "bump", &args([("by", Value::Int(2))]), id(4), 0);
    sim.call(1, "room", "stop", &Args::new(), id(5), 0);
    sim.call(0, "room", "stop", &Args::new(), id(6), 0);
    sim.call(0, "room", "bump", &args([("by", Value::Int(1))]), id(7), 0);
    for _ in 0..12 {
        sim.step();
    }
    sim.heal(1);
    sim.settle();
    claim("the live fleet converged", sim.live_converged().is_ok());
    let rec = sim.recording.take().expect("recorded");
    let from = |i: i64| format!("ark-vectors live, a fleet of two devices of one account, seed 7, client {i}");
    std::fs::create_dir_all(out.root.join("trace/live/falsify")).expect("creating trace/live/falsify");
    for (i, name) in [(0i64, "calls-confirmed"), (1, "partitioned-calls-sequenced")] {
        let t = &rec.traces[&i];
        let replayed = ark::trace::replay(m, &t.steps);
        claim(&format!("trace/live/{name}.json replays: {replayed:?}"), replayed.is_ok());
        out.write_live(&format!("trace/live/{name}.json"), &ark::trace::write(m, &from(i), &t.steps, &[]));
    }
    // The partitioned device's life with the view of its first live output
    // changed: a runner must fail it.
    let mut steps = rec.traces[&1].steps.clone();
    let (_, o) = steps
        .iter_mut()
        .find(|(_, o)| o.as_struct().contains_key("live"))
        .expect("a step that says its live state");
    let mut f = o.as_struct().clone();
    f.insert("live".into(), Value::from(vec![Value::text("not the room")]));
    *o = Value::from(f);
    claim("trace/live/falsify fails to replay", ark::trace::replay(m, &steps).is_err());
    out.write_live(
        "trace/live/falsify/the-view-changed.json",
        &ark::trace::write(m, &from(1), &steps, &[("expect", "\"fail\"".into())]),
    );
}
