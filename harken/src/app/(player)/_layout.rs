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

use crate::words;
use crate::HarkenRequest;
use crate::Route;

pub mod view;

#[cfg(test)]
mod tests;

/// The player, as `ark_app` runs it.
#[ark_router::screen(view = view::player, bar = view::now_playing)]
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
    /// session, and its queue.
    pub own: Option<Playing>,
    pub own_queue: Vec<QueueRow>,
    /// The queue's sheet shows; the transcript's.
    pub queue: bool,
    pub transcript: bool,
    /// What plays (its id's hex), as the host last said: what the
    /// transcript is read for.
    pub now: String,
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
    /// What this device plays on its own, and its queue, as the host's
    /// sound says it.
    Own {
        own: Option<Playing>,
        queue: Vec<QueueRow>,
    },
    /// What plays now (its id's hex), as the host says it.
    Now {
        key: String,
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
    /// A device put in the group that plays, or, in it, taken out.
    Group {
        device: String,
    },
    /// The queue's sheet, up and away.
    Queue,
    CloseQueue,
    /// A track of the queue played from its start; taken out; moved one
    /// up or one down.
    /// (Each by its row's `at`: where in the queue, as words.)
    Jump {
        at: String,
    },
    Remove {
        at: String,
    },
    MoveUp {
        at: String,
    },
    MoveDown {
        at: String,
    },
    /// The transcript's sheet, up and away.
    Transcript,
    CloseTranscript,
}

/// A track of the queue, as its sheet lists it: where in the queue, its
/// words and cover, and whether it is the one playing.
#[derive(Clone, Debug, Default, PartialEq, ark::twin::Twin)]
pub struct QueueRow {
    pub index: i64,
    /// `index` as words, what its messages name it by.
    pub at: String,
    pub key: String,
    pub title: String,
    pub under: String,
    pub art: String,
    pub seed: String,
    /// The one playing.
    pub now: bool,
    /// Can go up (not the first after what plays), or down.
    pub up: bool,
    pub down: bool,
}

/// A line of the transcript, the one being said now marked.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct Line {
    pub speaker: String,
    pub text: String,
    /// When it is said: from its start to the next line's (its own end,
    /// the last), lit while the player's clock is there.
    pub from_ms: i64,
    pub until_ms: i64,
    /// The player's clock, as its timeline counts it (`Playing`'s), so a
    /// line is lit as it is drawn with no message.
    pub at_ms: i64,
    pub since_ms: i64,
    pub playing: bool,
}

/// Another device of the session, as the group's picker lists it: in the
/// group that plays, or able to join it.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct GroupRow {
    pub id: String,
    pub name: String,
    /// "In the Group", or "".
    pub mark: String,
    pub joined: bool,
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
    /// The devices that can be grouped with what plays: a section of the
    /// devices sheet, none while there are none.
    pub has_group: bool,
    pub group: Vec<GroupRow>,
    /// The queue: what plays, then what plays after it.
    pub queue_open: bool,
    pub queue: Vec<QueueRow>,
    /// Up next, said: "Nothing After This", "3 Tracks".
    pub next: String,
    /// What plays has a transcript: its lines.
    pub has_transcript: bool,
    pub transcript_open: bool,
    pub lines: Vec<Line>,
}

