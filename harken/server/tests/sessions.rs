//! Who is signed in, end to end (`docs/plan-v5.md` S1): a sign-in is a
//! session row the hub writes and its token is judged by its signature and
//! that row; a session revoked or expired is closed and turned away; a
//! role granted judges its person again; a server fed by scope holds
//! nobody else's sign-ins.

mod common;

use std::sync::Arc;
use std::time::Duration;

use ark_auth::server::{Auth, Mode};
use common::*;
use ark_auth::{Grant, SignIn};
use harken_server::domain::{CreatePlaylist, Playlist};
use harken_server::hosted::Hosted;

/// Pump `p` until its link is turned away, or `secs` pass.
fn turned_away(p: &mut Peer, secs: u64) -> bool {
    eventually(secs, || {
        p.pump()
            .denied
            .or(p.client.link.token.is_none().then(String::new))
    })
    .is_some()
}

/// A dev login is a session row — its person, the program it came from —
/// and its token proves it while it is live: a token whose signature is
/// not this server's, or that names no row, is turned away, and a sign-out
/// through the domain closes every connection of that session. Falsified
/// by judging a token by its id alone (`verify` answering without the
/// signature): the forged token was taken.
#[test]
fn a_login_is_a_row_and_its_token_is_judged_by_it() {
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |_| {});
    let url = server.url();
    let login = ark_auth::client::login(&url, Some("alice"), |_| {}).unwrap();
    let rows = hub_rows::<SignIn>(&server, "sign_in");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].id.as_str(), rows[0].user.as_str()),
        (login.session.as_str(), "alice")
    );
    assert!(
        rows[0].device.starts_with("ureq") && !rows[0].revoked,
        "{:?}",
        rows[0]
    );

    let mut laptop = open(ark_peer::who(&login), None);
    laptop.connect(&url, &login.token);
    pump_until(&mut [&mut laptop], 10, |ps| {
        ps[0].client.current().is_some()
    });
    assert_eq!(laptop.client.current().unwrap().user, "alice");

    let mut forged = login.token.clone();
    let last = forged.pop().unwrap();
    forged.push(if last == '0' { '1' } else { '0' });
    let mut mallory = open(ark_peer::who(&login), None);
    mallory.connect(&url, &forged);
    assert!(
        turned_away(&mut mallory, 10),
        "a signature not the server's"
    );
    let mut nobody = open(ark_peer::who(&login), None);
    nobody.connect(&url, &server.auth.mint("no-such-session"));
    assert!(turned_away(&mut nobody, 10), "a session nobody wrote");

    // A second connection of the session, then the laptop signs out.
    let mut tab = open(ark_peer::who(&login), None);
    tab.connect(&url, &login.token);
    pump_until(&mut [&mut tab], 10, |ps| ps[0].linked());
    laptop.client.sign_out().unwrap();
    assert!(laptop.client.is_signed_out());
    assert!(
        turned_away(&mut tab, 10),
        "the other connection of the session closed and turned away"
    );
    assert!(hub_rows::<SignIn>(&server, "sign_in")[0].revoked);
    rt.block_on(server.stop());
}

/// A session past its expiry is closed by the hub and its token turned
/// away. Falsified by the hub's thread not asking `ended` on its tick: the
/// connection stayed open past its expiry.
#[test]
fn an_expired_session_is_closed_and_turned_away() {
    let rt = runtime();
    let auth = Arc::new(Auth::new::<harken_server::domain::Harken>(Mode::Dev, "http://127.0.0.1", b"key").with_ttl(1_500));
    let config = harken_server::hub_config(&Hosted::harken(), 100);
    let running = rt
        .block_on(
            ark_peer::Server::new(config)
                .auth(auth.clone())
                .listen("127.0.0.1:0"),
        )
        .unwrap();
    let login = auth
        .issue(
            &ark_auth::Account {
                id: "erin".into(),
                ..Default::default()
            },
            "a test",
        )
        .unwrap();
    let mut p = open(ark_peer::who(&login), None);
    p.connect(&format!("http://{}", running.addr), &login.token);
    pump_until(&mut [&mut p], 10, |ps| ps[0].linked());
    assert!(turned_away(&mut p, 10), "closed at expiry, and turned away");
    rt.block_on(running.stop());
}

/// A role an admin grants is held by its person's next connection, which
/// the hub makes them open by closing theirs; somebody else's grant is
/// refused. Falsified by the hub not watching the role table (`signs`
/// skipping `role`): bob's connection held its old roles.
#[test]
fn a_grant_judges_its_person_again() {
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |c| {
        c.roles = [("admin".to_string(), vec!["root".to_string()])].into();
    });
    let mut root = signed_in(&server, "root");
    let mut bob = signed_in(&server, "bob");
    pump_until(&mut [&mut root, &mut bob], 10, |ps| {
        ps.iter().all(|p| p.client.current().is_some())
    });
    let grant = Grant {
        user: "bob".into(),
        role: "library".into(),
    };
    bob.mutate("role.grant", &grant);
    assert!(
        bob.refused
            .iter()
            .any(|(_, why)| why == "only an admin grants roles"),
        "{:?}",
        bob.refused
    );
    root.mutate("role.grant", &grant);
    let session = bob.me();
    let bobs = |s: &harken_server::Server| {
        let bob = session.clone();
        s.running
            .hub
            .with(move |h| {
                h.connections()
                    .filter(|(_, w)| w.session == bob)
                    .map(|(_, w)| w.roles.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap()
    };
    let held = eventually(10, || {
        bob.pump();
        root.pump();
        bobs(&server)
            .into_iter()
            .find(|r| r.contains(&"library".to_string()))
    });
    assert!(
        held.is_some(),
        "bob's connection holds library: {:?}",
        bobs(&server)
    );
    pump_until(&mut [&mut bob], 10, |ps| {
        ps[0].client.me().roles.contains(&"library".to_string())
    });
    rt.block_on(server.stop());
}

/// Fed `session.feed`, a device holds the library and the playlists whole
/// and only its own person's sign-ins. Falsified by the scope holding
/// every session: alice's replica held bob's.
#[test]
fn a_scoped_server_holds_nobody_else_s_sign_ins() {
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |c| c.scoped = true);
    let mut alice = signed_in(&server, "alice");
    let mut bob = signed_in(&server, "bob");
    bob.mutate(
        "playlists.create_playlist",
        &CreatePlaylist {
            name: "Bob's".into(),
        },
    );
    pump_until(&mut [&mut alice, &mut bob], 10, |ps| {
        ps[1].pending() == 0
            && ps[0].rows::<Playlist>("playlist").len() == 1
            && ps[0].client.current().is_some()
    });
    assert_eq!(hub_rows::<SignIn>(&server, "sign_in").len(), 2);
    let users = |p: &Peer| {
        p.rows::<SignIn>("sign_in")
            .into_iter()
            .map(|s| s.user)
            .collect::<Vec<_>>()
    };
    assert_eq!(users(&alice), ["alice"]);
    assert_eq!(users(&bob), ["bob"]);
    std::thread::sleep(Duration::from_millis(50));
    rt.block_on(server.stop());
}
