//! harken's players (`docs/plan-v5.md` §1.9, S3b): **one reconciler** that
//! makes a player agree with the account's listening session and says what
//! happened to it, over any [`Sound`] — a phone's ExoPlayer through
//! `harken/ffi`, a Home Assistant speaker in `harken-peer --house`, a test's.
//!
//! The session is the domain's live state (`harken_domain::listening`,
//! `queue`, `devices`, `groups`), read as one value per account
//! ([`Listening`]). **The state is the instruction**: nothing tells a player
//! what to do. Every button on every device is a transition ([`Press`]); the
//! device the session names as its output makes its player agree
//! ([`Reconciler`]); every other device is silent. What the output says back
//! is only what happens to it: `started` once a hand-off has landed,
//! `advance` once per end of a track, `jump` when it was moved by hand,
//! `stalled` when it stopped by itself, `play` when it was resumed by hand,
//! `seek` and `report_position` when it is somewhere else than the session
//! counts it, `release` when it was taken from us; and its volume and group
//! (`equip`, `report_volume`, `report_group`).
//!
//! A [`Sound`] is what a reconciler drives: the commands a player takes, and
//! what it says of itself ([`Heard`], [`Event`]). How a player is *read* is
//! its adapter's — ExoPlayer's events, or Home Assistant's polls with their
//! blinks and stale dates — and what it means for the session is here, once.
//!
//! The reconciler's memory is rows ([`memory`]): an ephemeral local
//! instance (S2) holds what it last did and saw, typed so that an
//! impossible combination cannot be written; the plays it counted are a
//! durable local row. [`Player`] drives one over a device's own peer: its
//! presses, its plays authored in the log, and the one view a phone draws,
//! alone or joined.

pub mod memory;
pub mod player;
pub mod press;
pub mod reconcile;

pub use harken_domain::host::{Listening, SYNC_MS};
use harken_domain::{Id, Media, Track};
use serde::{Deserialize, Serialize};

pub use memory::Mind;
pub use player::{Player, Stats};
pub use press::{Change, Level, Press, NOT_PART};
pub use reconcile::Reconciler;

/// How far a playing player may be from the session before what moved it
/// was somebody's hand on it — a seek in the Sonos app, a buffer that ran
/// dry for seconds — said as the player's own `seek` rather than as where it
/// has drifted to (`report_position`).
pub const DRIFT_MS: i64 = 3_000;

/// A volume, 0 to 100, and its mute.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ark::prelude::Describe)]
pub struct Vol {
    pub level: i64,
    pub muted: bool,
}

/// What a player can be asked beyond playing, as its `equip` says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caps {
    pub can_volume: bool,
    /// The family it groups in; none for a player that joins no group.
    pub family: Option<String>,
}

/// Who plays with a player that groups, as it says: whether it leads its
/// group, and the others in it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Followers {
    pub leads: bool,
    pub members: Vec<String>,
}

/// Where a player is in its track, as far as it can say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// Nowhere anybody can place: fetching a hand-off.
    Unknown,
    /// Where it is counted to be — a buffer filling, a moment after it was
    /// told something — which a session that moved is compared with, and
    /// which is never said.
    Believed(i64),
    /// Where it measurably is, as of `at` (the server's clock): what may be
    /// said.
    Measured { ms: i64, at: i64 },
}

impl Place {
    pub fn ms(&self) -> Option<i64> {
        match *self {
            Place::Unknown => None,
            Place::Believed(ms) | Place::Measured { ms, .. } => Some(ms),
        }
    }
}

/// What a player says it is doing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heard {
    /// The queue it holds, by track: what it plays through on its own.
    pub queue: Vec<Id<Media>>,
    /// Where in it; none with nothing loaded, or nowhere it can say.
    pub at: Option<usize>,
    /// Asked to play — Media3's `playWhenReady`, what a speaker was last
    /// told — whether or not a buffer is still filling.
    pub playing: bool,
    /// It has what it was last loaded with, and can say where it is in it:
    /// a hand-off has landed.
    pub ready: bool,
    pub place: Place,
    /// At the end of the last track of its queue.
    pub ended: bool,
    /// Where the queue it holds was taken from, as whoever loaded it said.
    pub from: String,
}

/// What happened to a player by itself, said once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// It stopped by itself or by hand — focus lost, the headphones out, a
    /// pause on the speaker — here.
    Stalled { at_ms: i64 },
    /// It was resumed by hand.
    Resumed,
    /// It is not ours any more: sent elsewhere, or not answering.
    Lost { why: String },
}

/// What a reconciler drives: a player, its volume and its group.
pub trait Sound {
    fn heard(&self) -> Heard;

    /// What happened to it since the last ask, if anything.
    fn take(&mut self) -> Option<Event>;

    /// The tracks of the queue it holds, as [`Heard::queue`] names them.
    fn tracks(&self) -> Vec<Track>;

    /// Load this queue, at this track and point, playing or not.
    fn load(&mut self, queue: &[Track], at: usize, position_ms: i64, playing: bool, from: &str);

    /// Another track of the queue it holds, at this point.
    fn jump(&mut self, at: usize, position_ms: i64);

    fn resume(&mut self);

    fn pause(&mut self);

    fn seek(&mut self, ms: i64);

    /// It is not the output any more: silent — paused, unless it `follows`
    /// the device the sound went to, whose group it plays in.
    fn let_go(&mut self, follows: bool) {
        if !follows {
            self.pause();
        }
    }

    /// Reconcile again in `ms`: the moment a play would count.
    fn wake_in(&mut self, _ms: i64) {}

    /// What it can do; none while it cannot say.
    fn caps(&self) -> Option<Caps> {
        self.volume().map(|_| Caps {
            can_volume: true,
            family: None,
        })
    }

    /// Where its volume is; none where it has none anybody can set.
    fn volume(&self) -> Option<Vol> {
        None
    }

    fn set_volume(&mut self, _v: Vol) {}

    /// Who plays with it; none for a player that groups with nothing.
    fn followers(&self) -> Option<Followers> {
        None
    }

    /// These follow it.
    fn join(&mut self, _members: &[String]) {}

    /// `member` leaves whatever group it is in.
    fn unjoin(&mut self, _member: &str) {}
}

/// The session as one device reads it at one moment.
#[derive(Clone, Copy, Debug)]
pub struct View<'a> {
    /// The device: its id in the session.
    pub me: &'a str,
    /// Part of the session: linked, as itself, and standing in it. Not
    /// joined, a player is its own session.
    pub joined: bool,
    /// The connection it stands on: a new one is a room that has heard
    /// nothing of it.
    pub epoch: i64,
    pub listening: &'a Listening,
    /// The server's clock, as this device reads it.
    pub now: i64,
}

/// Where a reconciler's answers go: a transition of the session called as
/// the device, and a play authored in the log.
pub trait Say {
    /// `listening.<transition>` with `input`; `as_of` is the moment a
    /// position in it was measured, `why` what made it so.
    fn call(&mut self, transition: &'static str, input: Vec<u8>, as_of: Option<i64>, why: &str) -> Result<(), String>;

    fn play(&mut self, play: harken_domain::RecordPlay) -> Result<(), String>;
}
