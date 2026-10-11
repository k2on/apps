//! Writes kotlin/ark-client/fixtures/rust-peer.json: a Rust
//! `ark_client::Peer` over a `Memory` storage, with seeded autos, alone then
//! joined to an in-process `ark::protocol::Server`, and the whole storage
//! after every step — what a Kotlin peer driven by the same steps must
//! write, byte for byte (`StorageTests`).
//!
//! Not part of any build here. To write the fixture again, put this file
//! at `src/main.rs` of a crate whose Cargo.toml has an empty `[workspace]`
//! table and `ark-client = { path = "<repo>/rust/ark-client",
//! default-features = false }`, copy rust/Cargo.lock beside it, and run
//! `cargo run --offline --release -- rust-peer.json` in `nix develop .#rust`.

use std::collections::BTreeMap;

use ark_client::ark;
use ark_client::storage::{Memory, Storage};
use ark_client::{Args, Autos, Domain, Login, Options, Peer, Value};

use ark::canon;
use ark::store::Store;
use ark::json::{json, obj, quoted};
use ark::live::Silent;
use ark::peer::Authority;
use ark::protocol::{open_access, trusting, ClientMsg, Server, ServerMsg};

fn storage_value(m: &Memory) -> Value {
    let mut out = BTreeMap::new();
    for k in m.keys() {
        out.insert(k.clone(), Value::bytes(Storage::load(m, &k).unwrap().unwrap()));
    }
    Value::from(out)
}

fn id(k: u8) -> Value {
    let mut b = [0u8; 16];
    b[0] = 0xa0;
    b[15] = k;
    Value::Id(b)
}

struct Script {
    steps: Vec<String>,
    mem: Memory,
}

impl Script {
    fn step(&mut self, fields: Vec<(&str, Value)>) {
        let mut fs = fields;
        fs.push(("storage", storage_value(&self.mem)));
        self.steps.push(json(&Value::record(fs)));
    }
}

