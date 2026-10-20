//! `spec/vectors/{protocol,trace,rebase}/`, written by the vector writer
//! (`ark_dev::vectors`) and run as `spec/RUNNERS.md` says: each must be
//! what the writer writes now, nothing else may sit beside them, each
//! passes its runner, and each `falsify/` one fails it.

use std::collections::BTreeSet;

use ark::codec::Reader;
use ark::frames::{frame_ty, Frame};
use ark::hash::{hex, EntryHash, FnHash, StateHash};
use ark::log::EntryId;
use ark::peer::{Input, Output};
use ark::schema::Space;
use ark_client::Act;
use ark_dev::fuzz::session;
use ark_dev::trace::{record, replay};
use ark_dev::vectors::{bytes_of, fleet, fleet_script, json_of, on_disk, op_json, sessions, vector};
use serde_json::{json, Value};

/// Every name written to `d`, and only those, on disk.
fn only(d: &str, written: &BTreeSet<String>) {
    let there: BTreeSet<String> = on_disk(d).into_iter().map(|(n, _)| n).collect();
    assert_eq!(&there, written, "{d} holds what the writer did not write");
}

fn space(s: Space) -> &'static str {
    ["durable", "volatile"][s as usize]
}

/// A frame's name: who writes it, its kind, its space.
fn name(f: &Frame, client: bool) -> String {
    let d = format!("{f:?}");
    let kind = d.split([' ', '{', '(']).next().unwrap().to_lowercase();
    format!("{}-{kind}-{}", if client { "client" } else { "server" }, space(f.space()))
}

/// The protocol runner: the JSON written under the frame type is the
/// bytes, and the bytes decode to a frame that encodes to them.
fn run_protocol(v: &Value) -> Result<(), String> {
    let bytes = v["bytes"].as_str().and_then(ark::hash::unhex).ok_or("no bytes")?;
    let mut b = vec![];
    bytes_of(&frame_ty(), &v["frame"], &mut b)?;
    let f = Frame::dec(&bytes).map_err(|r| r.0)?;
    let read = json_of(&frame_ty(), &mut Reader::new(&bytes))?;
    (b == bytes && f.enc() == bytes && read == v["frame"])
        .then_some(())
        .ok_or("the frame is not its bytes".into())
}

/// `protocol/`: one of every frame each side writes, in each space it
/// names, from the scripted sessions, and those no session sent built from
/// their entries. Falsified by `bytes_of` writing a sum's index after its
/// fields: no frame's JSON was its bytes.
#[test]
fn protocol() {
    let mut frames: Vec<(String, Frame)> = vec![];
    let mut seen = BTreeSet::new();
    for (_, net) in sessions() {
        for c in net.clients.values() {
            for (act, out, _) in c.record.iter().flatten() {
                let mut add = |b: &Vec<u8>, client: bool| {
                    let f = Frame::dec(b).unwrap();
                    if seen.insert(name(&f, client)) {
                        frames.push((name(&f, client), f));
                    }
                };
                if let Act::Step(_, Input::Frame { bytes, .. }) = act {
                    add(bytes, false);
                }
                out.iter().for_each(|o| {
                    if let Output::Send(b) = o {
                        add(b, true)
                    }
                });
            }
        }
    }
    let id = EntryId::of("s", 1);
    for (n, f) in [
        (
            "server-held-durable",
            Frame::Held {
                space: Space::Durable,
                id,
                func: FnHash([3; 32]),
            },
        ),
        (
            "server-reject-durable",
            Frame::Reject {
                space: Space::Durable,
                id,
                why: "a playlist needs a name".into(),
            },
        ),
    ] {
        if seen.insert(n.into()) {
            frames.push((n.into(), f));
        }
    }
    let mut written = BTreeSet::new();
    for (n, f) in &frames {
        let b = f.enc();
        let v = json!({"bytes": hex(&b), "frame": json_of(&frame_ty(), &mut Reader::new(&b)).unwrap(), "name": n});
        run_protocol(&v).unwrap();
        vector("protocol", &format!("{n}.json"), &format!("{v:#}\n")).unwrap();
        written.insert(format!("{n}.json"));
    }
    only("protocol", &written);
    for name in [
        "client-hello-volatile",
        "server-ack-volatile",
        "server-batch-volatile",
        "client-join-durable",
    ] {
        assert!(written.contains(&format!("{name}.json")), "{name}");
    }
    let (_, ack) = on_disk("protocol").into_iter().find(|(n, _)| n == "server-ack-durable.json").unwrap();
    let mut wrong: Value = serde_json::from_str(&ack).unwrap();
    let b = wrong["bytes"].as_str().unwrap().to_string();
    wrong["bytes"] = json!(format!("{}{}", &b[..b.len() - 2], if b.ends_with('0') { "1" } else { "0" }));
    wrong["expect"] = json!("fail");
    vector("protocol/falsify", "server-ack-durable.json", &format!("{wrong:#}\n")).unwrap();
    for (n, text) in on_disk("protocol/falsify") {
        assert!(run_protocol(&serde_json::from_str(&text).unwrap()).is_err(), "{n} passed");
    }
}

