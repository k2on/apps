//! Who is signed in, as rows: a session is its person's to end, a role an
//! admin's to grant, `session.current` the caller's live session and
//! `session.feed` holds nobody else's.

mod common;

use ark::format::Encode;
use ark::log::EntryId;
use ark::peer::Ctx;
use ark::store::Change;
use common::{domain, Lib};
use harken_domain::session::{Grant, Revoke, Role, SignIn};

fn signed_in(lib: &mut Lib, id: &str, user: &str, expires_ms: i64) {
    let s = SignIn {
        id: id.into(),
        user: user.into(),
        name: user.into(),
        expires_ms,
        roles: vec!["library".into()],
        ..SignIn::default()
    };
    let c = Change::Add {
        table: "sign_in".into(),
        row: s.to_bytes(),
    };
    lib.store.apply(&c).unwrap();
}

/// A context as `user` on `session` holding `roles`, at `now`.
fn on(user: &str, session: &str, roles: &[&str], now: i64) -> Ctx {
    Ctx {
        draws: Default::default(),
        id: EntryId::of(session, 1),
        actor: user.into(),
        session: session.into(),
        roles: roles.iter().map(|r| r.to_string()).collect(),
        now,
    }
}

/// Author `name` (by its whole binding) as `ctx`: the refusal's text.
fn run(lib: &mut Lib, ctx: &Ctx, name: &str, input: &impl Encode) -> Result<(), String> {
    let f = domain().get(name).and_then(|f| f.proc.clone()).unwrap();
    f.apply(&mut lib.store, ctx, &input.to_bytes()).map(|_| ()).map_err(|r| r.0)
}

fn current(lib: &Lib, ctx: &Ctx) -> Option<SignIn> {
    let b = domain().ask(&lib.store, ctx, "session.current", &[]).unwrap();
    Option::<SignIn>::from_bytes(&b).unwrap()
}

/// Only its person ends a session, and an ended one proves nobody.
/// Falsified by `revoke` not asking whose the row is: bob's revoke of
/// alice's session went through.
#[test]
fn a_session_is_its_persons_to_end() {
    let mut lib = Lib::new();
    signed_in(&mut lib, "a1", "alice", 10_000);
    let alice = on("alice", "a1", &[], 2_000);
    assert_eq!(current(&lib, &alice).map(|s| s.user), Some("alice".into()));
    let bob = on("bob", "b1", &[], 2_000);
    let refused = run(&mut lib, &bob, "session.revoke", &Revoke { id: "a1".into() });
    assert_eq!(refused, Err("not your session".into()));
    assert!(current(&lib, &alice).is_some(), "bob ended nothing");
    run(&mut lib, &alice, "session.revoke", &Revoke { id: "a1".into() }).unwrap();
    assert_eq!(current(&lib, &alice), None, "revoked");
    let row: Vec<SignIn> = lib.rows("sign_in");
    assert!(row[0].revoked, "kept, revoked");
}

/// A session proves nobody once it has expired, and holds what it was
/// opened with and what was granted its person. Falsified by `live` not
/// asking the clock: the expired session was still current.
#[test]
fn current_is_the_live_session_with_its_grants() {
    let mut lib = Lib::new();
    signed_in(&mut lib, "a1", "alice", 10_000);
    let admin = on("root", "r1", &["admin"], 2_000);
    run(
        &mut lib,
        &admin,
        "role.grant",
        &Grant {
            user: "alice".into(),
            role: "admin".into(),
        },
    )
    .unwrap();
    let s = current(&lib, &on("alice", "a1", &[], 2_000)).unwrap();
    assert_eq!(s.roles, vec!["admin".to_string(), "library".into()]);
    assert_eq!(current(&lib, &on("alice", "a1", &[], 10_000)), None, "expired");
    assert_eq!(current(&lib, &on("alice", "nope", &[], 2_000)), None, "no such session");
}

/// Only an admin grants or takes back a role. Falsified by `admin` letting
/// anybody: bob granted himself admin.
#[test]
fn only_an_admin_grants() {
    let mut lib = Lib::new();
    let bob = on("bob", "b1", &["library"], 2_000);
    let grant = Grant {
        user: "bob".into(),
        role: "admin".into(),
    };
    assert_eq!(run(&mut lib, &bob, "role.grant", &grant), Err("only an admin grants roles".into()));
    assert!(lib.rows::<Role>("role").is_empty());
    let admin = on("root", "r1", &["admin"], 2_000);
    run(&mut lib, &admin, "role.grant", &grant).unwrap();
    assert_eq!(
        lib.rows::<Role>("role"),
        vec![Role {
            user: "bob".into(),
            role: "admin".into()
        }]
    );
    assert_eq!(run(&mut lib, &bob, "role.revoke", &grant), Err("only an admin grants roles".into()));
    run(&mut lib, &admin, "role.revoke", &grant).unwrap();
    assert!(lib.rows::<Role>("role").is_empty());
}

/// A connection fed `session.feed` holds its person's sessions and roles
/// and nobody else's, and the library whole; `session.mine` lists the
/// caller's live ones. Falsified by the feed holding every session: bob's
/// was held for alice.
#[test]
fn nobody_else_s_sessions_are_held_or_listed() {
    let mut lib = Lib::new();
    signed_in(&mut lib, "a1", "alice", 10_000);
    signed_in(&mut lib, "a2", "alice", 10_000);
    signed_in(&mut lib, "b1", "bob", 10_000);
    let alice = on("alice", "a1", &[], 2_000);
    let feed = domain().functions().get("session.feed").cloned().unwrap();
    let (at, _) = domain().schema.table("sign_in").unwrap();
    let held: Vec<String> = lib
        .rows::<SignIn>("sign_in")
        .into_iter()
        .filter(|s| feed.holds(&alice, &[&[at as u8][..], &s.to_bytes()].concat()).unwrap())
        .map(|s| s.id)
        .collect();
    assert_eq!(held, vec!["a1".to_string(), "a2".into()]);
    let (media, _) = domain().schema.table("media").unwrap();
    let row = harken_domain::Media::default().to_bytes();
    assert!(feed.holds(&alice, &[&[media as u8][..], &row].concat()).unwrap(), "the library whole");
    let b = domain().ask(&lib.store, &alice, "session.mine", &[]).unwrap();
    let mine = Vec::<SignIn>::from_bytes(&b).unwrap();
    assert_eq!(mine.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), vec!["a1", "a2"]);
}
