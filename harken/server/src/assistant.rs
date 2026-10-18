//! Home Assistant's media players, as devices in the listening session.
//!
//! A Sonos is a *device*, not a client. The session (`harken_domain::
//! listening`, a live state) already describes one — it stands in an
//! account's session, is handed the sound, and says what happens to it —
//! and nothing in that says the far end has to be somebody's screen. So
//! nothing here extends the session; what is here is the thing that is a
//! device on a speaker's behalf: a participant (`ark_server::HubHandle::
//! participate`) standing in each account's instance for each configured
//! speaker, reconciling the speaker against the state exactly as a phone
//! reconciles its player, and reporting what the speaker does as the
//! output's own transitions.
//!
//! **Home Assistant rather than Sonos directly**, and not for convenience: you
//! do not get Sonos, you get every `media_player` entity in the house. A
//! Chromecast, a television, an AirPlay receiver and a speaker group are all
//! the same few service calls, where talking UPnP would buy one make of
//! speaker and a discovery problem.
//!
//! [`Bridge`] owns no socket and no thread: what is worth asserting is the
//! rules — which account holds a speaker, what a change of the session
//! becomes, how what the speaker says is read back — and none of that needs
//! a network to be wrong in. It is handed the session and what the house
//! said, and hands back [`Act`]s: service calls, and transitions to call as
//! the speaker.
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

use std::collections::{HashMap, HashSet};

use ark::eval::Args;
use harken_domain::listening::read::{input, Active, Clock, Listening, Output};
use harken_domain::listening::url;

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

/// How far a playing speaker may be from where the session counts it before
/// one is told the other: the speaker is right about where it is (`seek`
/// said as it), and the session is right about where somebody asked it to
/// be (`media_seek` told to it).
pub const DRIFT_MS: i64 = 3_000;

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
    /// What it can do, as its `equip` says it.
    fn caps(&self) -> (bool, Option<String>) {
        (
            self.can_volume && self.volume.is_some(),
            self.family.clone(),
        )
    }

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
}

