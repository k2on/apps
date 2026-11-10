//! An app's pages written by its own server (ui/docs/app.md, "The
//! Browser's Modes", `ssr`): a capability the layers keep, which no app
//! turns on today. [`site`] is its pages as a server's web, one call to
//! `ark_peer::Server::site` beside the sync socket and sign-in:
//!
//! - **Who.** A request's session is the ark-auth login in its
//!   `ark-session` cookie (set by `/web/session`, where a sign-in's code is
//!   traded for it), judged by `Auth::authenticate` — the same `Who`
//!   (account, session, roles) the sync socket resolves a token to.
//! - **Reads.** The app is opened over a replica in this process
//!   ([`Visit`]): a peer the hub feeds through its in-process dialer, as
//!   the bank importer is, never a socket — so the hub's feed and its row
//!   scopes are the socket's — caught up to the hub's head, and its
//!   functions asked as that `Who` (`ark_peer::Peer::ask`: a query's
//!   `Ctx`).
//! - **Writes.** A form is taken against the same draw made again
//!   (`arkui_web::Served::take`, which refuses a stale or malformed press):
//!   its fields that changed, then its press, so the app's own `update`
//!   runs, and what it authors is the domain's mutators on that replica —
//!   their role checks and `Refusal`s run on it and again on the hub. The
//!   page waits for the hub's verdict: confirmed, `303` to the app's new
//!   route; refused (here or there), `422` with the page drawn again as the
//!   form left it, its input kept, and the refusal's own words over it.
//! - **Forms.** Every form carries the `ark-csrf` cookie's token (same-site
//!   `Strict`, so another site's form cannot send it) as `csrf`, which must
//!   match, and a token of its own (`once`): a form sent again with a used
//!   one runs nothing and is sent where the first went. The body is held
//!   to [`MAX_BODY`], and each field to arkui-web's `page::MAX_TEXT`.
//! - **Files.** A path whose last segment has a dot is the build's file
//!   (the module, the favicon), served as `ark_peer::web::router` serves
//!   one; every other path is a page.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ark::peer::Who;
use ark_peer::{Handle, Memory, Peer};
use arkui_web::page::{escape, Mode, Route};
use arkui_web::{document, Served, WebApp, Written};
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use tower::ServiceExt;

/// The largest form a page takes, in bytes.
pub const MAX_BODY: usize = 64 * 1024;
/// How long a page waits for the hub: to catch up, and for a verdict.
pub const WAIT: Duration = Duration::from_secs(10);
/// How many forms' tokens are remembered as used.
pub const ONCE: usize = 4096;

/// What an app is opened over for one request: a replica the hub feeds as
/// `who`, caught up to its head.
pub struct Visit {
    pub peer: Peer,
    pub who: Who,
}

/// An app a server writes pages of.
pub trait Page: WebApp {
    /// The app over the replica of a visit, its UI at its defaults.
    fn open(visit: Visit) -> Self;
    /// The replica it reads and authors on.
    fn peer(&mut self) -> &mut Peer;
    /// Why the last change it authored was refused where it was authored,
    /// taken: the domain's words.
    fn refused(&mut self) -> Option<String> {
        None
    }
}

/// How the pages look and what they are written with, besides the app.
#[derive(Clone, Default)]
pub struct Look {
    pub accent: String,
    pub css: String,
    /// The `<head>`'s own: the title, the icon, the module's script.
    pub head: String,
    /// The page's arguments (`today=…`), as `arkui_web::page::args`.
    pub args: Vec<(String, String)>,
}

/// Each form's token once used, and where it went (none while it runs),
/// and the order they came in.
type Used = (HashMap<String, Option<String>>, VecDeque<String>);

struct Pages<A> {
    hub: Handle,
    auth: Arc<ark_auth::server::Auth>,
    config: Arc<dyn Fn() -> ark_peer::Config + Send + Sync>,
    look: Look,
    files: Option<Router>,
    /// Each form's token once used, and where it went.
    once: Mutex<Used>,
    _app: std::marker::PhantomData<fn() -> A>,
}

/// `A`'s pages as a server's web, in one call: give it to
/// `ark_peer::Server::site`, beside the socket and sign-in, under the
/// server's layers.
pub fn site<A: Page>(
    auth: Arc<ark_auth::server::Auth>,
    config: Arc<dyn Fn() -> ark_peer::Config + Send + Sync>,
    look: Look,
    dir: Option<(PathBuf, Option<String>)>,
) -> ark_peer::web::Site {
    ark_peer::web::Site {
        pages: Some(Box::new(move |hub: &Handle| pages::<A>(hub.clone(), auth, config, look, dir))),
        ..ark_peer::web::Site::default()
    }
}

