//! Home Assistant's media players, as devices in the listening session.
//!
//! A Sonos is a *device*, not a client. The session (`harken/domain`'s
//! `listening.*`, live state over volatile tables) already describes one — it
//! stands in an account's session, is handed the sound, and says what
//! happens to it — and nothing in that says the far end has to be
//! somebody's screen. So nothing here extends the session; what is here is
//! the thing that is a device on a speaker's behalf: a connection to the
//! hub in this process ([`ha`]), a volatile machine of its own saying in
//! its `Hello` that it is a speaker, standing in each account's session for
//! each configured speaker, reconciled against the state by **the same
//! reconciler a phone's player is** (`harken-player`, S3b), which says what
//! the speaker does as the output's own transitions.
//!
//! **Home Assistant rather than Sonos directly**, and not for convenience: you
//! do not get Sonos, you get every `media_player` entity in the house. A
//! Chromecast, a television, an AirPlay receiver and a speaker group are all
//! the same few service calls, where talking UPnP would buy one make of
//! speaker and a discovery problem.
//!
//! What is here is the speaker as a [`harken_player::Sound`]: each command
//! a service call ([`Act`]), and Home Assistant's polls read into what the
//! speaker is doing and what happened to it — a hand-off fetched, a seek it
//! dropped, a blink that is not a stall, a position dated before the last
//! command, a URL spelt back another way, a speaker that went quiet — plus
//! which account holds a speaker. [`Speakers`] owns no socket and no
//! thread: it is handed the session and what the house said, and hands
//! back [`Act`]s, service calls and transitions to call as the speaker.
//!
//! **The queue is harken's and the position is the speaker's.** The whole list
//! is enqueued, so the physical buttons and the Sonos app keep working; where
//! the speaker *is* in that list is read back from what it says it is
//! playing, and said as the output's own `advance` or `jump`. Somebody
//! skipping on the speaker itself moves every phone; somebody pausing it by
//! hand is `stalled`, and resuming it is `play`. Nothing is reported for a
//! few polls after the bridge itself told the speaker something
//! ([`QUIET`]): a house a second behind its own speaker would otherwise
//! report the bridge's own pause back to it as somebody's hand.
//!
//! **A speaker's volume and its group are the session's too** (the "Volume
//! and groups" section of `docs/plan-live.md`), reconciled the same way and
//! by the same rule: what moved in the session since the bridge last looked
//! is told to the house (`volume_set`, `volume_mute`, `join`, `unjoin`), and
//! what differs in the house while the session stood still — outside
//! [`QUIET`] — was somebody's hand, said as the speaker (`report_volume`,
//! `report_group`). What a speaker can do is read off what Home Assistant
//! says it supports ([`VOLUME_SET`], [`GROUPING`]) and said once as its
//! `equip`. A member of a group is never handed a queue: it plays whatever
//! its leader plays, which is the house's to arrange.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use harken_player::{Caps, Event, Followers, Heard, Place, Reconciler, Say, View, Vol};

use crate::domain::host::url;
use crate::domain::host::{Kind, Listening};
use crate::domain::{Output, Track};

pub type Args = Vec<u8>;

/// The inputs of the session's transitions the bridge and its tests say,
/// each the domain's input type, encoded; and a position in one moved on.
pub mod input {
    use crate::domain::*;
    use ark::format::Encode;

    pub fn none() -> Vec<u8> {
        ().to_bytes()
    }

    pub fn start(queue: &[Track], at: usize, position_ms: i64, from: &str) -> Vec<u8> {
        Start {
            at: at as i64,
            from: from.into(),
            position: position_ms,
            queue: queue.to_vec(),
        }
        .to_bytes()
    }

    pub fn transfer(to: &str) -> Vec<u8> {
        Transfer { to: to.into() }.to_bytes()
    }

    pub fn at(at_ms: i64) -> Vec<u8> {
        At { at_ms }.to_bytes()
    }

    pub fn advance(from_track: usize) -> Vec<u8> {
        Advance {
            from_track: from_track as i64,
        }
        .to_bytes()
    }

    pub fn enqueue(tracks: &[Track], next: bool) -> Vec<u8> {
        Enqueue {
            next,
            tracks: tracks.to_vec(),
        }
        .to_bytes()
    }

    pub fn remove(index: usize) -> Vec<u8> {
        Remove {
            index: index as i64,
        }
        .to_bytes()
    }

    pub fn report_volume(level: i64, muted: bool) -> Vec<u8> {
        ReportVolume { level, muted }.to_bytes()
    }

    pub fn member(device: &str) -> Vec<u8> {
        Member {
            device: device.into(),
        }
        .to_bytes()
    }

    pub fn set_group_volume(level: i64) -> Vec<u8> {
        GroupVolume { level }.to_bytes()
    }

    pub fn record_play(track: &Track, from: &str) -> Vec<u8> {
        RecordPlay {
            track: track.id,
            title: track.title.clone(),
            creator: track.creator.clone(),
            album: track.album.clone(),
            duration_ms: track.duration_ms,
            file: track.file.clone(),
            from: from.into(),
        }
        .to_bytes()
    }

    /// The position an input says — `started`'s and `stalled`'s `at_ms`,
    /// `seek`'s `ms`, `report_position`'s `at_ms` — moved on by `by`.
    pub fn later(transition: &str, input: &[u8], by: i64) -> Vec<u8> {
        match transition {
            "started" | "stalled" => {
                At::from_bytes(input).map_or(input.to_vec(), |a| at(a.at_ms + by))
            }
            "seek" => Seek::from_bytes(input)
                .map_or(input.to_vec(), |a| Seek { ms: a.ms + by }.to_bytes()),
            "report_position" => Report::from_bytes(input).map_or(input.to_vec(), |r| {
                Report {
                    at_ms: r.at_ms + by,
                    since: r.since,
                }
                .to_bytes()
            }),
            _ => input.to_vec(),
        }
    }

    /// The position an input says, as [`later`] reads it.
    pub fn ms(transition: &str, input: &[u8]) -> Option<i64> {
        match transition {
            "started" | "stalled" => At::from_bytes(input).ok().map(|a| a.at_ms),
            "seek" => Seek::from_bytes(input).ok().map(|a| a.ms),
            "report_position" => Report::from_bytes(input).ok().map(|r| r.at_ms),
            _ => None,
        }
    }
}

/// How many polls a freshly handed speaker may answer with something that is
/// not what it was handed before the session stops claiming it.
///
/// One tick is a second, so five is five seconds of fetching — longer than a
/// Sonos on a LAN takes and long enough for a Chromecast to wake up, and
/// short enough that a speaker somebody sent elsewhere is let go while they
/// are still looking at the screen they did it from.
pub const SETTLE: u8 = 5;

/// How many polls a speaker that *was* playing ours may answer with something
/// else, or not answer at all, before it is let go.
///
/// Not zero, which it was. Between two tracks Home Assistant can report a
/// player with no `media_content_id` for a poll, a Sonos integration that
/// reloads makes the entity `unavailable` for a moment, and a proxy in front
/// of the house can refuse one request in a hundred — and each of those, read
/// as "it has been sent somewhere else", was a hand-off released and the
/// speaker paused in the middle of a track, for no reason anyone could see.
/// Three seconds is longer than any of those and shorter than a doorbell.
pub const LAPSE: u8 = 3;

/// How many polls after the bridge told a speaker something it reports no
/// difference between the speaker and the session as somebody's hand: the
/// house catching up with its own speaker is not somebody pressing pause.
pub const QUIET: u8 = 2;

/// How many times a speaker that started a hand-off at the beginning of the
/// track is told again to go where the track was, before it is believed.
/// The seek that follows the enqueue reaches a speaker that may not have read
/// a byte of the stream yet, and a Sonos then drops it: the track starts from
/// the beginning, and the session — told `started` at 0 — went back with it.
pub const SEEKS: u8 = 3;

/// How far a playing speaker may be from where the session counts it
/// before one is told the other ([`SYNC_MS`], the same bound every output
/// is held to): the speaker is right about where it is, so the session is
/// told (`report_position`, said as the speaker, and nothing told to any
/// player); and the session is right about where somebody asked it to be,
/// so the speaker is told (`media_seek`) when it is the session that moved.
pub use crate::domain::host::SYNC_MS;

/// How far a playing speaker may be from the session before what moved it
/// was somebody's hand on the speaker — a seek in the Sonos app — said as
/// the speaker's own `seek` rather than as where it has drifted to: every
/// player's bound.
pub use harken_player::DRIFT_MS;

/// How many polls in a row a speaker handed the sound must say it is not
/// playing before that is a stall. One is a blink: a Sonos between a seek
/// and the stream, or Home Assistant catching up with a state it had for a
/// moment, says `paused` or `idle` for a poll and plays on — and read as a
/// stall, that blink paused every bar where the house last dated the
/// position, the beginning of the track, and the next poll played it on
/// from there.
pub const STALL: u8 = 2;

/// How many polls after the bridge told a speaker something before a
/// position the house dated *before* that is believed. The house dates a
/// position at a change of state and not while it plays on, and only when
/// it has moved more than a second and a half from what it would have
/// counted; so a speaker sought by a little, or one the house has not heard
/// from yet, goes on showing a date from before the command. For a moment
/// that is a position from before the seek — read as now, the beginning of
/// the track — and after it, the house's own count, which is right.
pub const STALE: u8 = 4;

/// How many polls a speaker handed the sound may spend fetching it —
/// saying it plays nothing, buffering, idle while asked to play, or playing
/// with a position nobody can place yet — before the bridge stops waiting:
/// it says `started` at its best guess if the speaker is on the track, and
/// lets the session go if it never got there.
pub const WAIT: u8 = 15;

/// `MediaPlayerEntityFeature.VOLUME_SET` in Home Assistant's
/// `homeassistant/components/media_player/const.py`: the player takes
/// `media_player.volume_set`. A bit of the entity's `supported_features`.
pub const VOLUME_SET: u64 = 4;

/// `MediaPlayerEntityFeature.GROUPING`, the same file: the player takes
/// `media_player.join` and `media_player.unjoin`, and says who it is
/// grouped with as `group_members`.
pub const GROUPING: u64 = 524_288;

/// What Home Assistant says of a player's volume and its group, and what
/// it supports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Speaker {
    /// `volume_level`, 0 to 1 on the wire, as 0 to 100. `None` where the
    /// house says none, as an entity that is off does.
    pub volume: Option<i64>,
    /// `is_volume_muted`.
    pub muted: bool,
    /// [`VOLUME_SET`] is among its `supported_features`.
    pub can_volume: bool,
    /// The family it groups in: its integration, where it supports
    /// [`GROUPING`] — `None` where it does not.
    pub family: Option<String>,
    /// `group_members`: everything it plays with, the leader first, itself
    /// included. Empty, or itself alone, for a player in no group.
    pub members: Vec<String>,
}

impl Speaker {
    /// Whether it leads whatever group it is in: first of its members, or
    /// in none.
    fn leads(&self, entity: &str) -> bool {
        self.members.first().is_none_or(|l| l == entity)
    }
}

/// How Home Assistant describes a player, pared down to what decides
/// anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playing {
    /// `playing`, `paused`, `idle`, `off`. Anything that is not `playing` is
    /// not playing, which is the only distinction that matters.
    pub state: String,
    /// What it says it is playing, which is how it says where it is in the
    /// queue it was given. Empty when it is playing nothing.
    pub url: String,
    /// `media_position`: where it was **when the house last updated it**,
    /// which is not now. Home Assistant refreshes a player's position on a
    /// change of state — a play, a pause, a seek, a new track — and not
    /// while it plays on, so a playing Sonos says the same position for the
    /// whole of a track.
    pub position_ms: i64,
    /// `media_position_updated_at`, on the server's clock: the moment
    /// `position_ms` was true. `None` where the house did not say, and then
    /// a playing speaker's position cannot be dated and is not used.
    pub updated_ms: Option<i64>,
    /// `media_duration`, where the house says one: no count runs past it.
    pub duration_ms: Option<i64>,
    /// `queue_position`, which the Sonos integration says beside the URL:
    /// where in the speaker's own queue it is, from 1. Read only where the
    /// URL names the queue rather than the track ([`Holding::index`]).
    pub queue_position: Option<usize>,
    /// How far either way `updated_ms` may be from the moment the house
    /// stamped, on the server's clock: what is still unknown of the house's
    /// clock ([`when::Skew`]). A position dated within this of a command
    /// may be from either side of it.
    pub slack_ms: i64,
}

/// What a state says of the sound, which is all a state decides here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Says {
    /// `playing`.
    Playing,
    /// `paused`, `idle`, `off`, `standby`: nothing is coming out of it.
    Stopped,
    /// `buffering`, `on`, anything else: on its way to one or the other,
    /// which is neither a stall nor a sound to count a position from.
    Between,
}

impl Playing {
    pub fn says(&self) -> Says {
        match self.state.as_str() {
            "playing" => Says::Playing,
            "paused" | "idle" | "off" | "standby" => Says::Stopped,
            _ => Says::Between,
        }
    }

    /// Whether the house dated its position after `told` (the server's
    /// clock), as far as the house's clock is known.
    pub fn dated_after(&self, told: i64) -> bool {
        self.updated_ms.is_some_and(|u| u >= told - self.slack_ms)
    }

    /// Where it is at `now` (the server's clock): the position the house
    /// gave, counted on from when it was true while it plays, and as it is
    /// while it does not. `None` for a playing speaker whose position the
    /// house did not date — nothing is reported off a value nobody can
    /// place in time.
    pub fn position_at(&self, now: i64) -> Option<i64> {
        let at = self.position_ms.max(0);
        if self.state != "playing" {
            return Some(at);
        }
        let counted = at + (now - self.updated_ms?).max(0);
        Some(match self.duration_ms {
            Some(d) if d > 0 => counted.min(d),
            _ => counted,
        })
    }
}

/// What the bridge wants done. The thread performs these; this module never
/// sees one happen.
#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    /// Put this list on, from `at`, at this point. The thread clears the
    /// queue, puts the track on with `play_media` and `enqueue: play`, seeks
    /// it, pauses it when the session is paused, and then pushes the rest as
    /// `enqueue: add` a few at a time between polls ([`ha::PUSH`]) — which
    /// is what leaves the speaker's own next and previous working.
    Start {
        entity: String,
        urls: Vec<String>,
        at: usize,
        position_ms: i64,
        playing: bool,
    },
    /// Tracks put into the queue the speaker holds, without touching what
    /// it plays: `play_media` with `enqueue: next` for each, last first, so
    /// they land in order after the item playing — or `enqueue: add`, each
    /// at the end.
    Enqueue {
        entity: String,
        urls: Vec<String>,
        next: bool,
    },
    /// `media_player.<verb>` on `entity`, with nothing else to say.
    Verb {
        entity: String,
        verb: &'static str,
        why: &'static str,
    },
    Seek {
        entity: String,
        position_ms: i64,
    },
    /// `media_player.volume_set`, at a level of 0 to 100.
    Volume {
        entity: String,
        level: i64,
    },
    /// `media_player.volume_mute`.
    Mute {
        entity: String,
        muted: bool,
    },
    /// `media_player.join`: these follow `leader`.
    Join {
        leader: String,
        members: Vec<String>,
    },
    /// `media_player.unjoin`: it leaves whatever group it is in.
    Unjoin {
        entity: String,
    },
    /// A transition of `key`'s session, called as the speaker standing in it.
    Call {
        key: String,
        entity: String,
        transition: &'static str,
        input: Args,
        why: String,
        /// The moment (the server's clock) the position in `input` — its
        /// `at_ms` or `ms` — was where a playing speaker was: said later, it
        /// is counted on to when it is said, so the session is re-anchored at
        /// where the speaker is *then* and not where it was a few requests
        /// ago. `None` for an input that is not a moving position.
        as_of: Option<i64>,
    },
}

