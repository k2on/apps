//! money's server as a whole: signing in through its routes and syncing the
//! household's budget on its socket; a phone's sign-in, its code sent back
//! to `money://`; a token nobody issued turned away; the budget surviving
//! a restart; and the importer's role, which nobody signing in holds.

mod common;

use common::*;
use money_core::*;

fn open_account(name: &str) -> OpenAccount {
    OpenAccount {
        name: name.into(),
        kind: money_core::accounts::AccountKind::Checking,
        on_budget: true,
        balance: 10_000,
        date: "2026-10-01".into(),
    }
}

fn names(server: &money_server::Server) -> Vec<String> {
    let mut n: Vec<String> = hub_rows::<Account>(server, "account").into_iter().map(|a| a.name).collect();
    n.sort();
    n
}

/// The routes and the socket agree about who somebody is: a login from
/// `/auth/login` and `/auth/exchange` is a token the socket takes, and the
/// household's other member sees what was written, entered by whom. A token
/// nobody issued is closed and keeps what it authored. Falsified by `start`
/// not handing the builder the sign-in (`.auth`): no `/auth` routes, and
/// the login failed.
#[test]
fn one_household_signs_in_and_shares_one_budget() {
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |_| {});
    let url = server.url();
    let login = ark_auth::client::login(&url, Some("alice"), |u| panic!("dev auth opens no browser: {u}")).unwrap();
    let mut laptop = open(ark_peer::who(&login));
    laptop.connect(&url, &login.token);
    laptop.mutate("accounts.open_account", &open_account("Checking"));
    pump_until(&mut [&mut laptop], 10, |ps| ps[0].pending() == 0);
    assert_eq!(names(&server), ["Checking"]);

    let mut sam = signed_in(&server, "sam");
    pump_until(&mut [&mut sam], 10, |ps| ps[0].cursor() == head(&server));
    let accounts: Vec<AccountLine> = sam.query("accounts.accounts", &()).unwrap();
    assert_eq!(accounts.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["Checking"]);
    let txns: Vec<Txn> = hub_rows(&server, "txn");
    assert!(txns.iter().all(|t| t.user_id == "alice"), "entered by alice");

    let mut mallory = open(ark::peer::Who {
        actor: "mallory".into(),
        session: "nope".into(),
        roles: vec![],
    });
    mallory.connect(&url, "forged");
    mallory.mutate("accounts.open_account", &open_account("Mine now"));
    pump_until(&mut [&mut mallory], 10, |ps| ps[0].closed());
    assert_eq!(mallory.pending(), 1);
    assert_eq!(names(&server), ["Checking"]);
    rt.block_on(server.stop());
}

/// A phone signs in as the browser does, through the server — the only
/// OpenID Connect client — with its own scheme as the redirect: the code
/// comes back to `money://…` and is traded for a login at
/// `/auth/exchange`. A redirect anywhere else unconfigured is refused.
/// Falsified by not allowing `PHONE_SCHEME`: the login was a 400.
#[test]
fn a_phone_signs_in_through_its_scheme() {
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |_| {});
    let url = server.url();
    let agent = ureq::AgentBuilder::new().redirects(0).build();
    let back = agent
        .get(&format!("{url}/auth/login"))
        .query("redirect", "money://signed-in")
        .query("user", "alice")
        .call()
        .unwrap();
    assert!((300..400).contains(&back.status()), "{}", back.status());
    let to = back.header("location").unwrap().to_string();
    assert!(to.starts_with("money://signed-in?code="), "{to}");
    let code = ark_auth::query_value(to.split_once('?').unwrap().1, "code").unwrap();
    let login = ark_auth::client::exchange(&url, &code).unwrap();
    assert_eq!(login.user.id, "alice");
    let mut phone = open(ark_peer::who(&login));
    phone.connect(&url, &login.token);
    pump_until(&mut [&mut phone], 10, |ps| ps[0].client.linked);

    let refused = agent
        .get(&format!("{url}/auth/login"))
        .query("redirect", "https://elsewhere.example/")
        .query("user", "alice")
        .call();
    assert!(matches!(refused, Err(ureq::Error::Status(400, _))), "{refused:?}");
    rt.block_on(server.stop());
}

