//! The routes: `/auth/login`, `/auth/callback`, `/auth/exchange`,
//! `/auth/me`, `/auth/logout`. Mount them beside the socket.
//!
//! ```ignore
//! let sessions = SessionStore::open(data.join("sessions.json"))?;
//! let auth = Arc::new(Auth::new(sessions, Mode::Dev, "http://127.0.0.1:8787").allow_redirect("myapp://"));
//! // ark-server does the rest: `.auth(auth)` mounts these routes and opens
//! // the sync server with `auth.authenticator()`.
//! ```

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use ark::protocol::{Authenticate, Identity};
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use tower_http::cors::{Any, CorsLayer};

use crate::oidc::{Challenge, Provider};
use crate::session::{now_ms, random_token, SessionStore};
use crate::util::with_code;
use crate::{Account, Login};

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

/// A login that has been started and not finished: the state the provider
/// will hand back, and what has to match when it does.
#[derive(Debug)]
struct Pending {
    challenge: Challenge,
    redirect: String,
    started_ms: i64,
}

/// A login that has finished and not been collected: a one-minute,
/// single-use code standing in for the token in the URL.
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
    sessions: Mutex<SessionStore>,
    mode: Mode,
    /// Where this server is reachable from a browser, with no trailing
    /// slash: the provider sends people back to `{public_url}/auth/callback`.
    public_url: String,
    /// Prefixes a redirect may have, besides the loopback ones every server
    /// allows and its own `public_url`.
    redirects: Vec<String>,
    pending: Mutex<HashMap<String, Pending>>,
    codes: Mutex<HashMap<String, Issued>>,
    /// Told `(user, session)` of every session revoked ([`Auth::on_revoke`]).
    revoked: Mutex<Vec<Revoked>>,
    /// `docs/plan-auth.md` The roles the configuration grants, by account
    /// id ([`Auth::with_roles`]). Asked at every `Hello` rather than written
    /// into a session, so a restart with new configuration grants or
    /// revokes a role for every login at once.
    roles: BTreeMap<String, BTreeSet<String>>,
}

/// What [`Auth::on_revoke`] is handed: told the user and the session id of
/// every session revoked, after the store has written it down.
pub type Revoked = Box<dyn Fn(&str, &str) + Send + Sync>;

impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Auth")
            .field("mode", &self.mode)
            .field("public_url", &self.public_url)
            .field("redirects", &self.redirects)
            .finish_non_exhaustive()
    }
}

impl Auth {
    pub fn new(sessions: SessionStore, mode: Mode, public_url: &str) -> Self {
        Auth {
            sessions: Mutex::new(sessions),
            mode,
            public_url: public_url.trim_end_matches('/').to_string(),
            redirects: Vec::new(),
            pending: Mutex::new(HashMap::new()),
            codes: Mutex::new(HashMap::new()),
            revoked: Mutex::new(Vec::new()),
            roles: BTreeMap::new(),
        }
    }

    /// `docs/plan-auth.md` Grant roles from the configuration: role name →
    /// the account ids holding it, as `services.harken.roles` says them.
    /// Every login of such an account holds the role, beside any it was
    /// issued with (the scanner's own; a dev login's from its name).
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

    /// What a server says about how it signs people in, every time it
    /// starts: the mode, and every role the configuration grants, by role.
    pub fn announce(&self) -> String {
        let mut by_role: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for (account, roles) in &self.roles {
            for r in roles {
                by_role.entry(r).or_default().push(account);
            }
        }
        let mut out = self.mode.announce();
        for (r, accounts) in by_role {
            out.push_str(&format!("\nrole {r}: {}", accounts.join(", ")));
        }
        out
    }

