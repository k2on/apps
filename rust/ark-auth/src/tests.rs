//! The crate's unit tests: the URL helpers, the secret file, the redirect
//! rules, codes, roles and revocation, and the session store. What the
//! `openidconnect` crate now owns (the JWT payload's parsing, base64url,
//! the ID token's claim checks) is its own to test, and is tested end to
//! end against a provider in `tests/oidc.rs`.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use ark::peer::Who;

use crate::oidc::read_secret;
use crate::server::{dev_account, Auth, Mode};
use crate::session::{hash, SessionStore};
use crate::util::*;
use crate::Account;

#[test]
fn socket_follows_the_scheme() {
    assert_eq!(socket_url("https://h.example"), "wss://h.example/sync");
    assert_eq!(socket_url("http://127.0.0.1:8787/"), "ws://127.0.0.1:8787/sync");
    assert_eq!(socket_url("10.0.2.2:8787"), "ws://10.0.2.2:8787/sync");
}

#[test]
fn a_query_value_round_trips() {
    let url = login_url("http://s", "harken://auth?x=1 2", Some("al ice"));
    let query = url.split('?').nth(1).unwrap();
    assert_eq!(query_value(query, "redirect").as_deref(), Some("harken://auth?x=1 2"));
    assert_eq!(query_value(query, "user").as_deref(), Some("al ice"));
    assert_eq!(query_value(query, "missing"), None);
}

#[test]
fn the_code_joins_whatever_query_there_is() {
    assert_eq!(with_code("harken://auth", "c d"), "harken://auth?code=c%20d");
    assert_eq!(with_code("http://x/?a=1", "c"), "http://x/?a=1&code=c");
}

