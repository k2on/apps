//! The routes: `/auth/login`, `/auth/callback`, `/auth/exchange`. Mount
//! them beside the socket.
//!
//! A sign-in is a session row the hub writes as the authority
//! ([`Auth::on_sign_in`]), and its token is the row's id with this
//! server's HMAC of it ([`Auth::mint`]): nothing here keeps a session. The
//! hub judges a token by the HMAC ([`Auth::verify`]) and the row it names;
//! who is signed in, and signing out, are the domain's query and mutation.
//!
//! ```ignore
//! let auth = Arc::new(Auth::new(Mode::Dev, "http://127.0.0.1:8787", &key).allow_redirect("myapp://"));
//! // ark-peer does the rest: `.auth(auth)` mounts these routes, writes the
//! // sessions and judges every connection's token.
//! ```

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex, OnceLock};

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};

use ark::hash::hex;

use crate::oidc::{Challenge, Provider};
use crate::util::with_code;
use crate::{Account, Login};

/// Thirty days. A phone that has not spoken to the server in a month signs
/// in again, and loses nothing by it.
pub const DEFAULT_TTL_MS: i64 = 30 * 24 * 60 * 60 * 1000;

/// How people sign in.
#[derive(Debug)]
pub enum Mode {
    /// Through an OpenID Connect provider.
    Oidc(Provider),
    /// As whatever name they type. For a laptop and nowhere else; a server
    /// running this way should say so every time it starts.
    Dev,
}

impl Mode {
    /// What a server says about this at startup, every time.
    pub fn announce(&self) -> String {
        match self {
            Mode::Oidc(p) => format!("signing in through {}", p.issuer()),
            Mode::Dev => "*** DEV AUTH: anyone is whoever they say. A name is a login, `name:role,role` a login \
                          holding those roles, and nothing is checked. Fine on a laptop; never anywhere else. ***"
                .into(),
        }
    }
}

/// A sign-in as the session row the hub writes for it: the domain's
/// `sign_in` table's columns, by name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Opened {
    pub id: String,
    pub user: String,
    pub name: String,
    pub email: String,
    pub device: String,
    pub issued_ms: i64,
    pub expires_ms: i64,
    pub revoked: bool,
    pub roles: Vec<String>,
}

/// What [`Auth::on_sign_in`] is handed: the row to write, written before
/// the token is handed out.
pub type Open = Box<dyn Fn(&Opened) -> Result<(), String> + Send + Sync>;

/// A login started and not finished: what the provider's answer has to
/// match, where the code goes, and what is signing in.
#[derive(Debug)]
struct Pending {
    challenge: Challenge,
    redirect: String,
    device: String,
    started_ms: i64,
}

/// A login finished and not collected: a one-minute, single-use code
/// standing in for the token in the URL.
#[derive(Debug)]
struct Issued {
    login: Login,
    issued_ms: i64,
}

/// A login has a minute to go from the provider to the exchange, and a
/// person ten minutes to sign in at the provider.
const CODE_TTL_MS: i64 = 60 * 1000;
const PENDING_TTL_MS: i64 = 10 * 60 * 1000;

/// Everything the routes share. `Arc` it; the same one is what the hub is
/// opened with, so the socket and the routes agree on who is who.
pub struct Auth {
    mode: Mode,
    /// Where this server is reachable from a browser, with no trailing
    /// slash: the provider sends people back to `{public_url}/auth/callback`.
    public_url: String,
    /// Prefixes a redirect may have, besides the loopback ones every server
    /// allows and its own `public_url`.
    redirects: Vec<String>,
    /// What tokens are signed with.
    key: Vec<u8>,
    ttl_ms: i64,
    pending: Mutex<HashMap<String, Pending>>,
    codes: Mutex<HashMap<String, Issued>>,
    open: OnceLock<Open>,
    /// `docs/plan-auth.md` The roles the configuration grants, by account
    /// id ([`Auth::with_roles`]), merged at every judging.
    roles: BTreeMap<String, BTreeSet<String>>,
}

