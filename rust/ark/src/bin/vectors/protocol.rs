//! `protocol/` (§12): every frame as a value and its bytes; decode of
//! encode is the identity. Before the frames are written, the server's
//! rules about whose entry it takes are asserted: an older login of the
//! same user only where ownership is installed, a stranger's never, and
//! work authored before anyone signed in as whoever signs in; and a
//! `Hello` naming another log answered with this one's snapshot at the
//! head, where one naming this log, or none, is answered as it was; and a
//! server that says its module answering a `Hello` at its head with an
//! empty page saying it, and holding an intent it cannot run
//! (`docs/plan-db.md` D1).

use std::collections::BTreeMap;

use ark::canon::{decode, encode};
use ark::eval::{Args, Ctx};
use ark::hash::closures;
use ark::ir::SPEC_VERSION;
use ark::live::{ConnId, Silent};
use ark::log::Entry;
use ark::peer::{Authority, Replica};
use ark::protocol::{open_access, trusting, Client, ClientMsg, Mode, Server, ServerMsg, Subscription};
use ark::store::{Change, MemoryStore, Row, Store};
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
    // A log's identity (§10): the one the frames below name, and another.
    let (log_a, log_b) = (id_n(0xa1), id_n(0xb2));
    let row: Row = Row::from_struct(args([
        ("playlist_id", Value::Id(id_n(1))),
        ("track_id", Value::text("t7")),
        ("pos", Value::Int(1)),
    ]));
    // A hello carries the spec version the client speaks, which is this
    // crate's: a version bump moves these files and no others. Neither of
    // the first two names a log, so they are the bytes they were before
    // logs had names; `hello-named` is the first naming the log its cursor
    // is of (Round 4).
    let hello = |log_id| ClientMsg::Hello {
        sub: Subscription {
            since: 4,
            mode: Mode::Whole,
            log_id,
        },
        token: Some("tok".into()),
        spec: SPEC_VERSION,
    };
    let client_frames: Vec<(&str, ClientMsg)> = vec![
        ("hello", hello(None)),
        (
            "hello-facts",
            ClientMsg::Hello {
                sub: Subscription {
                    since: 0,
                    mode: Mode::ByFacts,
                    log_id: None,
                },
                token: None,
                spec: SPEC_VERSION,
            },
        ),
        ("hello-named", hello(Some(log_a))),
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
    // A batch and a snapshot of a log nobody named, as they always were,
    // and each again of a named one.
    //
    // A page and a snapshot also say the server's module hash, as `module`
    // (`docs/plan-db.md` D1): absent from every file written before, which
    // keep their bytes, and present in the `-module` pair, which a server
    // built from a module always sends.
    let module = || Some(ark::hash::module_hash(&m));
    let batch = |log_id, module| ServerMsg::Batch {
        items: vec![
            (5, entry.clone(), None),
            (6, entry.clone(), Some(vec![Change::Add("item".into(), row.clone())])),
        ],
        has_more: true,
        log_id,
        module,
    };
    let snapshot = |log_id, module| ServerMsg::SnapshotOf {
        seq: 2,
        hash: vec![0xcd; 32],
        rows: BTreeMap::from([("item".to_string(), vec![row.to_value()])]),
        log_id,
        module,
    };
    let server_frames: Vec<(&str, ServerMsg)> = vec![
        ("batch", batch(None, None)),
        ("batch-named", batch(Some(log_a), None)),
        ("batch-module", batch(Some(log_a), module())),
        (
            "facts",
            ServerMsg::FactsFor {
                items: vec![(
                    2,
                    vec![Change::Add("item".into(), row.clone()), Change::Remove("item".into(), row.clone())],
                )],
            },
        ),
        ("snapshot", snapshot(None, None)),
        ("snapshot-named", snapshot(Some(log_a), None)),
        ("snapshot-module", snapshot(Some(log_a), module())),
        (
            "ack",
            ServerMsg::Ack {
                ids: vec![id_n(9)],
                seqs: vec![5],
                log_id: None,
            },
        ),
        // An ack names the log as a page does, so a peer confirmed by an
        // ack alone knows which log it is at (`arkc fuzz`, D2).
        (
            "ack-named",
            ServerMsg::Ack {
                ids: vec![id_n(9)],
                seqs: vec![5],
                log_id: Some(log_a),
            },
        ),
        (
            "reject",
            ServerMsg::Reject {
                id: id_n(9),
                reason: "a playlist needs a name".into(),
            },
        ),
        // An intent naming a function this server never ran: kept pending
        // by the peer, not refused (`docs/plan-db.md` D1).
        (
            "held",
            ServerMsg::Held {
                id: id_n(9),
                reason: ark::protocol::unknown_function(&h_add),
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
                sub: Subscription {
                    since: 0,
                    mode: Mode::Whole,
                    log_id: None,
                },
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

    // A server whose log is named: a hello at its head naming another log
    // is answered with its snapshot at the head, named; one naming this log
    // and one naming none are answered with nothing, as a hello at the
    // head always was.
    let mut named = Authority::new(m.schema.clone(), bodies.clone());
    named.log.name_if_unnamed(log_a);
    let mut sv = Server::open(trusting(), open_access(), Silent, named);
    for (conn, said) in [(1, Some(log_b)), (2, Some(log_a)), (3, None)] {
        sv.recv(
            conn,
            ClientMsg::Hello {
                sub: Subscription {
                    since: 0,
                    mode: Mode::Whole,
                    log_id: said,
                },
                token: Some("alice".into()),
                spec: SPEC_VERSION,
            },
        );
    }
    let answers: Vec<(ConnId, Option<ark::value::Id>)> = sv
        .take_outgoing()
        .into_iter()
        .filter_map(|(c, f)| match f {
            ServerMsg::SnapshotOf { seq: 0, log_id, .. } => Some((c, log_id)),
            _ => None,
        })
        .collect();
    assert_eq!(
        answers,
        vec![(1, Some(log_a))],
        "protocol: a hello naming another log is answered with this one's snapshot, and no other hello is"
    );

    // A server that says its module (`docs/plan-db.md` D1): a hello at its
    // head is answered with an empty page carrying the module, where one
    // that says none is answered with nothing; and an intent at a hash no
    // module it ran shipped is held, not refused.
    let mut said =
        Server::open(trusting(), open_access(), Silent, Authority::new(m.schema.clone(), bodies.clone())).with_module(ark::hash::module_hash(&m));
    said.recv(
        1,
        ClientMsg::Hello {
            sub: Subscription {
                since: 0,
                mode: Mode::Whole,
                log_id: None,
            },
            token: Some("alice".into()),
            spec: SPEC_VERSION,
        },
    );
    let first = said.take_outgoing();
    assert!(
        matches!(first.as_slice(), [(1, ServerMsg::Batch { items, module: Some(_), .. })] if items.is_empty()),
        "protocol: a hello at the head of a server that says its module: {first:?}"
    );
    let unknown = Entry {
        id: id_n(40),
        actor: "alice".into(),
        session: "dev".into(),
        fn_hash: vec![7; 32],
        args: Args::new(),
        autos: Args::new(),
    };
    said.recv(1, ClientMsg::Push { entries: vec![unknown] });
    let answer = said.take_outgoing();
    assert!(
        matches!(answer.as_slice(), [(1, ServerMsg::Held { id, .. })] if *id == id_n(40)),
        "protocol: an unknown function is held: {answer:?}"
    );

    // Absence is the one encoding of "no log": a hello saying `log: null`
    // is not a frame.
    if let Some((_, h)) = client_frames.first() {
        let mut v = h.to_value();
        if let Value::Struct(fs) = &mut v {
            fs.insert("log".into(), Value::Null);
        }
        assert!(ClientMsg::from_value(&v).is_err(), "protocol: a hello whose log is null decoded");
    }

    // A snapshot whose bytes name another log than its frame: a runner
    // must read the log's identity, not only the rows.
    if let Some((_, snap)) = server_frames.iter().find(|(n, _)| *n == "snapshot-named") {
        let v = snap.to_value();
        let mut other = v.clone();
        if let Value::Struct(fs) = &mut other {
            fs.insert("log".into(), Value::Id(log_b));
        }
        assert_ne!(v, other, "the snapshot names its log");
        out.write(
            "protocol/falsify/server-snapshot-bytes-of-another-log.json",
            &obj(&[("frame", json(&v)), ("bytes", quoted(&hex(&encode(&other)))), ("expect", quoted("fail"))]),
        );
    }

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
