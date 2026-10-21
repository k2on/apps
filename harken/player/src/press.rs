//! What a button, a key, a row or a lock screen asks of the session (§1.9):
//! every press is a transition, read the desktop's way where it needs
//! reading — the one button pauses what sounds and otherwise plays; with
//! nothing queued it plays what this device had.

use ark::format::Encode;
use harken_domain::host::Listening;
use harken_domain::*;
use serde::{Deserialize, Serialize};

/// What a press is answered with while the device is its own session: the
/// player's, then.
pub const NOT_PART: &str = "not part of the session";

/// What a volume control moves.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Level {
    /// The output's group, every member with a volume in proportion.
    Group,
    /// One device of the session.
    Device { id: String },
    /// This phone's own media stream, alone: moved there, in no session.
    Here,
}

/// How a volume control moves what it moves.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Change {
    To { level: i64 },
    By { delta: i64 },
    Mute { muted: bool },
}

/// A press, as the apps say it (JSON, externally tagged: `"Toggle"`,
/// `{"Skip":{"by":1}}`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Press {
    Play,
    Pause,
    /// The one button that is both: pause what sounds, else play.
    Toggle,
    Skip {
        by: i64,
    },
    Seek {
        ms: i64,
    },
    /// A track of the queue, from its start.
    Jump {
        index: i64,
    },
    /// A track pressed: this queue, from here.
    Start {
        queue: Vec<Track>,
        at: i64,
        position_ms: i64,
        from: String,
    },
    /// Put these after what is playing.
    Next {
        tracks: Vec<Track>,
    },
    /// Put these at the end of the queue.
    Append {
        tracks: Vec<Track>,
    },
    Remove {
        index: i64,
    },
    Move {
        from: i64,
        to: i64,
    },
    /// Where this device's scrubber is being dragged to, or none.
    Scrub {
        at_ms: Option<i64>,
    },
    Transfer {
        to: String,
    },
    /// The sound to this device, if it is anywhere else (what a car asks
    /// before it plays).
    Here,
    Loud {
        level: Level,
        change: Change,
    },
    Join {
        device: String,
    },
    Unjoin {
        device: String,
    },
}

/// A transition by name and its input.
pub type Call = (&'static str, Vec<u8>);

impl Press {
    /// The transition this press is in session `s` for device `me`, whose
    /// player holds `local` — its queue, where, and where from — for a play
    /// with nothing queued. `Ok(None)` is nothing to ask; `Err`, why not.
    pub fn transition(&self, s: &Listening, me: &str, local: Option<(Vec<Track>, usize, String)>) -> Result<Option<Call>, &'static str> {
        let a = s.session.as_ref();
        let start_local = || -> Result<Option<Call>, &'static str> {
            let (queue, at, from) = local.filter(|(q, ..)| !q.is_empty()).ok_or("nothing to play")?;
            let at = at.min(queue.len() - 1) as i64;
            Ok(Some((
                "start",
                Start {
                    at,
                    from,
                    position: 0,
                    queue,
                }
                .to_bytes(),
            )))
        };
        let play = || Ok(Some(("play", ().to_bytes())));
        let to = |l: i64| l.clamp(0, 100);
        let by = |d: i64| d.clamp(-100, 100);
        Ok(Some(match self.clone() {
            Press::Toggle if s.sounding() => ("pause", ().to_bytes()),
            Press::Toggle | Press::Play if a.is_some() => return play(),
            Press::Toggle | Press::Play => return start_local(),
            Press::Pause => ("pause", ().to_bytes()),
            Press::Skip { by } => (
                "skip",
                Skip {
                    by: if by < 0 { -1 } else { 1 },
                }
                .to_bytes(),
            ),
            Press::Seek { ms } => ("seek", Seek { ms: ms.max(0) }.to_bytes()),
            Press::Jump { index } => ("jump", Jump { index }.to_bytes()),
            Press::Start {
                queue,
                at,
                position_ms,
                from,
            } => (
                "start",
                Start {
                    at,
                    from,
                    position: position_ms.max(0),
                    queue,
                }
                .to_bytes(),
            ),
            Press::Next { tracks } => ("enqueue", Enqueue { next: true, tracks }.to_bytes()),
            Press::Append { tracks } => ("enqueue", Enqueue { next: false, tracks }.to_bytes()),
            Press::Remove { index } => ("remove", Remove { index }.to_bytes()),
            Press::Move { from, to } => ("move", Move { from, to }.to_bytes()),
            Press::Scrub { at_ms } => (
                "scrub",
                Scrub {
                    at_ms: at_ms.map(|m| m.max(0)),
                }
                .to_bytes(),
            ),
            Press::Transfer { to } => ("transfer", Transfer { to }.to_bytes()),
            Press::Here if a.is_none() || s.outputs(me) => return Ok(None),
            Press::Here => ("transfer", Transfer { to: me.into() }.to_bytes()),
            Press::Join { device } => ("join", Member { device }.to_bytes()),
            Press::Unjoin { device } => ("unjoin", Member { device }.to_bytes()),
            Press::Loud { level: Level::Here, .. } => return Err(NOT_PART),
            Press::Loud { level: Level::Group, change } => match change {
                Change::To { level } => ("set_group_volume", GroupVolume { level: to(level) }.to_bytes()),
                Change::By { delta } => ("nudge_group_volume", GroupNudge { delta: by(delta) }.to_bytes()),
                Change::Mute { muted } => ("set_group_mute", GroupMute { muted }.to_bytes()),
            },
            Press::Loud {
                level: Level::Device { id },
                change,
            } => match change {
                Change::To { level } => (
                    "set_volume",
                    SetVolume {
                        device: id,
                        level: to(level),
                    }
                    .to_bytes(),
                ),
                Change::By { delta } => (
                    "nudge_volume",
                    NudgeVolume {
                        delta: by(delta),
                        device: id,
                    }
                    .to_bytes(),
                ),
                Change::Mute { muted } => ("set_mute", SetMute { device: id, muted }.to_bytes()),
            },
        }))
    }
}
