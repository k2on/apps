//! harken's server as a whole: signing in through its routes and syncing on
//! its socket; work done before anybody signed in, and under an older login,
//! arriving when somebody does; the log and the logins surviving a restart;
//! a server that cannot sign anybody in refusing to start; and `/healthz`.

mod common;

use ark::format::Encode;
use ark::store::Change;
use common::*;
use harken_server::domain::playlists::favorites_of;
use harken_server::domain::CreatePlaylist;
use harken_server::domain::Playlist;
use harken_server::{start, Config, SignIn};

/// The playlists people made, by name and owner: the Favorites a first
/// sign-in makes apart.
fn playlists(server: &harken_server::Server) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = hub_rows::<Playlist>(server, "playlist")
        .into_iter()
        .filter(|p| p.id != favorites_of(&p.user_id))
        .map(|p| (p.name, p.user_id))
        .collect();
    out.sort();
    out
}

fn named(name: &str) -> CreatePlaylist {
    CreatePlaylist { name: name.into() }
}

/// The routes and the socket agree about who somebody is: a login from
/// `/auth/login` and `/auth/exchange` is a token the socket takes, the actor
/// is who the server verified, and another device of the same person sees
/// it; a token nobody issued is closed and keeps what it authored.
/// Falsified by `start` not handing the builder the sign-in (`.auth`): no
/// `/auth` routes, and the login failed.
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
    let mut laptop = open(ark_peer::who(&login), None);
    laptop.connect(&url, &login.token);
    laptop.mutate("playlists.create_playlist", &named("Road trip"));
    pump_until(&mut [&mut laptop], 10, |ps| ps[0].pending() == 0);
    assert_eq!(
        playlists(&server),
        [("Road trip".to_string(), "alice".to_string())]
    );

    let mut phone = signed_in(&server, "alice");
    pump_until(&mut [&mut phone], 10, |ps| ps[0].cursor() == head(&server));
    let mine: Vec<Playlist> = phone.query("playlists.playlists", &()).unwrap();
    assert_eq!(
        mine.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["Favorites", "Road trip"]
    );

    // A token nobody issued is turned away, and keeps what it authored.
    let mut mallory = open(
        ark::peer::Who {
            actor: "mallory".into(),
            session: "nope".into(),
            roles: vec![],
        },
        None,
    );
    mallory.connect(&url, "forged");
    mallory.mutate("playlists.create_playlist", &named("Mine now"));
    pump_until(&mut [&mut mallory], 10, |ps| ps[0].closed());
    std::thread::sleep(std::time::Duration::from_millis(100));
    mallory.pump();
    assert_eq!(mallory.pending(), 1);
    assert_eq!(playlists(&server).len(), 1);
    rt.block_on(server.stop());
}

/// Somebody uses the app before they have an account: what they make is
/// nobody's, kept, and dials nothing. They sign in, and it is theirs — the
/// client authors it again under the login and the server takes it like
/// any other entry. Then the same person, on a new login after the old one
/// was revoked, pushes what they authored offline under the old one, and
/// that is theirs too. Falsified by signing in without the client
/// authoring its pending intents again under the login: they were refused
/// as entries of another session.
#[test]
fn work_done_signed_out_or_under_an_older_login_arrives_as_its_owners() {
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |_| {});
    let url = server.url();

    let mut p = open(nobody(), None);
    p.mutate("playlists.create_playlist", &named("Before"));
    pump_until(&mut [&mut p], 1, |_| true);
    assert!(
        playlists(&server).is_empty(),
        "nothing is dialled signed out"
    );

    let first = ark_auth::client::login(&url, Some("dana"), |_| {}).unwrap();
    p.sign_in(&url, &first);
    pump_until(&mut [&mut p], 10, |ps| ps[0].pending() == 0);
    assert!(p.refused.is_empty(), "{:?}", p.refused);
    assert_eq!(
        playlists(&server),
        [("Before".to_string(), "dana".to_string())]
    );

    // Offline, then signed out of that login everywhere, then in again.
    p.disconnect();
    p.mutate("playlists.create_playlist", &named("On the train"));
    let second = ark_auth::client::login(&url, Some("dana"), |_| {}).unwrap();
    assert_ne!(second.session, first.session);
    let mut other = open(ark_peer::who(&second), None);
    other.connect(&url, &second.token);
    pump_until(&mut [&mut other], 10, |ps| ps[0].client.current().is_some());
    let revoke = ark_auth::Revoke {
        id: first.session.clone(),
    };
    other.mutate("session.revoke", &revoke);
    pump_until(&mut [&mut other], 10, |ps| ps[0].pending() == 0);
    assert!(other.refused.is_empty(), "{:?}", other.refused);
    drop(other);
    p.sign_in(&url, &second);
    pump_until(&mut [&mut p], 10, |ps| ps[0].pending() == 0);
    assert!(p.refused.is_empty(), "{:?}", p.refused);
    assert_eq!(playlists(&server).len(), 2);
    rt.block_on(server.stop());
}