/// A URL as an address rather than as a spelling: what two spellings of one
/// file have in common, so that what a speaker says it is playing can be
/// matched with what it was handed.
///
/// A speaker does not say back the bytes it was given. The Sonos
/// integration reports the speaker's own `TrackURI`, and between harken's
/// `%20` and that lie a speaker's firmware, an XML envelope each way and
/// Home Assistant: a space or a parenthesis may come back decoded, an
/// escape in lower case, an `&` as `&amp;`, the scheme and host in another
/// case, a default port written out. Every one of those is the same file
/// to the server that serves it, and compared byte for byte every one of
/// them was "not ours" — the speaker let go, paused, on the first track.
///
/// So: HTML entities decoded (twice escaped is still `&`), the scheme and
/// the host lower-cased and a default port dropped, a fragment dropped, and
/// the path and query percent-decoded. `x-sonos-http:` and `x-sonos-https:`,
/// which Sonos writes for a stream it fetches itself, are read as `http:`
/// and `https:`. Anything that is not a URL with a host is itself, decoded.
pub fn address(url: &str) -> String {
    let mut s = url.trim().to_string();
    for _ in 0..3 {
        let decoded = s
            .replace("&amp;", "&")
            .replace("&#38;", "&")
            .replace("&#x26;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&apos;", "'");
        if decoded == s {
            break;
        }
        s = decoded;
    }
    let decode = |part: &str| -> String {
        let b = part.as_bytes();
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            let hex = |c: u8| (c as char).to_digit(16);
            match (
                b[i],
                b.get(i + 1).copied().and_then(hex),
                b.get(i + 2).copied().and_then(hex),
            ) {
                (b'%', Some(hi), Some(lo)) => {
                    out.push((hi * 16 + lo) as u8);
                    i += 3;
                }
                (c, _, _) => {
                    out.push(c);
                    i += 1;
                }
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    };
    let Some((scheme, rest)) = s.split_once("://") else {
        return decode(&s);
    };
    let scheme = match scheme.to_ascii_lowercase().as_str() {
        "x-sonos-http" => "http".to_string(),
        "x-sonos-https" => "https".to_string(),
        other => other.to_string(),
    };
    let rest = rest.split('#').next().unwrap_or_default();
    let cut = rest.find(['/', '?']).unwrap_or(rest.len());
    let (host, tail) = rest.split_at(cut);
    let mut host = host.to_ascii_lowercase();
    for (sch, port) in [("http", ":80"), ("https", ":443")] {
        if scheme == sch && host.ends_with(port) {
            host.truncate(host.len() - port.len());
        }
    }
    let tail = if tail.is_empty() { "/" } else { tail };
    format!("{scheme}://{host}{}", decode(tail))
}

/// What one speaker was handed, by whose session, and what the house has
/// said of it since: the reading, not the rules.
#[derive(Debug, Clone)]
struct Holding {
    key: String,
    /// The queue as handed, and where it was taken from.
    tracks: Vec<Track>,
    from: String,
    /// …as the URLs the speaker was given, so what it reports back can be
    /// matched to an index without re-deriving anything…
    urls: Vec<String>,
    /// …each as an [`address`], which is what a report is matched against.
    addresses: Vec<String>,
    /// Where in that list it was handed from: until it says it is playing
    /// that, it is fetching, and nothing else it says is news.
    handed_at: usize,
    /// Whether it has said it is playing what it was handed.
    started: bool,
    /// Where in the queue it last said it is.
    at: usize,
    /// What it was last told: playing or not.
    playing: bool,
    /// How many more polls it may answer with something that is not ours,
    /// or not answer at all, before it is let go: [`SETTLE`] on a hand-off,
    /// [`LAPSE`] again after every poll that names one of ours.
    settling: u8,
    /// [`QUIET`] after the bridge told it something.
    quiet: u8,
    /// Where it last said it was, and when.
    heard: Option<(i64, i64)>,
    /// Where it measurably is as of this poll — a position the house dated
    /// since it was last told anything — which only a poll's own look reads.
    fresh: Option<i64>,
    /// Where in the track it was handed from: what it is told to seek to
    /// until it is there.
    from_ms: i64,
    /// Seeks to `from_ms` said since the hand-off ([`SEEKS`] at most), and
    /// polls to wait before the next.
    sought: u8,
    seek_wait: u8,
    /// When it was handed the queue, and when it was last told anything that
    /// moves where it is (the queue, a seek, a play or a pause) — the server's
    /// clock — and how many polls since: a position dated before that is
    /// from before it ([`STALE`]).
    handed_ms: i64,
    told_ms: i64,
    since_told: u8,
    /// Polls spent fetching the hand-off ([`WAIT`]).
    waited: u8,
    /// Polls in a row it has said it is not playing while it should be
    /// ([`STALL`]), and where it was counted to at the first of them.
    silent: u8,
    stopped_at: Option<i64>,
    /// When a poll last heard it playing, the server's clock: a stopped
    /// position the house dated before that is not where it stopped.
    sounding_ms: Option<i64>,
}

impl Holding {
    /// Where in the queue the speaker says it is: the track whose address
    /// is the one it reports ([`address`]), or — where what it reports names
    /// the speaker's own queue (`x-rincon-queue:…`) rather than a track —
    /// its `queue_position` counted from where it was handed, since its
    /// queue is ours from there on.
    fn index(&self, p: &Playing) -> Option<usize> {
        if p.url.is_empty() {
            return None;
        }
        if p.url.starts_with("x-rincon-queue:") {
            let at = self.handed_at + p.queue_position?.checked_sub(1)?;
            return (at < self.urls.len()).then_some(at);
        }
        let reported = address(&p.url);
        self.addresses.iter().position(|a| *a == reported)
    }

    /// Where it is counted to be at `now`, from where it last said.
    fn believed(&self, now: i64) -> i64 {
        match self.heard {
            Some((at_ms, when)) if self.playing => at_ms + (now - when).max(0),
            Some((at_ms, _)) => at_ms,
            None => self.from_ms,
        }
    }

    /// It was told something that moves where it is, at `now`.
    fn told(&mut self, now: i64) {
        self.quiet = QUIET;
        self.told_ms = now;
        self.since_told = 0;
        self.silent = 0;
    }
}

/// What the house last said of a speaker's volume and group, how many more
/// polls a difference is the house catching up with what it was told, and
/// what that was.
#[derive(Debug, Clone, Default)]
struct Tuning {
    reading: Option<Speaker>,
    quiet: u8,
    volume: Option<Vol>,
    members: Option<Vec<String>>,
}

/// One speaker: what it holds and for whom, its volume and group, and what
/// happened to it at the last poll.
#[derive(Debug, Default)]
struct Room {
    held: Option<Holding>,
    tuning: Tuning,
    events: Vec<(String, Event)>,
}

/// One speaker as one session's reconciler drives it: a
/// [`harken_player::Sound`] whose commands are service calls ([`Act`]).
struct Hand<'a> {
    entity: &'a str,
    key: &'a str,
    media: &'a str,
    now: i64,
    room: &'a mut Room,
    acts: &'a RefCell<Vec<Act>>,
}

impl Hand<'_> {
    fn held(&self) -> Option<&Holding> {
        self.room.held.as_ref().filter(|h| h.key == self.key)
    }

    fn held_mut(&mut self) -> Option<&mut Holding> {
        let key = self.key;
        self.room.held.as_mut().filter(|h| h.key == key)
    }

    fn act(&self, a: Act) {
        self.acts.borrow_mut().push(a);
    }

    fn verb(&self, verb: &'static str, why: &'static str) {
        self.act(Act::Verb {
            entity: self.entity.into(),
            verb,
            why,
        });
    }

    /// The whole queue handed to the speaker, from `at`, at `position_ms`.
    fn hand(&mut self, tracks: &[Track], from: &str, at: usize, position_ms: i64, playing: bool) {
        let urls: Vec<String> = tracks.iter().map(|t| url(self.media, &t.file)).collect();
        self.room.held = Some(Holding {
            key: self.key.into(),
            tracks: tracks.to_vec(),
            from: from.into(),
            addresses: urls.iter().map(|u| address(u)).collect(),
            urls: urls.clone(),
            handed_at: at,
            started: false,
            at,
            playing,
            settling: SETTLE,
            quiet: QUIET,
            heard: None,
            fresh: None,
            from_ms: position_ms,
            sought: 0,
            seek_wait: 0,
            handed_ms: self.now,
            told_ms: self.now,
            since_told: 0,
            waited: 0,
            silent: 0,
            stopped_at: None,
            sounding_ms: None,
        });
        self.act(Act::Start {
            entity: self.entity.into(),
            urls,
            at,
            position_ms,
            playing,
        });
    }

    /// `tracks` is the queue the speaker holds with tracks put in after
    /// `at`, at its end, or both — the same tracks in the same order around
    /// them — and nothing else moved: those tracks, as the speaker's own
    /// queue takes them. False for any other change, which is a fresh
    /// hand-off, and for a queue longer than what the speaker was handed
    /// ([`ha::WINDOW`]).
    fn enqueued(&mut self, tracks: &[Track], at: usize) -> bool {
        let media = self.media;
        let entity = self.entity.to_string();
        let Some(h) = self.held_mut() else {
            return false;
        };
        let urls: Vec<String> = tracks.iter().map(|t| url(media, &t.file)).collect();
        let old = &h.urls;
        if !h.started
            || at != h.at
            || urls.len() <= old.len()
            || old.len() - h.handed_at > ha::WINDOW
            || urls[..=at] != old[..=at]
        {
            return false;
        }
        let (was, now) = (&old[at + 1..], &urls[at + 1..]);
        let Some(j) = (0..=now.len() - was.len()).find(|j| now[*j..*j + was.len()] == *was) else {
            return false;
        };
        let (next, last) = (now[..j].to_vec(), now[j + was.len()..].to_vec());
        h.addresses = urls.iter().map(|u| address(u)).collect();
        h.urls = urls;
        h.tracks = tracks.to_vec();
        h.quiet = QUIET;
        for (urls, next) in [(next, true), (last, false)] {
            if !urls.is_empty() {
                self.act(Act::Enqueue {
                    entity: entity.clone(),
                    urls,
                    next,
                });
            }
        }
        true
    }

    fn tuned(&self) -> Option<&Speaker> {
        self.room.tuning.reading.as_ref()
    }

    /// Who plays with it now, as it was last told while the house catches up.
    fn members(&self) -> Vec<String> {
        let t = &self.room.tuning;
        match (&t.members, &t.reading) {
            (Some(m), _) if t.quiet > 0 => m.clone(),
            (_, Some(r)) if r.leads(self.entity) => r
                .members
                .iter()
                .filter(|m| *m != self.entity)
                .cloned()
                .collect(),
            _ => vec![],
        }
    }
}

impl harken_player::Sound for Hand<'_> {
    fn heard(&self) -> Heard {
        let Some(h) = self.held() else {
            return Heard {
                queue: vec![],
                at: None,
                playing: false,
                ready: false,
                place: Place::Unknown,
                ended: false,
                from: String::new(),
            };
        };
        Heard {
            queue: h.tracks.iter().map(|t| t.id).collect(),
            at: Some(if h.started { h.at } else { h.handed_at }),
            playing: h.playing,
            ready: h.started,
            place: match (h.started, h.fresh) {
                (false, _) => Place::Unknown,
                (true, Some(ms)) => Place::Measured { ms, at: self.now },
                (true, None) => Place::Believed(h.believed(self.now)),
            },
            ended: false,
            from: h.from.clone(),
        }
    }

    fn take(&mut self) -> Option<Event> {
        let i = self.room.events.iter().position(|(k, _)| k == self.key)?;
        Some(self.room.events.remove(i).1)
    }

    fn tracks(&self) -> Vec<Track> {
        self.held().map(|h| h.tracks.clone()).unwrap_or_default()
    }

    fn load(&mut self, queue: &[Track], at: usize, position_ms: i64, playing: bool, from: &str) {
        // The same place in a queue with tracks put after the one playing,
        // or at its end: the speaker's own queue gets them, and plays on.
        // Anything else — a track taken out, or moved — is a hand-off from
        // where the track is: the house has no call for either that a Sonos
        // and a Chromecast both take.
        if !self.enqueued(queue, at) {
            self.hand(queue, from, at, position_ms, playing);
        }
    }

    fn jump(&mut self, at: usize, position_ms: i64) {
        let Some((tracks, from, playing)) = self
            .held()
            .map(|h| (h.tracks.clone(), h.from.clone(), h.playing))
        else {
            return;
        };
        self.hand(&tracks, &from, at, position_ms, playing);
    }

    fn resume(&mut self) {
        let now = self.now;
        let Some(h) = self.held_mut() else { return };
        h.heard = Some((h.believed(now), now));
        h.playing = true;
        h.told(now);
        self.verb("media_play", "the session plays");
    }

    fn pause(&mut self) {
        let now = self.now;
        let Some(h) = self.held_mut() else { return };
        h.heard = Some((h.believed(now), now));
        h.playing = false;
        h.told(now);
        self.verb("media_pause", "the session paused");
    }

    fn seek(&mut self, ms: i64) {
        let now = self.now;
        let Some(h) = self.held_mut() else { return };
        if !h.started {
            // Moved while the sound is still on its way: that is where the
            // speaker is to be, and where it is told to go once it is there.
            h.from_ms = ms;
            return;
        }
        h.heard = Some((ms, now));
        h.told(now);
        self.act(Act::Seek {
            entity: self.entity.into(),
            position_ms: ms,
        });
    }

    fn let_go(&mut self, follows: bool) {
        if self.held().is_none() {
            return;
        }
        self.room.held = None;
        // A member of the group the sound went to plays on in it: a pause
        // said to a member is a pause of its whole group.
        if !follows {
            self.verb("media_pause", "the session moved its sound elsewhere");
        }
    }

    fn caps(&self) -> Option<Caps> {
        let r = self.tuned()?;
        Some(Caps {
            can_volume: r.can_volume && r.volume.is_some(),
            family: r.family.clone(),
        })
    }

    fn volume(&self) -> Option<Vol> {
        let t = &self.room.tuning;
        match (t.volume, &t.reading) {
            (Some(v), _) if t.quiet > 0 => Some(v),
            (_, Some(r)) => Some(Vol {
                level: r.volume?,
                muted: r.muted,
            }),
            _ => None,
        }
    }

    fn set_volume(&mut self, v: Vol) {
        let Some(r) = self.tuned().cloned() else {
            return;
        };
        if r.volume != Some(v.level) {
            self.act(Act::Volume {
                entity: self.entity.into(),
                level: v.level,
            });
        }
        if r.muted != v.muted {
            self.act(Act::Mute {
                entity: self.entity.into(),
                muted: v.muted,
            });
        }
        let t = &mut self.room.tuning;
        (t.volume, t.quiet) = (Some(v), QUIET);
    }

    fn followers(&self) -> Option<Followers> {
        let t = &self.room.tuning;
        let r = t.reading.as_ref()?;
        Some(Followers {
            leads: (t.quiet > 0 && t.members.is_some()) || r.leads(self.entity),
            members: self.members(),
        })
    }

    fn join(&mut self, members: &[String]) {
        let mut now = self.members();
        now.extend(
            members
                .iter()
                .filter(|m| !now.contains(m))
                .cloned()
                .collect::<Vec<_>>(),
        );
        self.act(Act::Join {
            leader: self.entity.into(),
            members: members.to_vec(),
        });
        let t = &mut self.room.tuning;
        (t.members, t.quiet) = (Some(now), QUIET);
    }

    fn unjoin(&mut self, member: &str) {
        let now: Vec<String> = self.members().into_iter().filter(|m| m != member).collect();
        self.act(Act::Unjoin {
            entity: member.into(),
        });
        let t = &mut self.room.tuning;
        (t.members, t.quiet) = (Some(now), QUIET);
    }
}

/// What the reconciler says, as the speaker standing in `key`'s session.
struct Calls<'a> {
    key: &'a str,
    entity: &'a str,
    acts: &'a RefCell<Vec<Act>>,
}

impl Say for Calls<'_> {
    fn call(
        &mut self,
        transition: &'static str,
        input: Vec<u8>,
        as_of: Option<i64>,
        why: &str,
    ) -> Result<(), String> {
        self.acts.borrow_mut().push(Act::Call {
            key: self.key.into(),
            entity: self.entity.into(),
            transition,
            input,
            why: why.into(),
            as_of,
        });
        Ok(())
    }

    /// A speaker's plays are not counted here (the speakers' machines have
    /// no log).
    fn play(&mut self, _: crate::domain::RecordPlay) -> Result<(), String> {
        Ok(())
    }
}

/// Home Assistant, as a shelf of devices: each speaker read, and each
/// session's reconciler of each speaker standing in it.
pub struct Speakers {
    /// The entities offered, and what to call each.
    players: Vec<(String, String)>,
    /// Where a speaker fetches bytes from, which is not where a phone does:
    /// the phone may be on `https://harken.example.com` while the speaker
    /// only knows an address on the LAN.
    media: String,
    rooms: HashMap<String, Room>,
    /// The reconciler of each speaker in each session it stands in, by
    /// (account, entity).
    minds: HashMap<(String, String), Reconciler>,
    /// Each session as last read, which a poll's look is read against.
    sessions: HashMap<String, Listening>,
    /// Sessions a speaker has already let go of while still named their
    /// output — somebody else took it — so the release is said once.
    released: HashSet<(String, String)>,
}

