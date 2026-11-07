//! A server listening (§1.11; the `server` feature): an [`Authority`] on
//! its thread ([`crate::handle`]), and on axum.
//!
//! ```text
//! axum ── /sync ───── a socket: its first message the token, then frames ──┐
//!      ── /healthz                                                          ├─ Handle ── the server's thread:
//!      ── /auth/* (ark-auth, when signing in is on)                          │    the log's authority and the
//!      ── /*      (a web build, `web`)                                       │    live one; the log on disk,
//!      ── /media, … (the app's own routes)                                │    written before anything
//!                                                                          ─┘    said of it is delivered
//! ```
//!
//! A token proving nobody is closed with code 4401. The server
//! pings every `keepalive` and closes a socket that leaves three
//! unanswered. `/healthz` says, a line each, what any app's server is
//! asked of ([`Authority::health`]).
//!
//! An app's server starts the same way whatever the app
//! ([`Serve::auth`], [`Serve::server`]): what it is told mapped onto
//! sign-in and the server, the app adding its phones' scheme, its roles,
//! its welcome, its calls and its routes.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use ark_auth::server::Auth;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use tokio::sync::oneshot;

use crate::handle::{dev, Authenticate, Handle};
use crate::{now_ms, web, Authority, Disk, Files, Memory, Serve, ServerConfig};

/// What a server is made of: its configuration, and where it keeps
/// its log (in memory without one), how it signs people in (`auth`; or
/// `authenticate`, the dev reading by default), a web build at `/` and the
/// file whose change is a rebuild, the app's own routes, and how often it
/// pings.
pub struct Server {
    pub config: ServerConfig,
    pub data: Option<PathBuf>,
    pub authenticate: Authenticate,
    pub auth: Option<Arc<ark_auth::server::Auth>>,
    pub web: Option<(PathBuf, Option<String>)>,
    pub routes: Router,
    pub keepalive: Duration,
}

/// A server listening, until [`Running::stop`].
pub struct Running {
    pub addr: SocketAddr,
    pub hub: Handle<Authority>,
    serving: Vec<(oneshot::Sender<()>, tokio::task::JoinHandle<()>)>,
}

#[derive(Clone)]
struct Shared {
    hub: Handle<Authority>,
    authenticate: Authenticate,
    keepalive: Duration,
}

impl Server {
    pub fn new(config: impl Into<ServerConfig>) -> Server {
        Server {
            config: config.into(),
            data: None,
            authenticate: Arc::new(dev),
            auth: None,
            web: None,
            routes: Router::new(),
            keepalive: crate::settings::KEEPALIVE,
        }
    }

    /// Sign-in: its routes, each sign-in a session row the hub writes, a
    /// token judged by its signature and the row it names, and the hub
    /// watching for sessions to close — all `ark_auth::server::Auth`'s.
    pub fn auth(mut self, auth: Arc<ark_auth::server::Auth>) -> Self {
        self.auth = Some(auth);
        self
    }

    /// The server started on its thread: its handle and the routes.
    pub fn build(mut self) -> Result<(Handle<Authority>, Router)> {
        let disk: Box<dyn Disk> = match &self.data {
            Some(d) => {
                let mut f = Files::new(d.join("log"))?;
                f.sync = true;
                // Its schema beside it, so the log can be read by what does
                // not compile its domain (`arkdb export`).
                if !crate::disk::own(&mut f, &self.config.log.schema)? {
                    anyhow::bail!(crate::ANOTHER);
                }
                crate::disk::put_schema(&mut f, &self.config.log.schema)?;
                Box::new(f)
            }
            None => Box::new(Memory::default()),
        };
        let peer = Authority::serve(self.config, disk, now_ms()).map_err(anyhow::Error::msg)?;
        let hub = Handle::watched(peer, self.auth.as_ref().map(|a| a.watcher()));
        if let Some(auth) = &self.auth {
            auth.serve(Box::new(hub.clone()));
            self.authenticate = auth.authenticate();
        }
        let shared = Shared {
            hub: hub.clone(),
            authenticate: self.authenticate.clone(),
            keepalive: self.keepalive,
        };
        let sockets = Router::new().route("/sync", get(sync)).route("/healthz", get(health));
        let mut router = self.routes.merge(sockets.with_state(shared));
        if let Some(auth) = self.auth {
            router = router.merge(ark_auth::server::router(auth));
        }
        if let Some((dir, module)) = self.web {
            router = router.merge(web::router(dir, module));
        }
        Ok((hub, router))
    }

    /// Built and listening on `addr`.
    pub async fn listen(self, addr: &str) -> Result<Running> {
        let (hub, router) = self.build()?;
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let (addr, serving) = (listener.local_addr()?, vec![serving(listener, router)]);
        Ok(Running { addr, hub, serving })
    }
}

