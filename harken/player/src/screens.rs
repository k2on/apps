//! The phones' screens as rows (§1.9, the phone views experiment): what the
//! player, its queue, the device picker and the diagnostics' listening
//! numbers draw, made here from the session once — the same reading the
//! reconciler and the desktop make of it (`host::Listening`) — and what each
//! of their controls asks ([`Ask`]), resolved against it. A phone reads one
//! value per screen and sends asks; it draws, and decides nothing.
//!
//! A position is not a row's to keep moving: a row carries where the track
//! was and since when it has been counting ([`NowPlaying::at_ms`],
//! [`NowPlaying::since`]), and the screen ticks it on itself, so a screen at
//! rest asks for nothing.

use harken_domain::host::Listening;
use harken_domain::{Active, Clock, Output, Present, Track};

use crate::{Change, Level, Press, Vol, NOT_PART};

/// A track as a screen draws it: its id (hex), its words, and its cover's
/// URL ("" for none).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct Playing {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art: String,
}

/// Where the sound is, as the player's device button says it, while the
/// phone is part of the session.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct Where {
    pub name: String,
    pub kind: String,
    /// This phone: making the sound, or — the sound nowhere — where the
    /// next press plays.
    pub here: bool,
    /// The device the sound belongs to has gone: "(not answering)".
    #[cfg_attr(feature = "ffi", uniffi(default = false))]
    pub away: bool,
    /// The sound is on its way to it: "connecting…".
    #[cfg_attr(feature = "ffi", uniffi(default = false))]
    pub moving: bool,
}

/// What is playing's volume, as a slider, a mute and the system's panel
/// draw it: `remote` while it is another device's — the phone's buttons are
/// then that device's — and `group` while it is the output's group's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct Volume {
    pub level: i64,
    pub muted: bool,
    pub remote: bool,
    #[cfg_attr(feature = "ffi", uniffi(default = false))]
    pub group: bool,
}

/// The player screen — the mini player, the full player, the queue sheet
/// and the lock screen's volume — as one row: the session's while the
/// phone is part of it, its own player's alone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct NowPlaying {
    pub joined: bool,
    pub now: Option<Playing>,
    pub playing: bool,
    /// Alone, filling a buffer.
    pub buffering: bool,
    pub duration_ms: i64,
    /// Where in the track, as of `since` while it counts; standing otherwise.
    pub at_ms: i64,
    /// The moment `at_ms` was, while the position moves: playing, and not
    /// waiting on a hand-off. None, it stands.
    pub since: Option<i64>,
    /// What plays after this, in order, the first [`UP_NEXT`].
    pub up_next: Vec<Playing>,
    /// Where the sound is, while joined.
    pub on: Option<Where>,
    pub volume: Option<Volume>,
    /// Where another device's scrubber is being dragged to, if one is.
    pub scrubbing: Option<i64>,
}

/// How much of what is to come the queue sheet shows.
pub const UP_NEXT: usize = 100;

impl NowPlaying {
    /// The position at `now`, as the screen ticks it.
    pub fn position(&self, now: i64) -> i64 {
        match self.since {
            Some(since) => self.at_ms + (now - since).max(0),
            None => self.at_ms,
        }
    }
}

/// One row of the picker: a device of the account — or the group that
/// plays — and what it can be asked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct DeviceRow {
    /// The device; "" for the group's row.
    pub id: String,
    pub name: String,
    pub kind: String,
    /// Here and able to make a sound: a row that can be picked.
    pub takeable: bool,
    /// The device the sound still belongs to, whose socket has gone.
    pub away: bool,
    /// The sound is on its way to it.
    pub moving: bool,
    /// Where the sound is — the output and every speaker following it — or,
    /// while it is nowhere, this phone, where the next press plays.
    pub ticked: bool,
    pub me: bool,
    /// Its volume, where anybody can move it: none draws no slider.
    pub volume: Option<i64>,
    pub muted: bool,
    /// What its button does: join the group that plays (true), leave it
    /// (false), or nothing.
    pub join: Option<bool>,
    /// The row of the group as a whole, "Kitchen + 1".
    pub group: bool,
}

/// The device picker: its rows, and the one the cursor starts on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct Picker {
    pub rows: Vec<DeviceRow>,
    pub cursor: i64,
}

/// A line of the diagnostics' listening group.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct Line {
    pub name: String,
    pub value: String,
}

