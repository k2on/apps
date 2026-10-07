//! `docs/plan-guards.md` D4: the admin page's API over real sockets — the
//! authority's state as the explorer is shown it, a raw write and a domain
//! mutation authored as the authority and served to every peer, a write the
//! constraints refuse, and who may ask when the page is bound wider than
//! loopback.

mod common;

use std::net::SocketAddr;

use ark::store::{Change, Store};
use ark::value::Value;
use ark_client::{args, demo};
use ark_explorer::wire::{author_body, raw_body, State};
use ark_server::Admin;
use common::*;

fn admin(bind: &str) -> Admin {
    Admin {
        bind: bind.into(),
        page: None,
        server: Some("http://127.0.0.1:1".into()),
    }
}

/// One request: the status and the body. Blocking, from the test's own
/// thread: the server runs on the runtime's workers.
fn call(_rt: &tokio::runtime::Runtime, addr: SocketAddr, path: &str, token: Option<&str>, body: Option<String>) -> (u16, String) {
    let url = format!("http://127.0.0.1:{}{path}", addr.port());
    let req = match &body {
        Some(_) => ureq::post(&url),
        None => ureq::get(&url),
    };
    let req = match token {
        Some(t) => req.set("Authorization", &format!("Bearer {t}")),
        None => req,
    };
    let res = match body {
        Some(b) => req.send_string(&b),
        None => req.call(),
    };
    match res {
        Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
        Err(ureq::Error::Status(s, r)) => (s, r.into_string().unwrap_or_default()),
        Err(e) => panic!("{e}"),
    }
}

fn state(rt: &tokio::runtime::Runtime, addr: SocketAddr, token: Option<&str>) -> State {
    let (status, body) = call(rt, addr, "/admin/api/state", token, None);
    assert_eq!(status, 200, "{body}");
    State::from_json(&body).unwrap_or_else(|e| panic!("{e}: {body}"))
}

/// On loopback the page asks nothing. It is shown the authority's own
/// store, its log with who pushed each entry, and its connections; a raw
/// write it asks for is authored as the authority, judged by the
/// constraints, and reaches every peer like any entry; and a domain
/// mutation through the CRUD path is the authority's too. Falsified by the
/// hub writing raw as the first connection's login: the logged entry's
/// actor was alice, not the authority.
#[test]
fn the_admin_page_writes_as_the_authority_and_every_peer_sees_it() {
    let rt = runtime();
    let running = serve(&rt, None, |b| b.admin(admin("127.0.0.1:0")));
    let at = running.admin_addr.expect("the admin page listens");
    let mut alice = peer(&running, "alice");
    alice.mutate("create_playlist", args([("name", Value::text("Mine"))])).unwrap();
    pump_until(&mut [&mut alice], |ps| ps[0].cursor() == 1);

    assert_eq!(
        call(&rt, at, "/admin/api/hello", None, None),
        (200, "{\"open\":true,\"server\":\"http://127.0.0.1:1\"}".into())
    );
    let s = state(&rt, at, None);
    let m = ark::ir::module_from_value(&ark::canon::decode(&s.module).unwrap()).unwrap();
    assert_eq!(m.schema, demo::domain().module().schema);
    let st = ark::store::MemoryStore::from_value(m.schema.clone(), &s.store);
    let list = st.scan("playlist")[0].clone();
    assert_eq!(list.get("name"), Some(&Value::text("Mine")));
    assert_eq!(
        (s.lines.len(), s.lines[0].function.as_str(), s.lines[0].entry.actor.as_str()),
        (1, "create_playlist", "alice")
    );
    assert!(s.connections.iter().any(|c| c.who.starts_with("alice")), "{:?}", s.connections);
    assert!(s.raw && s.who == ark::raw::AUTHOR);

    // A raw write: the playlist renamed. Every peer is served it.
    let renamed = list.clone().with("name", Value::text("Fixed"));
    let (status, body) = call(
        &rt,
        at,
        "/admin/api/raw",
        None,
        Some(raw_body(&Change::Edit("playlist".into(), list.clone(), renamed))),
    );
    assert_eq!((status, body.as_str()), (200, "2"));
    pump_until(&mut [&mut alice], |ps| ps[0].cursor() == 2);
    assert_eq!(alice.store().scan("playlist")[0].get("name"), Some(&Value::text("Fixed")));
    let s = state(&rt, at, None);
    let last = s.lines.last().unwrap();
    assert_eq!((last.function.as_str(), last.entry.actor.as_str()), ("ark.put_row", ark::raw::AUTHOR));

    // One the constraints refuse is said, and logged nowhere.
    let orphan = Change::Add(
        "item".into(),
        [
            ("playlist_id".to_string(), Value::Id([9; 16])),
            ("track_id".to_string(), Value::text("t")),
            ("pos".to_string(), Value::Int(1)),
        ]
        .into_iter()
        .collect(),
    );
    let (status, body) = call(&rt, at, "/admin/api/raw", None, Some(raw_body(&orphan)));
    assert_eq!(status, 409, "{body}");
    assert!(body.contains("names no playlist"), "{body}");

    // A domain mutation, authored as the authority.
    let (status, body) = call(
        &rt,
        at,
        "/admin/api/author",
        None,
        Some(author_body("create_playlist", &args([("name", Value::text("Ops"))]))),
    );
    assert_eq!((status, body.as_str()), (200, "3"));
    pump_until(&mut [&mut alice], |ps| ps[0].cursor() == 3);
    let s = state(&rt, at, None);
    let last = s.lines.last().unwrap();
    assert_eq!((last.function.as_str(), last.entry.actor.as_str()), ("create_playlist", ark::raw::AUTHOR));
    rt.block_on(running.stop());
}

