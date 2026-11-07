//! A peer on a thread of its own (§1.7), a device's or a server's: the
//! machines are single-owner, and neither the thread an app draws on nor a
//! server's sockets wait on what the peer takes in — a part of a fill, an
//! entry's facts put, its views moved, the disk written. One [`Handle`] is
//! that thread, over whatever it drives ([`Driven`]): one loop, which
//! turns whenever it is rung or its peer's deadline comes.
//!
//! What is done with the peer is done on the peer itself, held
//! ([`Handle::lock`], [`Handle::with`]): a mutate, a query, a view opened
//! or settled. The thread holds it for one turn at a time, so a call waits
//! for at most one; let go, the thread is rung, so what it said is sent.
//!
//! **A device's** thread pumps it ([`Peer::pump`]) whenever the connection
//! rings, its deadline comes ([`Peer::deadline`]) or it was let go, and
//! tells the app (its waker, [`Handle::set_waker`]) when a pump moved what
//! it can see ([`Pumped::shown`]), so a library coming in part by part is
//! taken in without the app drawing again for each; the app then reads
//! what moved ([`Handle::pump`]). In a browser there is no thread: the
//! pump is the page's, one at a time between the page's own tasks
//! ([`page`]).
//!
//! **A server's** connections ([`Handle::connect`]) are what its sockets
//! (`serve`) said, queued for its thread, each one's frames going to its
//! [`crate::Sink`]; dropping a sink — the connection closed, its session
//! revoked or expired ([`Handle::watched`]) — closes the connection. A
//! device in this process dials it ([`Handle::dialer`]: the transport the
//! perf tests, the scanner and the phones' in-process server use). Threads
//! and channels only: no web server is linked. A connection is who its
//! token proves (with `ark-auth`, the session row its signature names;
//! with no sign-in configured the dev reading of [`dev`]). A server has no
//! handlers: its work is a device of its own, dialling it.
//!
//! **Work off the thread an app draws on is still work.** A pump that
//! takes longer than [`PUMP_BUDGET_MS`], and an app's wait for the peer
//! longer than [`WAIT_BUDGET_MS`], are said as events of the device's,
//! which go to its server (`crate::observe`): slow work is in the server's
//! events whichever thread did it.

use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex, MutexGuard};

use ark::peer::Ctx;

use crate::link::{Pumped, Sendable, Wake};
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

/// What a [`Handle`]'s thread drives: a device ([`Peer`]) or a server
/// ([`crate::Authority`]).
pub trait Driven: Sendable + Sized + 'static {
    /// What its handle keeps beside it, off its lock: a server's queue of
    /// what its connections said.
    type Beside: Default + Send + Sync;
    /// Its handle made: the bell that rings its thread.
    fn spawned(&mut self, _ring: &Wake) {}
    /// When its thread is next to turn, whatever rings.
    fn deadline(&self) -> Option<u64> {
        None
    }
    /// One turn of its thread's, rung or due: whether the thread goes on.
    fn turn(h: &Handle<Self>, rang: bool) -> bool;
    /// Its last handle gone, or stopped.
    fn end(&mut self) {}
}

struct Inner<P: Driven> {
    peer: Mutex<P>,
    /// What a device's pumps did since the app last asked.
    said: Mutex<Pumped>,
    /// The app's, rung when a pump moved anything.
    told: Mutex<Option<Wake>>,
    /// The thread's own: rung by the connection, by an app done with the
    /// peer, by a server's connections.
    ring: Wake,
    /// Pumped by the app ([`Handle::pump`]) from now on, and not by the
    /// thread: a test that says when what arrived is taken in.
    by_hand: AtomicBool,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    beside: P::Beside,
    /// In a browser, the page's timers.
    #[cfg(target_arch = "wasm32")]
    page: page::Page,
}

impl<P: Driven> Inner<P> {
    fn lock(&self) -> MutexGuard<'_, P> {
        held(&self.peer)
    }

    /// The app's waker rung.
    fn tell(&self) {
        let told = held(&self.told).clone();
        if let Some(w) = told {
            w();
        }
    }
}

impl<P: Driven> Drop for Inner<P> {
    fn drop(&mut self) {
        self.peer.get_mut().unwrap_or_else(|e| e.into_inner()).end();
    }
}

