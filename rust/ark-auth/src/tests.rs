//! The crate's unit tests: the URL helpers, the secret file, the redirect
//! rules, codes, roles and the tokens. What the
//! `openidconnect` crate now owns (the JWT payload's parsing, base64url,
//! the ID token's claim checks) is its own to test, and is tested end to
//! end against a provider in `tests/oidc.rs`.

use std::sync::{Arc, Mutex};

use crate::oidc::read_secret;
use crate::server::{dev_account, Auth, Mode, Opened};
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

/// An auth whose sign-ins are written to `rows`.
fn writing(a: Auth) -> (Auth, Arc<Mutex<Vec<Opened>>>) {
    let rows: Arc<Mutex<Vec<Opened>>> = Arc::default();
    let r = rows.clone();
    a.on_sign_in(move |o| {
        r.lock().unwrap().push(o.clone());
        Ok(())
    });
    (a, rows)
}

fn dev_auth() -> Auth {
    Auth::new(Mode::Dev, "https://app.example/", b"the key").allow_redirect("myapp://")
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

/// A code is redeemed once, for a login whose session is the row written
/// and whose token names it. Falsified by `finish` not writing the row:
/// the login named a session nobody wrote.
#[test]
fn a_code_is_redeemed_once_for_the_session_written() {
    let (a, rows) = writing(dev_auth());
    let alice = Account {
        id: "alice".into(),
        name: "alice".into(),
        ..Account::default()
    };
    let code = a.finish(&alice, "a laptop").unwrap();
    let login = a.redeem(&code).expect("first time");
    assert!(a.redeem(&code).is_none(), "second time");
    let row = rows.lock().unwrap().pop().expect("a row written");
    assert_eq!(
        (row.id.as_str(), row.user.as_str(), row.device.as_str()),
        (login.session.as_str(), "alice", "a laptop")
    );
    assert!(!row.revoked && row.expires_ms == login.expires_ms && row.expires_ms > row.issued_ms);
    assert_eq!(a.verify(&login.token), Some(login.session.as_str()));
    assert!(dev_auth().issue(&alice, "").is_err(), "nowhere to write a session, no login");
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
    let other = Auth::new(Mode::Dev, "https://app.example/", b"another key").mint("s1");
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

/// `docs/plan-auth.md` A login holds the roles it was issued with and
/// the ones the configuration grants its account; the row keeps only its
/// own, so a restart with other configuration grants or revokes at once,
/// and the dev form's `name:role,role` is a login holding them. Falsified
/// by leaving the configured roles out of `issue`: the login held only
/// `scanner`.
#[test]
fn a_login_holds_its_own_roles_and_the_configured_ones() {
    let a = Auth::new(Mode::Dev, "https://app.example/", b"k").with_roles([("library", vec!["alice"])]);
    let (a, rows) = writing(a);
    let account = Account {
        id: "alice".into(),
        roles: vec!["scanner".into()],
        ..Account::default()
    };
    let login = a.redeem(&a.finish(&account, "").unwrap()).expect("redeemed");
    assert_eq!(login.user.roles, ["library", "scanner"]);
    assert_eq!(rows.lock().unwrap()[0].roles, ["scanner"]);
    let dev = dev_account("bob:library, admin,");
    assert_eq!((dev.id.as_str(), dev.roles), ("bob", vec!["admin".to_string(), "library".into()]));
}
