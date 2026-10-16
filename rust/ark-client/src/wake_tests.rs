//! A peer driven by its wakes rather than a clock (`docs/plan-reactive.md`):
//! at rest nothing rings and nothing is published, a burst is one wake and
//! one settle, and the only timer is the redial a close arms and an open
//! clears. Each is driven over [`Queues`] against the engine's own sans-io
//! server, with time handed in (`Peer::pump_at`), so a minute of rest is a
//! minute of simulated milliseconds and not one of waiting.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ark::canon;
use ark::protocol::{open_access, trusting, ClientMsg, Server, ServerMsg};
use ark::store::Store;

use crate::link::{BoxTransport, Event, Queues};
use crate::peer::SETTLES;
use crate::{args, demo, Options, Peer, Timing, Value};

/// A peer over transports the test holds, every wake counted.
struct Rig {
    peer: Peer,
    made: Arc<Mutex<Vec<Queues>>>,
    wakes: Arc<AtomicUsize>,
    server: Server,
}

const BOB: i64 = 2;

impl Rig {
    fn new() -> Rig {
        let d = demo::domain();
        let mut a = ark::peer::Authority::new(d.module().schema.clone(), d.closures().clone());
        a.hold(d.native_list());
        let server = Server::open(trusting(), open_access(), a);
        let mut peer = Peer::open_memory(d, Options::dev("bob").with_timing(Timing::default())).unwrap();
        let wakes = Arc::new(AtomicUsize::new(0));
        let w = wakes.clone();
        peer.set_waker(Arc::new(move || {
            w.fetch_add(1, Ordering::SeqCst);
        }));
        let made: Arc<Mutex<Vec<Queues>>> = Arc::default();
        let m = made.clone();
        peer.connect_with(
            "queues",
            Box::new(move |_: &str| {
                let q = Queues::default();
                m.lock().unwrap().push(q.clone());
                Box::new(q) as BoxTransport
            }),
        );
        Rig { peer, made, wakes, server }
    }

    fn wakes(&self) -> usize {
        self.wakes.load(Ordering::SeqCst)
    }

    fn socket(&self) -> Queues {
        self.made.lock().unwrap().last().cloned().expect("dialled")
    }

    /// What the peer sent goes to the server, and what the server says back
    /// is pushed onto the socket as frames — each push a wake, as a reader
    /// thread's is. How many frames went down.
    fn exchange(&mut self) -> usize {
        let q = self.socket();
        for f in std::mem::take(&mut *q.sent.lock().unwrap()) {
            let m = ClientMsg::from_value(&canon::decode(&f).unwrap()).unwrap();
            self.server.recv(BOB, m);
        }
        let down: Vec<ServerMsg> = self
            .server
            .take_outgoing()
            .into_iter()
            .filter(|(c, _)| *c == BOB)
            .map(|(_, m)| m)
            .collect();
        for m in &down {
            q.push(Event::Frame(canon::encode(&m.to_value())));
        }
        down.len()
    }

    /// Dialled, open, and caught up: what a host's wakes come to until the
    /// two ends have nothing more to say.
    fn linked(&mut self, now: u64) {
        self.peer.pump_at(now);
        self.socket().push(Event::Opened);
        for _ in 0..20 {
            self.peer.pump_at(now);
            if self.exchange() == 0 {
                self.peer.pump_at(now);
                if self.exchange() == 0 {
                    break;
                }
            }
        }
        assert!(self.peer.linked());
        assert!(self.peer.pump_at(now).quiet(), "caught up");
    }
}

/// Connected and idle, a peer is a minute of nothing: no wake, no deadline
/// for a host to arm, and every pump a host might make anyway publishes
/// nothing — not its status, not its views. Falsified by publishing the
/// status on every pump (dropping the by-value compare): every one of the
/// sixty is loud; and by reporting a deadline while open
/// (`State::Open => Some(0)` in `Link::deadline`): the host would arm one.
#[test]
fn a_connected_peer_at_rest_is_never_woken_and_publishes_nothing() {
    let mut r = Rig::new();
    r.linked(1_000);
    let woken = r.wakes();
    for s in 1..=60u64 {
        let p = r.peer.pump_at(1_000 + s * 1_000);
        assert!(p.quiet(), "second {s} published {p:?}");
        assert_eq!(p.deadline, None, "no timer while open");
    }
    assert_eq!(r.peer.deadline(), None);
    assert_eq!(r.wakes(), woken, "a minute at rest rang nothing");
}

