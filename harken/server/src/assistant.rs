//! Home Assistant's media players, as devices in a listening session.
//!
//! A Sonos is a *device*, not a client. `harken_domain::listening` already describes
//! one — it joins a room, becomes the output, is told things and reports what
//! it is doing — and nothing in that says the far end has to be somebody's
//! screen. So nothing here extends the protocol; what is here is the thing
//! that is a device on a speaker's behalf.
//!
//! **Home Assistant rather than Sonos directly**, and not for convenience: you
//! do not get Sonos, you get every `media_player` entity in the house. A
//! Chromecast, a television, an AirPlay receiver and a speaker group are all
//! the same six service calls, where talking UPnP would buy one make of
//! speaker and a discovery problem.
//!
//! [`Bridge`] owns no socket, like [`crate::listening::Desk`] and for the same
//! reason: what is worth asserting is the rules — which account holds a
//! speaker, what a command becomes, how a position report is read back — and
//! none of that needs a network to be wrong in.
//!
//! **The queue is harken's and the position is the speaker's.** The whole list
//! is enqueued, so the physical buttons and the Sonos app keep working; where
//! the speaker *is* in that list is then read back from what it says it is
//! playing. That is not two sources of truth, it is the rule the `Desk`
//! already has — the output reports, and whoever is making the sound is right
//! about it. Somebody skipping on the speaker itself moves every phone.

use std::collections::HashMap;

use harken_domain::listening::{url, Command, Device, DeviceId, Track};

/// How many polls a freshly handed speaker may answer with something that is
/// not ours before the session stops claiming it.
///
/// One tick is a second, so five is five seconds of fetching — longer than a
/// Sonos on a LAN takes and long enough for a Chromecast to wake up, and
/// short enough that a speaker somebody sent elsewhere is let go while they
/// are still looking at the screen they did it from.
const SETTLE: u8 = 5;

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
const LAPSE: u8 = 3;

/// How Home Assistant describes a player, pared down to what decides
/// anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playing {
    /// `playing`, `paused`, `idle`, `off`, `unavailable`. Anything that is not
    /// `playing` is not playing, which is the only distinction that matters.
    pub state: String,
    /// What it says it is playing, which is how it says where it is in the
    /// queue it was given. Empty when it is playing nothing.
    pub url: String,
    pub position_ms: i64,
}

/// What the bridge wants done. The socket performs these; this module never
/// sees one happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    /// Put this list on, from `at`, at this point. The socket clears the
    /// queue, turns it into one `play_media` with `enqueue: play` and the
    /// rest as `enqueue: add`, which is what leaves the speaker's own next
    /// and previous working.
    Start {
        entity: DeviceId,
        urls: Vec<String>,
        at: u32,
        position_ms: i64,
        playing: bool,
    },
    /// `media_player.<verb>` on `entity`, with nothing else to say.
    Verb {
        entity: DeviceId,
        verb: &'static str,
    },
    Seek {
        entity: DeviceId,
        position_ms: i64,
    },
    /// …and the one that is not a service call: tell `user`'s room that the
    /// speaker they think they have is not theirs any more.
    ///
    /// Three things arrive here. A speaker is one piece of hardware and a room
    /// is one account's, so two people can both pick the kitchen: the second
    /// one gets it — which is what a real speaker does — and the first is told
    /// rather than left drawing a transport for a device playing somebody
    /// else's music. A speaker sent somewhere else entirely, by the Sonos app
    /// or a doorbell, is the same sentence. So is one that has stopped
    /// answering, because a bar with a scrubber counting along a speaker that
    /// is not there is the same lie one frame later.
    Release {
        user: String,
        entity: DeviceId,
    },
}

/// What a speaker said, in the shape `crate::listening::Desk`'s `report` takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Said {
    pub user: String,
    pub device: DeviceId,
    pub queue: Vec<Track>,
    pub at: u32,
    pub playing: bool,
    pub position_ms: i64,
}