impl Inner<Peer> {
    /// A pump, and what it did kept for the app; said when it took longer
    /// than [`PUMP_BUDGET_MS`].
    fn pump(&self) -> Pumped {
        let mut peer = self.lock();
        let began = crate::monotonic_ms();
        let p = peer.pump();
        let took = crate::monotonic_ms() - began;
        if took > PUMP_BUDGET_MS {
            let (frames, backlog) = (p.frames[0] + p.frames[1], peer.backlog());
            tracing::warn!(target: "ark", ms = took, frames, backlog, "a slow pump");
        }
        drop(peer);
        absorb(&mut held(&self.said), &p);
        p
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

impl Driven for Peer {
    type Beside = ();

    fn spawned(&mut self, ring: &Wake) {
        #[cfg(target_arch = "wasm32")]
        ((self.link.budget, self.log.machine.part) = (Some(PAGE_BUDGET_MS), Some(PAGE_PART)));
        self.set_waker(ring.clone());
    }

    fn deadline(&self) -> Option<u64> {
        Peer::deadline(self)
    }

    /// Pumped by hand, the app is told what rang, and pumps itself; its own
    /// clock is its deadlines.
    fn turn(h: &Handle<Peer>, rang: bool) -> bool {
        let by_hand = h.inner.by_hand.load(Relaxed);
        if by_hand && rang || !by_hand && h.inner.pump().shown {
            h.inner.tell();
        }
        true
    }
}

/// A peer on a thread of its own (in a browser, the page's): a device's
/// ([`Peer`]), or a server's ([`crate::Authority`]).
pub struct Handle<P: Driven = Peer> {
    inner: Arc<Inner<P>>,
}

impl<P: Driven> Clone for Handle<P> {
    fn clone(&self) -> Self {
        Handle { inner: self.inner.clone() }
    }
}

/// The peer, held: what [`Handle::lock`] gives, the peer itself. Let go,
/// the thread is rung, so what the app had it say is sent.
pub struct Locked<'a, P: Driven = Peer> {
    guard: MutexGuard<'a, P>,
    /// Which peer, as this thread holds it ([`HELD`]).
    held: usize,
    inner: &'a Inner<P>,
}

impl<P: Driven> std::ops::Deref for Locked<'_, P> {
    type Target = P;
    fn deref(&self) -> &P {
        &self.guard
    }
}

impl<P: Driven> std::ops::DerefMut for Locked<'_, P> {
    fn deref_mut(&mut self) -> &mut P {
        &mut self.guard
    }
}

impl<P: Driven> Drop for Locked<'_, P> {
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

