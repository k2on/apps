//! The ArkDB v5 server: the hub (§1.7, §1.9, §1.11) on axum.
//!
//! ```text
//! axum ── /sync ───── a socket: its first message the token, then frames ──┐
//!      ── /healthz                                                          ├─ Handle ── the hub's thread:
//!      ── /auth/* (ark-auth, when signing in is on)                          │    the log's authority and the
//!      ── /media  (a directory)                                              │    live one; the log on disk,
//!      ── /*      (a web build, `web`)                                       │    written before anything
//!      ── the app's own routes                                              ─┘    said of it is delivered
//! <admin bind> ── /admin (the admin page, [`admin`], on a listener of its own)
//! ```
//!
//! A connection is who its token proves (`ark-auth`'s login, or with no
//! sign-in configured the dev reading of [`dev`]); a token proving nobody
//! is closed with code 4401. The server pings every `keepalive` and closes
//! a socket that leaves three unanswered. A sign-out closes every
//! connection of its session at once.

pub mod admin;
pub mod handle;
pub mod hub;
pub mod web;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use ark5::peer::Who;
use ark5_client::{Disk, Files, Memory};
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use tower_http::services::ServeDir;

pub use handle::{Authenticate, Handle};
pub use hub::{Config, Hub, Live};

/// The dev reading of a token: `user@device` is `user` on a session named
/// by the whole token; `:role,role` after it, the roles.
pub fn dev(token: &str) -> Option<Who> {
    let (id, roles) = token.split_once(':').unwrap_or((token, ""));
    let actor = id.split_once('@').map_or(id, |(u, _)| u);
    let roles = roles.split(',').map(str::trim).filter(|r| !r.is_empty()).map(String::from).collect();
    (!actor.is_empty()).then(|| Who {
        actor: actor.into(),
        session: id.into(),
        roles,
    })
}

/// What a server is made of, said one piece at a time.
pub struct Builder {
    config: Config,
    data: Option<PathBuf>,
    authenticate: Authenticate,
    auth: Option<Arc<ark_auth::server::Auth>>,
    web: Option<(PathBuf, Option<String>)>,
    media: Option<PathBuf>,
    routes: Router,
    keepalive: Duration,
    admin: Option<admin::Admin>,
}

pub fn builder(config: Config) -> Builder {
    Builder {
        config,
        data: None,
        authenticate: Arc::new(dev),
        auth: None,
        web: None,
        media: None,
        routes: Router::new(),
        keepalive: Duration::from_secs(20),
        admin: None,
    }
}

impl Builder {
    /// The log on disk in `dir` (in memory without one).
    pub fn data(mut self, dir: impl Into<PathBuf>) -> Self {
        self.data = Some(dir.into());
        self
    }

    /// Sign-in: its routes, a token read as the login it is, and a
    /// sign-out closing that session's connections.
    pub fn auth(mut self, auth: Arc<ark_auth::server::Auth>) -> Self {
        let a = auth.clone();
        self.authenticate = Arc::new(move |token| {
            let l = a.whoami(token)?;
            Some(ark5_client::auth::who(&l))
        });
        self.auth = Some(auth);
        self
    }

    pub fn authenticate(mut self, f: Authenticate) -> Self {
        self.authenticate = f;
        self
    }

    /// A web build at `/`, `module` the file whose change is a rebuild.
    pub fn web(mut self, dir: impl Into<PathBuf>, module: Option<String>) -> Self {
        self.web = Some((dir.into(), module));
        self
    }

    pub fn media(mut self, dir: impl Into<PathBuf>) -> Self {
        self.media = Some(dir.into());
        self
    }

    pub fn merge(mut self, routes: Router) -> Self {
        self.routes = self.routes.merge(routes);
        self
    }

    pub fn keepalive(mut self, every: Duration) -> Self {
        self.keepalive = every;
        self
    }

    /// The admin page, on a listener of its own ([`admin`]).
    pub fn admin(mut self, a: admin::Admin) -> Self {
        self.admin = Some(a);
        self
    }

