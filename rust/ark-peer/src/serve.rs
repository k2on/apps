//! A server listening (§1.11; the `server` feature): the peer in its
//! server role on its thread ([`crate::handle`]), and on axum.
//!
//! ```text
//! axum ── /sync ───── a socket: its first message the token, then frames ──┐
//!      ── /healthz                                                          ├─ Handle ── the peer's thread:
//!      ── /auth/* (ark-auth, when signing in is on)                          │    the log's authority and the
//!      ── /*      (a web build, `web`)                                       │    live one; the log on disk,
//!      ── /media, … (the app's own routes)                                │    written before anything
//!                                                                          ─┘    said of it is delivered
//! <admin bind> ── /admin (the admin page's routes, `ark_explorer::serve`, on a listener of its own)
//! ```
//!
//! A token proving nobody is closed with code 4401. The server
//! pings every `keepalive` and closes a socket that leaves three
//! unanswered.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use tokio::sync::oneshot;

use crate::handle::{dev, Authenticate, Handle};
use crate::{now_ms, web, Config, Disk, Files, Memory, Peer};

/// The admin page's routes, made once the hub is there: from its handle and
/// how a token is read (`ark_explorer::serve::Admin::routes`).
pub type AdminRoutes = Box<dyn FnOnce(Handle, Authenticate) -> Router + Send>;

/// The admin page's bind and routes, made.
pub type Admin = (String, Router);

/// What a server is made of: its peer's configuration, and where it keeps
/// its log (in memory without one), how it signs people in (`auth`; or
/// `authenticate`, the dev reading by default), a web build at `/` and the
/// file whose change is a rebuild, the app's own routes,
/// how often it pings, and the admin page: its bind and its routes.
pub struct Server {
    pub config: Config,
    pub data: Option<PathBuf>,
    pub authenticate: Authenticate,
    pub auth: Option<Arc<ark_auth::server::Auth>>,
    pub web: Option<(PathBuf, Option<String>)>,
    pub routes: Router,
    pub keepalive: Duration,
    pub admin: Option<(String, AdminRoutes)>,
}

/// A server listening, until [`Running::stop`].
pub struct Running {
    pub addr: SocketAddr,
    /// Where the admin page listens, when there is one.
    pub admin_addr: Option<SocketAddr>,
    pub hub: Handle,
    serving: Vec<(oneshot::Sender<()>, tokio::task::JoinHandle<()>)>,
}

#[derive(Clone)]
struct Shared {
    hub: Handle,
    authenticate: Authenticate,
    keepalive: Duration,
}

impl Server {
    pub fn new(config: Config) -> Server {
        Server {
            config,
            data: None,
            authenticate: Arc::new(dev),
            auth: None,
            web: None,
            routes: Router::new(),
            keepalive: Duration::from_secs(20),
            admin: None,
        }
    }

    /// Sign-in: its routes, each sign-in a session row the hub writes, a
    /// token judged by its signature and the row it names, and the hub
    /// watching for sessions to close — all `ark_auth::server::Auth`'s.
    pub fn auth(mut self, auth: Arc<ark_auth::server::Auth>) -> Self {
        self.auth = Some(auth);
        self
    }

    /// The peer started on its thread: its handle, the routes, and the
    /// admin page's bind and routes.
    pub fn build(mut self) -> Result<(Handle, Router, Option<Admin>)> {
        let disk: Box<dyn Disk> = match &self.data {
            Some(d) => {
                let mut f = Files::new(d.join("log"))?;
                f.sync = true;
                Box::new(f)
            }
            None => Box::new(Memory::default()),
        };
        let peer = Peer::serve(self.config, disk, now_ms());
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
        let sockets = Router::new().route("/sync", get(sync)).with_state(shared);
        let mut router = self.routes.route("/healthz", get(|| async { "ok" })).merge(sockets);
        if let Some(auth) = self.auth {
            router = router.merge(ark_auth::server::router(auth));
        }
        if let Some((dir, module)) = self.web {
            router = router.merge(web::router(dir, module));
        }
        let admin = self.admin.map(|(bind, routes)| (bind, routes(hub.clone(), self.authenticate.clone())));
        Ok((hub, router, admin))
    }

    /// Built and listening on `addr`.
    pub async fn listen(self, addr: &str) -> Result<Running> {
        let (hub, router, admin) = self.build()?;
        Running::listen(hub, router, admin, addr).await
    }
}

/// What sign-in asks of the hub: its store read on its thread, a sign-in
/// written as an entry it authors.
impl ark_auth::server::Hub for Handle {
    fn read(&self, f: Box<dyn FnOnce(&ark::store::Store) + Send>) {
        self.with(move |p| f(&p.log.store));
    }

    fn write(&self, c: ark::store::Change) -> std::result::Result<(), String> {
        Handle::write(self, vec![c]).ok_or("the hub is gone")?.map(|_| ())
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
    /// `router` (and the admin page's) served for `hub`.
    pub async fn listen(hub: Handle, router: Router, admin: Option<Admin>, addr: &str) -> Result<Running> {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let addr = listener.local_addr()?;
        let mut admin_addr = None;
        let mut all = vec![];
        if let Some((bind, router)) = admin {
            let l = tokio::net::TcpListener::bind(&bind).await?;
            admin_addr = Some(l.local_addr()?);
            all.push(serving(l, router));
        }
        all.push(serving(listener, router));
        Ok(Running {
            addr,
            admin_addr,
            hub,
            serving: all,
        })
    }

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
