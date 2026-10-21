//! Who is signed in, as rows (S1), over a schema of the tests' own
//! (`common::App`): a session is its person's to end and a role an admin's
//! to grant; a token proves its session while the row is live and the
//! signature this server's; the hub's watch names what to close; a
//! connection holds nobody else's sign-ins. And the rules a code and a
//! login keep: where a code may go, once, with the roles it holds.

mod common;

use std::sync::Arc;

use ark::format::Encode;
use ark::peer::Who;
use ark::row::Row;
use ark::store::Change;
use ark_auth::server::{dev_account, Auth, Mode};
use ark_auth::{Account, Grant, Revoke, Role, SignIn};
use common::{domain, on, App, Memory};

/// Author `name` as `ctx` over the hub's store: the refusal's text.
fn run(hub: &Memory, ctx: &ark::peer::Ctx, name: &str, input: &impl Encode) -> Result<(), String> {
    let f = domain().get(name).and_then(|f| f.proc.clone()).unwrap();
    f.apply(&mut hub.0.lock().unwrap(), ctx, &input.to_bytes()).map(|_| ()).map_err(|r| r.0)
}

fn current(hub: &Memory, ctx: &ark::peer::Ctx) -> Option<SignIn> {
    let b = domain().ask(&hub.0.lock().unwrap(), ctx, "session.current", &[]).unwrap();
    Option::<SignIn>::from_bytes(&b).unwrap()
}

/// A row as a sign-in writes it, straight into the store.
fn signed_in(hub: &Memory, id: &str, user: &str, expires_ms: i64) {
    let s = SignIn {
        id: id.into(),
        user: user.into(),
        name: user.into(),
        expires_ms,
        roles: vec!["library".into()],
        ..SignIn::default()
    };
    let c = Change::Add {
        table: SignIn::TABLE.into(),
        row: s.to_bytes(),
    };
    hub.0.lock().unwrap().apply(&c).unwrap();
}

fn dev_auth() -> Auth {
    Auth::new::<App>(Mode::Dev, "https://app.example/", b"the key").allow_redirect("myapp://")
}

/// An auth writing to a hub of its own.
fn served(a: Auth) -> (Arc<Auth>, Memory) {
    let hub = Memory::new();
    a.serve(Box::new(hub.clone()));
    (Arc::new(a), hub)
}

fn alice() -> Account {
    Account {
        id: "alice".into(),
        name: "alice".into(),
        ..Account::default()
    }
}

/// Only its person ends a session, and an ended one proves nobody.
/// Falsified by `revoke` not asking whose the row is: bob's revoke of
/// alice's session went through.
#[test]
fn a_session_is_its_persons_to_end() {
    let hub = Memory::new();
    signed_in(&hub, "a1", "alice", 10_000);
    let alice = on("alice", "a1", &[], 2_000);
    assert_eq!(current(&hub, &alice).map(|s| s.user), Some("alice".into()));
    let bob = on("bob", "b1", &[], 2_000);
    let refused = run(&hub, &bob, "session.revoke", &Revoke { id: "a1".into() });
    assert_eq!(refused, Err("not your session".into()));
    assert!(current(&hub, &alice).is_some(), "bob ended nothing");
    run(&hub, &alice, "session.revoke", &Revoke { id: "a1".into() }).unwrap();
    assert_eq!(current(&hub, &alice), None, "revoked");
    assert!(hub.sign_ins()[0].revoked, "kept, revoked");
}

/// A session proves nobody once it has expired, and holds what it was
/// opened with and what was granted its person. Falsified by `live` not
/// asking the clock: the expired session was still current.
#[test]
fn current_is_the_live_session_with_its_grants() {
    let hub = Memory::new();
    signed_in(&hub, "a1", "alice", 10_000);
    let admin = on("root", "r1", &["admin"], 2_000);
    let grant = Grant {
        user: "alice".into(),
        role: "admin".into(),
    };
    run(&hub, &admin, "role.grant", &grant).unwrap();
    let s = current(&hub, &on("alice", "a1", &[], 2_000)).unwrap();
    assert_eq!(s.roles, vec!["admin".to_string(), "library".into()]);
    assert_eq!(current(&hub, &on("alice", "a1", &[], 10_000)), None, "expired");
    assert_eq!(current(&hub, &on("alice", "nope", &[], 2_000)), None, "no such session");
}