fn args(pairs: Vec<(&str, Value)>) -> Args {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn main() {
    let out = std::env::args().nth(1).expect("an output path");
    // The demo module of spec/AUTHORING.md Appendix B, interpreted: no native
    // procedures, so the Rust peer runs exactly what a Kotlin one runs.
    let demo = ark_client::demo::domain();
    let module = demo.module().clone();
    let bytes = canon::encode(&ark::ir::module_value(&module));
    let domain = Domain::of(module.clone(), vec![]);
    let mem = Memory::new();
    let mut s = Script { steps: vec![], mem: mem.clone() };

    let open = |opts: Options| Peer::open(domain.clone(), Box::new(mem.clone()), opts).unwrap();
    let alice = Options::alone("alice").with_autos(Autos::seeded(7));
    let mut p = open(alice);
    s.step(vec![
        ("do", Value::text("open")),
        ("user", Value::text("alice")),
        ("session", Value::text("local")),
        ("token", Value::Null),
        ("alone", Value::Bool(true)),
        ("seed", Value::Int(7)),
    ]);
    let mutate = |p: &mut Peer, s: &mut Script, name: &str, a: Args| {
        let r = p.mutate(name, a.clone());
        s.step(vec![
            ("do", Value::text("mutate")),
            ("name", Value::text(name)),
            ("args", Value::from(a)),
            ("refused", Value::Bool(r.is_err())),
        ]);
    };
    mutate(&mut p, &mut s, "create_playlist", args(vec![("name", Value::text("Road"))]));
    let pl = p.store().scan("playlist")[0].get("id").cloned().unwrap();
    for k in 0..6 {
        mutate(
            &mut p,
            &mut s,
            "add_to_playlist",
            args(vec![("playlist_id", pl.clone()), ("track_id", Value::text(format!("t{k}")))]),
        );
    }
    // A refusal: a second playlist of the same name.
    mutate(&mut p, &mut s, "create_playlist", args(vec![("name", Value::text("Road"))]));
    p.persist().unwrap();
    s.step(vec![("do", Value::text("persist"))]);
    drop(p);
    // Reopened alone, from what it left: the local history resumed.
    let mut p = open(Options::alone("alice").with_autos(Autos::seeded(8)));
    s.step(vec![
        ("do", Value::text("open")),
        ("user", Value::text("alice")),
        ("session", Value::text("local")),
        ("token", Value::Null),
        ("alone", Value::Bool(true)),
        ("seed", Value::Int(8)),
    ]);
    mutate(&mut p, &mut s, "create_playlist", args(vec![("name", Value::text("Home"))]));
    // Joined: the local history re-queued as bob's, under the server's session.
    p.join("ws://127.0.0.1:1/sync", Some(Login::new("bob", "dev", Some("bob".into())))).unwrap();
    s.step(vec![
        ("do", Value::text("join")),
        ("user", Value::text("bob")),
        ("session", Value::text("dev")),
        ("token", Value::text("bob")),
    ]);
    // A server of the same module, sans-io, in this process.
    let schema = module.schema.clone();
    let mut server = Server::open(trusting(), open_access(), Silent, Authority::new(schema.clone(), ark::hash::closures(&module)));
    // Somebody else's entries, already in the log.
    for k in 0..3u8 {
        let e = ark::log::Entry {
            id: match id(k) {
                Value::Id(i) => i,
                _ => unreachable!(),
            },
            actor: "carol".into(),
            session: "dev".into(),
            roles: Default::default(),
            fn_hash: domain.mutator("create_playlist").unwrap().0.clone(),
            args: args(vec![("name", Value::text(format!("carol {k}")))]),
            autos: args(vec![("id", id(100 + k))]),
        };
        server.author(&e);
    }
    let conn = 1;
    let mut exchange = |p: &mut Peer, s: &mut Script, what: &str| {
        // Rounds of the client's frames to the server and the server's back,
        // until neither says anything: each round the client's frames go in,
        // and what comes back is received frame by frame, then persisted.
        loop {
            let outgoing = p.take_outgoing();
            let mut back = vec![];
            for m in outgoing {
                let m = ClientMsg::from_value(&m.to_value()).unwrap();
                server.recv(conn, m);
            }
            for (c, m) in server.take_outgoing() {
                if c == conn {
                    back.push(canon::encode(&ServerMsg::to_value(&m)));
                }
            }
            if back.is_empty() {
                break;
            }
            for f in &back {
                p.recv_frame(f).unwrap();
            }
            p.persist().unwrap();
            s.step(vec![
                ("do", Value::text("recv")),
                ("what", Value::text(what)),
                ("frames", Value::from(back.into_iter().map(Value::bytes).collect::<Vec<_>>())),
            ]);
        }
    };
    p.connected();
    s.step(vec![("do", Value::text("connected"))]);
    exchange(&mut p, &mut s, "the join synced");
    for k in 0..30 {
        mutate(
            &mut p,
            &mut s,
            "add_to_playlist",
            args(vec![("playlist_id", pl.clone()), ("track_id", Value::text(format!("online{k}")))]),
        );
        if k % 3 == 2 {
            exchange(&mut p, &mut s, "acknowledged");
        }
    }
    exchange(&mut p, &mut s, "acknowledged");
    p.disconnected();
    s.step(vec![("do", Value::text("disconnected"))]);
    for k in 0..4 {
        mutate(
            &mut p,
            &mut s,
            "add_to_playlist",
            args(vec![("playlist_id", pl.clone()), ("track_id", Value::text(format!("offline{k}")))]),
        );
    }
    p.persist().unwrap();
    s.step(vec![("do", Value::text("persist"))]);
    let state = Value::record(vec![
        ("cursor", Value::Int(p.cursor())),
        ("hash", Value::bytes(ark::hash::state_hash(&p.replica().confirmed))),
        ("view_hash", Value::bytes(ark::hash::state_hash(&p.replica().view))),
        ("pending", Value::from(p.replica().pending.iter().map(|e| Value::Id(e.id)).collect::<Vec<_>>())),
    ]);
    drop(p);
    let p = open(Options::server("bob", "dev", Some("bob".into())).with_autos(Autos::seeded(9)));
    s.step(vec![
        ("do", Value::text("open")),
        ("user", Value::text("bob")),
        ("session", Value::text("dev")),
        ("token", Value::text("bob")),
        ("alone", Value::Bool(false)),
        ("seed", Value::Int(9)),
        ("state", state),
    ]);
    drop(p);
    let text = obj(&[
        (
            "from",
            quoted("a Rust ark_client::Peer over the demo module, seeded, alone then joined to an in-process ark::protocol::Server (the scratch program in docs/plan-phones.md round 3)"),
        ),
        ("module", json(&Value::bytes(bytes))),
        ("steps", format!("[\n{}\n]", s.steps.join(",\n"))),
    ]);
    std::fs::write(out, text + "\n").unwrap();
}
