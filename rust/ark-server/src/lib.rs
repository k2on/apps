//! An ArkDB sync server for any app's module, on axum.
//!
//! ```text
//! axum ── /sync ─────── the socket ──┐
//!      ── /healthz                   ├── HubHandle ── the hub's thread:
//!      ── /auth/* (ark-auth)         │     ark::protocol::Server<Relay>
//!      ── /media  (a directory)      │       the Authority, natives held
//!      ── /*      (a web build)      │       live rooms → the app's `Live`
//!      ── the app's own routes ──────┘     the log and kept rooms, written after each
//!                                          message and before anything is said of it
//! ```
//!
//! An app builds its server from a [`Builder`] and adds its own routes:
//!
//! ```no_run
//! # async fn run() -> anyhow::Result<()> {
//! use std::sync::Arc;
//! use ark_auth::server::{Auth, Mode};
//! use ark_auth::session::SessionStore;
//!
//! let data = std::path::PathBuf::from("app-data");
//! let sessions = SessionStore::open(data.join("sessions.json")).map_err(anyhow::Error::msg)?;
//! let auth = Arc::new(Auth::new(sessions, Mode::Dev, "http://127.0.0.1:8787").allow_redirect("myapp://"));
//! let app = ark_server::builder(ark_client::demo::domain())
//!     .name("demo")
//!     .data(&data)
//!     .auth(auth)
//!     .live(ark_server::Echo)
//!     .media("/srv/media")
//!     .web("/srv/web")
//!     .merge(axum::Router::new().route("/version", axum::routing::get(|| async { "1" })))
//!     .build()?;
//! let running = app.serve("127.0.0.1:8787").await?;
//! # running.stop().await; Ok(()) }
//! ```

pub mod hooks;
mod hub;
pub mod live;
pub mod modules;
pub mod persist;
pub mod retain;
mod sync;
pub mod web;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use ark::peer::Authority;
use ark::protocol::{open_access, trusting, Access, Authenticate, Owns, Server};
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tower_http::services::ServeDir;

pub use ark::retention::Retention;
pub use ark_client::Domain;
pub use hooks::Hook;
pub use hub::{Health, Hub, HubHandle, SessionHealth, REVOKED};
pub use live::{Echo, Live, Peer, Post, Quiet};
pub use sync::Keepalive;

use live::Relay;

/// The session the engine's dev `trusting` gives every login.
pub const DEV_SESSION: &str = "dev";

/// Start building a server for `domain`.
pub fn builder(domain: Domain) -> Builder {
    Builder {
        domain,
        name: "ark-server".into(),
        data: None,
        authenticate: None,
        owns: None,
        auth: None,
        announce: None,
        access: None,
        live: None,
        web: None,
        media: None,
        routes: Router::new(),
        keepalive: Keepalive::default(),
        retention: Retention::default(),
        ran_before: vec![],
        hooks: vec![],
    }
}

/// What a server is made of, said one piece at a time.
pub struct Builder {
    domain: Domain,
    name: String,
    data: Option<PathBuf>,
    authenticate: Option<Authenticate>,
    owns: Option<Owns>,
    auth: Option<Arc<ark_auth::server::Auth>>,
    announce: Option<String>,
    access: Option<Access>,
    live: Option<Box<dyn Live>>,
    web: Option<(PathBuf, Option<String>)>,
    media: Option<PathBuf>,
    routes: Router,
    keepalive: Keepalive,
    retention: Retention,
    ran_before: Vec<modules::Ran>,
    hooks: Vec<(String, Hook)>,
}

