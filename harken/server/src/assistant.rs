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

/// How far a playing speaker may be from where the session counts it before
/// one is told the other: the speaker is right about where it is (`seek`
/// said as it), and the session is right about where somebody asked it to
/// be (`media_seek` told to it).
pub const DRIFT_MS: i64 = 3_000;

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
    pub position_ms: i64,
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
    /// A transition of `key`'s session, called as the speaker standing in it.
    Call {
        key: String,
        entity: String,
        transition: &'static str,
        input: Args,
        why: String,
    },
}

/// What one speaker was handed, by whose session, and what the bridge
/// believes of it.
#[derive(Debug, Clone)]
struct Holding {
    key: String,
    /// The queue as the speaker was given it, so what it reports back can be
    /// matched to an index without re-deriving anything.
    urls: Vec<String>,
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
        let position_ms = a.clock.position(now);
        self.held.insert(
            entity.to_string(),
            Holding {
                key: key.to_string(),
                urls: urls.clone(),
                handed_at: a.at,
                started: false,
                at: a.at,
                playing: a.clock.playing(),
                clock: a.clock,
                settling: SETTLE,
                quiet: QUIET,
                heard: None,
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
            if self.holder(entity) == Some(key) {
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
            let want = a.clock.position(now);
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
        let at = poll.and_then(|p| h.urls.iter().position(|u| *u == p.url));
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
        let position = p.position_ms.max(0);
        let playing = p.state == "playing";
        let mut acts = vec![];
        if !h.started {
            // Still fetching: what it says is whatever it played before, even
            // a track of this same queue, until it says the one it was handed.
            if at != h.handed_at {
                return self.lapse(entity, poll);
            }
            h.started = true;
            h.settling = LAPSE;
            h.heard = Some((position, now));
            acts.push(call("started", input::at(position), "it has the hand-off"));
            return acts;
        }
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
        } else if playing && (position - h.clock.position(now)).abs() > DRIFT_MS {
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
        self.held.remove(entity);
        let why = match poll {
            Some(p) => format!("{} {:?}, which is not ours", p.state, p.url),
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

    /// Nobody is listening as `key` any more, so the speaker stops standing
    /// in that session — and anything it was playing for it stops, because
    /// nothing else would ever tell it to.
    pub fn left(&mut self, key: &str, entity: &str) -> Vec<Act> {
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

/// Whether anybody is *listening* in a session, as opposed to standing in it:
/// a client of any kind but a speaker. A speaker is in the session when
/// somebody is listening, not the other way round — so a speaker never opens
/// a session's presence nor keeps it, and the bridge stands only where this
/// holds.
pub fn listening(state: &Listening) -> bool {
    state
        .devices
        .iter()
        .any(|d| d.kind != harken_domain::listening::read::Kind::Speaker)
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
            devices: vec![Device {
                id: "phone".into(),
                name: "Phone".into(),
                kind: Kind::Phone,
                can_play: true,
                user: "alice".into(),
            }],
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

    fn playing(url: &str, position_ms: i64) -> Playing {
        Playing {
            state: "playing".into(),
            url: url.into(),
            position_ms,
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
    /// the session had got to.
    ///
    /// Falsified by handing the position as the clock's anchor rather than
    /// counted to now: the speaker was told 42 000 where 43 500 was right.
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
                position_ms: 43_500,
                playing: true,
            }]
        );
        assert_eq!(b.holder(KITCHEN), Some("alice"));
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
        let acts = b.heard(KITCHEN, Some(&playing(&url_of(files[0]), 100)), NOW + 5_000);
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
/// It stands in a session only while a client is in it ([`listening`]), so
/// the presence a person sees is theirs and the speakers beside it, never a
/// shelf of speakers in an account nobody has opened. It polls only the
/// players it is *holding*: a speaker nobody in a session handed anything to
/// costs nothing, and a server whose house is asleep makes no requests at
/// all.
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

    use super::{listening, Act, Bridge, Playing};

    /// How often a held player is asked what it is doing.
    pub const TICK: Duration = Duration::from_millis(1_000);

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
            let listened: Vec<&(String, Listening)> =
                states.iter().filter(|(_, s)| listening(s)).collect();
            // Stand where somebody is listening…
            for (key, _) in &listened {
                for (entity, name) in bridge.players().to_vec() {
                    let k = (key.clone(), entity.clone());
                    if wires.contains_key(&k) {
                        continue;
                    }
                    let tell = waker.clone();
                    let who = Identity {
                        user: key.clone(),
                        session: format!("house:{entity}"),
                        roles: Default::default(),
                    };
                    let device = DeviceDecl::new(entity.clone(), Kind::Speaker.name(), name, true);
                    if let Ok(c) = hub.participate(
                        who,
                        device,
                        vec![Instance::new(STATE, key.clone())],
                        move |_| {
                            let _ = tell.send(());
                        },
                    ) {
                        wires.insert(k, c);
                    }
                }
            }
            // …and nowhere else: a speaker does not keep a session's presence
            // open, and what one was playing for nobody stops.
            let gone: Vec<(String, String)> = wires
                .keys()
                .filter(|(key, _)| !listened.iter().any(|(k, _)| k == key))
                .cloned()
                .collect();
            for k in gone {
                if let Some(c) = wires.remove(&k) {
                    let _ = hub.detach(c);
                }
                for act in bridge.left(&k.0, &k.1) {
                    println!(
                        "harken-server: house: {}: nobody is listening as {} any more; paused",
                        k.1, k.0
                    );
                    acts.push(act);
                }
            }
            // What each session wants of each speaker standing in it.
            for (key, state) in &listened {
                for (entity, _) in bridge.players().to_vec() {
                    acts.extend(bridge.wants(key, &entity, state, now));
                }
            }
            // …and, a tick at a time, what the held ones are actually doing.
            if polled.elapsed() >= config.tick {
                polled = Instant::now();
                let held: Vec<String> = bridge
                    .players()
                    .iter()
                    .map(|(e, _)| e.clone())
                    .filter(|e| bridge.holds(e))
                    .collect();
                for entity in held {
                    let now_playing = house.state(&entity);
                    acts.extend(bridge.heard(&entity, now_playing.as_ref(), now_ms()));
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

    /// Home Assistant's REST API, the two calls of it this needs.
    struct House {
        base: String,
        bearer: String,
        agent: ureq::Agent,
    }

    impl House {
        fn new(config: &Config) -> House {
            House {
                base: config.url.trim_end_matches('/').to_string(),
                bearer: format!("Bearer {}", config.token),
                agent: ureq::AgentBuilder::new().timeout(TIMEOUT).build(),
            }
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

        /// What one player is doing, or `None` if it will not say.
        fn state(&self, entity: &str) -> Option<Playing> {
            let body: serde_json::Value = self
                .agent
                .get(&format!("{}/api/states/{entity}", self.base))
                .set("Authorization", &self.bearer)
                .call()
                .ok()?
                .into_json()
                .ok()?;
            let state = body.get("state")?.as_str()?.to_string();
            if state == "unavailable" || state == "unknown" {
                return None;
            }
            let attrs = body.get("attributes")?;
            Some(Playing {
                state,
                url: attrs
                    .get("media_content_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                // As the house last measured it. Not counted forward from
                // `media_position_updated_at`: within a poll it does not
                // matter, and past one the next poll says again.
                position_ms: (attrs
                    .get("media_position")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0)
                    * 1000.0) as i64,
            })
        }
    }
}