impl Speakers {
    /// `players` are the entities to offer, with their names. `media` is the
    /// base a speaker resolves a `file` against.
    pub fn new(players: Vec<(String, String)>, media: &str) -> Speakers {
        Speakers {
            players,
            media: media.trim_end_matches('/').to_string(),
            rooms: HashMap::new(),
            minds: HashMap::new(),
            sessions: HashMap::new(),
            released: HashSet::new(),
        }
    }

    /// The devices to stand in a session where somebody is listening.
    pub fn players(&self) -> &[(String, String)] {
        &self.players
    }

    pub fn has(&self, entity: &str) -> bool {
        self.players.iter().any(|(p, _)| p == entity)
    }

    /// Whose session this speaker is playing, if anybody's.
    pub fn holder(&self, entity: &str) -> Option<&str> {
        self.rooms
            .get(entity)?
            .held
            .as_ref()
            .map(|h| h.key.as_str())
    }

    /// Whether anybody has handed this speaker something.
    pub fn holds(&self, entity: &str) -> bool {
        self.holder(entity).is_some()
    }

    fn release(key: &str, entity: &str, why: String) -> Act {
        Act::Call {
            key: key.into(),
            entity: entity.into(),
            transition: "release",
            input: input::none(),
            why,
            as_of: None,
        }
    }

    /// `key`'s session, as it now is, read for `entity` at `now` (the
    /// server's clock): what the speaker is told, and what is said as it.
    ///
    /// A speaker is one piece of hardware and a session is one account's:
    /// handed the sound by a second session while a first holds it, the
    /// second gets it — which is what a real speaker does — and the first is
    /// told, by the speaker; a session still naming it after that hears so
    /// once.
    pub fn wants(&mut self, key: &str, entity: &str, state: &Listening, now: i64) -> Vec<Act> {
        if !self.has(entity) {
            return vec![];
        }
        self.sessions.insert(key.into(), state.clone());
        let mut acts = vec![];
        let k = (key.to_string(), entity.to_string());
        match state.output() {
            Some(Output::Device { id, moving }) if id == entity => {
                if let Some(other) = self.holder(entity).filter(|h| *h != key).map(String::from) {
                    if !*moving {
                        if self.released.insert(k) {
                            acts.push(Speakers::release(
                                key,
                                entity,
                                format!("held by {other}'s session"),
                            ));
                        }
                        return acts;
                    }
                    acts.push(Speakers::release(
                        &other,
                        entity,
                        format!("taken by {key}'s session"),
                    ));
                    self.released.remove(&k);
                    self.released.insert((other, entity.into()));
                    self.rooms.entry(entity.into()).or_default().held = None;
                }
            }
            _ => drop(self.released.remove(&k)),
        }
        acts.extend(self.look(key, entity, now));
        acts
    }

    /// `key`'s reconciler of `entity`, run against the session as last read,
    /// where the speaker stands in it.
    fn look(&mut self, key: &str, entity: &str, now: i64) -> Vec<Act> {
        let Some(state) = self
            .sessions
            .get(key)
            .filter(|s| s.device(entity).is_some())
        else {
            return vec![];
        };
        let mind = self
            .minds
            .entry((key.into(), entity.into()))
            .or_insert_with(|| Reconciler::new(entity, None));
        let acts = RefCell::new(vec![]);
        let mut hand = Hand {
            entity,
            key,
            media: &self.media,
            now,
            room: self.rooms.entry(entity.into()).or_default(),
            acts: &acts,
        };
        let v = View {
            me: entity,
            joined: true,
            epoch: 1,
            listening: state,
            now,
        };
        mind.reconcile(
            &v,
            &mut hand,
            &mut Calls {
                key,
                entity,
                acts: &acts,
            },
        );
        acts.into_inner()
    }

    /// Home Assistant said what a held player is doing — or `None`, it did
    /// not answer — at `now`: read, and what that means for the session
    /// that holds it said as the speaker; or, after its grace, the speaker
    /// let go.
    ///
    /// Nothing for a player nobody in a session handed anything to: a
    /// speaker somebody is using from the Sonos app is not this server's
    /// business, and reporting it would put a stranger's music in
    /// somebody's bar.
    pub fn heard(&mut self, entity: &str, poll: Option<&Playing>, now: i64) -> Vec<Act> {
        let Some(room) = self.rooms.get_mut(entity) else {
            return vec![];
        };
        room.events.clear();
        let Some(key) = room.held.as_ref().map(|h| h.key.clone()) else {
            return vec![];
        };
        let mut acts = read(entity, room, poll, now);
        acts.extend(self.look(&key, entity, now));
        if let Some(h) = self.rooms.get_mut(entity).and_then(|r| r.held.as_mut()) {
            h.fresh = None;
        }
        acts
    }

    /// One line on what the bridge made of a poll of `entity` at `now`: where
    /// it reads the speaker as being, where it has the session, and where it
    /// is in the hand-off — for the trace ([`ha::Config::trace`]).
    pub fn describe(&self, entity: &str, poll: Option<&Playing>, now: i64) -> String {
        let Some(h) = self.rooms.get(entity).and_then(|r| r.held.as_ref()) else {
            return "not held".to_string();
        };
        let speaker = match poll.map(|p| (p.position_at(now), p.dated_after(h.told_ms))) {
            Some((Some(ms), true)) => format!("{ms} ms"),
            Some((Some(ms), false)) => format!("{ms} ms (dated before the last command)"),
            Some((None, _)) => "undated".to_string(),
            None => "no answer".to_string(),
        };
        let session = self.sessions.get(&h.key).and_then(|s| s.session.as_ref());
        let session = match (h.started, session) {
            (true, Some(a)) => format!("{} ms", a.position(now)),
            _ => format!("{} ms, standing", h.from_ms),
        };
        format!(
            "speaker {speaker}; session {session}; track {} of {} (handed {}); {}; quiet {}, waited {}, sought {}, silent {}",
            h.at,
            h.urls.len(),
            h.handed_at,
            if h.started { "started" } else { "fetching" },
            h.quiet,
            h.waited,
            h.sought,
            h.silent
        )
    }

    /// Home Assistant said what a standing player's volume and group are —
    /// or `None`, it did not answer — which spends a poll of its quiet.
    pub fn polled(&mut self, entity: &str, reading: Option<&Speaker>) {
        let t = &mut self.rooms.entry(entity.to_string()).or_default().tuning;
        t.reading = reading.cloned();
        t.quiet = t.quiet.saturating_sub(1);
    }

    /// The speaker stops standing in `key`'s session — nobody is listening
    /// there and it is not making the sound, or has made none for
    /// [`ha::IDLE`] — and anything it was playing for it stops, because
    /// nothing else would ever tell it to.
    pub fn left(&mut self, key: &str, entity: &str) -> Vec<Act> {
        let k = (key.to_string(), entity.to_string());
        self.minds.remove(&k);
        self.released.remove(&k);
        if self.holder(entity) != Some(key) {
            return vec![];
        }
        if let Some(r) = self.rooms.get_mut(entity) {
            r.held = None;
        }
        vec![Act::Verb {
            entity: entity.into(),
            verb: "media_pause",
            why: "nobody is listening there any more",
        }]
    }
}

/// A poll of a held speaker, read: what it is doing now in the room, what
/// happened to it among the room's events, and what the house is told
/// meanwhile (a seek it dropped, said again).
fn read(entity: &str, room: &mut Room, poll: Option<&Playing>, now: i64) -> Vec<Act> {
    let Some(h) = room.held.as_mut() else {
        return vec![];
    };
    h.since_told = h.since_told.saturating_add(1);
    // Where it says it is. A URL that is not one of ours means either that
    // it has not started yet or that it has been sent somewhere else
    // entirely — a radio stream, a doorbell chime — and the only thing that
    // tells those apart is how long ago we handed it something.
    let at = poll.and_then(|p| h.index(p));
    if !h.started {
        return fetching(entity, room, poll, at, now);
    }
    let (Some(at), Some(p)) = (at, poll) else {
        lapse(room, poll);
        return vec![];
    };
    // Where it is now, while it plays, counted on from when the house last
    // said so — a position the house dated, and not one it dated before the
    // bridge last told the speaker something, for the polls the house takes
    // to hear of it ([`STALE`]). Anything else is no position at all: it is
    // never the reason for a seek or a report.
    let trusted = p.dated_after(h.told_ms) || h.since_told >= STALE;
    let position = p
        .position_at(now)
        .filter(|_| trusted && p.says() == Says::Playing);
    h.settling = LAPSE;
    if let Some(position) = position {
        h.heard = Some((position, now));
    }
    if p.says() == Says::Playing {
        h.sounding_ms = Some(now);
    }
    if at != h.at {
        // It moved on its own: the end of a track, or somebody's finger on
        // the speaker or in the Sonos app — the reconciler says which.
        (h.at, h.quiet, h.silent) = (at, QUIET, 0);
        return vec![];
    }
    if h.quiet > 0 {
        h.quiet -= 1;
        return vec![];
    }
    match p.says() {
        // Fetching, or buffering: neither a sound to count from nor a stall.
        Says::Between => {}
        Says::Stopped if !h.playing => h.silent = 0,
        Says::Stopped => {
            // Not playing while it was told to. Once is a blink; twice in a
            // row it has stopped, by itself or by hand — at the point the
            // house says, where that is a position it dated since a poll last
            // heard it playing (a stop is a change of state, which the house
            // dates), and otherwise where it had been counted to when it fell
            // silent: never a stale date's beginning of the track.
            let counted = h.believed(now);
            let stopped_at = *h.stopped_at.get_or_insert(counted);
            h.silent += 1;
            if h.silent >= STALL {
                let at_ms = match p.dated_after(h.sounding_ms.unwrap_or(h.told_ms)) {
                    true => p.position_ms.max(0),
                    false => stopped_at,
                };
                (h.playing, h.quiet, h.silent, h.stopped_at) = (false, QUIET, 0, None);
                h.heard = Some((at_ms, now));
                room.events.push((h.key.clone(), Event::Stalled { at_ms }));
            }
        }
        Says::Playing => {
            (h.silent, h.stopped_at) = (0, None);
            if !h.playing {
                (h.playing, h.quiet) = (true, QUIET);
                room.events.push((h.key.clone(), Event::Resumed));
                return vec![];
            }
            h.fresh = position;
        }
    }
    vec![]
}

/// A poll of a speaker that has not yet said it has what it was handed: it
/// has, once it is playing the queue — on the track it was handed or any
/// after it, since a short track can end before the first poll — at the
/// point where it actually is, which is measured, never assumed.
fn fetching(
    entity: &str,
    room: &mut Room,
    poll: Option<&Playing>,
    at: Option<usize>,
    now: i64,
) -> Vec<Act> {
    let Some(p) = poll else {
        lapse(room, poll);
        return vec![];
    };
    let Some(h) = room.held.as_mut() else {
        return vec![];
    };
    // The queue it was handed, at or after where it was handed from — but a
    // later track only once the house has dated it since the hand-off: until
    // then it may be what the speaker was playing before, which can be a
    // track of this same queue.
    let ours =
        at.filter(|at| *at == h.handed_at || (*at > h.handed_at && p.dated_after(h.handed_ms)));
    let wanted = match h.playing {
        true => Says::Playing,
        false => Says::Stopped,
    };
    let Some(at) = ours else {
        // Saying it plays nothing, or nothing yet, is fetching; naming
        // something that is not ours is perhaps being sent elsewhere.
        if p.url.is_empty() || p.says() == Says::Between {
            wait(room, None, now);
        } else {
            lapse(room, poll);
        }
        return vec![];
    };
    if p.says() != wanted {
        wait(room, Some(at), now);
        return vec![];
    }
    // The point it is at, counted on from when the house dated it. A playing
    // report the house did not date is no point at all — the Sonos
    // integration clears the position while a new stream's length is
    // unknown — and the hand-off waits for one it did. One the house dated
    // before the last seek is still the house's own count: a seek the
    // speaker dropped moves nothing, so nothing is dated again.
    let Some(position) = p.position_at(now) else {
        wait(room, Some(at), now);
        return vec![];
    };
    // On the track it was handed, but at its beginning when it was handed
    // from half way — playing, or paused there for a session handed over
    // paused: the seek after the enqueue reached it before it had loaded
    // and was dropped. It is told again, once it has the track, and it has
    // not started until it is there — `started` at 0 would take every
    // device's clock back to the beginning with it. The session stands
    // where it was meanwhile.
    if at == h.handed_at
        && h.from_ms > SYNC_MS
        && position < h.from_ms - SYNC_MS
        && h.sought < SEEKS
    {
        if h.seek_wait > 0 {
            h.seek_wait -= 1;
            return vec![];
        }
        (h.sought, h.seek_wait, h.told_ms, h.since_told) = (h.sought + 1, QUIET, now, 0);
        return vec![Act::Seek {
            entity: entity.into(),
            position_ms: h.from_ms,
        }];
    }
    landed(h, at, position, now);
    vec![]
}

/// A poll of the hand-off that was ours but not yet sounding as asked —
/// spent from [`WAIT`]. Run out on a track of the queue, it is believed
/// started where it would be had the last thing it was told landed when it
/// was told; run out anywhere else, it never got there.
fn wait(room: &mut Room, at: Option<usize>, now: i64) {
    let Some(h) = room.held.as_mut() else { return };
    h.waited = h.waited.saturating_add(1);
    if h.waited < WAIT {
        return;
    }
    match at {
        Some(at) => {
            let guess = match at == h.handed_at {
                true => h.from_ms + (now - h.told_ms).max(0) * i64::from(h.playing),
                false => 0,
            };
            landed(h, at, guess, now);
        }
        None => {
            h.settling = 0;
            lapse(room, None);
        }
    }
}

/// The hand-off has landed: on track `at`, at `position`, as of `now`.
fn landed(h: &mut Holding, at: usize, position: i64, now: i64) {
    (h.started, h.at, h.settling, h.quiet) = (true, at, LAPSE, QUIET);
    h.heard = Some((position, now));
    h.fresh = Some(position);
}

/// One poll that was not the speaker playing ours: spent from the grace it
/// has, and the last one lets it go — said as the speaker, and out loud.
fn lapse(room: &mut Room, poll: Option<&Playing>) {
    let Some(h) = room.held.as_mut() else { return };
    if h.settling > 0 {
        h.settling -= 1;
        return;
    }
    // What it was expected to say, beside what it said: a URL that is ours
    // spelt some other way reads, in the journal, as two strings that
    // differ only in their escapes.
    let expected = h
        .urls
        .get(if h.started { h.at } else { h.handed_at })
        .cloned()
        .unwrap_or_default();
    let next = h.urls.get(h.at + 1).filter(|_| h.started).cloned();
    let never = !h.started && h.waited >= WAIT;
    let why = match poll {
        Some(p) => match next {
            Some(next) => format!(
                "{} {:?}, which is not ours (playing {expected:?}, next {next:?})",
                p.state, p.url
            ),
            None => format!(
                "{} {:?}, which is not ours (playing {expected:?})",
                p.state, p.url
            ),
        },
        None if never => format!("it never started {expected:?}"),
        None => "not answering".to_string(),
    };
    let key = h.key.clone();
    room.held = None;
    room.events.push((key, Event::Lost { why }));
}

/// Home Assistant's timestamps, read with no date library: the two
/// spellings it uses, and the house's clock moved onto the server's.
pub mod when {
    /// Days from 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    fn days(y: i64, m: i64, d: i64) -> i64 {
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    fn num(s: &str) -> Option<i64> {
        (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse().ok())
            .flatten()
    }

    /// RFC 3339, as Home Assistant writes `media_position_updated_at`:
    /// `2024-05-01T12:34:56.789012+00:00`, or with a `Z`. Milliseconds since
    /// the Unix epoch.
    pub fn rfc3339_ms(s: &str) -> Option<i64> {
        let s = s.trim();
        let (date, rest) = s.split_at_checked(10)?;
        let mut d = date.split('-');
        let (y, mo, da) = (num(d.next()?)?, num(d.next()?)?, num(d.next()?)?);
        let rest = rest.strip_prefix(['T', 't', ' '])?;
        let (time, zone) = rest.split_at(rest.find(['Z', 'z', '+', '-'])?);
        let (hms, frac) = time.split_once('.').unwrap_or((time, ""));
        let mut t = hms.split(':');
        let (h, mi, se) = (num(t.next()?)?, num(t.next()?)?, num(t.next()?)?);
        let ms = match frac {
            "" => 0,
            f if f.bytes().all(|b| b.is_ascii_digit()) => num(&format!("{f:0<3}")[..3])?,
            _ => return None,
        };
        let offset = match zone {
            "Z" | "z" => 0,
            z => {
                let sign = if z.starts_with('-') { -1 } else { 1 };
                let (oh, om) = z[1..].split_once(':')?;
                sign * (num(oh)? * 60 + num(om)?)
            }
        };
        let secs = days(y, mo, da) * 86_400 + h * 3_600 + mi * 60 + se - offset * 60;
        Some(secs * 1_000 + ms)
    }

