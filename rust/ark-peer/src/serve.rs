//! A server listening (§1.11): the peer in its server role on a thread of
//! its own, and on axum.
//!
//! ```text
//! axum ── /sync ───── a socket: its first message the token, then frames ──┐
//!      ── /healthz                                                          ├─ Handle ── the peer's thread:
//!      ── /auth/* (ark-auth, when signing in is on)                          │    the log's authority and the
//!      ── /*      (a web build, `web`)                                       │    live one; the log on disk,
//!      ── /media, … (the app's own routes)                                │    written before anything
//!                                                                          ─┘    said of it is delivered
//! <admin bind> ── /admin (the admin page, [`crate::admin`], on a listener of its own)
//! ```
//!
//! The machines are single-owner and a disk write must not stall a socket,
//! so one thread owns the [`Peer`] and everything talks to it through a
//! [`Handle`]: a socket's task, a device in this process
//! ([`Handle::dialer`], the transport the perf tests and the scanner use),
//! the sign-in routes revoking a session. What the peer delivers goes to
//! each connection's [`Sink`]; dropping a sink — the connection closed, its
//! session revoked — closes the connection.
//!
//! A connection is who its token proves (`ark-auth`'s login, or with no
//! sign-in configured the dev reading of [`dev`]); a token proving nobody
//! is closed with code 4401. The server pings every `keepalive` and closes
//! a socket that leaves three unanswered.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use ark::hash::FnHash;
use ark::log::Seq;
use ark::peer::Who;
use ark::schema::Space;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use tokio::sync::oneshot;

use crate::link::{Closed, Dial, Event, Wake, Wire};
use crate::{admin, now_ms, web, Config, ConnId, Deliver, Disk, Files, Memory, Peer};

/// Where a connection's frames go; false once it is gone.
pub type Sink = Box<dyn Fn(Vec<u8>) -> bool + Send>;

/// The admin page's bind and routes.
pub type Admin = (String, Router);

/// Who a token proves, or nobody.
pub type Authenticate = Arc<dyn Fn(&str) -> Option<Who> + Send + Sync>;

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

type Job = Box<dyn FnOnce(&mut Peer, &mut BTreeMap<ConnId, Sink>) -> Vec<Deliver> + Send>;

enum Cmd {
    Run(Job),
    Stop(Sender<()>),
}

#[derive(Clone)]
pub struct Handle {
    tx: Sender<Cmd>,
    next: Arc<AtomicU64>,
}

impl Handle {
    /// The peer's thread, started; it ends when every handle is dropped or
    /// one says [`Handle::stop`], writing the volatile space for the next
    /// open.
    pub fn spawn(mut peer: Peer) -> Handle {
        let (tx, rx) = channel::<Cmd>();
        std::thread::spawn(move || {
            let mut sinks = BTreeMap::new();
            for cmd in rx {
                let job = match cmd {
                    Cmd::Run(job) => job,
                    Cmd::Stop(done) => {
                        peer.close();
                        let _ = done.send(());
                        return;
                    }
                };
                let mut out = job(&mut peer, &mut sinks);
                out.extend(peer.release());
                for (c, b) in out {
                    if sinks.get(&c).is_some_and(|s: &Sink| !s(b)) {
                        sinks.remove(&c);
                    }
                }
            }
            peer.close();
        });
        Handle {
            tx,
            next: Arc::new(AtomicU64::new(1 << 32)),
        }
    }

    fn run(&self, job: impl FnOnce(&mut Peer, &mut BTreeMap<ConnId, Sink>) -> Vec<Deliver> + Send + 'static) -> bool {
        self.tx.send(Cmd::Run(Box::new(job))).is_ok()
    }

    /// A connection verified as `who`, its frames to `sink`.
    pub fn connect(&self, who: Who, sink: Sink) -> ConnId {
        let c = self.next.fetch_add(1, Ordering::Relaxed);
        self.run(move |p, s| {
            p.accept_as(c, who);
            s.insert(c, sink);
            vec![]
        });
        c
    }

    pub fn frame(&self, c: ConnId, bytes: Vec<u8>) {
        self.run(move |p, _| p.frame(c, bytes, now_ms()));
    }

    pub fn close(&self, c: ConnId) {
        self.run(move |p, s| {
            s.remove(&c);
            p.leave(c, now_ms())
        });
    }

    /// Every connection of a session closed: what a sign-out does.
    pub fn revoke(&self, session: &str) {
        let session = session.to_string();
        self.run(move |p, s| {
            let mut out = vec![];
            for c in p.of_session(&session) {
                s.remove(&c);
                out.extend(p.leave(c, now_ms()));
            }
            out
        });
    }

    /// The peer's thread ended and the volatile space written, waited for:
    /// what a server stopping does before its process exits, which would
    /// otherwise cut that write short. Every handle is inert after.
    pub fn stop(&self) {
        let (tx, rx) = channel();
        if self.tx.send(Cmd::Stop(tx)).is_ok() {
            let _ = rx.recv();
        }
    }

