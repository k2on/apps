//! The sign-in routes over real HTTP, in dev mode — the provider is the one
//! part a test machine cannot have, and everything after the account is
//! known is the same code either way.

mod common;

use std::sync::Arc;

use ark_auth::server::{router, Auth, Mode};
use common::{App, Memory};

fn serve() -> (tokio::runtime::Runtime, String, Arc<Auth>, Memory) {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let (addr, auth) = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let auth = Arc::new(Auth::new::<App>(Mode::Dev, &format!("http://{addr}"), b"key"));
        let app = router(auth.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (addr, auth)
    });
    let rows = Memory::new();
    auth.serve(Box::new(rows.clone()));
    (rt, format!("http://{addr}"), auth, rows)
}

/// A dev login by name is a code, then a login whose session is the row
/// written for it — the device the program said it is — and whose token
/// names it; two logins are two sessions. Falsified by `login` handing the
/// dev form's name to `finish` without its device: the row's was empty.
#[test]
fn a_dev_login_is_a_code_then_a_session_row_and_its_token() {
    let (_rt, server, auth, rows) = serve();
    // Headless: a dev server given a name answers with the code at once.
    let login = ark_auth::client::login(&server, Some("alice"), |url| panic!("a browser was asked to open {url}")).expect("signed in");
    assert_eq!(login.user.id, "alice");
    let row = rows.sign_ins()[0].clone();
    assert_eq!((row.id.as_str(), row.user.as_str()), (login.session.as_str(), "alice"));
    assert!(row.device.starts_with("ureq"), "{}", row.device);
    assert_eq!(auth.verify(&login.token), Some(login.session.as_str()));
    let again = ark_auth::client::login(&server, Some("alice"), |_| {}).unwrap();
    assert_ne!(again.session, login.session);
    assert_eq!(rows.sign_ins().len(), 2);
}

#[test]
fn a_code_is_single_use() {
    let (_rt, server, ..) = serve();
    let url = ark_auth::login_url(&server, "http://127.0.0.1:9/", Some("bob"));
    let resp = ureq::AgentBuilder::new().redirects(0).build().get(&url).call().unwrap();
    assert_eq!(resp.status(), 303);
    let location = resp.header("Location").unwrap().to_string();
    let code = ark_auth::query_value(location.split_once('?').unwrap().1, "code").unwrap();
    assert_eq!(ark_auth::client::exchange(&server, &code).unwrap().user.id, "bob");
    let second = ark_auth::client::exchange(&server, &code);
    assert!(second.unwrap_err().contains("used"), "the second exchange is refused");
}

#[test]
fn a_code_cannot_go_somewhere_else() {
    let (_rt, server, ..) = serve();
    let url = ark_auth::login_url(&server, "https://elsewhere.example/", Some("alice"));
    let err = ureq::AgentBuilder::new().redirects(0).build().get(&url).call().unwrap_err();
    let ureq::Error::Status(400, resp) = err else {
        panic!("refused with 400, not {err}");
    };
    assert!(resp.into_string().unwrap().contains("cannot be sent"));
}

#[test]
fn a_dev_login_without_a_name_is_a_form() {
    let (_rt, server, ..) = serve();
    let url = ark_auth::login_url(&server, "http://127.0.0.1:9/", None);
    let body = ureq::get(&url).call().unwrap().into_string().unwrap();
    assert!(body.contains("<form") && body.contains("takes your word"), "{body}");
}

/// A signed-in device makes a code; another redeems it, once, for a session
/// of its own as the same person. A second try, an unknown code, a token
/// that is not a session's, and too many codes waiting are each refused in
/// a sentence. Falsified by `redeem_device_code` not marking the code used.
#[test]
fn a_device_code_signs_another_device_in_once() {
    let (_rt, server, auth, rows) = serve();
    let first = ark_auth::client::login(&server, Some("alice"), |_| {}).unwrap();
    let (code, expires) = ark_auth::client::device_code(&server, &first.token).unwrap();
    assert!(expires > 0);
    let second = ark_auth::client::redeem(&server, &code).unwrap();
    assert_eq!(second.user.id, "alice");
    assert_ne!(second.session, first.session);
    assert_eq!(rows.sign_ins().len(), 2);
    assert_eq!(ark_auth::client::redeem(&server, &code).unwrap_err(), ark_auth::server::CODE_USED);
    assert_eq!(ark_auth::client::redeem(&server, "nope").unwrap_err(), ark_auth::server::CODE_UNKNOWN);
    assert!(ark_auth::client::device_code(&server, "x.y").is_err());
    for _ in 0..ark_auth::server::DEVICE_CODES_PER_ACCOUNT {
        auth.device_code(&first.token).unwrap();
    }
    assert!(auth.device_code(&first.token).unwrap_err().contains("enough codes"));
}

/// A page's code is traded at `/auth/session` once, as at `/auth/exchange`:
/// the login comes back without its token, which is the cookie set —
/// `HttpOnly`, `SameSite=Strict`, the origin's, lasting as the session
/// does, and `Secure` only over https — and `/auth/logout` sets it empty.
/// Falsified by answering the login whole: the body held the token.
#[test]
fn a_page_s_session_is_a_strict_http_only_cookie() {
    let (_rt, server, auth, _) = serve();
    let agent = ureq::AgentBuilder::new().redirects(0).build();
    let resp = agent
        .get(&ark_auth::login_url(&server, &format!("{server}/"), Some("ann")))
        .call()
        .unwrap();
    let location = resp.header("Location").unwrap().to_string();
    let code = ark_auth::query_value(location.split_once('?').unwrap().1, "code").unwrap();
    let trade = || {
        agent
            .post(&format!("{server}/auth/session"))
            .set("Content-Type", "application/json")
            .send_string(&serde_json::json!({ "code": code }).to_string())
            .map_err(Box::new)
    };
    let resp = trade().unwrap();
    let set = resp.header("Set-Cookie").unwrap().to_string();
    let login: ark_auth::Login = serde_json::from_str(&resp.into_string().unwrap()).unwrap();
    assert_eq!((login.user.id.as_str(), login.token.as_str()), ("ann", ""));
    let token = ark_auth::cookie_token(set.split(';').next().unwrap()).unwrap();
    assert_eq!(auth.verify(token), Some(login.session.as_str()));
    assert!(set.contains("; Path=/; HttpOnly; SameSite=Strict; Max-Age="), "{set}");
    let age: i64 = set.split("Max-Age=").nth(1).unwrap().split(';').next().unwrap().parse().unwrap();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    assert!((age - (login.expires_ms - now) / 1000).abs() < 5, "{age}");
    assert!(!set.contains("Secure"), "plain http on loopback");
    assert!(matches!(trade().map_err(|e| *e), Err(ureq::Error::Status(400, _))), "once");
    let out = agent.post(&format!("{server}/auth/logout")).call().unwrap();
    assert!(out
        .header("Set-Cookie")
        .unwrap()
        .starts_with("ark-session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0"));
    let tls = Auth::new::<App>(Mode::Dev, "https://money.example", b"key");
    assert!(tls.cleared_cookie().ends_with("; Secure"));
}
