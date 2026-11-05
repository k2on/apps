//! The peer on a thread of its own (§1.7), a device's and a server's
//! alike: the machines are single-owner, and neither the thread an app
//! draws on nor a server's sockets wait on what the peer takes in — a
//! part of a fill, an entry's facts put, its views moved, the disk
//! written. One [`Handle`] is that thread.
//!
//! What is done with the peer is done on the [`Peer`] itself, held
//! ([`Handle::lock`], [`Handle::with`]): a mutate, a query, a view opened
//! or settled. The thread holds it for one pump at a time, so a call waits
//! for at most one; let go, the thread is rung, so what it said is sent.
//! The thread pumps whenever the connection rings, the peer's deadline
//! comes ([`Peer::deadline`]), the peer was let go or a server's
//! connection said something, and tells the app (its waker,
//! [`Handle::set_waker`]) when a pump moved what it can see
//! ([`Pumped::shown`]), so a library coming in part by part is taken in
//! without the app drawing again for each; the app then reads what moved
//! ([`Handle::pump`]). In a browser there is no thread: the pump is the
//! page's, one at a time between the page's own tasks ([`page`]).
//!
//! **A server's connections** ([`Handle::connect`]) are what its sockets
//! (`serve`) said, queued for the thread, and each one's [`Sink`], where
//! what the peer delivers to it goes; dropping a sink — the connection
//! closed, its session revoked or expired ([`Handle::watched`]) — closes
//! the connection. A device in this process dials it ([`Handle::dialer`]:
//! the transport the perf tests, the scanner and the phones' in-process
//! server use). Threads and channels only: no web server is linked. A
//! connection is who its token proves (with `ark-auth`, the session row
//! its signature names; with no sign-in configured the dev reading of
//! [`dev`]).
//!
//! **Work off the thread an app draws on is still work.** A pump that
//! takes longer than [`PUMP_BUDGET_MS`], and an app's wait for the peer
//! longer than [`WAIT_BUDGET_MS`], are said as events of the device's,
//! which go to its server (`crate::observe`): slow work is in the server's
//! events whichever thread did it.

use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex, MutexGuard};

use ark::peer::Ctx;

use crate::link::{Pumped, Wake};
use crate::Peer;

/// How long one pump may take, in ms, before it is said: past a frame,
/// what it did is work the device should not have had to do, wherever it
/// was done.
pub const PUMP_BUDGET_MS: u64 = 16;

/// How long an app may wait for the peer, in ms, before it is said: the
/// thread it draws on held up by the pump.
pub const WAIT_BUDGET_MS: u64 = 4;

/// How long one pump in a browser takes things in, in ms, before it leaves
/// the rest for the next: half a frame, the other half the page's.
pub const PAGE_BUDGET_MS: u64 = 8;

/// How many bytes of rows a page asks each part of a fill carry: what it
/// takes in, rows parsed, held and their views moved, in a millisecond or
/// two — a part being the least a pump takes in.
pub const PAGE_PART: usize = 16 << 10;

/// The dev reading of a token: `user@device` is `user` on a session named
/// by the whole token; `:role,role` after it, the roles.
pub fn dev(token: &str) -> Option<Ctx> {
    let (id, roles) = token.split_once(':').unwrap_or((token, ""));
    let actor = id.split_once('@').map_or(id, |(u, _)| u);
    let roles = roles.split(',').map(str::trim).filter(|r| !r.is_empty()).map(String::from).collect();
    (!actor.is_empty()).then(|| Ctx::who(actor, id, roles))
}

fn held<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

struct Inner {
    peer: Mutex<Peer>,
    /// What the pumps did since the app last asked.
    said: Mutex<Pumped>,
    /// The app's, rung when a pump moved anything.
    told: Mutex<Option<Wake>>,
    /// The thread's own: rung by the connection, by an app done with the
    /// peer, by a server's connections.
    ring: Wake,
    /// Pumped by the app ([`Handle::pump`]) from now on, and not by the
    /// thread: a test that says when what arrived is taken in.
    by_hand: AtomicBool,
    /// A server's connections natively; the page's timers in a browser.
    beside: Beside,
}