impl Builder {
    /// A module this server is to hold as one it has run, though it never
    /// started with it: its closures kept and an intent at one of its
    /// hashes sequenced through them (closure provenance, [`modules`]),
    /// recorded in `modules.cbor` as if it had been the module of an
    /// earlier start. What a server deployed fresh needs to take the
    /// intents of clients built before it — the old hashes of mutators the
    /// current module has since moved, a guard added among them
    /// (`docs/plan-guards.md` D1). Their entries run the old closures as
    /// they always did, unguarded.
    pub fn ran_before(mut self, domain: &Domain) -> Builder {
        let closures = domain.closures().iter().map(|(h, c)| (h.clone(), c.clone())).collect();
        self.ran_before.push(modules::Ran {
            module: domain.hash(),
            private: domain.private_hash(),
            closures,
        });
        self
    }

    /// What the server calls itself in what it prints.
    pub fn name(mut self, name: &str) -> Builder {
        self.name = name.into();
        self
    }

    /// Keep the log (`log.ark-log`, a snapshot, and `log.ark-journal`, the
    /// entries since it: [`persist`]), the rooms' kept snapshots
    /// (`live.cbor`) and every session's place in the log (`cursors.cbor`:
    /// [`retain`]) here, and read them back at start. Without it nothing
    /// survives a restart.
    pub fn data(mut self, dir: impl AsRef<Path>) -> Builder {
        self.data = Some(dir.as_ref().to_path_buf());
        self
    }

    /// Sign people in with `ark-auth`: its routes under `/auth`, its
    /// sessions asked at every `Hello`, and its record of whose session is
    /// whose asked of an entry authored under an earlier login of the same
    /// person — so signing in again does not strand what was pending.
    pub fn auth(mut self, auth: Arc<ark_auth::server::Auth>) -> Builder {
        self.announce = Some(auth.announce());
        self.authenticate = Some(auth.authenticator());
        self.owns = Some(auth.owns_fn());
        self.auth = Some(auth);
        self
    }

    /// The engine's dev auth with no routes at all: a token is a name, the
    /// session is always `"dev"`. For a test or a toy; said loudly at start.
    pub fn trusting(mut self) -> Builder {
        self.announce = Some("*** DEV AUTH (trusting): a token is a name, nothing is checked. Never anywhere but a laptop. ***".into());
        self.authenticate = Some(trusting());
        self
    }

    /// Whether a session is, or was, a user's — for an entry pushed under
    /// an older login of the connection's own user. `.auth` sets it.
    pub fn owns(mut self, owns: Owns) -> Builder {
        self.owns = Some(owns);
        self
    }

    /// Any other answer to what a token proves.
    pub fn authenticate(mut self, a: Authenticate, announce: &str) -> Builder {
        self.announce = Some(announce.into());
        self.authenticate = Some(a);
        self
    }

    /// Who may read the log; everyone signed in, without it.
    pub fn access(mut self, a: Access) -> Builder {
        self.access = Some(a);
        self
    }

    /// The app's live machine: what happens in each account's room.
    pub fn live(mut self, live: impl Live) -> Builder {
        self.live = Some(Box::new(live));
        self
    }

    /// Serve a web build at `/`, every miss falling back to its
    /// `index.html`, with the build as the validator ([`web`]).
    pub fn web(mut self, dir: impl AsRef<Path>) -> Builder {
        self.web = Some((dir.as_ref().to_path_buf(), None));
        self
    }

    /// Name the file whose change is a rebuild off the store
    /// (`pkg/app_bg.wasm`); see [`web::build_tag`].
    pub fn web_module(mut self, module: &str) -> Builder {
        if let Some(w) = &mut self.web {
            w.1 = Some(module.into());
        }
        self
    }

    /// Serve a directory at `/media`, with range requests, unauthenticated
    /// — which is what lets a speaker on the LAN fetch it, and what makes
    /// the directory public the moment the server is.
    pub fn media(mut self, dir: impl AsRef<Path>) -> Builder {
        self.media = Some(dir.as_ref().to_path_buf());
        self
    }

    /// The app's own routes, matched before the web build.
    pub fn merge(mut self, routes: Router) -> Builder {
        self.routes = self.routes.merge(routes);
        self
    }

