//! The reconciler's rules for a player that is read rather than driven —
//! a speaker, whose adapter says what it heard ([`Heard`], [`Event`]) — held
//! against sessions written by hand: what was `harken-server`'s bridge
//! (`assistant.rs`) before its rules and memory came here. A move of the
//! session told to the player, and a re-anchoring that is not one; where
//! the player went by itself said as `advance` or `jump`, a stall, a resume
//! by hand; a drift followed, a seek by hand said; what it can do said once;
//! its volume and group, both ways; silent once it is not the output, unless
//! it follows the leader the sound went to; and let go, not taken back.
//!
//! Each was falsified once by breaking what it holds in `reconcile.rs`; the
//! comment beside each says how.

use std::collections::VecDeque;

use ark::format::Encode;
use harken_domain::host::{Kind, Listening};
use harken_domain::*;
use harken_player::memory::{Equipped, Handoff, Role};
use harken_player::{Caps, Event, Followers, Heard, Place, Reconciler, Say, Sound, View, Vol};

const ME: &str = "kitchen";
const STUDY: &str = "study";
const NOW: i64 = 1_700_000_000_000;

fn track(n: u8) -> Track {
    let mut id = [0; 16];
    id[15] = n;
    Track {
        album: String::new(),
        creator: "Bach".into(),
        duration_ms: 60_000,
        file: format!("music/{n}.mp3"),
        id: Id::new(id),
        title: format!("{n}"),
    }
}

fn device(id: &str) -> Present {
    Present {
        can_play: true,
        can_volume: false,
        family: None,
        follows: None,
        id: id.into(),
        kind: Kind::Speaker.name().into(),
        muted: false,
        name: id.into(),
        user: "alice".into(),
        volume: harken_domain::host::UNSAID_VOLUME,
    }
}

/// A speaker as the session has it once it has said what it can do.
fn stood(id: &str, volume: i64, follows: Option<&str>) -> Present {
    Present {
        can_volume: true,
        volume,
        family: Some("sonos".into()),
        follows: follows.map(String::from),
        ..device(id)
    }
}

fn session(n: usize, at: i64, clock: Clock, output: &str, moving: bool) -> Listening {
    Listening {
        devices: vec![device("phone"), device(ME)],
        scrub: Default::default(),
        session: Some(Active {
            at,
            clock,
            from: String::new(),
            output: Output::Device { id: output.into(), moving },
            queue: (0..n as u8).map(track).collect(),
        }),
    }
}

fn playing(at_ms: i64, since: i64) -> Clock {
    Clock::Playing { at_ms, since }
}

/// A player that does what it is told and says what a test says it heard.
#[derive(Default)]
struct Fake {
    queue: Vec<Track>,
    at: Option<usize>,
    playing: bool,
    ready: bool,
    place: Option<Place>,
    events: VecDeque<Event>,
    did: Vec<String>,
    caps: Option<Caps>,
    volume: Option<Vol>,
    followers: Option<Followers>,
}

impl Fake {
    fn take_did(&mut self) -> Vec<String> {
        std::mem::take(&mut self.did)
    }
}

impl Sound for Fake {
    fn heard(&self) -> Heard {
        Heard {
            queue: self.queue.iter().map(|t| t.id).collect(),
            at: self.at,
            playing: self.playing,
            ready: self.ready,
            place: self.place.unwrap_or(Place::Unknown),
            ended: false,
            from: String::new(),
        }
    }

