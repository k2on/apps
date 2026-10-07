//! `docs/plan-guards.md` D1: `ctx.has_role(..)` in a guard, a provide, a
//! body and a check, natively and through the interpreter; and what the
//! authority's stamp does to an entry whose body reads a role its device
//! believed wrongly.

// The row structs are the vocabulary's: a body reads their fields only
// natively.
#![allow(dead_code)]

use ark::authoring::*;
use ark::eval::{self, Args};
use ark::hash::{closures, state_hash};
use ark::ir::{Expr, Stmt};
use ark::live::Silent;
use ark::peer::{Authority, Replica};
use ark::protocol::{open_access, trusting, Client, Mode, Server, ServerMsg};
use ark::store::{MemoryStore, Refusal, Store};
use ark::value::Value;

pub struct Notes {
    pub note: Table<Note>,
}
impl Tables for Notes {
    fn open() -> Self {
        Notes { note: table() }
    }
}

pub struct Note {
    pub id: Id<Note>,
    pub text: Text,
    pub by: Text,
    pub flagged: Bool,
}
impl Row for Note {
    const NAME: &str = "note";
    type Key = (Id<Note>,);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::id)
            .text(Self::text)
            .text(Self::by)
            .bool(Self::flagged)
            .key((Self::id,))
    }
}
#[allow(non_upper_case_globals)]
impl Note {
    pub const id: Col<Self, Id<Self>> = col("id");
    pub const text: Col<Self, Text> = col("text");
    pub const by: Col<Self, Text> = col("by");
    pub const flagged: Col<Self, Bool> = col("flagged");
}

pub struct Write {
    pub text: Text,
}
impl Input for Write {
    fn schema() -> Object<Self> {
        // A check asking a role: a long note is a `long` writer's.
        object()
            .field("text", text().min(1))
            .refine(|i: &Write| i.text.len().le(10).or(ctx().has_role("long")))
            .why("too long a note")
    }
}

/// A guard (`writer`), a provide (`admin` signs as the admins), a body
/// (`flagger` flags it) and a check (`long`), each asking one role.
fn module() -> Module {
    let notes = router::<Notes>("notes");
    let is_writer = notes.guard("is_writer", |ctx, _db| {
        unless(ctx.has_role("writer"), || refuse("only a writer writes a note"))
    });
    let signed = is_writer.provide("signed", |ctx, _db, _input: &()| pick(ctx.has_role("admin"), "the admins", ctx.user));
    Module::new((notes.routes((signed.input::<Write>().mutation("write", |ctx, db, input, signed| {
        db.note.insert(Note {
            id: ctx.new_id("id"),
            text: input.text,
            by: signed,
            flagged: ctx.has_role("flagger"),
        })
    }),)),))
}