    pub fn keepalive(mut self, k: Keepalive) -> Builder {
        self.keepalive = k;
        self
    }

    /// How much of the log is kept in memory and on the disk
    /// (`ark::retention`; the `hub` module docs): everything above the
    /// lowest cursor of a session heard within `days`, and never fewer
    /// than `entries` below the head. [`Retention::default`] without it.
    pub fn retain(mut self, r: Retention) -> Builder {
        self.retention = r;
        self
    }

    /// `docs/plan-guards.md` D3 A server hook: `hook` is handed every entry
    /// of the function `fn_name` once it is durable — after the write the
    /// `Ack` waits for — with its facts, the private blocks' included, on a
    /// thread of its own and in sequence order ([`hooks`]). For the world
    /// outside the database: a row is a private block's to write, and a hook
    /// that panics is said and leaves the entry as it was.
    pub fn on_committed(mut self, fn_name: &str, hook: Hook) -> Builder {
        self.hooks.push((fn_name.into(), hook));
        self
    }

    /// Host the log it left on disk, start the hub, and
    /// assemble the router. Refuses a server told nothing about who people
    /// are: dev auth has to be asked for.
    pub fn build(self) -> Result<App> {
        let Some(authenticate) = self.authenticate else {
            bail!(
                "{}: no sign-in configured — give it ark-auth (`.auth`), or ask for dev auth (`.trusting`)",
                self.name
            );
        };
        let name = self.name.clone();
        let mut notes = vec![format!(
            "{name}: module {} functions, {} native, {} tables",
            self.domain.module().functions.len(),
            self.domain.natives().len(),
            self.domain.module().schema.tables.len()
        )];
        if let Some(a) = &self.announce {
            notes.push(format!("{name}: {a}"));
        }
        let r = self.retention;
        notes.push(format!(
            "{name}: keeping the log above every session heard within {} days, and never fewer than {} entries",
            r.days, r.entries
        ));
        for n in &notes {
            eprintln!("{n}");
        }
        let (domain, data, access, owns, live) = (self.domain.clone(), self.data.clone(), self.access, self.owns, self.live);
        let ran_before = self.ran_before;
        let hooks = self.hooks;
        let hub = HubHandle::spawn(move || {
            open_hub(
                &name,
                &domain,
                data,
                authenticate,
                owns,
                access.unwrap_or_else(open_access),
                live.unwrap_or_else(|| Box::new(Quiet)),
                ran_before,
            )
            .map(|mut hub| {
                hub.retention = r;
                hub.hook(hooks);
                hub
            })
        })?;

        let mut router = Router::new()
            .route("/healthz", get(healthz))
            .with_state(hub.clone())
            .merge(Router::new().route("/sync", get(sync::sync)).with_state(sync::SyncState {
                hub: hub.clone(),
                keepalive: self.keepalive,
            }))
            .merge(self.routes);
        if let Some(auth) = &self.auth {
            // A session revoked at `/auth/logout` closes its sockets now,
            // not when they next dial (`hub.rs`, R6).
            auth.on_revoke(hub.revoker());
            router = router.merge(ark_auth::server::router(auth.clone()));
        }
        if let Some(media) = &self.media {
            eprintln!("{}: serving {} at /media, unauthenticated", self.name, media.display());
            router = router.nest_service("/media", ServeDir::new(media));
        }
        if let Some((dir, module)) = self.web {
            eprintln!("{}: serving the web build {}", self.name, dir.display());
            router = router.fallback_service(web::router_with_module(dir, module));
        }
        Ok(App {
            name: self.name,
            hub,
            router,
            notes,
        })
    }
}

