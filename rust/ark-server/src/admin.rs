//! The admin page (`docs/plan-guards.md` D4): the explorer, compiled to
//! wasm as a browser peer is, served on a listener of its own — bound to
//! loopback by default — over the log's authority: its store, its log and
//! the connections, writing as the authority.
//!
//! ```text
//! <bind> ── /admin/              the page (`ark-admin`, a static build)
//!        ── /admin/api/hello     whether a login is needed, and where to get one
//!        ── /admin/api/state     the module and its bodies, the store's rows, the log's
//!        │                       last lines, the connections, the CRUD exposed
//!        ── /admin/api/raw       a raw write, as the authority
//!        ── /admin/api/author    a binding of the module, as the authority
//!        ── /admin/api/load      a module file's functions, loaded into the authority
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
//! write: [`raw`] makes the change a function — a `Write` of no input, its
//! literals parsed under the module's types, the row it replaces checked to
//! be the one there — which the authority's content store loads and runs.
//! Replicas apply its facts like any entry's. Both are answered once the
//! entry is written, as an acknowledgement is.
//!
//! **Loading** is the operator giving the running authority another
//! module's functions — a newer release's a device authors at, over this
//! same schema — without a restart: the file (`(Module, [body])`, hex) is
//! verified whole, its bodies loaded ([`Handle::load`]), and an intent held
//! for one of them is judged with what waits behind it. It is the only way
//! a function reaches the content store but the modules the server started
//! with; nothing a client sends is loaded.

use std::path::PathBuf;
use std::sync::Arc;

use ark::check::Types;
use ark::ir::{Effect, Expr, Function, Node, Op};
use ark::module::{Lib, Module};
use ark::schema::{Column, Space};
use ark::store::Change;
use ark::ty::Ty;
use ark_explorer::wire::{self, Hello, State};
use ark_explorer::{crud_of, function_names, Connection, CrudVerbs, Line};
use axum::extract::State as With;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;

use crate::{Authenticate, Handle};

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
    /// The log's module (its bindings) and the library its functions are checked in.
    pub module: Module,
    pub lib: Lib,
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
        .route("/admin/api/load", post(load))
        .route_layer(axum::middleware::from_fn_with_state(ctx.clone(), refused))
        .route("/admin/api/hello", get(hello))
        .route("/", get(|| async { Redirect::to("/admin/") }))
        .with_state(ctx);
    if let Some(dir) = page {
        r = r.nest_service("/admin", crate::web::router(dir, None));
    }
    r
}

/// A raw write as a function the authority authors: `Write`, no input, the
/// change's rows as literals of the table's row type.
pub fn raw(module: &Module, c: &Change) -> Result<Function, String> {
    let (_, t) = module.schema.table(c.table()).ok_or_else(|| format!("no table {}", c.table()))?;
    let types = Types::new(&module.types);
    let col = |c: &Column| match types.decl(&c.decl) {
        Some(_) if types.deep(&Ty::Named(c.decl.clone())) == c.ty => Ty::Named(c.decl.clone()),
        _ => c.ty.clone(),
    };
    let row_ty = Ty::Tuple(t.columns.iter().map(col).collect());
    let key_ty = Ty::Tuple(t.columns[..t.key].iter().map(col).collect());
    let lit = |ty: &Ty, b: &[u8]| Box::new(Expr::Lit(ty.clone(), b.to_vec()));
    let key = |r: &[u8]| t.key_len(r).map(|n| lit(&key_ty, &r[..n])).map_err(|e| e.0);
    // Only over the row the change says is there.
    let over = |old: &[u8], then: Expr| -> Result<Expr, String> {
        let there = Expr::Lookup(t.name.clone(), key(old)?);
        let was = lit(&Ty::option(row_ty.clone()), &[&[1], old].concat());
        let why = lit(&Types::text(), &"the row is not the one the change replaces".to_string().to_bytes());
        Ok(Expr::If(
            Box::new(Expr::Op(Op::Eq, Box::new(there), was)),
            Box::new(then),
            Box::new(Expr::Refuse(why)),
        ))
    };
    let body = match c {
        Change::Add { row, .. } => Expr::Insert(t.name.clone(), lit(&row_ty, row)),
        Change::Remove { row, .. } => over(row, Expr::Delete(t.name.clone(), key(row)?))?,
        Change::Edit { old, new, .. } => over(old, Expr::Update(t.name.clone(), key(new)?, Box::new(Expr::Lambda(1, lit(&row_ty, new)))))?,
    };
    Ok(Function {
        types: module.types.clone(),
        params: vec![],
        ret: Ty::unit(),
        effect: Effect::Write,
        body,
    })
}

