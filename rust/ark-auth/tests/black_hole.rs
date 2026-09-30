//! A server that accepts and never answers — a lid closed on the far end,
//! a proxy holding the socket, a process stopped under its listener — is
//! given up on within the client's patience (`docs/plan-perf.md` R6). It
//! used to be waited on for ever: `login` had no timeout, so a desktop
//! signing in and a headless peer before its first frame hung there.

use std::io::Read;
use std::net::TcpListener;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use ark_auth::client::{self, Patience};

/// A listener that takes every connection and every byte and says
/// nothing; its sockets are held open for as long as the test runs.
fn black_hole() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let mut held = vec![];
        for s in listener.incoming().flatten() {
            let mut r = s.try_clone().unwrap();
            std::thread::spawn(move || {
                let mut buf = [0u8; 1024];
                while matches!(r.read(&mut buf), Ok(n) if n > 0) {}
            });
            held.push(s);
        }
    });
    url
}

/// `login` and `exchange` against a black hole fail, each within a
/// quarter-second patience and a second of slack, naming a timeout; and
/// the defaults are the constants, which are what `client.rs` argues for.
/// Run on a thread and watched for five seconds, so that the version that
/// waits for ever fails rather than hangs. Falsified by leaving
/// `timeout_read` off the agent: "still waiting after 5s".
#[test]
fn a_login_into_a_black_hole_gives_up_within_its_patience() {
    assert_eq!(
        (client::CONNECT_TIMEOUT, client::READ_TIMEOUT, Patience::default()),
        (Duration::from_secs(10), Duration::from_secs(20), Patience::DEFAULT)
    );
    let url = black_hole();
    let patience = Patience::of(Duration::from_millis(250));
    for which in ["login", "exchange"] {
        let (tx, rx) = channel();
        let u = url.clone();
        let t = Instant::now();
        std::thread::spawn(move || {
            let got = match which {
                "login" => client::login_with(&u, Some("alice"), |_| {}, patience).map(|_| ()),
                _ => client::exchange_with(&u, "a-code", patience).map(|_| ()),
            };
            let _ = tx.send(got);
        });
        let got = rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|_| panic!("{which}: still waiting after 5s"));
        let took = t.elapsed();
        let why = got.expect_err("a black hole signs nobody in");
        assert!(took < Duration::from_millis(1_250), "{which}: {took:?}");
        assert!(
            why.to_lowercase().contains("timed out") || why.to_lowercase().contains("timeout"),
            "{which}: {why}"
        );
    }
}
