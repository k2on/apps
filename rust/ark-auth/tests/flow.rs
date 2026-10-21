//! The sign-in routes over real HTTP, in dev mode — the provider is the one
//! part a test machine cannot have, and everything after the account is
//! known is the same code either way.

use std::sync::{Arc, Mutex};

use ark_auth::server::{router, Auth, Mode, Opened};

type Rows = Arc<Mutex<Vec<Opened>>>;

fn serve() -> (tokio::runtime::Runtime, String, Arc<Auth>, Rows) {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let (addr, auth) = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let auth = Arc::new(Auth::new(Mode::Dev, &format!("http://{addr}"), b"key"));
        let app = router(auth.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (addr, auth)
    });
    let rows = Rows::default();
    let r = rows.clone();
    auth.on_sign_in(move |o| Ok(r.lock().unwrap().push(o.clone())));
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
    let row = rows.lock().unwrap()[0].clone();
    assert_eq!((row.id.as_str(), row.user.as_str()), (login.session.as_str(), "alice"));
    assert!(row.device.starts_with("ureq"), "{}", row.device);
    assert_eq!(auth.verify(&login.token), Some(login.session.as_str()));
    let again = ark_auth::client::login(&server, Some("alice"), |_| {}).unwrap();
    assert_ne!(again.session, login.session);
    assert_eq!(rows.lock().unwrap().len(), 2);
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
