//! The one place networking lives: a transport that moves binary frames,
//! and a [`Link`] around it that dials, notices a drop, and dials again
//! with a backoff (half a second, doubling to thirty). The engine is
//! sans-io, so everything here happens on [`Link::poll`], which the peer's
//! `pump` calls.
//!
//! **Nothing here runs on a clock of the host's** (`docs/plan-reactive.md`).
//! A transport given a [`Wake`] calls it whenever it has queued something
//! to report — the socket opened, a frame, the socket closed — and the host
//! answers by pumping once; the redial after a close is a deadline,
//! [`Link::deadline`], which the host arms one timer for and which an open
//! clears. Connected and idle, a host has no timer and is never woken.
//!
//! Two transports ship: [`native`] — a WebSocket on a thread of its own,
//! blocking `tungstenite`, `ws://` and `wss://` — and [`web`], the
//! browser's `WebSocket`. A test can supply its own through [`Dial`], and
//! `ark-server` supplies one that reaches a hub in the same process.
//!
//! **Keepalive** is the server's: it pings every twenty seconds and a
//! browser answers without being asked, which is traffic both ways through
//! every proxy between them. The native transport pings too, and gives the
//! connection up when nothing at all has arrived for three of its ping
//! intervals — the one state no read ever reports, a socket the OS thinks
//! is open to a peer that is not there.

#[cfg(not(target_arch = "wasm32"))]
pub mod native;
#[cfg(target_arch = "wasm32")]
pub mod web;

use crate::autos::monotonic_ms;

/// What a transport reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// The socket is open: the peer says `Hello`.
    Opened,
    /// A binary frame arrived.
    Frame(Vec<u8>),
    /// The socket is gone, or never opened, and why.
    Closed(String),
}

/// What a transport calls when it has queued an [`Event`]: the host's
/// signal to pump (`docs/plan-reactive.md`). Called from the transport's own
/// thread natively, and from the `WebSocket`'s callbacks in a browser; it
/// must not pump itself, only ask for one. A burst of calls is one wake for
/// whoever coalesces them — the peer does ([`crate::Peer::set_waker`]).
#[cfg(not(target_arch = "wasm32"))]
pub type Wake = std::sync::Arc<dyn Fn() + Send + Sync>;
#[cfg(target_arch = "wasm32")]
pub type Wake = std::rc::Rc<dyn Fn()>;

/// One connection's worth of socket. Made by a [`Dial`], polled by a
/// [`Link`], dropped when it closes.
pub trait Transport {
    /// Queue a binary frame. Only called after `Opened`.
    fn send(&mut self, frame: Vec<u8>);
    /// Everything that happened since the last poll, oldest first. Never
    /// blocks.
    fn poll(&mut self) -> Vec<Event>;
    /// Close it; nothing more is reported.
    fn close(&mut self);
    /// From now on, call `wake` after queueing every event — and once now
    /// if one may already be waiting, since the link hands this over after
    /// the dial that may have started the socket. The default ignores it: a
    /// transport polled by hand, as `ark-server`'s in-process one is, is
    /// only drained by a host that pumps on its own.
    fn set_waker(&mut self, wake: Wake) {
        let _ = wake;
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub type BoxTransport = Box<dyn Transport + Send>;
#[cfg(target_arch = "wasm32")]
pub type BoxTransport = Box<dyn Transport>;

/// How a link opens a transport to a URL.
#[cfg(not(target_arch = "wasm32"))]
pub type Dial = Box<dyn FnMut(&str) -> BoxTransport + Send>;
#[cfg(target_arch = "wasm32")]
pub type Dial = Box<dyn FnMut(&str) -> BoxTransport>;

/// The platform's WebSocket: [`native::dial`] or [`web::dial`].
pub fn platform_dial(cfg: &Timing) -> Dial {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let cfg = cfg.clone();
        Box::new(move |url: &str| native::dial(url, &cfg))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = cfg;
        Box::new(|url: &str| web::dial(url))
    }
}

/// The numbers a link runs on. The defaults are what a deployment wants; a
/// test shortens them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Timing {
    pub first_backoff_ms: u64,
    pub max_backoff_ms: u64,
    /// How often the native transport pings; it gives up after three of
    /// these with nothing received.
    pub ping_every_ms: u64,
    /// How long the native transport waits for a TCP connection, and then
    /// for the answer to its WebSocket handshake (`docs/plan-perf.md` R6).
    pub connect_timeout_ms: u64,
}

