//! The admin page's routes (`docs/plan-guards.md` D4; the `server`
//! feature): the explorer, compiled to wasm as a browser peer is, served on
//! a listener of its own — bound to
//! loopback by default — over the log's authority: its store, its log and
//! the connections, writing as the authority.
//!
//! ```text
//! <bind> ── /admin/              the page (`ark-admin`, a static build)
//!        ── /admin/api/hello     whether a login is needed, and where to get one
//!        ── /admin/api/state     the domain's catalog, the store's rows, the log's
//!        │                       last lines, the connections, the CRUD exposed
//!        ── /admin/api/raw       a raw write, as the authority
//!        ── /admin/api/author    a function of the domain, by name, as the authority
//! ```
//!
//! Each body is a value of §1.1's codec in hex ([`crate::wire`]). A server
//! is handed them as [`Admin::routes`] (`ark_peer::Server::admin`).
//!
//! **Who may open it.** Bound to loopback, anyone who can reach the port is
//! on the machine already, and it asks nothing. Bound wider, every request
//! but `hello` carries a login's token as a bearer, and the login must hold
//! [`ROLE`]. The page signs in through the server's own `/auth/login`.
//!
//! **No second door** (§1.7). A mutation is an entry the authority authors
//! through `Input::Mutate`, as the server half of a mutator is; a raw
//! write is an entry it writes through `Input::Write` — the store judging
//! the change as any write, the row it replaces the one there — naming no
//! function, so no client's intent can carry it (an authority holds what
//! names no function it has). Replicas apply its facts like any entry's. Both are answered once the
//! entry is written, as an acknowledgement is. A domain's functions are
//! the ones its build registered (§1.16): nothing is loaded at run time.

use std::path::PathBuf;
use std::sync::Arc;

use ark::domain::Catalog;
use ark::schema::Space;
use ark_peer::serve::AdminRoutes;
use ark_peer::{Authenticate, Handle, Peer};
use axum::extract::State as With;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;

use crate::wire::{self, Hello, State};
use crate::{crud_of, Connection, CrudVerbs, Line};

/// The role a login must hold to open a wider-bound admin page.
pub const ROLE: &str = "admin";

/// How many entries of the log the page is shown, newest last.
pub const LINES: usize = 200;

/// Where the admin page is served, from what, and over which module.
#[derive(Clone)]
pub struct Admin {
    /// `host:port`. Loopback asks nothing; anything else a login holding [`ROLE`].
    pub bind: String,
    /// A static build of `ark-admin`; without one only the API is served.
    pub page: Option<PathBuf>,
    /// Where the page signs in, when it must: the server's public address.
    pub server: Option<String>,
    /// The domain the log's functions are, described.
    pub catalog: Catalog,
}

impl Admin {
    /// Its bind and its routes, as a server takes them.
    pub fn routes(self) -> (String, AdminRoutes) {
        let bind = self.bind.clone();
        (bind, Box::new(move |hub, authenticate| router(self, hub, authenticate)))
    }
}

/// Whether `bind` is plainly loopback: a loopback address, or `localhost`.
pub fn is_loopback(bind: &str) -> bool {
    match bind.parse::<std::net::SocketAddr>() {
        Ok(a) => a.ip().is_loopback(),
        Err(_) => bind.rsplit_once(':').is_some_and(|(h, _)| h == "localhost"),
    }
}

#[derive(Clone)]
struct Ctx {
    hub: Handle,
    admin: Arc<Admin>,
    /// `None` on loopback: nothing is asked.
    check: Option<Authenticate>,
}

/// The admin listener's routes; `authenticate` is asked of every request
/// but `hello` when the bind is wider than loopback.
pub fn router(admin: Admin, hub: Handle, authenticate: Authenticate) -> Router {
    let page = admin.page.clone();
    let check = (!is_loopback(&admin.bind)).then_some(authenticate);
    let ctx = Ctx {
        hub,
        admin: Arc::new(admin),
        check,
    };
    let mut r = Router::new()
        .route("/admin/api/state", get(state))
        .route("/admin/api/raw", post(raw_write))
        .route("/admin/api/author", post(author))
        .route_layer(axum::middleware::from_fn_with_state(ctx.clone(), refused))
        .route("/admin/api/hello", get(hello))
        .route("/", get(|| async { Redirect::to("/admin/") }))
        .with_state(ctx);
    if let Some(dir) = page {
        r = r.nest_service("/admin", ark_peer::web::router(dir, None));
    }
    r
}