    fn take(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    fn tracks(&self) -> Vec<Track> {
        self.queue.clone()
    }

    fn load(&mut self, queue: &[Track], at: usize, position_ms: i64, playing: bool, _: &str) {
        (self.queue, self.at, self.playing, self.ready) = (queue.to_vec(), Some(at), playing, false);
        self.did.push(format!("load {at} {position_ms} {playing}"));
    }

    fn jump(&mut self, at: usize, position_ms: i64) {
        self.at = Some(at);
        self.did.push(format!("jump {at} {position_ms}"));
    }

    fn resume(&mut self) {
        self.playing = true;
        self.did.push("resume".into());
    }

    fn pause(&mut self) {
        self.playing = false;
        self.did.push("pause".into());
    }

    fn seek(&mut self, ms: i64) {
        self.place = Some(Place::Believed(ms));
        self.did.push(format!("seek {ms}"));
    }

    fn let_go(&mut self, follows: bool) {
        (self.queue, self.at, self.playing) = (vec![], None, false);
        self.did.push(format!("let go{}", if follows { ", following" } else { "" }));
    }

    fn caps(&self) -> Option<Caps> {
        self.caps.clone()
    }

    fn volume(&self) -> Option<Vol> {
        self.volume
    }

    fn set_volume(&mut self, v: Vol) {
        self.volume = Some(v);
        self.did.push(format!("volume {} {}", v.level, v.muted));
    }

    fn followers(&self) -> Option<Followers> {
        self.followers.clone()
    }

    fn join(&mut self, members: &[String]) {
        self.did.push(format!("join {}", members.join(" ")));
    }

    fn unjoin(&mut self, member: &str) {
        self.did.push(format!("unjoin {member}"));
    }
}

/// What the reconciler said, as transitions by name and input.
#[derive(Default)]
struct Said(Vec<(&'static str, Vec<u8>, Option<i64>)>);

impl Say for Said {
    fn call(&mut self, transition: &'static str, input: Vec<u8>, as_of: Option<i64>, _: &str) -> Result<(), String> {
        self.0.push((transition, input, as_of));
        Ok(())
    }

    fn play(&mut self, _: RecordPlay) -> Result<(), String> {
        Ok(())
    }
}

struct Rig {
    r: Reconciler,
    sound: Fake,
}

impl Rig {
    fn new() -> Rig {
        Rig {
            r: Reconciler::new(ME, None),
            sound: Fake::default(),
        }
    }

    /// One look at `s` at `now`: what was said, by transition.
    fn look(&mut self, s: &Listening, now: i64) -> Vec<(&'static str, Vec<u8>, Option<i64>)> {
        let mut said = Said::default();
        let v = View {
            me: ME,
            joined: true,
            epoch: 1,
            listening: s,
            now,
        };
        self.r.reconcile(&v, &mut self.sound, &mut said);
        said.0
    }

    fn names(&mut self, s: &Listening, now: i64) -> Vec<&'static str> {
        self.look(s, now).into_iter().map(|c| c.0).collect()
    }

    /// Handed `n` tracks from `at`, and landed there at 0, playing.
    fn started(&mut self, n: usize, at: i64) {
        let handed = session(n, at, playing(0, NOW), ME, true);
        assert_eq!(self.names(&handed, NOW), Vec::<&str>::new());
        assert_eq!(self.sound.take_did(), [format!("load {at} 0 true")]);
        (self.sound.ready, self.sound.place) = (true, Some(Place::Measured { ms: 0, at: NOW }));
        assert_eq!(self.names(&handed, NOW), ["started"]);
        self.sound.place = Some(Place::Believed(0));
        let anchored = session(n, at, playing(0, NOW), ME, false);
        assert_eq!(self.names(&anchored, NOW), Vec::<&str>::new());
        assert_eq!(self.sound.take_did(), Vec::<String>::new());
    }
}

/// A session that moved is told to the player: a pause, a play, a seek,
/// another place in the queue. A clock re-anchored where the player already
/// is — a slow speaker's `started` five seconds on — moves nothing.
///
/// Falsified by a seek whenever the session moved, wherever the player
/// already is: the slow speaker's re-anchoring was sent back to it as a
/// seek.
#[test]
fn a_session_that_moved_is_told_to_the_player() {
    let mut rig = Rig::new();
    let handed = session(2, 0, playing(0, NOW), ME, true);
    rig.look(&handed, NOW);
    rig.sound.take_did();
    (rig.sound.ready, rig.sound.place) = (true, Some(Place::Measured { ms: 100, at: NOW + 5_000 }));
    let said = rig.look(&handed, NOW + 5_000);
    assert_eq!(said, [("started", At { at_ms: 100 }.to_bytes(), Some(NOW + 5_000))]);
    rig.sound.place = Some(Place::Believed(100));
    let slow = session(2, 0, playing(100, NOW + 5_000), ME, false);
    assert!(rig.look(&slow, NOW + 5_000).is_empty());
    assert_eq!(rig.sound.take_did(), Vec::<String>::new(), "the re-anchoring is not a seek");

    let mut rig = Rig::new();
    rig.started(2, 0);
    rig.sound.place = Some(Place::Believed(5_000));
    rig.look(&session(2, 0, Clock::Paused { at_ms: 5_000 }, ME, false), NOW + 5_000);
    assert_eq!(rig.sound.take_did(), ["pause"]);
    rig.look(&session(2, 0, Clock::Paused { at_ms: 40_000 }, ME, false), NOW + 6_000);
    assert_eq!(rig.sound.take_did(), ["seek 40000"]);
    rig.look(&session(2, 0, playing(40_000, NOW + 7_000), ME, false), NOW + 7_000);
    assert_eq!(rig.sound.take_did(), ["resume"]);
    rig.look(&session(2, 1, playing(0, NOW + 8_000), ME, false), NOW + 8_000);
    assert_eq!(rig.sound.take_did(), ["jump 1 0"]);
}

/// Where the player went by itself is where it is: the next track is
/// `advance`, said once while it lands, anywhere else `jump`; a stop by
/// itself is `stalled`, at its point, and not fought; a resume by hand
/// `play`, and not paused back.
///
/// Falsified by reporting every move as `jump` (the `i == at + 1` test
/// taken out): the end of a track came out a jump.
#[test]
fn where_the_player_went_by_itself_is_said() {
    let mut rig = Rig::new();
    rig.started(3, 0);
    rig.sound.at = Some(1);
    let s = session(3, 0, playing(0, NOW), ME, false);
    let said = rig.look(&s, NOW + 1_000);
    assert_eq!(said, [("advance", Advance { from_track: 0 }.to_bytes(), None)]);
    assert!(rig.look(&s, NOW + 1_100).is_empty(), "said once, while it lands");
    assert_eq!(rig.sound.take_did(), Vec::<String>::new(), "and not fought");
    let s = session(3, 1, playing(0, NOW + 1_200), ME, false);
    assert!(rig.look(&s, NOW + 1_200).is_empty());
    rig.sound.at = Some(0);
    assert_eq!(rig.look(&s, NOW + 2_000), [("jump", Jump { index: 0 }.to_bytes(), None)]);
    let s = session(3, 0, playing(0, NOW + 2_000), ME, false);
    assert!(rig.look(&s, NOW + 2_000).is_empty());

    rig.sound.playing = false;
    rig.sound.events.push_back(Event::Stalled { at_ms: 4_200 });
    assert_eq!(rig.look(&s, NOW + 6_000), [("stalled", At { at_ms: 4_200 }.to_bytes(), None)]);
    assert!(rig.look(&s, NOW + 6_100).is_empty());
    assert_eq!(rig.sound.take_did(), Vec::<String>::new(), "not resumed while the session hears of it");
    let paused = session(3, 0, Clock::Paused { at_ms: 4_200 }, ME, false);
    rig.sound.place = Some(Place::Believed(4_200));
    assert!(rig.look(&paused, NOW + 6_200).is_empty());

    rig.sound.playing = true;
    rig.sound.events.push_back(Event::Resumed);
    assert_eq!(rig.names(&paused, NOW + 7_000), ["play"]);
    assert!(rig.look(&paused, NOW + 7_100).is_empty());
    assert_eq!(
        rig.sound.take_did(),
        Vec::<String>::new(),
        "not paused back while the session hears of it"
    );
}

/// A player that has come apart from a clock nobody touched by more than
/// the bound is followed: the session is told where it is
/// (`report_position`, against the clock it was measured against, as of
/// when), once, and the player is told nothing, then or when the session
/// re-anchors there. Within the bound it is left alone; past three seconds
/// it was moved by hand, and that is its `seek`.
///
/// Falsified by a drift said whatever the bound (`SYNC_MS` read as 0): a
/// player 400 ms off reported.
#[test]
fn a_player_drifting_from_the_session_is_followed_and_told_nothing() {
    let mut rig = Rig::new();
    rig.started(1, 0);
    let s = session(1, 0, playing(0, NOW), ME, false);
    rig.sound.place = Some(Place::Measured {
        ms: 11_000,
        at: NOW + 10_000,
    });
    let said = rig.look(&s, NOW + 10_000);
    assert_eq!(
        said,
        [("report_position", Report { at_ms: 11_000, since: NOW }.to_bytes(), Some(NOW + 10_000))]
    );
    assert!(rig.look(&s, NOW + 10_500).is_empty(), "and it is not said twice");
    let followed = session(1, 0, playing(11_000, NOW + 10_000), ME, false);
    assert!(rig.look(&followed, NOW + 10_050).is_empty());
    assert_eq!(rig.sound.take_did(), Vec::<String>::new(), "nothing told to the player");
    rig.sound.place = Some(Place::Measured {
        ms: 13_400,
        at: NOW + 12_000,
    });
    assert!(rig.look(&followed, NOW + 12_000).is_empty(), "within the bound, nothing");
    rig.sound.place = Some(Place::Measured {
        ms: 60_000,
        at: NOW + 13_000,
    });
    assert_eq!(
        rig.look(&followed, NOW + 13_000),
        [("seek", Seek { ms: 60_000 }.to_bytes(), Some(NOW + 13_000))]
    );
}

/// What a player can do is read off it and said once, as its `equip`,
/// carrying the volume it reports — and nothing else is said or told of it
/// until the session has it.
///
/// Falsified by forgetting what was equipped (the `Equipped::Said` test
/// taken out): the equip was said again on every look until it landed.
#[test]
fn what_a_player_can_do_is_said_once_as_its_equip() {
    let mut rig = Rig::new();
    let mut fresh = session(1, 0, playing(0, NOW), "phone", false);
    assert!(rig.look(&fresh, NOW).is_empty(), "nothing read yet, nothing said");
    rig.sound.caps = Some(Caps {
        can_volume: true,
        family: Some("sonos".into()),
    });
    rig.sound.volume = Some(Vol { level: 35, muted: false });
    let said = rig.look(&fresh, NOW);
    let equip = Equip {
        can_volume: true,
        family: Some("sonos".into()),
        muted: false,
        volume: 35,
    };
    assert_eq!(said, [("equip", equip.to_bytes(), None)]);
    assert!(rig.look(&fresh, NOW).is_empty(), "said once, while it lands");
    assert!(
        matches!(
            rig.r.mind().equipped,
            Equipped::Said {
                epoch: 1,
                can_volume: true,
                ..
            }
        ),
        "a row says so"
    );
    fresh.devices[1] = stood(ME, 35, None);
    assert!(rig.look(&fresh, NOW).is_empty(), "and then the two agree");
    assert_eq!(rig.sound.take_did(), Vec::<String>::new());
}

/// A volume that moved in the session is told to the player; one that
/// moved on the player while the session stood still was somebody's hand,
/// said; the first look takes the player's.
///
/// Falsified by never reading the player after the session's write (the
/// `Settled` arm taken out): the knob turned to 22 was not said.
#[test]
fn a_volume_is_told_and_a_hand_on_the_knob_is_said() {
    let mut rig = Rig::new();
    rig.sound.caps = Some(Caps {
        can_volume: true,
        family: Some("sonos".into()),
    });
    rig.sound.volume = Some(Vol { level: 30, muted: false });
    let with = |v: i64, muted: bool| {
        let mut s = session(1, 0, playing(0, NOW), "phone", false);
        s.devices[1] = Present { muted, ..stood(ME, v, None) };
        s
    };
    assert_eq!(rig.names(&with(35, false), NOW), ["report_volume"], "the first look takes the player's");
    assert!(rig.look(&with(30, false), NOW).is_empty());
    assert!(rig.look(&with(60, false), NOW).is_empty());
    assert_eq!(rig.sound.take_did(), ["volume 60 false"]);
    rig.sound.volume = Some(Vol { level: 22, muted: true });
    let said = rig.look(&with(60, false), NOW);
    assert_eq!(said, [("report_volume", ReportVolume { level: 22, muted: true }.to_bytes(), None)]);
    assert!(rig.look(&with(22, true), NOW).is_empty());
    assert_eq!(rig.sound.take_did(), Vec::<String>::new(), "a hand is not fought");
}

/// The group is reconciled from its leader's side: a follower added in the
/// session is a `join` on the leader, one dropped an `unjoin`; a group made
/// by hand is said as the leader's; the first look takes the group the
/// house has rather than breaking it up.
///
/// Falsified by treating the first look as the session having moved
/// (`Grouped::Untold` read as the session's): standing the kitchen in a
/// session unjoined the study the Sonos app had grouped with it.
#[test]
fn a_group_is_told_and_a_group_made_by_hand_is_said() {
    let mut rig = Rig::new();
    rig.sound.caps = Some(Caps {
        can_volume: true,
        family: Some("sonos".into()),
    });
    rig.sound.volume = Some(Vol { level: 30, muted: false });
    rig.sound.followers = Some(Followers {
        leads: true,
        members: vec![STUDY.into()],
    });
    let with = |follows: Option<&str>| {
        let mut s = session(1, 0, playing(0, NOW), ME, false);
        s.devices = vec![device("phone"), stood(ME, 30, None), stood(STUDY, 30, follows)];
        s
    };
    let said = rig.look(&with(None), NOW);
    let report: Vec<_> = said.iter().filter(|c| c.0 == "report_group").collect();
    assert_eq!(report.len(), 1, "{said:?}");
    assert_eq!(report[0].1, ReportGroup { members: vec![STUDY.into()] }.to_bytes());
    rig.sound.take_did();
    rig.look(&with(Some(ME)), NOW);
    assert!(rig.sound.take_did().iter().all(|d| !d.contains("join")));
    rig.look(&with(None), NOW);
    assert!(
        rig.sound.take_did().contains(&format!("unjoin {STUDY}")),
        "unjoined in the session: an unjoin"
    );
    rig.sound.followers = Some(Followers {
        leads: true,
        members: vec![],
    });
    rig.look(&with(None), NOW);
    rig.look(&with(Some(ME)), NOW);
    assert!(rig.sound.take_did().contains(&format!("join {STUDY}")), "joined in the session: a join");
}

/// The sound handed to the member that follows nobody now — the study,
/// which leads the kitchen — is no pause for the kitchen: it plays on in
/// the study's group. Handed to a device it does not follow, it is let go,
/// once.
///
/// Falsified by a `let_go` that never asked whether the device follows the
/// new leader: the kitchen was paused as the study took the sound.
#[test]
fn a_player_let_go_is_silent_once_unless_it_follows_the_new_leader() {
    let mut rig = Rig::new();
    rig.started(1, 0);
    let mut moved = session(1, 0, playing(0, NOW), STUDY, true);
    moved.devices = vec![device("phone"), stood(ME, 30, Some(STUDY)), stood(STUDY, 30, None)];
    rig.look(&moved, NOW);
    assert_eq!(rig.sound.take_did(), ["let go, following"]);

    let mut rig = Rig::new();
    rig.started(1, 0);
    let phone = session(1, 0, playing(0, NOW), "phone", true);
    rig.look(&phone, NOW);
    assert_eq!(rig.sound.take_did(), ["let go"]);
    rig.look(&phone, NOW);
    assert_eq!(rig.sound.take_did(), Vec::<String>::new(), "once");
    assert_eq!(rig.r.mind().role, Role::Silent);
}

/// A player that is not ours any more lets the session go — `release`,
/// said once — and is not taken back while the session still names it; a
/// new hand-off takes it.
///
/// Falsified by forgetting the release (`Role::Released` never written):
/// the next look handed the speaker the queue again.
#[test]
fn a_player_lost_releases_the_session_and_is_not_taken_back() {
    let mut rig = Rig::new();
    rig.started(1, 0);
    let s = session(1, 0, playing(0, NOW), ME, false);
    (rig.sound.queue, rig.sound.at) = (vec![], None);
    rig.sound.events.push_back(Event::Lost { why: "radio".into() });
    assert_eq!(rig.names(&s, NOW), ["release"]);
    assert!(rig.look(&s, NOW).is_empty());
    assert_eq!(rig.sound.take_did(), Vec::<String>::new(), "not taken back");
    rig.look(&session(1, 0, Clock::Paused { at_ms: 0 }, ME, true), NOW);
    assert_eq!(rig.sound.take_did(), ["load 0 0 false"], "handed again");
    assert_eq!(rig.r.mind().handoff, Handoff::None);
}
