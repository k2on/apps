//! The relying party against a provider: discovery, the login's redirect to
//! the provider, the callback's code exchange and the ID token's checks,
//! through the server's own routes, over real HTTP. The provider is a few
//! axum routes here that sign their ID tokens HS256 with the client secret
//! (which `openidconnect` verifies, as it would RS256 against the key set).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ark_auth::oidc::Provider;
use ark_auth::server::{router, Auth, Mode, Opened};
use axum::extract::{Form, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

const SECRET: &str = "a client secret of thirty-two bytes or more";

/// What the provider is to put in its next ID token, and what it was sent.
#[derive(Default)]
struct Fake {
    issuer: String,
    nonce: String,
    key: String,
    asked: Vec<HashMap<String, String>>,
    /// The session rows the server wrote.
    rows: Vec<Opened>,
}

fn jwt(claims: serde_json::Value, key: &str) -> String {
    let head = B64.encode(br#"{"alg":"HS256","typ":"JWT"}"#);
    let body = B64.encode(claims.to_string());
    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes()).unwrap();
    mac.update(format!("{head}.{body}").as_bytes());
    format!("{head}.{body}.{}", B64.encode(mac.finalize().into_bytes()))
}

async fn token(State(fake): State<Arc<Mutex<Fake>>>, Form(form): Form<HashMap<String, String>>) -> Json<serde_json::Value> {
    let mut f = fake.lock().unwrap();
    f.asked.push(form);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let claims = serde_json::json!({
        "iss": f.issuer, "sub": "42", "aud": "harken", "iat": now, "exp": now + 600,
        "nonce": f.nonce, "name": "Ann", "email": "ann@example.org",
    });
    Json(serde_json::json!({ "access_token": "at", "token_type": "Bearer", "id_token": jwt(claims, &f.key) }))
}

fn listen(rt: &tokio::runtime::Runtime) -> (tokio::net::TcpListener, String) {
    let l = rt.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    (l, url)
}

/// The provider at one address, the server at another; the provider's
/// state, and the server's URL.
fn serve() -> (tokio::runtime::Runtime, Arc<Mutex<Fake>>, String) {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let (pl, issuer) = listen(&rt);
    let fake = Arc::new(Mutex::new(Fake {
        issuer: issuer.clone(),
        key: SECRET.into(),
        ..Fake::default()
    }));
    let meta = serde_json::json!({
        "issuer": issuer, "authorization_endpoint": format!("{issuer}/authorize"),
        "token_endpoint": format!("{issuer}/token"), "jwks_uri": format!("{issuer}/jwks"),
        "response_types_supported": ["code"], "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["HS256"],
    });
    let app = Router::new()
        .route("/.well-known/openid-configuration", get(move || async move { Json(meta) }))
        .route("/jwks", get(|| async { Json(serde_json::json!({ "keys": [] })) }))
        .route("/token", post(token))
        .with_state(fake.clone());
    rt.spawn(async move { axum::serve(pl, app).await.unwrap() });
    let provider = Provider::discover(&issuer, "harken", SECRET, &["openid", "profile", "email"]).expect("discovered");
    let (sl, server) = listen(&rt);
    let auth = Arc::new(Auth::new(Mode::Oidc(provider), &server, b"key"));
    let f = fake.clone();
    auth.on_sign_in(move |o| Ok(f.lock().unwrap().rows.push(o.clone())));
    rt.spawn(async move { axum::serve(sl, router(auth)).await.unwrap() });
    (rt, fake, server)
}

fn no_redirects() -> ureq::Agent {
    ureq::AgentBuilder::new().redirects(0).build()
}

/// `/auth/login` sends the person to the provider: where it is sent, as a
/// query.
fn start(server: &str) -> HashMap<String, String> {
    let resp = no_redirects()
        .get(&ark_auth::login_url(server, "http://127.0.0.1:9/", None))
        .call()
        .unwrap();
    let to = resp.header("Location").unwrap().to_string();
    let (_, q) = to.split_once('?').unwrap();
    q.split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.into(), ark_auth::percent_decode(v)))
        .collect()
}

/// The provider's callback, as the browser makes it: the server's answer.
fn callback(server: &str, state: &str) -> Result<String, (u16, String)> {
    match no_redirects().get(&format!("{server}/auth/callback?code=the-code&state={state}")).call() {
        Ok(r) => Ok(r.header("Location").unwrap_or_default().to_string()),
        Err(ureq::Error::Status(s, r)) => Err((s, r.into_string().unwrap_or_default())),
        Err(e) => Err((0, e.to_string())),
    }
}

/// A whole login: the provider is sent the PKCE challenge, state and nonce
/// and the scopes; the callback trades the code (with the verifier the
/// challenge was made from, and the secret) for an ID token whose nonce is
/// this login's, and the person is who its `sub` says, with its name and
/// email, and its session the row the callback wrote. Falsified by sending the token endpoint a verifier of its own
/// (`PkceCodeVerifier::new("x"…)` in `exchange`): the challenge did not
/// hash from what the provider was sent.
#[test]
fn a_login_through_the_provider_is_its_subject() {
    let (_rt, fake, server) = serve();
    let q = start(&server);
    assert_eq!((q["response_type"].as_str(), q["client_id"].as_str()), ("code", "harken"));
    assert_eq!(q["redirect_uri"], format!("{server}/auth/callback"));
    assert_eq!(q["code_challenge_method"], "S256");
    assert_eq!(q["scope"], "openid profile email");
    fake.lock().unwrap().nonce = q["nonce"].clone();
    let back = callback(&server, &q["state"]).expect("signed in");
    let code = back
        .strip_prefix("http://127.0.0.1:9/?code=")
        .expect("the code goes back to the redirect");
    let login = ark_auth::client::exchange(&server, code).unwrap();
    assert_eq!((login.user.id.as_str(), login.user.name.as_str()), ("42", "Ann"));
    assert_eq!(login.user.email, "ann@example.org");
    let row = fake.lock().unwrap().rows.pop().expect("the callback wrote the session");
    assert_eq!((row.id, row.user, row.email), (login.session, "42".into(), "ann@example.org".into()));
    let sent = fake.lock().unwrap().asked.pop().unwrap();
    assert_eq!((sent["grant_type"].as_str(), sent["code"].as_str()), ("authorization_code", "the-code"));
    assert_eq!(B64.encode(Sha256::digest(sent["code_verifier"].as_bytes())), q["code_challenge"]);
}

/// An ID token that does not answer this login is refused, and nobody is
/// signed in: another login's nonce, a signature not made with the secret,
/// a state never started here. Falsified by verifying with
/// `insecure_disable_signature_check()`: the forged token signed Ann in.
#[test]
fn an_id_token_that_does_not_answer_this_login_is_refused() {
    let (_rt, fake, server) = serve();
    let q = start(&server);
    fake.lock().unwrap().nonce = "another login's".into();
    let (status, why) = callback(&server, &q["state"]).unwrap_err();
    assert_eq!(status, 502, "{why}");
    assert!(why.contains("ID token"), "{why}");
    let q = start(&server);
    let mut f = fake.lock().unwrap();
    (f.nonce, f.key) = (q["nonce"].clone(), "not the client secret, but as long".into());
    drop(f);
    let (status, why) = callback(&server, &q["state"]).unwrap_err();
    assert_eq!(status, 502, "{why}");
    assert!(why.to_lowercase().contains("signature"), "{why}");
    let (status, _) = callback(&server, "never-started").unwrap_err();
    assert_eq!(status, 400);
}
