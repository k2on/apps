//! A peer driven on a thread of its own, so the thread an app draws on
//! never takes in what its server sent: a part of a fill, an entry's facts
//! put, its views moved, the disk written. What an app does with the peer
//! is what it did, on the [`Peer`] itself, through [`Driven::lock`]: a
//! mutate, a query, a view opened or settled. The lock is held by the
//! driver for one pump at a time, so a call waits for at most one.
//!
//! The driver pumps whenever the connection rings, the peer's deadline
//! comes ([`Peer::deadline`]) or the app has used the peer — what it said
//! is sent — and tells the app (its waker, [`Driven::set_waker`]) when
//! anything came in; the app then reads what moved, as it did after its
//! own pump ([`Driven::pump`]: what the driver's pumps did since it last
//! asked). In a browser there is no thread: the app pumps, as it did, and
//! a pump there is the peer's own.
//!
//! **Work off the thread an app draws on is still work.** A pump that
//! takes longer than [`PUMP_BUDGET_MS`], and an app's wait for the peer
//! longer than [`WAIT_BUDGET_MS`], are said as events of the device's, which
//! go to its server (`crate::observe`): slow work is in the server's events
//! whichever thread did it.

use std::sync::{Arc, Mutex, MutexGuard};

use crate::link::{Pumped, Wake};
use crate::Peer;

/// How long one pump may take, in ms, before it is said: past a frame,
/// what it did is work the device should not have had to do, wherever it
/// was done.
pub const PUMP_BUDGET_MS: u64 = 16;

/// How long an app may wait for the peer, in ms, before it is said: the
/// thread it draws on held up by the driver.
pub const WAIT_BUDGET_MS: u64 = 4;

/// How long one pump in a browser takes things in, in ms, before it leaves
/// the rest for the next: half a frame, the other half the page's.
pub const PAGE_BUDGET_MS: u64 = 8;

struct Inner {
    peer: Mutex<Peer>,
    /// What the driver's pumps did since the app last asked.
    said: Mutex<Pumped>,
    /// The app's, rung when a pump of the driver's moved anything.
    told: Mutex<Option<Wake>>,
    /// The driver's own: rung by the connection, and by an app done with
    /// the peer.
    #[cfg(not(target_arch = "wasm32"))]
    ring: Wake,
    /// Pumped by the app ([`Driven::pump`]) from now on, and not by the
    /// driver: a test that says when what arrived is taken in.
    by_hand: std::sync::atomic::AtomicBool,
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, Peer> {
        self.peer.lock().unwrap_or_else(|e| e.into_inner())
    }

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
        absorb(&mut self.said.lock().unwrap_or_else(|e| e.into_inner()), &p);
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

/// A [`Peer`] driven on a thread of its own (in a browser, by whoever
/// pumps it).
#[derive(Clone)]
pub struct Driven {
    inner: Arc<Inner>,
}

/// The peer, held: what [`Driven::lock`] gives, the [`Peer`] itself. Let
/// go, the driver is rung, so what the app had it say is sent.
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
        #[cfg(not(target_arch = "wasm32"))]
        if !self.inner.by_hand.load(std::sync::atomic::Ordering::Relaxed) {
            (self.inner.ring)();
        }
        #[cfg(target_arch = "wasm32")]
        let _ = self.inner;
    }
}

thread_local! {
    /// The peers this thread holds. One held again on the same thread
    /// would wait on itself for ever: a guard kept alive past where it was
    /// meant to be let go (a `match` on a call through it, two in one
    /// `format!`), which a debug build says rather than hangs on.
    static HELD: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) };
}

impl Driven {
    /// `peer`, driven from now on.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(mut peer: Peer) -> Driven {
        let (ring, rx) = crate::link::bell();
        peer.set_waker(ring.clone());
        let inner = Arc::new(Inner {
            peer: Mutex::new(peer),
            said: Mutex::default(),
            told: Mutex::new(None),
            ring,
            by_hand: Default::default(),
        });
        let weak = Arc::downgrade(&inner);
        std::thread::Builder::new()
            .name("ark-peer".into())
            .spawn(move || drive(weak, rx))
            .expect("a thread for the peer");
        Driven { inner }
    }

    /// `peer`, pumped by whoever pumps it: a browser has no thread. A pump
    /// there takes in what it can in [`PAGE_BUDGET_MS`] and leaves the rest
    /// for the next, so the page draws between them.
    #[cfg(target_arch = "wasm32")]
    pub fn new(mut peer: Peer) -> Driven {
        peer.link.budget = Some(PAGE_BUDGET_MS);
        Driven {
            inner: Arc::new(Inner {
                peer: Mutex::new(peer),
                said: Mutex::default(),
                told: Mutex::new(None),
                by_hand: std::sync::atomic::AtomicBool::new(true),
            }),
        }
    }

    /// The peer, held until the guard is let go: the driver waits for it,
    /// and it for the driver's pump — said when that was longer than
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

    /// `w` rung whenever the driver took something in. In a browser, the
    /// connection's own waker: the app pumps when it rings.
    pub fn set_waker(&self, w: Wake) {
        #[cfg(target_arch = "wasm32")]
        self.inner.lock().set_waker(w.clone());
        *self.inner.told.lock().unwrap_or_else(|e| e.into_inner()) = Some(w);
    }

    /// Pumped by [`Driven::pump`] from now on, never by the driver.
    pub fn by_hand(&self) {
        self.inner.by_hand.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// What was taken in since the app last asked: by the driver, or —
    /// pumped by hand — by a pump now.
    pub fn pump(&self) -> Pumped {
        if self.inner.by_hand.load(std::sync::atomic::Ordering::Relaxed) {
            self.inner.pump();
        }
        std::mem::take(&mut *self.inner.said.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

/// The driver: a pump whenever the connection or the app rings or the
/// peer's deadline comes, the app told when it moved anything; until the
/// peer is gone.
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
        // Pumped by hand, the app is told what rang, and pumps itself; its
        // own clock is its deadlines.
        let by_hand = i.by_hand.load(std::sync::atomic::Ordering::Relaxed);
        if by_hand && rang || !by_hand && i.pump().moved {
            let told = i.told.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if let Some(w) = told {
                w();
            }
        }
    }
}