#[cfg(not(target_arch = "wasm32"))]
type Beside = serving::Serving;
#[cfg(target_arch = "wasm32")]
type Beside = page::Page;

impl Inner {
    fn lock(&self) -> MutexGuard<'_, Peer> {
        held(&self.peer)
    }

    /// A pump — and, serving, what its connections said taken in — and
    /// what it did kept for the app; said when it took longer than
    /// [`PUMP_BUDGET_MS`].
    fn pump(&self) -> Pumped {
        let mut peer = self.lock();
        let began = crate::monotonic_ms();
        let p = peer.pump();
        #[cfg(not(target_arch = "wasm32"))]
        self.serve(&mut peer);
        let took = crate::monotonic_ms() - began;
        if took > PUMP_BUDGET_MS {
            let (frames, backlog) = (p.frames[0] + p.frames[1], peer.backlog());
            tracing::warn!(target: "ark", ms = took, frames, backlog, "a slow pump");
        }
        drop(peer);
        absorb(&mut held(&self.said), &p);
        p
    }

    /// The app's waker rung.
    fn tell(&self) {
        let told = held(&self.told).clone();
        if let Some(w) = told {
            w();
        }
    }
}

/// `p` added to what was said before.
fn absorb(said: &mut Pumped, p: &Pumped) {
    said.opened |= p.opened;
    said.moved |= p.moved;
    said.frames[0] += p.frames[0];
    said.frames[1] += p.frames[1];
    said.closed.extend(p.closed.iter().cloned());
    if p.denied.is_some() {
        said.denied.clone_from(&p.denied);
    }
}

/// A [`Peer`] on a thread of its own (in a browser, the page's).
#[derive(Clone)]
pub struct Handle {
    inner: Arc<Inner>,
}

/// The peer, held: what [`Handle::lock`] gives, the [`Peer`] itself. Let
/// go, the thread is rung, so what the app had it say is sent.
pub struct Locked<'a> {
    guard: MutexGuard<'a, Peer>,
    /// Which peer, as this thread holds it ([`HELD`]).
    held: usize,
    inner: &'a Inner,
}

impl std::ops::Deref for Locked<'_> {
    type Target = Peer;
    fn deref(&self) -> &Peer {
        &self.guard
    }
}

impl std::ops::DerefMut for Locked<'_> {
    fn deref_mut(&mut self) -> &mut Peer {
        &mut self.guard
    }
}

impl Drop for Locked<'_> {
    fn drop(&mut self) {
        HELD.with(|h| h.borrow_mut().retain(|p| *p != self.held));
        // Pumped by hand, what the app had it say goes with its next pump.
        if !self.inner.by_hand.load(Relaxed) {
            (self.inner.ring)();
        }
    }
}

thread_local! {
    /// The peers this thread holds. One held again on the same thread
    /// would wait on itself for ever: a guard kept alive past where it was
    /// meant to be let go (a `match` on a call through it, two in one
    /// `format!`), which a debug build says rather than hangs on.
    static HELD: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) };
}

impl Handle {
    /// `peer` on a thread of its own from now on: pumped, and — serving —
    /// its connections taken in. The thread ends when every handle is
    /// dropped, a server writing its volatile space for the next open. In
    /// a browser, which has no thread, the page pumps it ([`page`]): a pump
    /// takes in what it can in [`PAGE_BUDGET_MS`] and leaves the rest for
    /// the next, so the page draws between them, and a fill comes in parts
    /// of [`PAGE_PART`] bytes, each well inside one pump.
    pub fn spawn(mut peer: Peer) -> Handle {
        #[cfg(target_arch = "wasm32")]
        ((peer.link.budget, peer.log.machine.part) = (Some(PAGE_BUDGET_MS), Some(PAGE_PART)));
        #[cfg(not(target_arch = "wasm32"))]
        let mut thread = None;
        let inner = Arc::new_cyclic(|me| {
            #[cfg(not(target_arch = "wasm32"))]
            let ring = {
                let (ring, rx) = crate::link::bell();
                thread = Some((me.clone(), rx));
                ring
            };
            #[cfg(target_arch = "wasm32")]
            let ring = page::ring(me.clone());
            peer.set_waker(ring.clone());
            let (said, told, beside) = Default::default();
            let (peer, by_hand) = (Mutex::new(peer), Default::default());
            Inner {
                peer,
                said,
                told,
                ring,
                by_hand,
                beside,
            }
        });
        #[cfg(not(target_arch = "wasm32"))]
        if let Some((weak, rx)) = thread {
            let named = std::thread::Builder::new().name("ark-peer".into());
            named.spawn(move || drive(weak, rx)).expect("a thread for the peer");
        }
        Handle { inner }
    }