/// A queue's tracks as its sheet lists them, `at` the one playing.
pub fn queue_rows(covers: &words::Covers, queue: &[harken_domain::Track], at: i64) -> Vec<QueueRow> {
    let last = queue.len() as i64 - 1;
    queue
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let i = i as i64;
            QueueRow {
                index: i,
                at: i.to_string(),
                key: t.id.hex(),
                title: t.title.clone(),
                under: [t.creator.clone(), t.album.clone()]
                    .into_iter()
                    .filter(|w| !w.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · "),
                art: covers.track(&t.id.hex()),
                seed: t.title.clone(),
                now: i == at,
                up: i > at + 1,
                down: i > at && i < last,
            }
        })
        .collect()
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
pub fn playing_of(covers: &words::Covers, a: &Active, now: i64) -> Playing {
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
        art: covers.track(&t.id.hex()),
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

    fn holds(s: &State) -> Vec<HarkenView> {
        let transcript = crate::key_of(&s.now).map(|media| HarkenView::Transcript { media });
        [HarkenView::Listening]
            .into_iter()
            .chain(words::Covers::VIEWS)
            .chain(transcript)
            .collect()
    }

    fn update(s: &mut State, msg: PlayerMsg, data: &Data<'_, HarkenView>) -> Vec<Command<Self>> {
        let press = |r: HarkenRequest| vec![Command::Request(r)];
        match msg {
            PlayerMsg::Session { server, device, linked } => {
                (s.server, s.device, s.linked) = (server, device, linked);
                vec![]
            }
            PlayerMsg::Own { own, queue } => {
                (s.own, s.own_queue) = (own, queue);
                vec![]
            }
            PlayerMsg::Now { key } => {
                s.now = key;
                vec![]
            }
            PlayerMsg::Group { device } => {
                let joined = Player::screen(s, data).group.iter().any(|g| g.id == device && g.joined);
                match joined {
                    true => press(HarkenRequest::Unjoin { device }),
                    false => press(HarkenRequest::Join { device }),
                }
            }
            PlayerMsg::Queue => {
                (s.queue, s.devices, s.transcript) = (true, false, false);
                vec![]
            }
            PlayerMsg::CloseQueue => {
                s.queue = false;
                vec![]
            }
            PlayerMsg::Jump { at } | PlayerMsg::Remove { at } | PlayerMsg::MoveUp { at } | PlayerMsg::MoveDown { at }
                if at.parse::<i64>().is_err() =>
            {
                vec![]
            }
            PlayerMsg::Jump { at } => press(HarkenRequest::Jump {
                index: at.parse().unwrap_or(0),
            }),
            PlayerMsg::Remove { at } => press(HarkenRequest::Remove {
                index: at.parse().unwrap_or(0),
            }),
            PlayerMsg::MoveUp { at } => {
                let i: i64 = at.parse().unwrap_or(0);
                press(HarkenRequest::Move { from: i, to: i - 1 })
            }
            PlayerMsg::MoveDown { at } => {
                let i: i64 = at.parse().unwrap_or(0);
                press(HarkenRequest::Move { from: i, to: i + 1 })
            }
            PlayerMsg::Transcript => {
                (s.transcript, s.devices, s.queue) = (true, false, false);
                vec![]
            }
            PlayerMsg::CloseTranscript => {
                s.transcript = false;
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
                (s.devices, s.queue, s.transcript) = (true, false, false);
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
                ..playing_of(&words::Covers::read(&s.server, data), a, data.now_ms)
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
        let covers = words::Covers::read(&s.server, data);
        let queue = match (joined, &l.session) {
            (true, Some(a)) => queue_rows(&covers, &a.queue, a.at),
            (true, None) => vec![],
            (false, _) => s.own_queue.clone(),
        };
        let after = queue
            .iter()
            .filter(|q| q.index > queue.iter().find(|q| q.now).map_or(-1, |q| q.index))
            .count();
        let next = match after {
            0 => "Nothing After This".to_string(),
            1 => "1 Track".to_string(),
            n => format!("{n} Tracks"),
        };
        let group: Vec<GroupRow> = match (joined, l.leader()) {
            (true, Some(leader)) => l
                .devices
                .iter()
                .filter(|d| d.id != leader && d.can_play)
                .filter_map(|d| {
                    let joined = d.follows.as_deref() == Some(leader);
                    (joined || l.joinable(&d.id)).then(|| GroupRow {
                        id: d.id.clone(),
                        name: if d.id == s.device { "This Device".into() } else { d.name.clone() },
                        mark: if joined { "In the Group".into() } else { String::new() },
                        joined,
                    })
                })
                .collect(),
            _ => vec![],
        };
        let cues: Vec<harken_domain::TranscriptEntry> = match crate::key_of(&s.now) {
            Some(media) if now.as_ref().is_some_and(|n| n.key == s.now) => words::rows(data, &HarkenView::Transcript { media }),
            _ => vec![],
        };
        let clock = now.clone().unwrap_or_default();
        let lines: Vec<Line> = cues
            .iter()
            .enumerate()
            .map(|(i, c)| Line {
                speaker: match i.checked_sub(1).map(|b| cues[b].speaker.as_str()) {
                    Some(before) if before == c.speaker => String::new(),
                    _ => c.speaker.clone(),
                },
                text: c.text.clone(),
                from_ms: c.start_ms,
                until_ms: cues.get(i + 1).map_or(c.end_ms.max(c.start_ms + 1), |n| n.start_ms),
                at_ms: clock.at_ms,
                since_ms: clock.since_ms,
                playing: clock.playing,
            })
            .collect();
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
            has_group: !group.is_empty(),
            group,
            queue_open: s.queue,
            queue,
            next,
            has_transcript: !lines.is_empty(),
            transcript_open: s.transcript && !lines.is_empty(),
            lines,
        }
    }
}

/// A change of the session's, by its transition's name and input (what a
/// press and the reconciler say): `listening.<transition>`.
pub fn live(transition: &str, input: &[u8]) -> Option<HarkenChange> {
    HarkenChange::of(&format!("listening.{transition}"), input)
}
