//! `protocol/` (§12): every frame as a value and its bytes; decode of
//! encode is the identity. Before the frames are written, the server's
//! rules about whose entry it takes are asserted: an older login of the
//! same user only where ownership is installed, a stranger's never, and
//! work authored before anyone signed in as whoever signs in.

use std::collections::BTreeMap;

use ark::canon::{decode, encode};
use ark::eval::{Args, Ctx};
use ark::hash::closures;
use ark::ir::SPEC_VERSION;
use ark::live::{ConnId, Silent};
use ark::log::Entry;
use ark::peer::{Authority, Replica};
use ark::protocol::{open_access, trusting, Client, ClientMsg, Mode, Server, ServerMsg, Subscription};
use ark::store::{Change, MemoryStore, Store};
use ark::value::{hex, Value};

use super::demo::{self, hash_of, id_n};
use super::json::{json, obj, quoted};
use super::Out;

fn args<const N: usize>(pairs: [(&str, Value); N]) -> Args {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn is_verdict(m: &ServerMsg) -> bool {
    matches!(m, ServerMsg::Ack { .. } | ServerMsg::Reject { .. })
}

/// What a server said to one connection that was a verdict.
fn verdicts(sv: &mut Server<Silent>, c: ConnId) -> Vec<ServerMsg> {
    sv.take_outgoing()
        .into_iter()
        .filter(|(to, m)| *to == c && is_verdict(m))
        .map(|(_, m)| m)
        .collect()
}

pub fn protocol(out: &Out) {
    out.dir("protocol/");
    let m = demo::module();
    let bodies = closures(&m);
    let h_add = hash_of(&m, "add_to_playlist");
    let h_create = hash_of(&m, "create_playlist");
    let entry = Entry {
        id: id_n(9),
        actor: "alice".into(),
        session: "alice-dev".into(),
        fn_hash: h_add.clone(),
        args: args([("playlist_id", Value::Id(id_n(1))), ("track_id", Value::text("t7"))]),
        autos: Args::new(),
    };
    let row: BTreeMap<String, Value> = args([
        ("playlist_id", Value::Id(id_n(1))),
        ("track_id", Value::text("t7")),
        ("pos", Value::Int(1)),
    ]);
    // A hello carries the spec version the client speaks, which is this
    // crate's: a version bump moves these two files and no others.
    let client_frames: Vec<(&str, ClientMsg)> = vec![
        (
            "hello",
            ClientMsg::Hello {
                sub: Subscription { since: 4, mode: Mode::Whole },
                token: Some("tok".into()),
                spec: SPEC_VERSION,
            },
        ),
        (
            "hello-facts",
            ClientMsg::Hello {
                sub: Subscription {
                    since: 0,
                    mode: Mode::ByFacts,
                },
                token: None,
                spec: SPEC_VERSION,
            },
        ),
        (
            "push",
            ClientMsg::Push {
                entries: vec![entry.clone()],
            },
        ),
        ("need_facts", ClientMsg::NeedFacts { seqs: vec![2, 3] }),
        ("need_closures", ClientMsg::NeedClosures { hashes: vec![h_add.clone()] }),
        (
            "verify",
            ClientMsg::Verify {
                seq: 4,
                hash: vec![0xab; 32],
            },
        ),
        ("say", ClientMsg::Say { frame: vec![1, 2, 3] }),
    ];
    let server_frames: Vec<(&str, ServerMsg)> = vec![
        (
            "batch",
            ServerMsg::Batch {
                items: vec![
                    (5, entry.clone(), None),
                    (6, entry.clone(), Some(vec![Change::Add("item".into(), row.clone())])),
                ],
                has_more: true,
            },
        ),
        (
            "facts",
            ServerMsg::FactsFor {
                items: vec![(
                    2,
                    vec![Change::Add("item".into(), row.clone()), Change::Remove("item".into(), row.clone())],
                )],
            },
        ),
        (
            "snapshot",
            ServerMsg::SnapshotOf {
                seq: 2,
                hash: vec![0xcd; 32],
                rows: BTreeMap::from([("item".to_string(), vec![Value::Struct(row.clone())])]),
            },
        ),
        (
            "ack",
            ServerMsg::Ack {
                ids: vec![id_n(9)],
                seqs: vec![5],
            },
        ),
        (
            "reject",
            ServerMsg::Reject {
                id: id_n(9),
                reason: "a playlist needs a name".into(),
            },
        ),
        (
            "denied",
            ServerMsg::Denied {
                reason: "not signed in".into(),
            },
        ),
        (
            "closures",
            ServerMsg::Closures {
                items: vec![(h_add.clone(), bodies[&h_add].clone())],
            },
        ),
        (
            "agree",
            ServerMsg::Agree {
                seq: 4,
                hash: vec![0xab; 32],
                ok: true,
            },
        ),
        ("heard", ServerMsg::Heard { frame: vec![4, 5] }),
    ];

    // An entry from an older session of the same user is accepted only
    // where ownership is installed; a stranger's never is.
    let server = || Server::open(trusting(), open_access(), Silent, Authority::new(m.schema.clone(), bodies.clone()));
    let signed_in = |owns: Option<ark::protocol::Owns>| {
        let mut sv = server();
        sv.recv(
            1,
            ClientMsg::Hello {
                sub: Subscription { since: 0, mode: Mode::Whole },
                token: Some("alice".into()),
                spec: SPEC_VERSION,
            },
        );
        let mut sv = match owns {
            Some(o) => sv.with_owns(o),
            None => sv,
        };
        let _ = sv.take_outgoing();
        sv
    };
    let old = Entry {
        id: id_n(20),
        actor: "alice".into(),
        session: "alice-old".into(),
        fn_hash: h_create.clone(),
        args: args([("name", Value::text("Road"))]),
        autos: args([("id", Value::Id(id_n(21)))]),
    };
    let verdict_on = |mut sv: Server<Silent>, e: &Entry| {
        sv.recv(1, ClientMsg::Push { entries: vec![e.clone()] });
        match verdicts(&mut sv, 1).as_slice() {
            [ServerMsg::Ack { .. }] => "ack".to_string(),
            [ServerMsg::Reject { reason, .. }] => reason.clone(),
            other => format!("{other:?}"),
        }
    };
    let got = verdict_on(signed_in(None), &old);
    assert_eq!(got, "not yours", "protocol: an older session was accepted with no ownership");
    let got = verdict_on(signed_in(Some(Box::new(|u, _| u == "alice"))), &old);
    assert_eq!(got, "ack", "protocol: an owned older session was refused");
    let bob = Entry {
        actor: "bob".into(),
        ..old.clone()
    };
    let got = verdict_on(signed_in(Some(Box::new(|_, _| true))), &bob);
    assert_eq!(got, "not yours", "protocol: another user's entry was accepted");

    // A peer used for a while with no account, then signed in: everything
    // it authored as nobody is pushed as the person who signed in, every
    // entry is accepted, and the rows say who they belong to.
    let pid = id_n(30);
    let mut local = Replica::open(m.schema.clone(), bodies.clone(), MemoryStore::empty(m.schema.clone()), 0, vec![]);
    let mut author = |i: u8, fh: &Vec<u8>, autos: Args, a: Args| {
        local
            .mutate(id_n(i), &Ctx::nobody(), fh, &autos, &a)
            .unwrap_or_else(|e| panic!("authoring {i}: {e:?}"));
    };
    author(31, &h_create, args([("id", Value::Id(pid))]), args([("name", Value::text("Offline"))]));
    for k in 0..10u8 {
        author(
            32 + k,
            &h_add,
            Args::new(),
            args([("playlist_id", Value::Id(pid)), ("track_id", Value::text(format!("t{k}")))]),
        );
    }
    let pushed = |client: &mut Client| {
        client.connected();
        let mut sv = server();
        for f in client.take_outgoing() {
            sv.recv(7, f);
        }
        let vs = verdicts(&mut sv, 7);
        (sv, vs)
    };
    let mut client = Client::open(local.clone(), Mode::Whole, None);
    client.sign_in(&Ctx::new("alice", "dev"), Some("alice".into()));
    let (sv_end, vs) = pushed(&mut client);
    let acked: usize = vs.iter().map(|v| if let ServerMsg::Ack { ids, .. } = v { ids.len() } else { 0 }).sum();
    assert!(
        acked == 11 && !vs.iter().any(|v| matches!(v, ServerMsg::Reject { .. })),
        "protocol: a signed-in peer's offline work was not all accepted: {vs:?}"
    );
    let owners: Vec<Option<Value>> = sv_end
        .authority
        .store
        .scan("playlist")
        .into_iter()
        .map(|r| r.get("user_id").cloned())
        .collect();
    assert_eq!(
        owners,
        vec![Some(Value::text("alice"))],
        "protocol: the offline playlist belongs to the wrong user"
    );
    let (_, unsigned) = pushed(&mut Client::open(local, Mode::Whole, None));
    assert!(
        unsigned.len() == 11
            && unsigned
                .iter()
                .all(|v| matches!(v, ServerMsg::Reject { reason, .. } if reason == "not yours")),
        "protocol: work authored as nobody was accepted without signing in: {unsigned:?}"
    );

    // A hello whose bytes say spec version 3 beside a frame that says this
    // one's: a runner must compare the two, not only decode one.
    if let Some((_, hello)) = client_frames.first() {
        let v = hello.to_value();
        let mut v3 = v.clone();
        if let Value::Struct(fs) = &mut v3 {
            fs.insert("spec".into(), Value::Int(3));
        }
        assert_ne!(v, v3, "the hello carries a spec version");
        out.write(
            "protocol/falsify/client-hello-bytes-of-v3.json",
            &obj(&[("frame", json(&v)), ("bytes", quoted(&hex(&encode(&v3)))), ("expect", quoted("fail"))]),
        );
    }
    for (name, f) in client_frames {
        let v = f.to_value();
        match decode(&encode(&v)).map(|d| ClientMsg::from_value(&d)) {
            Ok(Ok(back)) if back == f => {}
            other => panic!("protocol client {name}: {other:?}"),
        }
        out.write(
            &format!("protocol/client-{name}.json"),
            &obj(&[("frame", json(&v)), ("bytes", quoted(&hex(&encode(&v))))]),
        );
    }
    for (name, f) in server_frames {
        let v = f.to_value();
        // Closures decode with empty symbol names, so compare through their values.
        match decode(&encode(&v)).map(|d| ServerMsg::from_value(&d)) {
            Ok(Ok(back)) if back.to_value() == v => {}
            other => panic!("protocol server {name}: {other:?}"),
        }
        out.write(
            &format!("protocol/server-{name}.json"),
            &obj(&[("frame", json(&v)), ("bytes", quoted(&hex(&encode(&v))))]),
        );
    }
}