/// What a poll of one speaker came to.
///
/// Three answers rather than two, and the third is the one that had to be
/// added: a speaker handed a queue does not play it *this instant*. It fetches
/// the first URL, which on a Sonos on a LAN is a second or two and on a
/// Chromecast can be longer — and for every tick of that it is still reporting
/// whatever it was playing before, or nothing at all. Read as "playing
/// something that is not ours" that is a hand-off which releases itself
/// half a second after it was asked for, and what somebody sees is a speaker
/// they picked handing the music straight back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    /// Where it is in the queue it was given.
    Said(Said),
    /// It was handed something and has not said so yet. Nothing to do but
    /// ask again — which is also what keeps `Session::moving` set, so the
    /// picker goes on saying *connecting* rather than *playing*.
    Settling,
    /// It is playing something that is not ours, or it has stopped answering.
    /// The session lets it go and `user`'s room is told.
    Gone { user: String },
}

/// What one speaker was handed, and by whom.
#[derive(Debug, Clone)]
struct Holding {
    user: String,
    queue: Vec<Track>,
    /// The same tracks as the speaker was given them, so what it reports back
    /// can be matched to an index without re-deriving anything.
    urls: Vec<String>,
    /// How many more polls it may answer with something that is not ours, or
    /// not answer at all, before it is let go. [`SETTLE`] on a hand-off,
    /// [`LAPSE`] again after every poll that names one of our URLs.
    settling: u8,
}

/// Home Assistant, as a shelf of devices.
pub struct Bridge {
    players: Vec<Device>,
    /// Where a speaker fetches bytes from, which is not where a phone does:
    /// the phone may be on `https://harken.example.com` while the speaker
    /// only knows an address on the LAN.
    media: String,
    held: HashMap<DeviceId, Holding>,
}

impl Bridge {
    /// `players` are the entities to offer, already named. `media` is the base
    /// a speaker resolves a `file` against.
    pub fn new(players: Vec<Device>, media: &str) -> Bridge {
        Bridge {
            players,
            media: media.trim_end_matches('/').to_string(),
            held: HashMap::new(),
        }
    }

    /// The devices to stand in every room.
    pub fn devices(&self) -> &[Device] {
        &self.players
    }

    pub fn has(&self, entity: &str) -> bool {
        self.players.iter().any(|p| p.id == entity)
    }

    /// `user`'s room told `entity` to do something.
    pub fn told(&mut self, user: &str, entity: &str, command: Command) -> Vec<Act> {
        if !self.has(entity) {
            return Vec::new();
        }
        let mut acts = Vec::new();
        // Somebody else had it. They are told before it moves, so the message
        // that takes the speaker away and the sound that leaves are the same
        // event rather than two.
        if let Some(held) = self.held.get(entity) {
            if held.user != user {
                acts.push(Act::Release {
                    user: held.user.clone(),
                    entity: entity.to_string(),
                });
            }
        }
        let entity = entity.to_string();
        match command {
            Command::Start {
                queue,
                at,
                position_ms,
                playing,
            } => {
                let urls: Vec<String> = queue.iter().map(|t| url(&self.media, &t.file)).collect();
                self.held.insert(
                    entity.clone(),
                    Holding {
                        user: user.to_string(),
                        queue,
                        urls: urls.clone(),
                        settling: SETTLE,
                    },
                );
                acts.push(Act::Start {
                    entity,
                    urls,
                    at,
                    position_ms,
                    playing,
                });
            }
            other => {
                // Everything else is about a queue the speaker already has, so
                // a command for a speaker nobody handed anything to is a
                // command with nothing to do. Play is the exception worth
                // making: a speaker that was paused by hand is one `play` away
                // from carrying on.
                if !self.held.contains_key(&entity) && !matches!(other, Command::Play) {
                    return acts;
                }
                if let Some(held) = self.held.get_mut(&entity) {
                    held.user = user.to_string();
                }
                acts.push(match other {
                    Command::Play => Act::Verb {
                        entity,
                        verb: "media_play",
                    },
                    Command::Pause => Act::Verb {
                        entity,
                        verb: "media_pause",
                    },
                    Command::Next => Act::Verb {
                        entity,
                        verb: "media_next_track",
                    },
                    Command::Previous => Act::Verb {
                        entity,
                        verb: "media_previous_track",
                    },
                    Command::Seek { position_ms } => Act::Seek {
                        entity,
                        position_ms,
                    },
                    Command::Start { .. } => unreachable!("matched above"),
                });
            }
        }
        acts
    }

