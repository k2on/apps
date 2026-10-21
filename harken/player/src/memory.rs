//! What a reconciler keeps between two looks, as rows (`docs/plan-v5.md`
//! §1.9, S2): its mind is a row of an **ephemeral** local instance
//! ([`Minds`], volatile, kept nowhere — a restart begins again from what
//! the player and the session say), and the play it counted is a row of a
//! **durable** one ([`Counts`]), so that a play is never authored twice for
//! one load, a restart between. Each column is a sum, so a combination the
//! rules never make cannot be written: a hand-off is not both started and
//! waited for, a volume is not "seen" before it was ever told.

use ark::prelude::*;
use harken_domain::{Clock, Id, Media};

use crate::Vol;

/// One place in one queue: track `at`, which is `track`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Describe)]
pub struct Here {
    pub at: i64,
    pub track: Id<Media>,
}

/// Whether the player was playing while the device was its own session —
/// what decides the rule for coming back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Describe)]
pub enum Alone {
    Quiet,
    Playing,
}

/// Whether the device was the output at its last look: one that stops being
/// it lets go once.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Describe)]
pub enum Role {
    Silent,
    Output,
}

/// What was said of the track `here`: the hand-off landed on it
/// (`started`), or the player moved off it by itself (`advance` at its end,
/// `jump` by a hand) and the session has not followed yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Describe)]
pub enum Handoff {
    None,
    Started { here: Here },
    Advanced { here: Here },
}

/// Where the reconciler last put the player in its queue: a move it did
/// not make is the player's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Describe)]
pub enum Expect {
    Nowhere,
    At { at: i64 },
}

/// The track and clock the player was last made to agree with: a
/// difference under this one is the player's, and the session is told;
/// under another, the session moved, and the player is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Describe)]
pub enum Against {
    Nothing,
    Clock { here: Here, clock: Clock },
}

/// When the player last said where it is, by the server's clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Describe)]
pub enum Reported {
    Never,
    At { ms: i64 },
}

/// What the device said it can do (`equip`), and on which connection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Describe)]
pub enum Equipped {
    Not,
    Said { epoch: i64, can_volume: bool, family: Option<String> },
}

/// The device's own volume in the session as the reconciler last left it:
/// the entry it last put the player at — or said, or found — and what the
/// player read after. A player that reads otherwise than `seen` was moved by
/// something else; until it has been read after a write, it is `Told`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Describe)]
pub enum Tuned {
    Untold,
    Told { wanted: Vol },
    Settled { wanted: Vol, seen: Vol },
}

/// Who the session had following the device when the two were last
/// reconciled, so that the session moving is told from the house moving.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Describe)]
pub enum Grouped {
    Untold,
    Told { members: Vec<String> },
}

/// One device's reconciler, between two looks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Row)]
#[ark(volatile)]
pub struct Mind {
    #[ark(key)]
    pub device: String,
    /// The connection whose session has been met: another is the moment
    /// of coming back.
    pub met: i64,
    pub alone: Alone,
    pub role: Role,
    pub handoff: Handoff,
    pub expect: Expect,
    pub against: Against,
    pub reported: Reported,
    pub equipped: Equipped,
    pub volume: Tuned,
    pub group: Grouped,
}

impl Mind {
    pub fn new(device: &str) -> Mind {
        Mind {
            device: device.into(),
            met: 0,
            alone: Alone::Quiet,
            role: Role::Silent,
            handoff: Handoff::None,
            expect: Expect::Nowhere,
            against: Against::Nothing,
            reported: Reported::Never,
            equipped: Equipped::Not,
            volume: Tuned::Untold,
            group: Grouped::Untold,
        }
    }
}

/// The ephemeral instance's tables.
#[derive(Schema)]
pub struct Minds {
    pub mind: Mind,
}

/// The track loaded last — a load of a track is a play — and whether that
/// play has been counted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Describe)]
pub enum Loaded {
    Nothing,
    Track { here: Here, counted: bool },
}

/// One device's play counter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Row)]
pub struct Count {
    #[ark(key)]
    pub device: String,
    pub loaded: Loaded,
}

/// The durable instance's tables.
#[derive(Schema)]
pub struct Counts {
    pub count: Count,
}