/// What the app asks for goes at once: a mutation is pushed by `mutate`
/// itself, with no pump after it, and the ack that comes back is one wake.
/// Falsified by taking the flush out of `mutate`: nothing reached the
/// server until a pump.
#[test]
fn a_mutation_is_pushed_by_the_call_that_made_it() {
    let mut r = Rig::new();
    r.linked(1_000);
    let woken = r.wakes();
    r.peer.mutate("create_playlist", args([("name", Value::text("Now"))])).unwrap();
    assert_eq!(r.socket().sent.lock().unwrap().len(), 1, "pushed by mutate");
    assert!(r.exchange() >= 1, "the server answered");
    assert_eq!(r.wakes(), woken + 1, "the answer is one wake");
    let p = r.peer.pump_at(2_000);
    assert!(p.moved && p.views);
    assert_eq!(r.peer.pending_len(), 0);
}

/// Fifty frames arriving while nobody pumps are one wake — the "drain
/// requested" flag — and the one pump that answers them settles once
/// (`docs/plan-perf.md` R8, per wake rather than per tick). Falsified by
/// ringing the host for every event (no flag in `Coalesced`): fifty wakes.
#[test]
fn a_burst_of_frames_is_one_wake_and_one_settle() {
    let mut r = Rig::new();
    r.linked(1_000);
    // Alice, on the same server, makes fifty playlists; each is a frame to
    // bob, as the server's fan-out sends it.
    let mut alice = Peer::open_memory(demo::domain(), Options::dev("alice")).unwrap();
    alice.connected();
    let mut to_bob = vec![];
    for n in 0..51 {
        if n > 0 {
            alice.mutate("create_playlist", args([("name", Value::text(format!("p{n}")))])).unwrap();
        }
        for m in alice.take_outgoing() {
            r.server.recv(1, m);
        }
        for (c, m) in r.server.take_outgoing() {
            match c {
                1 => alice.recv(m),
                _ => to_bob.push(m),
            }
        }
    }
    assert!(to_bob.len() >= 50, "{} frames for bob", to_bob.len());
    let woken = r.wakes();
    let q = r.socket();
    for m in &to_bob {
        q.push(Event::Frame(canon::encode(&m.to_value())));
    }
    assert_eq!(r.wakes() - woken, 1, "{} frames, one wake", to_bob.len());
    let settled = SETTLES.with(|n| n.get());
    let p = r.peer.pump_at(2_000);
    assert_eq!(SETTLES.with(|n| n.get()) - settled, 1, "one settle");
    assert!(p.views);
    assert_eq!(r.peer.store().scan("playlist").len(), 50);
    // The next frame after that pump rings again.
    q.push(Event::Frame(canon::encode(&to_bob[0].to_value())));
    assert_eq!(r.wakes() - woken, 2, "the flag was answered by the pump");
}

/// A close arms exactly one timer — the redial, at the backoff — and the
/// deadline stays put until it is due, so a host keyed by it arms nothing
/// twice; the dial clears it, and the open after it leaves none. Falsified
/// by `Link::deadline` answering `None` for `Waiting`: the close arms
/// nothing and the peer would never dial again.
#[test]
fn a_close_arms_one_redial_and_an_open_clears_it() {
    let mut r = Rig::new();
    r.linked(1_000);
    let woken = r.wakes();
    r.socket().push(Event::Closed("gone".into()));
    assert_eq!(r.wakes(), woken + 1, "the close is a wake");
    let p = r.peer.pump_at(1_000);
    assert_eq!(p.dropped.as_deref(), Some("gone"));
    assert_eq!(p.deadline, Some(1_500), "one redial, half a second out");
    for t in [1_100, 1_250, 1_499] {
        let p = r.peer.pump_at(t);
        assert_eq!(p.deadline, Some(1_500), "the same deadline at {t}: nothing re-armed");
        assert!(p.quiet(), "{p:?}");
    }
    assert_eq!(r.made.lock().unwrap().len(), 1, "not dialled early");
    let p = r.peer.pump_at(1_500);
    assert_eq!(r.made.lock().unwrap().len(), 2, "dialled at the deadline");
    assert_eq!(p.deadline, None, "dialling: the socket's own wakes are all there is");
    r.socket().push(Event::Opened);
    let p = r.peer.pump_at(1_520);
    assert!(p.opened);
    assert_eq!(p.deadline, None);
    assert_eq!(r.peer.deadline(), None, "an open cancels it");
}
