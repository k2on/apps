//! The admin page (`docs/plan-guards.md` D4): the explorer, compiled to
//! wasm as a browser peer is, served on a listener of its own — bound to
//! loopback by default — over the authority's own store, its log and its
//! connections, writing as the authority.
//!
//! ```text
//! <bind> ── /admin/              the page (`ark-admin`, a static build)
//!        ── /admin/api/hello     whether a login is needed, and where to get one
//!        ── /admin/api/state     the module, the store, the log, the connections,
//!        │                       the Verify answers, the CRUD exposed (ark_explorer::wire)
//!        ── /admin/api/raw       a raw write, as the authority (`Authority::edit`)
//!        ── /admin/api/author    a domain mutation, as the authority
//! ```
//!
//! **Who may open it.** Bound to loopback, anyone who can reach the port is
//! on the machine already, and it asks nothing. Bound wider, every request
//! but `hello` carries a login's token as a bearer, and the login must hold
//! [`ROLE`] — the roles a server's sign-in grants (`ark_auth`, an app's
//! configuration), asked at every request. The page signs in through the
//! server's own `/auth/login`, so its address must be one the server may
//! send a code back to.
//!
//! **The transport is HTTP, and the page is a client of the server
//! process, not of the log.** It is shown the authority's state as one
//! snapshot per request rather than paged entries, and what it writes the
//! server authors as itself — under its own identity
//! ([`crate::Hub::authority_identity`]): a raw write through
//! `ark::raw`, a mutation through the domain's CRUD like any intent. Both
//! go through the hub's thread, are judged there, and are answered after
//! the write that makes them durable, as an acknowledgement is.

use std::path::PathBuf;
use std::sync::Arc;

use ark::protocol::Identity;
use ark_client::{Autos, Domain};
use ark_explorer::{crud_of, function_names, Connection, CrudVerbs, Line, Verified};
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;

use crate::hub::HubHandle;

/// The role a login must hold to open a wider-bound admin page.
pub const ROLE: &str = "admin";

/// How many entries of the log the page is shown, newest last.
pub const LINES: usize = 200;

/// Where the admin page is served, and from what.
#[derive(Clone, Debug)]
pub struct Admin {
    /// `host:port`. Loopback (the default an app gives it) asks nothing;
    /// anything else asks for a login holding [`ROLE`].
    pub bind: String,
    /// The page: a static build of `ark-admin` (`index.html` and `pkg/`).
    /// Without one only the API is served.
    pub page: Option<PathBuf>,
    /// Where the page signs in, when it must: the server's public address.
    pub server: Option<String>,
}

/// Whether `bind` is loopback: an address of the loopback interface, or
/// `localhost`. Anything that is not plainly one is wider.
pub fn is_loopback(bind: &str) -> bool {
    match bind.parse::<std::net::SocketAddr>() {
        Ok(a) => a.ip().is_loopback(),
        Err(_) => bind.rsplit_once(':').is_some_and(|(h, _)| h == "localhost"),
    }
}

/// Who may write: nobody asked, or a login holding [`ROLE`].
pub(crate) type Check = Arc<dyn Fn(Option<&str>) -> Option<Identity> + Send + Sync>;

#[derive(Clone)]
struct Ctx {
    hub: HubHandle,
    domain: Domain,
    /// `None` on loopback: nothing is asked.
    check: Option<Check>,
    server: Option<String>,
    names: Arc<std::collections::BTreeMap<ark::hash::FnHash, String>>,
}

/// The admin listener's routes. `check` is the server's own authenticator,
/// asked of every request but `hello` when the bind is wider than loopback.
pub(crate) fn router(admin: &Admin, hub: HubHandle, domain: Domain, check: Check) -> Router {
    let ctx = Ctx {
        hub,
        names: Arc::new(function_names(domain.module())),
        domain,
        check: (!is_loopback(&admin.bind)).then_some(check),
        server: admin.server.clone(),
    };
    let mut r = Router::new()
        .route("/admin/api/hello", get(hello))
        .route("/admin/api/state", get(state))
        .route("/admin/api/raw", post(raw))
        .route("/admin/api/author", post(author))
        .route("/", get(|| async { Redirect::to("/admin/") }))
        .with_state(ctx);
    if let Some(page) = &admin.page {
        r = r.nest_service("/admin", crate::web::router_with_module(page.clone(), None));
    }
    r
}

fn json(body: String) -> Response {
    ([(header::CONTENT_TYPE, "application/json")], body).into_response()
}

