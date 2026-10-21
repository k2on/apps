//! The vectors D1 writes under `spec/vectors/` (`spec/RUNNERS.md` says
//! how each is run): `protocol/`, every frame as its bytes and as JSON read
//! under the frame type; `trace/`, every client of the scripted sessions;
//! `rebase/`, `fleet-seed-7` and the sessions the fuzzer found a bug with.
//! `ARK_WRITE_VECTORS` set writes them; otherwise each must be what is on
//! disk.

use std::path::PathBuf;
use std::sync::Arc;

use ark::codec::{bytes_enc, int_enc, put_bytes, put_int, put_list, Reader};
use ark::hash::{hex, unhex};
use ark::log::{EntryId, Origin};
use ark::peer::Who;
use ark::ty::Ty;
use ark_peer::{Config, Live, Module};
use serde_json::{json, Value};

use crate::demo::{arrival, domain, live, room};
use crate::sim::{who, Net, Op};

/// A value of `ty` as JSON: an `Int` a number, `Bytes` hex, a list or a
/// tuple an array, a sum `{"variant", "fields"}`.
pub fn json_of(ty: &Ty, r: &mut Reader) -> Result<Value, String> {
    Ok(match ty {
        Ty::Int => json!(r.int().map_err(|e| e.0)?),
        Ty::Bytes => json!(hex(&r.bytes().map_err(|e| e.0)?)),
        Ty::List(t) => Value::Array(r.list(|r| json_of(t, r).map_err(ark::Refusal)).map_err(|e| e.0)?),
        Ty::Tuple(ts) => Value::Array(ts.iter().map(|t| json_of(t, r)).collect::<Result<_, _>>()?),
        Ty::Sum(vs) => {
            let i = r.variant(vs.len()).map_err(|e| e.0)?;
            let fields: Vec<Value> = vs[i as usize].iter().map(|t| json_of(t, r)).collect::<Result<_, _>>()?;
            json!({"fields": fields, "variant": i})
        }
        Ty::Named(n) => return Err(format!("{n}: a frame's type is structural")),
    })
}

/// The bytes of a value of `ty` written as [`json_of`] writes it.
pub fn bytes_of(ty: &Ty, v: &Value, out: &mut Vec<u8>) -> Result<(), String> {
    let bad = || format!("not a value of {ty:?}: {v}");
    match ty {
        Ty::Int => put_int(out, v.as_i64().ok_or_else(bad)?),
        Ty::Bytes => put_bytes(out, &v.as_str().and_then(unhex).ok_or_else(bad)?),
        Ty::List(t) => {
            let xs = v.as_array().ok_or_else(bad)?;
            let mut err = Ok(());
            put_list(out, xs, |o, x| err = err.clone().and(bytes_of(t, x, o)));
            err?
        }
        Ty::Tuple(ts) => {
            let xs = v.as_array().filter(|xs| xs.len() == ts.len()).ok_or_else(bad)?;
            ts.iter().zip(xs).try_for_each(|(t, x)| bytes_of(t, x, out))?
        }
        Ty::Sum(vs) => {
            let i = v["variant"].as_u64().filter(|i| (*i as usize) < vs.len()).ok_or_else(bad)? as usize;
            out.push(i as u8);
            let xs = v["fields"].as_array().filter(|xs| xs.len() == vs[i].len()).ok_or_else(bad)?;
            vs[i].iter().zip(xs).try_for_each(|(t, x)| bytes_of(t, x, out))?
        }
        Ty::Named(_) => return Err(bad()),
    }
    Ok(())
}

fn dir(d: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/vectors").join(d)
}

/// A vector written, or held to what is on disk.
pub fn vector(d: &str, name: &str, text: &str) -> Result<(), String> {
    let path = dir(d).join(name);
    if std::env::var_os("ARK_WRITE_VECTORS").is_some() {
        std::fs::create_dir_all(dir(d)).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| e.to_string())?;
    }
    let on_disk = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    (on_disk == text)
        .then_some(())
        .ok_or(format!("{d}/{name} moved; ARK_WRITE_VECTORS=1 writes it again"))
}

/// Every vector file of a directory, by name.
pub fn on_disk(d: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = std::fs::read_dir(dir(d))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .map(|e| {
            (
                e.file_name().to_string_lossy().into(),
                std::fs::read_to_string(e.path()).unwrap_or_default(),
            )
        })
        .collect();
    out.sort();
    out
}

// * The scripted sessions

fn demo() -> Config {
    Config {
        live: None,
        ..Config::new(Module::of(&domain()))
    }
}

fn pair(id: &[u8], t: &str) -> Vec<u8> {
    [bytes_enc(id), bytes_enc(t.as_bytes())].concat()
}

fn create(i: i64, name: &str) -> Op {
    Op::Mutate(i, domain().get("demo.create_playlist"), bytes_enc(name.as_bytes()))
}

fn add(i: i64, owner: &str, t: &str) -> Op {
    Op::Mutate(
        i,
        domain().get("demo.add_to_playlist"),
        pair(&EntryId::of(Origin::named(owner), 1).draw(0), t),
    )
}

