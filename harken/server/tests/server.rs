//! harken's server as a whole: signing in through its routes and syncing on
//! its socket; work done before anybody signed in, and under an older login,
//! arriving when somebody does; the log and the logins surviving a restart;
//! a server that cannot sign anybody in refusing to start; `/healthz`; and
//! the admin page's listener, over its wire.

mod common;

use ark5::ir::Node;
use ark5::store::Change;
use common::*;
use harken_domain5::playlists::CreatePlaylist;
use harken_domain5::schema::Playlist;
use harken_server::{start, Config, SignIn};

fn playlists(server: &harken_server::Server) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = hub_rows::<Playlist>(server, "playlist")
        .into_iter()
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
    let mut laptop = open(ark5_client::auth::who(&login), None);
    laptop.connect(&url, &login.token);
    laptop.mutate("playlists.create_playlist", &named("Road trip"));
    pump_until(&mut [&mut laptop], 10, |ps| ps[0].pending() == 0);
    assert_eq!(
        playlists(&server),
        [("Road trip".to_string(), "alice".to_string())]
    );

    let mut phone = signed_in(&server, "alice");
    pump_until(&mut [&mut phone], 10, |ps| ps[0].cursor() == 1);
    let mine: Vec<Playlist> = phone.query("playlists.playlists", &()).unwrap();
    assert_eq!(
        mine.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["Road trip"]
    );

    // A token nobody issued is turned away, and keeps what it authored.
    let mut mallory = open(
        ark5::peer::Who {
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
    ark_auth::client::logout(&url, &first.token).unwrap();
    let second = ark_auth::client::login(&url, Some("dana"), |_| {}).unwrap();
    assert_ne!(second.session, first.session);
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
    let mut p = open(ark5_client::auth::who(&login), None);
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
    let mut again = open(ark5_client::auth::who(&login), None);
    again.connect(&server.url(), &login.token);
    pump_until(&mut [&mut again], 10, |ps| ps[0].cursor() == 1);
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
        ps[0].pending() == 0 && ps[1].cursor() == 1
    });
    let h = ureq::get(&format!("{}/healthz", server.url()))
        .call()
        .unwrap()
        .into_string()
        .unwrap();
    let lines: Vec<&str> = h.lines().collect();
    assert_eq!(lines[0], "ok", "{h}");
    assert!(lines.contains(&"head 1"), "{h}");
    assert!(lines.contains(&"horizon 0"), "{h}");
    assert!(lines.contains(&"room alice peers 2"), "{h}");
    assert!(
        lines.contains(&format!("module {} current", hosted().domain.module.hash()).as_str()),
        "{h}"
    );
    rt.block_on(server.stop());
}

/// What the admin page reads its answers as: the bytes of a body in hex,
/// and a status other than 200 as `Err(status, text)`.
fn admin(
    method: &str,
    url: &str,
    token: Option<&str>,
    body: Option<String>,
) -> Result<String, (u16, String)> {
    let mut req = ureq::request(method, url);
    if let Some(t) = token {
        req = req.set("Authorization", &format!("Bearer {t}"));
    }
    let r = match body {
        Some(b) => req.send_string(&b),
        None => req.call(),
    };
    match r {
        Ok(r) => Ok(r.into_string().unwrap()),
        Err(ureq::Error::Status(n, r)) => Err((n, r.into_string().unwrap())),
        Err(e) => panic!("{url}: {e}"),
    }
}

