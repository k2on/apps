//! `docs/plan-guards.md` D3: a mutation's server half, `ctx.private`. The
//! block in the IR and stripped from a client's module under one hash; run
//! last, at the authority alone, natively and through the interpreter; its
//! facts riding the acknowledgement and the page; and its refusal the
//! entry's verdict.

// The row structs are the vocabulary's: a body reads their fields only
// natively.
#![allow(dead_code)]

use std::collections::BTreeMap;

use ark::authoring::*;
use ark::eval::{self, Args};
use ark::hash::{closures, state_hash, Closure, FnHash};
use ark::ir::{StdFn, Stmt};
use ark::live::Silent;
use ark::peer::{Authority, Replica};
use ark::protocol::{open_access, trusting, Client, ClientMsg, Mode, Server, ServerMsg};
use ark::store::{MemoryStore, Refusal, Store};
use ark::value::Value;

pub struct Notes {
    pub note: Table<Note>,
    pub audit: Table<Audit>,
}
impl Tables for Notes {
    fn open() -> Self {
        Notes {
            note: table(),
            audit: table(),
        }
    }
}

pub struct Note {
    pub id: Id<Note>,
    pub text: Text,
    pub by: Text,
}
impl Row for Note {
    const NAME: &str = "note";
    type Key = (Id<Note>,);
    fn columns() -> Columns<Self> {
        columns().id(Self::id).text(Self::text).text(Self::by).key((Self::id,))
    }
}
#[allow(non_upper_case_globals)]
impl Note {
    pub const id: Col<Self, Id<Self>> = col("id");
    pub const text: Col<Self, Text> = col("text");
    pub const by: Col<Self, Text> = col("by");
}

/// What only the server writes: who wrote each note, and how long it was.
pub struct Audit {
    pub id: Id<Audit>,
    pub note: Id<Note>,
    pub who: Text,
    pub length: Int,
}
impl Row for Audit {
    const NAME: &str = "audit";
    type Key = (Id<Audit>,);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::id)
            .id(Self::note)
            .refs::<Note>()
            .text(Self::who)
            .int(Self::length)
            .key((Self::id,))
    }
}
#[allow(non_upper_case_globals)]
impl Audit {
    pub const id: Col<Self, Id<Self>> = col("id");
    pub const note: Col<Self, Id<Note>> = col("note");
    pub const who: Col<Self, Text> = col("who");
    pub const length: Col<Self, Int> = col("length");
}

pub struct Write {
    pub text: Text,
}
impl Input for Write {
    fn schema() -> Object<Self> {
        object().field("text", text().min(1))
    }
}

/// `write` files an audit row in a private block written *before* the note
/// it is about: the block refuses unless the note is already there, so it
/// holds only where the block runs after the body — and refuses a text the
/// server keeps. `plain` is the same note with no server half.
fn module() -> Module {
    let notes = router::<Notes>("notes");
    Module::new((notes.routes((
        notes.input::<Write>().mutation("write", |ctx, db, input| {
            let id: Id<Note> = ctx.new_id("id");
            let audit: Id<Audit> = ctx.new_id("audit");
            let (who, text) = (ctx.user, input.text);
            ctx.private(move |db: &Notes| {
                unless(db.note.exists((id,)), || refuse("a private block ran before its body"));
                when(text.eq("secret"), || refuse("the server keeps that one"));
                db.audit.insert(Audit {
                    id: audit,
                    note: id,
                    who,
                    length: text.len(),
                })
            });
            db.note.insert(Note { id, text, by: ctx.user })
        }),
        notes.input::<Write>().mutation("plain", |ctx, db, input| {
            db.note.insert(Note {
                id: ctx.new_id("id"),
                text: input.text,
                by: ctx.user,
            })
        }),
    )),))
}