fn text(code: StatusCode, why: impl Into<String>) -> Response {
    (code, why.into()).into_response()
}

/// `f` on the hub's thread, off the runtime's; nothing once the hub is gone.
async fn on_hub<T: Send + 'static>(hub: &Handle, f: impl FnOnce(&mut crate::Hub) -> T + Send + 'static) -> Option<T> {
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
    let names = function_names(&admin.module);
    let exposed: Vec<(String, CrudVerbs)> = crud_of(&admin.module, &admin.lib)
        .into_iter()
        .map(|(t, v)| (t, CrudVerbs { may_author: true, ..v }))
        .collect();
    let read = on_hub(&ctx.hub, move |h| {
        let head = h.log.log.head().0;
        let name = |f| match names.get(f) {
            Some(n) => n.clone(),
            None if admin.lib.get(f).is_some_and(|l| l.f.params.is_empty()) => "raw write".into(),
            None => f.to_string(),
        };
        let at = h.log.log.entries.len().saturating_sub(LINES);
        let lines = h.log.log.entries[at..]
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
            module: admin.module.to_bytes(),
            bodies: h.log.cas.bodies().cloned().collect(),
            rows: h.log.store.snapshot_rows(),
            head,
            lines,
            connections,
            exposed,
            raw: true,
            who: h.config.me.actor.clone(),
        }
    });
    match read.await {
        Some(s) => s.to_body().into_response(),
        None => text(StatusCode::SERVICE_UNAVAILABLE, "the hub is gone"),
    }
}

async fn raw_write(With(ctx): With<Ctx>, body: String) -> Response {
    let f = match wire::raw_from(&body).and_then(|c| raw(&ctx.admin.module, &c)) {
        Ok(f) => f,
        Err(why) => return text(StatusCode::BAD_REQUEST, why),
    };
    if let Err(e) = ctx.admin.lib.install(&ctx.admin.module.schema, f.clone()) {
        return text(StatusCode::CONFLICT, e.0);
    }
    answer(&ctx.hub, f.hash(), vec![], Some(f.to_bytes())).await
}

async fn author(With(ctx): With<Ctx>, body: String) -> Response {
    match wire::author_from(&body) {
        Ok((name, input)) => match ctx.admin.module.get(&name) {
            Some(func) => answer(&ctx.hub, func, input, None).await,
            None => text(StatusCode::BAD_REQUEST, format!("no function {name}")),
        },
        Err(why) => text(StatusCode::BAD_REQUEST, why),
    }
}

/// A module file verified over this schema: its bodies, in an order each
/// installs in.
pub fn module_bodies(schema: &ark::schema::Schema, bytes: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    let (m, mut todo) = <(Module, Vec<Vec<u8>>)>::from_bytes(bytes).map_err(|e| e.0)?;
    if &m.schema != schema {
        return Err("a module loaded runs over this schema; another needs a restart".into());
    }
    let (lib, mut done) = (Lib::default(), vec![]);
    while !todo.is_empty() {
        let before = todo.len();
        let mut why = String::new();
        for b in std::mem::take(&mut todo) {
            match lib.load(schema, &b) {
                Ok(_) => done.push(b),
                Err(e) => {
                    why = e.0;
                    todo.push(b);
                }
            }
        }
        if todo.len() == before {
            return Err(why);
        }
    }
    ark::module::verify(&m, &lib).map_err(|e| e.0)?;
    Ok(done)
}

async fn load(With(ctx): With<Ctx>, body: String) -> Response {
    let bodies = match ark::hash::unhex(body.trim())
        .ok_or("not hex".to_string())
        .and_then(|b| module_bodies(&ctx.admin.module.schema, &b))
    {
        Ok(b) => b,
        Err(why) => return text(StatusCode::BAD_REQUEST, why),
    };
    let hub = ctx.hub.clone();
    match tokio::task::spawn_blocking(move || hub.load(bodies)).await.ok().flatten() {
        Some(n) => n.to_string().into_response(),
        None => text(StatusCode::SERVICE_UNAVAILABLE, "the hub is gone"),
    }
}

// Authored as the authority, its deliveries delivered: the sequence once
// written, or the refusal. A raw write's body is runnable for its entry only.
async fn answer(hub: &Handle, func: ark::hash::FnHash, input: Vec<u8>, once: Option<Vec<u8>>) -> Response {
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