    // A login as a client and the engine are told of it: the roles it was
    // issued with and the configuration's, sorted, once each.
    fn completed(&self, mut login: Login) -> Login {
        let mut roles: BTreeSet<String> = login.user.roles.drain(..).collect();
        roles.extend(self.configured_roles(&login.user.id));
        login.user.roles = roles.into_iter().collect();
        login
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

    fn sessions(&self) -> std::sync::MutexGuard<'_, SessionStore> {
        self.sessions.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A session for `account` issued directly, with no code and no
    /// browser: for a peer the server runs itself (a scanner authoring what
    /// it finds), which signs in like everything else because the engine
    /// holds every entry to a session.
    pub fn issue(&self, account: &Account) -> Result<Login, String> {
        self.sessions().issue(account)
    }

    /// Sign `account` in and mint the code the redirect carries.
    fn finish(&self, account: &Account) -> Result<String, String> {
        let login = self.sessions().issue(account).map_err(|e| format!("could not record the session: {e}"))?;
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
        (now_ms() - issued.issued_ms < CODE_TTL_MS).then(|| self.completed(issued.login))
    }

    /// The login a bearer token proves, if it is live, with every role it
    /// holds now.
    pub fn whoami(&self, token: &str) -> Option<Login> {
        let login = self.sessions().lookup(token)?;
        Some(self.completed(login))
    }

    /// Whether `session` is or was `user`'s — live, expired or revoked. What
    /// the sync server asks of an entry authored under an earlier login of
    /// the same person (see `owns_fn`).
    pub fn owns(&self, user: &str, session: &str) -> bool {
        self.sessions().owned_by(user, session)
    }

    /// Be told of every session revoked from here on (`docs/plan-perf.md`
    /// R6). The token is asked at a connection's `Hello` and not again, so
    /// without this a revoked session's open socket went on syncing until
    /// it happened to drop; the sync server registers here and closes that
    /// session's connections at once. Called on the thread that revoked,
    /// with no lock of this one held.
    pub fn on_revoke(&self, f: impl Fn(&str, &str) + Send + Sync + 'static) {
        self.revoked.lock().unwrap_or_else(|e| e.into_inner()).push(Box::new(f));
    }

    /// End the session `token` proves, and tell everything registered with
    /// [`Auth::on_revoke`]. Whether there was a live one.
    pub fn revoke(&self, token: &str) -> Result<bool, String> {
        let gone = self.sessions().revoke_session(token)?;
        let Some((user, session)) = gone else {
            return Ok(false);
        };
        for f in self.revoked.lock().unwrap_or_else(|e| e.into_inner()).iter() {
            f(&user, &session);
        }
        Ok(true)
    }
}

impl Auth {
    /// What the sync server asks at every `Hello`: the login a token
    /// proves, as the engine's identity — the account's id, the session,
    /// and the roles it holds now (issued with it, and configured).
    pub fn authenticator(self: &Arc<Self>) -> Authenticate {
        let auth = self.clone();
        Box::new(move |token: Option<&str>| {
            let login = auth.whoami(token?)?;
            Some(Identity::new(login.user.id, login.session).with_roles(login.user.roles))
        })
    }

    /// [`Auth::owns`], as the engine's ownership check
    /// (`ark::protocol::Server::with_owns`): an entry authored offline under
    /// an earlier login of the same person, pushed after signing in again.
    pub fn owns_fn(self: &Arc<Self>) -> ark::protocol::Owns {
        let auth = self.clone();
        Box::new(move |user: &str, session: &str| auth.owns(user, session))
    }
}

/// The routes, with state applied, ready to `merge` into an app's router.
///
/// Permissive CORS on all of them: the token is a bearer, never a cookie, so
/// another origin reading these answers learns only what it already sent.
/// A browser client served from another port on a laptop is the case.
pub fn router(auth: Arc<Auth>) -> Router {
    Router::new()
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/exchange", post(exchange))
        .route("/auth/me", get(me))
        .route("/auth/logout", post(logout))
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any))
        .with_state(auth)
}

#[derive(Debug, Deserialize)]
struct LoginQuery {
    redirect: String,
    #[serde(default)]
    user: Option<String>,
}

