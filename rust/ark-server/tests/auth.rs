//! Signing in through ark-auth's routes on an ark-server, then the socket:
//! the whole way round in dev mode.

mod common;

use std::sync::Arc;

use ark::value::Value;
use ark_auth::server::{Auth, Mode};
use ark_auth::session::SessionStore;
use ark_client::{args, demo, Options, Peer};
use common::*;

fn serve_auth(rt: &tokio::runtime::Runtime) -> (ark_server::Running, Arc<Auth>) {
    // The public URL is where a browser reaches the server; for a test,
    // loopback, which every server accepts as a redirect anyway.
    let auth = Arc::new(Auth::new(SessionStore::memory(), Mode::Dev, "http://127.0.0.1"));
    let app = ark_server::builder(demo::domain()).name("test").auth(auth.clone()).build().unwrap();
    (rt.block_on(app.serve("127.0.0.1:0")).unwrap(), auth)
}

#[test]
fn a_login_is_a_token_the_socket_accepts_until_it_is_revoked() {
    let rt = runtime();
    let (running, _auth) = serve_auth(&rt);
    let server = running.url();
    let login = ark_auth::client::login(&server, Some("alice"), |url| panic!("a browser was asked to open {url}")).unwrap();
    assert_eq!(login.user.id, "alice");

    let opts = Options::server(login.user.id.clone(), login.session.clone(), Some(login.token.clone())).with_timing(quick());
    let mut p = Peer::open_memory(demo::domain(), opts).unwrap();
    p.connect(&ark_auth::socket_url(&server));
    p.mutate("create_playlist", args([("name", Value::text("Hers"))])).unwrap();
    pump_until(&mut [&mut p], |ps| ps[0].pending_len() == 0);
    assert_eq!(p.denied(), None);
    let rows = rt.block_on(running.hub.rows("playlist")).unwrap();
    assert_eq!(rows[0]["user_id"], Value::text("alice"), "the actor is who the server verified");
    // A session is a device: the room names this one by it.
    let who = rt
        .block_on(running.hub.read(|h| h.rooms()["alice"].iter().map(|p| p.who.clone()).collect::<Vec<_>>()))
        .unwrap();
    assert_eq!(who, std::slice::from_ref(&login.session));

    // Signed out: the same token is turned away, the link stops dialling,
    // and nothing authored meanwhile is lost.
    ark_auth::client::logout(&server, &login.token).unwrap();
    p.mutate("create_playlist", args([("name", Value::text("After"))])).unwrap();
    p.reconnect();
    pump_until(&mut [&mut p], |ps| ps[0].denied().is_some());
    assert_eq!(p.denied(), Some("not signed in"));
    assert_eq!(p.status().link, "idle", "a denied link does not dial again");
    assert_eq!(p.pending_len(), 1, "kept for the next login");

    // Signed in again as the same person — a new session, a new device —
    // the entry authored under the revoked one is still hers: the server
    // asks ark-auth whose session it was, and takes it.
    let again = ark_auth::client::login(&server, Some("alice"), |_| {}).unwrap();
    p.set_token(Some(again.token.clone()));
    p.set_session(again.session.clone());
    p.reconnect();
    let mine = p.mutate("create_playlist", args([("name", Value::text("New device"))])).unwrap();
    pump_until(&mut [&mut p], |ps| ps[0].pending_len() == 0);
    assert_eq!(p.cursor(), 3);
    assert!(p.take_rejections().is_empty());
    assert_eq!(p.standing(&mine), ark_client::Standing::Confirmed);
    rt.block_on(running.stop());
}

#[test]
fn nobody_else_can_write_as_her() {
    let rt = runtime();
    let (running, _auth) = serve_auth(&rt);
    let alice = ark_auth::client::login(&running.url(), Some("alice"), |_| {}).unwrap();
    // Signed in as alice, authoring as bob.
    let opts = Options::server("bob", alice.session.clone(), Some(alice.token.clone())).with_timing(quick());
    let mut mallory = Peer::open_memory(demo::domain(), opts).unwrap();
    mallory.connect(&running.sync_url());
    mallory.mutate("create_playlist", args([("name", Value::text("as bob"))])).unwrap();
    let mut rejections = vec![];
    pump_until(&mut [&mut mallory], |ps| {
        rejections.extend(ps[0].take_rejections());
        !rejections.is_empty()
    });
    assert_eq!(rejections[0].reason, "not yours");
    assert_eq!(mallory.pending_len(), 0, "and it will not be offered again");
    assert_eq!(mallory.standing(&rejections[0].id), ark_client::Standing::Rejected("not yours".into()));
    rt.block_on(running.stop());
}

/// An entry authored offline under one login, pushed after the same person
/// signed in again, is theirs: `.auth` gives the engine ark-auth's record
/// of whose session is whose.
#[test]
fn an_entry_from_an_earlier_login_of_the_same_person_is_accepted() {
    let rt = runtime();
    let (running, _auth) = serve_auth(&rt);
    let server = running.url();
    let first = ark_auth::client::login(&server, Some("alice"), |_| {}).unwrap();
    let opts = Options::server("alice", first.session.clone(), Some(first.token.clone())).with_timing(quick());
    let mut p = Peer::open_memory(demo::domain(), opts).unwrap();
    p.mutate("create_playlist", args([("name", Value::text("Offline"))])).unwrap();
    let second = ark_auth::client::login(&server, Some("alice"), |_| {}).unwrap();
    p.set_token(Some(second.token));
    p.set_session(second.session);
    p.connect(&running.sync_url());
    let mut rejections = vec![];
    pump_until(&mut [&mut p], |ps| {
        rejections.extend(ps[0].take_rejections());
        ps[0].pending_len() == 0
    });
    assert!(rejections.is_empty(), "{rejections:?}");
    assert_eq!(p.cursor(), 1);
    rt.block_on(running.stop());
}

#[test]
fn a_server_told_nothing_about_sign_in_refuses_to_start() {
    let err = ark_server::builder(demo::domain()).build().err().expect("refused");
    assert!(err.to_string().contains("no sign-in configured"), "{err}");
}
