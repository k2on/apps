//! An ArkDB sync server for any app's module, on axum.
//!
//! ```text
//! axum ── /sync ─────── the socket ──┐
//!      ── /healthz                   ├── HubHandle ── the hub's thread:
//!      ── /auth/* (ark-auth)         │     ark::protocol::Server<Relay>
//!      ── /media  (a directory)      │       the Authority, natives held
//!      ── /*      (a web build)      │       live rooms → the app's `Live`
//!      ── the app's own routes ──────┘     logs and kept rooms, written after each message
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

mod hub;
pub mod live;
pub mod persist;
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

pub use ark_client::Domain;
pub use hub::{Health, Hub, HubHandle};
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
}

impl Builder {
    /// What the server calls itself in what it prints.
    pub fn name(mut self, name: &str) -> Builder {
        self.name = name.into();
        self
    }

    /// Keep the log (`log.ark-log`) and the rooms' kept
    /// snapshots (`live.cbor`) here, and read them back at start. Without
    /// it nothing survives a restart.
    pub fn data(mut self, dir: impl AsRef<Path>) -> Builder {
        self.data = Some(dir.as_ref().to_path_buf());
        self
    }

    /// Sign people in with `ark-auth`: its routes under `/auth`, its
    /// sessions asked at every `Hello`, and its record of whose session is
    /// whose asked of an entry authored under an earlier login of the same
    /// person — so signing in again does not strand what was pending.
    pub fn auth(mut self, auth: Arc<ark_auth::server::Auth>) -> Builder {
        self.announce = Some(auth.mode().announce());
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
        for n in &notes {
            eprintln!("{n}");
        }
        let (domain, data, access, owns, live) = (self.domain.clone(), self.data.clone(), self.access, self.owns, self.live);
        let hub = HubHandle::spawn(move || {
            open_hub(
                &name,
                &domain,
                data,
                authenticate,
                owns,
                access.unwrap_or_else(open_access),
                live.unwrap_or_else(|| Box::new(Quiet)),
            )
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

fn open_hub(
    name: &str,
    domain: &Domain,
    data: Option<PathBuf>,
    auth: Authenticate,
    owns: Option<Owns>,
    access: Access,
    live: Box<dyn Live>,
) -> Result<Hub> {
    let relay = Relay::new(live);
    let schema = &domain.module().schema;
    let mut a = Authority::new(schema.clone(), domain.closures().clone());
    a.hold(domain.native_list());
    if let Some(dir) = &data {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        if let Some(log) = persist::load(dir, schema)? {
            a.store = log.state_at(log.head_seq()).context("a loaded log has no state at its head")?;
            a.log = log;
        }
    }
    eprintln!("{name}: the log at seq {}", a.log.head_seq());
    let mut server = Server::open(auth, access, relay.clone(), a);
    if let Some(owns) = owns {
        server = server.with_owns(owns);
    }
    Hub::new(server, relay, data)
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

async fn healthz(State(hub): State<HubHandle>) -> Response {
    match hub.health().await {
        Ok(h) => {
            let mut text = format!("ok\nconnections {}\n", h.connections);
            text.push_str(&format!("head {}\n", h.head));
            for (room, n) in h.rooms {
                text.push_str(&format!("room {room} peers {n}\n"));
            }
            text.into_response()
        }
        Err(e) => (axum::http::StatusCode::SERVICE_UNAVAILABLE, format!("{e:#}\n")).into_response(),
    }
}
