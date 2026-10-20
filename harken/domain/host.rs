//! What a host knows of harken's domain beyond its bindings: the session
//! as plain Rust for a player ([`Listening`]), the keys a scanner derives
//! ([`work_key`]…) — each by calling the domain's own helper, so there is
//! no second copy of a rule — and the constants every host shares. A host
//! includes this beside its generated bindings (`super`), whose types it
//! reads: `harken-server`, `harken-iced`, and the domain's tests.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use ark::ir::Node;
use ark::store::Read;
use ark::Domain;

use super::*;

/// The library's role: the scanner's account holds it, and nobody else
/// unless the server's configuration grants it. Every library mutation is
/// guarded by it (`library.is_library`).
pub const LIBRARY: &str = "library";

/// The level a device that has said nothing of its volume is drawn at.
pub const UNSAID_VOLUME: i64 = 100;

/// How far the output's player may be from where the session counts it
/// before one is put right (`report_position`): 600 ms, the grain of the
/// coarsest output (a speaker read through Home Assistant).
pub const SYNC_MS: i64 = 600;

/// The module these bindings were made with, read from its file.
pub fn load() -> Domain {
    Domain::from_file(MODULE).unwrap_or_else(|r| panic!("harken's module: {r}"))
}

/// [`load`], once.
pub fn domain() -> &'static Domain {
    static D: OnceLock<Domain> = OnceLock::new();
    D.get_or_init(load)
}

fn key(answer: Result<String, String>) -> String {
    answer.unwrap_or_else(|e| unreachable!("a key helper never refuses: {e}"))
}

/// `library.work_key`: a work's key, from its composer, catalogue number
/// and title, trimmed as `add_song` trims them.
pub fn work_key(composer: &str, catalogue: &str, title: &str) -> String {
    key(library::work_key(
        domain(),
        composer.trim(),
        catalogue.trim(),
        title.trim(),
    ))
}

/// `library.movement_key`: a movement's key, from its work's and its number.
pub fn movement_key(work_id: &str, no: i64) -> String {
    key(library::movement_key(domain(), work_id, &no))
}

/// `library.recording_key`: a recording's key, from what it is of and who
/// played.
pub fn recording_key(of: &str, who: &str) -> String {
    key(library::recording_key(domain(), of, who.trim()))
}

/// `library.key_part`: one name as it stands in a key.
pub fn key_part(name: &str) -> String {
    key(library::key_part(domain(), name.trim()))
}

/// What sort of thing a device is, for the glyph a picker draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Computer,
    Phone,
    Speaker,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Computer => "computer",
            Kind::Phone => "phone",
            Kind::Speaker => "speaker",
        }
    }

    /// A kind this build has never heard of reads as a computer.
    pub fn parse(s: &str) -> Kind {
        match s {
            "phone" => Kind::Phone,
            "speaker" => Kind::Speaker,
            _ => Kind::Computer,
        }
    }
}

impl Clock {
    /// How far in at `now`, the server's clock.
    pub fn position(&self, now: i64) -> i64 {
        match *self {
            Clock::Paused { at_ms } => at_ms,
            Clock::Playing { at_ms, since } => at_ms + (now - since).max(0),
        }
    }

    pub fn playing(&self) -> bool {
        matches!(self, Clock::Playing { .. })
    }
}

impl Output {
    /// The device the sound belongs to, here or not.
    pub fn id(&self) -> Option<&str> {
        match self {
            Output::Device { id, .. } | Output::Away { id, .. } => Some(id),
            Output::Nowhere => None,
        }
    }
}

impl Active {
    /// The track playing.
    pub fn now(&self) -> &Track {
        &self.queue[(self.at.max(0) as usize).min(self.queue.len() - 1)]
    }

    /// Where the session stands at `now`: the domain's `standing`.
    pub fn position(&self, now: i64) -> i64 {
        match (&self.output, &self.clock) {
            (
                Output::Device { moving: true, .. },
                Clock::Playing { at_ms, .. } | Clock::Paused { at_ms },
            ) => *at_ms,
            (_, clock) => clock.position(now),
        }
    }
}

impl LibraryEntry {
    /// The rows of a query's answer, a list of entries.
    pub fn list(bytes: &[u8]) -> Result<Vec<LibraryEntry>, ark::Refusal> {
        Vec::<LibraryEntry>::from_bytes(bytes)
    }

    /// On the playlist it was read against.
    pub fn on_playlist(&self) -> bool {
        self.playlist_pos.is_some()
    }
}

/// One account's state, as a player reads it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Listening {
    pub devices: Vec<Present>,
    /// Where each device is dragging its scrubber, by device id.
    pub scrub: BTreeMap<String, i64>,
    pub session: Option<Active>,
}