impl Serve {
    /// Sign-in as this says, over `S`'s sign-in rows (`ark_auth::server`):
    /// its key kept in the data directory; the app's own `scheme`
    /// (`harken://`) and the redirects allowed; the picture hosts trusted;
    /// and the roles granted. The app adds what is its own (its roles, its
    /// welcome).
    pub async fn auth<S: ark_auth::AuthTables>(&self, scheme: &str) -> Result<Auth> {
        std::fs::create_dir_all(&self.data).with_context(|| format!("creating {}", self.data.display()))?;
        let mode = self.sign_in.mode().await?;
        let mut auth = Auth::open::<S>(mode, &self.public_url(), &self.data)
            .context("the session key")?
            .allow_redirect(scheme)
            .with_roles(self.roles.clone());
        for prefix in &self.redirects {
            auth = auth.allow_redirect(prefix);
        }
        // A provider may point pictures at a server beside it on the private
        // network (ark's photos, say): trusted as the provider is.
        for host in &self.picture_hosts {
            auth = auth.trust_picture_host(host);
        }
        Ok(auth)
    }

    /// The server this says, of `config`, signed in by `auth`: its log in
    /// the data directory keeping `retain` entries above its snapshot, the
    /// largest file it takes `max_file`, every connection fed
    /// `session.feed` when scoped, its keepalive, and the web build at `/`.
    /// The app adds its routes.
    pub fn server(&self, config: impl Into<ServerConfig>, auth: Arc<Auth>) -> Server {
        let mut config = config.into();
        (config.keep, config.max_file) = (Some(self.retain), self.max_file);
        if self.scoped {
            config.feed = Arc::new(|_| Some("session.feed".into()));
        }
        Server {
            data: Some(self.data.clone()),
            keepalive: self.keepalive,
            web: self.web.clone().map(|dir| (dir, self.web_module.clone())),
            ..Server::new(config).auth(auth)
        }
    }
}

impl Authority {
    /// What `/healthz` says, a line each: `ok`; the connections open; the
    /// log's head and its horizon (the snapshot it stands on); the domain
    /// it runs (`domain <hash> <version> current`, [`crate::version`]),
    /// which a phone holds its own build to; and every account with a
    /// connection open, with how many.
    pub fn health(&self) -> String {
        let mut rooms: BTreeMap<&str, usize> = BTreeMap::new();
        for (_, who) in self.connections() {
            *rooms.entry(&who.actor).or_default() += 1;
        }
        let (log, domain) = (self.log.log(), crate::version(self.domain()));
        let (head, horizon, n) = (log.head().0, log.base_seq(), rooms.values().sum::<usize>());
        let mut text = format!("ok\nconnections {n}\nhead {head}\nhorizon {horizon}\ndomain {domain} current\n");
        for (room, n) in rooms {
            text.push_str(&format!("room {room} peers {n}\n"));
        }
        text
    }
}

async fn health(State(s): State<Shared>) -> Response {
    let hub = s.hub.clone();
    match tokio::task::spawn_blocking(move || hub.with(|h| h.health())).await.ok().flatten() {
        Some(text) => text.into_response(),
        None => (axum::http::StatusCode::SERVICE_UNAVAILABLE, "the hub is gone\n").into_response(),
    }
}

/// What sign-in asks of the hub: its store read on its thread, a sign-in
/// written as an entry it authors.
impl ark_auth::server::Hub for Handle<Authority> {
    fn read(&self, f: Box<dyn FnOnce(&ark::store::Store) + Send>) {
        self.with(move |p| f(&p.log.store));
    }

    fn write(&self, c: ark::store::Change) -> std::result::Result<(), String> {
        Handle::write(self, vec![c]).ok_or("the hub is gone")?.map(|_| ())
    }

    fn mutate(&self, func: &str, input: Vec<u8>) -> std::result::Result<(), String> {
        Handle::mutate(self, func.into(), input).ok_or("the hub is gone")?.map(|_| ())
    }
}

/// `router` served on `listener` until told to stop.
fn serving(listener: tokio::net::TcpListener, router: Router) -> (oneshot::Sender<()>, tokio::task::JoinHandle<()>) {
    let (stop, stopped) = oneshot::channel::<()>();
    let quit = async {
        let _ = stopped.await;
    };
    (
        stop,
        tokio::spawn(async move { drop(axum::serve(listener, router).with_graceful_shutdown(quit).await) }),
    )
}

impl Running {
    /// Stops serving, then the peer ([`Handle::stop`]): returned from, the
    /// volatile space is on disk whole.
    pub async fn stop(self) {
        for (stop, task) in self.serving {
            let _ = stop.send(());
            let _ = task.await;
        }
        let hub = self.hub;
        let _ = tokio::task::spawn_blocking(move || hub.stop()).await;
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
    let authenticate = s.authenticate.clone();
    let judged = tokio::task::spawn_blocking(move || authenticate(&token)).await;
    let Some(who) = judged.ok().flatten() else {
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
