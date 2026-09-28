//! The sign-in routes over real HTTP, in dev mode — the provider is the one
//! part a test machine cannot have, and everything after the account is
//! known is the same code either way.

use std::sync::Arc;

use ark_auth::server::{router, Auth, Mode};
use ark_auth::session::SessionStore;

fn serve() -> (tokio::runtime::Runtime, String, Arc<Auth>) {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let (addr, auth) = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let auth = Arc::new(Auth::new(SessionStore::memory(), Mode::Dev, &format!("http://{addr}")));
        let app = router(auth.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (addr, auth)
    });
    (rt, format!("http://{addr}"), auth)
}

#[test]
fn a_dev_login_is_a_code_then_a_token_then_nothing() {
    let (_rt, server, auth) = serve();
    // Headless: a dev server given a name answers with the code at once.
    let login = ark_auth::client::login(&server, Some("alice"), |url| panic!("a browser was asked to open {url}")).expect("signed in");
    assert_eq!(login.user.id, "alice");
    assert!(!login.token.is_empty() && !login.session.is_empty());
    // The token proves the session, over HTTP and to the engine alike.
    assert_eq!(
        ark_auth::client::whoami(&server, &login.token).unwrap().map(|l| l.session),
        Some(login.session.clone())
    );
    let who = auth.authenticator();
    assert_eq!(who(Some(&login.token)).map(|i| i.user), Some("alice".to_string()));
    // Two logins by one person are two sessions: two devices.
    let again = ark_auth::client::login(&server, Some("alice"), |_| {}).unwrap();
    assert_ne!(again.session, login.session);
    // Signed out: the token proves nothing, and the session is still hers.
    ark_auth::client::logout(&server, &login.token).unwrap();
    assert_eq!(ark_auth::client::whoami(&server, &login.token).unwrap(), None);
    assert!(who(Some(&login.token)).is_none());
    assert!(auth.owns("alice", &login.session));
    assert!(who(Some(&again.token)).is_some(), "the other device is still signed in");
}

#[test]
fn a_code_is_single_use() {
    let (_rt, server, _auth) = serve();
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
    let (_rt, server, _auth) = serve();
    let url = ark_auth::login_url(&server, "https://elsewhere.example/", Some("alice"));
    let err = ureq::AgentBuilder::new().redirects(0).build().get(&url).call().unwrap_err();
    let ureq::Error::Status(400, resp) = err else {
        panic!("refused with 400, not {err}");
    };
    assert!(resp.into_string().unwrap().contains("cannot be sent"));
}

#[test]
fn a_dev_login_without_a_name_is_a_form() {
    let (_rt, server, _auth) = serve();
    let url = ark_auth::login_url(&server, "http://127.0.0.1:9/", None);
    let body = ureq::get(&url).call().unwrap().into_string().unwrap();
    assert!(body.contains("<form") && body.contains("takes your word"), "{body}");
}