/// Only an admin grants or takes back a role. Falsified by `admin` letting
/// anybody: bob granted himself admin.
#[test]
fn only_an_admin_grants() {
    let hub = Memory::new();
    let bob = on("bob", "b1", &["library"], 2_000);
    let grant = Grant {
        user: "bob".into(),
        role: "admin".into(),
    };
    let roles = |h: &Memory| ark::db::read::<App, _>(&h.0.lock().unwrap(), |db| Ok::<_, ark::Refusal>(db.role.scan())).unwrap();
    assert_eq!(run(&hub, &bob, "role.grant", &grant), Err("only an admin grants roles".into()));
    assert!(roles(&hub).is_empty());
    let admin = on("root", "r1", &["admin"], 2_000);
    run(&hub, &admin, "role.grant", &grant).unwrap();
    let held = Role {
        user: "bob".into(),
        role: "admin".into(),
    };
    assert_eq!(roles(&hub), vec![held]);
    assert_eq!(run(&hub, &bob, "role.revoke", &grant), Err("only an admin grants roles".into()));
    run(&hub, &admin, "role.revoke", &grant).unwrap();
    assert!(roles(&hub).is_empty());
}

/// A connection fed by `feed` holds its person's sessions and roles and
/// nobody else's, and the app's own tables as the app says; `session.mine`
/// lists the caller's live ones. Falsified by `feed` holding every
/// session: bob's was held for alice.
#[test]
fn nobody_else_s_sessions_are_held_or_listed() {
    let hub = Memory::new();
    signed_in(&hub, "a1", "alice", 10_000);
    signed_in(&hub, "a2", "alice", 10_000);
    signed_in(&hub, "b1", "bob", 10_000);
    let alice = on("alice", "a1", &[], 2_000);
    let feed = domain().functions().get("feed").cloned().unwrap();
    let schema = domain().schema;
    let tagged = |table: &str, row: Vec<u8>| [vec![schema.table(table).unwrap().0 as u8], row].concat();
    let held: Vec<String> = hub
        .sign_ins()
        .into_iter()
        .filter(|s| feed.holds(&alice, &tagged("sign_in", s.to_bytes())).unwrap())
        .map(|s| s.id)
        .collect();
    assert_eq!(held, ["a1", "a2"]);
    let role = |user: &str| tagged("role", Role { user: user.into(), role: "x".into() }.to_bytes());
    assert!(feed.holds(&alice, &role("alice")).unwrap() && !feed.holds(&alice, &role("bob")).unwrap());
    assert!(feed.holds(&alice, &tagged("note", common::Note::default().to_bytes())).unwrap(), "the app's own");
    let b = domain().ask(&hub.0.lock().unwrap(), &alice, "session.mine", &[]).unwrap();
    let mine = Vec::<SignIn>::from_bytes(&b).unwrap();
    assert_eq!(mine.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["a1", "a2"]);
}

/// A token is its session's id signed with the server's key: another key's
/// signature, a changed id or a changed signature names nothing. Falsified
/// by `verify` answering the id without checking the signature: the
/// forged token named alice's session.
#[test]
fn a_token_is_judged_by_its_signature() {
    let a = dev_auth();
    let token = a.mint("s1");
    assert_eq!(a.verify(&token), Some("s1"));
    let other = Auth::new::<App>(Mode::Dev, "https://app.example/", b"another key").mint("s1");
    assert_eq!(a.verify(&other), None, "another server's key");
    let (_, sig) = token.split_once('.').unwrap();
    assert_eq!(a.verify(&format!("s2.{sig}")), None, "another session under this one's signature");
    let mut flipped = token.clone().into_bytes();
    let last = flipped.len() - 1;
    flipped[last] = if flipped[last] == b'0' { b'1' } else { b'0' };
    assert_eq!(a.verify(std::str::from_utf8(&flipped).unwrap()), None, "a signature changed");
    assert_eq!(a.verify("s1"), None);
    assert_eq!(a.verify(""), None);
}

