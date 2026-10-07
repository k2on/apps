//! `docs/plan-guards.md` D4: every table's CRUD is one line on a router.
//! `r.crud::<T>()` emits `insert_<t>`, `update_<t>`, `delete_<t>` and
//! `put_<t>` — ordinary mutations, byte for byte what the same lines written
//! by hand emit — under the router's middleware; `.except(..)` leaves one out
//! to be written by hand; and a table a scope projects refuses the generated
//! writes with an error that says what to do.

// The row structs are the vocabulary's: a body reads their fields only
// natively.
#![allow(dead_code)]

use ark::authoring::*;
use ark::eval::{self, Args};
use ark::hash::{closures, module_hash};
use ark::ir::FnKind;
use ark::store::{MemoryStore, Refusal, Store};
use ark::value::Value;

pub struct People {
    pub team: Table<Team>,
    pub user: Table<User>,
}
impl Tables for People {
    fn open() -> Self {
        People {
            team: table(),
            user: table(),
        }
    }
}

pub struct Team {
    pub name: Text,
}
impl Row for Team {
    const NAME: &str = "team";
    type Key = (Text,);
    fn columns() -> Columns<Self> {
        columns().text(Self::name).key((Self::name,))
    }
}
#[allow(non_upper_case_globals)]
impl Team {
    pub const name: Col<Self, Text> = col("name");
}

/// A row with every shape a column has that matters here: an id key, a
/// reference, a nullable column.
pub struct User {
    pub id: Id<User>,
    pub name: Text,
    pub team: Text,
    pub email: Opt<Text>,
}
impl Row for User {
    const NAME: &str = "user";
    type Key = (Id<User>,);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::id)
            .text(Self::name)
            .text(Self::team)
            .refs::<Team>()
            .text(Self::email)
            .nullable()
            .key((Self::id,))
    }
}
#[allow(non_upper_case_globals)]
impl User {
    pub const id: Col<Self, Id<Self>> = col("id");
    pub const name: Col<Self, Text> = col("name");
    pub const team: Col<Self, Text> = col("team");
    pub const email: Col<Self, Opt<Text>> = col("email");
}

/// What the four hand-written mutations take: a user's columns, in order.
pub struct UserIn {
    pub id: Id<User>,
    pub name: Text,
    pub team: Text,
    pub email: Opt<Text>,
}
impl Input for UserIn {
    fn schema() -> Object<Self> {
        object()
            .field("id", id::<User>())
            .field("name", text())
            .field("team", text())
            .field("email", opt(text()))
    }
}

/// …and what a delete takes: the key's.
pub struct UserKey {
    pub id: Id<User>,
}
impl Input for UserKey {
    fn schema() -> Object<Self> {
        object().field("id", id::<User>())
    }
}

fn row(i: UserIn) -> User {
    User {
        id: i.id,
        name: i.name,
        team: i.team,
        email: i.email,
    }
}

/// The router both modules share: a guard, so the generated writes are seen
/// to run the router's middleware like any procedure on it.
fn people() -> (Router<People>, Router<People>) {
    let people = router::<People>("people");
    let signed_in = people.guard("signed_in", |ctx, _db| when(ctx.user.is_empty(), || refuse("sign in first")));
    (people, signed_in)
}

/// The module with `crud`.
fn generated() -> Module {
    let (people, signed_in) = people();
    Module::new(people.routes((signed_in.crud::<Team>(), signed_in.crud::<User>())))
}

/// The same eight mutations, written by hand: one line each, and the two
/// refusals a CRUD verb has.
fn handwritten() -> Module {
    let (people, signed_in) = people();
    let team = |n: &str| format!("team: {n}");
    let (exists, missing) = (team("a row with this key exists"), team("no row with this key"));
    Module::new(
        people.routes((
            signed_in.input::<TeamIn>().mutation("insert_team", move |_ctx, db, input| {
                when(db.team.exists((input.name,)), || refuse(exists.as_str()));
                db.team.insert(Team { name: input.name })
            }),
            signed_in.input::<TeamIn>().mutation("update_team", move |_ctx, db, input| {
                unless(db.team.exists((input.name,)), || refuse(missing.as_str()));
                db.team.update((input.name,), |_| Team { name: input.name })
            }),
            signed_in
                .input::<TeamIn>()
                .mutation("delete_team", |_ctx, db, input| db.team.delete((input.name,))),
            signed_in
                .input::<TeamIn>()
                .mutation("put_team", |_ctx, db, input| db.team.upsert(Team { name: input.name })),
            signed_in.input::<UserIn>().mutation("insert_user", |_ctx, db, input| {
                when(db.user.exists((input.id,)), || refuse("user: a row with this key exists"));
                db.user.insert(row(input))
            }),
            signed_in.input::<UserIn>().mutation("update_user", |_ctx, db, input| {
                let key = (input.id,);
                unless(db.user.exists(key), || refuse("user: no row with this key"));
                db.user.update((input.id,), |_| row(input))
            }),
            signed_in
                .input::<UserKey>()
                .mutation("delete_user", |_ctx, db, input| db.user.delete((input.id,))),
            signed_in
                .input::<UserIn>()
                .mutation("put_user", |_ctx, db, input| db.user.upsert(row(input))),
        )),
    )
}