/// The router of `A`'s pages on `hub`: every path a page, its files (the
/// build in `dir`, `module` its rebuild's file) beside them, and
/// `/web/session`. `config` is the replica's peer configuration (the
/// domain's).
pub fn pages<A: Page>(
    hub: Handle,
    auth: Arc<ark_auth::server::Auth>,
    config: Arc<dyn Fn() -> ark_peer::Config + Send + Sync>,
    look: Look,
    dir: Option<(PathBuf, Option<String>)>,
) -> Router {
    let pages = Arc::new(Pages::<A> {
        hub,
        auth,
        config,
        look,
        files: dir.map(|(d, m)| ark_peer::web::router(d, m)),
        once: Mutex::new((HashMap::new(), VecDeque::new())),
        _app: std::marker::PhantomData,
    });
    Router::new()
        .route("/web/session", axum::routing::get(session::<A>))
        .fallback(page::<A>)
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .with_state(pages)
}

fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
}

fn form(body: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(body);
    text.split('&')
        .filter(|kv| !kv.is_empty())
        .filter_map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            Some((arkui_web::page::decode(k)?, arkui_web::page::decode(v)?))
        })
        .collect()
}

fn html(status: StatusCode, body: String) -> Response {
    let mut r = (status, body).into_response();
    let h = r.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

fn see(location: &str) -> Response {
    let mut r = StatusCode::SEE_OTHER.into_response();
    if let Ok(v) = HeaderValue::from_str(location) {
        r.headers_mut().insert(header::LOCATION, v);
    }
    r
}

/// A sign-in's code traded for the session's cookie, then to `to`.
async fn session<A: Page>(State(p): State<Arc<Pages<A>>>, req: Request) -> Response {
    let q = Route::of_query(req.uri().query().unwrap_or_default());
    let to = q
        .get("to")
        .filter(|t| t.starts_with('/') && !t.starts_with("//"))
        .unwrap_or("/")
        .to_string();
    let Some(code) = q.get("code").map(str::to_string) else {
        return (StatusCode::BAD_REQUEST, "no code\n").into_response();
    };
    let auth = p.auth.clone();
    let Ok(Some(login)) = tokio::task::spawn_blocking(move || auth.redeem(&code)).await else {
        return (StatusCode::FORBIDDEN, "that code is not a sign-in's\n").into_response();
    };
    let mut r = see(&to);
    let c = format!("ark-session={}; Path=/; HttpOnly; SameSite=Lax", login.token);
    if let Ok(v) = HeaderValue::from_str(&c) {
        r.headers_mut().append(header::SET_COOKIE, v);
    }
    r
}

async fn page<A: Page>(State(p): State<Arc<Pages<A>>>, req: Request) -> Response {
    let file = req.uri().path().rsplit('/').next().is_some_and(|last| last.contains('.'));
    if let (true, Some(files)) = (file, p.files.clone()) {
        return files.oneshot(req).await.into_response();
    }
    let method = req.method().clone();
    if method != Method::GET && method != Method::POST {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let url = req.uri().path_and_query().map(|p| p.to_string()).unwrap_or_else(|| "/".into());
    let headers = req.headers().clone();
    let body = match axum::body::to_bytes(req.into_body(), MAX_BODY).await {
        Ok(b) => b,
        Err(_) => return (StatusCode::PAYLOAD_TOO_LARGE, "too large\n").into_response(),
    };
    let token = cookie(&headers, "ark-session");
    let authenticate = p.auth.authenticate();
    let who = match token.clone() {
        Some(t) => tokio::task::spawn_blocking(move || authenticate(&t)).await.ok().flatten(),
        None => None,
    };
    let (Some(who), Some(token)) = (who, token) else {
        let back = Route::at(["web", "session"]).with("to", url.clone()).url();
        let login = Route::at(["auth", "login"])
            .with("redirect", format!("{}{back}", p.auth.public_url()))
            .url();
        return html(
            StatusCode::UNAUTHORIZED,
            format!(
                "<!doctype html><meta charset=\"utf-8\"><title>Sign In</title><p><a href=\"{}\">Sign In</a></p>",
                escape(&login)
            ),
        );
    };
    let csrf = cookie(&headers, "ark-csrf");
    let fresh = csrf.is_none().then(ark_auth::server::random_token);
    let csrf = csrf.or_else(|| fresh.clone()).unwrap_or_default();
    let p2 = p.clone();
    let answered = tokio::task::spawn_blocking(move || {
        let route = Route::of_url(&url, "/").unwrap_or_default();
        match method {
            Method::POST => p2.post(&route, &body, &csrf, who, &token),
            _ => p2.get(&route, &csrf, who, &token, StatusCode::OK, None),
        }
    })
    .await;
    let mut r = match answered {
        Ok(r) => r,
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "the page failed\n").into_response(),
    };
    if let Some(t) = fresh {
        let c = format!("ark-csrf={t}; Path=/; HttpOnly; SameSite=Strict");
        if let Ok(v) = HeaderValue::from_str(&c) {
            r.headers_mut().append(header::SET_COOKIE, v);
        }
    }
    r
}

impl<A: Page> Pages<A> {
    /// A replica as `who`, its link the hub's in-process dialer, caught up.
    fn visit(&self, who: Who, token: &str) -> Result<Visit, String> {
        let mut peer = Peer::open((self.config)(), who.clone(), Box::new(Memory::default()), false);
        peer.sign_in(who.clone());
        let authenticate = self.auth.authenticate();
        let dial = self.hub.dialer(Arc::new(move |t: &str| authenticate(t)));
        peer.set_token(Some(token.to_string()));
        peer.connect_with("", dial);
        let head = self.hub.with(|h| h.log.log().head().0).ok_or("the hub is gone")?;
        let mut denied = None;
        let up = ark_peer::link::until(WAIT, Duration::from_millis(2), || {
            denied = denied.take().or(peer.pump().denied);
            denied.is_some() || (peer.linked && peer.log.log().head().0 >= head)
        });
        match (up, denied) {
            (_, Some(why)) => Err(why),
            (false, _) => Err("the hub did not answer".into()),
            _ => Ok(Visit { peer, who }),
        }
    }

    fn written(&self, route: &Route, csrf: &str, notice: Option<String>) -> Written {
        Written {
            mode: Mode::Server,
            route: route.clone(),
            base: "/".into(),
            args: self.look.args.clone(),
            width: None,
            accent: self.look.accent.clone(),
            css: self.look.css.clone(),
            head: self.look.head.clone(),
            action: route.url(),
            hidden: vec![("csrf".into(), csrf.into()), ("once".into(), ark_auth::server::random_token())],
            notice,
        }
    }

    fn get(&self, route: &Route, csrf: &str, who: Who, token: &str, status: StatusCode, notice: Option<String>) -> Response {
        let visit = match self.visit(who, token) {
            Ok(v) => v,
            Err(why) => return (StatusCode::SERVICE_UNAVAILABLE, format!("{why}\n")).into_response(),
        };
        let s = Served::new(A::open(visit), Mode::Server, route, &self.look.args, None);
        let view = s.draw();
        html(status, document(&self.written(route, csrf, notice), &view))
    }

    fn post(&self, route: &Route, body: &[u8], csrf: &str, who: Who, token: &str) -> Response {
        let fields = form(body);
        let field = |k: &str| fields.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        if csrf.is_empty() || field("csrf") != Some(csrf) {
            return (StatusCode::FORBIDDEN, "this form is not this site's\n").into_response();
        }
        let route = field("route").and_then(|r| Route::of_url(r, "/")).unwrap_or_else(|| route.clone());
        // A form sent again (the back button, a second click) runs nothing.
        let once = field("once").unwrap_or_default().to_string();
        {
            let mut used = self.once.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(went) = used.0.get(&once) {
                return see(went.as_deref().unwrap_or(&route.url()));
            }
            if !once.is_empty() {
                used.0.insert(once.clone(), None);
                used.1.push_back(once.clone());
                while used.1.len() > ONCE {
                    if let Some(old) = used.1.pop_front() {
                        used.0.remove(&old);
                    }
                }
            }
        }
        let visit = match self.visit(who, token) {
            Ok(v) => v,
            Err(why) => return (StatusCode::SERVICE_UNAVAILABLE, format!("{why}\n")).into_response(),
        };
        let s = Served::new(A::open(visit), Mode::Server, &route, &self.look.args, None);
        let press = field("ark").unwrap_or_default();
        let typed: Vec<(String, String)> = fields.iter().filter(|(k, _)| k.starts_with('t')).cloned().collect();
        if let Err(why) = s.take(press, &typed) {
            let why = match why {
                arkui_web::page::Refused::Stale => "This page changed since it was drawn. Try again.",
                arkui_web::page::Refused::Malformed => "That form could not be read.",
            };
            drop(s);
            return (StatusCode::CONFLICT, format!("{why}\n")).into_response();
        }
        let here = s.app_mut().refused();
        let refused = match here {
            Some(why) => Some(why),
            None => verdict(s.app_mut().peer()),
        };
        let to = s.app().route();
        match refused {
            Some(why) => {
                let view = s.draw();
                html(StatusCode::UNPROCESSABLE_ENTITY, document(&self.written(&to, csrf, Some(why)), &view))
            }
            None => {
                let went = to.url();
                let mut used = self.once.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(w) = used.0.get_mut(&once) {
                    *w = Some(went.clone());
                }
                see(&went)
            }
        }
    }
}

/// What the hub said of everything the replica authored: none when every
/// change was confirmed, else the first refusal's words.
fn verdict(peer: &mut Peer) -> Option<String> {
    let mut why = None;
    let done = ark_peer::link::until(WAIT, Duration::from_millis(2), || {
        let _ = peer.pump();
        for (_, w) in peer.take_rejections() {
            why.get_or_insert(w);
        }
        peer.log.pending.is_empty()
    });
    why.or_else(|| (!done).then(|| "The server did not confirm the change in time.".into()))
}
