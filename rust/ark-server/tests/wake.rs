//! Peers on real sockets driven only by their wakes (`docs/plan-reactive.md`):
//! nothing here pumps on a clock. Each peer's waker sends its name down one
//! channel; the test pumps a peer when it is rung, and at the deadline it
//! reports — and at no other time — so a wake the native transport failed
//! to ring is a test that never converges.

mod common;

use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ark::store::Store;
use ark::value::Value;
use ark_client::{args, autos::monotonic_ms, demo, Options, Peer, Timing};
use ark_server::Keepalive;

/// Pump whichever peer is rung, or due, until `done` — never otherwise.
/// How many wakes each took.
fn drive(peers: &mut [&mut Peer], woken: &Receiver<usize>, wakes: &mut [usize], mut done: impl FnMut(&mut [&mut Peer]) -> bool) {
    let give_up = Instant::now() + Duration::from_secs(10);
    for p in peers.iter_mut() {
        p.pump();
    }
    while !done(peers) {
        assert!(
            Instant::now() < give_up,
            "never converged: {:#?}",
            peers.iter().map(|p| p.status()).collect::<Vec<_>>()
        );
        let due = peers.iter().filter_map(|p| p.deadline()).min();
        let wait = due.map_or(Duration::from_millis(500), |at| Duration::from_millis(at.saturating_sub(monotonic_ms())));
        match woken.recv_timeout(wait) {
            Ok(i) => {
                wakes[i] += 1;
                peers[i].pump();
            }
            Err(RecvTimeoutError::Timeout) => {
                // Only a deadline may be answered by a pump; half a second
                // of nothing with none armed is just more waiting.
                let now = monotonic_ms();
                for p in peers.iter_mut() {
                    if p.deadline().is_some_and(|at| at <= now) {
                        p.pump();
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => unreachable!(),
        }
    }
}

/// Two peers on a real server converge on wakes alone; then, linked and
/// idle for two seconds while the native transport pings every 150 ms and
/// the server every 200 ms, not one wake rings and no deadline is armed.
/// Falsified by not calling the waker from the transport's reader
/// (`report` sending without waking): the first `Hello` is never answered
/// and the drive times out.
#[test]
fn peers_on_real_sockets_converge_on_wakes_and_rest_in_silence() {
    let rt = common::runtime();
    let running = common::serve(&rt, None, |b| {
        b.keepalive(Keepalive {
            every: Duration::from_millis(200),
            missed: 3,
        })
    });
    let timing = Timing {
        ping_every_ms: 150,
        ..common::quick()
    };
    let (tx, woken) = channel();
    let make = |name: &str, i: usize| {
        let mut p = Peer::open_memory(demo::domain(), Options::dev(name).with_timing(timing.clone())).unwrap();
        let tx = tx.clone();
        p.set_waker(Arc::new(move || {
            let _ = tx.send(i);
        }));
        p.connect(&running.sync_url());
        p
    };
    let (mut alice, mut bob) = (make("alice", 0), make("bob", 1));
    let mut wakes = [0usize; 2];
    drive(&mut [&mut alice, &mut bob], &woken, &mut wakes, |ps| ps.iter().all(|p| p.linked()));
    alice.mutate("create_playlist", args([("name", Value::text("Woken"))])).unwrap();
    drive(&mut [&mut alice, &mut bob], &woken, &mut wakes, |ps| {
        ps[0].pending_len() == 0 && ps[1].store().scan("playlist").len() == 1
    });
    // Whatever the last frames were (an ack, a verify's answer), they land.
    std::thread::sleep(Duration::from_millis(300));
    while let Ok(i) = woken.try_recv() {
        [&mut alice, &mut bob][i].pump();
    }
    assert!(alice.pump().quiet() && bob.pump().quiet(), "caught up");

    let rest = Instant::now();
    let mut rang = 0;
    while rest.elapsed() < Duration::from_secs(2) {
        if woken.recv_timeout(Duration::from_millis(100)).is_ok() {
            rang += 1;
        }
    }
    assert_eq!(rang, 0, "two seconds linked and idle, with pings both ways, rang nothing");
    assert_eq!((alice.deadline(), bob.deadline()), (None, None), "no timer to arm");
    assert!(alice.linked() && bob.linked(), "still linked: the pings kept it, unseen");
    assert!(alice.pump().quiet() && bob.pump().quiet());
    assert!(wakes.iter().all(|&w| w > 0), "{wakes:?}");
}

/// A server that goes away is a close, which arms one redial deadline; a
/// server back on the same port is dialled at it and opens, which clears
/// it — and nothing but wakes and that deadline pumped. Falsified by a
/// `Link::deadline` that is `None` while waiting: the drive never dials
/// again and times out.
#[test]
fn a_close_is_redialled_at_its_deadline_and_the_open_clears_it() {
    // A runtime of its own, so dropping it closes every socket it served.
    let first = common::runtime();
    let running = common::serve(&first, None, |b| b);
    let url = running.sync_url();
    let (tx, woken) = channel();
    let mut p = Peer::open_memory(demo::domain(), Options::dev("carol").with_timing(common::quick())).unwrap();
    p.set_waker(Arc::new(move || {
        let _ = tx.send(0);
    }));
    p.connect(&url);
    let mut wakes = [0usize];
    drive(&mut [&mut p], &woken, &mut wakes, |ps| ps[0].linked());
    let addr = url.trim_start_matches("ws://").split('/').next().unwrap().to_string();
    drop(running);
    drop(first);
    drive(&mut [&mut p], &woken, &mut wakes, |ps| ps[0].deadline().is_some());
    assert!(!p.linked());
    let rt = common::runtime();
    let again = rt.block_on(
        ark_server::builder(demo::domain())
            .name("test")
            .trusting()
            .build()
            .expect("builds")
            .serve(&addr),
    );
    let _again = again.expect("the port is free again");
    drive(&mut [&mut p], &woken, &mut wakes, |ps| ps[0].linked());
    assert_eq!(p.deadline(), None, "the open cleared it");
}