    /// The peer, held until the guard is let go: the thread waits for it,
    /// and it for the thread's pump — said when that was longer than
    /// [`WAIT_BUDGET_MS`].
    pub fn lock(&self) -> Locked<'_> {
        let held = Arc::as_ptr(&self.inner) as usize;
        HELD.with(|h| {
            let mut h = h.borrow_mut();
            debug_assert!(!h.contains(&held), "the peer is held already on this thread: a guard kept too long");
            h.push(held);
        });
        let asked = crate::monotonic_ms();
        let guard = self.inner.lock();
        let waited = crate::monotonic_ms() - asked;
        if waited > WAIT_BUDGET_MS {
            tracing::warn!(target: "ark", ms = waited, "the app waited for the peer");
        }
        Locked {
            guard,
            held,
            inner: &self.inner,
        }
    }

    /// `w` rung whenever a pump of the thread's moved what the app can see
    /// ([`Pumped::shown`]); pumped by hand, whenever the connection rang.
    pub fn set_waker(&self, w: Wake) {
        *held(&self.inner.told) = Some(w);
    }

    /// When the app must pump: pumped by hand, at the peer's deadline;
    /// otherwise never — the thread keeps the peer's deadlines itself.
    pub fn deadline(&self) -> Option<u64> {
        match self.inner.by_hand.load(Relaxed) {
            true => self.inner.lock().deadline(),
            false => None,
        }
    }

    /// Pumped by [`Handle::pump`] from now on, never by the thread.
    pub fn by_hand(&self) {
        self.inner.by_hand.store(true, Relaxed);
    }

    /// What was taken in since the app last asked: by the thread, or —
    /// pumped by hand — by a pump now.
    pub fn pump(&self) -> Pumped {
        if self.inner.by_hand.load(Relaxed) {
            self.inner.pump();
        }
        std::mem::take(&mut *held(&self.inner.said))
    }
}

/// The thread: a pump whenever the connection, the app or a connection
/// rings or the peer's deadline comes — at least every second, a server's
/// watch's clock — the app told when it moved anything; until the peer is
/// gone or stopped.
#[cfg(not(target_arch = "wasm32"))]
fn drive(inner: std::sync::Weak<Inner>, rx: std::sync::mpsc::Receiver<()>) {
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Duration;
    loop {
        let wait = match inner.upgrade() {
            Some(i) => {
                let now = crate::monotonic_ms();
                let due = i.lock().deadline().map_or(1000, |d| d.saturating_sub(now));
                Duration::from_millis(due.min(1000))
            }
            None => return,
        };
        let rang = match rx.recv_timeout(wait) {
            Err(RecvTimeoutError::Disconnected) => return,
            r => r.is_ok(),
        };
        while rx.try_recv().is_ok() {}
        let Some(i) = inner.upgrade() else { return };
        if i.beside.stopped.load(Relaxed) {
            return;
        }
        // Pumped by hand, the app is told what rang, and pumps itself; its
        // own clock is its deadlines.
        let by_hand = i.by_hand.load(Relaxed);
        if by_hand && rang || !by_hand && i.pump().shown {
            i.tell();
        }
    }
}

/// A server's half: its connections, queued for the thread, and its
/// in-process transport.
#[cfg(not(target_arch = "wasm32"))]
mod serving {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
    use std::sync::mpsc::{channel, Receiver, TryRecvError};
    use std::sync::{Arc, Mutex};

    use ark::log::Seq;
    use ark::peer::Ctx;
    use ark_auth::server::Watch;

    use super::{held, Handle, Inner};
    use crate::link::{Closed, Dial, Event, Wake, Wire};
    use crate::{now_ms, ConnId, Deliver, Peer};