impl Auth {
    /// Signing in by `mode` at `public_url`, tokens signed with `key`.
    pub fn new(mode: Mode, public_url: &str, key: &[u8]) -> Self {
        Auth {
            mode,
            public_url: public_url.trim_end_matches('/').to_string(),
            redirects: Vec::new(),
            key: key.to_vec(),
            ttl_ms: DEFAULT_TTL_MS,
            pending: Mutex::new(HashMap::new()),
            codes: Mutex::new(HashMap::new()),
            open: OnceLock::new(),
            roles: BTreeMap::new(),
        }
    }

    /// How long a session lasts from issue.
    pub fn with_ttl(mut self, ttl_ms: i64) -> Self {
        self.ttl_ms = ttl_ms;
        self
    }

    /// `docs/plan-auth.md` Grant roles from the configuration: role → the
    /// account ids holding it, as `services.harken.roles` says them.
    pub fn with_roles<R: Into<String>, A: Into<String>>(mut self, roles: impl IntoIterator<Item = (R, Vec<A>)>) -> Self {
        for (role, accounts) in roles {
            let role = role.into();
            for a in accounts {
                self.roles.entry(a.into()).or_default().insert(role.clone());
            }
        }
        self
    }

    /// The roles the configuration grants an account.
    pub fn configured_roles(&self, account: &str) -> BTreeSet<String> {
        self.roles.get(account).cloned().unwrap_or_default()
    }

    /// Let a login code go back to anything starting with `prefix` — an
    /// app's URL scheme, `harken://`, or a web client served elsewhere.
    pub fn allow_redirect(mut self, prefix: &str) -> Self {
        self.redirects.push(prefix.to_string());
        self
    }

    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    /// Whether a code may be sent to `url`. Loopback is always fine — it is
    /// how a desktop program listens, and nothing off the machine can be
    /// there — and so is this server's own origin, where the browser client
    /// it serves lives.
    pub fn redirect_allowed(&self, url: &str) -> bool {
        // The origin, not a prefix: `https://h.example.evil/` starts with
        // `https://h.example` and is somebody else's.
        if let Some(rest) = url.strip_prefix(&self.public_url) {
            if rest.is_empty() || rest.starts_with(['/', '?', '#']) {
                return true;
            }
        }
        if let Some(rest) = url.strip_prefix("http://") {
            let host = rest.split(['/', '?', '#']).next().unwrap_or("");
            let host = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
            if matches!(host, "127.0.0.1" | "localhost" | "[::1]") {
                return true;
            }
        }
        self.redirects.iter().any(|p| url.starts_with(p))
    }

    /// Where every sign-in's session row is written: the hub, as its
    /// authority. Once; a sign-in before it is refused.
    pub fn on_sign_in(&self, f: impl Fn(&Opened) -> Result<(), String> + Send + Sync + 'static) {
        let _ = self.open.set(Box::new(f));
    }

    fn mac(&self, id: &str) -> Hmac<sha2::Sha256> {
        let mut m = Hmac::<sha2::Sha256>::new_from_slice(&self.key).expect("an HMAC takes any key");
        m.update(id.as_bytes());
        m
    }

    /// The token for session `id`: `id.hmac(key, id)`, hex.
    pub fn mint(&self, id: &str) -> String {
        format!("{id}.{}", hex(&self.mac(id).finalize().into_bytes()))
    }