impl Default for Timing {
    fn default() -> Timing {
        Timing {
            first_backoff_ms: 500,
            max_backoff_ms: 30_000,
            ping_every_ms: 20_000,
            connect_timeout_ms: 5_000,
        }
    }
}

/// Where a link is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Not asked to connect, or told to stop (offline, or denied).
    Idle,
    /// Dialled, waiting for the socket to open.
    Connecting,
    /// Open; the peer is linked.
    Open,
    /// Down; the next attempt is at this monotonic millisecond.
    Waiting(u64),
}

/// A reconnecting connection to one URL.
pub struct Link {
    url: String,
    dial: Dial,
    timing: Timing,
    transport: Option<BoxTransport>,
    state: State,
    backoff: u64,
    /// Handed to every transport this link dials.
    waker: Option<Wake>,
    /// Connections opened so far.
    pub opens: u64,
    /// Why the last connection ended, or the last attempt failed.
    pub last_close: Option<String>,
}

impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Link")
            .field("url", &self.url)
            .field("state", &self.state)
            .field("opens", &self.opens)
            .finish()
    }
}

/// What one poll of a link came to, for the peer to hand the engine.
#[derive(Debug, Default)]
pub struct Polled {
    /// The socket opened: say `connected()`.
    pub opened: bool,
    pub frames: Vec<Vec<u8>>,
    /// The socket that was open closed: say `disconnected()`.
    pub closed: Option<String>,
}

