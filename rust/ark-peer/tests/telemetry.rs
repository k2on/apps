//! A device's events travel to its server (docs/plan-observe.md): kept in
//! the device's outbox, sent as a call of `ark.events` when it ships, and
//! collected by the server named by the login that sent them. The device
//! and the server share this process, so the outbox is the sink of both:
//! what the server collects comes back into it, marked.

use std::sync::Arc;

use ark::format::Encode;

use ark_peer::observe::{install, Event, Sink};
use ark_peer::{dev, Config, Memory, Peer};
use arkdb::demo::{demo, CreatePlaylist};

/// Falsified by the hub taking the events' call for one of its app's
/// (`take_calls`): nothing came back marked as sent by the device's login.
#[test]
fn a_devices_events_reach_its_server_named_by_its_login() {
    assert!(install(Sink::Outbox).is_ok());
    let config = || Config::of(Arc::new(demo()));
    let mut hub = Peer::serve(config(), Box::new(Memory::default()), 0);
    let mut d = Peer::open(config(), dev("alice@phone").unwrap(), Box::new(Memory::default()));
    let c = hub.accept(d.me().clone());
    d.connected();
    d.linked = true;
    let f = d.function("create_playlist").unwrap().name.clone();
    d.mutate_at(&f, CreatePlaylist { name: "Road".into() }.to_bytes(), 1).unwrap();
    let frames = d.take_frames();
    assert!(
        frames
            .iter()
            .any(|b| matches!(ark::frames::Frame::dec(b), Ok(ark::frames::Frame::Call { name, .. }) if name == ark::frames::EVENTS)),
        "the device shipped"
    );
    for b in frames {
        let _ = hub.frame(c, b, 2);
    }
    // What the server collected from the device is marked `from`, in the
    // outbox it keeps its own in: what the device, linked to itself here,
    // would send on.
    let back: Vec<Event> = (d.take_frames().iter())
        .filter_map(|b| match ark::frames::Frame::dec(b) {
            Ok(ark::frames::Frame::Call { name, input, .. }) if name == ark::frames::EVENTS => Vec::<Event>::from_bytes(&input).ok(),
            _ => None,
        })
        .flatten()
        .collect();
    let from = back
        .iter()
        .filter(|e| e.fields.iter().any(|(k, v)| k == "from" && v.contains("phone")))
        .count();
    assert!(from > 0, "the server collected the device's events, marked: {back:?}");
}