// Everything a hub is opened from, as the builder holds it.
#[allow(clippy::too_many_arguments)]
fn open_hub(
    name: &str,
    domain: &Domain,
    data: Option<PathBuf>,
    auth: Authenticate,
    owns: Option<Owns>,
    access: Access,
    live: Box<dyn Live>,
    ran_before: Vec<modules::Ran>,
) -> Result<Hub> {
    let relay = Relay::new(live);
    let schema = &domain.module().schema;
    let mut a = Authority::new(schema.clone(), domain.closures().clone());
    a.hold(domain.native_list());
    // Every module this server has run, this one among them, and their
    // closures held and kept (closure provenance, `modules` module docs).
    // The public hash — of the module a client loads, private blocks
    // stripped — is what every page says; the private one, where the module
    // has a server half, is recorded beside it (`docs/plan-guards.md` D3).
    let module = domain.hash();
    let this = modules::Ran {
        module: module.clone(),
        private: domain.private_hash(),
        closures: domain.closures().iter().map(|(h, c)| (h.clone(), c.clone())).collect(),
    };
    // The modules it is told it ran before ([`Builder::ran_before`]) are
    // recorded first, as earlier starts would have been; whether this
    // start's own module is new is asked after them.
    let (ran, fresh) = match &data {
        Some(dir) => {
            for r in ran_before.into_iter().filter(|r| r.module != module) {
                modules::start_with(dir, r)?;
            }
            modules::start_with(dir, this)?
        }
        None => {
            let mut ran: Vec<modules::Ran> = ran_before.into_iter().filter(|r| r.module != module).collect();
            ran.push(this);
            (ran, true)
        }
    };
    if ran.len() > 1 {
        eprintln!("{name}: holding the closures of {} modules run before this one", ran.len() - 1);
    }
    // The current module's closures are held first (`Authority::new`
    // above), and `ran` keeps a closure already held: so an intent at a hash
    // an earlier module shipped too runs this module's private half, which is
    // the one this server runs now.
    for r in ran {
        a.ran(r.module, r.closures);
    }
    let mut file = None;
    if let Some(dir) = &data {
        // The first start with a module may be over a log written under
        // another: its snapshot is hashed again under this schema before
        // it is read (`persist::rehome`, `docs/plan-db.md` D1). So is one
        // hashed by an older construction of the state hash (D3).
        if persist::rehome(dir, schema, fresh)? {
            eprintln!("{name}: the log's snapshot hashed again, under this module and this build's state hash");
        }
        let (f, log) = persist::LogFile::open(dir, schema)?;
        if let Some(log) = log {
            // Rows the journal's older facts wrote without a column this
            // schema added hold it `Null`, as every peer of it does
            // (`persist::widen`).
            a.store = persist::widen(&log.state_at(log.head_seq()).context("a loaded log has no state at its head")?);
            a.log = log;
        }
        file = Some(f);
    }
    eprintln!("{name}: the log at seq {}, its horizon at {}", a.log.head_seq(), a.log.horizon());
    // `docs/plan-guards.md` D2 Each connection is served what its identity
    // holds of the module's scopes; a module with none, everything, as
    // ever.
    let mut server = Server::open(auth, access, relay.clone(), a)
        .with_module(module)
        .with_scopes(ark::scope::Scopes::of(domain.module()));
    if let Some(owns) = owns {
        server = server.with_owns(owns);
    }
    Hub::new(server, relay, data, file)
}

/// A server that is built and not yet listening: its hub, and the router
/// to serve (or to nest in a bigger one).
pub struct App {
    name: String,
    pub hub: HubHandle,
    pub router: Router,
    /// What it said at startup.
    pub notes: Vec<String>,
}

impl App {
    /// Bind and serve. Port 0 takes an ephemeral one, reported in
    /// [`Running::addr`].
    pub async fn serve(self, listen: &str) -> Result<Running> {
        let listener = TcpListener::bind(listen).await.with_context(|| format!("binding {listen}"))?;
        let addr = listener.local_addr()?;
        eprintln!("{}: listening on http://{addr} (sync at ws://{addr}/sync)", self.name);
        let (shutdown, stopped) = oneshot::channel::<()>();
        let router = self.router;
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .context("serving")
        });
        Ok(Running {
            addr,
            hub: self.hub,
            shutdown: Some(shutdown),
            task,
        })
    }
}