/// What a control of a screen asks, before it is a transition: resolved
/// against the session as it stands ([`Ask::presses`]).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Enum))]
pub enum Ask {
    /// Play or pause.
    Toggle,
    Next,
    Previous,
    /// The scrubber let go at `ms`: a seek, and the scrubber's slot emptied.
    Seek {
        ms: i64,
    },
    /// The scrubber being dragged, or let go (none).
    Scrub {
        ms: Option<i64>,
    },
    /// The `n`th of [`NowPlaying::up_next`].
    UpNext {
        n: i64,
    },
    /// What is playing's volume moved to `level`.
    Volume {
        level: i64,
    },
    /// …moved a step (a volume button).
    Nudge {
        delta: i64,
    },
    /// …muted, or the other way from how it is (none).
    Mute {
        muted: Option<bool>,
    },
    /// The picker's row `id` picked: the sound handed to it.
    Pick {
        id: String,
    },
    /// A row's slider; "" is the group's.
    RowVolume {
        id: String,
        level: i64,
    },
    RowMute {
        id: String,
    },
    /// A row's join or leave button.
    RowJoin {
        id: String,
    },
}

/// What an ask came to while the phone is alone, for its own player: a
/// level put on its media stream, or what the caller does to the player.
pub enum Alone {
    Stream(Vol),
    Player,
}

/// The session's volume rules, as the picker and the player read them.
fn volume(s: &Listening, me: &str) -> Option<(Level, Volume)> {
    let device = |id: &str, remote: bool| {
        let d = s.device(id).filter(|d| d.can_volume)?;
        Some((
            Level::Device { id: id.into() },
            Volume {
                level: d.volume,
                muted: d.muted,
                remote,
                group: false,
            },
        ))
    };
    match s.leader() {
        Some(out) if s.grouped() => s.group_volume(out).map(|level| {
            let v = Volume {
                level,
                muted: s.group_muted(out),
                remote: out != me,
                group: true,
            };
            (Level::Group, v)
        }),
        Some(out) => device(out, out != me),
        None => device(me, false),
    }
}

fn playing(t: &Track, cover: &dyn Fn(&str) -> String) -> Playing {
    Playing {
        id: t.id.hex(),
        title: t.title.clone(),
        artist: t.creator.clone(),
        album: t.album.clone(),
        art: cover(&t.album),
    }
}

/// The device button: the output — a group named for its leader and how
/// many follow it — or, the sound nowhere, this phone, lit.
fn where_(s: &Listening, me: &str, here: &str) -> Where {
    let phone = |moving| Where {
        name: here.into(),
        kind: "phone".into(),
        here: true,
        away: false,
        moving,
    };
    match s.output() {
        Some(Output::Device { id, moving }) if id == me => phone(*moving),
        Some(Output::Device { id, moving }) => {
            let d = s.device(id);
            let named = d.map(|d| d.name.clone()).unwrap_or_else(|| id.clone());
            let n = s.followers(id).len();
            Where {
                name: if n > 0 { format!("{named} + {n}") } else { named },
                kind: d.map(|d| d.kind.clone()).unwrap_or_else(|| "computer".into()),
                here: false,
                away: false,
                moving: *moving,
            }
        }
        Some(Output::Away { id, name }) => Where {
            name: name.clone(),
            kind: "computer".into(),
            here: id == me,
            away: true,
            moving: false,
        },
        Some(Output::Nowhere) | None => phone(false),
    }
}

/// What the player screen draws. `active` is what plays — the session's
/// joined, the player's own alone — `own` the phone's own stream and
/// whether its player is filling a buffer, `here` what this phone is called,
/// `cover` an album's cover's URL by its name.
pub fn now_playing(
    s: &Listening,
    me: &str,
    joined: bool,
    active: Option<&Active>,
    own: (Option<Vol>, bool),
    here: &str,
    cover: &dyn Fn(&str) -> String,
) -> NowPlaying {
    let volume = match joined {
        true => volume(s, me).map(|(_, v)| v),
        false => own.0.map(|v| Volume {
            level: v.level,
            muted: v.muted,
            remote: false,
            group: false,
        }),
    };
    let mut out = NowPlaying {
        joined,
        on: joined.then(|| where_(s, me, here)),
        volume,
        scrubbing: joined.then(|| s.scrub.iter().find(|(d, _)| *d != me).map(|(_, at)| *at)).flatten(),
        ..NowPlaying::default()
    };
    let Some(a) = active.filter(|a| !a.queue.is_empty()) else { return out };
    let at = (a.at.max(0) as usize).min(a.queue.len() - 1);
    let moving = matches!(a.output, Output::Device { moving: true, .. });
    let t = a.now();
    out.now = Some(playing(t, cover));
    out.playing = a.clock.playing();
    out.buffering = !joined && own.1;
    out.duration_ms = t.duration_ms;
    (out.at_ms, out.since) = match a.clock {
        Clock::Playing { at_ms, since } if !moving => (at_ms, Some(since)),
        Clock::Playing { at_ms, .. } | Clock::Paused { at_ms } => (at_ms, None),
    };
    out.up_next = a.queue.iter().skip(at + 1).take(UP_NEXT).map(|t| playing(t, cover)).collect();
    out
}