/// `docs/plan-guards.md` D4 The admin page, where `HARKEN_ADMIN_BIND` asks
/// for one, over the page's own wire (`ark_explorer::wire`, hex bodies): on
/// loopback it asks nothing; the state it is shown reads back as the page
/// reads it — harken's module, every body checking into a library of the
/// page's own, the authority's rows and its log's lines by name; a raw
/// write it asks for is the authority's entry, in the hub and in every
/// peer; a raw write over a row that is no longer there is refused; and a
/// binding it authors is in the log at the sequence it is answered with.
/// Falsified by not handing the builder the admin page (nothing listened),
/// and by a raw write that replaced the row without checking it was the
/// one the change replaces (the stale write was taken).
#[test]
fn the_admin_page_is_the_authoritys() {
    use ark_explorer::wire::{self, Hello, State};
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |c| {
        c.admin_bind = Some("127.0.0.1:0".into())
    });
    let at = format!(
        "http://{}",
        server.running.admin_addr.expect("the admin page listens")
    );
    let mut phone = signed_in(&server, "alice");
    phone.mutate("playlists.create_playlist", &named("Mine"));
    pump_until(&mut [&mut phone], 10, |ps| ps[0].pending() == 0);

    let hello: Hello =
        wire::read(&admin("GET", &format!("{at}/admin/api/hello"), None, None).unwrap()).unwrap();
    // Where it signs in is the public URL, `http://{listen}` when unset.
    assert_eq!(
        hello,
        Hello {
            open: true,
            server: Some("http://127.0.0.1:0".into())
        }
    );
    let state =
        State::from_body(&admin("GET", &format!("{at}/admin/api/state"), None, None).unwrap())
            .unwrap();
    let module = ark5::module::Module::from_bytes(&state.module).unwrap();
    assert_eq!(module, hosted().domain.module);
    let lib = ark5::module::Lib::default();
    let mut left = state.bodies.clone();
    while !left.is_empty() {
        let before = left.len();
        left.retain(|b| lib.load(&module.schema, b).is_err());
        assert!(left.len() < before, "{} bodies do not check", left.len());
    }
    ark5::module::verify(&module, &lib).unwrap();
    let rows = &state.rows.iter().find(|(t, _)| t == "playlist").unwrap().1;
    let mine = Playlist::from_bytes(&rows[0]).unwrap();
    assert_eq!((mine.name.as_str(), state.head), ("Mine", 1));
    assert_eq!(
        state.lines.last().unwrap().function,
        "playlists.create_playlist"
    );
    assert!(
        state.raw && state.connections.iter().any(|c| c.who.starts_with("alice")),
        "{:?}",
        state.connections
    );

    let edit = |old: &Playlist, name: &str| {
        let new = Playlist {
            name: name.into(),
            ..old.clone()
        };
        wire::raw_body(&Change::Edit {
            table: "playlist".into(),
            old: old.to_bytes(),
            new: new.to_bytes(),
        })
    };
    let raw = format!("{at}/admin/api/raw");
    assert_eq!(
        admin("POST", &raw, None, Some(edit(&mine, "Kept"))),
        Ok("2".into())
    );
    pump_until(&mut [&mut phone], 10, |ps| {
        ps[0]
            .rows::<Playlist>("playlist")
            .iter()
            .any(|p| p.name == "Kept")
    });
    assert_eq!(
        playlists(&server),
        [("Kept".to_string(), "alice".to_string())]
    );
    let (status, why) = admin("POST", &raw, None, Some(edit(&mine, "Stale"))).unwrap_err();
    assert_eq!(
        (status, why.as_str()),
        (409, "the row is not the one the change replaces")
    );

    let body = wire::author_body(
        "playlists.create_playlist",
        &named("The house's").to_bytes(),
    );
    assert_eq!(
        admin("POST", &format!("{at}/admin/api/author"), None, Some(body)),
        Ok("3".into())
    );
    let state =
        State::from_body(&admin("GET", &format!("{at}/admin/api/state"), None, None).unwrap())
            .unwrap();
    let names: Vec<&str> = state.lines.iter().map(|l| l.function.as_str()).collect();
    assert_eq!(
        names,
        [
            "playlists.create_playlist",
            "raw write",
            "playlists.create_playlist"
        ]
    );
    assert_eq!(state.lines[2].entry.actor, state.who);
    rt.block_on(server.stop());
}

/// `docs/plan-guards.md` D4 Bound wider than loopback, the admin page asks
/// for a login, and the login must hold `admin`: none is 401, one without
/// the role 403, one `HARKEN_ROLES` grants it is let in. Falsified by
/// reading no login from the bearer: the login without the role was
/// answered 401, not 403.
#[test]
fn a_wider_admin_page_asks_for_the_admin_role() {
    use ark_explorer::wire::{self, Hello, State};
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |c| {
        c.admin_bind = Some("0.0.0.0:0".into());
        c.roles = [("admin".to_string(), vec!["root".to_string()])]
            .into_iter()
            .collect();
    });
    let at = format!(
        "http://127.0.0.1:{}",
        server.running.admin_addr.unwrap().port()
    );
    let hello: Hello =
        wire::read(&admin("GET", &format!("{at}/admin/api/hello"), None, None).unwrap()).unwrap();
    assert!(!hello.open);
    let state = format!("{at}/admin/api/state");
    assert_eq!(admin("GET", &state, None, None).unwrap_err().0, 401);
    let alice = login(&server, "alice");
    assert_eq!(
        admin("GET", &state, Some(&alice.token), None)
            .unwrap_err()
            .0,
        403
    );
    let root = login(&server, "root");
    let s = State::from_body(&admin("GET", &state, Some(&root.token), None).unwrap()).unwrap();
    assert_eq!(s.head, 0);
    rt.block_on(server.stop());
}

/// `docs/plan-guards.md` D4 Only the authority writes raw. A client builds
/// the function the admin page's raw write is (bob renaming alice's
/// playlist: a `Write` past every guard the domain has), loads its body
/// and authors it. It previews and is pushed; the authority, whose content
/// store holds only the functions of the modules its operator loaded,
/// answers `Held`, and no frame carries a body to it: the intent stays
/// pending on bob's device and alice's playlist keeps its name. Falsified
/// by putting back the `Push` with a body the client answered `Held` with
/// and the authority's loading of it (`ark5::peer::Peer::push`): the
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
    let raw = ark5_server::admin::raw(&hosted().domain.module, &edit).unwrap();
    let mut bob = signed_in(&server, "bob");
    pump_until(&mut [&mut bob], 10, |ps| ps[0].cursor() == 1);
    let h = bob.client.log.cas.add_body(raw.to_bytes());
    bob.client.mutate(h, vec![], ark5_server::handle::now());
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
