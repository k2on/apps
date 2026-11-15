//! The player: what plays, where, and the hand on it — the now-playing
//! bar and the full player it grows into (the shell's accessory, which
//! says whether it has grown), every press the host's
//! (`host::Harken`), which sends it to the session as a live call while
//! this device stands in it, and does it to this device's own sound while
//! it is alone.
//!
//! What it draws is the account's listening session while the device is
//! part of it (`HarkenView::Listening`, read on the device from its live
//! space), and otherwise what this device plays on its own, as the host
//! says it ([`PlayerMsg::Own`]). The position is a clock ([`PlayerPage`]'s
//! `at_ms`, `since_ms`, `playing`, `length_ms`) a timeline counts as it
//! draws, so it moves with no message.

use ark_app::{App, Command, Data};
use harken_domain::host::{Kind, Listening};
use harken_domain::{Active, Clock, HarkenChange, HarkenView};

use crate::routes::Route;
use crate::words;
use crate::HarkenRequest;

pub mod view;

#[cfg(test)]
mod tests;

/// The player, as `ark_app` runs it.
pub struct Player;

/// What the player remembers: who and where this device is, whether its
/// devices show, and what plays here alone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// Where covers are fetched from.
    pub server: String,
    /// This device as the session names it; empty alone.
    pub device: String,
    pub linked: bool,
    /// The devices sheet shows.
    pub devices: bool,
    /// What this device plays on its own, while it is not part of the
    /// session.
    pub own: Option<Playing>,
}

/// What plays, as the player draws it: the track, where it is (a clock the
/// timeline counts) and whether it can go back or on.
#[derive(Clone, Debug, Default, PartialEq, ark::twin::Twin)]
pub struct Playing {
    pub key: String,
    pub title: String,
    pub under: String,
    pub art: String,
    pub seed: String,
    pub at_ms: i64,
    /// When `at_ms` was read, the device's clock.
    pub since_ms: i64,
    pub playing: bool,
    pub length_ms: i64,
    pub can_back: bool,
    pub can_forward: bool,
    /// How loud it is where it plays, 0 to 100 (the group's while
    /// joined), and whether that is muted.
    pub volume: i64,
    pub muted: bool,
}

#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub enum PlayerMsg {
    /// Who and where this device is, as the host's session says it.
    Session {
        server: String,
        device: String,
        linked: bool,
    },
    /// What this device plays on its own, as the host's sound says it.
    Own {
        own: Option<Playing>,
    },
    /// Pause what sounds, else play.
    Toggle,
    Next,
    Previous,
    /// The scrubber let go: there.
    Seek {
        at_ms: i64,
    },
    /// The scrubber dragged, still held: where it is, for the others.
    Scrub {
        at_ms: i64,
    },
    /// The sound handed to another device.
    Transfer {
        to: String,
    },
    /// The volume let go at this level.
    Volume {
        level: i64,
    },
    /// The loudspeaker pressed: muted, or not.
    Mute,
    /// The devices sheet, up and away.
    Devices,
    CloseDevices,
}

/// A device in the session, as the devices sheet lists it.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct DeviceRow {
    pub id: String,
    pub name: String,
    /// What kind of device it is, said ("Phone"), and whether the sound is
    /// there ("Playing").
    pub status: String,
    /// The sound is here.
    pub current: bool,
    /// This device.
    pub mine: bool,
}

/// Everything the player shows.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct PlayerPage {
    /// Something to show: the bar is up.
    pub has: bool,
    pub now: Playing,
    pub play_glyph: String,
    pub play_label: String,
    /// Its volume can be turned (`now.volume`).
    pub has_volume: bool,
    /// Part of the session: other devices can be chosen.
    pub joined: bool,
    /// Where the sound is, said: "This Device", or another's name.
    pub output: String,
    pub devices_open: bool,
    pub devices: Vec<DeviceRow>,
}

/// The account's listening session as the views hold it.
pub fn listening(data: &Data<'_, HarkenView>) -> Listening {
    let rows = |k: HarkenView| data.rows::<Vec<u8>>(&k).to_vec();
    Listening::read(&rows(HarkenView::Session), &rows(HarkenView::Devices), &rows(HarkenView::Scrubbing))
}

/// Whether the device is part of the session: linked, and standing in it.
pub fn joined(s: &State, l: &Listening) -> bool {
    s.linked && !s.device.is_empty() && l.device(&s.device).is_some()
}