    /// Where a connection's frames go; false once it is gone.
    pub type Sink = Box<dyn Fn(Vec<u8>) -> bool + Send>;

    /// Who a token proves, or nobody.
    pub use ark_auth::server::Authenticate;

    type Job = Box<dyn FnOnce(&mut Peer, &mut BTreeMap<ConnId, Sink>) -> Vec<Deliver> + Send>;

    /// What a server's thread keeps beside its peer: what its connections
    /// said, in order, not yet taken in; where each one's frames go; the
    /// next one's number; its watch and when it last looked; and whether it
    /// stopped.
    #[derive(Default)]
    pub(super) struct Serving {
        jobs: Arc<Mutex<Vec<Job>>>,
        sinks: Mutex<BTreeMap<ConnId, Sink>>,
        next: AtomicU64,
        watch: Mutex<(Option<Watch>, i64)>,
        pub stopped: AtomicBool,
    }

    impl Inner {
        /// What the connections said, taken in, what is written delivered,
        /// the calls made run on threads of their own, and the watch told
        /// what changed and, each second, the clock: the sessions it names
        /// have their connections closed.
        pub(super) fn serve(&self, peer: &mut Peer) {
            let s = &self.beside;
            if !peer.hub.serving || s.stopped.load(Relaxed) {
                return;
            }
            let jobs = std::mem::take(&mut *held(&s.jobs));
            let mut sinks = held(&s.sinks);
            let mut out: Vec<Deliver> = jobs.into_iter().flat_map(|job| job(peer, &mut sinks)).collect();
            out.extend(peer.release());
            for (call, f) in peer.take_calls() {
                let (c, id) = (call.conn, call.id);
                let Some(f) = f else {
                    out.extend(peer.reply(c, id, Err(format!("this server has no function {}", call.name))));
                    continue;
                };
                // Its own thread: a call may take its time (another
                // service's sign-in) and the log does not wait for it.
                let (jobs, ring) = (Arc::downgrade(&s.jobs), self.ring.clone());
                std::thread::spawn(move || {
                    let answer = f(&call.who, &call.input);
                    if let Some(jobs) = jobs.upgrade() {
                        held(&jobs).push(Box::new(move |p, _| p.reply(c, id, answer)));
                        ring();
                    }
                });
            }
            deliver(&mut sinks, out);
            let mut watch = held(&s.watch);
            let (Some(w), ticked) = &mut *watch else { return };
            // Delivered first: a sign-out's own confirmation, then the close.
            let (now, seen) = (now_ms(), std::mem::take(&mut peer.hub.seen));
            let tick = now - *ticked >= 1000;
            if !tick && seen.is_empty() {
                return;
            }
            *ticked = if tick { now } else { *ticked };
            let open: Vec<Ctx> = peer.connections().map(|(_, w)| w.clone()).collect();
            for session in w(&peer.log.store, (!tick).then_some(&seen[..]), &open, now) {
                let out = close(peer, &mut sinks, &session);
                deliver(&mut sinks, out);
            }
        }
    }

    impl Drop for Inner {
        fn drop(&mut self) {
            let peer = self.peer.get_mut().unwrap_or_else(|e| e.into_inner());
            if peer.hub.serving && !self.beside.stopped.swap(true, Relaxed) {
                peer.close();
            }
        }
    }

    impl Handle {
        /// [`Handle::spawn`] of a server, with `watch` told what every
        /// entry changed and, each second, the clock; the sessions it names
        /// have their connections closed (`ark_auth::server::Auth::watcher`).
        pub fn watched(mut peer: Peer, watch: Option<Watch>) -> Handle {
            peer.hub.watched = watch.is_some();
            let h = Handle::spawn(peer);
            held(&h.inner.beside.watch).0 = watch;
            h
        }

        /// `job` for the thread, in order after what came before.
        fn run(&self, job: impl FnOnce(&mut Peer, &mut BTreeMap<ConnId, Sink>) -> Vec<Deliver> + Send + 'static) {
            held(&self.inner.beside.jobs).push(Box::new(job));
            (self.inner.ring)();
        }