    /// `f` run on the peer's thread, and its answer.
    pub fn with<T: Send + 'static>(&self, f: impl FnOnce(&mut Peer) -> T + Send + 'static) -> Option<T> {
        let (tx, rx) = channel();
        self.run(move |p, _| {
            let _ = tx.send(f(p));
            vec![]
        });
        rx.recv().ok()
    }

    /// An entry the authority of `space` authors itself
    /// ([`Peer::author`]): the head it was sequenced at, or the refusal.
    /// With `once`, `func` is that function (a raw write), runnable for
    /// this entry only: never one a client's intent may name.
    pub fn author(
        &self,
        space: Space,
        func: FnHash,
        input: Vec<u8>,
        once: Option<std::sync::Arc<dyn ark::peer::Procedure>>,
    ) -> Option<Result<Seq, String>> {
        self.with(move |p| {
            let raw = once.is_some();
            if let Some(f) = once {
                p.log.cas.insert(func, f);
            }
            let r = p.author(space, func, input, now_ms());
            if raw && p.config.log.cas.get(&func).is_none() {
                p.log.cas.forget(&func);
            }
            r.map(|_| p.machine(space).map_or(0, |m| m.log().head().0))
        })
    }

    /// The in-process transport: a device's link dialling this server in
    /// this process, its token resolved by `authenticate`; a frame goes from
    /// the caller's thread straight to the peer's.
    pub fn dialer(&self, authenticate: Authenticate) -> impl Dial {
        let hub = self.clone();
        move |_: &str, token: &str, wake: Option<Wake>| -> Box<dyn Wire> {
            let Some(who) = authenticate(token) else {
                return Box::new(Closed(Some("denied: not signed in".into())));
            };
            let (tx, rx) = channel();
            let sink: Sink = Box::new(move |b| {
                let sent = tx.send(b).is_ok();
                wake.iter().for_each(|w| w());
                sent
            });
            let c = hub.connect(who, sink);
            Box::new(InProcess(hub.clone(), c, rx, false))
        }
    }
}

struct InProcess(Handle, ConnId, Receiver<Vec<u8>>, bool);

impl Wire for InProcess {
    fn send(&mut self, frame: Vec<u8>) {
        self.0.frame(self.1, frame);
    }

    fn events(&mut self) -> Vec<Event> {
        let mut out = vec![];
        if !std::mem::replace(&mut self.3, true) {
            out.push(Event::Open);
        }
        loop {
            match self.2.try_recv() {
                Ok(f) => out.push(Event::Frame(f)),
                Err(TryRecvError::Empty) => return out,
                Err(TryRecvError::Disconnected) => {
                    out.push(Event::Closed("the connection closed".into()));
                    return out;
                }
            }
        }
    }
}

impl Drop for InProcess {
    fn drop(&mut self) {
        self.0.close(self.1);
    }
}

/// What a server is made of: its peer's configuration, and where it keeps
/// its log (in memory without one), how it signs people in (`auth`; or
/// `authenticate`, the dev reading by default), a web build at `/` and the
/// file whose change is a rebuild, the app's own routes,
/// how often it pings, and the admin page ([`crate::admin`]).
pub struct Server {
    pub config: Config,
    pub data: Option<PathBuf>,
    pub authenticate: Authenticate,
    pub auth: Option<Arc<ark_auth::server::Auth>>,
    pub web: Option<(PathBuf, Option<String>)>,
    pub routes: Router,
    pub keepalive: Duration,
    pub admin: Option<admin::Admin>,
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

    /// Sign-in: its routes, a token read as the login it is, and a
    /// sign-out closing that session's connections.
    pub fn auth(mut self, auth: Arc<ark_auth::server::Auth>) -> Self {
        let a = auth.clone();
        self.authenticate = Arc::new(move |token| Some(crate::who(&a.whoami(token)?)));
        self.auth = Some(auth);
        self
    }

    /// The peer started on its thread: its handle, the routes, and the
    /// admin page's bind and routes.
    pub fn build(self) -> Result<(Handle, Router, Option<Admin>)> {
        let disk: Box<dyn Disk> = match &self.data {
            Some(d) => {
                let mut f = Files::new(d.join("log"))?;
                f.sync = true;
                Box::new(f)
            }
            None => Box::new(Memory::default()),
        };
        let hub = Handle::spawn(Peer::serve(self.config, disk, now_ms()));
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
        if let Some((dir, module)) = self.web {
            router = router.merge(web::router(dir, module));
        }
        let admin = self
            .admin
            .map(|a| (a.bind.clone(), admin::router(a, hub.clone(), self.authenticate.clone())));
        Ok((hub, router, admin))
    }

    /// Built and listening on `addr`.
    pub async fn listen(self, addr: &str) -> Result<Running> {
        let (hub, router, admin) = self.build()?;
        Running::listen(hub, router, admin, addr).await
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