    /// An HTTP `Date` header, `Sun, 06 Nov 1994 08:49:37 GMT`: milliseconds
    /// since the Unix epoch, to the second.
    pub fn http_date_ms(s: &str) -> Option<i64> {
        let parts: Vec<&str> = s.split_whitespace().collect();
        let [_, day, month, year, time, "GMT"] = parts[..] else {
            return None;
        };
        let month = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ]
        .iter()
        .position(|m| *m == month)? as i64
            + 1;
        let mut t = time.split(':');
        let (h, mi, se) = (num(t.next()?)?, num(t.next()?)?, num(t.next()?)?);
        Some((days(num(year)?, month, num(day)?) * 86_400 + h * 3_600 + mi * 60 + se) * 1_000)
    }

    /// The house's clock less the server's, narrowed answer by answer.
    ///
    /// One `Date` says the house's clock to the second, so read alone — and
    /// taken at the middle of its second, as the bridge did — it places a
    /// moment the house stamped only to within half a second either way,
    /// which is most of the bound every output is held to. But each answer also says *when*: the house wrote its `Date`
    /// somewhere between the server asking (`sent`) and hearing back (`got`),
    /// so the house's clock less the server's lies between `date - got` and
    /// `date + 1000 - sent`. Answers land at every phase of the second, and
    /// the intersection of what they say closes in on the truth to within
    /// the round trip. Each answer widens what is known by a millisecond
    /// first, so two clocks drifting apart are followed; answers that cannot
    /// both be true — a clock stepped — start it again from the latest.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub struct Skew {
        bounds: Option<(i64, i64)>,
    }

    impl Skew {
        /// An answer dated `date` (the house's clock, to the second), asked
        /// for at `sent` and heard at `got` (the server's).
        pub fn answer(&mut self, date: i64, sent: i64, got: i64) {
            let (lo, hi) = (date - got, date + 1_000 - sent);
            self.bounds = Some(match self.bounds {
                Some((l, h)) if lo <= h + 1 && hi >= l - 1 => ((l - 1).max(lo), (h + 1).min(hi)),
                _ => (lo, hi),
            });
        }

        /// A moment the house stamped, on the server's clock, and how far
        /// either way it may be: `None` before any answer was dated.
        pub fn ours(&self, stamped: i64) -> Option<(i64, i64)> {
            let (lo, hi) = self.bounds?;
            Some((stamped - (lo + hi) / 2, (hi - lo + 1) / 2))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Both of Home Assistant's spellings, and a house whose clock is
        /// a minute out still placing a moment right. Falsified by reading
        /// the offset with its sign flipped: the `+02:00` stamp came out
        /// four hours late.
        #[test]
        fn the_houses_timestamps_are_read_and_moved_onto_our_clock() {
            assert_eq!(rfc3339_ms("1970-01-01T00:00:01+00:00"), Some(1_000));
            assert_eq!(
                rfc3339_ms("2024-05-01T12:34:56.789012+00:00"),
                Some(1_714_566_896_789)
            );
            assert_eq!(
                rfc3339_ms("2024-05-01T14:34:56.789+02:00"),
                Some(1_714_566_896_789)
            );
            assert_eq!(rfc3339_ms("2024-05-01T12:34:56Z"), Some(1_714_566_896_000));
            assert_eq!(rfc3339_ms("yesterday"), None);
            assert_eq!(
                rfc3339_ms("2024-05-01T12:34:56.1\u{20ac}+00:00"),
                None,
                "and nothing panics on one"
            );
            assert_eq!(
                http_date_ms("Wed, 01 May 2024 12:34:56 GMT"),
                Some(1_714_566_896_000)
            );
            assert_eq!(http_date_ms("Wed, 01 May 2024 12:34:56"), None);
            // The house a minute fast: stamped ten seconds before its now
            // is ten seconds before ours, to within the half second one
            // answer can say.
            let ours = 1_714_566_896_000;
            let house = ours + 60_000;
            let mut skew = Skew::default();
            assert_eq!(
                skew.ours(house - 10_000),
                None,
                "nothing known before an answer"
            );
            skew.answer(house, ours - 20, ours + 20);
            let (at, slack) = skew.ours(house - 10_000).unwrap();
            assert!(
                (at - (ours - 10_000)).abs() <= slack && slack <= 520,
                "{at} ± {slack}"
            );
        }

        /// Answers at every phase of the house's second close in on its
        /// clock to within the round trip, where one answer alone is half a
        /// second either way — and a clock stepped starts it again rather
        /// than leaving nothing true.
        ///
        /// Falsified by an `answer` that kept only the latest answer: the
        /// slack stayed at 505 ms.
        #[test]
        fn the_houses_clock_is_narrowed_answer_by_answer() {
            // The house 1 234 ms ahead of the server; a 10 ms round trip.
            let ahead: i64 = 1_234;
            let mut skew = Skew::default();
            for i in 0..40 {
                let sent = 1_700_000_000_000 + i * 1_037;
                let date = (sent + 5 + ahead).div_euclid(1_000) * 1_000;
                skew.answer(date, sent, sent + 10);
            }
            let stamped = 1_700_000_050_000 + ahead;
            let (at, slack) = skew.ours(stamped).unwrap();
            assert!(slack <= 50, "narrowed to {slack} ms");
            assert!((at - 1_700_000_050_000).abs() <= slack, "placed at {at}");
            // The house's clock stepped back a minute.
            let sent = 1_700_000_100_000;
            skew.answer(
                (sent + ahead - 60_000).div_euclid(1_000) * 1_000,
                sent,
                sent + 10,
            );
            let (at, slack) = skew.ours(stamped - 60_000).unwrap();
            assert!(
                slack > 400 && (at - 1_700_000_050_000).abs() <= slack,
                "{at} ± {slack}"
            );
        }
    }
}

/// Whether anybody is *listening* in a session, as opposed to standing in it:
/// a client of any kind but a speaker. Where this holds every speaker stands
/// beside them; where it does not, only the ones playing ([`playing`]).
pub fn listening(state: &Listening) -> bool {
    state
        .devices
        .iter()
        .any(|d| Kind::parse(&d.kind) != Kind::Speaker)
}

/// Which of `entities` are making the session's sound: its output, here —
/// handed the sound or on its way to it — and whoever follows it. These go on
/// standing in a session nobody is listening in, so that closing the last
/// browser does not stop the music in the kitchen: the bridge goes on
/// reconciling them and saying what they do (`advance` at the end of each
/// track, a stall, a volume, a group) for as long as the session sounds, and
/// for [`ha::IDLE`] after it stops.
pub fn playing<'a>(state: &Listening, entities: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let Some(leader) = state.leader() else {
        return vec![];
    };
    entities
        .into_iter()
        .filter(|e| {
            *e == leader || state.device(e).and_then(|d| d.follows.as_deref()) == Some(leader)
        })
        .map(str::to_string)
        .collect()
}