impl<P: Driven> Handle<P> {
    /// `peer` on a thread of its own from now on: turned whenever it is
    /// rung or due. The thread ends when every handle is dropped, a server
    /// writing its volatile space for the next open. In a browser, which
    /// has no thread, the page pumps a device ([`page`]): a pump takes in
    /// what it can in [`PAGE_BUDGET_MS`] and leaves the rest for the next,
    /// so the page draws between them, and a fill comes in parts of
    /// [`PAGE_PART`] bytes, each well inside one pump.
    pub fn spawn(mut peer: P) -> Handle<P> {
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
            peer.spawned(&ring);
            let (said, told, beside) = Default::default();
            let (peer, by_hand) = (Mutex::new(peer), Default::default());
            Inner {
                peer,
                said,
                told,
                ring,
                by_hand,
                beside,
                #[cfg(target_arch = "wasm32")]
                page: Default::default(),
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
    /// and it for the thread's turn — said when that was longer than
    /// [`WAIT_BUDGET_MS`].
    pub fn lock(&self) -> Locked<'_, P> {
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
}

impl Handle<Peer> {
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

/// The thread: a turn whenever it is rung or its peer's deadline comes —
/// at least every second, a server's watch's clock; until the peer is gone
/// or stopped.
#[cfg(not(target_arch = "wasm32"))]
fn drive<P: Driven>(inner: std::sync::Weak<Inner<P>>, rx: std::sync::mpsc::Receiver<()>) {
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
        if !P::turn(&Handle { inner: i }, rang) {
            return;
        }
    }
}

/// A server's half: its connections, queued for the thread, its watch,
/// and its in-process transport.
#[cfg(not(target_arch = "wasm32"))]
mod serving {
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
    use std::sync::mpsc::{channel, Receiver, TryRecvError};
    use std::sync::{Arc, Mutex};

    use ark::log::Seq;
    use ark::peer::Ctx;
    use ark_auth::server::Watch;

    use super::{held, Driven, Handle};
    use crate::link::{Closed, Dial, Event, Wake, Wire};
    use crate::{now_ms, Authority, ConnId, Deliver, Sink};

    /// Who a token proves, or nobody.
    pub use ark_auth::server::Authenticate;

    type Job = Box<dyn FnOnce(&mut Authority) -> Vec<Deliver> + Send>;

    /// What a server's handle keeps beside it: what its connections said,
    /// in order, not yet taken in; the next one's number; and its watch
    /// and when it last looked.
    #[derive(Default)]
    pub struct Queue {
        jobs: Arc<Mutex<Vec<Job>>>,
        next: AtomicU64,
        watch: Mutex<(Option<Watch>, i64)>,
    }

    impl Authority {
        /// Each frame given to its connection's sink; a connection whose
        /// sink is gone, gone.
        fn deliver(&mut self, out: Vec<Deliver>) {
            for (c, b) in out {
                if self.sinks.get(&c).is_some_and(|s| !s(b)) {
                    self.sinks.remove(&c);
                }
            }
        }
    }

    impl Driven for Authority {
        type Beside = Queue;

        fn turn(h: &Handle<Authority>, _: bool) -> bool {
            let mut p = h.inner.lock();
            h.serve(&mut p);
            !p.stopped
        }

        /// Its volatile space written for the next open, once.
        fn end(&mut self) {
            if !std::mem::replace(&mut self.stopped, true) {
                self.close();
            }
        }
    }

    impl Handle<Authority> {
        /// [`Handle::spawn`] of a server, with `watch` told what every
        /// entry changed and, each second, the clock; the sessions it names
        /// have their connections closed (`ark_auth::server::Auth::watcher`).
        pub fn watched(mut peer: Authority, watch: Option<Watch>) -> Handle<Authority> {
            peer.seen = watch.is_some().then(Vec::new);
            let h = Handle::spawn(peer);
            held(&h.inner.beside.watch).0 = watch;
            h
        }

        /// What the connections said, taken in, what is written delivered,
        /// the calls made run on threads of their own, and the watch told
        /// what changed and, each second, the clock: the sessions it names
        /// have their connections closed.
        fn serve(&self, p: &mut Authority) {
            if p.stopped {
                return;
            }
            let q = &self.inner.beside;
            let jobs = std::mem::take(&mut *held(&q.jobs));
            let mut out: Vec<Deliver> = jobs.into_iter().flat_map(|job| job(p)).collect();
            out.extend(p.release());
            for (call, f) in p.take_calls() {
                let (c, id) = (call.conn, call.id);
                let Some(f) = f else {
                    out.extend(p.reply(c, id, Err(format!("this server has no function {}", call.name))));
                    continue;
                };
                // Its own thread: a call may take its time (another
                // service's sign-in) and the log does not wait for it.
                let (jobs, ring) = (Arc::downgrade(&q.jobs), self.inner.ring.clone());
                std::thread::spawn(move || {
                    let answer = f(&call.who, &call.input);
                    if let Some(jobs) = jobs.upgrade() {
                        held(&jobs).push(Box::new(move |p| p.reply(c, id, answer)));
                        ring();
                    }
                });
            }
            p.deliver(out);
            let mut watch = held(&q.watch);
            let (Some(w), ticked) = &mut *watch else { return };
            // Delivered first: a sign-out's own confirmation, then the close.
            let (now, seen) = (now_ms(), p.seen.as_mut().map(std::mem::take).unwrap_or_default());
            let tick = now - *ticked >= 1000;
            if !tick && seen.is_empty() {
                return;
            }
            *ticked = if tick { now } else { *ticked };
            let open: Vec<Ctx> = p.connections().map(|(_, w)| w.clone()).collect();
            for session in w(&p.log.store, (!tick).then_some(&seen[..]), &open, now) {
                let out = p.revoke(&session, now);
                p.deliver(out);
            }
        }

        /// `job` for the thread, in order after what came before.
        fn run(&self, job: impl FnOnce(&mut Authority) -> Vec<Deliver> + Send + 'static) {
            held(&self.inner.beside.jobs).push(Box::new(job));
            (self.inner.ring)();
        }

        /// A connection verified as `who`, its frames to `sink`.
        pub fn connect(&self, who: Ctx, sink: Sink) -> ConnId {
            // Above any `Authority::accept`'s own.
            let c = (1 << 32) + self.inner.beside.next.fetch_add(1, Relaxed);
            self.run(move |p| {
                p.accept_as(c, who);
                p.sinks.insert(c, sink);
                vec![]
            });
            c
        }

        pub fn frame(&self, c: ConnId, bytes: Vec<u8>) {
            self.run(move |p| p.frame(c, bytes, now_ms()));
        }

        pub fn close(&self, c: ConnId) {
            self.run(move |p| p.leave(c, now_ms()));
        }

        /// Every connection of a session closed.
        pub fn revoke(&self, session: &str) {
            let session = session.to_string();
            self.run(move |p| p.revoke(&session, now_ms()));
        }

        /// What came before taken in, the volatile space written and the
        /// thread ended: what a server stopping does before its process
        /// exits, which would otherwise cut that write short. Every handle
        /// is inert after.
        pub fn stop(&self) {
            let mut p = self.inner.lock();
            self.serve(&mut p);
            p.end();
            drop(p);
            (self.inner.ring)();
        }

        /// `f` on the server, after what came before, and its answer; none
        /// once stopped.
        pub fn with<T>(&self, f: impl FnOnce(&mut Authority) -> T) -> Option<T> {
            let mut p = self.inner.lock();
            self.serve(&mut p);
            if p.stopped {
                return None;
            }
            let t = f(&mut p);
            self.serve(&mut p);
            Some(t)
        }

        /// An entry the authority authors itself ([`Authority::mutate_at`]):
        /// the head of the space it is on, sequenced, or the refusal.
        pub fn mutate(&self, name: String, input: Vec<u8>) -> Option<Result<Seq, String>> {
            self.with(move |p| {
                let space = p.function(&name)?.space;
                p.mutate_at(&name, input, now_ms())?;

                Ok(p.machine(space).map_or(0, |m| m.log().head().0))
            })
        }

        /// Facts the authority writes raw ([`Authority::write`]): the head
        /// they were sequenced at, or the refusal.
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

    struct InProcess(Handle<Authority>, ConnId, Receiver<Vec<u8>>, bool);

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
pub use serving::{Authenticate, Queue};

/// The pump in a browser: a run as soon as the page is free whenever the
/// connection or the app rings and while what arrived is not all taken in,
/// one task each so the page draws between them, and a timer for the
/// peer's next deadline — the app told when a pump moved what it can see.
#[cfg(target_arch = "wasm32")]
mod page {
    use std::sync::atomic::Ordering;
    use std::sync::Weak;

    use wasm_bindgen::JsCast;

    use super::{held, Driven, Handle, Inner};

    /// A run of the page's pump asked for and not yet made, and the
    /// deadline the page has a timer set for.
    #[derive(Default)]
    pub struct Page {
        asked: std::sync::atomic::AtomicBool,
        armed: std::sync::Mutex<Option<u64>>,
    }

    /// The page's bell: a run as soon as the page is free.
    pub fn ring<P: Driven>(inner: Weak<Inner<P>>) -> crate::link::Wake {
        std::rc::Rc::new(move || soon(&inner))
    }

    fn soon<P: Driven>(inner: &Weak<Inner<P>>) {
        let Some(i) = inner.upgrade() else { return };
        if !i.page.asked.swap(true, Ordering::Relaxed) {
            after(inner.clone(), 0);
        }
    }

    /// A task of the page's, in `ms`.
    fn after<P: Driven>(inner: Weak<Inner<P>>, ms: u64) {
        let run = wasm_bindgen::closure::Closure::once_into_js(move || drive(&inner));
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(run.unchecked_ref(), ms.min(i32::MAX as u64) as i32);
        }
    }

    fn drive<P: Driven>(inner: &Weak<Inner<P>>) {
        let Some(i) = inner.upgrade() else { return };
        i.page.asked.store(false, Ordering::Relaxed);
        if i.by_hand.load(Ordering::Relaxed) {
            return i.tell();
        }
        P::turn(&Handle { inner: i.clone() }, true);
        let (now, due) = (crate::monotonic_ms(), i.lock().deadline());
        match due {
            Some(d) if d <= now => soon(inner),
            Some(d) => {
                let mut armed = held(&i.page.armed);

                if *armed != Some(d) {
                    *armed = Some(d);
                    after(inner.clone(), d - now);
                }
            }
            None => {}
        }
    }
}
