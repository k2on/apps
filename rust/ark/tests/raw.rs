//! `docs/plan-guards.md` D4: the authority's raw writes, `ark.put_row` and
//! `ark.delete_row`. Named by fixed hashes no module carries; authored by a
//! server's authority alone, as itself; replayed by every whole peer and
//! taken by its facts, filtered to its union, by every partial one; and
//! refused, landing nowhere, when a client pushes one.

#[path = "support/orgs.rs"]
mod orgs;

use std::collections::BTreeSet;

use ark::canon::encode;
use ark::eval::{Args, Ctx};
use ark::hash::closures;
use ark::log::Entry;
use ark::peer::{Authority, Replica, Sequenced};
use ark::raw::{self, Raw};
use ark::sim::{Op, Sim};
use ark::store::{Change, MemoryStore, Refusal, Row, Store};
use ark::value::{Id, Value};

fn id(k: u8) -> Id {
    let mut i = [0u8; 16];
    i[0] = 0xd4;
    i[15] = k;
    i
}

fn row(pairs: &[(&str, Value)]) -> Row {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

/// Each is named by `sha256(enc(Text name))`, which is no closure's hash —
/// a closure's encoding is a struct — and read back to itself. Falsified
/// by hashing the bare name's bytes: the hash was not the encoding's.
#[test]
fn a_raw_write_is_named_by_a_fixed_hash() {
    for r in [Raw::PutRow, Raw::DeleteRow] {
        assert_eq!(*r.hash(), ark::sha256::sha256(&encode(&Value::text(r.name()))));
        assert_eq!(raw::of(r.hash()), Some(r));
    }
    assert_ne!(Raw::PutRow.hash(), Raw::DeleteRow.hash());
    let m = orgs::module();
    for h in closures(m.build()).keys() {
        assert!(!raw::is_raw(h), "a module's function is never a raw write");
    }
}

/// A fleet over a scoped domain: client 0 an admin, who holds everything
/// and replays; client 1 a member, who holds a union and takes facts. The
/// authority writes raw — an account with its password, an org renamed, an
/// org and a membership that bring it into client 1's union, then the
/// membership taken away again — and everybody converges on the authority,
/// client 1 holding none of the password, and the log replays from its
/// intents and its facts alike. Falsified by having every peer but the
/// authority skip a raw write: client 0 diverged from the authority.
#[test]
fn the_authority_writes_raw_and_every_peer_converges() {
    let m = orgs::module();
    let built = m.build().clone();
    let mut sim = Sim::new(built.schema.clone(), closures(&built), 2, 7).durable();
    let admin: BTreeSet<String> = ["admin".to_string()].into();
    sim.run(&Op::Roles {
        peer: 0,
        roles: admin.clone(),
        believes: admin,
    })
    .unwrap();
    sim.settle();
    let (one, two) = (Value::Id(id(1)), Value::Id(id(2)));
    let account = |password: &str| {
        row(&[
            ("user", Value::text("peer-1")),
            ("name", Value::text("One")),
            ("password", Value::text(password)),
        ])
    };
    let edits = [
        Change::Add("org".into(), row(&[("id", one.clone()), ("name", Value::text("Ones"))])),
        Change::Add("account".into(), account("hunter2")),
        Change::Edit(
            "org".into(),
            row(&[("id", one.clone()), ("name", Value::text("Ones"))]),
            row(&[("id", one.clone()), ("name", Value::text("The Ones"))]),
        ),
        Change::Add("org".into(), row(&[("id", two.clone()), ("name", Value::text("Twos"))])),
        Change::Add("member".into(), row(&[("org_id", two.clone()), ("user", Value::text("peer-1"))])),
    ];
    for (k, change) in edits.into_iter().enumerate() {
        sim.run(&Op::Edit {
            eid: id(0x10 + k as u8),
            change,
        })
        .unwrap();
        sim.run(&Op::Step).unwrap();
    }
    sim.settle();
    sim.converged().unwrap_or_else(|e| panic!("{e}"));
    let partial = &sim.clients[&1].replica;
    assert!(partial.partial.is_some(), "client 1 holds a union");
    assert_eq!(partial.confirmed.scan("org").len(), 1, "the org a membership brought in, and only it");
    let mine = partial.confirmed.scan("account");
    assert_eq!(mine.len(), 1);
    assert!(mine[0].get("password").is_none(), "the password a scope leaves out is not held");
    assert_eq!(sim.clients[&0].replica.confirmed.scan("org").len(), 2, "the admin holds both");
    // The membership taken away: the org leaves the union as the row goes.
    sim.run(&Op::Edit {
        eid: id(0x20),
        change: Change::Remove("member".into(), row(&[("org_id", two), ("user", Value::text("peer-1"))])),
    })
    .unwrap();
    sim.settle();
    sim.converged().unwrap_or_else(|e| panic!("{e}"));
    assert!(sim.clients[&1].replica.confirmed.scan("org").is_empty());
    sim.replays().unwrap_or_else(|e| panic!("{e}"));
    let raw_entries: Vec<&Entry> = sim
        .server
        .authority
        .log
        .entries
        .values()
        .map(|(e, _)| e)
        .filter(|e| raw::is_raw(&e.fn_hash))
        .collect();
    assert_eq!(raw_entries.len(), 6);
    assert!(raw_entries.iter().all(|e| e.actor == raw::AUTHOR && e.autos.is_empty()));
}

/// A raw write the constraints refuse is the authority's verdict and is
/// logged nowhere, as any refused write is. Falsified by taking a refusal
/// as no change: the orphan membership was appended, as an entry that
/// changed nothing.
#[test]
fn a_raw_write_is_judged_by_the_constraints() {
    let m = orgs::module();
    let built = m.build().clone();
    let mut sim = Sim::new(built.schema.clone(), closures(&built), 1, 3);
    let orphan = Change::Add("member".into(), row(&[("org_id", Value::Id(id(9))), ("user", Value::text("peer-0"))]));
    let out = sim.server.edit(id(1), &ark::sim::authority_identity(), &orphan);
    assert_eq!(
        out,
        Sequenced::Rejected(Refusal::MissingParent("member".into(), "org_id".into(), "org".into()))
    );
    assert_eq!(sim.server.authority.log.head_seq(), 0);
}

/// A client that pushes a raw write — built by hand, since no replica
/// authors one — is refused it as forbidden, and it lands nowhere, while
/// the same client's ordinary write goes in; one claiming to be the
/// authority's is refused as forbidden too, before whose it is is asked.
/// Falsified by taking the server's check out: the one claiming the
/// authority was answered "not yours" (the client's own was still refused,
/// by the authority's refusal to sequence one); with that taken out as
/// well, the client's own was sequenced.
#[test]
fn a_pushed_raw_write_is_refused_and_lands_nowhere() {
    let m = orgs::module();
    let built = m.build().clone();
    let mut sim = Sim::new(built.schema.clone(), closures(&built), 2, 11).durable();
    sim.settle();
    let mine = Change::Add(
        "account".into(),
        row(&[
            ("user", Value::text("peer-1")),
            ("name", Value::text("Mine")),
            ("password", Value::text("now")),
        ]),
    );
    sim.run(&Op::PushRaw {
        peer: 1,
        eid: id(0x31),
        change: mine,
    })
    .unwrap();
    // Delivered by hand, every frame of it: a settle drops what is in
    // flight, and the network may lose a step.
    let conn = sim.conn[&1];
    let in_flight = sim.to_server.get_mut(&1).map(std::mem::take).unwrap_or_default();
    assert_eq!(in_flight.len(), 1, "the push is on the wire");
    for f in in_flight {
        sim.server.recv(conn, f);
    }
    let reject = |said: &[(i64, ark::protocol::ServerMsg)], eid: Id, table: &str| {
        said.iter().any(|(c, m)| {
            *c == conn
                && matches!(m, ark::protocol::ServerMsg::Reject { id, reason } if *id == eid && *reason == format!("{table}: not this login's to write"))
        })
    };
    let said = sim.server.take_outgoing();
    assert!(reject(&said, id(0x31), "account"), "{said:?}");
    // And one claiming to be the authority's own is refused as forbidden
    // before whose it is is asked: no connection pushes a raw write.
    let (r, args) = raw::call_of(
        &built.schema,
        &Change::Add("org".into(), row(&[("id", Value::Id(id(0x34))), ("name", Value::text("Ours"))])),
    );
    sim.server.recv(
        conn,
        ark::protocol::ClientMsg::Push {
            entries: vec![Entry {
                id: id(0x35),
                actor: raw::AUTHOR.into(),
                session: "sim".into(),
                roles: Default::default(),
                fn_hash: r.hash().clone(),
                args,
                autos: Args::new(),
            }],
        },
    );
    let said = sim.server.take_outgoing();
    assert!(reject(&said, id(0x35), "org"), "{said:?}");
    // The same client's ordinary write goes in, and the fleet converges
    // with neither raw write anywhere.
    let create = closures(&built)
        .into_iter()
        .find(|(_, c)| c.function.name == "create_org")
        .map(|(h, _)| h)
        .unwrap();
    let mut a = Args::new();
    a.insert("name".into(), Value::text("Theirs"));
    let mut autos = Args::new();
    autos.insert("id".into(), Value::Id(id(0x32)));
    sim.mutate(1, id(0x33), &create, &autos, &a);
    sim.settle();
    sim.converged().unwrap_or_else(|e| panic!("{e}"));
    let log = &sim.server.authority.log;
    assert!(
        log.seq_of(&id(0x31)).is_none() && log.seq_of(&id(0x35)).is_none(),
        "no pushed raw write is in the log"
    );
    assert!(log.seq_of(&id(0x33)).is_some(), "the ordinary write is");
    assert!(sim.server.authority.store.scan("account").is_empty());
}

/// Nobody but a server's authority authors one: a replica refuses to, an
/// authority asked to sequence one as an intent refuses, and an authority
/// that is not a server's — a peer alone's, a client here — cannot `edit`.
/// Falsified by letting `Replica::mutate` run it: the replica previewed the
/// write and pended it.
#[test]
fn nobody_but_a_servers_authority_authors_one() {
    let m = orgs::module();
    let built = m.build().clone();
    let sch = built.schema.clone();
    let put = Change::Add(
        "account".into(),
        row(&[("user", Value::text("x")), ("name", Value::text("X")), ("password", Value::text("p"))]),
    );
    let (r, args) = raw::call_of(&sch, &put);
    assert_eq!(r, Raw::PutRow);
    let mut replica = Replica::open(sch.clone(), closures(&built), MemoryStore::empty(sch.clone()), 0, vec![]);
    assert_eq!(
        replica.mutate(id(1), &Ctx::new("x", "s"), r.hash(), &Args::new(), &args),
        Err(Refusal::Forbidden("account".into()))
    );
    assert!(replica.pending.is_empty() && replica.view.scan("account").is_empty());
    let mut alone = Authority::new(sch.clone(), closures(&built));
    assert_eq!(
        alone.edit(id(2), &Ctx::new(raw::AUTHOR, "alone"), &put),
        Sequenced::Rejected(Refusal::Forbidden("account".into())),
        "an authority that is not a server's is a client here"
    );
    let e = Entry {
        id: id(3),
        actor: "x".into(),
        session: "s".into(),
        roles: Default::default(),
        fn_hash: r.hash().clone(),
        args: args.clone(),
        autos: Args::new(),
    };
    alone.private = true;
    assert_eq!(alone.sequence_entry(&e), Sequenced::Rejected(Refusal::Forbidden("account".into())));
    assert!(matches!(
        alone.edit(id(4), &Ctx::new(raw::AUTHOR, "server"), &put),
        Sequenced::Appended(1, _)
    ));
    let delete = Change::Remove("account".into(), alone.store.scan("account")[0].clone());
    let (r, args) = raw::call_of(&sch, &delete);
    assert_eq!((r, args.get("key")), (Raw::DeleteRow, Some(&Value::List(vec![Value::text("x")].into()))));
    assert!(matches!(
        alone.edit(id(5), &Ctx::new(raw::AUTHOR, "server"), &delete),
        Sequenced::Appended(2, _)
    ));
    assert!(alone.store.scan("account").is_empty());
}