pub struct TeamIn {
    pub name: Text,
}
impl Input for TeamIn {
    fn schema() -> Object<Self> {
        object().field("name", text())
    }
}

/// What `crud` emits is what the lines written by hand emit, function by
/// function and so module by module: the same IR, the same closure hashes,
/// the same module hash. Nothing in the IR says where a function came from.
/// Falsified by dropping the insert's refusal from the generated body: the
/// two `insert_team`s were different values.
#[test]
fn crud_emits_what_the_same_lines_by_hand_emit() {
    let (g, h) = (generated(), handwritten());
    let (gm, hm) = (g.build(), h.build());
    let names: Vec<&str> = gm.functions.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "signed_in",
            "insert_team",
            "update_team",
            "delete_team",
            "put_team",
            "insert_user",
            "update_user",
            "delete_user",
            "put_user"
        ]
    );
    for f in &gm.functions {
        let by_hand = hm.lookup_function(&f.name).expect("written by hand too");
        assert_eq!(f, by_hand, "{}: generated and written by hand differ", f.name);
        if f.kind == FnKind::Mutator {
            assert_eq!(f.uses, ["signed_in"], "{} runs the router's middleware", f.name);
        }
    }
    let delete = gm.lookup_function("delete_user").unwrap();
    assert_eq!(
        delete.input.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
        ["id"],
        "a delete takes the key"
    );
    let put = gm.lookup_function("put_user").unwrap();
    assert_eq!(
        put.input.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
        ["id", "name", "team", "email"],
        "a write takes the row's columns, in order"
    );
    assert_eq!(closures(gm).keys().collect::<Vec<_>>(), closures(hm).keys().collect::<Vec<_>>());
    assert_eq!(module_hash(gm), module_hash(hm));
    assert_eq!(g.emit(), h.emit());
}

fn user(id: u8, name: &str, team: &str, email: Option<&str>) -> Args {
    let mut a = Args::new();
    a.insert("id".into(), Value::Id([id; 16]));
    a.insert("name".into(), Value::text(name));
    a.insert("team".into(), Value::text(team));
    a.insert("email".into(), email.map_or(Value::Null, Value::text));
    a
}

/// Each verb does what its name says, natively and through the
/// interpreter alike (`Procedure::agrees` on every call): insert refuses a
/// key that is there, update one that is not, put does either, delete is
/// judged by the store's constraints, and the router's guard runs first.
/// Falsified by making the update's condition `when` rather than `unless`:
/// the first update of a row that is there was refused.
#[test]
fn each_verb_does_what_it_says() {
    let m = generated();
    let ctx = eval::Ctx::new("alice", "s1");
    let procs = m.procedures();
    let call = |name: &str, ctx: &eval::Ctx, args: Args, st: &mut MemoryStore| {
        let (_, p) = procs.iter().find(|(_, p)| p.name() == name).expect("a generated procedure");
        let out = p.agrees(ctx, &Args::new(), &args, st).unwrap_or_else(|e| panic!("{e}"));
        if let Ok(Ok(chs)) = &out {
            st.apply_changes(chs);
        }
        out.expect("no bug")
    };
    let mut st = MemoryStore::empty(m.build().schema.clone());
    let mut team = Args::new();
    team.insert("name".into(), Value::text("blue"));
    assert!(call("insert_team", &ctx, team.clone(), &mut st).is_ok());
    assert_eq!(
        call("insert_user", &eval::Ctx::new("", ""), user(1, "ann", "blue", None), &mut st),
        Err(Refusal::Refused("sign in first".into())),
        "the router's guard runs first"
    );
    assert!(call("insert_user", &ctx, user(1, "ann", "blue", None), &mut st).is_ok());
    assert_eq!(
        call("insert_user", &ctx, user(1, "bob", "blue", None), &mut st),
        Err(Refusal::Refused("user: a row with this key exists".into()))
    );
    assert_eq!(
        call("update_user", &ctx, user(2, "bob", "blue", None), &mut st),
        Err(Refusal::Refused("user: no row with this key".into()))
    );
    assert!(call("update_user", &ctx, user(1, "ann", "blue", Some("ann@example.com")), &mut st).is_ok());
    assert_eq!(st.get("user", &[Value::Id([1; 16])]).unwrap()["email"], Value::text("ann@example.com"));
    assert!(call("put_user", &ctx, user(2, "bob", "blue", None), &mut st).is_ok());
    assert_eq!(
        call("put_user", &ctx, user(3, "cat", "red", None), &mut st),
        Err(Refusal::MissingParent("user".into(), "team".into(), "team".into())),
        "judged by the store's constraints"
    );
    assert_eq!(
        call("delete_team", &ctx, team, &mut st),
        Err(Refusal::StillReferenced("team".into(), "user".into()))
    );
    let mut key = Args::new();
    key.insert("id".into(), Value::Id([2; 16]));
    assert!(call("delete_user", &ctx, key, &mut st).is_ok());
    assert_eq!(st.scan("user").len(), 1);
}

