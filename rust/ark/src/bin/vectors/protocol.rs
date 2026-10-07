//! `protocol/` (§12): every frame as a value and its bytes; decode of
//! encode is the identity. Before the frames are written, the server's
//! rules about whose entry it takes are asserted: an older login of the
//! same user only where ownership is installed, a stranger's never, and
//! work authored before anyone signed in as whoever signs in; and a
//! `Hello` naming another log answered with this one's snapshot at the
//! head, where one naming this log, or none, is answered as it was; and a
//! server that says its module answering a `Hello` at its head with an
//! empty page saying it, and holding an intent it cannot run
//! (`docs/plan-db.md` D1); and every entry sequenced with the roles of the
//! connection that pushed it, not the ones its device believed
//! (`docs/plan-guards.md` D1).

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
        roles: Default::default(),
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
            partial: false,
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
                    partial: false,
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
        // The same entry, frozen with the roles its author's device
        // believed (`docs/plan-guards.md` D1): `roles`, texts in ascending
        // order, which `push` above leaves out because it holds none — so
        // that file is the bytes it was.
        (
            "push-roles",
            ClientMsg::Push {
                entries: vec![Entry {
                    roles: ["library".to_string(), "editor".to_string()].into(),
                    ..entry.clone()
                }],
            },
        ),
        ("need_facts", ClientMsg::NeedFacts { seqs: vec![2, 3] }),
        ("need_closures", ClientMsg::NeedClosures { hashes: vec![h_add.clone()] }),
        (
            "verify",
            ClientMsg::Verify {
                partial: false,
                seq: 4,
                hash: vec![0xab; 32],
                log_id: None,
            },
        ),
        // The log the sequence is of, named (`docs/plan-db.md` D2): an
        // authority on another log answers `unknown`.
        (
            "verify-named",
            ClientMsg::Verify {
                partial: false,
                seq: 4,
                hash: vec![0xab; 32],
                log_id: Some(log_a),
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
        covers: None,
        items: vec![
            (5, entry.clone(), None),
            (6, entry.clone(), Some(vec![Change::Add("item".into(), row.clone())])),
        ],
        has_more: true,
        log_id,
        module,
    };
    let snapshot = |log_id, module| ServerMsg::SnapshotOf {
        held: None,
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
                facts: vec![],
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
                facts: vec![],
            },
        ),
        // An entry the authority stamped with other roles than its device
        // froze in it is acknowledged with its facts (`docs/plan-guards.md`
        // D1): `facts`, present only then, so the two above are the bytes
        // they were.
        (
            "ack-facts",
            ServerMsg::Ack {
                ids: vec![id_n(9)],
                seqs: vec![5],
                log_id: Some(log_a),
                facts: vec![(5, vec![Change::Add("item".into(), row.clone())])],
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
                unknown: false,
            },
        ),
        // A sequence the authority holds no state at — below its horizon,
        // past its head — is answered "cannot say", not a disagreement
        // (`docs/plan-db.md` D3).
        (
            "agree-unknown",
            ServerMsg::Agree {
                seq: 4,
                hash: vec![0xab; 32],
                ok: false,
                unknown: true,
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
                    partial: false,
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
        roles: Default::default(),
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

    // The authority stamps (`docs/plan-guards.md` D1): an entry is logged
    // with the roles of the connection that pushed it, whatever its device
    // believed — a role it was never given is not in the log, and one it
    // did not know it held is. The acknowledgement carries the entry's facts
    // only where the two differ and its function reads a role beyond a
    // guard's refusal: `create_playlist` reads none, so a restamped one of
    // it — every intent of a peer older than roles, which sends none — is
    // acknowledged as it always was; the guarded demo's `feature` reads
    // `editor` in its body, so one of it is acknowledged with its facts.
    // And an entry holding no role carries no `roles` at all.
    let stamped_as = |token: &str, believed: &[&str]| stamp_on(Authority::new(m.schema.clone(), bodies.clone()), &old, token, believed);
    let guarded = demo::guarded();
    let featured = Entry {
        fn_hash: hash_of(&guarded, "feature"),
        args: args([("name", Value::text("Picks"))]),
        autos: args([("id", Value::Id(id_n(51)))]),
        ..old.clone()
    };
    let feature_as =
        |token: &str, believed: &[&str]| stamp_on(Authority::new(guarded.schema.clone(), closures(&guarded)), &featured, token, believed);
    assert_eq!(
        stamped_as("alice", &["library"]),
        (vec![], "ack batch".to_string()),
        "protocol: a role the device believed and the login does not hold reached the log"
    );
    assert_eq!(
        stamped_as("alice:library", &[]),
        (vec!["library".to_string()], "ack batch".to_string()),
        "protocol: the login's role was not stamped on an entry its device authored without it"
    );
    assert_eq!(
        stamped_as("alice:library", &["library"]),
        (vec!["library".to_string()], "ack batch".to_string()),
        "protocol: a device that believed rightly was sent more than an acknowledgement"
    );
    assert_eq!(
        feature_as("alice:curator,editor", &["curator"]),
        (vec!["curator".to_string(), "editor".to_string()], "ack with its facts batch".to_string()),
        "protocol: a restamped entry whose body reads a role was acknowledged without its facts"
    );
    assert_eq!(
        feature_as("alice:curator", &["curator"]),
        (vec!["curator".to_string()], "ack batch".to_string()),
        "protocol: an entry whose roles the stamp kept was acknowledged with its facts"
    );
    let plain = ark::protocol::entry_value(&old);
    assert!(
        plain.as_struct().get("roles").is_none(),
        "protocol: an entry holding no role carries `roles`"
    );
    let mut empty = plain.clone();
    if let Value::Struct(fs) = &mut empty {
        fs.insert("roles".into(), Value::List(vec![].into()));
    }
    assert!(ark::protocol::entry_from_value(&empty).is_err(), "protocol: an empty `roles` decoded");
    let mut unordered = plain.clone();
    if let Value::Struct(fs) = &mut unordered {
        fs.insert("roles".into(), Value::List(vec![Value::text("library"), Value::text("editor")].into()));
    }
    assert!(
        ark::protocol::entry_from_value(&unordered).is_err(),
        "protocol: roles out of order decoded"
    );
    // And an ack's `facts` is absent where there are none: `facts: []` is
    // a second spelling, refused.
    if let Some((_, ack)) = server_frames.iter().find(|(n, _)| *n == "ack") {
        let mut v = ack.to_value();
        if let Value::Struct(fs) = &mut v {
            fs.insert("facts".into(), Value::List(vec![].into()));
        }
        assert!(ServerMsg::from_value(&v).is_err(), "protocol: an ack with empty facts decoded");
    }

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
                    partial: false,
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
                partial: false,
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
        roles: Default::default(),
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

/// An entry pushed to a server over `a` by a connection whose token is
/// `token`, its device believing `believed`: the roles the log holds for
/// it, and what that connection was sent (`docs/plan-guards.md` D1).
fn stamp_on(a: Authority, e: &Entry, token: &str, believed: &[&str]) -> (Vec<String>, String) {
    let mut sv = Server::open(trusting(), open_access(), Silent, a);
    sv.recv(
        1,
        ClientMsg::Hello {
            sub: Subscription {
                partial: false,
                since: 0,
                mode: Mode::Whole,
                log_id: None,
            },
            token: Some(token.into()),
            spec: SPEC_VERSION,
        },
    );
    let _ = sv.take_outgoing();
    let e = Entry {
        id: id_n(50),
        session: "dev".into(),
        roles: believed.iter().map(|r| r.to_string()).collect(),
        ..e.clone()
    };
    sv.recv(1, ClientMsg::Push { entries: vec![e] });
    let said: Vec<&str> = sv
        .take_outgoing()
        .iter()
        .filter(|(c, _)| *c == 1)
        .map(|(_, m)| match m {
            ServerMsg::Ack { facts, .. } if facts.len() == 1 && facts[0].0 == 1 => "ack with its facts",
            ServerMsg::Ack { facts, .. } if facts.is_empty() => "ack",
            ServerMsg::Batch { .. } => "batch",
            ServerMsg::Reject { .. } => "reject",
            _ => "other",
        })
        .collect();
    let logged = sv
        .authority
        .log
        .entries
        .get(&1)
        .map_or(vec![], |(e, _)| e.roles.iter().cloned().collect());
    (logged, said.join(" "))
}

/// `docs/plan-guards.md` D2 The frames of a peer served its union, from a
/// run over [`demo::scoped`] (`mine` and `tracks`, with the demo's two
/// mutators beside them to write the rows): alice's `hello` saying she held
/// one, the snapshot that starts her — her playlist, every item without its
/// position, and the tables held in part with their columns — a page that
/// passes an entry she holds nothing of and carries one she does, as its
/// envelope with its projected facts, a page carrying a playlist that a
/// track named for her made hers (the `exists` form, a row the entry never
/// touched), and her `verify` answered from the union's digest.
pub fn scoped(out: &Out) {
    let sm = demo::scoped();
    let dm = demo::module();
    let mut bodies = closures(&dm);
    bodies.extend(closures(&sm));
    let h_create = hash_of(&dm, "create_playlist");
    let h_add = hash_of(&dm, "add_to_playlist");
    let a = Authority::new(sm.schema.clone(), bodies.clone());
    let mut sv = Server::open(trusting(), open_access(), Silent, a).with_scopes(ark::scope::Scopes::of(&sm));
    let push = |sv: &mut Server<Silent>, c: ConnId, who: &str, k: u8, fh: &Vec<u8>, args: Args, autos: Args| {
        let e = Entry {
            id: id_n(k),
            actor: who.into(),
            session: "dev".into(),
            roles: Default::default(),
            fn_hash: fh.clone(),
            args,
            autos,
        };
        sv.recv(c, ClientMsg::Push { entries: vec![e] });
    };
    let hello = |partial, since| ClientMsg::Hello {
        sub: Subscription {
            since,
            mode: Mode::Whole,
            log_id: None,
            partial,
        },
        token: Some("alice".into()),
        spec: SPEC_VERSION,
    };
    // Bob's connection is whole to nobody's eyes but its own: it is the
    // writer here.
    sv.recv(
        2,
        ClientMsg::Hello {
            sub: Subscription {
                since: 0,
                mode: Mode::Whole,
                log_id: None,
                partial: false,
            },
            token: Some("bob".into()),
            spec: SPEC_VERSION,
        },
    );
    push(
        &mut sv,
        2,
        "bob",
        1,
        &h_create,
        args([("name", Value::text("Bob's"))]),
        args([("id", Value::Id(id_n(0x21)))]),
    );
    push(
        &mut sv,
        2,
        "bob",
        2,
        &h_create,
        args([("name", Value::text("Ours"))]),
        args([("id", Value::Id(id_n(0x22)))]),
    );
    sv.recv(1, hello(false, 0));
    push(
        &mut sv,
        1,
        "alice",
        3,
        &h_create,
        args([("name", Value::text("Mine"))]),
        args([("id", Value::Id(id_n(0x11)))]),
    );
    let _ = sv.take_outgoing();
    // Alice comes back, saying she held a union: the snapshot starts her.
    sv.disconnect(1);
    let hello_partial = hello(true, 3);
    sv.recv(1, hello_partial.clone());
    let to_alice =
        |sv: &mut Server<Silent>| -> Vec<ServerMsg> { sv.take_outgoing().into_iter().filter(|(to, _)| *to == 1).map(|(_, m)| m).collect() };
    let started = to_alice(&mut sv);
    let snapshot = started
        .iter()
        .find(|m| matches!(m, ServerMsg::SnapshotOf { held: Some(_), .. }))
        .cloned()
        .expect("protocol: a partial peer is started from a snapshot of its union");
    // In one push, bob makes a playlist alice does not hold and adds a track
    // to hers: one page passes the first and carries the second.
    let entry = |k: u8, fh: &Vec<u8>, args: Args, autos: Args| Entry {
        id: id_n(k),
        actor: "bob".into(),
        session: "dev".into(),
        roles: Default::default(),
        fn_hash: fh.clone(),
        args,
        autos,
    };
    sv.recv(
        2,
        ClientMsg::Push {
            entries: vec![
                entry(
                    5,
                    &h_create,
                    args([("name", Value::text("Bob's too"))]),
                    args([("id", Value::Id(id_n(0x23)))]),
                ),
                entry(
                    6,
                    &h_add,
                    args([("playlist_id", Value::Id(id_n(0x11))), ("track_id", Value::text("t2"))]),
                    Args::new(),
                ),
            ],
        },
    );
    let paged: Vec<ServerMsg> = to_alice(&mut sv);
    let page = paged
        .iter()
        .find(|m| matches!(m, ServerMsg::Batch { covers: Some(_), items, .. } if items.len() == 1))
        .cloned()
        .unwrap_or_else(|| panic!("protocol: a partial page: {paged:?}"));
    // A track named for alice on bob's other playlist makes that playlist
    // hers.
    push(
        &mut sv,
        2,
        "bob",
        7,
        &h_add,
        args([("playlist_id", Value::Id(id_n(0x22))), ("track_id", Value::text("alice"))]),
        Args::new(),
    );
    let seen = to_alice(&mut sv);
    let visible = seen
        .iter()
        .find(|m| {
            matches!(m, ServerMsg::Batch { items, .. }
                if items.iter().any(|(_, _, f)| f.as_ref().is_some_and(|f| f.iter().any(|c| matches!(c, Change::Add(t, _) if t == "playlist")))))
        })
        .cloned()
        .unwrap_or_else(|| panic!("protocol: the exists form's row arrives: {seen:?}"));
    // Alice, fed all of it, asks whether the authority agrees.
    let mut c = Client::open(
        Replica::open(sm.schema.clone(), bodies.clone(), MemoryStore::empty(sm.schema.clone()), 0, vec![]),
        Mode::Whole,
        Some("alice".into()),
    );
    c.connected();
    let mut sv2 = sv;
    sv2.disconnect(1);
    for _ in 0..4 {
        for m in c.take_outgoing() {
            sv2.recv(1, m);
        }
        for m in to_alice(&mut sv2) {
            c.recv(m);
        }
        c.settle();
    }
    c.verify_all();
    let verify = c
        .take_outgoing()
        .into_iter()
        .find(|m| matches!(m, ClientMsg::Verify { partial: true, .. }))
        .expect("a partial verify");
    sv2.recv(1, verify.clone());
    let agree = to_alice(&mut sv2)
        .into_iter()
        .find(|m| matches!(m, ServerMsg::Agree { ok: true, .. }))
        .expect("protocol: a partial verify agreed from the union's digest");
    super::claim(
        "the partial page passes what alice holds nothing of",
        matches!(&page, ServerMsg::Batch { covers: Some(cv), .. } if cv.upto - cv.after == 2),
    );
    super::claim(
        "the projected snapshot carries no position",
        matches!(&snapshot, ServerMsg::SnapshotOf { rows, .. } if rows.get("item").is_none_or(|rs| rs.iter().all(|r| !matches!(r, Value::Struct(m) if m.contains_key("pos"))))),
    );
    let client_frames = [("hello-partial", hello_partial), ("verify-partial", verify)];
    let server_frames = [
        ("snapshot-scoped", snapshot),
        ("batch-scoped", page),
        ("batch-scoped-visible", visible),
        ("agree-scoped", agree),
    ];
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

/// `docs/plan-guards.md` D3 The frames of a function with a server half,
/// from a run over [`demo::private`]: alice pushes a plain playlist, an
/// audited one — whose private block files an `audit` item the device never
/// previewed — and a track on the first, in one push. Her acknowledgement
/// carries the audited entry's facts, the whole run's, and no other's; bob,
/// who replays, is sent one page whose middle entry alone carries facts, and
/// takes it by them. A name the private block refuses is refused, and
/// nothing of it is logged.
pub fn private(out: &Out) {
    let pm = demo::private();
    let bodies = closures(&pm);
    let h_create = hash_of(&pm, "create_playlist");
    let h_audited = hash_of(&pm, "create_audited");
    let h_add = hash_of(&pm, "add_to_playlist");
    let mut sv = Server::open(trusting(), open_access(), Silent, Authority::new(pm.schema.clone(), bodies.clone()));
    let hello = |who: &str| ClientMsg::Hello {
        sub: Subscription {
            since: 0,
            mode: Mode::Whole,
            log_id: None,
            partial: false,
        },
        token: Some(who.into()),
        spec: SPEC_VERSION,
    };
    sv.recv(1, hello("alice"));
    sv.recv(2, hello("bob"));
    let _ = sv.take_outgoing();
    let entry = |k: u8, fh: &Vec<u8>, args: Args, autos: Args| Entry {
        id: id_n(k),
        actor: "alice".into(),
        session: "dev".into(),
        roles: Default::default(),
        fn_hash: fh.clone(),
        args,
        autos,
    };
    sv.recv(
        1,
        ClientMsg::Push {
            entries: vec![
                entry(
                    1,
                    &h_create,
                    args([("name", Value::text("Plain"))]),
                    args([("id", Value::Id(id_n(0x31)))]),
                ),
                entry(
                    2,
                    &h_audited,
                    args([("name", Value::text("Audited"))]),
                    args([("id", Value::Id(id_n(0x32)))]),
                ),
                entry(
                    3,
                    &h_add,
                    args([("playlist_id", Value::Id(id_n(0x31))), ("track_id", Value::text("t1"))]),
                    Args::new(),
                ),
            ],
        },
    );
    let said = sv.take_outgoing();
    let ack = said
        .iter()
        .find(|(c, m)| *c == 1 && matches!(m, ServerMsg::Ack { .. }))
        .map(|(_, m)| m.clone())
        .unwrap_or_else(|| panic!("protocol: alice is acknowledged: {said:?}"));
    let page = said
        .iter()
        .find(|(c, m)| *c == 2 && matches!(m, ServerMsg::Batch { items, .. } if items.len() == 3))
        .map(|(_, m)| m.clone())
        .unwrap_or_else(|| panic!("protocol: bob is paged: {said:?}"));
    let audit = |f: &[Change]| {
        f.iter()
            .any(|c| matches!(c, Change::Add(t, r) if t == "item" && r.get("track_id") == Some(&Value::text("audit"))))
    };
    super::claim(
        "the ack carries the audited entry's facts, private write and all, and no other's",
        matches!(&ack, ServerMsg::Ack { facts, .. } if facts.len() == 1 && facts[0].0 == 2 && audit(&facts[0].1)),
    );
    super::claim(
        "the page carries facts for the audited entry alone",
        matches!(&page, ServerMsg::Batch { items, .. }
            if items.iter().map(|(n, _, f)| (*n, f.is_some())).collect::<Vec<_>>() == vec![(1, false), (2, true), (3, false)]),
    );
    // Bob replays what he can and takes the audited entry by its facts: his
    // store is the authority's, the item he could never have run included.
    let client_bodies = ark::sim::client_bodies(&bodies);
    let mut bob = Client::open(
        Replica::open(pm.schema.clone(), client_bodies, MemoryStore::empty(pm.schema.clone()), 0, vec![]),
        Mode::Whole,
        Some("bob".into()),
    );
    bob.recv(page.clone());
    bob.settle();
    super::claim(
        "a replaying peer reaches the authority's state through the facts it was sent",
        bob.replica.verify_at() == (3, ark::hash::state_hash(&sv.authority.store)) && bob.replica.diverged.is_empty(),
    );
    // A name the private block keeps is the entry's verdict.
    sv.recv(
        1,
        ClientMsg::Push {
            entries: vec![entry(
                4,
                &h_audited,
                args([("name", Value::text("forbidden"))]),
                args([("id", Value::Id(id_n(0x33)))]),
            )],
        },
    );
    let refused = verdicts(&mut sv, 1);
    super::claim(
        "a private block's refusal is the entry's verdict, and nothing of it is logged",
        matches!(refused.as_slice(), [ServerMsg::Reject { reason, .. }] if reason.contains("keeps that name")) && sv.authority.log.head_seq() == 3,
    );
    for (name, f) in [("ack-private", ack), ("batch-private", page)] {
        let v = f.to_value();
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