fn args<const N: usize>(pairs: [(&str, Value); N]) -> Args {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn idv(k: u8) -> Value {
    let mut b = [0u8; 16];
    b[15] = k;
    Value::Id(b)
}

/// Every role asked in each of the four places it can be: the module
/// emits `HasRole` in the guard, the provide, the body and the check; and
/// natively and through the interpreter the same verdict, the same row,
/// for authors holding every subset of the four roles. Falsified by
/// evaluating `HasRole` as false in `eval` (the interpreter refused every
/// writer: native and interpreted disagree), and by reading no role
/// natively (`has_role` answering `false` under `Native`): likewise.
#[test]
fn a_role_is_asked_alike_natively_and_interpreted() {
    let m = module();
    let built = m.build();
    let shown = format!("{:?}", built.functions);
    for r in ["writer", "admin", "flagger", "long"] {
        assert!(shown.contains(&format!("HasRole(\"{r}\")")), "{r} is asked in the IR");
    }
    let write = built.lookup_function("write").unwrap();
    assert!(
        matches!(write.body.first(), Some(Stmt::Insert(_, Expr::Struct(fs), _)) if matches!(fs.get("flagged"), Some(Expr::HasRole(r)) if r == "flagger")),
        "{:?}",
        write.body
    );
    let (_, p) = m.procedure("write").unwrap();
    let st = MemoryStore::empty(built.schema.clone());
    let roles = ["writer", "admin", "flagger", "long"];
    for mask in 0..16u8 {
        let held: Vec<&str> = roles.iter().enumerate().filter(|(i, _)| mask & (1 << i) != 0).map(|(_, r)| *r).collect();
        let ctx = eval::Ctx::new("alice", "a").with_roles(held.iter().copied());
        for text in ["short", "a rather longer note"] {
            let out = p
                .agrees(&ctx, &args([("id", idv(1))]), &args([("text", Value::text(text))]), &st)
                .unwrap_or_else(|e| panic!("{held:?}: {e}"));
            let has = |r: &str| held.contains(&r);
            let want = if text.len() > 10 && !has("long") {
                Err(Refusal::Refused("too long a note".into()))
            } else if !has("writer") {
                Err(Refusal::Refused("only a writer writes a note".into()))
            } else {
                Ok(())
            };
            match (out, want) {
                (Ok(Ok(chs)), Ok(())) => {
                    let [ark::store::Change::Add(_, row)] = chs.as_slice() else {
                        panic!("{chs:?}")
                    };
                    let by = if has("admin") { "the admins" } else { "alice" };
                    assert_eq!(
                        (row.get("by"), row.get("flagged")),
                        (Some(&Value::text(by)), Some(&Value::Bool(has("flagger"))))
                    );
                }
                (Ok(Err(got)), Err(want)) => assert_eq!(got, want, "{held:?} {text}"),
                (got, want) => panic!("{held:?} {text}: {got:?}, wanted {want:?}"),
            }
        }
    }
    // The form validator asks the check's role too.
    let long = args([("text", Value::text("a rather longer note"))]);
    let plain = eval::Ctx::new("alice", "a");
    assert_eq!(
        p.check(&plain, &long, &st).unwrap().messages,
        vec![("".to_string(), "too long a note".to_string())]
    );
    assert!(p.check(&plain.clone().with_roles(["long"]), &long, &st).unwrap().messages.is_empty());
}

/// The stamp, where it decides a row: a device that believes `flagger`
/// under a login holding only `writer` previews a flagged note; the
/// authority logs the entry with `writer` and writes it unflagged. The
/// device, confirmed by the acknowledgement and the facts it carries — the page
/// lost — holds the authority's row and its hash, its preview recorded as
/// the divergence it was; a peer fed the log replays the stamped entry to
/// the same. A device without `writer` is refused on itself; one that
/// believes it wrongly is refused by the server, and nothing is logged.
/// Falsified by acknowledging without the facts: the device keeps
/// its flagged preview and its hash is not the authority's.
#[test]
fn the_stamp_decides_what_a_body_reading_a_role_writes() {
    let m = module();
    let built = m.build().clone();
    let bodies = closures(&built);
    let (fh, _) = m.procedure("write").unwrap();
    let mut a = Authority::new(built.schema.clone(), bodies.clone());
    a.hold(m.procedures());
    let mut sv = Server::open(trusting(), open_access(), Silent, a);
    let replica = || Replica::open(built.schema.clone(), bodies.clone(), MemoryStore::empty(built.schema.clone()), 0, vec![]);
    let pump = |sv: &mut Server<Silent>, c: &mut Client, conn: i64, keep: &dyn Fn(&ServerMsg) -> bool| {
        for f in c.take_outgoing() {
            sv.recv(conn, f);
        }
        for (to, f) in sv.take_outgoing() {
            if to == conn && keep(&f) {
                c.recv(f);
            }
        }
        c.settle();
    };
    let note = |text: &str| args([("text", Value::text(text))]);

    let mut dev = Client::open(replica(), Mode::Whole, Some("alice:writer".into()));
    dev.connected();
    pump(&mut sv, &mut dev, 1, &|_| true);
    let believes = eval::Ctx::new("alice", "dev").with_roles(["writer", "flagger"]);
    dev.mutate([1; 16], &believes, &fh, &args([("id", idv(1))]), &note("hi")).unwrap();
    let previewed = dev.replica.view.scan("note");
    assert_eq!(
        previewed[0].get("flagged"),
        Some(&Value::Bool(true)),
        "the preview, as the device believes"
    );
    pump(&mut sv, &mut dev, 1, &|f| !matches!(f, ServerMsg::Batch { .. }));
    let logged = &sv.authority.log.entries[&1].0;
    assert_eq!(logged.roles, ["writer".to_string()].into(), "stamped");
    assert_eq!(sv.authority.store.scan("note")[0].get("flagged"), Some(&Value::Bool(false)));
    assert!(dev.replica.pending.is_empty());
    assert_eq!(dev.replica.diverged, vec![1], "the preview disagreed, and the authority's facts won");
    assert_eq!(dev.replica.verify_at(), (1, state_hash(&sv.authority.store)));

    let mut late = Client::open(replica(), Mode::Whole, Some("bob".into()));
    late.connected();
    pump(&mut sv, &mut late, 2, &|_| true);
    assert_eq!(
        late.replica.verify_at(),
        (1, state_hash(&sv.authority.store)),
        "replayed with the stamped roles"
    );
    assert!(late.replica.diverged.is_empty());

    // Without `writer`: refused on the device, never pending.
    let mut bob = Client::open(replica(), Mode::Whole, Some("bob".into()));
    bob.connected();
    pump(&mut sv, &mut bob, 3, &|_| true);
    let refused = bob.mutate([2; 16], &eval::Ctx::new("bob", "dev"), &fh, &args([("id", idv(2))]), &note("hey"));
    assert_eq!(refused, Err(Refusal::Refused("only a writer writes a note".into())));
    assert!(bob.replica.pending.is_empty());
    // Believing it wrongly: refused by the server, nothing logged.
    bob.mutate(
        [3; 16],
        &eval::Ctx::new("bob", "dev").with_roles(["writer"]),
        &fh,
        &args([("id", idv(3))]),
        &note("hey"),
    )
    .unwrap();
    pump(&mut sv, &mut bob, 3, &|_| true);
    let why: Vec<&Refusal> = bob.replica.rejections.iter().map(|(_, r)| r).collect();
    assert_eq!(why, vec![&Refusal::Refused("only a writer writes a note".into())]);
    assert_eq!(sv.authority.log.head_seq(), 1, "nothing logged");
    assert_eq!(bob.replica.verify_at(), (1, state_hash(&sv.authority.store)));
}

/// Found by `arkc fuzz` (seed 112): the facts of an entry the authority
/// stamped arrive with the log they are of. A device that has never been
/// told a log's name believes `writer`; its login holds `flagger` too,
/// which `write`'s body reads, so the authority acknowledges the entry with
/// its facts — and nothing it sends reaches the device. The server then
/// loses its log; on the new one somebody else's note is first; the device
/// comes back naming no log, is paged from 0, and must reach the new log's
/// hash. Falsified by sending the facts as a frame of their own ahead of
/// the acknowledgement as well, as the stamp first did, and losing only the
/// acknowledgement and the page: the facts wait in the inbox at sequence 1,
/// the new log's first entry is applied by them, and the device diverges.
#[test]
fn the_stamps_facts_arrive_with_the_log_they_are_of() {
    let m = module();
    let built = m.build().clone();
    let bodies = closures(&built);
    let (fh, _) = m.procedure("write").unwrap();
    let server = |name: u8| {
        let mut a = Authority::new(built.schema.clone(), bodies.clone());
        a.hold(m.procedures());
        a.log.name_if_unnamed([name; 16]);
        Server::open(trusting(), open_access(), Silent, a)
    };
    let replica = || Replica::open(built.schema.clone(), bodies.clone(), MemoryStore::empty(built.schema.clone()), 0, vec![]);
    let note = |text: &str| args([("text", Value::text(text))]);

    let mut a = server(0xa1);
    let mut dev = Client::open(replica(), Mode::Whole, Some("bob:writer,flagger".into()));
    dev.connected();
    dev.mutate(
        [1; 16],
        &eval::Ctx::new("bob", "dev").with_roles(["writer"]),
        &fh,
        &args([("id", idv(1))]),
        &note("mine"),
    )
    .unwrap();
    for f in dev.take_outgoing() {
        a.recv(1, f);
    }
    assert_eq!(a.authority.log.entries[&1].0.roles.len(), 2, "stamped");
    // Everything but the acknowledgement and the page reaches it.
    for (_, f) in a.take_outgoing() {
        if !matches!(f, ServerMsg::Ack { .. } | ServerMsg::Batch { .. }) {
            dev.recv(f);
        }
    }
    dev.settle();
    dev.disconnected();
    assert_eq!((dev.replica.cursor, dev.replica.log_id), (0, None), "it has heard no log");

    // The server comes back without its log; somebody else is first.
    let mut b = server(0xb2);
    let mut alice = Client::open(replica(), Mode::Whole, Some("alice:writer".into()));
    alice.connected();
    alice
        .mutate(
            [2; 16],
            &eval::Ctx::new("alice", "dev").with_roles(["writer"]),
            &fh,
            &args([("id", idv(2))]),
            &note("hers"),
        )
        .unwrap();
    for f in alice.take_outgoing() {
        b.recv(2, f);
    }
    let _ = b.take_outgoing();
    dev.connected();
    for _ in 0..4 {
        for f in dev.take_outgoing() {
            b.recv(1, f);
        }
        for (to, f) in b.take_outgoing() {
            if to == 1 {
                dev.recv(f);
            }
        }
        dev.settle();
    }
    assert_eq!(b.authority.log.head_seq(), 2);
    assert!(dev.replica.pending.is_empty(), "{:?}", dev.replica.pending);
    assert!(dev.replica.diverged.is_empty(), "{:?}", dev.replica.diverged);
    assert_eq!(dev.replica.verify_at(), (2, state_hash(&b.authority.store)));
}