/// A server that is up: where, and how to stop it.
pub struct Running {
    pub addr: SocketAddr,
    pub hub: HubHandle,
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<Result<()>>,
}

impl Running {
    /// `http://host:port`.
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// `ws://host:port/sync`.
    pub fn sync_url(&self) -> String {
        format!("ws://{}/sync", self.addr)
    }

    /// Stop accepting, let open connections finish, and give up on them
    /// after a few seconds.
    pub async fn stop(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if tokio::time::timeout(Duration::from_secs(5), &mut self.task).await.is_err() {
            self.task.abort();
        }
    }
}

async fn healthz(State(hub): State<HubHandle>, headers: axum::http::HeaderMap) -> Response {
    // Asked for JSON, it answers JSON: the same facts and the sessions
    // beside them, for a tool rather than a person (`docs/plan-db.md` D6).
    let json = headers
        .get_all(axum::http::header::ACCEPT)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .any(|v| v.split(',').any(|t| t.split(';').next().is_some_and(|m| m.trim() == "application/json")));
    match hub.health().await {
        Ok(h) if json => ([(axum::http::header::CONTENT_TYPE, "application/json")], health_json(&h)).into_response(),
        Ok(h) => {
            let mut text = format!("ok\nconnections {}\n", h.connections);
            text.push_str(&format!("head {}\n", h.head));
            text.push_str(&format!("horizon {}\n", h.horizon));
            // Every module this server has run, the current one marked
            // (closure provenance, `modules`).
            for m in &h.modules {
                let current = if Some(m) == h.module.as_ref() { " current" } else { "" };
                text.push_str(&format!("module {}{current}\n", ark::value::hex(m)));
            }
            for (room, n) in h.rooms {
                text.push_str(&format!("room {room} peers {n}\n"));
            }
            text.into_response()
        }
        Err(e) => (axum::http::StatusCode::SERVICE_UNAVAILABLE, format!("{e:#}\n")).into_response(),
    }
}

/// `/healthz` as JSON (`docs/plan-db.md` D6): one object, hashes and ids
/// in hex, times in milliseconds since the Unix epoch.
///
/// ```text
/// { "status": "ok", "head": 12, "horizon": 0, "log": "…" | null,
///   "module": "…" | null, "modules": ["…"], "connections": 2,
///   "sessions": [ { "user", "session", "cursor", "heard_ms", "open" } ],
///   "rooms": [ { "room", "peers" } ] }
/// ```
pub fn health_json(h: &Health) -> String {
    use ark::json::{array, quoted};
    use ark::value::hex;
    let hexed = |b: Option<&[u8]>| b.map_or_else(|| "null".to_string(), |b| quoted(&hex(b)));
    let sessions = array(h.sessions.iter().map(|s| {
        format!(
            "{{\"user\":{},\"session\":{},\"cursor\":{},\"heard_ms\":{},\"open\":{}}}",
            quoted(&s.user),
            quoted(&s.session),
            s.cursor,
            s.heard_ms,
            s.open
        )
    }));
    let rooms = array(h.rooms.iter().map(|(r, n)| format!("{{\"room\":{},\"peers\":{n}}}", quoted(r))));
    format!(
        "{{\"status\":\"ok\",\"head\":{},\"horizon\":{},\"log\":{},\"module\":{},\"modules\":{},\"connections\":{},\"sessions\":{sessions},\"rooms\":{rooms}}}\n",
        h.head,
        h.horizon,
        hexed(h.log_id.as_ref().map(|i| &i[..])),
        hexed(h.module.as_deref()),
        array(h.modules.iter().map(|m| quoted(&hex(m)))),
        h.connections,
    )
}
