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

/// A picture's address names the person and the picture: none, nothing to
/// load; another picture, another address, so a cache keyed by it lets the
/// old one go. Falsified by leaving `v` out: the two addresses are one.
#[test]
fn a_picture_is_loaded_through_the_server_by_version() {
    assert_eq!(picture_url("https://h.example", "u", ""), None);
    let a = picture_url("https://h.example/", "google|42", "https://p.example/a.png").unwrap();
    assert!(a.starts_with("https://h.example/auth/picture?user=google%7C42&v="), "{a}");
    let query = a.split('?').nth(1).unwrap();
    assert_eq!(query_value(query, "user").as_deref(), Some("google|42"));
    let b = picture_url("https://h.example", "google|42", "https://p.example/b.png").unwrap();
    assert_ne!(a, b);
}

/// What the server fetches a picture from: the internet at large, never
/// this machine or a network only it can reach. Falsified by dropping
/// `is_private` from `public`: `10.0.0.5` is fetched.
#[test]
fn a_picture_is_fetched_from_a_public_address_alone() {
    use crate::server::public;
    for ok in ["93.184.216.34", "2606:2800:220:1:248:1893:25c8:1946"] {
        assert!(public(ok.parse().unwrap()), "{ok}");
    }
    for no in [
        "127.0.0.1",
        "10.0.0.5",
        "172.16.0.1",
        "192.168.1.1",
        "169.254.169.254",
        "100.64.0.1",
        "0.0.0.0",
        "::1",
        "fd00::1",
        "fe80::1",
        "::ffff:127.0.0.1",
    ] {
        assert!(!public(no.parse().unwrap()), "{no}");
    }
}