/// A token proves its person while its row is live: signed by another key,
/// naming no row, revoked or expired, nobody. Falsified by `current`
/// reading the row without asking whether it is live: the expired session
/// still proved alice.
#[test]
fn a_token_proves_a_live_session_only() {
    let (auth, hub) = served(dev_auth().with_ttl(60_000));
    let authenticate = auth.authenticate();
    let login = auth.issue(&alice(), "a laptop").unwrap();
    let who = authenticate(&login.token).expect("a live session");
    assert_eq!((who.actor.as_str(), who.session.as_str()), ("alice", login.session.as_str()));
    let forged = Auth::new::<App>(Mode::Dev, "https://app.example/", b"another key").mint(&login.session);
    assert!(authenticate(&forged).is_none(), "another key");
    assert!(authenticate(&auth.mint("nobody-wrote-this")).is_none(), "no row");
    let expires = login.expires_ms;
    let store = hub.0.clone();
    assert!(auth.judge(&store.lock().unwrap(), &login.session, expires - 1).is_some());
    assert!(auth.judge(&store.lock().unwrap(), &login.session, expires).is_none(), "expired");
    let ctx = on("alice", &login.session, &[], login.expires_ms - 1);
    run(&hub, &ctx, "session.revoke", &Revoke { id: login.session.clone() }).unwrap();
    assert!(authenticate(&login.token).is_none(), "revoked");
}

/// The hub's watch: at an entry, a session it revoked and every session of
/// a person whose roles it changed; at the clock's tick, every session no
/// longer live; nobody else, and never a connection no row names.
/// Falsified by the watch skipping the role table: bob's session was not
/// judged again; and by the tick closing what no row names: c1 was
/// closed (which the speaker bridge's tests caught first).
#[test]
fn the_watch_names_what_to_close() {
    let (auth, hub) = served(dev_auth());
    let mut watch = auth.watcher();
    for (id, user, expires) in [("a1", "alice", 10_000), ("a2", "alice", 5_000), ("b1", "bob", 10_000)] {
        signed_in(&hub, id, user, expires);
    }
    // c1 is no sign-in's: a connection the server's process made itself.
    let open: Vec<Who> = [("alice", "a1"), ("alice", "a2"), ("bob", "b1"), ("carol", "c1")]
        .map(|(a, s)| Who {
            actor: a.into(),
            session: s.into(),
            roles: vec![],
        })
        .into();
    let revoke = Change::Edit {
        table: SignIn::TABLE.into(),
        old: vec![],
        new: SignIn {
            id: "a1".into(),
            user: "alice".into(),
            revoked: true,
            ..SignIn::default()
        }
        .to_bytes(),
    };
    run(&hub, &on("alice", "a1", &[], 2_000), "session.revoke", &Revoke { id: "a1".into() }).unwrap();
    let store = hub.0.clone();
    assert_eq!(watch(&store.lock().unwrap(), Some(&[revoke]), &open, 2_000), ["a1"]);
    let granted = Change::Add {
        table: Role::TABLE.into(),
        row: Role {
            user: "bob".into(),
            role: "admin".into(),
        }
        .to_bytes(),
    };
    assert_eq!(watch(&store.lock().unwrap(), Some(&[granted]), &open, 2_000), ["b1"]);
    assert_eq!(watch(&store.lock().unwrap(), Some(&[]), &open, 6_000), Vec::<String>::new(), "an entry of nothing");
    assert_eq!(watch(&store.lock().unwrap(), None, &open, 6_000), ["a1", "a2"], "a2 expired by the tick");
}

