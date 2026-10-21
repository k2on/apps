//! harken's library for its phones (`docs/plan-v5.md` §1.16, N3): the
//! bridge (`ark-ffi`) with harken's domain linked in. The apps load this
//! one shared library, open their peer with [`harken_open`] and call every
//! function of the domain by name through it — typed by what `ark-dev gen`
//! writes from the domain — and read the listening session with
//! [`harken_listening`], which is the domain's own reading of it.

use std::sync::Arc;

use ark_ffi::{Login, Peer, Refusal};
use ark_peer::{Config, Live, Module};
use harken_domain::host::{self, Listening};
use serde_json::json;

uniffi::setup_scaffolding!();

/// The milliseconds between two ephemeral calls the relay sends — a
/// scrubber dragged — twenty a second; the last always goes.
pub const THROTTLE_MS: i64 = 50;

/// harken's peer as a phone is made of it: the domain for the log and the
/// listening session, saying `device` (a `Device`'s JSON) in its volatile
/// `Hello`, `scrub` relayed as ephemeral.
pub fn config(device: Vec<u8>) -> Config {
    let d = harken_domain::shared();
    let live = Live {
        device: Some(device),
        ephemeral: ["listening.scrub".to_string()].into(),
        every_ms: THROTTLE_MS,
        ..Live::new(Module::of(&d))
    };
    Config {
        live: Some(live),
        ..Config::of(d)
    }
}

/// harken's peer on the disk at `dir`, as whoever last signed in there or
/// as `login`, saying `device` (a `Device`'s JSON) in the listening session.
#[uniffi::export]
pub fn harken_open(dir: String, login: Option<Login>, device: String) -> Result<Arc<Peer>, Refusal> {
    Peer::open(config(device.into_bytes()), &dir, login)
}

/// The account's listening session as this device holds it, as JSON:
/// `{"devices": [Present], "scrub": {device: at_ms}, "session": Active or
/// null}` — the server's, with this device's own calls previewed on top.
#[uniffi::export]
pub fn harken_listening(peer: Arc<Peer>) -> String {
    let l = peer.with(|p| match &p.live {
        Some(live) => Listening::of(&live.store, &p.me().actor),
        None => Listening::default(),
    });
    json!({ "devices": l.devices, "scrub": l.scrub, "session": l.session }).to_string()
}

/// Where a track's file is, from where its server is.
#[uniffi::export]
pub fn harken_media_url(server: String, file: String) -> String {
    host::url(&server, &file)
}