/// What the session plays, as the player draws it.
pub fn playing_of(server: &str, a: &Active, now: i64) -> Playing {
    let t = a.now();
    let (at_ms, since_ms, playing) = match (&a.output, a.clock) {
        (harken_domain::Output::Device { moving: true, .. }, _) => (a.position(now), now, false),
        (_, Clock::Playing { at_ms, since }) => (at_ms, since, true),
        (_, Clock::Paused { at_ms }) => (at_ms, now, false),
    };
    Playing {
        key: t.id.hex(),
        title: t.title.clone(),
        under: [t.creator.clone(), t.album.clone()]
            .into_iter()
            .filter(|w| !w.is_empty())
            .collect::<Vec<_>>()
            .join(" · "),
        art: words::art_url(server, &t.file),
        seed: t.title.clone(),
        at_ms,
        since_ms,
        playing,
        length_ms: t.duration_ms,
        can_back: true,
        can_forward: (a.at.max(0) as usize) + 1 < a.queue.len(),
        volume: 0,
        muted: false,
    }
}

/// What a device's row says beside its name: what kind of device it is,
/// or that the sound is there.
fn status(kind: &str, current: bool) -> String {
    match (current, Kind::parse(kind)) {
        (true, _) => "Playing".into(),
        (false, Kind::Phone) => "Phone".into(),
        (false, Kind::Speaker) => "Speaker".into(),
        (false, Kind::Computer) => "Computer".into(),
    }
}

impl App for Player {
    type Key = HarkenView;
    type State = State;
    type Msg = PlayerMsg;
    type Change = HarkenChange;
    type Route = Route;
    type Request = HarkenRequest;
    type Screen = PlayerPage;

    fn holds(_: &State) -> Vec<HarkenView> {
        vec![HarkenView::Listening]
    }

    fn update(s: &mut State, msg: PlayerMsg, data: &Data<'_, HarkenView>) -> Vec<Command<Self>> {
        let press = |r: HarkenRequest| vec![Command::Request(r)];
        match msg {
            PlayerMsg::Session { server, device, linked } => {
                (s.server, s.device, s.linked) = (server, device, linked);
                vec![]
            }
            PlayerMsg::Own { own } => {
                s.own = own;
                vec![]
            }
            PlayerMsg::Toggle => press(HarkenRequest::Toggle),
            PlayerMsg::Next => press(HarkenRequest::Skip { by: 1 }),
            PlayerMsg::Previous => press(HarkenRequest::Skip { by: -1 }),
            PlayerMsg::Seek { at_ms } => press(HarkenRequest::Seek { at_ms }),
            PlayerMsg::Scrub { at_ms } => press(HarkenRequest::Scrub { at_ms }),
            PlayerMsg::Transfer { to } => {
                s.devices = false;
                press(HarkenRequest::Transfer { to })
            }
            PlayerMsg::Volume { level } => press(HarkenRequest::Volume { level }),
            PlayerMsg::Mute => {
                let muted = Player::screen(s, data).now.muted;
                press(HarkenRequest::Mute { muted: !muted })
            }
            PlayerMsg::Devices => {
                s.devices = true;
                vec![]
            }
            PlayerMsg::CloseDevices => {
                s.devices = false;
                vec![]
            }
        }
    }

    fn screen(s: &State, data: &Data<'_, HarkenView>) -> PlayerPage {
        let l = listening(data);
        let joined = joined(s, &l);
        let leader = l.leader().map(str::to_string);
        let now = match (joined, &l.session) {
            (true, Some(a)) => Some(Playing {
                volume: leader.as_deref().and_then(|d| l.group_volume(d)).unwrap_or(0),
                muted: leader.as_deref().is_some_and(|d| l.group_muted(d)),
                ..playing_of(&s.server, a, data.now_ms)
            }),
            (true, None) => None,
            (false, _) => s.own.clone(),
        };
        let has_volume = match joined {
            true => leader.as_deref().and_then(|d| l.group_volume(d)).is_some(),
            false => now.is_some(),
        };
        let output = match (joined, l.leader()) {
            (true, Some(id)) if id != s.device => l.device(id).map_or_else(String::new, |d| d.name.clone()),
            _ => "This Device".into(),
        };
        let devices = match joined {
            true => l
                .devices
                .iter()
                .filter(|d| d.can_play)
                .map(|d| DeviceRow {
                    id: d.id.clone(),
                    name: if d.id == s.device { "This Device".into() } else { d.name.clone() },
                    status: status(&d.kind, l.leader() == Some(d.id.as_str())),
                    current: l.leader() == Some(d.id.as_str()),
                    mine: d.id == s.device,
                })
                .collect(),
            false => vec![],
        };
        let playing = now.as_ref().is_some_and(|n| n.playing);
        PlayerPage {
            has: now.is_some(),
            now: now.unwrap_or_default(),
            play_glyph: if playing { "pause" } else { "play" }.into(),
            play_label: if playing { "Pause" } else { "Play" }.into(),
            has_volume,
            joined,
            output,
            devices_open: s.devices,
            devices,
        }
    }
}

/// A change of the session's, by its transition's name and input (what a
/// press and the reconciler say): `listening.<transition>`.
pub fn live(transition: &str, input: &[u8]) -> Option<HarkenChange> {
    HarkenChange::of(&format!("listening.{transition}"), input)
}