// The login a request carries, held to the role, where one is asked for:
// the answer that turns it away, or nothing.
fn refused(ctx: &Ctx, headers: &HeaderMap) -> Option<Response> {
    let Some(check) = &ctx.check else { return None };
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    match check(token) {
        None => Some((StatusCode::UNAUTHORIZED, "sign in: this admin page is not on loopback").into_response()),
        Some(who) if who.roles.contains(ROLE) => None,
        Some(who) => Some((StatusCode::FORBIDDEN, format!("{} does not hold the {ROLE} role", who.user)).into_response()),
    }
}

async fn hello(State(ctx): State<Ctx>) -> Response {
    let server = ctx.server.as_deref().map_or_else(|| "null".to_string(), ark::json::quoted);
    json(format!("{{\"open\":{},\"server\":{server}}}", ctx.check.is_none()))
}

async fn state(State(ctx): State<Ctx>, headers: HeaderMap) -> Response {
    if let Some(r) = refused(&ctx, &headers) {
        return r;
    }
    let module = ark::canon::encode(&ark::ir::module_value(ctx.domain.module()));
    let exposed: Vec<(String, CrudVerbs)> = crud_of(ctx.domain.module())
        .into_iter()
        .map(|(t, v)| (t, CrudVerbs { may_author: true, ..v }))
        .collect();
    let names = ctx.names.clone();
    let read = ctx
        .hub
        .read(move |h| {
            let a = h.authority();
            let head = a.log.head_seq();
            let name = |fh: &ark::hash::FnHash| {
                names
                    .get(fh)
                    .cloned()
                    .or_else(|| a.bodies.get(fh).map(|c| c.function.name.clone()))
                    .unwrap_or_else(|| ark::value::hex(fh))
            };
            let lines = a
                .log
                .entries
                .iter()
                .rev()
                .take(LINES)
                .rev()
                .map(|(n, (e, f))| Line {
                    seq: Some(*n),
                    function: name(&e.fn_hash),
                    entry: e.clone(),
                    facts: Some(f.clone()),
                    standing: "in the log".into(),
                })
                .collect();
            let connections = h
                .connections()
                .into_iter()
                .map(|(who, sent, partial)| Connection {
                    who: format!("{} \u{b7} {}", who.user, who.session),
                    cursor: sent,
                    pending: None,
                    note: format!("{} behind{}", head - sent, if partial { " \u{b7} holds a union" } else { "" }),
                })
                .collect();
            let verifies = h
                .verified()
                .map(|(who, seq, answer)| Verified {
                    who: who.clone(),
                    seq: *seq,
                    answer: *answer,
                })
                .collect();
            ark_explorer::wire::State {
                module,
                store: ark::store::Store::store_value(&a.store),
                head,
                lines,
                connections,
                verifies,
                exposed,
                raw: true,
                who: h.authority_identity().user.clone(),
            }
        })
        .await;
    match read {
        Ok(s) => json(s.to_json()),
        Err(e) => (StatusCode::SERVICE_UNAVAILABLE, format!("{e:#}")).into_response(),
    }
}

async fn raw(State(ctx): State<Ctx>, headers: HeaderMap, body: String) -> Response {
    if let Some(r) = refused(&ctx, &headers) {
        return r;
    }
    let change = match ark_explorer::wire::raw_from(&body) {
        Ok(c) => c,
        Err(why) => return (StatusCode::BAD_REQUEST, why).into_response(),
    };
    answer(ctx.hub.write(move |h| h.edit(&change)).await)
}

async fn author(State(ctx): State<Ctx>, headers: HeaderMap, body: String) -> Response {
    if let Some(r) = refused(&ctx, &headers) {
        return r;
    }
    let (function, args) = match ark_explorer::wire::author_from(&body) {
        Ok(x) => x,
        Err(why) => return (StatusCode::BAD_REQUEST, why).into_response(),
    };
    let (fh, f) = match ctx.domain.mutator(&function) {
        Ok((h, f)) => (h.clone(), f.clone()),
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    let autos = Autos::system().draw(&f);
    answer(ctx.hub.write(move |h| h.author(fh, args, autos)).await)
}

fn answer(r: anyhow::Result<Result<ark::log::Seq, String>>) -> Response {
    match r {
        Ok(Ok(n)) => format!("{n}").into_response(),
        Ok(Err(why)) => (StatusCode::CONFLICT, why).into_response(),
        Err(e) => (StatusCode::SERVICE_UNAVAILABLE, format!("{e:#}")).into_response(),
    }
}