impl Playing {
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
    /// queue, turns it into one `play_media` with `enqueue: play` and the
    /// rest as `enqueue: add` — which is what leaves the speaker's own next
    /// and previous working — seeks after the enqueue, and pauses when the
    /// session is paused.
    Start {
        entity: String,
        urls: Vec<String>,
        at: usize,
        position_ms: i64,
        playing: bool,
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

/// What one speaker was handed, by whose session, and what the bridge
/// believes of it.
#[derive(Debug, Clone)]
struct Holding {
    key: String,
    /// The queue as the speaker was given it, so what it reports back can be
    /// matched to an index without re-deriving anything.
    urls: Vec<String>,
    /// …each as an [`address`], which is what a report is matched against.
    addresses: Vec<String>,
    /// Where in that list it was handed from: until it says it is playing
    /// that, it is fetching, and nothing else it says is news.
    handed_at: usize,
    /// Whether it has said it is playing what it was handed.
    started: bool,
    /// Where in the queue the bridge believes it is.
    at: usize,
    /// What it was last told: playing or not.
    playing: bool,
    /// The session's clock as the bridge last reconciled against it.
    clock: Clock,
    /// How many more polls it may answer with something that is not ours,
    /// or not answer at all, before it is let go: [`SETTLE`] on a hand-off,
    /// [`LAPSE`] again after every poll that names one of ours.
    settling: u8,
    /// [`QUIET`] after the bridge told it something.
    quiet: u8,
    /// Where it last said it was, and when.
    heard: Option<(i64, i64)>,
    /// Where in the track it was handed from: what `started` is said at once
    /// it is there, and what it is told to seek to until it is.
    from_ms: i64,
    /// Seeks to `from_ms` said since the hand-off ([`SEEKS`] at most), and
    /// polls to wait before the next.
    sought: u8,
    seek_wait: u8,
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
}

/// What the house last said of a speaker's volume and group, and how many
/// more polls a difference is the house catching up with what it was told.
#[derive(Debug, Clone, Default)]
struct Tuning {
    reading: Option<Speaker>,
    quiet: u8,
}

/// What the bridge last saw of one speaker in one session: what it said the
/// speaker can do, and the volume and followers the session had the last
/// time the two were reconciled — so the bridge can tell the session moving
/// from the house moving.
#[derive(Debug, Clone, Default)]
struct Seen {
    equipped: Option<(bool, Option<String>)>,
    volume: Option<(i64, bool)>,
    followers: Option<Vec<String>>,
}

/// Home Assistant, as a shelf of devices.
pub struct Bridge {
    /// The entities offered, and what to call each.
    players: Vec<(String, String)>,
    /// Where a speaker fetches bytes from, which is not where a phone does:
    /// the phone may be on `https://harken.example.com` while the speaker
    /// only knows an address on the LAN.
    media: String,
    held: HashMap<String, Holding>,
    /// Sessions a speaker has already let go of while still named their
    /// output — somebody else took it — so the release is said once.
    released: HashSet<(String, String)>,
    /// Each speaker's volume and group, as the house last said them.
    tuning: HashMap<String, Tuning>,
    /// …and as each session last had them.
    seen: HashMap<(String, String), Seen>,
}

impl Bridge {
    /// `players` are the entities to offer, with their names. `media` is the
    /// base a speaker resolves a `file` against.
    pub fn new(players: Vec<(String, String)>, media: &str) -> Bridge {
        Bridge {
            players,
            media: media.trim_end_matches('/').to_string(),
            held: HashMap::new(),
            released: HashSet::new(),
            tuning: HashMap::new(),
            seen: HashMap::new(),
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
        self.held.get(entity).map(|h| h.key.as_str())
    }

    /// Whether anybody has handed this speaker something. Only those are
    /// worth asking Home Assistant about.
    pub fn holds(&self, entity: &str) -> bool {
        self.held.contains_key(entity)
    }

    fn urls(&self, a: &Active) -> Vec<String> {
        a.queue.iter().map(|t| url(&self.media, &t.file)).collect()
    }

    fn hand(&mut self, key: &str, entity: &str, a: &Active, now: i64) -> Act {
        let urls = self.urls(a);
        // Where the session stands — still, while the sound is on its way.
        let position_ms = a.position(now);
        self.held.insert(
            entity.to_string(),
            Holding {
                key: key.to_string(),
                addresses: urls.iter().map(|u| address(u)).collect(),
                urls: urls.clone(),
                handed_at: a.at,
                started: false,
                at: a.at,
                playing: a.clock.playing(),
                clock: a.clock,
                settling: SETTLE,
                quiet: QUIET,
                heard: None,
                from_ms: position_ms,
                sought: 0,
                seek_wait: 0,
            },
        );
        Act::Start {
            entity: entity.to_string(),
            urls,
            at: a.at,
            position_ms,
            playing: a.clock.playing(),
        }
    }

    /// `key`'s session, as it now is, read for `entity`, at `now` (the
    /// server's clock): what the speaker has to be told to agree with it.
    pub fn wants(&mut self, key: &str, entity: &str, state: &Listening, now: i64) -> Vec<Act> {
        if !self.has(entity) {
            return vec![];
        }
        let mut acts = vec![];
        let ours = match &state.session {
            Some(a) => match &a.output {
                Output::Device { id, moving } if id == entity => Some((a, *moving)),
                _ => None,
            },
            None => None,
        };
        let Some((a, moving)) = ours else {
            // Not this speaker's to play in this session. If it was, the
            // session has moved its sound somewhere else — a phone took it
            // back, somebody stopped it — and a speaker has nobody to go
            // quiet for it.
            self.released.remove(&(key.to_string(), entity.to_string()));
            // …unless it follows the device the sound went to: the sound was
            // handed to a member of its group, which leads it now, and a
            // pause said to a member is a pause of its whole group.
            let follows_it = state
                .device(entity)
                .and_then(|d| d.follows.as_deref())
                .is_some_and(|l| state.leader() == Some(l));
            if self.holder(entity) == Some(key) && follows_it {
                self.held.remove(entity);
            } else if self.holder(entity) == Some(key) {
                self.held.remove(entity);
                acts.push(Act::Verb {
                    entity: entity.into(),
                    verb: "media_pause",
                    why: "the session moved its sound elsewhere",
                });
            }
            return acts;
        };
        match self.held.get(entity).map(|h| h.key.clone()) {
            Some(other) if other != key => {
                if moving {
                    // Somebody else had it, and this session has just been
                    // handed it. A speaker is one piece of hardware and a
                    // session is one account's, so the second one gets it —
                    // which is what a real speaker does — and the first is
                    // told, by the speaker, rather than left drawing a
                    // transport for somebody else's music.
                    acts.push(Act::Call {
                        key: other.clone(),
                        entity: entity.into(),
                        transition: "release",
                        input: input::none(),
                        why: format!("taken by {key}'s session"),
                    });
                    self.released.remove(&(key.to_string(), entity.to_string()));
                    self.released.insert((other, entity.to_string()));
                    acts.push(self.hand(key, entity, a, now));
                } else if self.released.insert((key.to_string(), entity.to_string())) {
                    // This session still names it, settled, but the speaker
                    // went to somebody else since: it lets this one go.
                    acts.push(Act::Call {
                        key: key.into(),
                        entity: entity.into(),
                        transition: "release",
                        input: input::none(),
                        why: format!("held by {other}'s session"),
                    });
                }
            }
            None => acts.push(self.hand(key, entity, a, now)),
            Some(_) => acts.extend(self.reconcile(entity, a, now)),
        }
        acts
    }

    // The speaker is this session's: what changed in the session since it
    // was last reconciled, told to the speaker.
    fn reconcile(&mut self, entity: &str, a: &Active, now: i64) -> Vec<Act> {
        let urls = self.urls(a);
        let key = self.held[entity].key.clone();
        let h = &self.held[entity];
        // Another queue, or another place in it, than the speaker was given
        // or has said it reached: the session moved, so the speaker is handed
        // it again from there.
        if urls != h.urls || a.at != h.at {
            return vec![self.hand(&key, entity, a, now)];
        }
        let mut acts = vec![];
        let h = self.held.get_mut(entity).expect("held");
        if a.clock != h.clock {
            // Where the speaker is now, counted on from where it last said it
            // was, as it was then: playing or not.
            let want = a.position(now);
            let speaker = match h.heard {
                Some((at_ms, when)) if h.playing => at_ms + (now - when).max(0),
                Some((at_ms, _)) => at_ms,
                None => want,
            };
            h.heard = Some((speaker, now));
            if a.clock.playing() != h.playing {
                h.playing = a.clock.playing();
                acts.push(Act::Verb {
                    entity: entity.into(),
                    verb: if h.playing {
                        "media_play"
                    } else {
                        "media_pause"
                    },
                    why: if h.playing {
                        "the session plays"
                    } else {
                        "the session paused"
                    },
                });
            }
            // A seek in the session is a seek on the speaker; a clock
            // re-anchored at where the speaker said it was (`started`, the
            // bridge's own `seek`) is not one.
            if (speaker - want).abs() > DRIFT_MS {
                acts.push(Act::Seek {
                    entity: entity.into(),
                    position_ms: want,
                });
                h.heard = Some((want, now));
            }
            h.clock = a.clock;
            if !acts.is_empty() {
                h.quiet = QUIET;
            }
        }
        acts
    }

    /// Home Assistant said what a held player is doing — or `None`, it did
    /// not answer — at `now`. What the speaker did that the session does not
    /// know, said as the speaker; or, after its grace, the speaker let go.
    ///
    /// Nothing for a player nobody in a session handed anything to: a
    /// speaker somebody is using from the Sonos app is not this server's
    /// business, and reporting it would put a stranger's music in
    /// somebody's bar.
    pub fn heard(&mut self, entity: &str, poll: Option<&Playing>, now: i64) -> Vec<Act> {
        let Some(h) = self.held.get_mut(entity) else {
            return vec![];
        };
        // Where it says it is. A URL that is not one of ours means either that
        // it has not started yet or that it has been sent somewhere else
        // entirely — a radio stream, a doorbell chime — and the only thing
        // that tells those apart is how long ago we handed it something.
        let at = poll.and_then(|p| h.index(p));
        let (Some(at), Some(p)) = (at, poll) else {
            return self.lapse(entity, poll);
        };
        let key = h.key.clone();
        let call = |transition: &'static str, input: Args, why: &str| Act::Call {
            key: key.clone(),
            entity: entity.to_string(),
            transition,
            input,
            why: why.to_string(),
        };
        // Where it is now, counted on from when the house last said so.
        // Undated, it is taken to be where the session counts it — so it is
        // never the reason for a seek.
        let dated = p.position_at(now);
        let playing = p.state == "playing";
        let mut acts = vec![];
        if !h.started {
            // Still fetching: what it says is whatever it played before, even
            // a track of this same queue, until it says the one it was handed.
            if at != h.handed_at {
                return self.lapse(entity, poll);
            }
            let position = dated.unwrap_or(h.from_ms);
            // Playing the track it was handed, but from its beginning when it
            // was handed from half way: the seek after the enqueue reached it
            // before it had loaded and was dropped. It is told again, once it
            // has the track, and it has not started until it is there —
            // `started` at 0 would take every device's clock back to the
            // beginning with it. The session stands where it was meanwhile.
            if h.from_ms > DRIFT_MS && position < h.from_ms - DRIFT_MS && h.sought < SEEKS {
                if h.seek_wait > 0 {
                    h.seek_wait -= 1;
                    return acts;
                }
                h.sought += 1;
                h.seek_wait = QUIET;
                h.settling = SETTLE;
                acts.push(Act::Seek {
                    entity: entity.into(),
                    position_ms: h.from_ms,
                });
                return acts;
            }
            h.started = true;
            h.settling = LAPSE;
            h.quiet = QUIET;
            h.heard = Some((position, now));
            acts.push(call("started", input::at(position), "it has the hand-off"));
            return acts;
        }
        let position = dated.unwrap_or_else(|| h.clock.position(now));
        h.settling = LAPSE;
        h.heard = Some((position, now));
        if at != h.at {
            // It moved on its own: the end of a track, or somebody's finger
            // on the speaker or in the Sonos app.
            let was = h.at;
            h.at = at;
            h.quiet = QUIET;
            acts.push(if at == was + 1 {
                call("advance", input::advance(was), "the track ended")
            } else {
                call("jump", input::jump(at), "it was moved by hand")
            });
            return acts;
        }
        if h.quiet > 0 {
            h.quiet -= 1;
            return acts;
        }
        if playing != h.playing {
            h.playing = playing;
            h.quiet = QUIET;
            acts.push(match playing {
                true => call("play", input::none(), "it was resumed by hand"),
                false => call(
                    "stalled",
                    input::at(position),
                    "it stopped by itself, or by hand",
                ),
            });
        } else if playing && dated.is_some() && (position - h.clock.position(now)).abs() > DRIFT_MS
        {
            h.quiet = QUIET;
            acts.push(call(
                "seek",
                input::seek(position),
                "it is somewhere else in the track",
            ));
        }
        acts
    }

    /// One poll that was not the speaker playing ours: spent from the grace it
    /// has, and the last one lets it go — said as the speaker, and out loud.
    fn lapse(&mut self, entity: &str, poll: Option<&Playing>) -> Vec<Act> {
        let Some(h) = self.held.get_mut(entity) else {
            return vec![];
        };
        if h.settling > 0 {
            h.settling -= 1;
            return vec![];
        }
        let key = h.key.clone();
        // What it was expected to say, beside what it said: a URL that is
        // ours spelt some other way reads, in the journal, as two strings
        // that differ only in their escapes.
        let expected = h
            .urls
            .get(if h.started { h.at } else { h.handed_at })
            .cloned()
            .unwrap_or_default();
        let next = h.urls.get(h.at + 1).filter(|_| h.started).cloned();
        self.held.remove(entity);
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
            None => "not answering".to_string(),
        };
        vec![Act::Call {
            key,
            entity: entity.into(),
            transition: "release",
            input: input::none(),
            why,
        }]
    }

    /// Home Assistant said what a standing player's volume and group are —
    /// or `None`, it did not answer — which spends a poll of its quiet.
    pub fn polled(&mut self, entity: &str, reading: Option<&Speaker>) {
        let t = self.tuning.entry(entity.to_string()).or_default();
        t.reading = reading.cloned();
        t.quiet = t.quiet.saturating_sub(1);
    }

    /// `key`'s session as it now is, read for `entity`'s volume and group:
    /// what the speaker says it can do, if the session does not know it
    /// yet; what moved in the session since the last look, told to the
    /// house; and what moved in the house while the session stood still,
    /// said as the speaker — after its quiet, since a house a moment behind
    /// what it was just told is not somebody's hand.
    ///
    /// The group is reconciled from its leader's side alone: who follows
    /// this speaker in the session against who plays with it in the house.
    /// A member's own reading says only that it is a member, which its
    /// leader's already says.
    pub fn tune(&mut self, key: &str, entity: &str, state: &Listening) -> Vec<Act> {
        if !self.has(entity) {
            return vec![];
        }
        let Some(reading) = self.tuning.get(entity).and_then(|t| t.reading.clone()) else {
            return vec![];
        };
        let Some(me) = state.device(entity) else {
            return vec![];
        };
        let quiet = self.tuning[entity].quiet;
        let seen = self
            .seen
            .entry((key.to_string(), entity.to_string()))
            .or_default();
        let call = |transition: &'static str, input: Args, why: &str| Act::Call {
            key: key.to_string(),
            entity: entity.to_string(),
            transition,
            input,
            why: why.to_string(),
        };
        let mut acts = vec![];
        // What it can do, once a session has it standing: until the session
        // says so, nothing else here can be read off it.
        let caps = reading.caps();
        if (me.can_volume, me.family.clone()) != caps {
            if seen.equipped.as_ref() != Some(&caps) {
                let volume = reading
                    .volume
                    .unwrap_or(harken_domain::listening::UNSAID_VOLUME);
                seen.equipped = Some(caps.clone());
                seen.volume = Some((volume, reading.muted));
                acts.push(call(
                    "equip",
                    input::equip(caps.0, caps.1.as_deref(), volume, reading.muted),
                    "what the house says it can do",
                ));
            }
            return acts;
        }
        seen.equipped = Some(caps);
        let mut told = false;
        if let (true, Some(level)) = (me.can_volume, reading.volume) {
            let session = (me.volume, me.muted);
            let house = (level, reading.muted);
            if session == house {
                seen.volume = Some(session);
            } else if seen.volume.is_some_and(|v| v != session) {
                // The session moved: the house is told.
                if me.volume != level {
                    acts.push(Act::Volume {
                        entity: entity.into(),
                        level: me.volume,
                    });
                }
                if me.muted != reading.muted {
                    acts.push(Act::Mute {
                        entity: entity.into(),
                        muted: me.muted,
                    });
                }
                seen.volume = Some(session);
                told = true;
            } else if quiet == 0 || seen.volume.is_none() {
                // The house moved, or this is the first look: the knob on the
                // speaker, the Sonos app.
                acts.push(call(
                    "report_volume",
                    input::report_volume(level, reading.muted),
                    "its volume was changed by hand",
                ));
                seen.volume = Some(house);
            }
        }
        if me.family.is_some() {
            let mut session: Vec<String> = state
                .followers(entity)
                .iter()
                .map(|d| d.id.clone())
                .collect();
            let mut house: Vec<String> = match reading.leads(entity) {
                true => reading
                    .members
                    .iter()
                    .filter(|m| *m != entity && state.device(m).is_some())
                    .cloned()
                    .collect(),
                false => vec![],
            };
            session.sort();
            house.sort();
            if session == house {
                seen.followers = Some(session);
            } else if seen.followers.as_ref().is_some_and(|f| *f != session) {
                let join: Vec<String> = session
                    .iter()
                    .filter(|m| !house.contains(m))
                    .cloned()
                    .collect();
                if !join.is_empty() {
                    acts.push(Act::Join {
                        leader: entity.into(),
                        members: join,
                    });
                }
                // Let go of only what the session has in no group at all: a
                // member another leader has taken is that leader's to join,
                // and an unjoin would undo it.
                for m in house.iter().filter(|m| !session.contains(m)) {
                    let alone = state
                        .device(m)
                        .is_some_and(|d| d.follows.is_none() && state.followers(m).is_empty());
                    if alone {
                        acts.push(Act::Unjoin { entity: m.clone() });
                    }
                }
                seen.followers = Some(session);
                told = true;
            } else if reading.leads(entity) && (quiet == 0 || seen.followers.is_none()) {
                let members: Vec<&str> = house.iter().map(String::as_str).collect();
                acts.push(call(
                    "report_group",
                    input::report_group(&members),
                    "it was grouped by hand",
                ));
                seen.followers = Some(house);
            }
        }
        if told {
            if let Some(t) = self.tuning.get_mut(entity) {
                t.quiet = QUIET;
            }
        }
        acts
    }