/// What the page calls an entry naming no function: a raw write.
pub const RAW: &str = "raw write";

fn text(code: StatusCode, why: impl Into<String>) -> Response {
    (code, why.into()).into_response()
}

/// `f` on the hub's thread, off the runtime's; nothing once the hub is gone.
async fn on_hub<T: Send + 'static>(hub: &Handle, f: impl FnOnce(&mut Peer) -> T + Send + 'static) -> Option<T> {
    let hub = hub.clone();
    tokio::task::spawn_blocking(move || hub.with(f)).await.ok().flatten()
}

// The login a request carries, held to the role where one is asked for.
async fn refused(With(ctx): With<Ctx>, req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let Some(check) = &ctx.check else { return next.run(req).await };
    let token = req.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok());
    let (token, check) = (token.and_then(|v| v.strip_prefix("Bearer ")).map(String::from), check.clone());
    match tokio::task::spawn_blocking(move || check(&token?)).await.ok().flatten() {
        None => text(StatusCode::UNAUTHORIZED, "sign in: this admin page is not on loopback"),
        Some(who) if who.roles.iter().any(|r| r == ROLE) => next.run(req).await,
        Some(who) => text(StatusCode::FORBIDDEN, format!("{} does not hold the {ROLE} role", who.actor)),
    }
}

async fn hello(With(ctx): With<Ctx>) -> String {
    let server = ctx.admin.server.clone();
    wire::body(&Hello {
        open: ctx.check.is_none(),
        server,
    })
}

async fn state(With(ctx): With<Ctx>) -> Response {
    let admin = ctx.admin.clone();
    let exposed: Vec<(String, CrudVerbs)> = crud_of(&admin.catalog)
        .into_iter()
        .map(|(t, v)| (t, CrudVerbs { may_author: true, ..v }))
        .collect();
    let read = on_hub(&ctx.hub, move |h| {
        let head = h.log.log().head().0;
        let name = |f: &str| if f.is_empty() { RAW } else { f }.to_string();
        let at = h.log.log().entries().len().saturating_sub(LINES);
        let lines = h.log.log().entries()[at..]
            .iter()
            .map(|l| Line {
                seq: Some(l.seq),
                function: name(&l.entry.func),
                entry: l.entry.clone(),
                facts: Some(l.facts.clone()),
                standing: "in the log".into(),
            })
            .collect();
        // The hub keeps no cursor per connection: each is shown at the head.
        let connections = h
            .connections()
            .map(|(c, who)| Connection {
                who: format!("{} \u{b7} {}", who.actor, who.session),
                cursor: head,
                pending: None,
                note: format!("connection {c}"),
            })
            .collect();
        State {
            catalog: admin.catalog.clone(),
            rows: h.log.store.snapshot_rows(),
            head,
            lines,
            connections,
            exposed,
            raw: true,
            who: h.log.me.actor.clone(),
        }
    });
    match read.await {
        Some(s) => s.to_body().into_response(),
        None => text(StatusCode::SERVICE_UNAVAILABLE, "the hub is gone"),
    }
}

async fn raw_write(With(ctx): With<Ctx>, body: String) -> Response {
    match wire::raw_from(&body) {
        Ok(c) => answer(&ctx.hub, move |h| h.write(vec![c])).await,
        Err(why) => text(StatusCode::BAD_REQUEST, why),
    }
}

async fn author(With(ctx): With<Ctx>, body: String) -> Response {
    match wire::author_from(&body) {
        Ok((name, input)) if ctx.admin.catalog.get(&name).is_some() => answer(&ctx.hub, move |h| h.author(Space::Durable, name, input)).await,
        Ok((name, _)) => text(StatusCode::BAD_REQUEST, format!("no function {name}")),
        Err(why) => text(StatusCode::BAD_REQUEST, why),
    }
}

// Authored as the authority, its deliveries delivered: the sequence once
// written, or the refusal.
async fn answer(hub: &Handle, f: impl FnOnce(&Handle) -> Option<Result<i64, String>> + Send + 'static) -> Response {
    let hub = hub.clone();
    match tokio::task::spawn_blocking(move || f(&hub)).await.ok().flatten() {
        Some(Ok(n)) => n.to_string().into_response(),
        Some(Err(why)) => text(StatusCode::CONFLICT, why),
        None => text(StatusCode::SERVICE_UNAVAILABLE, "the hub is gone"),
    }
}