/// Bound wider than loopback, every request but `hello` must carry a login
/// that holds `admin`: one without the role is refused — under dev auth a
/// missing token is somebody too, holding nothing; under a real sign-in it
/// is told to sign in — and one with it is served. Falsified by asking
/// nothing however it is bound: the request with no login was served.
#[test]
fn a_wider_admin_page_asks_for_the_admin_role() {
    let rt = runtime();
    let running = serve(&rt, None, |b| b.admin(admin("0.0.0.0:0")));
    let at = running.admin_addr.expect("the admin page listens");
    let (_, hello) = call(&rt, at, "/admin/api/hello", None, None);
    assert!(hello.starts_with("{\"open\":false"), "{hello}");
    // Dev auth takes a missing token for "anonymous", who holds nothing.
    let (status, body) = call(&rt, at, "/admin/api/state", None, None);
    assert_eq!((status, body.as_str()), (403, "anonymous does not hold the admin role"));
    let (status, body) = call(&rt, at, "/admin/api/state", Some("carol"), None);
    assert_eq!(status, 403, "{body}");
    assert!(body.contains("carol does not hold the admin role"), "{body}");
    let s = state(&rt, at, Some("dave:admin"));
    assert_eq!(s.head, 0);
    let change = Change::Add(
        "playlist".into(),
        [
            ("id".to_string(), Value::Id([1; 16])),
            ("name".to_string(), Value::text("x")),
            ("user_id".to_string(), Value::text("y")),
        ]
        .into_iter()
        .collect(),
    );
    assert_eq!(call(&rt, at, "/admin/api/raw", Some("carol"), Some(raw_body(&change))).0, 403);
    assert_eq!(
        call(&rt, at, "/admin/api/raw", Some("dave:admin"), Some(raw_body(&change))),
        (200, "1".into())
    );
    assert!(ark_server::admin::is_loopback("127.0.0.1:8788") && ark_server::admin::is_loopback("[::1]:8788"));
    assert!(ark_server::admin::is_loopback("localhost:8788") && !ark_server::admin::is_loopback("0.0.0.0:8788"));
    rt.block_on(running.stop());
}