/// How the house is read, and what a speaker is told: the reading of Home
/// Assistant's polls — a hand-off fetched and the seek it dropped said
/// again, an undated position, a later track, a blink, a lapse, a URL spelt
/// back another way, the speaker's own queue — and a command as the service
/// calls it is, with which account holds a speaker. What the session makes
/// of a speaker (its volume, its group, a move, a stall, a drift, losing
/// it) is the reconciler's, and tested there (`harken/player/tests`).
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::host::Kind;
    use crate::domain::{Active, Clock, Id, Present};

    const KITCHEN: &str = "media_player.kitchen";
    const NOW: i64 = 1_700_000_000_000;

    /// A device as it declared itself, with nothing said of its volume or
    /// its group: what `on_join` writes.
    fn device(id: &str, kind: Kind, can_play: bool) -> Present {
        Present {
            can_play,
            can_volume: false,
            family: None,
            follows: None,
            id: id.into(),
            kind: kind.name().into(),
            muted: false,
            name: id.into(),
            user: String::new(),
            volume: crate::domain::host::UNSAID_VOLUME,
        }
    }

    fn track(file: &str, n: u8) -> Track {
        let mut id = [0; 16];
        id[15] = n;
        Track {
            id: Id::new(id),
            title: file.into(),
            creator: "Bach".into(),
            album: String::new(),
            duration_ms: 60_000,
            file: file.into(),
        }
    }

    fn bridge() -> Speakers {
        Speakers::new(
            vec![
                (KITCHEN.into(), "Kitchen".into()),
                ("media_player.study".into(), "Study".into()),
            ],
            "http://10.0.0.2:8787/",
        )
    }

    fn url_of(file: &str) -> String {
        format!("http://10.0.0.2:8787/media/{file}")
    }

    /// A session whose output is `output`, on `files` from `at`, with a
    /// phone and the kitchen standing in it.
    fn session(files: &[&str], at: usize, clock: Clock, output: Output) -> Listening {
        Listening {
            devices: vec![
                device("phone", Kind::Phone, true),
                device(KITCHEN, Kind::Speaker, true),
            ],
            scrub: Default::default(),
            session: Some(Active {
                queue: files
                    .iter()
                    .enumerate()
                    .map(|(i, f)| track(f, i as u8))
                    .collect(),
                at: at as i64,
                clock,
                output,
                from: String::new(),
            }),
        }
    }

    fn kitchen(moving: bool) -> Output {
        Output::Device {
            id: KITCHEN.into(),
            moving,
        }
    }

    fn playing_at(at_ms: i64) -> Clock {
        Clock::Playing { at_ms, since: NOW }
    }

    /// Playing `url`, at `position_ms` as of `NOW`.
    fn playing(url: &str, position_ms: i64) -> Playing {
        Playing {
            state: "playing".into(),
            url: url.into(),
            position_ms,
            updated_ms: Some(NOW),
            duration_ms: None,
            queue_position: None,
            slack_ms: 0,
        }
    }

    fn calls(acts: &[Act]) -> Vec<(&str, &'static str)> {
        acts.iter()
            .filter_map(|a| match a {
                Act::Call {
                    key, transition, ..
                } => Some((key.as_str(), *transition)),
                _ => None,
            })
            .collect()
    }

    fn input_of(acts: &[Act], n: usize) -> &[u8] {
        match &acts[n] {
            Act::Call { input, .. } => input,
            other => panic!("not a transition: {other:?}"),
        }
    }

    /// A speaker handed the session, `files` from `at`, that has started.
    fn started(b: &mut Speakers, key: &str, files: &[&str], at: usize) {
        b.wants(
            key,
            KITCHEN,
            &session(files, at, playing_at(0), kitchen(true)),
            NOW,
        );
        let acts = b.heard(KITCHEN, Some(&playing(&url_of(files[at]), 0)), NOW);
        assert_eq!(calls(&acts), [(key, "started")]);
    }

    /// A hand-off is the whole queue as URLs the speaker can fetch —
    /// resolved against the address the *speaker* can reach, which is not
    /// the one a phone uses — from the track it was on, at the point in it
    /// the session stands at: still, while the sound is on its way, so a
    /// speaker that took a second and a half to be asked is not told to
    /// start a second and a half past where the track stopped.
    ///
    /// Falsified by handing the clock counted on to now (the reconciler's
    /// `want` as `a.clock.position`): the speaker was told 43 500 where
    /// 42 000 was right.
    #[test]
    fn a_hand_off_is_the_whole_queue_as_urls_the_speaker_can_fetch() {
        let mut b = bridge();
        let acts = b.wants(
            "alice",
            KITCHEN,
            &session(
                &["music/a.mp3", "music/b.mp3"],
                1,
                playing_at(42_000),
                kitchen(true),
            ),
            NOW + 1_500,
        );
        assert_eq!(
            acts,
            [Act::Start {
                entity: KITCHEN.into(),
                urls: vec![url_of("music/a.mp3"), url_of("music/b.mp3")],
                at: 1,
                position_ms: 42_000,
                playing: true,
            }]
        );
        assert_eq!(b.holder(KITCHEN), Some("alice"));
    }

    /// A speaker handed a track from half way that starts it from the
    /// beginning — the seek after the enqueue reached it before it had
    /// loaded, and a Sonos drops it — is told again where the track was,
    /// once it has the track, and has not started until it is there. The
    /// session hears `started` at the point it was handed from, never at 0.
    /// One that will not go there after three tries is believed.
    ///
    /// Falsified by saying `started` at whatever the speaker first reports
    /// (the `SEEKS` branch taken out): the session was told 0.
    #[test]
    fn a_speaker_that_starts_the_track_from_its_beginning_is_sent_where_it_was() {
        let mut b = bridge();
        let files = ["music/a.mp3", "music/b.mp3"];
        let handed = session(&files, 0, playing_at(90_000), kitchen(true));
        b.wants("alice", KITCHEN, &handed, NOW);
        let seek = [Act::Seek {
            entity: KITCHEN.into(),
            position_ms: 90_000,
        }];
        let at_start = playing(&url_of(files[0]), 0);
        assert_eq!(
            b.heard(KITCHEN, Some(&at_start), NOW),
            seek,
            "told again, now it has the track"
        );
        for _ in 0..2 {
            assert!(
                b.heard(KITCHEN, Some(&at_start), NOW).is_empty(),
                "a moment for it to get there"
            );
        }
        assert_eq!(b.heard(KITCHEN, Some(&at_start), NOW), seek, "and again");
        let there = playing(&url_of(files[0]), 90_200);
        let acts = b.heard(KITCHEN, Some(&there), NOW);
        assert_eq!(calls(&acts), [("alice", "started")]);
        assert_eq!(
            input_of(&acts, 0),
            input::at(90_200),
            "started where the track was"
        );

        // A speaker that never goes: believed after three tries.
        let mut b = bridge();
        b.wants("alice", KITCHEN, &handed, NOW);
        let mut seeks = 0;
        for _ in 0..20 {
            let acts = b.heard(KITCHEN, Some(&at_start), NOW);
            seeks += acts
                .iter()
                .filter(|a| matches!(a, Act::Seek { .. }))
                .count();
            if !calls(&acts).is_empty() {
                assert_eq!(calls(&acts), [("alice", "started")]);
                break;
            }
        }
        assert_eq!(seeks, 3);
        assert!(b.holds(KITCHEN), "and never let go while it was being told");

        // From the beginning, nothing to seek: it starts at once.
        let mut b = bridge();
        started(&mut b, "alice", &files, 0);
    }

    /// An entity this server was not told to offer is not a device, so a
    /// session naming one reaches nothing in the house — and a session whose
    /// output is somebody else asks nothing of the speaker either.
    ///
    /// Falsified by dropping `has` from `wants`: the bedroom was handed a
    /// queue.
    #[test]
    fn nothing_is_said_to_a_player_that_was_not_offered_or_not_the_output() {
        let mut b = bridge();
        let bedroom = Output::Device {
            id: "media_player.bedroom".into(),
            moving: true,
        };
        let mut s = session(&["music/a.mp3"], 0, playing_at(0), bedroom);
        s.devices
            .push(device("media_player.bedroom", Kind::Speaker, true));
        assert!(b.wants("alice", "media_player.bedroom", &s, NOW).is_empty());
        assert!(b
            .heard("media_player.bedroom", Some(&playing("x", 0)), NOW)
            .is_empty());
        let phone = Output::Device {
            id: "phone".into(),
            moving: false,
        };
        assert!(b
            .wants(
                "alice",
                KITCHEN,
                &session(&["music/a.mp3"], 0, playing_at(0), phone),
                NOW
            )
            .is_empty());
        assert!(!b.holds(KITCHEN));
    }

    /// The speaker's first word about what it was handed ends the hand-off —
    /// `started`, at where it is — and until then nothing it says is news,
    /// not even another track of the same queue it was playing before.
    ///
    /// Falsified by taking any track of the queue as having started (`ours`
    /// the index alone): the speaker still on the old track 0 was believed.
    #[test]
    fn a_speaker_has_started_when_it_says_what_it_was_handed() {
        let mut b = bridge();
        b.wants(
            "alice",
            KITCHEN,
            &session(
                &["music/a.mp3", "music/b.mp3"],
                1,
                playing_at(0),
                kitchen(true),
            ),
            NOW,
        );
        assert!(b
            .heard(KITCHEN, Some(&playing(&url_of("music/a.mp3"), 9_000)), NOW)
            .is_empty());
        let acts = b.heard(KITCHEN, Some(&playing(&url_of("music/b.mp3"), 1_200)), NOW);
        assert_eq!(calls(&acts), [("alice", "started")]);
        assert_eq!(input_of(&acts, 0), input::at(1_200));
    }

    /// The house says a speaker is playing before it says where: the Sonos
    /// integration clears the position while a new stream's length is
    /// unknown. That is no position at all, and the hand-off is not over:
    /// it waits for one it can place, and that is where the hand-off ends.
    ///
    /// Falsified by reading an undated report as the point it was handed at
    /// (`position_at` answering `from_ms` for no date): `started` at 90 000
    /// on the first poll, with the speaker at the beginning.
    #[test]
    fn a_hand_off_is_not_over_until_the_house_says_where_the_speaker_is() {
        let mut b = bridge();
        let files = ["music/a.mp3", "music/b.mp3"];
        b.wants(
            "alice",
            KITCHEN,
            &session(&files, 0, playing_at(90_000), kitchen(true)),
            NOW,
        );
        let undated = Playing {
            updated_ms: None,
            ..playing(&url_of(files[0]), 0)
        };
        for poll in 0..3 {
            assert!(
                b.heard(KITCHEN, Some(&undated), NOW + 100 * poll)
                    .is_empty(),
                "playing, but nobody can say where: poll {poll}"
            );
        }
        let buffering = Playing {
            state: "buffering".into(),
            ..playing(&url_of(files[0]), 0)
        };
        assert!(b.heard(KITCHEN, Some(&buffering), NOW + 300).is_empty());
        // Dated at last, and at the beginning: told where the track was.
        let at_start = Playing {
            updated_ms: Some(NOW + 400),
            ..playing(&url_of(files[0]), 0)
        };
        assert!(matches!(
            &b.heard(KITCHEN, Some(&at_start), NOW + 500)[..],
            [Act::Seek {
                position_ms: 90_000,
                ..
            }]
        ));
        // The house goes on dating the beginning: the seek was dropped, so
        // nothing moved to date again. A moment for it, and it is told again.
        for poll in 0..2 {
            assert!(b
                .heard(KITCHEN, Some(&at_start), NOW + 1_000 + 100 * poll)
                .is_empty());
        }
        assert!(matches!(
            &b.heard(KITCHEN, Some(&at_start), NOW + 1_300)[..],
            [Act::Seek {
                position_ms: 90_000,
                ..
            }]
        ));
        let there = Playing {
            updated_ms: Some(NOW + 2_000),
            ..playing(&url_of(files[0]), 90_000)
        };
        let acts = b.heard(KITCHEN, Some(&there), NOW + 2_300);
        assert_eq!(calls(&acts), [("alice", "started")]);
        let Act::Call { input, as_of, .. } = &acts[0] else {
            unreachable!()
        };
        assert_eq!(
            input,
            &input::at(90_300),
            "where it measurably is, counted to the poll"
        );
        assert_eq!(
            *as_of,
            Some(NOW + 2_300),
            "and on from there, to when it is said"
        );
    }

    /// A short track can end before the bridge first hears of a hand-off,
    /// and the speaker is then on the second track of what it was handed:
    /// ours, followed (`advance`, then — once the session has followed —
    /// `started` where it is in that track), never let go. A later track
    /// reported from before the hand-off, though, may be what it was
    /// playing before, so that waits for a report dated since; and two
    /// tracks on is a `jump`.
    ///
    /// Falsified by taking only the track it was handed as having started
    /// (`ours` matching `handed_at` alone): the second track was a lapse.
    #[test]
    fn a_speaker_already_on_a_later_track_is_followed_and_not_let_go() {
        let mut b = bridge();
        let files = ["music/a.mp3", "music/b.mp3", "music/c.mp3"];
        b.wants(
            "alice",
            KITCHEN,
            &session(&files, 0, playing_at(0), kitchen(true)),
            NOW,
        );
        // The third track, dated before the hand-off: perhaps the last queue.
        let old = Playing {
            updated_ms: Some(NOW - 30_000),
            ..playing(&url_of(files[2]), 0)
        };
        assert!(b.heard(KITCHEN, Some(&old), NOW + 100).is_empty());
        assert!(b.holds(KITCHEN));
        // The second, dated since: it got there by itself.
        let on = Playing {
            updated_ms: Some(NOW + 2_000),
            ..playing(&url_of(files[1]), 0)
        };
        let acts = b.heard(KITCHEN, Some(&on), NOW + 2_500);
        assert_eq!(calls(&acts), [("alice", "advance")]);
        assert_eq!(input_of(&acts, 0), input::advance(0));
        assert!(
            b.heard(KITCHEN, Some(&on), NOW + 2_600).is_empty(),
            "said once, while it lands"
        );
        // The session followed it, still on its way: the hand-off is over
        // where the speaker is in that track.
        let advanced = session(
            &files,
            1,
            Clock::Playing {
                at_ms: 0,
                since: NOW + 2_700,
            },
            kitchen(true),
        );
        let acts = b.wants("alice", KITCHEN, &advanced, NOW + 2_700);
        assert_eq!(calls(&acts), [("alice", "started")]);
        assert_eq!(
            input_of(&acts, 0),
            input::at(700),
            "where it is in that track"
        );
        // The session following it hands it nothing again.
        let followed = session(
            &files,
            1,
            Clock::Playing {
                at_ms: 700,
                since: NOW + 2_700,
            },
            kitchen(false),
        );
        assert!(b.wants("alice", KITCHEN, &followed, NOW + 2_800).is_empty());
        for poll in 0..8 {
            let on = Playing {
                updated_ms: Some(NOW + 2_700),
                ..playing(&url_of(files[1]), 700)
            };
            assert!(b
                .heard(KITCHEN, Some(&on), NOW + 3_000 + 1_000 * poll)
                .is_empty());
        }
        assert_eq!(b.holder(KITCHEN), Some("alice"));
        // …and two tracks on is a jump.
        let mut b = bridge();
        b.wants(
            "alice",
            KITCHEN,
            &session(&files, 0, playing_at(0), kitchen(true)),
            NOW,
        );
        let two_on = Playing {
            updated_ms: Some(NOW + 1_000),
            ..playing(&url_of(files[2]), 0)
        };
        let acts = b.heard(KITCHEN, Some(&two_on), NOW + 1_000);
        assert_eq!(calls(&acts), [("alice", "jump")]);
    }

    /// One poll of `paused` or `idle` from a speaker playing on is a blink —
    /// a Sonos between a seek and its stream, the house a moment behind — and
    /// not a stall. Two in a row is a stall, at the point the house dated
    /// since it was last told anything, and otherwise where it had been
    /// counted to when it fell silent — never a stale date.
    ///
    /// Falsified by a stall on the first silent poll (`STALL` at 1):
    /// `stalled` at 0.
    #[test]
    fn a_blink_is_not_a_stall_and_a_stall_is_never_a_stale_beginning() {
        let mut b = bridge();
        let files = ["music/a.mp3"];
        started(&mut b, "alice", &files, 0);
        let a = url_of(files[0]);
        for poll in 0..2 {
            b.heard(KITCHEN, Some(&playing(&a, 0)), NOW + 1_000 + poll);
        }
        let blink = Playing {
            state: "idle".into(),
            updated_ms: Some(NOW - 60_000),
            ..playing(&a, 0)
        };
        assert!(b.heard(KITCHEN, Some(&blink), NOW + 4_000).is_empty());
        assert!(b
            .heard(KITCHEN, Some(&playing(&a, 0)), NOW + 5_000)
            .is_empty());
        // Stopped for real, the house still dating the beginning.
        assert!(b.heard(KITCHEN, Some(&blink), NOW + 6_000).is_empty());
        let acts = b.heard(KITCHEN, Some(&blink), NOW + 7_000);
        assert_eq!(calls(&acts), [("alice", "stalled")]);
        assert_eq!(
            input_of(&acts, 0),
            input::at(6_000),
            "where it was counted to when it fell silent"
        );
    }

    /// Where the speaker says it is, is where it is — including when nobody
    /// asked it to move: the next track is `advance`, anywhere else `jump`
    /// (as the reconciler says it, from what is read here), and a pause by
    /// hand, read through a blink, `stalled` where the house says it stopped.
    ///
    /// Falsified by not moving `at` to where the speaker says (the `at !=
    /// h.at` branch taken out): nothing was said of either move.
    #[test]
    fn what_the_speaker_says_it_is_playing_is_where_it_is_in_the_queue() {
        let mut b = bridge();
        let files = ["music/a.mp3", "music/b.mp3", "music/c.mp3"];
        started(&mut b, "alice", &files, 0);
        let acts = b.heard(KITCHEN, Some(&playing(&url_of("music/b.mp3"), 100)), NOW);
        assert_eq!(calls(&acts), [("alice", "advance")]);
        let moved = session(
            &files,
            1,
            Clock::Playing {
                at_ms: 0,
                since: NOW,
            },
            kitchen(false),
        );
        assert!(
            b.wants("alice", KITCHEN, &moved, NOW).is_empty(),
            "the session followed: nothing to tell it"
        );
        let acts = b.heard(KITCHEN, Some(&playing(&url_of("music/a.mp3"), 100)), NOW);
        assert_eq!(calls(&acts), [("alice", "jump")]);
        let back = session(
            &files,
            0,
            Clock::Playing {
                at_ms: 100,
                since: NOW,
            },
            kitchen(false),
        );
        assert!(b.wants("alice", KITCHEN, &back, NOW).is_empty());
        // Quiet for a moment after a move, then a pause by hand.
        let paused = Playing {
            state: "paused".into(),
            ..playing(&url_of("music/a.mp3"), 4_200)
        };
        assert!(b.heard(KITCHEN, Some(&paused), NOW).is_empty());
        assert!(b.heard(KITCHEN, Some(&paused), NOW).is_empty());
        assert!(
            b.heard(KITCHEN, Some(&paused), NOW).is_empty(),
            "one poll of it is a blink"
        );
        let acts = b.heard(KITCHEN, Some(&paused), NOW);
        assert_eq!(calls(&acts), [("alice", "stalled")]);
        assert_eq!(
            input_of(&acts, 0),
            input::at(4_200),
            "where the house says it stopped"
        );
    }

    /// A session that moved is told to the speaker as Home Assistant's
    /// service calls: a pause `media_pause`, a play `media_play`, a seek
    /// `media_seek`, another place in the queue a fresh hand-off from there.
    ///
    /// Falsified by a `seek` that told the speaker nothing while it was
    /// started (the `Act::Seek` taken out): no `media_seek`.
    #[test]
    fn a_session_that_moved_is_told_as_service_calls() {
        let mut b = bridge();
        let files = ["music/a.mp3", "music/b.mp3"];
        started(&mut b, "alice", &files, 0);
        let anchored = session(
            &files,
            0,
            Clock::Playing {
                at_ms: 0,
                since: NOW,
            },
            kitchen(false),
        );
        assert!(b.wants("alice", KITCHEN, &anchored, NOW + 100).is_empty());
        let paused = session(&files, 0, Clock::Paused { at_ms: 5_000 }, kitchen(false));
        assert_eq!(
            b.wants("alice", KITCHEN, &paused, NOW + 5_000),
            [Act::Verb {
                entity: KITCHEN.into(),
                verb: "media_pause",
                why: "the session paused"
            }]
        );
        let sought = session(&files, 0, Clock::Paused { at_ms: 40_000 }, kitchen(false));
        assert_eq!(
            b.wants("alice", KITCHEN, &sought, NOW + 6_000),
            [Act::Seek {
                entity: KITCHEN.into(),
                position_ms: 40_000
            }]
        );
        let resumed = session(
            &files,
            0,
            Clock::Playing {
                at_ms: 40_000,
                since: NOW + 7_000,
            },
            kitchen(false),
        );
        assert!(matches!(
            &b.wants("alice", KITCHEN, &resumed, NOW + 7_000)[..],
            [Act::Verb {
                verb: "media_play",
                ..
            }]
        ));
        let jumped = session(
            &files,
            1,
            Clock::Playing {
                at_ms: 0,
                since: NOW + 8_000,
            },
            kitchen(false),
        );
        assert!(matches!(
            &b.wants("alice", KITCHEN, &jumped, NOW + 8_000)[..],
            [Act::Start { at: 1, .. }]
        ));
    }

    /// A speaker sent somewhere else entirely is no longer in this session:
    /// once its grace is spent it lets the session go, as itself, said with
    /// what it reported — and stays let go rather than being recovered by
    /// the next report that happens to match.
    ///
    /// Falsified by `lapse` not removing the hold: the matching report after
    /// the release was read as the speaker playing ours again.
    #[test]
    fn a_speaker_playing_something_that_is_not_ours_is_let_go() {
        let mut b = bridge();
        started(&mut b, "alice", &["music/a.mp3"], 0);
        let radio = playing("http://radio.example/stream", 0);
        for _ in 0..3 {
            assert!(b.heard(KITCHEN, Some(&radio), NOW).is_empty());
        }
        let acts = b.heard(KITCHEN, Some(&radio), NOW);
        assert_eq!(calls(&acts), [("alice", "release")]);
        let Act::Call { why, .. } = &acts[0] else {
            unreachable!()
        };
        assert!(
            why.contains("radio.example"),
            "said out loud with what it reported: {why}"
        );
        assert!(b
            .heard(KITCHEN, Some(&playing(&url_of("music/a.mp3"), 0)), NOW)
            .is_empty());
        assert!(!b.holds(KITCHEN));
        let still = session(&["music/a.mp3"], 0, playing_at(0), kitchen(false));
        assert!(
            b.wants("alice", KITCHEN, &still, NOW).is_empty(),
            "not taken back while the release lands"
        );
    }

    /// A speaker handed a queue does not play it that instant: it fetches the
    /// first URL, reporting whatever it was playing before — fetching, not
    /// "not ours". Having started, silence is a lapse: three polls of it and
    /// the fourth lets it go; a poll that matched in between starts the
    /// three again. (Literal counts, so that the test does not shrink with
    /// the constants it holds.)
    ///
    /// Falsified by setting `WAIT` to 1: the first quiet poll of the
    /// hand-off released it.
    #[test]
    fn a_speaker_is_given_a_moment_to_start_before_it_is_let_go() {
        let mut b = bridge();
        b.wants(
            "alice",
            KITCHEN,
            &session(&["music/a.mp3"], 0, playing_at(0), kitchen(true)),
            NOW,
        );
        let quiet = Playing {
            state: "idle".into(),
            url: String::new(),
            position_ms: 0,
            updated_ms: None,
            duration_ms: None,
            queue_position: None,
            slack_ms: 0,
        };
        for tick in 0..3 {
            assert!(
                b.heard(KITCHEN, Some(&quiet), NOW).is_empty(),
                "still fetching, {tick} ticks in"
            );
        }
        assert_eq!(
            calls(&b.heard(KITCHEN, Some(&playing(&url_of("music/a.mp3"), 0)), NOW)),
            [("alice", "started")]
        );
        let landed = session(&["music/a.mp3"], 0, playing_at(0), kitchen(false));
        assert!(b.wants("alice", KITCHEN, &landed, NOW).is_empty());
        for tick in 0..3 {
            assert!(
                b.heard(KITCHEN, Some(&quiet), NOW).is_empty(),
                "a lapse, {tick} ticks in"
            );
        }
        assert_eq!(
            calls(&b.heard(KITCHEN, Some(&quiet), NOW)),
            [("alice", "release")],
            "silent for three seconds, it is not ours any more"
        );
        let mut b = bridge();
        started(&mut b, "alice", &["music/a.mp3"], 0);
        b.wants("alice", KITCHEN, &landed, NOW);
        for _ in 0..4 {
            b.heard(KITCHEN, Some(&playing(&url_of("music/a.mp3"), 0)), NOW);
            b.heard(KITCHEN, Some(&quiet), NOW);
            b.heard(KITCHEN, Some(&quiet), NOW);
        }
        assert_eq!(b.holder(KITCHEN), Some("alice"));
    }

    /// A speaker that has stopped answering is let go too, and whoever had it
    /// is told — not on the first poll it misses, though.
    ///
    /// Falsified by setting `LAPSE` to 0: the first unanswered poll released
    /// it.
    #[test]
    fn a_speaker_that_stops_answering_is_let_go() {
        let mut b = bridge();
        assert!(b.heard(KITCHEN, None, NOW).is_empty());
        started(&mut b, "alice", &["music/a.mp3"], 0);
        for miss in 0..3 {
            assert!(
                b.heard(KITCHEN, None, NOW).is_empty(),
                "still held after {miss} unanswered polls"
            );
        }
        assert_eq!(calls(&b.heard(KITCHEN, None, NOW)), [("alice", "release")]);
        assert_eq!(b.holder(KITCHEN), None);
    }

    /// One address, however the speaker spells it back.
    ///
    /// Falsified by an `address` that returned its argument: every
    /// re-spelling was a different address.
    #[test]
    fn an_address_is_the_same_however_it_is_spelt() {
        let ours = crate::domain::host::url(
            "http://10.0.0.2:8787",
            "music/Bach/Air (BWV 1068) & Café d'Été.mp3",
        );
        assert_eq!(
            ours,
            "http://10.0.0.2:8787/media/music/Bach/Air%20%28BWV%201068%29%20%26%20Caf%C3%A9%20d%27%C3%89t%C3%A9.mp3"
        );
        for spelt in [
            "http://10.0.0.2:8787/media/music/Bach/Air (BWV 1068) & Café d'Été.mp3",
            "http://10.0.0.2:8787/media/music/Bach/Air%20(BWV%201068)%20&amp;%20Caf%c3%a9%20d'%c3%89t%c3%a9.mp3",
            "HTTP://10.0.0.2:8787/media/music/Bach/Air (BWV 1068) &amp;amp; Caf%C3%A9 d&#39;%C3%89t%C3%A9.mp3",
            " http://10.0.0.2:8787/media/music/Bach/Air%20%28BWV%201068%29%20%26%20Caf%C3%A9%20d%27%C3%89t%C3%A9.mp3#t=0",
        ] {
            assert_eq!(address(spelt), address(&ours), "{spelt}");
        }
        assert_eq!(
            address("x-sonos-http://Box.lan:80/media/a%20b.mp3"),
            address("http://box.lan/media/a b.mp3")
        );
        assert_ne!(
            address("http://10.0.0.2:8787/media/music/a.mp3"),
            address("http://10.0.0.2:8787/media/music/b.mp3"),
            "and two files are still two"
        );
        assert_ne!(
            address("http://10.0.0.3:8787/media/music/a.mp3"),
            address("http://10.0.0.2:8787/media/music/a.mp3"),
            "nor is another host the same"
        );
    }

    /// A speaker that spells back what it was handed its own way still
    /// starts, and still moves through the queue — and a report that names
    /// the speaker's own queue rather than a track is read by its
    /// `queue_position`, counted from where it was handed.
    ///
    /// Falsified by matching the report byte for byte: the re-spelt first
    /// track was not ours, and nothing was said.
    #[test]
    fn a_speaker_that_spells_its_queue_its_own_way_is_still_playing_it() {
        let mut b = bridge();
        let files = ["music/Air (1).mp3", "music/B & C.mp3", "music/D.mp3"];
        b.wants(
            "alice",
            KITCHEN,
            &session(&files, 0, playing_at(0), kitchen(true)),
            NOW,
        );
        let spelt = |f: &str| {
            url_of(f)
                .replace("%20", " ")
                .replace("%28", "(")
                .replace("%29", ")")
                .replace("%26", "&amp;")
        };
        assert_eq!(
            calls(&b.heard(KITCHEN, Some(&playing(&spelt(files[0]), 0)), NOW)),
            [("alice", "started")]
        );
        b.wants(
            "alice",
            KITCHEN,
            &session(&files, 0, playing_at(0), kitchen(false)),
            NOW,
        );
        assert_eq!(
            calls(&b.heard(KITCHEN, Some(&playing(&spelt(files[1]), 0)), NOW)),
            [("alice", "advance")]
        );
        b.wants(
            "alice",
            KITCHEN,
            &session(&files, 1, playing_at(0), kitchen(false)),
            NOW,
        );
        let queued = Playing {
            queue_position: Some(3),
            ..playing("x-rincon-queue:RINCON_000E58C0FFEE01400#0", 0)
        };
        assert_eq!(
            calls(&b.heard(KITCHEN, Some(&queued), NOW)),
            [("alice", "advance")],
            "the third item of a queue handed from the first"
        );
        let past = Playing {
            queue_position: Some(4),
            ..queued
        };
        assert!(
            b.heard(KITCHEN, Some(&past), NOW).is_empty(),
            "past the end is not ours: a lapse"
        );
    }

    /// The session's queue edited while the speaker plays it: tracks put
    /// after the one playing are `enqueue: next`; tracks put at the end are
    /// `enqueue: add`; both at once are both; and anything else — a track
    /// taken out, one moved — is a fresh hand-off from where the track
    /// playing is.
    ///
    /// Falsified by an `enqueued` that read only a pure append: putting one
    /// next and one last at once was handed again.
    #[test]
    fn tracks_put_into_the_queue_reach_the_speakers_own_queue() {
        let mut b = bridge();
        let files = ["music/a.mp3", "music/b.mp3"];
        started(&mut b, "alice", &files, 0);
        let enq = |urls: &[&str], next: bool| Act::Enqueue {
            entity: KITCHEN.into(),
            urls: urls.iter().map(|f| url_of(f)).collect(),
            next,
        };
        // The tracks a queue is made of are the session's: an inserted one
        // shifts the ids of those after it, as `n` here says.
        let at = |files: &[&str]| session(files, 0, playing_at(0), kitchen(false));
        let edit = |s: &mut Listening, ids: &[u8]| {
            for (t, n) in s.session.as_mut().unwrap().queue.iter_mut().zip(ids) {
                t.id = track("", *n).id;
            }
        };
        let mut one = at(&["music/a.mp3", "music/n.mp3", "music/b.mp3", "music/z.mp3"]);
        edit(&mut one, &[0, 2, 1, 3]);
        assert_eq!(
            b.wants("alice", KITCHEN, &one, NOW),
            [enq(&["music/n.mp3"], true), enq(&["music/z.mp3"], false)]
        );
        let mut two = at(&[
            "music/a.mp3",
            "music/n.mp3",
            "music/b.mp3",
            "music/z.mp3",
            "music/y.mp3",
        ]);
        edit(&mut two, &[0, 2, 1, 3, 4]);
        assert_eq!(
            b.wants("alice", KITCHEN, &two, NOW),
            [enq(&["music/y.mp3"], false)]
        );
        let mut out = at(&["music/a.mp3", "music/b.mp3", "music/z.mp3", "music/y.mp3"]);
        edit(&mut out, &[0, 1, 3, 4]);
        assert!(
            matches!(
                &b.wants("alice", KITCHEN, &out, NOW)[..],
                [Act::Start { at: 0, .. }]
            ),
            "a track taken out is handed again"
        );
    }

    /// Two people can both pick the kitchen, because a kitchen is one room.
    /// The second gets it and the first is told — by the speaker, in the
    /// first one's session — and told once, however often the first
    /// session is read before the release lands.
    ///
    /// Falsified by a `wants` that took the speaker for a session that named
    /// it settled while another held it: alice took the kitchen back from
    /// bob on the next read.
    #[test]
    fn a_speaker_taken_by_somebody_else_releases_the_first() {
        let mut b = bridge();
        started(&mut b, "alice", &["music/a.mp3"], 0);
        let acts = b.wants(
            "bob",
            KITCHEN,
            &session(&["music/b.mp3"], 0, playing_at(0), kitchen(true)),
            NOW,
        );
        assert_eq!(calls(&acts), [("alice", "release")]);
        assert!(matches!(acts[1], Act::Start { .. }));
        assert_eq!(b.holder(KITCHEN), Some("bob"));
        // Alice's session still says the kitchen until her release lands.
        let alice = session(&["music/a.mp3"], 0, playing_at(0), kitchen(false));
        assert!(
            b.wants("alice", KITCHEN, &alice, NOW).is_empty(),
            "already told"
        );
        assert_eq!(b.holder(KITCHEN), Some("bob"));
        let Some(Act::Call { why, .. }) = acts.first() else {
            unreachable!()
        };
        assert!(why.contains("bob"));
    }

    /// A Sonos playing on through a track says the same `media_position`
    /// the whole way — Home Assistant refreshes it on a change of state, and
    /// dates it — so where it is, is that position counted on from its date.
    /// Twenty seconds of polls of a speaker playing on are twenty seconds of
    /// nothing to report.
    ///
    /// Falsified by reading `media_position` as now (`position_at` not
    /// counting on): the fourth poll reported `seek`, to 0.
    #[test]
    fn a_speaker_playing_on_reports_no_seek_from_a_frozen_position() {
        let mut b = bridge();
        started(&mut b, "alice", &["music/a.mp3"], 0);
        let anchored = session(&["music/a.mp3"], 0, playing_at(0), kitchen(false));
        assert!(b.wants("alice", KITCHEN, &anchored, NOW).is_empty());
        let frozen = playing(&url_of("music/a.mp3"), 0);
        for second in 1..=20 {
            let now = NOW + second * 1_000;
            let acts = b.heard(KITCHEN, Some(&frozen), now);
            assert!(calls(&acts).is_empty(), "{second} s in: {acts:?}");
            assert!(b.wants("alice", KITCHEN, &anchored, now).is_empty());
        }
        assert_eq!(
            frozen.position_at(NOW + 20_000),
            Some(20_000),
            "counted on, not stuck"
        );
        let undated = Playing {
            updated_ms: None,
            ..frozen.clone()
        };
        assert!(calls(&b.heard(KITCHEN, Some(&undated), NOW + 60_000)).is_empty());
        let paused = Playing {
            state: "paused".into(),
            ..playing(&url_of("music/a.mp3"), 12_000)
        };
        assert_eq!(paused.position_at(NOW + 99_000), Some(12_000));
        let short = Playing {
            duration_ms: Some(5_000),
            ..frozen
        };
        assert_eq!(short.position_at(NOW + 99_000), Some(5_000));
    }

    /// A volume told to the house is `volume_set` (and `volume_mute` only
    /// where the mute moved), and for the polls the house takes to catch up
    /// the speaker reads as what it was told: the house a poll behind is
    /// not a hand on the knob, and after it a difference is one — a knob
    /// turned, or a house that never took what it was told, said as it is.
    ///
    /// Falsified by a `volume` that read the house whatever the quiet: the
    /// house that never took 60 went on saying 35, and nothing was said.
    #[test]
    fn a_volume_told_is_what_the_house_reads_until_it_catches_up() {
        let stood = |volume: i64, muted: bool| Present {
            can_volume: true,
            volume,
            muted,
            family: Some("sonos".into()),
            ..device(KITCHEN, Kind::Speaker, true)
        };
        let house = |v: i64, muted: bool| Speaker {
            volume: Some(v),
            muted,
            can_volume: true,
            family: Some("sonos".into()),
            members: vec![],
        };
        let phone = Output::Device {
            id: "phone".into(),
            moving: false,
        };
        let with = |d: Present| {
            let mut s = session(&["music/a.mp3"], 0, playing_at(0), phone.clone());
            s.devices[1] = d;
            s
        };
        let set = |b: &mut Speakers| {
            b.polled(KITCHEN, Some(&house(35, false)));
            assert!(b
                .wants("alice", KITCHEN, &with(stood(35, false)), NOW)
                .is_empty());
            assert_eq!(
                b.wants("alice", KITCHEN, &with(stood(60, false)), NOW),
                [Act::Volume {
                    entity: KITCHEN.into(),
                    level: 60
                }]
            );
            b.polled(KITCHEN, Some(&house(35, false)));
            assert!(
                b.wants("alice", KITCHEN, &with(stood(60, false)), NOW)
                    .is_empty(),
                "the house catching up"
            );
        };
        let mut b = bridge();
        set(&mut b);
        b.polled(KITCHEN, Some(&house(60, false)));
        assert!(b
            .wants("alice", KITCHEN, &with(stood(60, false)), NOW)
            .is_empty());
        b.polled(KITCHEN, Some(&house(22, true)));
        let acts = b.wants("alice", KITCHEN, &with(stood(60, false)), NOW);
        assert_eq!(calls(&acts), [("alice", "report_volume")]);
        assert_eq!(input_of(&acts, 0), input::report_volume(22, true));
        assert_eq!(
            b.wants("alice", KITCHEN, &with(stood(22, false)), NOW),
            [Act::Mute {
                entity: KITCHEN.into(),
                muted: false
            }],
            "a mute moved is a mute, and nothing else"
        );
        // A house that never took it is where it is.
        let mut b = bridge();
        set(&mut b);
        b.polled(KITCHEN, Some(&house(35, false)));
        let acts = b.wants("alice", KITCHEN, &with(stood(60, false)), NOW);
        assert_eq!(calls(&acts), [("alice", "report_volume")]);
        assert_eq!(input_of(&acts, 0), input::report_volume(35, false));
    }

    /// A speaker that stops standing pauses what it holds for the session
    /// that holds it, and only for that one.
    ///
    /// Falsified by `left` pausing whatever it was asked about: bob closing
    /// his app paused alice's kitchen.
    #[test]
    fn a_speaker_left_pauses_only_its_holders_sound() {
        let mut b = bridge();
        started(&mut b, "alice", &["music/a.mp3"], 0);
        assert!(b.left("bob", KITCHEN).is_empty(), "not bob's to pause");
        assert!(matches!(
            &b.left("alice", KITCHEN)[..],
            [Act::Verb {
                verb: "media_pause",
                ..
            }]
        ));
        assert!(!b.holds(KITCHEN));
    }
}

