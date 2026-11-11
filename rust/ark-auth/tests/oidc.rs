//! The relying party against a provider: discovery, the login's redirect to
//! the provider, the callback's code exchange and the ID token's checks,
//! through the server's own routes, over real HTTP. The provider is a few
//! axum routes here that sign their ID tokens HS256 with the client secret,
//! or ES256 with a key its key set publishes (Kanidm's only algorithm).

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ark_auth::oidc::{Groups, Provider};
use ark_auth::server::{router, Auth, Mode};
use axum::extract::{Form, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use common::{App, Memory};
use hmac::{Hmac, Mac};
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use sha2::{Digest, Sha256};

const SECRET: &str = "a client secret of thirty-two bytes or more";

/// What the provider is to put in its next ID token, and what it was sent.
#[derive(Default)]
struct Fake {
    issuer: String,
    nonce: String,
    key: String,
    asked: Vec<HashMap<String, String>>,
    /// Signed ES256 with [`es256`]'s key, not HS256 with `key`.
    es256: bool,
    /// ES256, with a key the key set does not hold.
    stranger: bool,
    /// The audience, when not this server's (`harken`).
    aud: Option<serde_json::Value>,
    /// A groups claim, when there is one.
    groups: Option<serde_json::Value>,
    /// The session rows the server wrote.
    rows: Memory,
}

/// The provider's ES256 key: fixed, so its key set is too.
fn es256() -> SigningKey {
    SigningKey::from_bytes(&[7u8; 32].into()).unwrap()
}

fn es256_jwt(claims: serde_json::Value, stranger: bool) -> String {
    let head = B64.encode(br#"{"alg":"ES256","typ":"JWT","kid":"k1"}"#);
    let body = B64.encode(claims.to_string());
    let key = match stranger {
        true => SigningKey::from_bytes(&[8u8; 32].into()).unwrap(),
        false => es256(),
    };
    let sig: Signature = key.sign(format!("{head}.{body}").as_bytes());
    format!("{head}.{body}.{}", B64.encode(sig.to_bytes()))
}

fn key_set() -> serde_json::Value {
    let point = es256().verifying_key().to_encoded_point(false);
    serde_json::json!({ "keys": [{
        "kty": "EC", "crv": "P-256", "alg": "ES256", "use": "sig", "kid": "k1",
        "x": B64.encode(point.x().unwrap()), "y": B64.encode(point.y().unwrap()),
    }] })
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
    let mut claims = serde_json::json!({
        "iss": f.issuer, "sub": "42", "aud": f.aud.clone().unwrap_or("harken".into()), "iat": now, "exp": now + 600,
        "nonce": f.nonce, "name": "Ann", "email": "ann@example.org",
        "picture": "https://pictures.example.invalid/ann.png",
    });
    if let Some(g) = &f.groups {
        claims["groups"] = g.clone();
    }
    let id_token = match f.es256 {
        true => es256_jwt(claims, f.stranger),
        false => jwt(claims, &f.key),
    };
    Json(serde_json::json!({ "access_token": "at", "token_type": "Bearer", "id_token": id_token }))
}

fn listen(rt: &tokio::runtime::Runtime) -> (tokio::net::TcpListener, String) {
    let l = rt.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    (l, url)
}

/// The provider at one address, the server at another; the provider's
/// state, and the server's URL.
fn serve() -> (tokio::runtime::Runtime, Arc<Mutex<Fake>>, String) {
    serve_with(|p| p)
}

/// [`serve`], with the provider as `configure` makes it.
fn serve_with(configure: impl FnOnce(Provider) -> Provider) -> (tokio::runtime::Runtime, Arc<Mutex<Fake>>, String) {
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
        "id_token_signing_alg_values_supported": ["HS256", "ES256"],
    });
    let app = Router::new()
        .route("/.well-known/openid-configuration", get(move || async move { Json(meta) }))
        .route("/jwks", get(|| async { Json(key_set()) }))
        .route("/token", post(token))
        .with_state(fake.clone());
    rt.spawn(async move { axum::serve(pl, app).await.unwrap() });
    let provider = configure(Provider::discover(&issuer, "harken", SECRET, &["openid", "profile", "email"]).expect("discovered"));
    let (sl, server) = listen(&rt);
    let auth = Arc::new(Auth::new::<App>(Mode::Oidc(provider), &server, b"key"));
    auth.serve(Box::new(fake.lock().unwrap().rows.clone()));
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
/// email and picture, and its session the row the callback wrote. Falsified by sending the token endpoint a verifier of its own
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
    assert_eq!(login.user.picture, "https://pictures.example.invalid/ann.png");
    let row = fake.lock().unwrap().rows.sign_ins().pop().expect("the callback wrote the session");
    assert_eq!((row.id, row.user, row.email), (login.session, "42".into(), "ann@example.org".into()));
    assert_eq!(row.picture, login.user.picture);
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

/// `/auth/picture` hands on the picture a person's sign-in holds, and
/// nobody else's: a person with none is a 404, and one whose provider's
/// host cannot be reached is the provider's failure, not the server's own.
/// (Its fetch is refused anything but a public address, so a test cannot
/// serve it one: `public` is unit-tested.)
#[test]
fn a_picture_is_the_sign_ins_and_nobody_elses() {
    let (_rt, fake, server) = serve();
    let get = |user: &str| match ureq::get(&format!("{server}/auth/picture?user={user}&v=1")).call() {
        Ok(r) => r.status(),
        Err(ureq::Error::Status(s, _)) => s,
        Err(e) => panic!("{e}"),
    };
    assert_eq!(get("42"), 404, "nobody has signed in yet");
    let q = start(&server);
    fake.lock().unwrap().nonce = q["nonce"].clone();
    callback(&server, &q["state"]).expect("signed in");
    assert_eq!(get("42"), 502, "the provider's host is not there");
    assert_eq!(get("43"), 404);
}

/// One login through the provider as `fake` is set up: the callback's
/// answer.
fn log_in(fake: &Arc<Mutex<Fake>>, server: &str) -> Result<String, (u16, String)> {
    let q = start(server);
    fake.lock().unwrap().nonce = q["nonce"].clone();
    callback(server, &q["state"])
}

/// An ES256 ID token — Kanidm's only algorithm — is verified against the
/// key the provider's key set publishes, and one signed by another key is
/// not. The algorithms allowed are the ones discovery names: falsified by
/// leaving ES256 out of the provider's metadata, which refused Ann's token.
#[test]
fn an_es256_id_token_is_checked_against_the_key_set() {
    let (_rt, fake, server) = serve();
    fake.lock().unwrap().es256 = true;
    let back = log_in(&fake, &server).expect("signed in with ES256");
    assert!(back.starts_with("http://127.0.0.1:9/?code="), "{back}");
    fake.lock().unwrap().stranger = true;
    let (status, why) = log_in(&fake, &server).unwrap_err();
    assert_eq!(status, 502, "{why}");
    assert!(why.to_lowercase().contains("signature"), "{why}");
}

/// An ID token issued to another of the provider's clients is refused,
/// unless that client is trusted (`_OIDC_TRUSTED_CLIENTS`): what a device's
/// shared sign-in will bring later. This server's own is trusted whatever
/// the list. Falsified by `trusting` ignoring its list: ark's token was
/// refused.
#[test]
fn another_clients_id_token_only_when_that_client_is_trusted() {
    let (_rt, fake, server) = serve();
    (fake.lock().unwrap().es256, fake.lock().unwrap().aud) = (true, Some("ark".into()));
    let (status, why) = log_in(&fake, &server).unwrap_err();
    assert_eq!(status, 502, "{why}");
    assert!(why.contains("audience") || why.contains("ID token"), "{why}");

    let (_rt2, fake, server) = serve_with(|p| p.trusting(["ark"]));
    (fake.lock().unwrap().es256, fake.lock().unwrap().aud) = (true, Some("ark".into()));
    log_in(&fake, &server).expect("ark is trusted");
    fake.lock().unwrap().aud = Some("stranger".into());
    assert!(log_in(&fake, &server).is_err(), "only those named");
    fake.lock().unwrap().aud = Some(serde_json::json!(["harken", "ark"]));
    log_in(&fake, &server).expect("its own, beside one it trusts");
}

/// With groups required, the groups claim decides: a group's name, or the
/// name a provider qualifies with `@domain`; anyone else, or a token with
/// no claim, is not let in. Falsified by `verified` never consulting the
/// rule: the money household's member signed in to harken.
#[test]
fn the_groups_claim_lets_in_only_those_required() {
    let (_rt, fake, server) = serve_with(|p| p.requiring(vec!["harken_users".into()]));
    fake.lock().unwrap().groups = Some(serde_json::json!([
        "harken_users@idm.example.invalid",
        "00000000-0000-0000-0000-000000000001"
    ]));
    log_in(&fake, &server).expect("in the group");
    fake.lock().unwrap().groups = Some(serde_json::json!(["money_users@idm.example.invalid"]));
    let (status, why) = log_in(&fake, &server).unwrap_err();
    assert_eq!(status, 502, "{why}");
    assert!(why.contains("group"), "{why}");
    fake.lock().unwrap().groups = None;
    assert!(log_in(&fake, &server).is_err(), "no claim, no way in");
}

/// The rule by itself, as any way in will use it.
#[test]
fn a_group_matches_by_name_or_qualified_name() {
    let g = Groups(vec!["money_users".into()]);
    assert!(g.admits(&serde_json::json!({ "groups": ["money_users"] })));
    assert!(g.admits(&serde_json::json!({ "groups": ["money_users@idm.example.invalid"] })));
    assert!(!g.admits(&serde_json::json!({ "groups": ["money_users_old@idm.example.invalid"] })));
    assert!(!g.admits(&serde_json::json!({ "groups": "money_users" })));
    assert!(!g.admits(&serde_json::json!({})));
}
