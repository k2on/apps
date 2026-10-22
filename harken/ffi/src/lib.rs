//! harken's library for its phones (`docs/plan-v5.md` §1.16, N3): the
//! bridge (`ark-ffi`) with harken's domain linked in. The apps load this
//! one shared library, open their peer with [`harken_open`] and call every
//! function of the domain by name through it — typed by what `ark-dev gen`
//! writes from the domain — and read the listening session with
//! [`harken_listening`], which is the domain's own reading of it. A
//! [`Hall`] is a harken server in the same process — the hub
//! `harken-server` runs, over memory — that devices dial without a socket:
//! what the apps' tests sync through.

use std::sync::{Arc, Mutex, Weak};

use ark::peer::Who;
use ark_ffi::{Login, Peer, Refusal};
use ark_peer::{now_ms, Config, Handle, Live, Memory, Module};
use harken_domain::host::{self, Listening};
use serde_json::json;

uniffi::setup_scaffolding!();

mod player;
pub use player::{Loudness, PhoneSound, Player, SoundState};

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

/// A session given whole, as [`harken_listening`] writes it; nothing where it
/// does not read.
fn listening_of(json: &str) -> Listening {
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    Listening {
        devices: serde_json::from_value(v["devices"].clone()).unwrap_or_default(),
        scrub: serde_json::from_value(v["scrub"].clone()).unwrap_or_default(),
        session: serde_json::from_value(v["session"].clone()).unwrap_or_default(),
    }
}

/// The device picker of a session given whole (`harken_listening`'s JSON)
/// for the device `me`: what a screen with no peer behind it — a
/// screenshot's — draws.
#[uniffi::export]
pub fn harken_picker(listening: String, me: String) -> harken_player::Devices {
    harken_player::screens::picker(&listening_of(&listening), &me)
}

/// The diagnostics' listening lines of a session given whole, for `me`,
/// part of it and linked.
#[uniffi::export]
pub fn harken_lines(listening: String, me: String) -> Vec<harken_player::Line> {
    let l = listening_of(&listening);
    harken_player::screens::numbers(Some(&l), &me, true, true, 0, &Default::default(), None, now_ms())
}

/// The domain this library carries as a server's `/healthz` names it:
/// its schema's hash and its version (`harken-server`'s `domain` line).
#[uniffi::export]
pub fn harken_domain() -> String {
    let d = harken_domain::shared();
    format!("{} {}", d.schema_hash(), d.version)
}

/// Where a track's file is, from where its server is.
#[uniffi::export]
pub fn harken_media_url(server: String, file: String) -> String {
    host::url(&server, &file)
}

/// A harken server in this process: the hub `harken-server` runs — the log
/// and the listening session, `on_join`, `on_leave` and `on_restore` the
/// domain's — over memory, which devices dial without a socket. Its devices
/// are pumped by hand ([`Hall::carry`]), so a test says when what was said
/// has been answered.
/// Who a hall's token, `user/session`, is.
fn token(t: &str) -> Option<Who> {
    let (actor, session) = t.split_once('/')?;
    Some(Who {
        actor: actor.into(),
        session: session.into(),
        roles: vec![],
    })
}

#[derive(uniffi::Object)]
pub struct Hall {
    hub: Handle,
    devices: Mutex<Vec<Weak<Peer>>>,
}

#[uniffi::export]
impl Hall {
    #[uniffi::constructor]
    pub fn new() -> Arc<Hall> {
        let d = harken_domain::shared();
        let get = |n: &str| d.binding(&format!("listening.{n}")).map(String::from);
        let mut c = Config::of(d.clone());
        c.live = Some(Live {
            on_join: get("on_join"),
            on_leave: get("on_leave"),
            on_restore: get("on_restore"),
            arrive: Arc::new(host::arrive),
            keep: Some(256),
            ..Live::new(Module::of(&d))
        });
        let hub = Handle::spawn(ark_peer::Peer::serve(c, Box::new(Memory::default()), now_ms()));
        Arc::new(Hall {
            hub,
            devices: Mutex::default(),
        })
    }

    /// A device on the disk at `dir`, signed in as `user` on the session
    /// `session` — which is the device's id in the listening session —
    /// saying `device` (a `Device`'s JSON), dialling this hall.
    pub fn device(&self, dir: String, user: String, session: String, device: String) -> Result<Arc<Peer>, Refusal> {
        let login = Login {
            token: format!("{user}/{session}"),
            session,
            user,
            name: String::new(),
            email: String::new(),
            roles: vec![],
            expires_ms: 0,
        };
        let p = Peer::open(config(said(&device)?), &dir, Some(login))?;
        p.by_hand();
        p.connect_with("hall", self.hub.dialer(Arc::new(token)));
        self.devices.lock().unwrap_or_else(|e| e.into_inner()).push(Arc::downgrade(&p));
        self.carry();
        Ok(p)
    }

    /// `user`'s listening session as the hall holds it, as
    /// [`harken_listening`] says a device's.
    pub fn listening(&self, user: String) -> String {
        let l = self.hub.with(move |p| p.live.as_ref().map(|l| Listening::of(&l.store, &user)));
        listening_json(&l.flatten().unwrap_or_default())
    }

    /// Everything every device said answered and taken in, until nobody
    /// has anything more to say.
    pub fn carry(&self) {
        let devices: Vec<Arc<Peer>> = self
            .devices
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter_map(Weak::upgrade)
            .collect();
        let mut still = 0;
        for _ in 0..64 {
            // Whatever the hub had, answered: its thread runs this after.
            let _ = self.hub.with(|_| ());
            let moved = devices.iter().fold(false, |m, p| p.pump() | m);
            still = if moved { 0 } else { still + 1 };
            if still == 2 {
                return;
            }
        }
    }
}
