//! A device's events travel to its server (docs/plan-observe.md): kept in
//! the device's outbox, sent as a `Telemetry` frame when it ships, and
//! collected by the server named by the login that sent them. The device
//! and the server share this process, so the outbox is the sink of both:
//! what the server collects comes back into it, marked.

use std::sync::Arc;

use ark::format::Encode;
use ark::schema::Space;
use ark_peer::observe::{install, Sink};
use ark_peer::{dev, Config, Memory, Peer};
use arkdb::demo::{demo, CreatePlaylist};

/// Falsified by the hub routing the telemetry to its machine like any
/// frame: nothing came back marked as sent by the device's login.
#[test]
fn a_devices_events_reach_its_server_named_by_its_login() {
    assert!(install(Sink::Outbox));
    let config = || Config::of(Arc::new(demo()));
    let mut hub = Peer::serve(config(), Box::new(Memory::default()), 0);
    let mut d = Peer::open(config(), dev("alice@phone").unwrap(), Box::new(Memory::default()), false);
    let c = hub.accept(d.me().clone());
    d.connected();
    d.linked = true;
    let f = d.binding("create_playlist").unwrap();
    d.author(Space::Durable, &f, CreatePlaylist { name: "Road".into() }.to_bytes(), 1)
        .unwrap();
    d.ship();
    let frames = d.take_frames();
    assert!(
        frames
            .iter()
            .any(|b| matches!(ark::frames::Frame::dec(b), Ok(ark::frames::Frame::Telemetry { .. }))),
        "the device shipped"
    );
    for b in frames {
        let _ = hub.frame(c, b, 2);
    }
    // The server's own peer has an empty label in the outbox as anything
    // else; what it collected from the device is marked `from`.
    let back = ark_peer::observe::take_all();
    let from = back
        .iter()
        .filter(|e| e.fields.iter().any(|(k, v)| k == "from" && v.contains("phone")))
        .count();
    assert!(from > 0, "the server collected the device's events, marked: {back:?}");
}