/// `trace/`: every client of every scripted session, recorded; each one on
/// disk replays into fresh machines step for step; the falsified one, a
/// byte of one frame changed, does not. Falsified by the player opening
/// the live machine without the device's declaration: the-room's clients'
/// `Hello`s were other bytes than recorded.
#[test]
fn traces() {
    let mut written = BTreeSet::new();
    for (session, net) in sessions() {
        for (i, c) in &net.clients {
            let name = format!("{session}-client-{i}.json");
            vector("trace", &name, &record(&format!("{session}, client {i}"), c)).unwrap();
            written.insert(name);
        }
    }
    only("trace", &written);
    for (n, text) in on_disk("trace") {
        replay(&text).unwrap_or_else(|e| panic!("{n}: {e}"));
    }
    let (_, text) = on_disk("trace").into_iter().find(|(n, _)| n == "the-demo-client-1.json").unwrap();
    let at = text.find("\"bytes\":\"").unwrap() + 20;
    let flipped = if &text[at..at + 1] == "0" { "1" } else { "0" };
    let wrong = format!("{}{flipped}{}", &text[..at], &text[at + 1..]).replacen("{\"device\"", "{\"expect\":\"fail\",\"device\"", 1);
    vector("trace/falsify", "the-demo-client-1.json", &wrong).unwrap();
    for (n, text) in on_disk("trace/falsify") {
        assert!(replay(&text).is_err(), "{n} replayed");
    }
}

/// A trace corpus (`ark-fuzz N SEED traces DIR`, the flake's
/// `trace-corpus`): every file under `ARK_TRACE_CORPUS` replays, and there
/// is at least one; with the variable unset, nothing to do. What the
/// flake's `traces` check runs beside `trace()` over `spec/vectors/trace/`.
/// Falsified by a corpus with one byte of one frame changed: that trace
/// did not replay.
#[test]
fn trace_corpus() {
    let Some(dir) = std::env::var_os("ARK_TRACE_CORPUS") else {
        println!("ARK_TRACE_CORPUS is not set: no corpus replayed");
        return;
    };
    let mut n = 0;
    for e in std::fs::read_dir(&dir).expect("the corpus").flatten() {
        let text = std::fs::read_to_string(e.path()).unwrap();
        replay(&text).unwrap_or_else(|why| panic!("{}: {why}", e.path().display()));
        n += 1;
    }
    assert!(n > 0, "the corpus is empty");
    println!("{n} traces of the corpus replayed");
}

/// The sessions the fuzzer found a bug with, by seed, and what it was.
const FOUND: [(u64, &str); 5] = [
    (6, "an intent refused alone drew a local, and the server held its chain for the gap"),
    (29, "a Join lost on the way left its chain pushed one by one into a gap"),
    (202, "a push sequenced below a reopened horizon was answered by nobody"),
    (225, "a volatile push overtaken by a later one was never answered"),
    (7144, "a device reopened after joining, unanswered, pushed its chain into a gap"),
];

fn fleet_vector(seed: u64, head: i64, hash: StateHash) -> Value {
    let script: Vec<Value> = fleet_script().iter().map(op_json).collect();
    json!({"clients": 3, "hash": hash.hex(), "head": head, "playlist": "Fleet", "script": script, "seed": seed})
}

/// `rebase/`: `fleet-seed-7` through the hub at K1's pin, and every fuzz
/// session that found a bug, each run again to the head and state it
/// reached once fixed; the falsified fleet, its hash changed, does not
/// pass. Falsified by taking out the rule that acknowledges a push below a
/// reopened horizon: seed 202 did not settle.
#[test]
fn rebase() {
    let net = fleet(7);
    let (head, hash) = (net.hub.log.log.head().0, net.hub.log.store.state_hash());
    assert_eq!(hash.hex(), "ff04f56d613ee73dad721e02cb6ebe125fa93fe801b2f1713499038f55475e95", "K1's pin");
    let right = fleet_vector(7, head, hash);
    vector("rebase", "fleet-seed-7.json", &format!("{right:#}\n")).unwrap();
    let mut written = BTreeSet::from(["fleet-seed-7.json".to_string()]);
    for (seed, found) in FOUND {
        let (net, _) = session(seed, true).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        let (head, at) = net.hub.log.log.head();
        let v = json!({"found": found, "hash": at.hex(), "head": head, "seed": seed, "state": net.hub.log.store.state_hash().hex()});
        let name = format!("fleet-fuzz-seed-{seed}.json");
        vector("rebase", &name, &format!("{v:#}\n")).unwrap();
        written.insert(name);
    }
    only("rebase", &written);
    let mut wrong = right.clone();
    wrong["hash"] = json!(StateHash([0; 32]).hex());
    wrong["expect"] = json!("fail");
    vector("rebase/falsify", "fleet-seed-7.json", &format!("{wrong:#}\n")).unwrap();
    for (n, text) in on_disk("rebase").into_iter().chain(on_disk("rebase/falsify")) {
        let v: Value = serde_json::from_str(&text).unwrap();
        let seed = v["seed"].as_u64().unwrap();
        let ok = if n.starts_with("fleet-seed") {
            let net = fleet(seed);
            net.hub.log.log.head().0 == v["head"] && net.hub.log.store.state_hash().hex() == v["hash"]
        } else {
            let (net, _) = session(seed, false).unwrap();
            let h: EntryHash = net.hub.log.log.head().1;
            net.hub.log.log.head().0 == v["head"] && h.hex() == v["hash"]
        };
        assert_eq!(ok, v["expect"] != "fail", "{n}");
    }
}