/// `fleet-seed-7`'s script after its playlist (v4's, move for move).
pub fn fleet_script() -> Vec<Op> {
    let mut s = vec![];
    for k in 1..=4 {
        s.extend((0..=2).map(|i| add(i, "peer-0-session", &format!("{i}-{k}"))));
    }
    s.push(Op::Partition(2));
    s.extend((5..=7).map(|k| add(2, "peer-0-session", &format!("2-{k}"))));
    s.extend([add(0, "peer-0-session", "0-8"), add(1, "peer-0-session", "1-9")]);
    s.extend((0..60).map(|_| Op::Step));
    s.push(Op::Partition(0));
    s.extend([add(1, "peer-0-session", "1-10"), add(0, "peer-0-session", "0-11")]);
    s.extend((0..60).map(|_| Op::Step));
    s.extend([Op::Heal(0), Op::Heal(2)]);
    s.extend((0..80).map(|_| Op::Step));
    s
}

/// `fleet-seed-7` (or another seed) run: the playlist, settled, the
/// script, settled.
pub fn fleet(seed: u64) -> Net {
    let mut net = Net::new(Config::new(Module::of(&domain())), |_| demo(), 3, seed);
    net.run(&create(0, "Fleet"));
    net.settle().expect("the playlist settles");
    for op in fleet_script() {
        net.run(&op);
    }
    net.settle().expect("the fleet settles");
    net
}

/// [`fleet`] over the demo in plain Rust (§1.16): the same script, each
/// function called by its name's hash.
pub fn fleet_native(seed: u64) -> Net {
    let d = crate::native::demo();
    let config = || Config::new(Module::native(&d));
    let lang = domain();
    let rename = |op: Op| match op {
        Op::Mutate(i, f, input) => {
            let (name, _) = lang.module.bindings.iter().find(|(_, h)| *h == f).expect("a demo function");
            Op::Mutate(i, ark::domain::Domain::hash_of(name), input)
        }
        op => op,
    };
    let mut net = Net::new(config(), |_| config(), 3, seed);
    net.run(&rename(create(0, "Fleet")));
    net.settle().expect("the playlist settles");
    for op in fleet_script() {
        net.run(&rename(op));
    }
    net.settle().expect("the fleet settles");
    net
}

/// The scripted sessions whose clients are the `trace/` vectors.
pub fn sessions() -> Vec<(&'static str, Net)> {
    let room = room();
    let p = "peer-0-session";
    let run = |mut net: Net, ops: Vec<Op>| {
        for op in ops {
            net.run(&op);
        }
        net.settle().expect("a scripted session settles");
        net
    };
    let two = |seed| Net::new(Config::new(Module::of(&domain())), |_| demo(), 2, seed);
    let mut keep = Config::new(Module::of(&domain()));
    keep.keep = Some(1);
    let anon = Who {
        actor: String::new(),
        session: "anonymous".into(),
        roles: vec![],
    };
    let alice = Who {
        actor: "alice".into(),
        session: "alice-session".into(),
        roles: vec!["listener".into()],
    };
    let mut hub = Config::new(Module::of(&domain()));
    hub.live = Some(Live {
        on_join: Some(room.get("room.on_join")),
        on_leave: Some(room.get("room.on_leave")),
        arrive: Arc::new(arrival),
        keep: Some(4),
        ..Live::new(Module::of(&room))
    });
    let device = |name: &str| Config {
        live: Some(live(name, 100)),
        ..Config::new(Module::of(&domain()))
    };
    let point = |i, x| Op::Call(i, room.get("room.point"), int_enc(x).to_vec());
    let steps = |n: usize| vec![Op::Step; n];
    vec![
        (
            "the-demo",
            run(two(1), [vec![create(0, "Mix"), add(0, p, "t1"), add(1, p, "t2")], steps(12)].concat()),
        ),
        (
            "a-restart",
            run(
                Net::new(keep, |_| demo(), 2, 2),
                vec![
                    create(0, "Mix"),
                    add(0, p, "t1"),
                    Op::Partition(1),
                    add(0, p, "t2"),
                    add(0, p, "t3"),
                    Op::Restart,
                    Op::Heal(1),
                ],
            ),
        ),
        (
            "a-reopen",
            run(two(3), vec![Op::Partition(0), create(0, "Road"), add(0, p, "t5"), Op::Reopen(0)]),
        ),
        (
            "a-sign-in",
            run(
                Net::of(Config::new(Module::of(&domain())), vec![(anon, demo()), (who(1), demo())], 4),
                vec![Op::Partition(0), create(0, "Mine"), Op::SignIn(0, alice)],
            ),
        ),
        (
            "alone-then-join",
            run(
                two(5),
                vec![Op::Alone(0), create(0, "Road"), add(0, p, "t5"), create(1, "Bob's"), Op::Join(0)],
            ),
        ),
        (
            "the-room",
            run(
                Net::of(hub, vec![(who(0), device("phone")), (who(1), device("laptop"))], 6),
                [
                    vec![
                        Op::Call(0, room.get("room.rename"), bytes_enc(b"kitchen")),
                        point(0, 1),
                        point(0, 2),
                        point(0, 3),
                        Op::Wait(100),
                    ],
                    steps(4),
                    vec![Op::Partition(0)],
                ]
                .concat(),
            ),
        ),
        ("fleet-seed-7", fleet(7)),
    ]
}

/// An op as the rebase vector writes it.
pub fn op_json(op: &Op) -> Value {
    match op {
        Op::Mutate(i, f, input) => json!({"client": i, "fn": f.hex(), "input": hex(input), "t": "mutate"}),
        Op::Partition(i) => json!({"client": i, "t": "partition"}),
        Op::Heal(i) => json!({"client": i, "t": "heal"}),
        Op::Step => json!({"t": "step"}),
        other => json!({"t": format!("{other:?}")}),
    }
}