/// The picker, as the desktop's: a row per device of the account by name —
/// and the output that has gone, drawn and not selectable — and, while the
/// output leads a group, the group first, as a whole; the cursor on the
/// output, or on this phone.
pub fn picker(s: &Listening, me: &str) -> Picker {
    let output = s.output();
    let ticked = output.and_then(|o| o.id()).unwrap_or(me);
    let leader = s.leader();
    let mut rows: Vec<DeviceRow> = s
        .devices
        .iter()
        .map(|d: &Present| {
            let follows = leader.is_some() && d.follows.as_deref() == leader;
            DeviceRow {
                id: d.id.clone(),
                name: d.name.clone(),
                kind: d.kind.clone(),
                takeable: d.can_play,
                away: false,
                moving: s.moving_to(&d.id),
                ticked: d.id == ticked || follows,
                me: d.id == me,
                volume: d.can_volume.then_some(d.volume),
                muted: d.muted,
                join: if follows { Some(false) } else { s.joinable(&d.id).then_some(true) },
                group: false,
            }
        })
        .collect();
    if let Some(Output::Away { id, name }) = output {
        if !rows.iter().any(|r| &r.id == id) {
            rows.push(DeviceRow {
                id: id.clone(),
                name: name.clone(),
                kind: "computer".into(),
                away: true,
                ticked: true,
                me: id == me,
                ..DeviceRow::default()
            });
        }
    }
    rows.sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));
    if let Some(l) = leader.filter(|_| s.grouped()) {
        let head = s.device(l);
        rows.insert(
            0,
            DeviceRow {
                name: format!("{} + {}", head.map_or(l, |h| &h.name), s.followers(l).len()),
                kind: head.map_or("speaker".into(), |h| h.kind.clone()),
                volume: s.group_volume(l),
                muted: s.group_muted(l),
                group: true,
                ..DeviceRow::default()
            },
        );
    }
    let cursor = rows.iter().position(|r| r.ticked && !r.group && r.join != Some(false)).unwrap_or(0) as i64;
    Picker { rows, cursor }
}

/// `m:ss`, or `h:mm:ss` past an hour.
pub fn clock(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    match s >= 3600 {
        true => format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60),
        false => format!("{}:{:02}", s / 60, s % 60),
    }
}

/// The diagnostics' listening numbers, by name: what `D` draws on the
/// desktop. `s` is the session while joined; `stats` the player's.
pub fn numbers(
    s: Option<&Listening>,
    me: &str,
    joined: bool,
    linked: bool,
    epoch: i64,
    stats: &crate::Stats,
    refused: Option<&str>,
    now: i64,
) -> Vec<Line> {
    let mut out: Vec<(String, String)> = vec![
        ("device".into(), if me.is_empty() { "—".into() } else { me.into() }),
        ("part of the session".into(), joined.to_string()),
        ("linked".into(), linked.to_string()),
        ("epoch".into(), epoch.to_string()),
        ("calls".into(), stats.calls.to_string()),
        ("refused".into(), stats.refused.to_string()),
        ("plays recorded".into(), stats.plays.to_string()),
        ("came back playing".into(), stats.came_back.to_string()),
        ("last refusal".into(), refused.or(stats.last_refusal.as_deref()).unwrap_or("—").into()),
    ];
    let a = s.and_then(|s| s.session.as_ref());
    let session = match (s, a) {
        (None, _) => "—".into(),
        (_, None) => "idle".into(),
        (_, Some(a)) => format!(
            "{} of {}, {} at {}",
            a.at + 1,
            a.queue.len(),
            if a.clock.playing() { "playing" } else { "paused" },
            clock(a.position(now))
        ),
    };
    out.push(("session".into(), session));
    let output = match a.map(|a| &a.output) {
        None => "—".into(),
        Some(Output::Device { id, moving }) => format!("{id}{}", if *moving { " (moving)" } else { "" }),
        Some(Output::Away { id, .. }) => format!("{id} (away)"),
        Some(Output::Nowhere) => "nowhere".into(),
    };
    out.push(("output".into(), output));
    let Some(s) = s else { return named(out) };
    for d in &s.devices {
        out.push((
            format!("device {}", d.name),
            format!("{}, {}, {}", d.kind, if d.can_play { "can play" } else { "cannot play" }, d.id),
        ));
        let volume = match (d.can_volume, d.muted) {
            (false, _) => "none".into(),
            (true, true) => format!("{} (muted)", d.volume),
            (true, false) => d.volume.to_string(),
        };
        let family = d.family.as_deref().unwrap_or("none");
        out.push((
            "  volume".into(),
            format!("{volume} · family {family} · follows {}", d.follows.as_deref().unwrap_or("nobody")),
        ));
    }
    let group = s.leader().filter(|_| s.grouped()).map(|l| {
        let level = s.group_volume(l).map_or("none".into(), |v| v.to_string());
        let ids: Vec<&str> = s.followers(l).iter().map(|d| d.id.as_str()).collect();
        format!(
            "{l} + {} · volume {level}{}",
            ids.join(", "),
            if s.group_muted(l) { " (muted)" } else { "" }
        )
    });
    out.push(("group".into(), group.unwrap_or_else(|| "none".into())));
    if s.scrub.is_empty() {
        out.push(("scrub".into(), "none".into()));
    }
    for (id, at) in &s.scrub {
        out.push((format!("scrub {}", if id == me { "(this phone)" } else { id }), clock(*at)));
    }
    named(out)
}