/// The thread: Home Assistant's REST API, and the sessions.
///
/// Blocking and polled, on a thread of its own, exactly as the scanner is.
/// There is no async in this server's own code and a bridge to a house is
/// not a reason to start — a handful of small requests a second on a LAN is
/// not a problem worth a runtime.
///
/// **Each speaker is a device**, one connection to the hub per session it
/// stands in ([`ark_peer::Handle::connect`]): a volatile machine of its
/// own, signed in as the session's account on a session of its own
/// ([`speaker`]), which says in its `Hello` that it is a speaker — so the
/// hub authors its `on_join`, and its `on_leave` when the connection
/// closes. It has no log and no socket. The frames it is sent keep its
/// machine current and wake the thread, which reads the sessions whole
/// from the hub, which is in this process.
///
/// A device is a session (`harken/domain/devices.ark`), so a speaker's id
/// in the session is its connection's session, `user@entity`, which two
/// accounts' kitchens do not share; the bridge reads and says it as the
/// entity ([`as_entities`]). A speaker's machine is kept for the life of
/// the thread and connects again when it stands again, so the entries it
/// authors go on from its last `local`.
///
/// Every speaker stands in a session while a client is in it
/// ([`listening`]), so the presence a person sees is theirs and the speakers
/// beside it, never a shelf of speakers in an account nobody has opened.
/// Where no client is, the speakers making the sound go on standing
/// ([`playing`]) — the music is the kitchen's, not the browser's that sent it
/// there — for as long as the session sounds and for [`IDLE`] after it stops;
/// then they go too, and the session is kept, paused, as any is. It polls
/// only the speakers standing: a server whose house nobody is listening in
/// makes no requests at all.
pub mod ha {
    use std::collections::{HashMap, VecDeque};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::sync::Arc;
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    use crate::domain::host::{Kind, Listening};
    use crate::domain::{Device, Output as Sound};
    use crate::domain::{Listening as Row, ListeningDevice};
    use ark::format::Encode;
    use ark::peer::{Input, Output, Peer, Role, Who};
    use ark::schema::Space;
    use ark::store::{Read, Scan};
    use ark_peer::Handle;
    use ark_peer::{now_ms, ConnId};

    use super::{input, listening, playing, Act, Playing, Speaker, Speakers, GROUPING, VOLUME_SET};
    use crate::hosted::Hosted;

    /// How many of a handed queue's tracks are pushed at a speaker on one
    /// pass of the bridge. The first track goes at once, with its seek; the
    /// rest follow this many at a time between polls, so a queue of two
    /// hundred — two hundred requests, each a round trip to the speaker —
    /// does not leave every session in the house unread, and the hand-off
    /// standing still, for the minute it takes.
    pub const PUSH: usize = 8;

    /// How often a held player is asked what it is doing.
    pub const TICK: Duration = Duration::from_millis(1_000);

    /// How long a speaker making no sound goes on standing in a session no
    /// client is in. Long enough that a pause to answer the door, or a queue
    /// that ran out while somebody chose the next, finds the speaker there
    /// and the session as it was; short enough that a house left paused at
    /// night stops being asked about by morning.
    pub const IDLE: Duration = Duration::from_secs(10 * 60);