fn args<const N: usize>(pairs: [(&str, Value); N]) -> Args {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn idv(k: u8) -> Value {
    let mut b = [0u8; 16];
    b[15] = k;
    Value::Id(b)
}

fn autos(k: u8) -> Args {
    args([("id", idv(k)), ("audit", idv(100 + k))])
}

fn says(t: &str) -> Args {
    args([("text", Value::text(t))])
}

/// The block in the IR, and gone from the module a client loads: `write`
/// says `private` in both, carries a `Private` statement only in the
/// server's, and is named by one hash in the two — which is not the hash of
/// the server's function hashed whole. A module with no block emits and
/// hashes as it always did. Falsified by hashing the function whole
/// (`function_hash` without `strip`): the two modules name `write` by two
/// hashes.
#[test]
fn a_private_block_is_stripped_under_one_hash() {
    let m = module();
    let server = m.build();
    let write = server.lookup_function("write").unwrap();
    assert!(write.private, "the server's write says private");
    assert!(matches!(write.body.first(), Some(Stmt::Private(_))), "{:?}", write.body);
    assert!(!server.lookup_function("plain").unwrap().private);
    let client = ark::ir::module_from_value(&ark::canon::decode(&m.emit()).unwrap()).unwrap();
    let cw = client.lookup_function("write").unwrap();
    assert!(cw.private, "the client's write says private");
    assert!(!ark::ir::has_private(&cw.body), "and carries no block: {:?}", cw.body);
    let shown = format!("{:?}", cw.body);
    assert!(!shown.contains("audit"), "nothing of the server half reaches a client: {shown}");
    let by_name = |cs: BTreeMap<FnHash, Closure>| -> BTreeMap<String, FnHash> { cs.into_iter().map(|(h, c)| (c.function.name, h)).collect() };
    assert_eq!(by_name(closures(server)), by_name(closures(&client)), "one hash per function, in both");
    assert_ne!(m.hash(), m.server_hash(), "two modules");
    assert_eq!(m.hash(), ark::hash::module_hash(&client), "the public hash is the client module's");
    // Hashed whole, `write` would be another function.
    let whole = ark::canon::encode(&ark::ir::function_value(&BTreeMap::new(), write));
    let h = by_name(closures(server))["write"].clone();
    assert_ne!(ark::sha256::sha256(&whole), h);
    assert!(ark::verify::verify(&client).is_ok(), "the client's module verifies as it is");
}

/// The verifier holds a block to a mutator's body, one level deep, with no
/// `return` in it, and a function carrying one to saying so. Falsified by
/// letting `private: false` pass with a block (the `PrivateUndeclared`
/// check removed): the undeclared module verified.
#[test]
fn the_verifier_holds_a_private_block_to_a_mutator() {
    use ark::verify::{Complaint, VerifyError};
    let server = module().build().clone();
    let complaint = |f: &dyn Fn(&mut ark::ir::Function)| -> Vec<Complaint> {
        let mut m = server.clone();
        for g in m.functions.iter_mut().filter(|g| g.name == "write") {
            f(g);
        }
        match ark::verify::verify(&m) {
            Ok(_) => vec![],
            Err(es) => es
                .into_iter()
                .filter_map(|e| match e {
                    VerifyError::In(_, c) => Some(c),
                    _ => None,
                })
                .collect(),
        }
    };
    assert_eq!(complaint(&|f| f.private = false), vec![Complaint::PrivateUndeclared]);
    assert_eq!(
        complaint(&|f| {
            let inner = f.body[0].clone();
            if let Stmt::Private(b) = &mut f.body[0] {
                b.push(inner);
            }
        }),
        vec![Complaint::NestedPrivate]
    );
    assert_eq!(
        complaint(&|f| {
            if let Stmt::Private(b) = &mut f.body[0] {
                b.push(Stmt::Return(None));
            }
        }),
        vec![Complaint::ReturnInPrivate]
    );
    let mut q = server.clone();
    for g in q.functions.iter_mut().filter(|g| g.name == "plain") {
        g.private = true;
        g.kind = ark::ir::FnKind::Helper;
        g.router = None;
        g.autos.clear();
        g.input.clear();
        g.ret = Some(ark::schema::Ty::Int);
        g.body = vec![
            Stmt::Private(vec![]),
            Stmt::Return(Some(ark::ir::Expr::Std(StdFn::Len, vec![ark::ir::Expr::List(vec![])]))),
        ];
    }
    q.routers.iter_mut().for_each(|r| r.uses.clear());
    let es = ark::verify::verify(&q).unwrap_err();
    assert!(
        es.iter()
            .any(|e| matches!(e, VerifyError::In(n, Complaint::PrivateOutsideMutator) if n == "plain")),
        "{es:?}"
    );
}

/// Private runs last, at the authority alone, natively and through the
/// interpreter alike: on a device `write` writes the note and nothing else;
/// as the authority it writes the note, then the audit row — whose block,
/// written first, finds the note there — and the two runs agree, verdict,
/// changes and store. Its refusal is the entry's verdict, at the authority
/// and nowhere else. Falsified by running the block where it is written
/// (eval's `Private` executing its block at once): the interpreter refused
/// "a private block ran before its body" and disagreed with the native run;
/// and by not deferring natively (the closure called at once): the same,
/// the other way round.
#[test]
fn private_runs_last_and_only_at_the_authority() {
    let m = module();
    let (_, p) = m.procedure("write").unwrap();
    let st = MemoryStore::empty(m.build().schema.clone());
    let device = eval::Ctx::new("alice", "dev");
    let authority = device.clone().as_authority(true);
    let on_device = p.agrees(&device, &autos(1), &says("hello"), &st).unwrap().unwrap().unwrap();
    assert_eq!(on_device.len(), 1, "the note alone: {on_device:?}");
    let at_authority = p.agrees(&authority, &autos(1), &says("hello"), &st).unwrap().unwrap().unwrap();
    assert_eq!(at_authority.len(), 2, "the note, then the audit: {at_authority:?}");
    assert_eq!(at_authority[0], on_device[0], "the body is the device's");
    assert_eq!(at_authority[1].table(), "audit");
    let refused = p.agrees(&authority, &autos(2), &says("secret"), &st).unwrap();
    assert_eq!(refused, Ok(Err(Refusal::Refused("the server keeps that one".into()))));
    assert!(
        matches!(p.agrees(&device, &autos(2), &says("secret"), &st), Ok(Ok(Ok(_)))),
        "the device previews it"
    );
}

struct Fleet {
    sv: Server<Silent>,
    clients: Vec<Client>,
}

impl Fleet {
    // A server over the module with its blocks, and clients built from the
    // module a client loads — two hold natives, so both paths are run.
    fn new(n: usize) -> (Fleet, FnHash, FnHash) {
        let m = module();
        let built = m.build().clone();
        let bodies = closures(&built);
        let mut a = Authority::new(built.schema.clone(), bodies.clone());
        a.hold(m.procedures());
        let sv = Server::open(trusting(), open_access(), Silent, a);
        let theirs = ark::sim::client_bodies(&bodies);
        let clients = (0..n)
            .map(|i| {
                let r = Replica::open(built.schema.clone(), theirs.clone(), MemoryStore::empty(built.schema.clone()), 0, vec![]);
                let mut c = Client::open(r, Mode::Whole, Some(["alice", "bob", "carol"][i].into()));
                if i == 1 {
                    c.hold(&m.procedures());
                }
                c.connected();
                c
            })
            .collect();
        let mut f = Fleet { sv, clients };
        f.pump();
        let (w, _) = m.procedure("write").unwrap();
        let (pl, _) = m.procedure("plain").unwrap();
        (f, w, pl)
    }

    fn pump(&mut self) {
        for _ in 0..4 {
            for (i, c) in self.clients.iter_mut().enumerate() {
                for f in c.take_outgoing() {
                    self.sv.recv(i as i64 + 1, f);
                }
            }
            for (to, f) in self.sv.take_outgoing() {
                let c = &mut self.clients[to as usize - 1];
                c.recv(f);
            }
            for c in &mut self.clients {
                c.settle();
            }
        }
    }
}

/// The facts path: alice previews `write` — the note, no audit — and is
/// confirmed by the acknowledgement's facts, which are the whole run's, with
/// nothing recorded as a divergence; bob, who replays (natively), and carol,
/// who replays through the interpreter, take the entry by the facts the page
/// carries for it and for nothing else; every peer is at the authority's
/// hash. Falsified twice: the server's `is_private` answering false — the
/// acknowledgement carried no facts; and the replica's answering false —
/// alice's preview, compared with the facts, was recorded as a divergence.
#[test]
fn a_private_entry_is_confirmed_by_the_whole_runs_facts() {
    let (mut f, write, plain) = Fleet::new(3);
    let alice = eval::Ctx::new("alice", "dev");
    f.clients[0].mutate([1; 16], &alice, &write, &autos(1), &says("hello")).unwrap();
    f.clients[0]
        .mutate([2; 16], &alice, &plain, &args([("id", idv(2))]), &says("plain"))
        .unwrap();
    assert_eq!(f.clients[0].replica.view.scan("audit").len(), 0, "the preview has no audit");
    // What the server says, as it says it.
    for m in f.clients[0].take_outgoing() {
        f.sv.recv(1, m);
    }
    let said = f.sv.take_outgoing();
    let ack_facts: Vec<i64> = said
        .iter()
        .filter_map(|(to, m)| match m {
            ServerMsg::Ack { facts, .. } if *to == 1 => Some(facts.iter().map(|(n, _)| *n).collect::<Vec<_>>()),
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(ack_facts, vec![1], "the ack carries the private entry's facts alone");
    let paged: Vec<(i64, bool)> = said
        .iter()
        .filter_map(|(to, m)| match m {
            ServerMsg::Batch { items, .. } if *to == 2 => Some(items.iter().map(|(n, _, f)| (*n, f.is_some())).collect::<Vec<_>>()),
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(
        paged,
        vec![(1, true), (2, false)],
        "a replaying peer is sent the private entry's facts alone"
    );
    for (to, m) in said {
        f.clients[to as usize - 1].recv(m);
    }
    for c in &mut f.clients {
        c.settle();
    }
    f.pump();
    let want = (2, state_hash(&f.sv.authority.store));
    assert_eq!(f.sv.authority.store.scan("audit").len(), 1);
    for (i, c) in f.clients.iter().enumerate() {
        assert!(c.replica.pending.is_empty(), "client {i}: {:?}", c.replica.pending);
        assert!(c.replica.diverged.is_empty(), "client {i} diverged at {:?}", c.replica.diverged);
        assert_eq!(c.replica.verify_at(), want, "client {i}");
        assert_eq!(c.replica.view.scan("audit").len(), 1, "client {i}'s view holds the audit row");
    }
}

/// A private block's refusal is the entry's verdict: alice's preview of the
/// note is undone when the verdict arrives, nothing is logged, and no peer
/// ever holds it. Falsified by a server whose authority runs no private
/// block (`Server::open` leaving `private` false): the note is logged.
#[test]
fn a_private_refusal_lands_nowhere() {
    let (mut f, write, _) = Fleet::new(2);
    let alice = eval::Ctx::new("alice", "dev");
    f.clients[0].mutate([1; 16], &alice, &write, &autos(1), &says("secret")).unwrap();
    assert_eq!(f.clients[0].replica.view.scan("note").len(), 1, "previewed");
    f.pump();
    assert_eq!(f.sv.authority.log.head_seq(), 0, "nothing logged");
    let c = &f.clients[0];
    assert!(c.replica.pending.is_empty());
    assert_eq!(c.replica.view.scan("note").len(), 0, "the preview is undone");
    let why: Vec<&Refusal> = c.replica.rejections.iter().map(|(_, r)| r).collect();
    assert_eq!(why, vec![&Refusal::Refused("the server keeps that one".into())]);
    assert_eq!(f.clients[1].replica.verify_at(), (0, state_hash(&f.sv.authority.store)));
}

/// A closure is asked for and sent as a client may hold it: stripped, under
/// the hash it was asked by. Falsified by sending the server's closure
/// whole: the block arrives with it.
#[test]
fn a_closure_is_sent_stripped() {
    let (mut f, write, _) = Fleet::new(1);
    f.sv.recv(1, ClientMsg::NeedClosures { hashes: vec![write.clone()] });
    let sent = f.sv.take_outgoing();
    let Some((_, ServerMsg::Closures { items })) = sent.iter().find(|(_, m)| matches!(m, ServerMsg::Closures { .. })) else {
        panic!("{sent:?}")
    };
    let (h, c) = &items[0];
    assert_eq!(h, &write);
    assert!(c.function.private && !ark::ir::has_private(&c.function.body), "{:?}", c.function.body);
    assert_eq!(ark::hash::function_hash(c), write, "under the hash it was asked by");
}
