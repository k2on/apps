//! harken's server as a whole: signing in through its routes and syncing on
//! its socket; work done before anybody signed in, and under an older login,
//! arriving when somebody does; the log and the logins surviving a restart;
//! and a server that cannot sign anybody in refusing to start.

mod common;

use ark::value::Value;
use ark_client::{args, Options, Peer};
use common::*;
use harken_server::{start, Config, SignIn};

fn playlists(server: &harken_server::Server) -> Vec<(String, String)> {
    let rows = server
        .running
        .hub
        .read_blocking(|h| h.rows("playlist"))
        .unwrap();
    let mut out: Vec<(String, String)> = rows
        .iter()
        .map(|r| match (&r["name"], &r["user_id"]) {
            (Value::Text(n), Value::Text(u)) => (n.to_string(), u.to_string()),
            other => panic!("{other:?}"),
        })
        .collect();
    out.sort();
    out
}

/// The routes and the socket agree about who somebody is: a login from
/// `/auth/login` and `/auth/exchange` is a token the socket takes, the actor
/// is who the server verified, and another device of the same person sees
/// it.
#[test]
fn a_login_from_the_routes_is_who_the_socket_says_you_are() {
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |_| {});
    let url = server.url();
    let login = ark_auth::client::login(&url, Some("alice"), |u| {
        panic!("dev auth opens no browser: {u}")
    })
    .unwrap();
    let opts = Options::server(
        login.user.id.clone(),
        login.session.clone(),
        Some(login.token.clone()),
    )
    .with_timing(quick());
    let mut laptop = Peer::open_memory(domain(), opts).unwrap();
    laptop.connect(&ark_auth::socket_url(&url));
    laptop
        .mutate(
            "create_playlist",
            args([("name", Value::text("Road trip"))]),
        )
        .unwrap();
    pump_until(&mut [&mut laptop], 10, |ps| ps[0].pending_len() == 0);
    assert_eq!(
        playlists(&server),
        [("Road trip".to_string(), "alice".to_string())]
    );

    let mut phone = signed_in(&server, "alice");
    pump_until(&mut [&mut phone], 10, |ps| ps[0].cursor() == 1);
    let mine = phone.query("playlists", &args([])).unwrap();
    assert!(format!("{mine:?}").contains("Road trip"), "{mine:?}");

    // A token nobody issued is turned away, and keeps what it authored.
    let opts = Options::server("mallory", "nope", Some("forged".into())).with_timing(quick());
    let mut mallory = Peer::open_memory(domain(), opts).unwrap();
    mallory.connect(&server.running.sync_url());
    mallory
        .mutate("create_playlist", args([("name", Value::text("Mine now"))]))
        .unwrap();
    pump_until(&mut [&mut mallory], 10, |ps| ps[0].denied().is_some());
    assert_eq!(mallory.pending_len(), 1);
    assert_eq!(playlists(&server).len(), 1);
    rt.block_on(server.stop());
}

/// Somebody uses the app before they have an account: what they make is
/// nobody's, kept, and dials nothing. They sign in, and it is theirs — the
/// client re-stamps it and the server takes it like any other entry. Then
/// the same person, on a new login after the old one was revoked, pushes
/// what they authored offline under the old one, and that is theirs too.
#[test]
fn work_done_signed_out_or_under_an_older_login_arrives_as_its_owners() {
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |_| {});
    let url = server.url();

    let mut p = Peer::open_memory(domain(), Options::signed_out().with_timing(quick())).unwrap();
    p.connect(&ark_auth::socket_url(&url));
    let early = p
        .mutate("create_playlist", args([("name", Value::text("Before"))]))
        .unwrap();
    pump_until(&mut [&mut p], 1, |_| true);
    assert!(
        playlists(&server).is_empty(),
        "nothing is dialled signed out"
    );

    let first = ark_auth::client::login(&url, Some("dana"), |_| {}).unwrap();
    p.sign_in(
        first.user.id.clone(),
        first.session.clone(),
        Some(first.token.clone()),
    )
    .unwrap();
    pump_until(&mut [&mut p], 10, |ps| ps[0].pending_len() == 0);
    assert_eq!(p.standing(&early), ark_client::Standing::Confirmed);
    assert_eq!(
        playlists(&server),
        [("Before".to_string(), "dana".to_string())]
    );

    // Offline, then signed out of that login everywhere, then in again.
    p.disconnect();
    let offline = p
        .mutate(
            "create_playlist",
            args([("name", Value::text("On the train"))]),
        )
        .unwrap();
    ark_auth::client::logout(&url, &first.token).unwrap();
    let second = ark_auth::client::login(&url, Some("dana"), |_| {}).unwrap();
    assert_ne!(second.session, first.session);
    p.set_token(Some(second.token.clone()));
    p.set_session(second.session.clone());
    p.reconnect();
    pump_until(&mut [&mut p], 10, |ps| ps[0].pending_len() == 0);
    assert!(p.take_rejections().is_empty());
    assert_eq!(p.standing(&offline), ark_client::Standing::Confirmed);
    assert_eq!(playlists(&server).len(), 2);
    rt.block_on(server.stop());
}

/// The log, the logins and the kept rooms are in the data directory, and a
/// restart reads them back: a token issued before still signs in.
#[test]
fn the_log_and_the_logins_survive_a_restart() {
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |_| {});
    let login = ark_auth::client::login(&server.url(), Some("erin"), |_| {}).unwrap();
    let opts = || {
        Options::server(
            login.user.id.clone(),
            login.session.clone(),
            Some(login.token.clone()),
        )
        .with_timing(quick())
    };
    let mut p = Peer::open_memory(domain(), opts()).unwrap();
    p.connect(&server.running.sync_url());
    p.mutate("create_playlist", args([("name", Value::text("Kept"))]))
        .unwrap();
    pump_until(&mut [&mut p], 10, |ps| ps[0].pending_len() == 0);
    drop(p);
    rt.block_on(server.stop());

    let server = serve(&rt, data.path(), |_| {});
    assert_eq!(
        playlists(&server),
        [("Kept".to_string(), "erin".to_string())]
    );
    let mut again = Peer::open_memory(domain(), opts()).unwrap();
    again.connect(&server.running.sync_url());
    pump_until(&mut [&mut again], 10, |ps| ps[0].cursor() == 1);
    assert_eq!(again.denied(), None, "the login outlived the restart");
    rt.block_on(server.stop());
}

/// A provider that cannot be reached, or a secret that is not there, is a
/// server that does not start — and says which.
#[test]
fn a_server_that_cannot_sign_anybody_in_does_not_start() {
    let rt = runtime();
    let dir = tempfile::tempdir().unwrap();
    let secret = dir.path().join("oidc-secret");
    let oidc = |file: &std::path::Path| {
        let mut c = Config::dev("127.0.0.1:0", &dir.path().join("data"));
        c.sign_in = SignIn::Oidc {
            issuer: "http://127.0.0.1:9/nowhere".into(),
            client_id: "harken".into(),
            secret_file: file.into(),
            scopes: vec!["openid".into()],
        };
        c
    };
    let e = rt
        .block_on(start(oidc(&secret)))
        .err()
        .expect("no secret, no server")
        .to_string();
    assert!(e.contains("oidc-secret"), "names the file: {e}");
    std::fs::write(&secret, "s3cret\n").unwrap();
    let e = rt
        .block_on(start(oidc(&secret)))
        .err()
        .expect("no provider, no server")
        .to_string();
    assert!(e.contains("127.0.0.1:9"), "names the provider: {e}");
    assert!(!e.contains("s3cret"), "and never the secret: {e}");
}