/// A code is redeemed once, for a dev login whose session is the row
/// written — its person, its device, live — and whose token names it; with
/// no hub to write to, no login. Falsified by `finish` not writing the
/// row: the login named a session nobody wrote.
#[test]
fn a_dev_login_is_a_row_and_a_code_redeemed_once() {
    let (a, hub) = served(dev_auth());
    let code = a.finish(&alice(), "a laptop").unwrap();
    let login = a.redeem(&code).expect("first time");
    assert!(a.redeem(&code).is_none(), "second time");
    let row = hub.sign_ins().pop().expect("a row written");
    assert_eq!(
        (row.id.as_str(), row.user.as_str(), row.device.as_str()),
        (login.session.as_str(), "alice", "a laptop")
    );
    assert!(!row.revoked && row.expires_ms == login.expires_ms && row.expires_ms > row.issued_ms);
    assert_eq!(a.verify(&login.token), Some(login.session.as_str()));
    assert!(dev_auth().issue(&alice(), "").is_err(), "nowhere to write a session, no login");
}

/// A server's own peer signs in once: a restart mints its live session
/// again and writes nothing. Falsified by `own` always issuing: a second
/// row was written.
#[test]
fn a_server_s_own_peer_keeps_its_session() {
    let (a, hub) = served(dev_auth());
    let first = a.own(&alice(), "the scanner").unwrap();
    let again = a.own(&alice(), "the scanner").unwrap();
    assert_eq!((again.session, again.token), (first.session, first.token));
    assert_eq!(hub.sign_ins().len(), 1);
    a.own(&alice(), "elsewhere").unwrap();
    assert_eq!(hub.sign_ins().len(), 2, "another device, another session");
}

#[test]
fn a_code_goes_only_where_it_is_allowed() {
    let a = dev_auth();
    assert!(a.redirect_allowed("http://127.0.0.1:53211/"));
    assert!(a.redirect_allowed("http://localhost:8080/?x=1"));
    assert!(a.redirect_allowed("https://app.example/"));
    assert!(a.redirect_allowed("https://app.example/app/"));
    assert!(a.redirect_allowed("myapp://auth"));
    assert!(!a.redirect_allowed("https://app.example.evil/"));
    assert!(!a.redirect_allowed("http://127.0.0.1.evil/"));
    assert!(!a.redirect_allowed("http://10.0.0.5:8080/"));
    assert!(!a.redirect_allowed("https://elsewhere/"));
}

/// `docs/plan-auth.md` A login holds the roles it was issued with and
/// the ones the configuration grants its account; the row keeps only its
/// own, so a restart with other configuration grants or revokes at once,
/// and the dev form's `name:role,role` is a login holding them. Falsified
/// by leaving the configured roles out of `issue`: the login held only
/// `scanner`.
#[test]
fn a_login_holds_its_own_roles_and_the_configured_ones() {
    let (a, hub) = served(Auth::new::<App>(Mode::Dev, "https://app.example/", b"k").with_roles([("library", vec!["alice"])]));
    let account = Account {
        roles: vec!["scanner".into()],
        ..alice()
    };
    let login = a.redeem(&a.finish(&account, "").unwrap()).expect("redeemed");
    assert_eq!(login.user.roles, ["library", "scanner"]);
    assert_eq!(hub.sign_ins()[0].roles, ["scanner"]);
    let who = a.authenticate()(&login.token).unwrap();
    assert_eq!(who.roles, ["library", "scanner"], "the configured roles merged at judging");
    let dev = dev_account("bob:library, admin,");
    assert_eq!((dev.id.as_str(), dev.roles), ("bob", vec!["admin".to_string(), "library".into()]));
}

/// The key is made once, readable by its owner alone, and read back the
/// same; a file that is not a key is refused. Falsified by `key_file`
/// drawing a key every time: the second read was another key.
#[test]
fn the_key_is_kept() {
    let dir = std::env::temp_dir().join(format!("ark-auth-key-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("session.key");
    let key = ark_auth::server::key_file(&path).unwrap();
    assert_eq!(key.len(), 32);
    assert_eq!(ark_auth::server::key_file(&path).unwrap(), key);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }
    let a = Auth::open::<App>(Mode::Dev, "http://x", &dir).unwrap();
    assert_eq!(a.verify(&Auth::new::<App>(Mode::Dev, "http://x", &key).mint("s")), Some("s"), "opened with the kept key");
    std::fs::write(&path, "not hex").unwrap();
    assert!(ark_auth::server::key_file(&path).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