    /// The session a token names, when this server signed it.
    pub fn verify<'t>(&self, token: &'t str) -> Option<&'t str> {
        let (id, sig) = token.rsplit_once('.')?;
        let sig = unhex(sig)?;
        self.mac(id).verify_slice(&sig).ok().map(|_| id)
    }

    /// Sign `account` in on `device`, with no code and no browser: the
    /// session row written, and the login. What a peer the server runs
    /// itself signs in with (a scanner authoring what it finds).
    pub fn issue(&self, account: &Account, device: &str) -> Result<Login, String> {
        let open = self.open.get().ok_or("no hub writes sessions here")?;
        let issued_ms = now_ms();
        let row = Opened {
            id: random_id(),
            user: account.id.clone(),
            name: account.name.clone(),
            email: account.email.clone(),
            device: device.into(),
            issued_ms,
            expires_ms: issued_ms + self.ttl_ms,
            revoked: false,
            roles: account.roles.clone(),
        };
        open(&row).map_err(|e| format!("could not record the session: {e}"))?;
        let mut roles: BTreeSet<String> = account.roles.iter().cloned().collect();
        roles.extend(self.configured_roles(&account.id));
        Ok(Login {
            token: self.mint(&row.id),
            session: row.id,
            user: Account {
                roles: roles.into_iter().collect(),
                ..account.clone()
            },
            expires_ms: row.expires_ms,
        })
    }

    /// Sign `account` in and mint the code the redirect carries.
    pub fn finish(&self, account: &Account, device: &str) -> Result<String, String> {
        let login = self.issue(account, device)?;
        let code = random_token();
        let now = now_ms();
        let mut codes = self.codes.lock().unwrap_or_else(|e| e.into_inner());
        codes.retain(|_, c| now - c.issued_ms < CODE_TTL_MS);
        codes.insert(code.clone(), Issued { login, issued_ms: now });
        Ok(code)
    }

    /// The login a code stands for, once.
    pub fn redeem(&self, code: &str) -> Option<Login> {
        let mut codes = self.codes.lock().unwrap_or_else(|e| e.into_inner());
        let issued = codes.remove(code)?;
        (now_ms() - issued.issued_ms < CODE_TTL_MS).then_some(issued.login)
    }
}

/// Dev auth's reading of a name: `name:role,role` is `name` holding those
/// roles, trimmed, empty ones dropped, sorted and once each.
pub fn dev_account(name: &str) -> Account {
    let (id, roles) = name.split_once(':').unwrap_or((name, ""));
    let roles: BTreeSet<String> = roles.split(',').map(str::trim).filter(|r| !r.is_empty()).map(String::from).collect();
    Account {
        id: id.trim().into(),
        name: id.trim().into(),
        email: String::new(),
        roles: roles.into_iter().collect(),
    }
}

/// The routes, with state applied, ready to `merge` into an app's router.
///
/// Permissive CORS on all of them: the token is a bearer, never a cookie, so
/// another origin reading these answers learns only what it already sent.
pub fn router(auth: Arc<Auth>) -> Router {
    Router::new()
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/exchange", post(exchange))
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any))
        .with_state(auth)
}

#[derive(Debug, Deserialize)]
struct LoginQuery {
    redirect: String,
    user: Option<String>,
}

/// What is signing in, as its browser or program says.
fn device(headers: &HeaderMap) -> String {
    let agent = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok());
    agent.unwrap_or_default().chars().take(200).collect()
}

/// `account` signed in on `device` off the runtime (the hub is asked), and
/// sent back to `redirect` with the code.
async fn finished(auth: Arc<Auth>, account: Account, device: String, redirect: String) -> Response {
    match tokio::task::spawn_blocking(move || auth.finish(&account, &device)).await {
        Ok(Ok(code)) => Redirect::to(&with_code(&redirect, &code)).into_response(),
        Ok(Err(e)) => failed(e),
        Err(e) => failed(e.to_string()),
    }
}

/// Start a login, or in dev mode finish one.
async fn login(State(auth): State<Arc<Auth>>, headers: HeaderMap, Query(q): Query<LoginQuery>) -> Response {
    if !auth.redirect_allowed(&q.redirect) {
        return bad(format!(
            "a login code cannot be sent to {}; the server allows loopback, its own \
             origin, and what it was configured with",
            q.redirect
        ));
    }
    match &auth.mode {
        Mode::Dev => {
            let Some(user) = q.user.filter(|u| !u.trim().is_empty()) else {
                return Html(dev_form(&q.redirect)).into_response();
            };
            // `name:role,role` is a login holding those roles
            // ([`dev_account`]), which the announcement says.
            let account = dev_account(user.trim());
            finished(auth.clone(), account, device(&headers), q.redirect).await
        }
        Mode::Oidc(provider) => {
            let challenge = Challenge::new();
            let url = match provider.authorize_url(&challenge, &callback_url(&auth)) {
                Ok(url) => url,
                Err(e) => return failed(e),
            };
            let now = now_ms();
            let mut pending = auth.pending.lock().unwrap_or_else(|e| e.into_inner());
            pending.retain(|_, p| now - p.started_ms < PENDING_TTL_MS);
            pending.insert(
                challenge.state.clone(),
                Pending {
                    challenge,
                    redirect: q.redirect,
                    device: device(&headers),
                    started_ms: now,
                },
            );
            Redirect::to(&url).into_response()
        }
    }
}