/// Start a login, or in dev mode finish one.
async fn login(State(auth): State<Arc<Auth>>, Query(q): Query<LoginQuery>) -> Response {
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
            // `name:role,role` is a login holding those roles, as the
            // engine's own dev auth reads a token (`ark::protocol::
            // dev_identity`), which the announcement says.
            let who = ark::protocol::dev_identity(user.trim(), "");
            let account = Account {
                id: who.user.clone(),
                name: who.user,
                email: String::new(),
                roles: who.roles.into_iter().collect(),
            };
            match auth.finish(&account) {
                Ok(code) => Redirect::to(&with_code(&q.redirect, &code)).into_response(),
                Err(e) => failed(e),
            }
        }
        Mode::Oidc(provider) => {
            let challenge = Challenge::new();
            let url = provider.authorize_url(&challenge, &callback_url(&auth));
            let now = now_ms();
            let mut pending = auth.pending.lock().unwrap_or_else(|e| e.into_inner());
            pending.retain(|_, p| now - p.started_ms < PENDING_TTL_MS);
            pending.insert(
                challenge.state.clone(),
                Pending {
                    challenge,
                    redirect: q.redirect,
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
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
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
    let provider = provider.clone();
    let callback = callback_url(&auth);
    let exchanged = tokio::task::spawn_blocking(move || provider.exchange(&code, &pending.challenge, &callback)).await;
    let account = match exchanged {
        Ok(Ok(account)) => account,
        Ok(Err(e)) => return failed(e),
        Err(e) => return failed(e.to_string()),
    };
    match auth.finish(&account) {
        Ok(code) => Redirect::to(&with_code(&pending.redirect, &code)).into_response(),
        Err(e) => failed(e),
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

/// Who a bearer token is.
async fn me(State(auth): State<Arc<Auth>>, headers: HeaderMap) -> Response {
    match bearer(&headers).and_then(|t| auth.whoami(t)) {
        Some(login) => Json(login).into_response(),
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

/// End the session a bearer token proves.
async fn logout(State(auth): State<Arc<Auth>>, headers: HeaderMap) -> Response {
    let revoked = bearer(&headers).map(|t| auth.revoke(t).unwrap_or(false)).unwrap_or(false);
    if revoked {
        StatusCode::NO_CONTENT.into_response()
    } else {
        StatusCode::UNAUTHORIZED.into_response()
    }
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers.get(header::AUTHORIZATION)?.to_str().ok()?.strip_prefix("Bearer ").map(str::trim)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn auth() -> Auth {
        Auth::new(SessionStore::memory(), Mode::Dev, "https://app.example/").allow_redirect("myapp://")
    }

    #[test]
    fn a_code_goes_only_where_it_is_allowed() {
        let a = auth();
        assert!(a.redirect_allowed("http://127.0.0.1:53211/"));
        assert!(a.redirect_allowed("http://localhost:8080/?x=1"));
        assert!(a.redirect_allowed("https://app.example/"));
        assert!(a.redirect_allowed("https://app.example/app/"));
        assert!(a.redirect_allowed("myapp://auth"));
        assert!(!a.redirect_allowed("https://app.example.evil/"));
        assert!(!a.redirect_allowed("http://127.0.0.1.evil/"));
        assert!(!a.redirect_allowed("http://10.0.0.5:8080/"));
        assert!(!a.redirect_allowed("https://elsewhere/"));
    }

    #[test]
    fn a_code_is_redeemed_once_and_the_engine_is_told_who_it_is() {
        let a = Arc::new(auth());
        let code = a
            .finish(&Account {
                id: "alice".into(),
                name: "alice".into(),
                ..Account::default()
            })
            .unwrap();
        let login = a.redeem(&code).expect("first time");
        assert_eq!(login.user.id, "alice");
        assert!(a.redeem(&code).is_none(), "second time");
        let who = a.authenticator();
        assert_eq!(who(Some(&login.token)), Some(Identity::new("alice", login.session.clone())));
        assert_eq!(who(Some("forged")), None);
        assert_eq!(who(None), None);
        let owns = a.owns_fn();
        assert!(owns("alice", &login.session));
        assert!(!owns("bob", &login.session));
    }

    /// `docs/plan-auth.md` A login holds the roles it was issued with and
    /// the ones the configuration grants its account, in what the exchange
    /// hands the client and in what the engine is told at `Hello`; a
    /// server started again with other configuration answers the same
    /// token with the new roles, and the dev form's `name:role,role` is a
    /// login holding them. Falsified by leaving the configured roles out
    /// of `completed`: the exchange's login held only `scanner`.
    #[test]
    fn a_login_holds_its_own_roles_and_the_configured_ones() {
        let sessions = || SessionStore::memory();
        let a = Arc::new(Auth::new(sessions(), Mode::Dev, "https://app.example/").with_roles([("library", vec!["alice"])]));
        let code = a
            .finish(&Account {
                id: "alice".into(),
                roles: vec!["scanner".into()],
                ..Account::default()
            })
            .unwrap();
        let login = a.redeem(&code).expect("redeemed");
        assert_eq!(login.user.roles, ["library", "scanner"]);
        let who = a.authenticator()(Some(&login.token)).expect("signed in");
        assert_eq!(who, Identity::new("alice", login.session.clone()).with_roles(["library", "scanner"]));
        assert!(a.announce().contains("role library: alice"), "{}", a.announce());
        // The same session store, the configuration moved: the role goes.
        let store = std::mem::replace(&mut *a.sessions(), sessions());
        let b = Arc::new(Auth::new(store, Mode::Dev, "https://app.example/"));
        let who = b.authenticator()(Some(&login.token)).expect("signed in");
        assert_eq!(who.roles, ["scanner".to_string()].into_iter().collect());
        // Dev auth's name, with roles.
        let dev = ark::protocol::dev_identity("bob:library, admin,", "s");
        assert_eq!(dev, Identity::new("bob", "s").with_roles(["admin", "library"]));
    }

    /// R6: revoking a session tells every listener whose and which, once,
    /// and a token already revoked or never issued tells nobody. Falsified
    /// by leaving out the loop over the listeners in `Auth::revoke` (the
    /// store revoking alone, as `logout` did): the listener hears nothing.
    #[test]
    fn a_revocation_is_told_to_whoever_asked() {
        let a = auth();
        let heard: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
        let h = heard.clone();
        a.on_revoke(move |user, session| h.lock().unwrap().push((user.into(), session.into())));
        let account = Account {
            id: "alice".into(),
            name: "Alice".into(),
            ..Account::default()
        };
        let login = a.issue(&account).unwrap();
        assert!(a.revoke(&login.token).unwrap());
        assert!(!a.revoke(&login.token).unwrap(), "already gone");
        assert!(!a.revoke("no such token").unwrap());
        assert_eq!(*heard.lock().unwrap(), [("alice".to_string(), login.session.clone())]);
    }
}