    /// The speaker stops standing in `key`'s session — nobody is listening
    /// there and it is not making the sound, or has made none for
    /// [`ha::IDLE`] — and anything it was playing for it stops, because
    /// nothing else would ever tell it to.
    pub fn left(&mut self, key: &str, entity: &str) -> Vec<Act> {
        self.seen.remove(&(key.to_string(), entity.to_string()));
        self.released.remove(&(key.to_string(), entity.to_string()));
        if self.holder(entity) != Some(key) {
            return vec![];
        }
        self.held.remove(entity);
        vec![Act::Verb {
            entity: entity.into(),
            verb: "media_pause",
            why: "nobody is listening there any more",
        }]
    }
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

    /// A moment the house stamped, on the server's clock: as long before
    /// `server_now` as it was before the house's own now, which its answer's
    /// `Date` says — to the second, so taken at the middle of it. With no
    /// `Date` the two clocks are taken to agree, which on one LAN with NTP
    /// they do to well inside the three seconds anything here is measured
    /// against.
    pub fn on_our_clock(stamped: i64, house_now: Option<i64>, server_now: i64) -> i64 {
        match house_now {
            Some(date) => server_now - (date + 500 - stamped),
            None => stamped,
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
            // is ten seconds before ours.
            let ours = 1_714_566_896_000;
            let house = ours + 60_000;
            assert_eq!(
                on_our_clock(house - 10_000, Some(house), ours),
                ours - 10_500
            );
            assert_eq!(on_our_clock(42, None, ours), 42);
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
        .any(|d| d.kind != harken_domain::listening::read::Kind::Speaker)
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

#[cfg(test)]
mod tests {
    use super::*;
    use ark::value::Id;
    use harken_domain::listening::read::{Device, Kind, Track};

    const KITCHEN: &str = "media_player.kitchen";
    const NOW: i64 = 1_700_000_000_000;

    fn track(file: &str, n: u8) -> Track {
        let mut id: Id = [0; 16];
        id[15] = n;
        Track {
            id,
            title: file.into(),
            creator: "Bach".into(),
            album: String::new(),
            duration_ms: 60_000,
            file: file.into(),
        }
    }

    fn bridge() -> Bridge {
        Bridge::new(
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

    /// A session whose output is `output`, on `files` from `at`.
    fn session(files: &[&str], at: usize, clock: Clock, output: Output) -> Listening {
        Listening {
            devices: vec![Device::new("phone", "Phone", Kind::Phone, true)],
            scrub: Default::default(),
            session: Some(Active {
                queue: files
                    .iter()
                    .enumerate()
                    .map(|(i, f)| track(f, i as u8))
                    .collect(),
                at,
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

    /// A speaker handed the session, `files` from `at`, that has started.
    fn started(b: &mut Bridge, key: &str, files: &[&str], at: usize) {
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
    /// Falsified by handing the clock counted on to now: the speaker was
    /// told 43 500 where 42 000 was right.
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
    /// Falsified by saying `started` at whatever the speaker first reports:
    /// the session was told the track had started at 0.
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
        let Act::Call { input, .. } = &acts[0] else {
            unreachable!()
        };
        assert_eq!(input, &input::at(90_200), "started where the track was");

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
        assert!(b
            .wants(
                "alice",
                "media_player.bedroom",
                &session(&["music/a.mp3"], 0, playing_at(0), bedroom),
                NOW
            )
            .is_empty());
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
    /// Falsified by taking any track of the queue as having started: the
    /// speaker still on the old track 0 reported `started`.
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
        let Act::Call { input, .. } = &acts[0] else {
            unreachable!()
        };
        assert_eq!(input, &input::at(1_200));
    }

    /// Where the speaker says it is, is where it is — including when nobody
    /// asked it to move. Somebody pressing the button on the speaker moves
    /// every phone: the next track is `advance`, anywhere else `jump`, a
    /// pause by hand `stalled` and a resume `play`.
    ///
    /// Falsified by reporting every move as `jump`: the end of a track came
    /// out a jump, which an output's own `advance` is not.
    #[test]
    fn what_the_speaker_says_it_is_playing_is_where_it_is_in_the_queue() {
        let mut b = bridge();
        let files = ["music/a.mp3", "music/b.mp3", "music/c.mp3"];
        started(&mut b, "alice", &files, 0);
        let acts = b.heard(KITCHEN, Some(&playing(&url_of("music/b.mp3"), 100)), NOW);
        assert_eq!(calls(&acts), [("alice", "advance")]);
        let acts = b.heard(KITCHEN, Some(&playing(&url_of("music/a.mp3"), 100)), NOW);
        assert_eq!(calls(&acts), [("alice", "jump")]);
        // Quiet for a moment after a move, then a pause by hand.
        let paused = Playing {
            state: "paused".into(),
            ..playing(&url_of("music/a.mp3"), 4_200)
        };
        assert!(b.heard(KITCHEN, Some(&paused), NOW).is_empty());
        assert!(b.heard(KITCHEN, Some(&paused), NOW).is_empty());
        let acts = b.heard(KITCHEN, Some(&paused), NOW);
        assert_eq!(calls(&acts), [("alice", "stalled")]);
    }

    /// A session that moved is told to the speaker: a pause becomes
    /// `media_pause`, a play `media_play`, a seek `media_seek`, and another
    /// place in the queue a fresh hand-off from there. A clock re-anchored
    /// where the speaker already is — `started` — moves nothing.
    ///
    /// Falsified by comparing the session's position with its own previous
    /// clock rather than with where the speaker said it was: a slow
    /// speaker's `started`, re-anchoring the session five seconds back, was
    /// sent back to it as a seek.
    #[test]
    fn a_session_that_moved_is_told_to_the_speaker() {
        let mut b = bridge();
        let files = ["music/a.mp3", "music/b.mp3"];
        // A slow speaker: five seconds fetching, and it starts at 0.1 s.
        // `started` re-anchors the session there, which is where the
        // speaker is, and so is nothing to tell it.
        b.wants(
            "alice",
            KITCHEN,
            &session(&files, 0, playing_at(0), kitchen(true)),
            NOW,
        );
        let late = Playing {
            updated_ms: Some(NOW + 5_000),
            ..playing(&url_of(files[0]), 100)
        };
        let acts = b.heard(KITCHEN, Some(&late), NOW + 5_000);
        assert_eq!(calls(&acts), [("alice", "started")]);
        let slow = session(
            &files,
            0,
            Clock::Playing {
                at_ms: 100,
                since: NOW + 5_000,
            },
            kitchen(false),
        );
        assert!(
            b.wants("alice", KITCHEN, &slow, NOW + 5_000).is_empty(),
            "the re-anchoring is not a seek"
        );

        let mut b = bridge();
        started(&mut b, "alice", &files, 0);
        // `started` landed: the clock is re-anchored where the speaker is.
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

    /// A speaker sent somewhere else entirely is no longer in this session,
    /// and saying otherwise would be a lie with a scrubber on it: once its
    /// grace is spent it lets the session go, as itself — and stays let go
    /// rather than being recovered by the next report that happens to match.
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
    }

    /// A speaker handed a queue does not play it that instant: it fetches the
    /// first URL, and for every tick of that it is still reporting whatever it
    /// was playing before. Read as "not ours" that is a hand-off which releases
    /// itself half a second after it was asked for — so the tell is a speaker
    /// somebody picked handing the music straight back.
    ///
    /// The counts here are numbers rather than `SETTLE`: written in terms of
    /// the constant, setting it to zero makes the first loop empty and the
    /// "not yet" assertion never runs, so the test would shrink with the thing
    /// it holds.
    ///
    /// Falsified by setting `SETTLE` to 1: the second quiet poll released it.
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
        };
        for tick in 0..3 {
            assert!(
                b.heard(KITCHEN, Some(&quiet), NOW).is_empty(),
                "still fetching, {tick} ticks in"
            );
        }
        // …and then it starts, which is the end of the grace whether or not it
        // was spent: a speaker that takes two seconds must not be let go on
        // its sixth.
        assert_eq!(
            calls(&b.heard(KITCHEN, Some(&playing(&url_of("music/a.mp3"), 0)), NOW)),
            [("alice", "started")]
        );
        // Having started, silence is a lapse rather than fetching: three
        // polls of it — a track boundary, a blink of the integration — and
        // the fourth is the one that lets it go. Literal counts, for the
        // reason above.
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
        // …and a poll that matched in between starts the three again, which
        // is what keeps a speaker on a long album.
        started(&mut b, "alice", &["music/a.mp3"], 0);
        for _ in 0..4 {
            b.heard(KITCHEN, Some(&playing(&url_of("music/a.mp3"), 0)), NOW);
            b.heard(KITCHEN, Some(&quiet), NOW);
            b.heard(KITCHEN, Some(&quiet), NOW);
        }
        assert_eq!(b.holder(KITCHEN), Some("alice"));
    }

    /// A speaker that has stopped answering is let go too, and whoever had it
    /// is told: a bar counting up for a speaker that is not there is worse
    /// than one that stops. Not on the first poll it misses, though — a proxy
    /// refusing one request, or an integration reloading, is a second of
    /// silence and not a speaker that is gone.
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
        let acts = b.heard(KITCHEN, None, NOW);
        assert_eq!(calls(&acts), [("alice", "release")]);
        assert_eq!(b.holder(KITCHEN), None);
    }

    /// One address, however the speaker spells it back: decoded where it was
    /// escaped, escaped in lower case, an ampersand escaped for XML, the
    /// scheme and host in capitals, a default port written out — each the
    /// same file, and each a different string.
    ///
    /// Falsified by an `address` that returned its argument: every
    /// re-spelling was a different address.
    #[test]
    fn an_address_is_the_same_however_it_is_spelt() {
        let ours = harken_domain::listening::url(
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
        assert_eq!(
            calls(&b.heard(KITCHEN, Some(&playing(&spelt(files[1]), 0)), NOW)),
            [("alice", "advance")]
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

    // Volume and groups ------------------------------------------------------

    const STUDY: &str = "media_player.study";

    /// A speaker as the session has it once it has said what it can do.
    fn stood(id: &str, volume: i64, follows: Option<&str>) -> Device {
        Device {
            can_volume: true,
            volume,
            family: Some("sonos".into()),
            follows: follows.map(str::to_string),
            ..Device::new(id, id, Kind::Speaker, true)
        }
    }

    /// A session with the phone and these speakers in it, playing on
    /// `output`.
    fn house_of(speakers: Vec<Device>, output: &str) -> Listening {
        let mut s = session(
            &["music/a.mp3"],
            0,
            playing_at(0),
            Output::Device {
                id: output.into(),
                moving: false,
            },
        );
        s.devices.extend(speakers);
        s
    }

    /// What the house says of a Sonos.
    fn sonos(volume: i64, muted: bool, members: &[&str]) -> Speaker {
        Speaker {
            volume: Some(volume),
            muted,
            can_volume: true,
            family: Some("sonos".into()),
            members: members.iter().map(|m| m.to_string()).collect(),
        }
    }

    fn acts_but_calls(acts: &[Act]) -> Vec<&Act> {
        acts.iter()
            .filter(|a| !matches!(a, Act::Call { .. }))
            .collect()
    }

    /// What a speaker can do is read off the house and said once, as the
    /// speaker's `equip`, carrying the volume the house reports — and nothing
    /// else is said or told of it until the session has it.
    ///
    /// Falsified by forgetting what was equipped: the equip was said again
    /// on every read until it landed.
    #[test]
    fn what_a_speaker_can_do_is_said_once_as_its_equip() {
        let mut b = bridge();
        let fresh = house_of(
            vec![Device::new(KITCHEN, "Kitchen", Kind::Speaker, true)],
            "phone",
        );
        assert!(
            b.tune("alice", KITCHEN, &fresh).is_empty(),
            "nothing read yet, nothing said"
        );
        b.polled(KITCHEN, Some(&sonos(35, false, &[])));
        let acts = b.tune("alice", KITCHEN, &fresh);
        assert_eq!(calls(&acts), [("alice", "equip")]);
        let Act::Call { input, .. } = &acts[0] else {
            unreachable!()
        };
        assert_eq!(input, &input::equip(true, Some("sonos"), 35, false));
        assert!(
            b.tune("alice", KITCHEN, &fresh).is_empty(),
            "said once, while it lands"
        );
        let landed = house_of(vec![stood(KITCHEN, 35, None)], "phone");
        assert!(
            b.tune("alice", KITCHEN, &landed).is_empty(),
            "and then the two agree"
        );
    }

    /// A volume that moved in the session is told to the house; one that
    /// moved in the house while the session stood still was somebody's
    /// hand, said as the speaker — but not while the house is still
    /// catching up with what it was just told.
    ///
    /// Falsified by reporting a difference whatever the quiet: the house's
    /// stale 35, read the poll after `volume_set 60`, went back to the
    /// session as a hand.
    #[test]
    fn a_volume_is_told_and_a_hand_on_the_knob_is_reported_after_the_quiet() {
        let mut b = bridge();
        b.polled(KITCHEN, Some(&sonos(35, false, &[])));
        b.tune(
            "alice",
            KITCHEN,
            &house_of(vec![stood(KITCHEN, 35, None)], "phone"),
        );
        let moved = house_of(vec![stood(KITCHEN, 60, None)], "phone");
        assert_eq!(
            acts_but_calls(&b.tune("alice", KITCHEN, &moved)),
            [&Act::Volume {
                entity: KITCHEN.into(),
                level: 60
            }]
        );
        // The house a poll behind: quiet.
        for _ in 0..QUIET - 1 {
            b.polled(KITCHEN, Some(&sonos(35, false, &[])));
            assert!(
                b.tune("alice", KITCHEN, &moved).is_empty(),
                "the house catching up"
            );
        }
        // It caught up, and then somebody turned the knob and muted it.
        b.polled(KITCHEN, Some(&sonos(60, false, &[])));
        assert!(b.tune("alice", KITCHEN, &moved).is_empty());
        b.polled(KITCHEN, Some(&sonos(22, true, &[])));
        let acts = b.tune("alice", KITCHEN, &moved);
        assert_eq!(calls(&acts), [("alice", "report_volume")]);
        let Act::Call { input, .. } = &acts[0] else {
            unreachable!()
        };
        assert_eq!(input, &input::report_volume(22, true));
        // Muting in the session is a mute in the house, and nothing else.
        let muted = house_of(
            vec![Device {
                muted: true,
                ..stood(KITCHEN, 22, None)
            }],
            "phone",
        );
        let mut b2 = bridge();
        b2.polled(KITCHEN, Some(&sonos(22, false, &[])));
        b2.tune(
            "alice",
            KITCHEN,
            &house_of(vec![stood(KITCHEN, 22, None)], "phone"),
        );
        assert_eq!(
            acts_but_calls(&b2.tune("alice", KITCHEN, &muted)),
            [&Act::Mute {
                entity: KITCHEN.into(),
                muted: true
            }]
        );
    }

    /// The group is reconciled from its leader's side: a follower added in
    /// the session is a `join` on the leader, one dropped an `unjoin` on it;
    /// a group made in the Sonos app is reported as the leader's; and the
    /// first look at a speaker takes the group the house already has rather
    /// than breaking it up.
    ///
    /// Falsified by treating the first look as the session having moved:
    /// standing the kitchen in a session unjoined the study the Sonos app
    /// had grouped with it.
    #[test]
    fn a_group_is_told_to_the_house_and_a_group_made_by_hand_is_reported() {
        let mut b = bridge();
        // The Sonos app already has the study playing with the kitchen.
        b.polled(KITCHEN, Some(&sonos(30, false, &[KITCHEN, STUDY])));
        let apart = house_of(
            vec![stood(KITCHEN, 30, None), stood(STUDY, 30, None)],
            KITCHEN,
        );
        let acts = b.tune("alice", KITCHEN, &apart);
        assert_eq!(calls(&acts), [("alice", "report_group")], "{acts:?}");
        let Act::Call { input, .. } = &acts[0] else {
            unreachable!()
        };
        assert_eq!(input, &input::report_group(&[STUDY]));
        let together = house_of(
            vec![stood(KITCHEN, 30, None), stood(STUDY, 30, Some(KITCHEN))],
            KITCHEN,
        );
        assert!(b.tune("alice", KITCHEN, &together).is_empty());
        // Unjoined in the session: an unjoin of the study.
        assert_eq!(
            acts_but_calls(&b.tune("alice", KITCHEN, &apart)),
            [&Act::Unjoin {
                entity: STUDY.into()
            }]
        );
        b.polled(KITCHEN, Some(&sonos(30, false, &[])));
        b.polled(KITCHEN, Some(&sonos(30, false, &[])));
        assert!(b.tune("alice", KITCHEN, &apart).is_empty());
        // Joined in the session: a join on the kitchen, naming the study.
        assert_eq!(
            acts_but_calls(&b.tune("alice", KITCHEN, &together)),
            [&Act::Join {
                leader: KITCHEN.into(),
                members: vec![STUDY.into()]
            }]
        );
    }

    /// A member the session has handed to another leader is not unjoined by
    /// its old one — the new leader's `join` takes it, and an unjoin would
    /// undo that — and the old leader, now a member, is not paused when the
    /// sound moves to the member that leads it: a pause said to a member is
    /// a pause of its whole group.
    ///
    /// Falsified by unjoining every follower the session dropped: the
    /// kitchen unjoined the study the moment the study became its leader.
    #[test]
    fn the_sound_handed_to_a_member_regroups_the_house_without_a_pause() {
        let mut b = bridge();
        started(&mut b, "alice", &["music/a.mp3"], 0);
        b.polled(KITCHEN, Some(&sonos(30, false, &[KITCHEN, STUDY])));
        b.polled(STUDY, Some(&sonos(30, false, &[KITCHEN, STUDY])));
        let led = house_of(
            vec![stood(KITCHEN, 30, None), stood(STUDY, 30, Some(KITCHEN))],
            KITCHEN,
        );
        b.tune("alice", KITCHEN, &led);
        b.tune("alice", STUDY, &led);
        // Handed to the study, which leads now.
        let mut moved = house_of(
            vec![stood(KITCHEN, 30, Some(STUDY)), stood(STUDY, 30, None)],
            STUDY,
        );
        if let Some(a) = moved.session.as_mut() {
            a.output = Output::Device {
                id: STUDY.into(),
                moving: true,
            };
        }
        assert!(
            acts_but_calls(&b.tune("alice", KITCHEN, &moved)).is_empty(),
            "the kitchen lets go of nothing"
        );
        assert_eq!(
            acts_but_calls(&b.tune("alice", STUDY, &moved)),
            [&Act::Join {
                leader: STUDY.into(),
                members: vec![KITCHEN.into()]
            }]
        );
        assert!(
            b.wants("alice", KITCHEN, &moved, NOW).is_empty(),
            "no pause for a member of the new leader"
        );
        assert!(!b.holds(KITCHEN));
        assert!(matches!(
            &b.wants("alice", STUDY, &moved, NOW)[..],
            [Act::Start { .. }]
        ));
    }

    /// A Sonos playing on through a track says the same `media_position`
    /// the whole way — Home Assistant refreshes it on a change of state, and
    /// dates it — so where it is, is that position counted on from its date.
    /// Twenty seconds of polls of a speaker playing on are twenty seconds of
    /// nothing to report; the bug was a `seek` back to where the track
    /// started, every six seconds, on every device.
    ///
    /// Falsified by reading `media_position` as now (the old raw read): the
    /// fourth poll reported `seek` "it is somewhere else in the track", to 0.
    #[test]
    fn a_speaker_playing_on_reports_no_seek_from_a_frozen_position() {
        let mut b = bridge();
        started(&mut b, "alice", &["music/a.mp3"], 0);
        let anchored = session(&["music/a.mp3"], 0, playing_at(0), kitchen(false));
        assert!(b.wants("alice", KITCHEN, &anchored, NOW).is_empty());
        // What the house says for the whole of it: 0, as of the start.
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
        // Undated, a playing position is not a reason for anything.
        let undated = Playing {
            updated_ms: None,
            ..frozen.clone()
        };
        assert!(calls(&b.heard(KITCHEN, Some(&undated), NOW + 60_000)).is_empty());
        // Paused, it is where it says; and no count runs past the track.
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

    /// …and a jump made on the speaker itself is still one: a position the
    /// house dated, counted on, three seconds or more from where the session
    /// counts it, is said as the speaker's `seek`.
    ///
    /// Falsified by dropping the drift branch's `seek`: nothing was said.
    #[test]
    fn a_seek_made_on_the_speaker_is_reported() {
        let mut b = bridge();
        started(&mut b, "alice", &["music/a.mp3"], 0);
        let anchored = session(&["music/a.mp3"], 0, playing_at(0), kitchen(false));
        b.wants("alice", KITCHEN, &anchored, NOW);
        for second in 1..=3 {
            b.heard(
                KITCHEN,
                Some(&playing(&url_of("music/a.mp3"), 0)),
                NOW + second * 1_000,
            );
        }
        // Somebody drags the Sonos app to 90 s, five seconds in.
        let jumped = Playing {
            updated_ms: Some(NOW + 5_000),
            ..playing(&url_of("music/a.mp3"), 90_000)
        };
        let acts = b.heard(KITCHEN, Some(&jumped), NOW + 5_500);
        assert_eq!(calls(&acts), [("alice", "seek")]);
        let Act::Call { input, .. } = &acts[0] else {
            unreachable!()
        };
        assert_eq!(
            input,
            &input::seek(90_500),
            "where it is now: 90 s, half a second on"
        );
    }

    /// The session moving its sound elsewhere pauses the speaker, and nobody
    /// listening any more does too — once each, and only for the session that
    /// holds it.
    ///
    /// Falsified by `left` pausing whatever it was asked about: bob closing
    /// his app paused alice's kitchen.
    #[test]
    fn losing_the_session_pauses_the_speaker_once() {
        let mut b = bridge();
        started(&mut b, "alice", &["music/a.mp3"], 0);
        let phone = Output::Device {
            id: "phone".into(),
            moving: true,
        };
        let moved = session(&["music/a.mp3"], 0, playing_at(0), phone);
        assert!(matches!(
            &b.wants("alice", KITCHEN, &moved, NOW)[..],
            [Act::Verb {
                verb: "media_pause",
                ..
            }]
        ));
        assert!(b.wants("alice", KITCHEN, &moved, NOW).is_empty(), "once");

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
/// **Each speaker is a participant**, one connection per session it stands
/// in ([`ark_server::HubHandle::participate`]): a device and not a client,
/// so it is in a live instance and has no replica, no log and no socket. It
/// is told every frame of the instance, which is only a wake-up here: the
/// thread reads the sessions whole from the hub, which is in this process.
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
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::sync::Arc;
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    use ark::live::state::{Call, DeviceDecl, Instance};
    use ark::live::ConnId;
    use ark::protocol::Identity;
    use ark_server::HubHandle;
    use harken_domain::listening::read::{Kind, Listening, STATE};

    use super::{listening, playing, Act, Bridge, Playing, Speaker, GROUPING, VOLUME_SET};

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
    const WINDOW: usize = 200;

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
    pub fn start(config: Config, hub: HubHandle) -> Assistant {
        let stop = Arc::new(AtomicBool::new(false));
        let (wake, woken) = channel();
        let flag = stop.clone();
        let waker = wake.clone();
        let thread = std::thread::Builder::new()
            .name("harken-house".into())
            .spawn(move || {
                let mut bridge = Bridge::new(config.players.clone(), &config.media);
                let house = House::new(&config);
                run(&config, &house, &mut bridge, &hub, &woken, &waker, &flag);
            })
            .ok();
        if thread.is_none() {
            eprintln!("harken-server: house: could not start its thread");
        }
        Assistant { stop, wake, thread }
    }

    fn now_ms() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }

    /// Every session, as the hub holds it now.
    fn sessions(hub: &HubHandle) -> Vec<(String, Listening)> {
        hub.read_blocking(|h| {
            h.live()
                .instances
                .iter()
                .filter(|(i, _)| i.state == STATE)
                .filter_map(|(i, held)| Some((i.key.clone(), Listening::from_value(&held.value)?)))
                .collect()
        })
        .unwrap_or_default()
    }

    fn run(
        config: &Config,
        house: &House,
        bridge: &mut Bridge,
        hub: &HubHandle,
        woken: &Receiver<()>,
        waker: &Sender<()>,
        stop: &AtomicBool,
    ) {
        // One participant per (session, speaker).
        let mut wires: HashMap<(String, String), ConnId> = HashMap::new();
        let mut polled = Instant::now() - config.tick;
        let mut autos = ark_client::autos::Autos::system();
        // Since when each session with nobody listening and a speaker
        // standing has been silent.
        let mut silent: HashMap<String, Instant> = HashMap::new();
        loop {
            if stop.load(Ordering::Relaxed) {
                for (_, c) in wires.drain() {
                    let _ = hub.detach(c);
                }
                return;
            }
            let now = now_ms();
            let mut acts = vec![];
            let states = sessions(hub);
            // Where to stand, and which speakers: every one beside a client;
            // where no client is, the ones making the sound, until it has
            // been silent for `idle`.
            let entities: Vec<String> = bridge.players().iter().map(|(e, _)| e.clone()).collect();
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
                    if wires.contains_key(&k) {
                        continue;
                    }
                    let name = bridge
                        .players()
                        .iter()
                        .find(|(e, _)| e == entity)
                        .map(|(_, n)| n.clone())
                        .unwrap_or_default();
                    let tell = waker.clone();
                    let who = Identity {
                        user: (*key).clone(),
                        session: format!("house:{entity}"),
                        roles: Default::default(),
                    };
                    let device = DeviceDecl::new(entity.clone(), Kind::Speaker.name(), name, true);
                    if let Ok(c) = hub.participate(
                        who,
                        device,
                        vec![Instance::new(STATE, (*key).clone())],
                        move |_| {
                            let _ = tell.send(());
                        },
                    ) {
                        wires.insert(k, c);
                    }
                }
            }
            // …and nowhere else: a speaker that is not making the sound does
            // not keep a session's presence open, and one silent where nobody
            // is listening for longer than `idle` goes, pausing what it held.
            let gone: Vec<(String, String)> = wires
                .keys()
                .filter(|(key, entity)| !wanted(key, entity))
                .cloned()
                .collect();
            for k in gone {
                if let Some(c) = wires.remove(&k) {
                    let _ = hub.detach(c);
                }
                for act in bridge.left(&k.0, &k.1) {
                    println!(
                        "harken-server: house: {}: nobody is listening as {} and it is not playing; paused",
                        k.1, k.0
                    );
                    acts.push(act);
                }
            }
            // What each session wants of each speaker standing in it: its
            // volume and its group first, because a member about to lead is
            // made the leader in the house before anything is handed to it —
            // a queue handed to a member is its leader's.
            for (key, state, es) in &stood {
                for entity in es {
                    acts.extend(bridge.tune(key, entity, state));
                }
            }
            for (key, state, es) in &stood {
                for entity in es {
                    acts.extend(bridge.wants(key, entity, state, now));
                }
            }
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
                    .filter(|e| wires.keys().any(|(_, w)| w == e))
                    .collect();
                for entity in standing {
                    let reading = house.read(&entity);
                    bridge.polled(&entity, reading.as_ref().map(|(_, s)| s));
                    acts.extend(bridge.heard(&entity, reading.as_ref().map(|(p, _)| p), now_ms()));
                }
            }
            for act in acts {
                perform(house, hub, &wires, &mut autos, act);
            }
            // Until a frame, the next poll, or a stop.
            let wait = config
                .tick
                .saturating_sub(polled.elapsed())
                .max(Duration::from_millis(10));
            let _ = woken.recv_timeout(wait);
            while woken.try_recv().is_ok() {}
        }
    }

    fn seek(position_ms: i64) -> serde_json::Value {
        // Home Assistant takes seconds, as a number: the one float here, and
        // it is on the wire to the house, never in anything a replica applies.
        serde_json::json!({ "seek_position": position_ms as f64 / 1000.0 })
    }

    fn perform(
        house: &House,
        hub: &HubHandle,
        wires: &HashMap<(String, String), ConnId>,
        autos: &mut ark_client::autos::Autos,
        act: Act,
    ) {
        match act {
            Act::Call {
                key,
                entity,
                transition,
                input,
                why,
            } => {
                // A release is said out loud, because a speaker being let go
                // looks exactly like one that stopped for no reason, and the
                // reason is only ever known here.
                if transition == "release" {
                    println!("harken-server: house: {entity}: {why}; let go of {key}'s session");
                }
                let Some(c) = wires.get(&(key.clone(), entity.clone())) else {
                    return;
                };
                let _ = hub.call(
                    *c,
                    Call {
                        instance: Instance::new(STATE, key),
                        transition: transition.into(),
                        input,
                        id: autos.new_id(),
                        counter: None,
                    },
                );
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
                house.call(&entity, "clear_playlist", None);
                if !house.call(&entity, "play_media", Some(media(first, "play"))) {
                    return;
                }
                for url in urls.iter().skip(at + 1).take(WINDOW) {
                    house.call(&entity, "play_media", Some(media(url, "add")));
                }
                // Halfway through a track is where it picks up: the queue
                // starts the track from the beginning and this moves it to
                // where the session actually is. Issued against a speaker that
                // may not have read any of the stream yet, which Home
                // Assistant queues rather than refuses.
                if position_ms > 0 {
                    house.call(&entity, "media_seek", Some(seek(position_ms)));
                }
                if !playing {
                    house.call(&entity, "media_pause", None);
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
    }

    impl House {
        fn new(config: &Config) -> House {
            House {
                base: config.url.trim_end_matches('/').to_string(),
                bearer: format!("Bearer {}", config.token),
                agent: ureq::AgentBuilder::new().timeout(TIMEOUT).build(),
                families: Default::default(),
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
        fn read(&self, entity: &str) -> Option<(Playing, Speaker)> {
            let response = self
                .agent
                .get(&format!("{}/api/states/{entity}", self.base))
                .set("Authorization", &self.bearer)
                .call()
                .ok()?;
            // The house's clock, as its answer is dated: what its timestamps
            // are read against, so a house whose clock is not the server's
            // still places a position right.
            let house_now = response.header("date").and_then(super::when::http_date_ms);
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
                updated_ms: attrs
                    .get("media_position_updated_at")
                    .and_then(|v| v.as_str())
                    .and_then(super::when::rfc3339_ms)
                    .map(|at| super::when::on_our_clock(at, house_now, now_ms())),
                duration_ms: attrs
                    .get("media_duration")
                    .and_then(|v| v.as_f64())
                    .map(|d| (d * 1000.0) as i64),
                queue_position: attrs
                    .get("queue_position")
                    .and_then(|v| v.as_u64())
                    .and_then(|n| usize::try_from(n).ok()),
            };
            Some((playing, speaker))
        }
    }
}