fn named(lines: Vec<(String, String)>) -> Vec<Line> {
    lines.into_iter().map(|(name, value)| Line { name, value }).collect()
}

impl Ask {
    /// The presses this ask is in session `s` for device `me`, joined — in
    /// order; none is nothing to ask. Alone ([`Ask::alone`]) it is the
    /// player's.
    pub fn presses(&self, s: &Listening, me: &str) -> Result<Vec<Press>, &'static str> {
        let loud = |change| volume(s, me).map(|(level, _)| Press::Loud { level, change });
        let row = |id: &str| picker(s, me).rows.into_iter().find(|r| r.id == id).ok_or("no such row");
        let level = |r: &DeviceRow| if r.group { Level::Group } else { Level::Device { id: r.id.clone() } };
        Ok(match self {
            Ask::Toggle => vec![Press::Toggle],
            Ask::Next => vec![Press::Skip { by: 1 }],
            Ask::Previous => vec![Press::Skip { by: -1 }],
            Ask::Seek { ms } => vec![Press::Seek { ms: *ms }, Press::Scrub { at_ms: None }],
            Ask::Scrub { ms } => vec![Press::Scrub { at_ms: *ms }],
            Ask::UpNext { n } => match &s.session {
                Some(a) => vec![Press::Jump { index: a.at + 1 + n }],
                None => vec![],
            },
            Ask::Volume { level } => loud(Change::To { level: *level }).into_iter().collect(),
            Ask::Nudge { delta } => volume(s, me)
                .filter(|(_, v)| v.remote)
                .map(|(level, _)| Press::Loud {
                    level,
                    change: Change::By { delta: *delta },
                })
                .into_iter()
                .collect(),
            Ask::Mute { muted } => {
                let now = volume(s, me).map(|(_, v)| v.muted).unwrap_or(false);
                loud(Change::Mute {
                    muted: muted.unwrap_or(!now),
                })
                .into_iter()
                .collect()
            }
            Ask::Pick { id } => {
                let r = row(id)?;
                match r.group || !r.takeable || s.session.is_none() || (s.outputs(&r.id) && !s.moving_to(&r.id)) {
                    true => vec![],
                    false => vec![Press::Transfer { to: r.id }],
                }
            }
            Ask::RowVolume { id, level: to } => {
                let r = row(id)?;
                r.volume
                    .map(|_| Press::Loud {
                        level: level(&r),
                        change: Change::To { level: *to },
                    })
                    .into_iter()
                    .collect()
            }
            Ask::RowMute { id } => {
                let r = row(id)?;
                r.volume
                    .map(|_| Press::Loud {
                        level: level(&r),
                        change: Change::Mute { muted: !r.muted },
                    })
                    .into_iter()
                    .collect()
            }
            Ask::RowJoin { id } => {
                let r = row(id)?;
                match r.join {
                    Some(true) => vec![Press::Join { device: r.id }],
                    Some(false) => vec![Press::Unjoin { device: r.id }],
                    None => vec![],
                }
            }
        })
    }

    /// What this ask is while the phone is its own session, its stream at
    /// `stream`: a level for it, the player's to do, or nothing (a picker's,
    /// a scrub, which no other device is there to see).
    pub fn alone(&self, stream: Option<Vol>) -> Result<Option<Alone>, &'static str> {
        Ok(match (self, stream) {
            (Ask::Volume { level }, Some(v)) => Some(Alone::Stream(Vol {
                level: (*level).clamp(0, 100),
                muted: v.muted,
            })),
            (Ask::Mute { muted }, Some(v)) => Some(Alone::Stream(Vol {
                level: v.level,
                muted: muted.unwrap_or(!v.muted),
            })),
            (Ask::Toggle | Ask::Next | Ask::Previous | Ask::Seek { .. } | Ask::UpNext { .. }, _) => Some(Alone::Player),
            (Ask::Nudge { .. }, _) => return Err(NOT_PART),
            _ => None,
        })
    }
}