/// `.except(..)` leaves a generated mutation out, so one written by hand can
/// take its name — a new hash for the new body, and the other three as they
/// were. Falsified by ignoring the exclusion: the build refused two
/// functions named `insert_user`.
#[test]
fn a_hand_written_mutation_replaces_the_generated_one() {
    let (people, signed_in) = people();
    let grown = Module::new(people.routes((
        signed_in.crud::<Team>(),
        signed_in.crud::<User>().except(&["insert_user"]),
        signed_in.input::<UserIn>().mutation("insert_user", |_ctx, db, input| {
            when(db.user.exists((input.id,)), || refuse("user: a row with this key exists"));
            db.user.insert(User {
                id: input.id,
                name: input.name.trim(),
                team: input.team,
                email: input.email,
            })
        }),
    )));
    let built = grown.try_build().unwrap_or_else(|es| panic!("{es:?}"));
    let base = generated();
    let base = base.build();
    let hash = |m: &ark::ir::Module, n: &str| {
        closures(m)
            .into_iter()
            .find(|(_, c)| c.function.name == n)
            .map(|(h, _)| h)
            .expect("the function")
    };
    assert_ne!(
        hash(built, "insert_user"),
        hash(base, "insert_user"),
        "the hand-written one is a new hash"
    );
    for n in ["update_user", "delete_user", "put_user", "insert_team"] {
        assert_eq!(hash(built, n), hash(base, n), "{n} is as it was");
    }
    assert_eq!(built.functions.iter().filter(|f| f.name == "insert_user").count(), 1);
}

/// A misspelt exclusion is said at once rather than left to a duplicate
/// name the build refuses without mentioning the typo. Falsified by not
/// checking the names: the call returned.
#[test]
#[should_panic(expected = "insrt_user is not one of insert_user, update_user, delete_user, put_user")]
fn an_exclusion_names_one_of_the_four() {
    let (_, signed_in) = people();
    let _ = signed_in.crud::<User>().except(&["insrt_user"]);
}

/// A table a scope projects: the generated writes name every column, so
/// for a person who does not hold one they are refused — rightly — and the
/// error says what to do instead. Falsified by leaving the advice out: the
/// message named the column and not `ctx.private`.
#[test]
fn a_projected_table_is_written_by_hand() {
    let people = router::<People>("people");
    let mine = people.server("mine", |_ctx, db| (db.team.rows(), db.user.exclude((User::email,))));
    let m = Module::new(people.routes((mine.crud::<Team>(), mine.crud::<User>())));
    let errors = m.try_build().map(|_| ()).expect_err("put_user names a column nobody holds");
    let about = errors.iter().find(|e| e.contains("put_user")).expect("put_user is refused");
    assert!(about.contains("NotHeld"), "{about}");
    assert!(about.contains("generated by `crud`"), "{about}");
    assert!(about.contains(".except(&[\"put_user\"])"), "{about}");
    assert!(about.contains("ctx.private"), "{about}");
    assert!(!errors.iter().any(|e| e.contains("_team")), "a table held whole is fine: {errors:?}");
}
