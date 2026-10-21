//! The routes — `/auth/login`, `/auth/callback`, `/auth/exchange` — and
//! the hub's side of who is signed in (S1, `crate::rows`).
//!
//! A sign-in is a [`SignIn`] row the hub writes as its authority
//! ([`Auth::issue`]), and its token is the row's id with this server's HMAC
//! of it ([`Auth::mint`]): the server keeps nothing but its key
//! (`session.key` in the data directory it is handed, [`key_file`]). The
//! hub judges a token by the HMAC and the row it names ([`Auth::judge`],
//! [`Auth::authenticate`]) and closes the connections of a session revoked
//! or expired, or of a person whose roles changed ([`Auth::watcher`]).
//!
//! ```ignore
//! let auth = Arc::new(Auth::open::<App>(Mode::Dev, "http://127.0.0.1:8787", &data)?.allow_redirect("myapp://"));
//! // ark-peer does the rest: `.auth(auth)` mounts these routes, hands the
//! // hub over (`Auth::serve`) and plugs in the authenticator and watcher.
//! ```

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::{mpsc, Arc, Mutex, OnceLock};

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use tower_http::cors::{Any, CorsLayer};

use ark::db::read;
use ark::format::Encode;
use ark::hash::hex;
use ark::log::EntryId;
use ark::peer::{Ctx, Who};
use ark::row::Row;
use ark::store::{Change, Store};

use crate::oidc::{Challenge, Provider};
use crate::rows::{current, live, AuthTables, Role, SignIn};
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

/// What sign-in asks of the hub it serves (`ark-peer`'s `Handle`).
pub trait Hub: Send + Sync {
    /// `f` over the authority's store, on its thread, waited for.
    fn read(&self, f: Box<dyn FnOnce(&Store) + Send>);
    /// One change written as an entry the authority authors: a raw write,
    /// which no client's intent can name.
    fn write(&self, c: Change) -> Result<(), String>;
}

/// The hub's watch over who is signed in: given what an entry changed — or
/// `None`, the clock's tick — and the sessions connected, the sessions
/// whose connections to close.
pub type Watch = Box<dyn FnMut(&Store, Option<&[Change]>, &[Who], i64) -> Vec<String> + Send>;

/// Who signs in, as a token proves it.
pub type Authenticate = Arc<dyn Fn(&str) -> Option<Who> + Send + Sync>;

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

/// The schema's reads, its type erased: a session live at `ctx.now` with
/// its granted roles, a sign-in by id, and a person's sign-ins.
struct Reads {
    current: fn(&Store, &Ctx) -> Option<SignIn>,
    get: fn(&Store, &str) -> Option<SignIn>,
    of: fn(&Store, &str) -> Vec<SignIn>,
}

fn reads<S: AuthTables>() -> Reads {
    Reads {
        current: |store, ctx| read::<S, _>(store, |db| current::<S>(ctx, db, ())).ok().flatten(),
        get: |store, id| {
            read::<S, _>(store, |db| Ok::<_, ark::Refusal>(S::sign_in(db).get(id.to_string())))
                .ok()
                .flatten()
        },
        of: |store, user| {
            read::<S, _>(store, |db| {
                Ok::<_, ark::Refusal>(S::sign_in(db).by(SignIn::USER).from(user.to_string()).all())
            })
            .unwrap_or_default()
        },
    }
}

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
    hub: OnceLock<Box<dyn Hub>>,
    reads: Reads,
    /// `docs/plan-auth.md` The roles the configuration grants, by account
    /// id ([`Auth::with_roles`]), merged at every judging.
    roles: BTreeMap<String, BTreeSet<String>>,
}

impl Auth {
    /// Signing in by `mode` at `public_url` to an app of schema `S`,
    /// tokens signed with `key`.
    pub fn new<S: AuthTables>(mode: Mode, public_url: &str, key: &[u8]) -> Self {
        Auth {
            mode,
            public_url: public_url.trim_end_matches('/').to_string(),
            redirects: Vec::new(),
            key: key.to_vec(),
            ttl_ms: DEFAULT_TTL_MS,
            pending: Mutex::new(HashMap::new()),
            codes: Mutex::new(HashMap::new()),
            hub: OnceLock::new(),
            reads: reads::<S>(),
            roles: BTreeMap::new(),
        }
    }

