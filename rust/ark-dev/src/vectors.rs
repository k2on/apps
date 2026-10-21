//! The vectors D1 writes under `spec/vectors/` (`spec/RUNNERS.md` says
//! how each is run): `protocol/`, every frame as its bytes and its JSON;
//! `trace/`, every client of the scripted sessions;
//! `rebase/`, `fleet-seed-7` and the sessions the fuzzer found a bug with.
//! `ARK_WRITE_VECTORS` set writes them; otherwise each must be what is on
//! disk.

use std::path::PathBuf;
use std::sync::Arc;

use ark::format::Encode;
use ark::hash::hex;
use ark::log::{EntryId, Origin};
use ark::peer::Who;
use ark::row::Id;
use ark_peer::{Config, Live, Module};
use serde_json::{json, Value};

use crate::demo::{arrival, live, room, AddToPlaylist, CreatePlaylist, Point, Rename};
use crate::sim::{who, Net, Op};

fn dir(d: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/vectors").join(d)
}

/// Whether this build's value format is the vectors': postcard, the
/// default. A build of `values-json` runs everything but what holds bytes
/// to them, and says so.
pub fn held() -> bool {
    let held = ark::format::NAME == "postcard";
    if !held {
        eprintln!("spec/vectors are postcard's; this build writes {}: not held", ark::format::NAME);
    }
    held
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
    Config::of(Arc::new(crate::demo::demo()))
}

fn create(i: i64, name: &str) -> Op {
    Op::Mutate(i, "demo.create_playlist".into(), CreatePlaylist { name: name.into() }.to_bytes())
}

fn add(i: i64, owner: &str, t: &str) -> Op {
    let input = AddToPlaylist {
        playlist_id: Id::new(EntryId::of(Origin::named(owner), 1).draw(0)),
        track_id: t.into(),
    };
    Op::Mutate(i, "demo.add_to_playlist".into(), input.to_bytes())
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
    let mut net = Net::new(demo(), |_| demo(), 3, seed);
    net.run(&create(0, "Fleet"));
    net.settle().expect("the playlist settles");
    for op in fleet_script() {
        net.run(&op);
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
    let two = |seed| Net::new(demo(), |_| demo(), 2, seed);
    let mut keep = demo();
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
    let mut hub = demo();
    hub.live = Some(Live {
        on_join: Some("room.on_join".into()),
        on_leave: Some("room.on_leave".into()),
        arrive: Arc::new(arrival),
        keep: Some(4),
        ..Live::new(Module::of(&room))
    });
    let device = |name: &str| Config {
        live: Some(live(name, 100)),
        ..demo()
    };
    let point = |i, x| Op::Call(i, "room.point".into(), Point { x }.to_bytes());
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
                Net::of(demo(), vec![(anon, demo()), (who(1), demo())], 4),
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
                        Op::Call(0, "room.rename".into(), Rename { name: "kitchen".into() }.to_bytes()),
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
        Op::Mutate(i, f, input) => json!({"client": i, "fn": f, "input": hex(input), "t": "mutate"}),
        Op::Partition(i) => json!({"client": i, "t": "partition"}),
        Op::Heal(i) => json!({"client": i, "t": "heal"}),
        Op::Step => json!({"t": "step"}),
        other => json!({"t": format!("{other:?}")}),
    }
}