fn callback_url(auth: &Auth) -> String {
    format!("{}/auth/callback", auth.public_url)
}

#[derive(Debug, Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// The provider sending the person back.
async fn callback(State(auth): State<Arc<Auth>>, Query(q): Query<CallbackQuery>) -> Response {
    let Mode::Oidc(provider) = &auth.mode else {
        return bad("this server does not use a provider".into());
    };
    if let Some(error) = q.error {
        return bad(format!("the provider refused: {error} {}", q.error_description.unwrap_or_default()));
    }
    let (Some(code), Some(state)) = (q.code, q.state) else {
        return bad("the provider sent no code".into());
    };
    let Some(pending) = auth.pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&state) else {
        return bad("this login was not started here, or took too long".into());
    };
    if now_ms() - pending.started_ms >= PENDING_TTL_MS {
        return bad("this login took too long; start again".into());
    }

    // Two round trips to the provider, off the runtime.
    let (provider, callback, challenge) = (provider.clone(), callback_url(&auth), pending.challenge);
    let exchanged = tokio::task::spawn_blocking(move || provider.exchange(&code, &challenge, &callback)).await;
    match exchanged {
        Ok(Ok(account)) => finished(auth.clone(), account, pending.device, pending.redirect).await,
        Ok(Err(e)) => failed(e),
        Err(e) => failed(e.to_string()),
    }
}

#[derive(Debug, Deserialize)]
struct Exchange {
    code: String,
}

/// The code, for the login it stands for. Once.
async fn exchange(State(auth): State<Arc<Auth>>, Json(body): Json<Exchange>) -> Response {
    match auth.redeem(&body.code) {
        Some(login) => Json(login).into_response(),
        None => (StatusCode::BAD_REQUEST, "that code has been used, or has expired").into_response(),
    }
}

fn bad(why: String) -> Response {
    (StatusCode::BAD_REQUEST, why).into_response()
}

fn failed(why: String) -> Response {
    (StatusCode::BAD_GATEWAY, format!("sign-in failed: {why}")).into_response()
}

/// Dev mode's provider: a text box.
fn dev_form(redirect: &str) -> String {
    let redirect = redirect.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;");
    format!(
        "<!doctype html><meta charset=utf-8><meta name=viewport content=\"width=device-width\">\
         <title>sign in</title>\
         <style>body{{font-family:system-ui;max-width:24em;margin:4em auto;padding:0 1em}}\
         input,button{{font:inherit;padding:.5em;width:100%;box-sizing:border-box;margin:.25em 0}}\
         p{{color:#666}}</style>\
         <h1>sign in</h1>\
         <p>This server is running without an identity provider, so it takes your word \
         for who you are. That is fine on a laptop and nowhere else.</p>\
         <form method=get action=/auth/login>\
         <input type=hidden name=redirect value=\"{redirect}\">\
         <input name=user placeholder=\"a name\" autofocus autocapitalize=off>\
         <button>sign in</button></form>"
    )
}

/// 128 bits from the OS, as hex: a session id, which entries carry.
pub fn random_id() -> String {
    random_hex::<16>()
}

/// 256 bits from the OS, as hex.
pub fn random_token() -> String {
    random_hex::<32>()
}

fn random_hex<const N: usize>() -> String {
    let mut b = [0u8; N];
    getrandom::getrandom(&mut b).expect("the OS gives no randomness");
    hex(&b)
}

/// Hex's bytes, when it is hex.
pub fn unhex(s: &str) -> Option<Vec<u8>> {
    let pairs = (0..s.len())
        .step_by(2)
        .map(|i| s.get(i..i + 2).and_then(|p| u8::from_str_radix(p, 16).ok()));
    (s.len() % 2 == 0).then(|| pairs.collect()).flatten()
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