    /// How much of a queue is pushed at a speaker. Enough that nobody reaches
    /// the end of it by hand, and few enough that starting a track is not a
    /// hundred service calls.
    pub const WINDOW: usize = 200;

    /// How long one request to the house may take. A house that does not
    /// answer is asked again next tick; a request that never returns would
    /// stop the bridge for every session.
    const TIMEOUT: Duration = Duration::from_secs(5);

    /// What the server was told about the house.
    #[derive(Clone, Debug)]
    pub struct Config {
        /// Where Home Assistant is, e.g. `http://homeassistant.local:8123`.
        pub url: String,
        /// A long-lived access token, read from a file by the caller — never
        /// a store path and never an argument.
        pub token: String,
        /// Which entities to offer, and what to call them.
        pub players: Vec<(String, String)>,
        /// Where a *speaker* fetches bytes from, which is not where a phone
        /// does: the phone may be on `https://harken.example.com` while the
        /// speaker only knows an address on the LAN.
        pub media: String,
        /// How often to poll; [`TICK`] outside a test.
        pub tick: Duration,
        /// How long a silent speaker stands where nobody is listening;
        /// [`IDLE`] outside a test.
        pub idle: Duration,
        /// One line per poll per speaker: what the house said — state,
        /// `media_content_id`, `media_position`, `media_position_updated_at`,
        /// its `Date` — where the bridge reads the speaker as being and has
        /// the session, and what it did about it. `HARKEN_BRIDGE_TRACE=1`
        /// prints them; off by default, being a line a second for every
        /// speaker standing.
        pub trace: Trace,
    }

    /// Where the bridge's trace goes ([`Config::trace`]).
    #[derive(Clone, Debug, Default)]
    pub enum Trace {
        #[default]
        Off,
        /// Standard output, so the journal: `journalctl -u harken`.
        Out,
        /// Each line sent here, for a test to read.
        Lines(Sender<String>),
    }