    /// Home Assistant said something about a player.
    ///
    /// `None` for a player nobody in a session handed anything to: a speaker
    /// somebody is using from the Sonos app is not this server's business, and
    /// reporting it would put a stranger's music in somebody's bar.
    pub fn heard(&mut self, entity: &str, now: &Playing) -> Option<Heard> {
        let held = self.held.get_mut(entity)?;
        // Where it says it is. A URL that is not one of ours means either that
        // it has not started yet or that it has been sent somewhere else
        // entirely — a radio stream, a doorbell chime — and the only thing
        // that tells those apart is how long ago we handed it something.
        let Some(at) = held.urls.iter().position(|u| *u == now.url) else {
            return self.lapse(entity);
        };
        held.settling = LAPSE;
        Some(Heard::Said(Said {
            user: held.user.clone(),
            device: entity.to_string(),
            queue: held.queue.clone(),
            at: at as u32,
            playing: now.state == "playing",
            position_ms: now.position_ms.max(0),
        }))
    }

    /// The speaker did not answer this poll. A few of those in a row and it is
    /// let go, and whoever had it is told — for the reason [`Act::Release`]
    /// gives; one alone is a lapse, for the reason [`LAPSE`] gives.
    pub fn lost(&mut self, entity: &str) -> Option<Heard> {
        self.lapse(entity)
    }

    /// One poll that was not the speaker playing ours: spent from the grace it
    /// has, and the last one lets it go.
    fn lapse(&mut self, entity: &str) -> Option<Heard> {
        let held = self.held.get_mut(entity)?;
        if held.settling > 0 {
            held.settling -= 1;
            return Some(Heard::Settling);
        }
        let user = held.user.clone();
        self.held.remove(entity);
        Some(Heard::Gone { user })
    }

    /// Whose queue this speaker is playing, if anybody's.
    pub fn holder(&self, entity: &str) -> Option<&str> {
        self.held.get(entity).map(|h| h.user.as_str())
    }

    /// Whether anybody has handed this speaker something. Only those are
    /// worth asking Home Assistant about.
    pub fn holds(&self, entity: &str) -> bool {
        self.held.contains_key(entity)
    }