#[test]
fn a_secret_file_loses_its_newline_and_an_empty_one_is_refused() {
    let dir = std::env::temp_dir().join(format!("ark-auth-secret-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("s"), "hunter2\n").unwrap();
    assert_eq!(read_secret(dir.join("s")).unwrap(), "hunter2");
    std::fs::write(dir.join("e"), "\n").unwrap();
    assert!(read_secret(dir.join("e")).is_err());
    assert!(read_secret(dir.join("missing")).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

fn dev_auth() -> Auth {
    Auth::new(SessionStore::memory(), Mode::Dev, "https://app.example/").allow_redirect("myapp://")
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

#[test]
fn a_code_is_redeemed_once_and_the_engine_is_told_who_it_is() {
    let a = Arc::new(dev_auth());
    let code = a
        .finish(&Account {
            id: "alice".into(),
            name: "alice".into(),
            ..Account::default()
        })
        .unwrap();
    let login = a.redeem(&code).expect("first time");
    assert_eq!(login.user.id, "alice");
    assert!(a.redeem(&code).is_none(), "second time");
    let who = a.authenticator();
    let alice = Who {
        actor: "alice".into(),
        session: login.session.clone(),
        roles: vec![],
    };
    assert_eq!(who(&login.token), Some(alice));
    assert_eq!(who("forged"), None);
    assert_eq!(who(""), None);
    assert!(a.owns("alice", &login.session));
    assert!(!a.owns("bob", &login.session));
}

/// `docs/plan-auth.md` A login holds the roles it was issued with and
/// the ones the configuration grants its account, in what the exchange
/// hands the client and in what the engine is told at `Hello`; a
/// server started again with other configuration answers the same
/// token with the new roles, and the dev form's `name:role,role` is a
/// login holding them. Falsified by leaving the configured roles out
/// of `completed`: the exchange's login held only `scanner`.
#[test]
fn a_login_holds_its_own_roles_and_the_configured_ones() {
    let sessions = || SessionStore::memory();
    let a = Arc::new(Auth::new(sessions(), Mode::Dev, "https://app.example/").with_roles([("library", vec!["alice"])]));
    let code = a
        .finish(&Account {
            id: "alice".into(),
            roles: vec!["scanner".into()],
            ..Account::default()
        })
        .unwrap();
    let login = a.redeem(&code).expect("redeemed");
    assert_eq!(login.user.roles, ["library", "scanner"]);
    let who = a.authenticator()(&login.token).expect("signed in");
    assert_eq!(
        (who.actor.as_str(), &who.session, who.roles),
        ("alice", &login.session, vec!["library".to_string(), "scanner".into()])
    );
    assert!(a.announce().contains("role library: alice"), "{}", a.announce());
    // The same session store, the configuration moved: the role goes.
    let store = std::mem::replace(&mut *a.sessions(), sessions());
    let b = Arc::new(Auth::new(store, Mode::Dev, "https://app.example/"));
    let who = b.authenticator()(&login.token).expect("signed in");
    assert_eq!(who.roles, ["scanner".to_string()]);
    // Dev auth's name, with roles.
    let dev = dev_account("bob:library, admin,");
    assert_eq!((dev.id.as_str(), dev.roles), ("bob", vec!["admin".to_string(), "library".into()]));
}

/// R6: revoking a session tells every listener whose and which, once,
/// and a token already revoked or never issued tells nobody. Falsified
/// by leaving out the loop over the listeners in `Auth::revoke` (the
/// store revoking alone, as `logout` did): the listener hears nothing.
#[test]
fn a_revocation_is_told_to_whoever_asked() {
    let a = dev_auth();
    let heard: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
    let h = heard.clone();
    a.on_revoke(move |user, session| h.lock().unwrap().push((user.into(), session.into())));
    let account = Account {
        id: "alice".into(),
        name: "Alice".into(),
        ..Account::default()
    };
    let login = a.issue(&account).unwrap();
    assert!(a.revoke(&login.token).unwrap());
    assert!(!a.revoke(&login.token).unwrap(), "already gone");
    assert!(!a.revoke("no such token").unwrap());
    assert_eq!(*heard.lock().unwrap(), [("alice".to_string(), login.session.clone())]);
}

fn alice() -> Account {
    Account {
        id: "sub-alice".into(),
        name: "Alice".into(),
        email: "alice@example".into(),
        roles: vec![],
    }
}

#[test]
fn a_token_proves_its_session_until_it_does_not() {
    let clock = Arc::new(AtomicI64::new(1_000));
    let tick = clock.clone();
    let mut store = SessionStore::memory().with_ttl(100).with_clock(move || tick.load(Ordering::SeqCst));
    let login = store.issue(&alice()).unwrap();
    assert_eq!(login.expires_ms, 1_100);
    assert_eq!(store.lookup(&login.token).unwrap().session, login.session);
    assert!(store.lookup("not a token").is_none());
    clock.store(1_100, Ordering::SeqCst);
    assert!(store.lookup(&login.token).is_none(), "expired");
    assert!(store.owned_by("sub-alice", &login.session), "expired is not forgotten");
    assert!(!store.owned_by("sub-bob", &login.session));
}

#[test]
fn revoking_ends_the_token_and_keeps_the_ownership() {
    let mut store = SessionStore::memory();
    let login = store.issue(&alice()).unwrap();
    assert!(store.revoke(&login.token).unwrap());
    assert!(!store.revoke(&login.token).unwrap(), "already gone");
    assert!(store.lookup(&login.token).is_none());
    assert!(store.owned_by("sub-alice", &login.session));
}

#[test]
fn the_file_survives_a_restart_and_holds_no_token() {
    let dir = std::env::temp_dir().join(format!("ark-auth-sessions-{}", std::process::id()));
    let path = dir.join("sessions.json");
    let _ = std::fs::remove_dir_all(&dir);
    let (kept, gone) = {
        let mut store = SessionStore::open(&path).unwrap();
        let kept = store.issue(&alice()).unwrap();
        let gone = store.issue(&alice()).unwrap();
        store.revoke(&gone.token).unwrap();
        (kept, gone)
    };
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains(&kept.token), "the token itself is never written");
    assert!(text.contains(&hash(&kept.token)));
    let mut again = SessionStore::open(&path).unwrap();
    assert_eq!(again.lookup(&kept.token).map(|l| l.session), Some(kept.session));
    assert!(again.lookup(&gone.token).is_none(), "a revocation survives the restart");
    assert!(again.owned_by("sub-alice", &gone.session));
    let _ = std::fs::remove_dir_all(&dir);
}
