//! This device's end of the account's listening session (§1.9): harken's
//! one reconciler (`harken_player::Player`) over this device's peer, and the
//! little that is the window's — what it calls itself, and the device it
//! declares at its volatile `Hello`.
//!
//! The session is the domain's live state, read off the peer's live store
//! (the server's, this device's own calls previewed on top). **The state is
//! the instruction**: every button is a [`harken_player::Press`]
//! ([`Remote::call`]); the device the session names as its output makes its
//! player agree ([`Remote::reconcile`], over the window's [`crate::Player`]
//! as a `harken_player::Sound`); every other device is silent and draws it.
//! What the output says back, the rule for coming back, the volume and the
//! plays are the reconciler's, the same as a phone's and a speaker's.
//!
//! **A device is a login**: its id is the login's session id, what a
//! volatile `Hello` declares with an empty id and the server fills in.
//! **The desktop has no audio device** (`Player::AUDIBLE` is false), so it
//! declares itself one that cannot play: never the output, every button a
//! remote control, no volume to say.

use crate::client::Replica as Peer;
use harken_player::{Press, Sound, Stats};
// What the rest of this program asks *this* module about.
pub use crate::domain::host::{Kind, Listening};
pub use crate::domain::{Active, Clock, Output, Present as Device, Track};

/// This device's end of the account's listening session.
pub struct Remote {
    /// What the picker draws for it.
    name: String,
    /// The reconciler and its presses, as this device's login.
    player: harken_player::Player,
}

impl Default for Remote {
    fn default() -> Remote {
        Remote::new()
    }
}

/// The device this build declares at its volatile `Hello`: its login's
/// session (an empty id), a computer, and whether it can make a sound at all.
pub fn device() -> Vec<u8> {
    crate::client::device(crate::Player::AUDIBLE, &device_name())
}

impl Remote {
    pub fn new() -> Remote {
        Remote {
            name: device_name(),
            player: player(""),
        }
    }

    /// This device is a login, so a new login is a new device — and a new
    /// reconciler, which has met nothing. Called at sign-in and whenever the
    /// login changes.
    pub fn open(&mut self, device: &str) {
        let stats = std::mem::take(&mut self.player.stats);
        self.player = player(device);
        self.player.stats = stats;
    }

    /// Stop. A peer that has signed out has no session to be part of.
    pub fn close(&mut self) {
        self.open("");
    }

    /// This device, as the picker names it.
    pub fn me(&self) -> &str {
        self.player.me()
    }

    /// What this device calls itself.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn stats(&self) -> &Stats {
        &self.player.stats
    }

    /// The session, when there is one to be part of; `None` is this device
    /// being its own session.
    pub fn session(&self, peer: &Peer) -> Option<Listening> {
        let s = harken_player::Player::listening(peer);
        self.player.joined(peer, &s).then_some(s)
    }

    /// Whether this device is part of the session now: linked, as the login
    /// it was opened for, and standing in it.
    pub fn joined(&self, peer: &Peer) -> bool {
        self.session(peer).is_some()
    }

    /// Whether this device is the one making the sound.
    pub fn outputs_here(&self, s: &Listening) -> bool {
        s.outputs(self.me())
    }

    /// Whether the sound belongs somewhere else — here or not — which is
    /// when this device is silent and its bar draws the session.
    ///
    /// Not the negation of [`Remote::outputs_here`]: a session with *no*
    /// output is neither, and there a press plays here.
    pub fn elsewhere(&self, s: &Listening) -> bool {
        s.output().and_then(Output::id).is_some_and(|id| id != self.me())
    }

    /// How far into the track, counted on from the session's clock against
    /// the server's ([`Active::position`]): what a scrubber drawn from any
    /// device shows.
    pub fn position_ms(a: &Active) -> i64 {
        a.position(crate::client::now_ms())
    }

    /// A press, as the session's transition; `local` is this device's own
    /// player, whose queue a play presses when the session has none. An
    /// `Err` of [`harken_player::NOT_PART`] is this device being its own
    /// session: the caller does it to its player.
    pub fn call(&mut self, peer: &mut Peer, press: &Press, local: Option<&dyn Sound>) -> Result<(), String> {
        match self.player.press(peer, press, local) {
            None => Ok(()),
            Some(why) => Err(why),
        }
    }

    /// Make this device agree with the session, and say what happened to it
    /// (`harken_player::Reconciler`). Called after every update, whatever it
    /// was. What the server refused of this device's calls since, taken.
    pub fn reconcile(&mut self, peer: &mut Peer, sound: &mut dyn Sound) -> Vec<String> {
        match self.me().is_empty() {
            true => self.player.refusals(peer),
            false => self.player.step(peer, sound),
        }
    }
}

/// The reconciler of the device `me`, counting its plays in memory, as this
/// window always has.
fn player(me: &str) -> harken_player::Player {
    harken_player::Player::new(me, Some(Box::new(ark_peer::Memory::default())))
}

/// What this device is called in somebody else's picker.
///
/// No more than the build, deliberately: more than this would want a
/// user-agent string, which is a guess dressed as a fact. The picker marks
/// which row is you anyway.
fn device_name() -> String {
    if cfg!(target_arch = "wasm32") {
        "Browser".to_string()
    } else {
        "Desktop".to_string()
    }
}