/// Nobody who signs in holds `bank`: an import from a household member is
/// refused on the device as the hub would, and nothing lands. Falsified by
/// granting the role to everybody: the import landed.
#[test]
fn only_the_importer_imports() {
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |_| {});
    let mut alice = signed_in(&server, "alice");
    pump_until(&mut [&mut alice], 10, |ps| ps[0].client.linked);
    let import = Import {
        bank: "chase".into(),
        cursor: "c1".into(),
        ..Import::default()
    };
    alice.mutate("banks.import", &import);
    assert_eq!(alice.refused, ["only the bank importer imports"]);
    assert!(hub_rows::<Bank>(&server, "bank").is_empty());
    rt.block_on(server.stop());
}

/// The budget is in the data directory: a restart holds it, and a login
/// from before the restart still signs in.
#[test]
fn the_budget_and_the_logins_survive_a_restart() {
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |_| {});
    let l = login(&server, "alice");
    let mut alice = open(ark_peer::who(&l));
    alice.connect(&server.url(), &l.token);
    alice.mutate("accounts.open_account", &open_account("Savings"));
    pump_until(&mut [&mut alice], 10, |ps| ps[0].pending() == 0);
    rt.block_on(server.stop());

    let server = serve(&rt, data.path(), |_| {});
    assert_eq!(names(&server), ["Savings"]);
    let mut again = open(ark_peer::who(&l));
    again.connect(&server.url(), &l.token);
    pump_until(&mut [&mut again], 10, |ps| ps[0].client.linked && ps[0].cursor() == head(&server));
    rt.block_on(server.stop());
}

/// `/healthz` says what domain the server runs — its schema's hash and
/// version, as `domain <hash> <version> current` — which is what a phone
/// holds its own build to, and how many are connected: alice, and the
/// importer, which every server runs for its receipts folder.
#[test]
fn health_says_which_domain() {
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |_| {});
    let mut alice = signed_in(&server, "alice");
    pump_until(&mut [&mut alice], 10, |ps| ps[0].client.linked);
    let text = ureq::get(&format!("{}/healthz", server.url())).call().unwrap().into_string().unwrap();
    let d = money_core::shared();
    assert!(text.starts_with("ok\n"), "{text}");
    assert!(
        text.contains(&format!("domain {} {} current\n", d.schema_hash(), money_core::VERSION)),
        "{text}"
    );
    assert!(text.contains("connections 2\n"), "{text}");
    rt.block_on(server.stop());
}

/// One process: the browser client's build is served by the server that
/// holds the socket and sign-in — a deep link answered with its
/// index.html, the module with the build as its validator — and
/// `/healthz`, money's middleware, is still over all of it.
#[test]
fn the_browser_client_is_served_beside_the_socket() {
    let rt = runtime();
    let (data, web) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    std::fs::write(web.path().join("index.html"), "<!doctype html><title>Money</title>").unwrap();
    std::fs::create_dir(web.path().join("pkg")).unwrap();
    std::fs::write(web.path().join("pkg/money-web.js"), "export default 1").unwrap();
    let dir = web.path().to_path_buf();
    let server = serve(&rt, data.path(), move |c| c.web = Some(dir));
    let get = |path: &str| ureq::get(&format!("{}{path}", server.url())).call().unwrap();
    for deep in ["/", "/budget/2026-10/category/0a1b/?inspector=1"] {
        let page = get(deep);
        assert_eq!(page.header("cache-control"), Some("no-cache"));
        assert!(page.header("etag").is_some());
        assert!(page.into_string().unwrap().contains("<title>Money</title>"), "{deep}");
    }
    assert_eq!(get("/pkg/money-web.js").into_string().unwrap(), "export default 1");
    assert!(get("/healthz").into_string().unwrap().contains("\ndomain "));
    let mut alice = signed_in(&server, "alice");
    pump_until(&mut [&mut alice], 10, |ps| ps[0].client.linked);
    rt.block_on(server.stop());
}