impl Link {
    /// A link to `url` that dials on the next poll.
    pub fn new(url: &str, dial: Dial, timing: Timing) -> Link {
        Link {
            url: url.into(),
            dial,
            backoff: timing.first_backoff_ms,
            timing,
            transport: None,
            state: State::Waiting(0),
            waker: None,
            opens: 0,
            last_close: None,
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn is_open(&self) -> bool {
        self.state == State::Open
    }

    /// When this link next wants a [`Link::poll`] with nothing having woken
    /// it: the redial after a close, as a monotonic millisecond — `0` is
    /// now. `None` while it is dialling, open or stopped, which is when the
    /// transport's own wakes are all there is (`docs/plan-reactive.md`).
    /// The host arms one timer for it; the value does not move until the
    /// link does, so a host keyed by it re-arms nothing.
    pub fn deadline(&self) -> Option<u64> {
        match self.state {
            State::Waiting(at) => Some(at),
            _ => None,
        }
    }

    /// Wake the host when the socket has something to report: handed to
    /// the transport open now, and to every one dialled after.
    pub fn set_waker(&mut self, wake: Wake) {
        if let Some(t) = &mut self.transport {
            t.set_waker(wake.clone());
        }
        self.waker = Some(wake);
    }

    /// Stop, and stay stopped until [`Link::resume`]. Reports whether a
    /// socket that was open went.
    pub fn stop(&mut self, why: &str) -> bool {
        let was = self.state == State::Open;
        if let Some(mut t) = self.transport.take() {
            t.close();
        }
        self.state = State::Idle;
        self.backoff = self.timing.first_backoff_ms;
        self.last_close = Some(why.into());
        was
    }

    /// Dial again on the next poll, after [`Link::stop`].
    pub fn resume(&mut self) {
        if self.state == State::Idle {
            self.state = State::Waiting(0);
        }
    }

    /// Queue a frame on the open socket; dropped otherwise, which is right:
    /// the engine re-offers everything durable on the next `Hello`.
    pub fn send(&mut self, frame: Vec<u8>) {
        if self.state == State::Open {
            if let Some(t) = &mut self.transport {
                t.send(frame);
            }
        }
    }

    /// One turn: dial if due, and collect what the socket did.
    pub fn poll(&mut self) -> Polled {
        self.poll_at(monotonic_ms())
    }

    /// [`Link::poll`] at a given time, so the backoff is testable.
    pub fn poll_at(&mut self, now: u64) -> Polled {
        let mut out = Polled::default();
        if let State::Waiting(at) = self.state {
            if now >= at {
                let mut t = (self.dial)(&self.url);
                if let Some(w) = &self.waker {
                    t.set_waker(w.clone());
                }
                self.transport = Some(t);
                self.state = State::Connecting;
            }
        }
        let Some(t) = &mut self.transport else { return out };
        for e in t.poll() {
            match e {
                Event::Opened => {
                    self.state = State::Open;
                    self.opens += 1;
                    self.backoff = self.timing.first_backoff_ms;
                    out.opened = true;
                    out.closed = None;
                }
                Event::Frame(f) if self.state == State::Open => out.frames.push(f),
                Event::Frame(_) => {}
                Event::Closed(why) => {
                    if self.state == State::Open {
                        out.closed = Some(why.clone());
                    }
                    if let Some(mut t) = self.transport.take() {
                        t.close();
                    }
                    self.last_close = Some(why);
                    self.state = State::Waiting(now + self.backoff);
                    self.backoff = (self.backoff * 2).min(self.timing.max_backoff_ms);
                    break;
                }
            }
        }
        out
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        if let Some(mut t) = self.transport.take() {
            t.close();
        }
    }
}

/// A transport over two queues, for a test or an in-process server: what
/// the link sends goes to `sent`, and what is pushed onto `events` is what
/// it reports — waking the link's host when it is pushed with
/// [`Queues::push`], as a socket's reader does.
#[derive(Clone, Default)]
pub struct Queues {
    pub sent: std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    pub events: std::sync::Arc<std::sync::Mutex<Vec<Event>>>,
    pub closed: std::sync::Arc<std::sync::atomic::AtomicBool>,
    #[allow(clippy::type_complexity)]
    pub waker: std::sync::Arc<std::sync::Mutex<Option<Wake>>>,
}

impl Queues {
    /// Report `e`, and wake whoever is listening — what a socket's reader
    /// does with a frame.
    pub fn push(&self, e: Event) {
        self.events.lock().unwrap_or_else(|e| e.into_inner()).push(e);
        let w = self.waker.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(w) = w {
            w();
        }
    }
}

impl Transport for Queues {
    fn send(&mut self, frame: Vec<u8>) {
        self.sent.lock().unwrap_or_else(|e| e.into_inner()).push(frame);
    }
    fn poll(&mut self) -> Vec<Event> {
        std::mem::take(&mut *self.events.lock().unwrap_or_else(|e| e.into_inner()))
    }
    fn close(&mut self) {
        self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    fn set_waker(&mut self, wake: Wake) {
        *self.waker.lock().unwrap_or_else(|e| e.into_inner()) = Some(wake.clone());
        if !self.events.lock().unwrap_or_else(|e| e.into_inner()).is_empty() {
            wake();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Every dial hands back a fresh `Queues` and records it.
    fn recording() -> (Dial, Arc<Mutex<Vec<Queues>>>) {
        let made: Arc<Mutex<Vec<Queues>>> = Arc::default();
        let m = made.clone();
        let dial: Dial = Box::new(move |_url: &str| {
            let q = Queues::default();
            m.lock().unwrap().push(q.clone());
            Box::new(q) as BoxTransport
        });
        (dial, made)
    }

    fn push(q: &Queues, e: Event) {
        q.events.lock().unwrap().push(e);
    }

    #[test]
    fn a_link_backs_off_doubling_and_resets_when_it_opens() {
        let (dial, made) = recording();
        let mut l = Link::new("ws://x/sync", dial, Timing::default());
        l.poll_at(0);
        assert_eq!(made.lock().unwrap().len(), 1, "dials at once");
        push(&made.lock().unwrap()[0], Event::Closed("refused".into()));
        let p = l.poll_at(0);
        assert_eq!(p.closed, None, "a socket that never opened is not a disconnect");
        assert_eq!(l.state(), &State::Waiting(500));
        l.poll_at(499);
        assert_eq!(made.lock().unwrap().len(), 1, "not before the backoff");
        l.poll_at(500);
        assert_eq!(made.lock().unwrap().len(), 2);
        push(&made.lock().unwrap()[1], Event::Closed("refused".into()));
        l.poll_at(500);
        assert_eq!(l.state(), &State::Waiting(1500), "doubled");
        l.poll_at(1500);
        let q = made.lock().unwrap()[2].clone();
        push(&q, Event::Opened);
        push(&q, Event::Frame(vec![1]));
        let p = l.poll_at(1500);
        assert!(p.opened);
        assert_eq!(p.frames, vec![vec![1]]);
        l.send(vec![9]);
        assert_eq!(*q.sent.lock().unwrap(), vec![vec![9]]);
        push(&q, Event::Closed("gone".into()));
        let p = l.poll_at(2000);
        assert_eq!(p.closed.as_deref(), Some("gone"));
        assert_eq!(l.state(), &State::Waiting(2500), "the backoff started over when it opened");
        assert!(!l.stop("offline"), "it was not open");
        l.poll_at(99_999);
        assert_eq!(made.lock().unwrap().len(), 3, "a stopped link does not dial");
        l.resume();
        l.poll_at(99_999);
        assert_eq!(made.lock().unwrap().len(), 4);
    }
}
