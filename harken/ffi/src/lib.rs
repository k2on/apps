//! harken's library for its phones (`docs/plan-v5.md` §1.16, N3): the
//! bridge (`ark-ffi`) with harken's domain linked in. The apps load this
//! one shared library, open their peer with [`harken_open`] and call every
//! function of the domain by name through it — typed by what `arkdb gen`
//! writes from the domain — and read the listening session with
//! [`harken_listening`], which is the domain's own reading of it. A
//! [`Hall`] is a harken server in the same process — the hub
//! `harken-server` runs, over memory — that devices dial without a socket:
//! what the apps' tests sync through.

use std::sync::Arc;

use ark_ffi::{Login, Peer, Refusal};
use ark_peer::Config;
use harken_domain::host::{self, Listening};
use serde_json::json;

uniffi::setup_scaffolding!();

mod player;
pub use player::{Loudness, PhoneSound, Player, SoundState};

/// harken's peer as a phone is made of it (`harken_player::config::peer`):
/// the domain for the log and the listening session, saying `device` in its
/// volatile `Hello`, `scrub` relayed as ephemeral.
pub fn config(device: Vec<u8>) -> Config {
    harken_player::config::peer(&harken_domain::shared(), device, harken_player::config::THROTTLE_MS)
}

/// What a device says it is, from its JSON: a `Device` in the value format.
fn said(device: &str) -> Result<Vec<u8>, Refusal> {
    let d: harken_domain::types::Device = serde_json::from_str(device).map_err(|e| Refusal::from(format!("a device: {e}")))?;
    Ok(ark::format::Encode::to_bytes(&d))
}

/// harken's peer on the disk at `dir`, as whoever last signed in there or
/// as `login`, saying `device` (a `Device`'s JSON) in the listening session.
#[uniffi::export]
pub fn harken_open(dir: String, login: Option<Login>, device: String) -> Result<Arc<Peer>, Refusal> {
    Peer::open(config(said(&device)?), &dir, login)
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
    listening_json(&l)
}

fn listening_json(l: &Listening) -> String {
    json!({ "devices": l.devices, "scrub": l.scrub, "session": l.session }).to_string()
}

/// The domain this library carries as a server's `/healthz` names it:
/// its schema's hash and its version (`ark_peer::version`: the server's `domain` line).
#[uniffi::export]
pub fn harken_domain() -> String {
    ark_peer::version(&harken_domain::shared())
}

/// Where a track's file is, from where its server is.
#[uniffi::export]
pub fn harken_media_url(server: String, file: String) -> String {
    host::url(&server, &file)
}

/// A harken server in this process ([`ark_ffi::Hall`]): the hub
/// `harken-server` runs — the log and the listening session, `on_join`,
/// `on_leave` and `on_restore` the domain's — over memory, which devices
/// dial without a socket. Its devices are pumped by hand ([`Hall::carry`]),
/// so a test says when what was said has been answered.
#[derive(uniffi::Object)]
pub struct Hall(ark_ffi::Hall);

#[uniffi::export]
impl Hall {
    #[uniffi::constructor]
    pub fn new() -> Arc<Hall> {
        Arc::new(Hall(ark_ffi::Hall::new(harken_player::config::hub(&harken_domain::shared(), None))))
    }

    /// A device on the disk at `dir`, signed in as `user` on the session
    /// `session` — which is the device's id in the listening session —
    /// saying `device` (a `Device`'s JSON), dialling this hall.
    pub fn device(&self, dir: String, user: String, session: String, device: String) -> Result<Arc<Peer>, Refusal> {
        self.0.device(config(said(&device)?), &dir, user, session, vec![])
    }

    /// `user`'s listening session as the hall holds it, as
    /// [`harken_listening`] says a device's.
    pub fn listening(&self, user: String) -> String {
        let l = self.0.hub.with(move |p| p.live.as_ref().map(|l| Listening::of(&l.store, &user)));
        listening_json(&l.flatten().unwrap_or_default())
    }

    /// Everything every device said answered and taken in, until nobody
    /// has anything more to say.
    pub fn carry(&self) {
        self.0.carry()
    }
}
