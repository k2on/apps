//! The native socket speaks TLS: a phone or a desktop given an `https://`
//! server dials `wss://…/sync`, and that dial must reach the server as a
//! TLS handshake. Without TLS compiled in, the dial fails before it
//! connects ("TLS support not compiled in") and the device retries for
//! ever while a browser, which brings its own TLS, syncs.
//!
//! Falsified by dropping `rustls-tls-webpki-roots` from ark-peer's
//! tungstenite: the listener is never dialled and the socket closes with
//! the URL error.

#![cfg(not(target_arch = "wasm32"))]

use std::io::Read;
use std::net::TcpListener;
use std::time::Duration;

use ark_peer::link::{socket, until, Event};

#[test]
fn a_wss_dial_opens_with_a_tls_handshake() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let heard = std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        conn.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut first = [0u8; 3];
        conn.read_exact(&mut first).unwrap();
        first
    });
    let mut wire = socket(&format!("wss://localhost:{port}/sync"), "token", None);
    let first = heard.join().expect("the socket dialled the listener");
    // A TLS record: handshake (22), protocol 3.x.
    assert_eq!((first[0], first[1]), (22, 3), "a ClientHello, not plain WebSocket bytes: {first:?}");
    // The listener speaks no TLS back, so the dial then fails; what matters
    // is that it failed in the handshake, not before connecting.
    let mut why = None;
    until(Duration::from_secs(10), Duration::from_millis(20), || {
        why = wire.events().into_iter().find_map(|e| match e {
            Event::Closed(w) => Some(w),
            _ => None,
        });
        why.is_some()
    });
    let why = why.expect("the socket said why it closed");
    assert!(!why.contains("TLS support not compiled in"), "{why}");
}