    impl std::fmt::Display for Config {
        /// Everything but the token.
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let names: Vec<&str> = self.players.iter().map(|(id, _)| id.as_str()).collect();
            write!(
                f,
                "{} player(s) from {} ({}), fetching from {}",
                names.len(),
                self.url,
                names.join(", "),
                self.media
            )
        }
    }

    /// Kept alive for the life of the process; dropping it stops the bridge
    /// and takes every speaker out of every session.
    pub struct Assistant {
        stop: Arc<AtomicBool>,
        wake: Sender<()>,
        thread: Option<JoinHandle<()>>,
    }

    impl Drop for Assistant {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            let _ = self.wake.send(());
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    /// Offer the house's players to every account somebody is listening as.
    pub fn start(config: Config, hub: Handle, hosted: Hosted) -> Assistant {
        let stop = Arc::new(AtomicBool::new(false));
        let (wake, woken) = channel();
        let flag = stop.clone();
        let waker = wake.clone();
        #[allow(clippy::let_and_return)]
        let thread = std::thread::Builder::new()
            .name("harken-house".into())
            .spawn(move || {
                let mut bridge = Speakers::new(config.players.clone(), &config.media);
                let house = House::new(&config);
                let mut wires = Wires {
                    hub,
                    hosted,
                    entities: config.players.iter().map(|(e, _)| e.clone()).collect(),
                    machines: HashMap::new(),
                    standing: HashMap::new(),
                    frames: channel(),
                    waker,
                };
                run(&config, &house, &mut bridge, &mut wires, &woken, &flag);
            })
            .ok();
        if thread.is_none() {
            eprintln!("harken-server: house: could not start its thread");
        }
        Assistant { stop, wake, thread }
    }

    /// A speaker's session in `user`'s listening session: its connection's,
    /// and so its id there.
    pub fn speaker(user: &str, entity: &str) -> String {
        format!("{user}@{entity}")
    }

    /// `user`'s session with each of `entities`' ids ([`speaker`]) read as
    /// the entity, which is how the bridge names a speaker.
    pub fn as_entities(mut s: Listening, user: &str, entities: &[String]) -> Listening {
        let of = |id: &mut String| {
            if let Some(e) = entities.iter().find(|e| *id == speaker(user, e)) {
                *id = e.clone();
            }
        };
        for d in &mut s.devices {
            of(&mut d.id);
            if let Some(f) = &mut d.follows {
                of(f);
            }
        }
        if let Some(a) = &mut s.session {
            if let Sound::Device { id, .. } | Sound::Away { id, .. } = &mut a.output {
                of(id);
            }
        }
        s.scrub = std::mem::take(&mut s.scrub)
            .into_iter()
            .map(|(mut id, at)| {
                of(&mut id);
                (id, at)
            })
            .collect();
        s
    }

    /// Every session, as the hub holds it now: each account with a row in
    /// the session's tables, its speakers named by their entities; and the
    /// head of the volatile log they were read at.
    fn sessions(hub: &Handle, entities: &[String]) -> (i64, Vec<(String, Listening)>) {
        let read = hub.with(|h| {
            let live = h.live.as_ref()?;
            let (head, store) = (live.log().head().0, &live.store);
            let mut users: Vec<String> = store
                .scan("listening", &Scan::all())
                .iter()
                .filter_map(|r| Row::from_bytes(r).ok())
                .map(|r| r.user)
                .chain(
                    store
                        .scan("listening_device", &Scan::all())
                        .iter()
                        .filter_map(|r| ListeningDevice::from_bytes(r).ok())
                        .map(|r| r.user),
                )
                .collect();
            users.dedup();
            users.sort();
            users.dedup();
            Some((
                head,
                users
                    .into_iter()
                    .map(|u| (u.clone(), Listening::of(store, &u)))
                    .collect::<Vec<_>>(),
            ))
        });
        let (head, read) = read.flatten().unwrap_or_default();
        let read = read
            .into_iter()
            .map(|(u, s)| {
                let s = as_entities(s, &u, entities);
                (u, s)
            })
            .collect();
        (head, read)
    }

    /// A frame the hub sent the speaker at (account, entity).
    type Framed = ((String, String), Vec<u8>);

    /// The speakers' machines and their connections to the hub.
    struct Wires {
        hub: Handle,
        hosted: Hosted,
        /// The entities offered.
        entities: Vec<String>,
        /// Each speaker's volatile machine in each session it has stood
        /// in, by (account, entity): kept, so its `local` goes on.
        machines: HashMap<(String, String), Peer>,
        /// The connections of those standing now.
        standing: HashMap<(String, String), ConnId>,
        /// Frames the hub sent a speaker, and the thread woken for them.
        frames: (Sender<Framed>, Receiver<Framed>),
        waker: Sender<()>,
    }

    impl Wires {
        /// Whether every speaker standing in `user`'s session has heard the
        /// hub's volatile log to `head` — the head the sessions were read at
        /// ([`sessions`]) — no more and no less, and has nothing of its own
        /// unanswered: only then is the session acted on, so that every
        /// speaker in it acts on that one moment and what each says is
        /// previewed over it. Otherwise the session waits for the next frame,
        /// which wakes the thread. In this process the hub's order of jobs
        /// leaves a speaker no further behind than its last unanswered call
        /// (a job's frames are delivered before the next job, the read
        /// among them), but not never ahead: [`Handle::with`] answers from
        /// inside its job, so the next job's frames can reach one speaker's
        /// channel and not yet another's before [`Wires::receive`] takes
        /// them. The rule is kept here, not left to that order.
        fn agree(&self, user: &str, head: i64) -> bool {
            self.standing.keys().filter(|(u, _)| u == user).all(|k| {
                self.machines
                    .get(k)
                    .is_some_and(|m| m.pending.is_empty() && m.log().head().0 == head)
            })
        }

        fn stands(&self, k: &(String, String)) -> bool {
            self.standing.contains_key(k)
        }

        /// The speaker `entity`, called `name`, standing in `user`'s session.
        fn stand(&mut self, user: &str, entity: &str, name: &str) {
            let k = (user.to_string(), entity.to_string());
            let who = Who {
                actor: user.into(),
                session: speaker(user, entity),
                roles: vec![],
            };
            let module = &self.hosted.module;
            let peer = self.machines.entry(k.clone()).or_insert_with(|| {
                let mut p = Peer::new(
                    module.schema.clone(),
                    module.functions.clone(),
                    Role::Replica,
                    who.clone(),
                )
                .in_space(Space::Volatile);
                let said = Device {
                    can_play: true,
                    id: entity.into(),
                    kind: Kind::Speaker.name().into(),
                    name: name.into(),
                    user: user.into(),
                };
                p.device = Some(said.to_bytes());
                p
            });
            let (to, wake, key) = (self.frames.0.clone(), self.waker.clone(), k.clone());
            let c = self.hub.connect(
                who,
                Box::new(move |b| {
                    let _ = wake.send(());
                    to.send((key.clone(), b)).is_ok()
                }),
            );
            let out = peer.step(Input::Connected);
            self.standing.insert(k, c);
            self.send(c, out);
        }

        /// The speaker out of the session: its connection closed, which is
        /// its `on_leave`.
        fn leave(&mut self, k: &(String, String)) {
            if let Some(c) = self.standing.remove(k) {
                self.hub.close(c);
            }
        }

        /// A speaker's machine's frames to its connection; a refusal of
        /// what it said, said out loud.
        fn send(&self, c: ConnId, out: Vec<Output>) {
            for o in out {
                match o {
                    Output::Send(b) => self.hub.frame(c, b),
                    Output::Refused { why, .. } => {
                        eprintln!("harken-server: house: a speaker's transition was refused: {why}")
                    }
                    _ => {}
                }
            }
        }

        /// What the hub sent each speaker, to its machine.
        fn receive(&mut self) {
            let got: Vec<_> = self.frames.1.try_iter().collect();
            for (k, bytes) in got {
                let (Some(peer), Some(c)) =
                    (self.machines.get_mut(&k), self.standing.get(&k).copied())
                else {
                    continue;
                };
                let out = peer.step(Input::Frame { from: None, bytes });
                self.send(c, out);
            }
        }

        /// A transition of `user`'s session, called as the speaker `entity`.
        fn call(&mut self, user: &str, entity: &str, transition: &str, input: Vec<u8>) {
            let k = (user.to_string(), entity.to_string());
            let Some(c) = self.standing.get(&k).copied() else {
                return;
            };
            let Some(func) = self.hosted.get(&format!("listening.{transition}")) else {
                return;
            };
            let Some(peer) = self.machines.get_mut(&k) else {
                return;
            };
            let out = peer.step(Input::Mutate {
                func,
                input,
                now: now_ms(),
            });
            self.send(c, out);
        }

        fn drain(&mut self) {
            let all: Vec<_> = self.standing.keys().cloned().collect();
            for k in all {
                self.leave(&k);
            }
        }
    }

    fn run(
        config: &Config,
        house: &House,
        bridge: &mut Speakers,
        wires: &mut Wires,
        woken: &Receiver<()>,
        stop: &AtomicBool,
    ) {
        let mut polled = Instant::now() - config.tick;
        // Since when each session with nobody listening and a speaker
        // standing has been silent.
        let mut silent: HashMap<String, Instant> = HashMap::new();
        // The rest of each handed queue, still to be pushed at its speaker.
        let mut pushing: HashMap<String, VecDeque<String>> = HashMap::new();
        loop {
            if stop.load(Ordering::Relaxed) {
                wires.drain();
                return;
            }
            let now = now_ms();
            let mut acts = vec![];
            let entities: Vec<String> = bridge.players().iter().map(|(e, _)| e.clone()).collect();
            let (head, states) = sessions(&wires.hub, &entities);
            // Every frame the hub sent the speakers before it was read is in
            // their channel by now (the hub delivers a step's frames before
            // it takes the next job): each speaker's machine holds at least
            // the state the bridge acts on, so what it says as the speaker
            // is previewed over that and not refused for being behind.
            wires.receive();
            // Where to stand, and which speakers: every one beside a client;
            // where no client is, the ones making the sound, until it has
            // been silent for `idle`.
            let mut stood: Vec<(&String, &Listening, Vec<String>)> = vec![];
            for (key, s) in &states {
                if listening(s) {
                    silent.remove(key);
                    stood.push((key, s, entities.clone()));
                    continue;
                }
                let making = playing(s, entities.iter().map(String::as_str));
                if making.is_empty() {
                    silent.remove(key);
                    continue;
                }
                if s.sounding() {
                    silent.remove(key);
                } else if silent
                    .entry(key.clone())
                    .or_insert_with(Instant::now)
                    .elapsed()
                    >= config.idle
                {
                    continue;
                }
                stood.push((key, s, making));
            }
            let wanted = |key: &String, entity: &String| {
                stood
                    .iter()
                    .any(|(k, _, es)| *k == key && es.contains(entity))
            };
            // Stand where wanted…
            for (key, _, es) in &stood {
                for entity in es {
                    let k = ((*key).clone(), entity.clone());
                    if wires.stands(&k) {
                        continue;
                    }
                    let name = bridge
                        .players()
                        .iter()
                        .find(|(e, _)| e == entity)
                        .map(|(_, n)| n.clone())
                        .unwrap_or_default();
                    wires.stand(key, entity, &name);
                }
            }
            // …and nowhere else: a speaker that is not making the sound does
            // not keep a session's presence open, and one silent where nobody
            // is listening for longer than `idle` goes, pausing what it held.
            let gone: Vec<(String, String)> = wires
                .standing
                .keys()
                .filter(|(key, entity)| !wanted(key, entity))
                .cloned()
                .collect();
            for k in gone {
                wires.leave(&k);
                for act in bridge.left(&k.0, &k.1) {
                    println!(
                        "harken-server: house: {}: nobody is listening as {} and it is not playing; paused",
                        k.1, k.0
                    );
                    acts.push(act);
                }
            }
            // What each session wants of each speaker standing in it — its
            // volume and its group before its queue, so that a member about
            // to lead is made the leader in the house before anything is
            // handed to it: a queue handed to a member is its leader's.
            // Only a session every speaker standing in it agrees on
            // ([`Wires::agree`]).
            for (key, state, es) in &stood {
                if !wires.agree(key, head) {
                    continue;
                }
                for entity in es {
                    acts.extend(bridge.wants(key, entity, state, now));
                }
            }
            // Told before anything is read back, so that what a poll below
            // says is always after what the house was told: a reading taken
            // before a hand-off was performed is the speaker's last queue.
            for act in acts {
                perform(house, wires, &mut pushing, act);
            }
            push(house, &mut pushing);
            // …and, a tick at a time, what the standing ones are actually
            // doing: every one's volume and group, and what the held ones
            // are playing. A speaker stands only where somebody is listening
            // or where it is playing, so a house nobody is listening in, and
            // that plays nothing, is asked nothing.
            if polled.elapsed() >= config.tick {
                polled = Instant::now();
                let standing: Vec<String> = bridge
                    .players()
                    .iter()
                    .map(|(e, _)| e.clone())
                    .filter(|e| wires.standing.keys().any(|(_, w)| w == e))
                    .collect();
                for entity in standing {
                    let reading = house.read(&entity);
                    let poll = reading.as_ref().map(|r| &r.playing);
                    bridge.polled(&entity, reading.as_ref().map(|r| &r.speaker));
                    let now = now_ms();
                    let heard = bridge.heard(&entity, poll, now);
                    if !matches!(config.trace, Trace::Off) {
                        let line = format!(
                            "harken-server: house: trace: {entity}: {}; {}; {}",
                            reading
                                .as_ref()
                                .map_or("no answer".to_string(), |r| r.said.clone()),
                            bridge.describe(&entity, poll, now),
                            done(&heard)
                        );
                        match &config.trace {
                            Trace::Lines(to) => {
                                let _ = to.send(line);
                            }
                            _ => println!("{line}"),
                        }
                    }
                    // Said at once, each poll's: a position is as of the poll.
                    for act in heard {
                        perform(house, wires, &mut pushing, act);
                    }
                }
            }
            // Until a frame, the next poll, or a stop — or at once, while a
            // queue is still being pushed.
            let wait = match pushing.is_empty() {
                true => config
                    .tick
                    .saturating_sub(polled.elapsed())
                    .max(Duration::from_millis(10)),
                false => Duration::ZERO,
            };
            let _ = woken.recv_timeout(wait);
            while woken.try_recv().is_ok() {}
        }
    }

    /// What a poll's acts were, for the trace: the transitions said as the
    /// speaker, with the point each was said at, and what the house was told.
    fn done(acts: &[Act]) -> String {
        if acts.is_empty() {
            return "nothing done".to_string();
        }
        let each: Vec<String> = acts
            .iter()
            .map(|a| match a {
                Act::Call {
                    transition, input, ..
                } => match input::ms(transition, input) {
                    Some(ms) => format!("{transition} {ms} ms"),
                    None => transition.to_string(),
                },
                Act::Seek { position_ms, .. } => format!("media_seek {position_ms} ms"),
                Act::Verb { verb, .. } => verb.to_string(),
                other => format!("{other:?}"),
            })
            .collect();
        format!("did {}", each.join(", "))
    }

    /// The next few tracks of each queue still being pushed ([`PUSH`]).
    fn push(house: &House, pushing: &mut HashMap<String, VecDeque<String>>) {
        for (entity, rest) in pushing.iter_mut() {
            for url in rest.drain(..rest.len().min(PUSH)).collect::<Vec<_>>() {
                house.call(entity, "play_media", Some(media(&url, "add")));
            }
        }
        pushing.retain(|_, rest| !rest.is_empty());
    }

    fn seek(position_ms: i64) -> serde_json::Value {
        // Home Assistant takes seconds, as a number: the one float here, and
        // it is on the wire to the house, never in anything a replica applies.
        serde_json::json!({ "seek_position": position_ms as f64 / 1000.0 })
    }

    fn perform(
        house: &House,
        wires: &mut Wires,
        pushing: &mut HashMap<String, VecDeque<String>>,
        act: Act,
    ) {
        match act {
            Act::Call {
                key,
                entity,
                transition,
                mut input,
                why,
                as_of,
            } => {
                // A playing speaker's point, counted on from when it was
                // measured to now, when it is said.
                if let Some(at) = as_of {
                    input = input::later(transition, &input, (now_ms() - at).max(0));
                }
                // A release is said out loud, because a speaker being let go
                // looks exactly like one that stopped for no reason, and the
                // reason is only ever known here.
                if transition == "release" {
                    println!("harken-server: house: {entity}: {why}; let go of {key}'s session");
                }
                // The members a speaker reports are speakers: by their ids in
                // this session.
                if transition == "report_group" {
                    if let Ok(mut g) = crate::domain::ReportGroup::from_bytes(&input) {
                        for m in &mut g.members {
                            if wires.entities.contains(m) {
                                *m = speaker(&key, m);
                            }
                        }
                        input = g.to_bytes();
                    }
                }
                wires.call(&key, &entity, transition, input);
            }
            Act::Enqueue { entity, urls, next } => {
                // At the end of a queue still being pushed is after what is
                // still to come.
                if let (false, Some(rest)) = (next, pushing.get_mut(&entity)) {
                    rest.extend(urls);
                    return;
                }
                println!(
                    "harken-server: house: {entity}: {} track(s) {}",
                    urls.len(),
                    if next { "put next" } else { "added to the end" }
                );
                // `next` puts each after the item playing, so the last goes
                // in first and the first ends up next.
                let ordered: Vec<&String> = match next {
                    true => urls.iter().rev().collect(),
                    false => urls.iter().collect(),
                };
                for url in ordered {
                    house.call(
                        &entity,
                        "play_media",
                        Some(media(url, if next { "next" } else { "add" })),
                    );
                }
            }
            Act::Verb { entity, verb, why } => {
                if verb == "media_pause" {
                    println!("harken-server: house: {entity}: {why}; paused");
                }
                house.call(&entity, verb, None);
            }
            Act::Seek {
                entity,
                position_ms,
            } => {
                house.call(&entity, "media_seek", Some(seek(position_ms)));
            }
            Act::Volume { entity, level } => {
                // A level of 0 to 1 on the wire: a float to the house, never
                // in anything a replica applies.
                house.call(
                    &entity,
                    "volume_set",
                    Some(serde_json::json!({ "volume_level": level.clamp(0, 100) as f64 / 100.0 })),
                );
            }
            Act::Mute { entity, muted } => {
                house.call(
                    &entity,
                    "volume_mute",
                    Some(serde_json::json!({ "is_volume_muted": muted })),
                );
            }
            Act::Join { leader, members } => {
                println!(
                    "harken-server: house: {leader}: joined by {}",
                    members.join(", ")
                );
                house.call(
                    &leader,
                    "join",
                    Some(serde_json::json!({ "group_members": members })),
                );
            }
            Act::Unjoin { entity } => {
                println!("harken-server: house: {entity}: unjoined");
                house.call(&entity, "unjoin", None);
            }
            Act::Start {
                entity,
                urls,
                at,
                position_ms,
                playing,
            } => {
                // The queue is cleared, the one it should be on goes in, and
                // the rest go after it — which is what leaves the speaker's
                // own next and previous working, and the Sonos app with a
                // queue in it. Previous walks back as far as the track you
                // started from and no further.
                //
                // `play` rather than `replace`, because for a bare URL the
                // Sonos integration reads `replace` as `play_uri` — a direct
                // `SetAVTransportURI` that detaches the transport from the
                // queue. The adds still land, so the speaker holds a queue it
                // is not playing from, and `media_next_track` blocks until
                // SoCo gives up at 9.5s.
                let Some(first) = urls.get(at) else {
                    return;
                };
                println!(
                    "harken-server: house: {entity}: handed {} track(s) from {at}, {}",
                    urls.len(),
                    if playing { "playing" } else { "paused" }
                );
                pushing.remove(&entity);
                house.call(&entity, "clear_playlist", None);
                if !house.call(&entity, "play_media", Some(media(first, "play"))) {
                    return;
                }
                // Halfway through a track is where it picks up: the queue
                // starts the track from the beginning and this moves it to
                // where the session actually is. Issued against a speaker that
                // may not have read any of the stream yet, which a Sonos drops
                // — the bridge tells it again once it reports the track
                // (`SEEKS`) — and sent before the rest of the queue, which is
                // pushed a few at a time from here on, so that it lands on the
                // track it is about.
                if position_ms > 0 {
                    house.call(&entity, "media_seek", Some(seek(position_ms)));
                }
                if !playing {
                    house.call(&entity, "media_pause", None);
                }
                let rest: VecDeque<String> =
                    urls.iter().skip(at + 1).take(WINDOW).cloned().collect();
                if !rest.is_empty() {
                    pushing.insert(entity.clone(), rest);
                }
            }
        }
    }

    fn media(url: &str, enqueue: &str) -> serde_json::Value {
        serde_json::json!({
            "media_content_id": url,
            // `music` rather than the file's own type: it is what every
            // `media_player` platform understands, and the speaker sniffs the
            // stream for the rest.
            "media_content_type": "music",
            "enqueue": enqueue,
        })
    }

    /// The family of a player that groups, where Home Assistant will not
    /// say which integration it is: one family for the whole house. Right
    /// for a house of one make, and harmless in one of two — the house
    /// refuses a join across makes, and the group it reports back is the
    /// truth the session then takes.
    const HOUSE_FAMILY: &str = "home-assistant";

    /// Home Assistant's REST API, the three calls of it this needs.
    struct House {
        base: String,
        bearer: String,
        agent: ureq::Agent,
        /// Each grouping player's family, asked once.
        families: std::sync::Mutex<HashMap<String, String>>,
        /// The house's clock, as its answers have narrowed it.
        skew: std::sync::Mutex<super::when::Skew>,
    }

    /// One answer about a player: what it is doing, its volume and group,
    /// and — for the trace — what the house said, as it said it.
    struct Reading {
        playing: Playing,
        speaker: Speaker,
        said: String,
    }

    impl House {
        fn new(config: &Config) -> House {
            House {
                base: config.url.trim_end_matches('/').to_string(),
                bearer: format!("Bearer {}", config.token),
                agent: ureq::AgentBuilder::new().timeout(TIMEOUT).build(),
                families: Default::default(),
                skew: Default::default(),
            }
        }

        /// Which integration a player is, which is the family it groups in:
        /// Home Assistant's state for an entity does not say, and its
        /// template API does — `config_entry_attr(config_entry_id(e),
        /// 'domain')` is `sonos` for a Sonos. Asked once per player; a house
        /// that cannot answer it (an older Home Assistant) puts every
        /// grouping player in [`HOUSE_FAMILY`].
        fn family(&self, entity: &str) -> String {
            if let Some(f) = self
                .families
                .lock()
                .ok()
                .and_then(|m| m.get(entity).cloned())
            {
                return f;
            }
            let template =
                format!("{{{{ config_entry_attr(config_entry_id('{entity}'), 'domain') }}}}");
            let answer = self
                .agent
                .post(&format!("{}/api/template", self.base))
                .set("Authorization", &self.bearer)
                .send_json(serde_json::json!({ "template": template }))
                .ok()
                .and_then(|r| r.into_string().ok())
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty() && t != "None" && !t.contains(char::is_whitespace));
            let family = answer.unwrap_or_else(|| HOUSE_FAMILY.to_string());
            if let Ok(mut m) = self.families.lock() {
                m.insert(entity.to_string(), family.clone());
            }
            family
        }

        /// `media_player.<service>` on `entity`. Whether it went through, and
        /// nothing about why it did not: a house that is asleep and one that
        /// refused both mean the speaker did not do it, and both are answered
        /// by the next tick asking again.
        fn call(&self, entity: &str, service: &str, extra: Option<serde_json::Value>) -> bool {
            let mut body = serde_json::json!({ "entity_id": entity });
            if let (Some(serde_json::Value::Object(extra)), Some(map)) =
                (extra, body.as_object_mut())
            {
                map.extend(extra);
            }
            let sent = self
                .agent
                .post(&format!(
                    "{}/api/services/media_player/{service}",
                    self.base
                ))
                .set("Authorization", &self.bearer)
                .send_json(body);
            if let Err(e) = &sent {
                eprintln!("harken-server: house: {entity}: {service} failed: {e}");
            }
            sent.is_ok()
        }

        /// What one player is doing, and its volume and group, or `None`
        /// if it will not say.
        fn read(&self, entity: &str) -> Option<Reading> {
            let sent = now_ms();
            let response = self
                .agent
                .get(&format!("{}/api/states/{entity}", self.base))
                .set("Authorization", &self.bearer)
                .call()
                .ok()?;
            let got = now_ms();
            // The house's clock, as its answers are dated: what its
            // timestamps are read against, so a house whose clock is not the
            // server's still places a position right — to the round trip,
            // once a few answers have narrowed it ([`super::when::Skew`]).
            let date = response.header("date").map(str::to_string);
            let house_now = date.as_deref().and_then(super::when::http_date_ms);
            let skew = match (house_now, self.skew.lock()) {
                (Some(d), Ok(mut skew)) => {
                    skew.answer(d, sent, got);
                    *skew
                }
                (_, Ok(skew)) => *skew,
                (_, Err(_)) => Default::default(),
            };
            let body: serde_json::Value = response.into_json().ok()?;
            let state = body.get("state")?.as_str()?.to_string();
            if state == "unavailable" || state == "unknown" {
                return None;
            }
            let attrs = body.get("attributes")?;
            let features = attrs
                .get("supported_features")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let speaker = Speaker {
                // As 0 to 100, rounded: the one float read from the house,
                // and never in anything a replica applies.
                volume: attrs
                    .get("volume_level")
                    .and_then(|v| v.as_f64())
                    .map(|v| (v * 100.0).round().clamp(0.0, 100.0) as i64),
                muted: attrs
                    .get("is_volume_muted")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                can_volume: features & VOLUME_SET != 0,
                family: (features & GROUPING != 0).then(|| self.family(entity)),
                members: attrs
                    .get("group_members")
                    .and_then(|v| v.as_array())
                    .map(|xs| {
                        xs.iter()
                            .filter_map(|x| x.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
            };
            let stamped = attrs
                .get("media_position_updated_at")
                .and_then(|v| v.as_str())
                .and_then(super::when::rfc3339_ms);
            let playing = Playing {
                state,
                url: attrs
                    .get("media_content_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                // As the house last measured it — at a change of state, and
                // not since: a playing Sonos says the position its track
                // started at for the whole of the track. [`Playing::position_at`]
                // counts it on from `media_position_updated_at`.
                position_ms: (attrs
                    .get("media_position")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0)
                    * 1000.0) as i64,
                updated_ms: stamped.map(|at| skew.ours(at).map_or(at, |(ms, _)| ms)),
                duration_ms: attrs
                    .get("media_duration")
                    .and_then(|v| v.as_f64())
                    .map(|d| (d * 1000.0) as i64),
                queue_position: attrs
                    .get("queue_position")
                    .and_then(|v| v.as_u64())
                    .and_then(|n| usize::try_from(n).ok()),
                // With no `Date` at all the two clocks are taken to agree,
                // which on one LAN with NTP they do to well inside the bound.
                slack_ms: stamped
                    .and_then(|at| skew.ours(at))
                    .map_or(0, |(_, slack)| slack),
            };
            let said = format!(
                "state {:?}, media_content_id {:?}, media_position {}, media_position_updated_at {}, Date {}",
                playing.state,
                playing.url,
                attrs.get("media_position").map_or("none".to_string(), |v| v.to_string()),
                attrs.get("media_position_updated_at").map_or("none".to_string(), |v| v.to_string()),
                date.as_deref().unwrap_or("none"),
            );
            Some(Reading {
                playing,
                speaker,
                said,
            })
        }
    }

    /// The rule a session is acted on by ([`Wires::agree`]), with the hub's
    /// order perturbed as it can be in this process: the sessions read at
    /// one head, and the next job's frames reaching the kitchen's channel
    /// before the study's. Two speakers stand in max's session; a third, in
    /// ann's, moves the volatile log, which every speaker hears.
    ///
    /// Falsified by `agree` not comparing heads: the kitchen, a frame past
    /// the reading, was taken as agreeing; and by its not looking at what a
    /// speaker has unanswered: the kitchen's own call, not yet confirmed.
    #[cfg(test)]
    #[test]
    fn a_session_is_acted_on_only_when_its_speakers_have_heard_what_was_read() {
        use crate::domain::Scrub;
        const KITCHEN: &str = "media_player.kitchen";
        const STUDY: &str = "media_player.study";
        let hosted = Hosted::harken();
        let hub = Handle::spawn(ark_peer::Peer::serve(
            crate::hub_config(&hosted, 1_000),
            Box::new(ark_peer::Memory::default()),
            now_ms(),
        ));
        let mut w = Wires {
            hub,
            hosted,
            entities: vec![KITCHEN.into(), STUDY.into()],
            machines: HashMap::new(),
            standing: HashMap::new(),
            frames: channel(),
            waker: channel().0,
        };
        // Read and heard, as the thread does, until max's speakers agree.
        let settled = |w: &mut Wires| {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let (head, _) = sessions(&w.hub, &w.entities);
                w.receive();
                if w.agree("max", head) {
                    return head;
                }
                assert!(Instant::now() < deadline, "max's speakers never agreed");
                std::thread::sleep(Duration::from_millis(2));
            }
        };
        let scrub = |at_ms| Scrub { at_ms: Some(at_ms) }.to_bytes();
        w.stand("max", KITCHEN, "Kitchen");
        w.stand("max", STUDY, "Study");
        w.stand("ann", KITCHEN, "Kitchen");
        let first = settled(&mut w);

        // A call of the kitchen's own, unanswered.
        w.call("max", KITCHEN, "scrub", scrub(1));
        assert!(!w.agree("max", first), "the kitchen's call is unanswered");
        let read = settled(&mut w);
        assert!(read > first);

        // Read at `read`; then ann's kitchen moves the log, and its frames
        // reach max's kitchen and not yet max's study.
        assert_eq!(sessions(&w.hub, &w.entities).0, read);
        w.call("ann", KITCHEN, "scrub", scrub(2));
        w.hub.with(|_| ());
        let (study, rest): (Vec<Framed>, Vec<Framed>) = w
            .frames
            .1
            .try_iter()
            .partition(|((u, e), _)| u == "max" && e == STUDY);
        assert!(!study.is_empty(), "the study is sent ann's change");
        assert!(rest.iter().any(|((u, e), _)| u == "max" && e == KITCHEN));
        for f in rest {
            w.frames.0.send(f).unwrap();
        }
        w.receive();
        assert!(
            !w.agree("max", read),
            "the kitchen has heard past what was read"
        );
        let now = sessions(&w.hub, &w.entities).0;
        assert!(now > read);
        assert!(!w.agree("max", now), "the study has not heard it yet");
        for f in study {
            w.frames.0.send(f).unwrap();
        }
        w.receive();
        assert!(w.agree("max", now), "both have heard what was read");
    }
}
