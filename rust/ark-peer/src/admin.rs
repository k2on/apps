//! The admin page (`docs/plan-guards.md` D4): the explorer, compiled to
//! wasm as a browser peer is, served on a listener of its own — bound to
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
//! Each body is a value of §1.1's codec in hex (`ark_explorer::wire`).
//!
//! **Who may open it.** Bound to loopback, anyone who can reach the port is
//! on the machine already, and it asks nothing. Bound wider, every request
//! but `hello` carries a login's token as a bearer, and the login must hold
//! [`ROLE`]. The page signs in through the server's own `/auth/login`.
//!
//! **No second door** (§1.7). A mutation is an entry the authority authors
//! through `Input::Mutate`, as the server half of a mutator is; so is a raw
//! write: [`raw`] makes the change a function of no input — the store
//! judging it as any write, the row it replaces the one there — runnable
//! for its own entry only, under a name no client's intent can carry.
//! Replicas apply its facts like any entry's. Both are answered once the
//! entry is written, as an acknowledgement is. A domain's functions are
//! the ones its build registered (§1.16): nothing is loaded at run time.

use std::path::PathBuf;
use std::sync::Arc;

use ark::codec::Node;
use ark::peer::{Ctx as Run, Procedure};
use ark::schema::Space;
use ark::store::Change;
use ark_explorer::wire::{self, Hello, State};
pub use ark_explorer::Catalog;
use ark_explorer::{crud_of, Connection, CrudVerbs, Line};
use axum::extract::State as With;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;

use crate::{Authenticate, Handle, Peer};

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
        r = r.nest_service("/admin", crate::web::router(dir, None));
    }
    r
}

/// A raw write as a function the authority authors: no input, its one
/// change judged by the store as any write is.
pub struct Raw(pub Change);

impl Procedure for Raw {
    fn apply(&self, store: &mut ark::store::Store, _: &Run, _: &[u8]) -> Result<Vec<Change>, ark::Refusal> {
        store.apply(&self.0)?;
        Ok(vec![self.0.clone()])
    }
}

/// What a raw write's entry names: the change's hash, under a prefix no
/// domain's function has.
pub const RAW: &str = "raw write ";

/// A raw write's function, and the name its entry calls it by: the
/// change's, so it is called by no name a client knows.
pub fn raw(c: &Change) -> (String, Arc<dyn Procedure>) {
    let h = ark::hash::hex(&ark::hash::sha256(&[&c.to_bytes()]));
    (format!("{RAW}{h}"), Arc::new(Raw(c.clone())))
}

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
    match token.and_then(|v| v.strip_prefix("Bearer ")).and_then(|t| check(t)) {
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
        let name = |f: &str| match f.starts_with(RAW) {
            true => "raw write".to_string(),
            false => f.to_string(),
        };
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
        Ok(c) => {
            let (h, f) = raw(&c);
            answer(&ctx.hub, h, vec![], Some(f)).await
        }
        Err(why) => text(StatusCode::BAD_REQUEST, why),
    }
}

async fn author(With(ctx): With<Ctx>, body: String) -> Response {
    match wire::author_from(&body) {
        Ok((name, input)) if ctx.admin.catalog.get(&name).is_some() => answer(&ctx.hub, name, input, None).await,
        Ok((name, _)) => text(StatusCode::BAD_REQUEST, format!("no function {name}")),
        Err(why) => text(StatusCode::BAD_REQUEST, why),
    }
}

// Authored as the authority, its deliveries delivered: the sequence once
// written, or the refusal. A raw write's function is runnable for its entry only.
async fn answer(hub: &Handle, func: String, input: Vec<u8>, once: Option<Arc<dyn Procedure>>) -> Response {
    let hub = hub.clone();
    match tokio::task::spawn_blocking(move || hub.author(Space::Durable, func, input, once))
        .await
        .ok()
        .flatten()
    {
        Some(Ok(n)) => n.to_string().into_response(),
        Some(Err(why)) => text(StatusCode::CONFLICT, why),
        None => text(StatusCode::SERVICE_UNAVAILABLE, "the hub is gone"),
    }
}