/// A page signs in as the phones do and keeps its session where no script
/// reads it: `/auth/session` trades the code for the login without its
/// token, which is set as an `HttpOnly`, `SameSite=Strict` cookie, and the
/// socket opened from the server's own origin with that cookie and an
/// empty first message is that session — the household's budget comes. A
/// socket from another origin with the same cookie proves nobody, and so
/// does an empty message with no cookie. Falsified by taking the cookie
/// whatever the origin: the other site's socket synced.
#[test]
fn a_page_s_session_is_its_cookie_and_its_origin() {
    use tungstenite::client::IntoClientRequest;
    use tungstenite::Message;

    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |_| {});
    let url = server.url();
    let mut laptop = signed_in(&server, "alice");
    laptop.mutate("accounts.open_account", &open_account("Checking"));
    pump_until(&mut [&mut laptop], 10, |ps| ps[0].pending() == 0);

    // The page's own address is a redirect the server allows.
    let agent = ureq::AgentBuilder::new().redirects(0).build();
    let back = agent
        .get(&format!("{url}/auth/login"))
        .query("redirect", &format!("{url}/budget/"))
        .query("user", "sam")
        .call()
        .unwrap();
    let location = back.header("Location").unwrap().to_string();
    assert!(location.starts_with(&format!("{url}/budget/?code=")), "{location}");
    let code = ark_auth::query_value(location.split_once('?').unwrap().1, "code").unwrap();
    let answer = agent
        .post(&format!("{url}/auth/session"))
        .send_json(serde_json::json!({ "code": code }))
        .unwrap();
    let cookie = answer.header("Set-Cookie").unwrap().to_string();
    for part in ["HttpOnly", "SameSite=Strict", "Path=/"] {
        assert!(cookie.contains(part), "{cookie}");
    }
    let login: ark_auth::Login = answer.into_json().unwrap();
    assert_eq!(
        (login.user.id.as_str(), login.token.as_str()),
        ("sam", ""),
        "the token is the cookie's alone"
    );
    let pair = cookie.split(';').next().unwrap().to_string();
    assert!(server.auth.verify(ark_auth::cookie_token(&pair).unwrap()) == Some(login.session.as_str()));

    // A socket from `origin`, sending `cookie`, its first message empty.
    let socket = |origin: &str, cookie: Option<&str>| {
        let mut req = ark_auth::socket_url(&url).into_client_request().unwrap();
        req.headers_mut().insert("Origin", origin.parse().unwrap());
        if let Some(c) = cookie {
            req.headers_mut().insert("Cookie", c.parse().unwrap());
        }
        let (mut ws, _) = tungstenite::connect(req).unwrap();
        if let tungstenite::stream::MaybeTlsStream::Plain(s) = ws.get_mut() {
            s.set_read_timeout(Some(std::time::Duration::from_millis(20))).unwrap();
        }
        ws.send(Message::Binary(vec![])).unwrap();
        ws
    };
    // Closed with 4401 within five seconds; anything else is not denied.
    let denied = |mut ws: tungstenite::WebSocket<_>| {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < until {
            match ws.read() {
                Ok(Message::Close(Some(f))) => return u16::from(f.code) == 4401,
                Ok(Message::Close(None)) | Err(tungstenite::Error::ConnectionClosed) => return false,
                Ok(_) => {}
                Err(tungstenite::Error::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => panic!("{e}"),
            }
        }
        false
    };
    assert!(denied(socket("https://elsewhere.example", Some(&pair))), "another site's page is nobody");
    assert!(denied(socket(&url, None)), "no cookie is nobody");

    // The page's own: a peer driven by hand over it, as the page's is.
    let mut page = open(ark_peer::who(&login));
    let mut ws = socket(&url, Some(&pair));
    page.client.connected();
    let synced = eventually(10, || {
        for f in page.client.take_frames() {
            ws.send(Message::Binary(f)).unwrap();
        }
        loop {
            match ws.read() {
                Ok(Message::Binary(b)) => page.client.receive(b),
                Ok(Message::Close(f)) => panic!("closed: {f:?}"),
                Ok(_) => {}
                Err(tungstenite::Error::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => panic!("{e}"),
            }
        }
        page.cursor() == head(&server)
    });
    assert!(synced, "the page's session synced");
    let accounts: Vec<AccountLine> = page.query("accounts.accounts", &()).unwrap();
    assert_eq!(accounts.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["Checking"]);

    // Signing out forgets the cookie.
    let out = agent.post(&format!("{url}/auth/logout")).call().unwrap();
    assert!(out.header("Set-Cookie").unwrap().contains("Max-Age=0"));
    rt.block_on(server.stop());
}