        /// A connection verified as `who`, its frames to `sink`.
        pub fn connect(&self, who: Ctx, sink: Sink) -> ConnId {
            // Above any `Peer::accept`'s own.
            let c = (1 << 32) + self.inner.beside.next.fetch_add(1, Relaxed);
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

        /// What came before taken in, the volatile space written and the
        /// thread ended: what a server stopping does before its process
        /// exits, which would otherwise cut that write short. Every handle
        /// is inert after.
        pub fn stop(&self) {
            let mut peer = self.inner.lock();
            self.inner.serve(&mut peer);
            if !self.inner.beside.stopped.swap(true, Relaxed) {
                peer.close();
            }
            drop(peer);
            (self.inner.ring)();
        }

        /// `f` on the peer, after what came before, and its answer; none
        /// once stopped.
        pub fn with<T>(&self, f: impl FnOnce(&mut Peer) -> T) -> Option<T> {
            let mut peer = self.inner.lock();
            self.inner.serve(&mut peer);
            if self.inner.beside.stopped.load(Relaxed) {
                return None;
            }
            let t = f(&mut peer);
            self.inner.serve(&mut peer);
            Some(t)
        }

        /// An entry the authority authors itself ([`Peer::mutate`]): the head
        /// of the space it is on, sequenced, or the refusal.
        pub fn mutate(&self, name: String, input: Vec<u8>) -> Option<Result<Seq, String>> {
            self.with(move |p| {
                let space = p.function(&name)?.space;
                p.mutate(&name, input)?;
                Ok(p.machine(space).map_or(0, |m| m.log().head().0))
            })
        }

        /// Facts the authority writes raw ([`Peer::write`]): the head they
        /// were sequenced at, or the refusal.
        pub fn write(&self, facts: Vec<ark::store::Change>) -> Option<Result<Seq, String>> {
            self.with(move |p| p.write(facts, now_ms()).map(|_| p.log.log().head().0))
        }

        /// The in-process transport: a device's link dialling this server in
        /// this process, its token resolved by `authenticate`; a frame goes from
        /// the caller's thread straight to the server's queue.
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
}

#[cfg(not(target_arch = "wasm32"))]
pub use serving::{Authenticate, Sink};

/// The pump in a browser: a run as soon as the page is free whenever the
/// connection or the app rings and while what arrived is not all taken in,
/// one task each so the page draws between them, and a timer for the
/// peer's next deadline — the app told when a pump moved what it can see.
#[cfg(target_arch = "wasm32")]
mod page {
    use std::sync::atomic::Ordering;
    use std::sync::Weak;

    use wasm_bindgen::JsCast;

    use super::{held, Inner};

    /// A run of the page's pump asked for and not yet made, and the
    /// deadline the page has a timer set for.
    #[derive(Default)]
    pub struct Page {
        asked: std::sync::atomic::AtomicBool,
        armed: std::sync::Mutex<Option<u64>>,
    }

    /// The page's bell: a run as soon as the page is free.
    pub fn ring(inner: Weak<Inner>) -> crate::link::Wake {
        std::rc::Rc::new(move || soon(&inner))
    }

    fn soon(inner: &Weak<Inner>) {
        let Some(i) = inner.upgrade() else { return };
        if !i.beside.asked.swap(true, Ordering::Relaxed) {
            after(inner.clone(), 0);
        }
    }

    /// A task of the page's, in `ms`.
    fn after(inner: Weak<Inner>, ms: u64) {
        let run = wasm_bindgen::closure::Closure::once_into_js(move || drive(&inner));
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(run.unchecked_ref(), ms.min(i32::MAX as u64) as i32);
        }
    }

    fn drive(inner: &Weak<Inner>) {
        let Some(i) = inner.upgrade() else { return };
        i.beside.asked.store(false, Ordering::Relaxed);
        if i.by_hand.load(Ordering::Relaxed) {
            return i.tell();
        }
        if i.pump().shown {
            i.tell();
        }
        let (now, due) = (crate::monotonic_ms(), i.lock().deadline());
        match due {
            Some(d) if d <= now => soon(inner),
            Some(d) => {
                let mut armed = held(&i.beside.armed);
                if *armed != Some(d) {
                    *armed = Some(d);
                    after(inner.clone(), d - now);
                }
            }
            None => {}
        }
    }
}
