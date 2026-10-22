//! The peer on a thread of its own (§1.7): the machines are single-owner
//! and a disk write must not stall a socket, so one thread owns the
//! [`Peer`] and everything talks to it through a [`Handle`] — a socket's
//! task (`serve`), a device in this process ([`Handle::dialer`], the
//! transport the perf tests, the scanner and the phones' in-process server
//! use), the sign-in routes writing a session. What the peer delivers goes
//! to each connection's [`Sink`]; dropping a sink — the connection closed,
//! its session revoked or expired (`Handle::watched`) — closes the
//! connection. Threads and channels only: no web server is linked.
//!
//! A connection is who its token proves (with `ark-auth`, the session row
//! its signature names; with no sign-in configured the dev reading of
//! [`dev`]).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

use ark::log::Seq;
use ark::peer::Who;
use ark::schema::Space;
use ark_auth::server::Watch;

use crate::link::{Closed, Dial, Event, Wake, Wire};
use crate::{now_ms, ConnId, Deliver, Peer};

/// Where a connection's frames go; false once it is gone.
pub type Sink = Box<dyn Fn(Vec<u8>) -> bool + Send>;

/// Who a token proves, or nobody.
pub use ark_auth::server::Authenticate;

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
    pub fn spawn(peer: Peer) -> Handle {
        Handle::watched(peer, None)
    }

    /// [`Handle::spawn`], with `watch` told what every job's entries
    /// changed and, each second, the clock; the sessions it names have
    /// their connections closed (`ark_auth::server::Auth::watcher`).
    pub fn watched(mut peer: Peer, mut watch: Option<Watch>) -> Handle {
        let (tx, rx) = channel::<Cmd>();
        peer.hub.watched = watch.is_some();
        std::thread::spawn(move || {
            let mut sinks = BTreeMap::new();
            let mut ticked = now_ms();
            loop {
                let mut out = match rx.recv_timeout(Duration::from_secs(1)) {
                    Ok(Cmd::Run(job)) => job(&mut peer, &mut sinks),
                    Ok(Cmd::Stop(done)) => {
                        peer.close();
                        let _ = done.send(());
                        return;
                    }
                    Err(RecvTimeoutError::Timeout) => vec![],
                    Err(RecvTimeoutError::Disconnected) => break,
                };
                out.extend(peer.release());
                deliver(&mut sinks, out);
                let Some(w) = watch.as_mut() else { continue };
                // Delivered first: a sign-out's own confirmation, then the close.
                let (now, seen) = (now_ms(), std::mem::take(&mut peer.hub.seen));
                let tick = now - ticked >= 1000;
                if !tick && seen.is_empty() {
                    continue;
                }
                ticked = if tick { now } else { ticked };
                let open: Vec<Who> = peer.connections().map(|(_, w)| w.clone()).collect();
                for session in w(&peer.log.store, (!tick).then_some(&seen[..]), &open, now) {
                    let out = close(&mut peer, &mut sinks, &session);
                    deliver(&mut sinks, out);
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

    /// Every connection of a session closed.
    pub fn revoke(&self, session: &str) {
        let session = session.to_string();
        self.run(move |p, s| close(p, s, &session));
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
    pub fn author(&self, space: Space, func: String, input: Vec<u8>) -> Option<Result<Seq, String>> {
        self.sequenced(space, move |p| p.author(space, &func, input, now_ms()))
    }

    /// Facts the authority writes raw ([`Peer::write`]): as `author`.
    pub fn write(&self, facts: Vec<ark::store::Change>) -> Option<Result<Seq, String>> {
        self.sequenced(Space::Durable, move |p| p.write(facts, now_ms()))
    }

    fn sequenced(&self, space: Space, f: impl FnOnce(&mut Peer) -> Result<(), String> + Send + 'static) -> Option<Result<Seq, String>> {
        self.with(move |p| f(p).map(|_| p.machine(space).map_or(0, |m| m.log().head().0)))
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

fn deliver(sinks: &mut BTreeMap<ConnId, Sink>, out: Vec<Deliver>) {
    for (c, b) in out {
        if sinks.get(&c).is_some_and(|s: &Sink| !s(b)) {
            sinks.remove(&c);
        }
    }
}

/// Every connection of a session closed: its sinks dropped, and left.
fn close(p: &mut Peer, sinks: &mut BTreeMap<ConnId, Sink>, session: &str) -> Vec<Deliver> {
    let mut out = vec![];
    for c in p.of_session(session) {
        sinks.remove(&c);
        out.extend(p.leave(c, now_ms()));
    }
    out
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
