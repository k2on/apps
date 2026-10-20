//! The hub on a thread of its own, and how connections reach it.
//!
//! The machines are single-owner and a disk write must not stall a socket,
//! so one thread owns the [`Hub`] and everything talks to it through a
//! [`Handle`]: a socket's task, an in-process client ([`Handle::dial`],
//! the transport the perf tests use), the sign-in routes revoking a
//! session. What the hub delivers goes to each connection's [`Sink`]; the
//! hub dropping a sink — the connection closed, its session revoked —
//! closes the connection.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use ark5::hash::FnHash;
use ark5::log::Seq;
use ark5::peer::Who;
use ark5::schema::Space;
use ark5_client::link::{Conn, Dial, End};

use crate::hub::{ConnId, Deliver, Hub};

/// Where a connection's frames go; false once it is gone.
pub type Sink = Box<dyn Fn(Vec<u8>) -> bool + Send>;

/// Who a token proves, or nobody.
pub type Authenticate = Arc<dyn Fn(&str) -> Option<Who> + Send + Sync>;

type Job = Box<dyn FnOnce(&mut Hub) -> Vec<Deliver> + Send>;

enum Cmd {
    Connect(ConnId, Who, Sink),
    Frame(ConnId, Vec<u8>),
    Close(ConnId),
    Revoke(String),
    Run(Job),
}

#[derive(Clone)]
pub struct Handle {
    tx: Sender<Cmd>,
    next: Arc<AtomicU64>,
}

/// Milliseconds since the epoch: the hub's clock.
pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

impl Handle {
    /// The hub's thread, started; it ends when every handle is dropped,
    /// writing the volatile space for the next open.
    pub fn spawn(mut hub: Hub) -> Handle {
        let (tx, rx) = channel::<Cmd>();
        thread::spawn(move || {
            let mut sinks: BTreeMap<ConnId, Sink> = BTreeMap::new();
            let send = |sinks: &mut BTreeMap<ConnId, Sink>, out: Vec<Deliver>| {
                for (c, b) in out {
                    if sinks.get(&c).is_some_and(|s| !s(b)) {
                        sinks.remove(&c);
                    }
                }
            };
            for cmd in rx {
                let out = match cmd {
                    Cmd::Connect(c, who, sink) => {
                        hub.connect_as(c, who);
                        sinks.insert(c, sink);
                        hub.flushed()
                    }
                    Cmd::Frame(c, b) => hub.frame(c, b, now()),
                    Cmd::Close(c) => {
                        sinks.remove(&c);
                        hub.disconnect(c, now())
                    }
                    Cmd::Revoke(session) => {
                        let mut out = vec![];
                        for c in hub.of_session(&session) {
                            sinks.remove(&c);
                            out.extend(hub.disconnect(c, now()));
                        }
                        out
                    }
                    Cmd::Run(job) => {
                        let mut out = job(&mut hub);
                        out.extend(hub.flushed());
                        out
                    }
                };
                send(&mut sinks, out);
            }
            hub.close();
        });
        Handle {
            tx,
            next: Arc::new(AtomicU64::new(1 << 32)),
        }
    }

    /// A connection verified as `who`, its frames to `sink`.
    pub fn connect(&self, who: Who, sink: Sink) -> ConnId {
        let c = self.next.fetch_add(1, Ordering::Relaxed);
        let _ = self.tx.send(Cmd::Connect(c, who, sink));
        c
    }

    pub fn frame(&self, c: ConnId, bytes: Vec<u8>) {
        let _ = self.tx.send(Cmd::Frame(c, bytes));
    }

    pub fn close(&self, c: ConnId) {
        let _ = self.tx.send(Cmd::Close(c));
    }

    /// Every connection of a session closed: what a sign-out does.
    pub fn revoke(&self, session: &str) {
        let _ = self.tx.send(Cmd::Revoke(session.into()));
    }

    /// `f` run on the hub's thread, and its answer.
    pub fn with<T: Send + 'static>(&self, f: impl FnOnce(&mut Hub) -> T + Send + 'static) -> Option<T> {
        let (tx, rx) = channel();
        let job: Job = Box::new(move |h| {
            let _ = tx.send(f(h));
            vec![]
        });
        self.tx.send(Cmd::Run(job)).ok()?;
        rx.recv().ok()
    }

    /// Bodies the operator loads into the log's authority ([`Hub::load`]),
    /// what it says delivered: how many it runs now that it did not.
    pub fn load(&self, bodies: Vec<Vec<u8>>) -> Option<usize> {
        let (tx, rx) = channel();
        let job: Job = Box::new(move |h| {
            let (n, out) = h.load(bodies);
            let _ = tx.send(n);
            out
        });
        self.tx.send(Cmd::Run(job)).ok()?;
        rx.recv().ok()
    }

    /// An entry the authority of `space` authors itself ([`Hub::author`]),
    /// what it says delivered: the head it was sequenced at, or the refusal.
    /// With `once`, the function is that body (a raw write), runnable for
    /// this entry only: never one a client's intent may name.
    pub fn author(&self, space: Space, func: FnHash, input: Vec<u8>, once: Option<Vec<u8>>) -> Option<Result<Seq, String>> {
        let (tx, rx) = channel();
        let job: Job = Box::new(move |h| {
            let raw = once.map(|b| h.log.cas.add_body(b));
            let (r, out) = h.author(space, raw.unwrap_or(func), input, now());
            if let Some(f) = raw.filter(|f| h.config.log.cas.get(f).is_none()) {
                h.log.cas.forget(&f);
            }
            let peer = if space == Space::Durable { Some(&h.log) } else { h.live.as_ref() };
            let _ = tx.send(r.map(|_| peer.map_or(0, |p| p.log.head().0)));
            out
        });
        self.tx.send(Cmd::Run(job)).ok()?;
        rx.recv().ok()
    }

    /// The in-process transport: a client's link dials the hub in this
    /// process, its token resolved by `authenticate`.
    pub fn dial(&self, authenticate: Authenticate) -> InProcess {
        InProcess {
            hub: self.clone(),
            authenticate,
        }
    }
}

pub struct InProcess {
    hub: Handle,
    authenticate: Authenticate,
}

impl Dial for InProcess {
    fn dial(&mut self, token: &str) -> Result<Box<dyn Conn>, String> {
        let who = (self.authenticate)(token).ok_or("not signed in")?;
        let ((to_client, rx), (tx, from_client)) = (channel(), channel::<Vec<u8>>());
        let c = self.hub.connect(who, Box::new(move |b| to_client.send(b).is_ok()));
        let hub = self.hub.clone();
        thread::spawn(move || {
            for f in from_client {
                hub.frame(c, f);
            }
            hub.close(c);
        });
        Ok(Box::new(End { tx, rx }))
    }
}