    pub fn build(self) -> Result<App> {
        let disk: Box<dyn Disk> = match &self.data {
            Some(d) => {
                let mut f = Files::new(d.join("log"))?;
                f.sync = true;
                Box::new(f)
            }
            None => Box::new(Memory::default()),
        };
        let hub = Handle::spawn(Hub::open(self.config, disk, handle::now()));
        let shared = Shared {
            hub: hub.clone(),
            authenticate: self.authenticate.clone(),
            keepalive: self.keepalive,
        };
        let sockets = Router::new().route("/sync", get(sync)).with_state(shared);
        let mut router = self.routes.route("/healthz", get(|| async { "ok" })).merge(sockets);
        if let Some(auth) = self.auth {
            let h = hub.clone();
            auth.on_revoke(move |_, session| h.revoke(session));
            router = router.merge(ark_auth::server::router(auth));
        }
        if let Some(m) = self.media {
            router = router.nest_service("/media", ServeDir::new(m));
        }
        if let Some((dir, module)) = self.web {
            router = router.merge(web::router(dir, module));
        }
        let admin = self
            .admin
            .map(|a| (a.bind.clone(), admin::router(a, hub.clone(), self.authenticate.clone())));
        Ok(App {
            hub,
            router,
            authenticate: self.authenticate,
            admin,
        })
    }
}

#[derive(Clone)]
struct Shared {
    hub: Handle,
    authenticate: Authenticate,
    keepalive: Duration,
}

/// A server built: its hub and its routes.
pub struct App {
    pub hub: Handle,
    pub router: Router,
    pub authenticate: Authenticate,
    /// The admin page's bind and routes, served beside the rest.
    pub admin: Option<(String, Router)>,
}

/// A server serving, until [`Running::stop`].
pub struct Running {
    pub addr: SocketAddr,
    /// Where the admin page listens, when there is one.
    pub admin_addr: Option<SocketAddr>,
    pub hub: Handle,
    serving: Vec<(tokio::sync::oneshot::Sender<()>, tokio::task::JoinHandle<()>)>,
}

/// `router` served on `listener` until told to stop.
fn serving(listener: tokio::net::TcpListener, router: Router) -> (tokio::sync::oneshot::Sender<()>, tokio::task::JoinHandle<()>) {
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        let quit = async {
            let _ = stopped.await;
        };
        let _ = axum::serve(listener, router).with_graceful_shutdown(quit).await;
    });
    (stop, task)
}

impl App {
    pub async fn serve(self, addr: &str) -> Result<Running> {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let addr = listener.local_addr()?;
        let mut admin_addr = None;
        let mut all = vec![];
        if let Some((bind, router)) = self.admin {
            let l = tokio::net::TcpListener::bind(&bind).await?;
            admin_addr = Some(l.local_addr()?);
            all.push(serving(l, router));
        }
        all.push(serving(listener, self.router));
        Ok(Running {
            addr,
            admin_addr,
            hub: self.hub,
            serving: all,
        })
    }
}

impl Running {
    pub async fn stop(self) {
        for (stop, task) in self.serving {
            let _ = stop.send(());
            let _ = task.await;
        }
    }
}

async fn sync(ws: WebSocketUpgrade, State(s): State<Shared>) -> Response {
    ws.on_upgrade(move |socket| connection(socket, s))
}

/// One socket: the token, then frames both ways and the pings.
async fn connection(mut socket: WebSocket, s: Shared) {
    let token = match socket.recv().await {
        Some(Ok(Message::Binary(b))) => String::from_utf8(b).unwrap_or_default(),
        _ => return,
    };
    let Some(who) = (s.authenticate)(&token) else {
        let reason = "not signed in".into();
        let _ = socket.send(Message::Close(Some(CloseFrame { code: 4401, reason }))).await;
        return;
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let c = s.hub.connect(who, Box::new(move |b| tx.send(b).is_ok()));
    let mut ping = tokio::time::interval(s.keepalive);
    ping.tick().await;
    let mut unanswered = 0;
    loop {
        tokio::select! {
            m = socket.recv() => match m {
                Some(Ok(Message::Binary(b))) => {
                    unanswered = 0;
                    s.hub.frame(c, b);
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => unanswered = 0,
            },
            f = rx.recv() => match f {
                Some(b) => if socket.send(Message::Binary(b)).await.is_err() { break },
                None => {
                    let _ = socket.send(Message::Close(None)).await;
                    break;
                }
            },
            _ = ping.tick() => {
                if unanswered >= 3 || socket.send(Message::Ping(vec![])).await.is_err() {
                    break;
                }
                unanswered += 1;
            }
        }
    }
    s.hub.close(c);
}