/// The log and the logins are in the data directory, and a restart reads
/// them back: a token issued before still signs in. Falsified by `start`
/// not handing the builder the data directory: the restarted hub's log was
/// empty.
#[test]
fn the_log_and_the_logins_survive_a_restart() {
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |_| {});
    let login = ark_auth::client::login(&server.url(), Some("erin"), |_| {}).unwrap();
    let mut p = open(ark_peer::who(&login), None);
    p.connect(&server.url(), &login.token);
    p.mutate("playlists.create_playlist", &named("Kept"));
    pump_until(&mut [&mut p], 10, |ps| ps[0].pending() == 0);
    drop(p);
    rt.block_on(server.stop());

    let server = serve(&rt, data.path(), |_| {});
    assert_eq!(
        playlists(&server),
        [("Kept".to_string(), "erin".to_string())]
    );
    let mut again = open(ark_peer::who(&login), None);
    again.connect(&server.url(), &login.token);
    pump_until(&mut [&mut again], 10, |ps| ps[0].cursor() == head(&server));
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

/// `/healthz` says the log's head, the module it runs and who is
/// connected, a line each, as the fleet and the VM test read it. Falsified
/// by not putting harken's answer in front of the hub's own: it said `ok`
/// and nothing else.
#[test]
fn the_health_says_the_head_and_who_is_connected() {
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |_| {});
    let mut a = signed_in(&server, "alice");
    let mut b = signed_in(&server, "alice");
    a.mutate("playlists.create_playlist", &named("One"));
    pump_until(&mut [&mut a, &mut b], 10, |ps| {
        ps[0].pending() == 0 && ps[1].cursor() == head(&server)
    });
    let h = ureq::get(&format!("{}/healthz", server.url()))
        .call()
        .unwrap()
        .into_string()
        .unwrap();
    let lines: Vec<&str> = h.lines().collect();
    assert_eq!(lines[0], "ok", "{h}");
    // Two sign-ins, the first one's welcome, and the playlist.
    assert!(lines.contains(&"head 4"), "{h}");
    assert!(lines.contains(&"horizon 0"), "{h}");
    assert!(lines.contains(&"room alice peers 2"), "{h}");
    assert!(
        lines.contains(&format!("domain {} current", hosted().version()).as_str()),
        "{h}"
    );
    rt.block_on(server.stop());
}

/// `docs/plan-guards.md` D4 Only the authority writes raw. A client builds
/// the function an authority's raw write is (bob renaming alice's
/// playlist: a `Write` past every guard the domain has), loads its body
/// and authors it. It previews and is pushed; the authority, whose content
/// store holds only the functions of the modules its operator loaded,
/// answers `Held`, and no frame carries a body to it: the intent stays
/// pending on bob's device and alice's playlist keeps its name. Falsified
/// by putting back the `Push` with a body the client answered `Held` with
/// and the authority's loading of it (`ark::peer::Peer::push`): the
/// playlist was renamed `Hijacked`.
#[test]
fn a_clients_raw_write_is_refused() {
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |_| {});
    let mut alice = signed_in(&server, "alice");
    alice.mutate("playlists.create_playlist", &named("Mine"));
    pump_until(&mut [&mut alice], 10, |ps| ps[0].pending() == 0);
    let mine = hub_rows::<Playlist>(&server, "playlist").remove(0);
    let hijacked = Playlist {
        name: "Hijacked".into(),
        ..mine.clone()
    };
    let edit = Change::Edit {
        table: "playlist".into(),
        old: mine.to_bytes(),
        new: hijacked.to_bytes(),
    };
    // The authority's raw write before S3a, a function of no input.
    struct Raw(Change);
    impl ark::peer::Procedure for Raw {
        fn apply(
            &self,
            store: &mut ark::store::Store,
            _: &ark::peer::Ctx,
            _: &[u8],
        ) -> Result<Vec<Change>, ark::Refusal> {
            store.apply(&self.0)?;
            Ok(vec![self.0.clone()])
        }
    }
    let (h, raw) = ("raw write".to_string(), std::sync::Arc::new(Raw(edit)));
    let mut bob = signed_in(&server, "bob");
    pump_until(&mut [&mut bob], 10, |ps| ps[0].cursor() == head(&server));
    bob.client.log.functions.insert(h.clone(), raw);
    let _ = bob
        .client
        .author(ark::schema::Space::Durable, &h, vec![], ark_peer::now_ms());
    for _ in 0..100 {
        bob.pump();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(
        playlists(&server),
        [("Mine".to_string(), "alice".to_string())]
    );
    assert_eq!(
        (bob.pending(), bob.refused.len()),
        (1, 0),
        "held, not refused"
    );
    rt.block_on(server.stop());
}

/// A person's first sign-in gives them Favorites, made by the server: one,
/// theirs, however many devices sign in after it and before any of them
/// has heard the log; the server's own scanner gets none. Falsified by
/// `start` not naming `playlists.welcome` (`Auth::welcome`): nobody has a
/// playlist.
#[test]
fn a_first_sign_in_makes_favorites_once() {
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |_| {});
    login(&server, "alice");
    login(&server, "alice");
    login(&server, "bob");
    let mut phone = signed_in(&server, "alice");
    pump_until(&mut [&mut phone], 10, |ps| ps[0].cursor() == head(&server));
    let mut made: Vec<(String, String, bool)> = hub_rows::<Playlist>(&server, "playlist")
        .into_iter()
        .map(|p| (p.name, p.user_id.clone(), p.id == favorites_of(&p.user_id)))
        .collect();
    made.sort();
    assert_eq!(
        made,
        [
            ("Favorites".to_string(), "alice".to_string(), true),
            ("Favorites".to_string(), "bob".to_string(), true)
        ]
    );
    let mine: Vec<Playlist> = phone.query("playlists.playlists", &()).unwrap();
    assert_eq!(
        mine.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["Favorites"]
    );
}