    /// [`Auth::new`] with the key kept in `data` ([`key_file`]).
    pub fn open<S: AuthTables>(mode: Mode, public_url: &str, data: &Path) -> std::io::Result<Self> {
        Ok(Auth::new::<S>(mode, public_url, &key_file(&data.join("session.key"))?))
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

    /// `roles` joined by those the configuration grants `account`, sorted,
    /// once each.
    fn merged(&self, account: &str, roles: &[String]) -> Vec<String> {
        let mut all: BTreeSet<String> = roles.iter().cloned().collect();
        all.extend(self.roles.get(account).into_iter().flatten().cloned());
        all.into_iter().collect()
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

    /// The hub every sign-in's row is written to and every token judged
    /// over. Once; a sign-in before it is refused.
    pub fn serve(&self, hub: Box<dyn Hub>) {
        let _ = self.hub.set(hub);
    }

    fn ask<T: Send + 'static>(&self, f: impl FnOnce(&Store) -> T + Send + 'static) -> Option<T> {
        let (tx, rx) = mpsc::channel();
        self.hub.get()?.read(Box::new(move |s| drop(tx.send(f(s)))));
        rx.recv().ok()
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

    /// Who session `id` proves over `store` at `now`: its row live, its
    /// person holding the roles it was opened with, those the role table
    /// grants and those the configuration does.
    pub fn judge(&self, store: &Store, id: &str, now: i64) -> Option<Who> {
        let s = (self.reads.current)(store, &at(id, now))?;
        Some(Who {
            roles: self.merged(&s.user, &s.roles),
            actor: s.user,
            session: id.into(),
        })
    }

    /// A token judged: signed by this server, and [`Auth::judge`]d over the
    /// hub's store. Asked on the caller's thread, of the hub's.
    pub fn authenticate(self: &Arc<Self>) -> Authenticate {
        let auth = self.clone();
        Arc::new(move |token| {
            let id = auth.verify(token)?.to_string();
            let a = auth.clone();
            auth.ask(move |store| a.judge(store, &id, now_ms())).flatten()
        })
    }

    /// The hub's [`Watch`]: at a tick, every connected session whose row is
    /// no longer live; at an entry, those of its changed sign-ins no longer
    /// live or gone, and every session of a person whose roles changed, to
    /// be judged again. A connection no row names is not a sign-in's (one
    /// the server's process connected itself) and is left be.
    pub fn watcher(self: &Arc<Self>) -> Watch {
        let auth = self.clone();
        Box::new(move |store, changes, open, now| {
            let (mut ids, mut users) = (BTreeSet::new(), BTreeSet::new());
            for c in changes.unwrap_or_default() {
                let row = match c {
                    Change::Add { row, .. } | Change::Remove { row, .. } | Change::Edit { new: row, .. } => row,
                };
                match c.table() {
                    t if t == SignIn::TABLE => ids.extend(SignIn::from_bytes(row).ok().map(|s| s.id)),
                    t if t == Role::TABLE => users.extend(Role::from_bytes(row).ok().map(|r| r.user)),
                    _ => {}
                }
            }
            let ended = open.iter().filter(|w| {
                let row = || (auth.reads.get)(store, &w.session);
                users.contains(&w.actor)
                    || match changes {
                        None => row().is_some_and(|s| !live(&s, now)),
                        Some(_) => ids.contains(&w.session) && row().is_none_or(|s| !live(&s, now)),
                    }
            });
            let ended: BTreeSet<String> = ended.map(|w| w.session.clone()).collect();
            ended.into_iter().collect()
        })
    }

    /// The login of row `s` for `account`.
    fn login(&self, account: &Account, s: SignIn) -> Login {
        Login {
            token: self.mint(&s.id),
            user: Account {
                roles: self.merged(&account.id, &account.roles),
                ..account.clone()
            },
            session: s.id,
            expires_ms: s.expires_ms,
        }
    }

    /// Sign `account` in on `device`, with no code and no browser: the
    /// session row written, and the login.
    pub fn issue(&self, account: &Account, device: &str) -> Result<Login, String> {
        let hub = self.hub.get().ok_or("no hub writes sessions here")?;
        let issued_ms = now_ms();
        let row = SignIn {
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
        let c = Change::Add {
            table: SignIn::TABLE.into(),
            row: row.to_bytes(),
        };
        hub.write(c).map_err(|e| format!("could not record the session: {e}"))?;
        Ok(self.login(account, row))
    }

    /// What a peer the server runs itself signs in with (a scanner
    /// authoring what it finds): its live session on `device` from an
    /// earlier start with a day or more left, minted again — a restart
    /// writes no entry — or a new one.
    pub fn own(&self, account: &Account, device: &str) -> Result<Login, String> {
        let (user, of, now) = (account.id.clone(), self.reads.of, now_ms());
        let day = 24 * 60 * 60 * 1000;
        let earlier = self.ask(move |store| of(store, &user)).unwrap_or_default();
        match earlier.into_iter().rfind(|s| s.device == device && live(s, now + day)) {
            Some(s) => Ok(self.login(account, s)),
            None => self.issue(account, device),
        }
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

/// A context as session `id` at `now`: what judging reads under.
fn at(id: &str, now: i64) -> Ctx {
    Ctx {
        draws: Default::default(),
        id: EntryId::of(id, 0),
        actor: String::new(),
        session: id.into(),
        roles: vec![],
        now,
    }
}

/// The key kept in `path`: read, or 32 bytes from the OS written there
/// (readable by its owner alone) when there is none.
pub fn key_file(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::{Error, ErrorKind, Write};
    match std::fs::read_to_string(path) {
        Ok(text) => unhex(text.trim())
            .filter(|k| !k.is_empty())
            .ok_or_else(|| Error::other(format!("{}: not a hex key", path.display()))),
        Err(e) if e.kind() == ErrorKind::NotFound => {
            let key = random_token();
            std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
            let mut f = std::fs::OpenOptions::new();
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut f, 0o600);
            f.write(true).create_new(true).open(path)?.write_all(key.as_bytes())?;
            Ok(unhex(&key).expect("hex"))
        }
        Err(e) => Err(e),
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
    (s.len() & 1 == 0).then(|| pairs.collect()).flatten()
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
