//! The crate's unit tests: the URL helpers and the secret file (the
//! redirect rules, codes, roles, tokens and rows are `tests/rows.rs`'s,
//! over a schema of its own). What the
//! `openidconnect` crate now owns (the JWT payload's parsing, base64url,
//! the ID token's claim checks) is its own to test, and is tested end to
//! end against a provider in `tests/oidc.rs`.

use crate::oidc::read_secret;
use crate::util::*;

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