    /// The speaker is gone — unavailable, or the room it was in has closed.
    pub fn forget(&mut self, entity: &str) {
        self.held.remove(entity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harken_domain::listening::Kind;

    fn speaker(id: &str) -> Device {
        Device {
            id: id.into(),
            name: id.into(),
            audible: true,
            here: true,
            kind: Kind::Speaker,
        }
    }

    fn track(file: &str) -> Track {
        Track {
            id: String::new(),
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
                speaker("media_player.kitchen"),
                speaker("media_player.study"),
            ],
            "http://10.0.0.2:8787/",
        )
    }

    fn start(queue: &[&str], at: u32) -> Command {
        Command::Start {
            queue: queue.iter().map(|f| track(f)).collect(),
            at,
            position_ms: 0,
            playing: true,
        }
    }

    /// A hand-off arrives as the whole list, resolved against the address the
    /// *speaker* can reach — which is not the one a phone uses.
    #[test]
    fn a_hand_off_is_the_whole_queue_as_urls_the_speaker_can_fetch() {
        let mut bridge = bridge();
        let acts = bridge.told(
            "alice",
            "media_player.kitchen",
            start(&["music/a.mp3", "music/b.mp3"], 1),
        );
        assert_eq!(
            acts,
            [Act::Start {
                entity: "media_player.kitchen".into(),
                urls: vec![
                    "http://10.0.0.2:8787/media/music/a.mp3".into(),
                    "http://10.0.0.2:8787/media/music/b.mp3".into(),
                ],
                at: 1,
                position_ms: 0,
                playing: true,
            }]
        );
    }

    /// An entity this server was not told to offer is not a device, so a
    /// command for one does nothing rather than reaching into the house.
    #[test]
    fn nothing_is_said_to_a_player_that_was_not_offered() {
        let mut bridge = bridge();
        assert!(bridge
            .told("alice", "media_player.bedroom", start(&["music/a.mp3"], 0))
            .is_empty());
        assert!(bridge
            .heard("media_player.bedroom", &playing("x", 0))
            .is_none());
    }

    fn playing(url: &str, position_ms: i64) -> Playing {
        Playing {
            state: "playing".into(),
            url: url.into(),
            position_ms,
        }
    }

    /// Where the speaker says it is, is where it is — including when nobody
    /// asked it to move. Somebody pressing the button on the speaker moves
    /// every phone.
    #[test]
    fn what_the_speaker_says_it_is_playing_is_where_it_is_in_the_queue() {
        let mut bridge = bridge();
        bridge.told(
            "alice",
            "media_player.kitchen",
            start(&["music/a.mp3", "music/b.mp3", "music/c.mp3"], 0),
        );

        let Some(Heard::Said(said)) = bridge.heard(
            "media_player.kitchen",
            &playing("http://10.0.0.2:8787/media/music/c.mp3", 4_200),
        ) else {
            panic!("it is playing one of ours")
        };
        assert_eq!(said.user, "alice");
        assert_eq!(said.at, 2, "the third, because that is what it named");
        assert_eq!(said.queue.len(), 3);
        assert!(said.playing);
        assert_eq!(said.position_ms, 4_200);

        // Paused is every state that is not `playing`, which is the only
        // distinction this needs to make.
        let Some(Heard::Said(said)) = bridge.heard(
            "media_player.kitchen",
            &Playing {
                state: "paused".into(),
                ..playing("http://10.0.0.2:8787/media/music/c.mp3", 4_200)
            },
        ) else {
            panic!("still one of ours")
        };
        assert!(!said.playing);
    }

    /// A speaker sent somewhere else entirely is no longer in this session,
    /// and saying otherwise would be a lie with a scrubber on it.
    #[test]
    fn a_speaker_playing_something_that_is_not_ours_is_let_go() {
        let mut bridge = bridge();
        bridge.told("alice", "media_player.kitchen", start(&["music/a.mp3"], 0));
        // It settles first, though — see the next test for why that is not
        // the same question.
        for _ in 0..SETTLE {
            bridge.heard(
                "media_player.kitchen",
                &playing("http://radio.example/stream", 0),
            );
        }
        assert_eq!(
            bridge.heard(
                "media_player.kitchen",
                &playing("http://radio.example/stream", 0)
            ),
            Some(Heard::Gone {
                user: "alice".into()
            })
        );
        // …and it stays let go, rather than being recovered by the next
        // report that happens to match.
        assert!(bridge
            .heard(
                "media_player.kitchen",
                &playing("http://10.0.0.2:8787/media/music/a.mp3", 0)
            )
            .is_none());
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
    #[test]
    fn a_speaker_is_given_a_moment_to_start_before_it_is_let_go() {
        let mut bridge = bridge();
        bridge.told("alice", "media_player.kitchen", start(&["music/a.mp3"], 0));
        let quiet = Playing {
            state: "idle".into(),
            url: String::new(),
            position_ms: 0,
        };
        for tick in 0..3 {
            assert_eq!(
                bridge.heard("media_player.kitchen", &quiet),
                Some(Heard::Settling),
                "still fetching, {tick} ticks in"
            );
        }
        // …and then it starts, which is the end of the grace whether or not it
        // was spent: a speaker that takes two seconds must not be let go on
        // its sixth.
        assert!(matches!(
            bridge.heard(
                "media_player.kitchen",
                &playing("http://10.0.0.2:8787/media/music/a.mp3", 0)
            ),
            Some(Heard::Said(_))
        ));
        // Having started, silence is a lapse rather than fetching: three
        // polls of it — a track boundary, a blink of the integration — and
        // the fourth is the one that lets it go. Literal counts, for the
        // reason above.
        for tick in 0..3 {
            assert_eq!(
                bridge.heard("media_player.kitchen", &quiet),
                Some(Heard::Settling),
                "a lapse, {tick} ticks in"
            );
        }
        assert_eq!(
            bridge.heard("media_player.kitchen", &quiet),
            Some(Heard::Gone {
                user: "alice".into()
            }),
            "silent for three seconds, it is not ours any more"
        );
        // …and a poll that matched in between starts the three again, which
        // is what keeps a speaker on a long album.
        bridge.told("alice", "media_player.kitchen", start(&["music/a.mp3"], 0));
        for _ in 0..4 {
            bridge.heard(
                "media_player.kitchen",
                &playing("http://10.0.0.2:8787/media/music/a.mp3", 0),
            );
            bridge.heard("media_player.kitchen", &quiet);
            bridge.heard("media_player.kitchen", &quiet);
        }
        assert_eq!(bridge.holder("media_player.kitchen"), Some("alice"));
    }

    /// A speaker that has stopped answering is let go too, and whoever had it
    /// is told: a bar counting up for a speaker that is not there is worse
    /// than one that stops. Not on the first poll it misses, though — a proxy
    /// refusing one request, or an integration reloading, is a second of
    /// silence and not a speaker that is gone.
    #[test]
    fn a_speaker_that_stops_answering_is_let_go() {
        let mut bridge = bridge();
        assert!(bridge.lost("media_player.kitchen").is_none());
        bridge.told("alice", "media_player.kitchen", start(&["music/a.mp3"], 0));
        let Some(Heard::Said(_)) = bridge.heard(
            "media_player.kitchen",
            &playing("http://10.0.0.2:8787/media/music/a.mp3", 0),
        ) else {
            panic!("playing ours")
        };
        assert_eq!(bridge.holder("media_player.kitchen"), Some("alice"));
        for miss in 0..3 {
            assert_eq!(
                bridge.lost("media_player.kitchen"),
                Some(Heard::Settling),
                "still held after {miss} unanswered polls"
            );
        }
        assert_eq!(
            bridge.lost("media_player.kitchen"),
            Some(Heard::Gone {
                user: "alice".into()
            })
        );
        assert_eq!(bridge.holder("media_player.kitchen"), None);
    }

    /// Two people can both pick the kitchen, because a kitchen is one room.
    /// The second gets it and the first is told, rather than left drawing a
    /// transport for somebody else's music.
    #[test]
    fn a_speaker_taken_by_somebody_else_releases_the_first() {
        let mut bridge = bridge();
        bridge.told("alice", "media_player.kitchen", start(&["music/a.mp3"], 0));

        let acts = bridge.told("bob", "media_player.kitchen", start(&["music/b.mp3"], 0));
        assert_eq!(
            acts[0],
            Act::Release {
                user: "alice".into(),
                entity: "media_player.kitchen".into(),
            }
        );
        assert!(matches!(acts[1], Act::Start { .. }));
        assert_eq!(acts.len(), 2);

        // …and it is bob's now, so his pause is not alice's release.
        let acts = bridge.told("bob", "media_player.kitchen", Command::Pause);
        assert_eq!(
            acts,
            [Act::Verb {
                entity: "media_player.kitchen".into(),
                verb: "media_pause",
            }]
        );
        let Some(Heard::Said(said)) = bridge.heard(
            "media_player.kitchen",
            &playing("http://10.0.0.2:8787/media/music/b.mp3", 0),
        ) else {
            panic!("it is playing bob's")
        };
        assert_eq!(said.user, "bob");
    }

    /// The four verbs that are about a queue the speaker already has, and the
    /// one that is not.
    #[test]
    fn a_verb_needs_a_queue_except_the_one_that_does_not() {
        let mut bridge = bridge();
        // Nothing has been handed over, so there is nothing to pause or skip.
        for command in [Command::Pause, Command::Next, Command::Previous] {
            assert!(bridge
                .told("alice", "media_player.study", command)
                .is_empty());
        }
        // Play is the exception: a speaker somebody paused by hand is one
        // press away from carrying on.
        assert_eq!(
            bridge.told("alice", "media_player.study", Command::Play),
            [Act::Verb {
                entity: "media_player.study".into(),
                verb: "media_play",
            }]
        );

        bridge.told("alice", "media_player.study", start(&["music/a.mp3"], 0));
        assert_eq!(
            bridge.told(
                "alice",
                "media_player.study",
                Command::Seek { position_ms: 9_000 }
            ),
            [Act::Seek {
                entity: "media_player.study".into(),
                position_ms: 9_000,
            }]
        );
        assert_eq!(
            bridge.told("alice", "media_player.study", Command::Next),
            [Act::Verb {
                entity: "media_player.study".into(),
                verb: "media_next_track",
            }],
            "the speaker's own next, because it was given the whole list"
        );
    }
}

/// The socket half: one thread, Home Assistant's REST API, and the rooms.
///
/// Blocking and polled, on a thread of its own, exactly as the scanner is.
/// There is no async in this server's own code and a bridge to a house is
/// not a reason to start — a handful of small requests a second on a LAN is
/// not a problem worth a runtime.
///
/// **Each speaker is a standing peer of the live channel**, one per room it
/// is offered in. [`ark_server::HubHandle::stand`] is for exactly this: a
/// thing that is a device and not a client, so it is in a room and has no
/// replica, no log and no socket. What reaches it is the app's own `Hear`
/// and nothing else — which is why the listening protocol needed no
/// speaker-shaped hole in it.
///
/// It polls only the players it is *holding*: a speaker nobody in a session
/// handed anything to costs nothing, and a server whose house is asleep makes
/// no requests at all.
pub mod ha {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{channel, Receiver};
    use std::sync::Arc;
    use std::thread::JoinHandle;
    use std::time::Duration;

    use ark_server::HubHandle;
    use harken_domain::listening::{Device, DeviceId, Hear, Kind, Say};

    use super::{Act, Bridge, Heard, Playing, Said};
    use crate::listening::Watch;

    /// How often a held player is asked what it is doing. The same cadence
    /// the clients report on, for the same reason: it is what keeps a
    /// scrubber drawn from somewhere else honest.
    pub const TICK: Duration = Duration::from_millis(1_000);

    /// How much of a queue is pushed at a speaker. Enough that nobody reaches
    /// the end of it by hand, and few enough that starting a track is not a
    /// hundred service calls.
    const WINDOW: usize = 200;

    /// How long one request to the house may take. A house that does not
    /// answer is asked again next tick; a request that never returns would
    /// stop the bridge for every room.
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
    /// and takes every speaker out of every room.
    pub struct Assistant {
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl Drop for Assistant {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    /// One speaker's wire into one room.
    struct Wire {
        conn: ark::live::ConnId,
        hears: Receiver<Vec<u8>>,
    }

    /// Offer the house's players to every account that is listening. `rooms`
    /// is what [`crate::listening::Desk::watch`] was given.
    pub fn start(config: Config, hub: HubHandle, rooms: Receiver<Watch>) -> Assistant {
        let players: Vec<Device> = config
            .players
            .iter()
            .map(|(id, name)| Device {
                id: id.clone(),
                name: name.clone(),
                // A speaker is a speaker. This is the whole reason it is worth
                // being a device rather than a controller.
                audible: true,
                here: true,
                kind: Kind::Speaker,
            })
            .collect();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::Builder::new()
            .name("harken-house".into())
            .spawn(move || {
                let mut bridge = Bridge::new(players, &config.media);
                let house = House::new(&config);
                run(&config, &house, &mut bridge, &hub, &rooms, &flag);
            })
            .ok();
        if thread.is_none() {
            eprintln!("harken-server: house: could not start its thread");
        }
        Assistant { stop, thread }
    }

    fn run(
        config: &Config,
        house: &House,
        bridge: &mut Bridge,
        hub: &HubHandle,
        rooms: &Receiver<Watch>,
        stop: &AtomicBool,
    ) {
        // One wire per (room, speaker). A speaker stands in a room while
        // somebody is listening in it and is taken out again when nobody is —
        // because a bridge standing in every room for ever would mean no room
        // was ever empty, so the queue somebody left behind would still be
        // there tomorrow.
        let mut wires: HashMap<(String, DeviceId), Wire> = HashMap::new();
        loop {
            if stop.load(Ordering::Relaxed) {
                for (_, wire) in wires.drain() {
                    let _ = hub.detach(wire.conn);
                }
                return;
            }

            while let Ok(news) = rooms.try_recv() {
                match news {
                    Watch::Open(room) => open(hub, bridge, &mut wires, &room),
                    Watch::Shut(room) => shut(house, hub, bridge, &mut wires, &room),
                }
            }

            // What those rooms have said. In order, because a hand-off is a
            // `Do` and a `State` one after the other, and reading them the
            // other way round would have the speaker let go of a queue it was
            // handed in the same breath.
            let mut acts = Vec::new();
            let mut dropped = Vec::new();
            let mut keys: Vec<&(String, DeviceId)> = wires.keys().collect();
            keys.sort();
            for key in keys {
                let (room, entity) = key;
                let wire = &wires[key];
                while let Ok(frame) = wire.hears.try_recv() {
                    match Hear::decode(&frame) {
                        Ok(Hear::Do { command }) => acts.extend(bridge.told(room, entity, command)),
                        // The room saying who the output is, which for a
                        // speaker is the one thing a broadcast is good for. A
                        // client that is not the output simply goes quiet; a
                        // speaker has nobody to do that for it, so losing the
                        // sound has to become a `media_pause` here.
                        Ok(Hear::State { session }) => {
                            if !session.outputs(entity)
                                && bridge.holder(entity).is_some_and(|u| u == room)
                            {
                                dropped.push(entity.clone());
                            }
                        }
                        Err(_) => {}
                    }
                }
            }
            for entity in dropped {
                println!(
                    "harken-server: house: {entity}: the room moved its sound elsewhere; paused"
                );
                bridge.forget(&entity);
                house.call(&entity, "media_pause", None);
            }
            for act in acts {
                perform(house, hub, &wires, act);
            }

            // …and what they are actually doing.
            for entity in holding(bridge) {
                let now = house.state(&entity);
                let answer = match &now {
                    Some(now) => bridge.heard(&entity, now),
                    // Unreachable, unavailable, or renamed. Letting go is the
                    // honest answer — after a few polls of it, since one is a
                    // proxy blinking.
                    None => bridge.lost(&entity),
                };
                match answer {
                    Some(Heard::Said(said)) => report(hub, &wires, said),
                    // Still fetching, or one blink. Saying nothing is what
                    // leaves `moving` set and the picker reading *connecting*.
                    Some(Heard::Settling) | None => {}
                    Some(Heard::Gone { user }) => {
                        // Said out loud, because a speaker being let go looks
                        // exactly like one that stopped for no reason, and the
                        // reason is only ever known here.
                        match &now {
                            Some(now) => println!("harken-server: house: {entity}: {} {:?}, which is not ours; let go", now.state, now.url),
                            None => println!("harken-server: house: {entity}: not answering; let go"),
                        }
                        perform(
                            house,
                            hub,
                            &wires,
                            Act::Release {
                                user,
                                entity: entity.clone(),
                            },
                        );
                    }
                }
            }

            // Sleep in slices, so a stop is not a whole tick late.
            let mut slept = Duration::ZERO;
            while slept < config.tick && !stop.load(Ordering::Relaxed) {
                let step = Duration::from_millis(20).min(config.tick - slept);
                std::thread::sleep(step);
                slept += step;
            }
        }
    }

    /// Somebody is listening as this account, so every speaker is offered
    /// to them: it stands in their room and says what it is, which is what
    /// puts it in their picker.
    fn open(
        hub: &HubHandle,
        bridge: &Bridge,
        wires: &mut HashMap<(String, DeviceId), Wire>,
        room: &str,
    ) {
        for device in bridge.devices() {
            let key = (room.to_string(), device.id.clone());
            if wires.contains_key(&key) {
                continue;
            }
            let (tx, hears) = channel();
            // A device id is whatever names one output stably. For a client
            // that is its login; for a speaker it is the entity, because that
            // is the thing that is still the same speaker tomorrow.
            let Ok(conn) = hub.stand(room, &device.id, move |frame| {
                let _ = tx.send(frame);
            }) else {
                continue;
            };
            let _ = hub.say(
                conn,
                Say::Here {
                    name: device.name.clone(),
                    audible: true,
                    kind: Kind::Speaker,
                }
                .encode(),
            );
            wires.insert(key, Wire { conn, hears });
        }
    }

    /// Nobody is listening as this account any more. The speakers come out,
    /// which is what lets the room empty — and anything one of them was
    /// playing stops, because nothing else would ever tell it to.
    fn shut(
        house: &House,
        hub: &HubHandle,
        bridge: &mut Bridge,
        wires: &mut HashMap<(String, DeviceId), Wire>,
        room: &str,
    ) {
        let mut mine: Vec<(String, DeviceId)> =
            wires.keys().filter(|(r, _)| r == room).cloned().collect();
        mine.sort();
        for key in mine {
            if let Some(wire) = wires.remove(&key) {
                let _ = hub.detach(wire.conn);
            }
            let entity = key.1;
            // Only if this room is the one holding it: somebody else may have
            // taken the kitchen in the meantime, and pausing their music
            // because this room closed would be the bug the release rule
            // exists to avoid, arriving from the other side.
            if bridge.holder(&entity).is_some_and(|u| u == room) {
                println!("harken-server: house: {entity}: nobody is listening as {room} any more; paused");
                bridge.forget(&entity);
                house.call(&entity, "media_pause", None);
            }
        }
    }

    /// Which entities are worth asking about.
    fn holding(bridge: &Bridge) -> Vec<DeviceId> {
        bridge
            .devices()
            .iter()
            .map(|d| d.id.clone())
            .filter(|id| bridge.holds(id))
            .collect()
    }

    /// Say something into a room as the speaker standing in it.
    fn say_as(
        hub: &HubHandle,
        wires: &HashMap<(String, DeviceId), Wire>,
        room: &str,
        entity: &str,
        what: Say,
    ) {
        if let Some(wire) = wires.get(&(room.to_string(), entity.to_string())) {
            let _ = hub.say(wire.conn, what.encode());
        }
    }

    fn report(hub: &HubHandle, wires: &HashMap<(String, DeviceId), Wire>, said: Said) {
        say_as(
            hub,
            wires,
            &said.user,
            &said.device,
            Say::Report {
                queue: said.queue,
                at: said.at,
                playing: said.playing,
                position_ms: said.position_ms,
            },
        );
    }

    fn seek(position_ms: i64) -> serde_json::Value {
        // Home Assistant takes seconds, as a number: the one float here, and
        // it is on the wire to the house, never in anything a replica applies.
        serde_json::json!({ "seek_position": position_ms as f64 / 1000.0 })
    }

    fn perform(
        house: &House,
        hub: &HubHandle,
        wires: &HashMap<(String, DeviceId), Wire>,
        act: Act,
    ) {
        match act {
            Act::Release { user, entity } => {
                // The speaker is not this room's any more — somebody else took
                // it, it is playing something that is not ours, or it has
                // stopped answering. The room is told rather than left drawing
                // a transport for music it is not making, and it is told *by
                // the speaker*, the one thing standing in that room that
                // knows.
                println!("harken-server: house: {entity}: released from {user}'s session");
                say_as(hub, wires, &user, &entity, Say::Transfer { to: None });
            }
            Act::Verb { entity, verb } => {
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
                let at = at as usize;
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
                // `media_position_updated_at`: the output reports about once a
                // second and every client counts from when *it* heard, which
                // is the same extrapolation done once, at the far end.
                position_ms: (attrs
                    .get("media_position")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0)
                    * 1000.0) as i64,
            })
        }
    }
}