impl Listening {
    /// `user`'s state as a store holds it.
    pub fn of(store: &dyn Read, user: &str) -> Listening {
        let session = ark::gen::rows::<super::Listening>(store, "listening")
            .into_iter()
            .find(|r| r.user == user)
            .and_then(|r| match r.session {
                Session::Active { active } => Some(active),
                Session::Idle => None,
            });
        let devices = ark::gen::rows::<ListeningDevice>(store, "listening_device")
            .into_iter()
            .filter(|r| r.user == user)
            .map(|r| r.present)
            .collect();
        let scrub = ark::gen::rows::<ListeningScrub>(store, "listening_scrub")
            .into_iter()
            .filter(|r| r.user == user)
            .map(|r| (r.device, r.at_ms))
            .collect();
        Listening {
            devices,
            scrub,
            session,
        }
    }

    pub fn device(&self, id: &str) -> Option<&Present> {
        self.devices.iter().find(|d| d.id == id)
    }

    /// The output, if anything is queued.
    pub fn output(&self) -> Option<&Output> {
        self.session.as_ref().map(|a| &a.output)
    }

    /// Whether `id` is the output and here.
    pub fn outputs(&self, id: &str) -> bool {
        matches!(self.output(), Some(Output::Device { id: o, .. }) if o == id)
    }

    /// Whether a hand-off to `id` is in flight.
    pub fn moving_to(&self, id: &str) -> bool {
        matches!(self.output(), Some(Output::Device { id: o, moving: true }) if o == id)
    }

    /// Whether something is making a sound.
    pub fn sounding(&self) -> bool {
        matches!(&self.session, Some(a) if a.clock.playing() && matches!(a.output, Output::Device { .. }))
    }

    /// The output, where it is a device that is here.
    pub fn leader(&self) -> Option<&str> {
        match self.output()? {
            Output::Device { id, .. } => Some(id),
            _ => None,
        }
    }

    /// Who follows `leader`, in id order.
    pub fn followers(&self, leader: &str) -> Vec<&Present> {
        let mut out: Vec<&Present> = self
            .devices
            .iter()
            .filter(|d| d.follows.as_deref() == Some(leader))
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    /// The group `leader` leads: it, then whoever follows it.
    pub fn group(&self, leader: &str) -> Vec<&Present> {
        self.device(leader)
            .into_iter()
            .chain(self.followers(leader))
            .collect()
    }

    /// Whether the output is a group.
    pub fn grouped(&self) -> bool {
        self.leader().is_some_and(|l| !self.followers(l).is_empty())
    }

    /// The group's level as the domain reads it; `None` when nothing in it
    /// has a volume.
    pub fn group_volume(&self, leader: &str) -> Option<i64> {
        let levels: Vec<i64> = self
            .group(leader)
            .iter()
            .filter(|d| d.can_volume)
            .map(|d| d.volume)
            .collect();
        (!levels.is_empty()).then(|| levels.iter().sum::<i64>() / levels.len() as i64)
    }

    /// Whether every member of `leader`'s group with a volume is muted.
    pub fn group_muted(&self, leader: &str) -> bool {
        let members: Vec<&Present> = self
            .group(leader)
            .into_iter()
            .filter(|d| d.can_volume)
            .collect();
        !members.is_empty() && members.iter().all(|d| d.muted)
    }

    /// Whether `id` can be joined to the output's group now.
    pub fn joinable(&self, id: &str) -> bool {
        let Some(leader) = self.leader().and_then(|l| self.device(l)) else {
            return false;
        };
        let Some(d) = self.device(id) else {
            return false;
        };
        leader.follows.is_none()
            && d.id != leader.id
            && d.can_play
            && d.family.is_some()
            && d.family == leader.family
    }
}

/// How long a track has to have played before it counts as a play: thirty
/// seconds or half its length, whichever is first.
pub fn counts_after(duration_ms: i64) -> i64 {
    if duration_ms <= 0 {
        return 30_000;
    }
    (duration_ms / 2).min(30_000)
}

/// Where a track's file is, from where a server is. An absolute URL is
/// already an answer.
pub fn url(base: &str, file: &str) -> String {
    if file.is_empty() || file.starts_with("http://") || file.starts_with("https://") {
        return file.to_string();
    }
    let mut out = format!("{}/media", base.trim_end_matches('/'));
    for part in file.split('/') {
        out.push('/');
        for b in part.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(b as char)
                }
                _ => out.push_str(&format!("%{b:02X}")),
            }
        }
    }
    out
}
