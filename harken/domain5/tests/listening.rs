//! The listening session's rules, as transitions: `harken/domain/tests/
//! listening.rs`, case for case, the transitions run by the interpreter
//! over a store holding the session's volatile tables (§1.9) and their
//! changes applied — what the volatile machine will do with each entry.
//!
//! A device arriving is `on_join` authored for it, and its connection
//! going `on_leave`, as the authority will author them. What v4's desk
//! did besides — relaying an ephemeral call, telling every device what
//! was applied, counting a device's connections, keeping the ordered
//! state across a restart — is the volatile machine's and its driver's
//! (D1's), not the domain's; those assertions are split into tests below
//! that are `#[ignore]`d and say what they wait for.
//!
//! Each test was falsified once by breaking the rule it holds; its doc
//! comment says how, and what failed.

mod common;

use std::collections::BTreeMap;

use ark5::author::{children, Id, Node};
use ark5::ir::Expr;
use ark5::log::EntryId;
use ark5::peer::Ctx;
use ark5::store::Store;
use common::domain;
use harken_domain5::listening::read::Listening;
use harken_domain5::listening::*;

/// The server's clock at the start of every test.
const T0: i64 = 1_700_000_000_000;

/// The session's tables, and the devices standing in them.
struct House {
    store: Store,
    devices: BTreeMap<String, Device>,
    local: i64,
    now: i64,
}

impl House {
    fn new() -> House {
        House {
            store: Store::new(domain().schema.clone()),
            devices: BTreeMap::new(),
            local: 0,
            now: T0,
        }
    }

    fn wait(&mut self, ms: i64) {
        self.now += ms;
    }

    /// An entry of `name`, authored as `user` on `session`.
    fn author(&mut self, user: &str, session: &str, name: &str, input: &impl Node) -> Result<(), String> {
        self.local += 1;
        let ctx = Ctx {
            id: EntryId::of(session, self.local),
            actor: user.into(),
            session: session.into(),
            roles: vec![],
            now: self.now,
        };
        let changes = domain()
            .run(&self.store, &ctx, &format!("listening.{name}"), &input.to_bytes())
            .map_err(|r| r.0)?;
        self.store.apply_all(&changes).unwrap_or_else(|r| panic!("{name}: the store refused {r}"));
        Ok(())
    }

    /// A device of `user` connects, saying what it is: `on_join`.
    fn arrive_as(&mut self, user: &str, device: &str, kind: &str, can_play: bool) {
        let d = Device {
            can_play,
            id: device.into(),
            kind: kind.into(),
            name: device.into(),
            user: user.into(),
        };
        self.author("authority", "authority", "on_join", &d).unwrap();
        self.devices.insert(device.into(), d);
    }

    fn arrive(&mut self, device: &str, can_play: bool) {
        self.arrive_as("alice", device, "computer", can_play)
    }

    /// Its connection goes: `on_leave`. Not a decision about the music.
    fn leave(&mut self, device: &str) {
        let d = self.devices.remove(device).expect("standing");
        self.author("authority", "authority", "on_leave", &d).unwrap();
    }

    /// Call as `device`: nothing, or the refusal.
    fn call(&mut self, device: &str, transition: &str, input: &impl Node) -> Result<(), String> {
        let user = self.devices.get(device).map_or("alice".to_string(), |d| d.user.clone());
        self.author(&user, device, transition, input)
    }

    fn ok(&mut self, device: &str, transition: &str, input: &impl Node) {
        self.call(device, transition, input)
            .unwrap_or_else(|why| panic!("{device} {transition}: refused: {why}"))
    }

    fn state_of(&self, user: &str) -> Listening {
        Listening::of(&self.store, user)
    }

    fn state(&self) -> Listening {
        self.state_of("alice")
    }

    /// A speaker of the house: here, able to play, a volume anybody can
    /// move, and of `family`.
    fn speaker(&mut self, id: &str, family: &str, volume: i64) {
        self.arrive_as("alice", id, "speaker", true);
        self.ok(
            id,
            "equip",
            &Equip {
                can_volume: true,
                family: Some(Family(family.into())),
                muted: false,
                volume,
            },
        );
    }

    /// Something playing on `speaker`, handed there from the phone and
    /// started.
    fn playing_in(&mut self, speaker: &str) {
        self.playing_on("phone", 0);
        self.ok("phone", "transfer", &to(speaker));
        self.ok(speaker, "started", &at_ms(0));
    }

    fn volume(&self, id: &str) -> (i64, bool) {
        let s = self.state();
        let d = s.device(id).unwrap_or_else(|| panic!("{id} is not here"));
        (d.volume, d.muted)
    }

    fn follows(&self, id: &str) -> Option<String> {
        self.state().device(id).and_then(|d| d.follows.clone())
    }

    /// Something playing on `device`, pressed there.
    fn playing_on(&mut self, device: &str, position_ms: i64) {
        self.ok(device, "start", &start(&queue(), 0, position_ms, "album/Brandenburg"));
    }
}

fn track(title: &str, n: u8) -> Track {
    let mut id = [0; 16];
    id[15] = n;
    Track {
        id: Id::new(id),
        title: title.into(),
        creator: "Bach".into(),
        album: "Brandenburg".into(),
        duration_ms: 300_000,
        file: format!("music/{title}.mp3"),
    }
}

fn queue() -> Vec<Track> {
    vec![track("a", 1), track("b", 2)]
}

fn start(queue: &[Track], at: i64, position: i64, from: &str) -> Start {
    Start {
        at,
        from: from.into(),
        position,
        queue: Queue(queue.to_vec()),
    }
}

fn to(device: &str) -> Transfer {
    Transfer { to: device.into() }
}

fn at_ms(at_ms: i64) -> At {
    At { at_ms }
}

fn member(device: &str) -> Member {
    Member { device: device.into() }
}

fn output(l: &Listening) -> Output {
    l.session.as_ref().expect("a session").output.clone()
}

fn clock(l: &Listening) -> Clock {
    l.session.as_ref().expect("a session").clock
}

fn titles(l: &Listening) -> Vec<String> {
    l.session.as_ref().expect("a session").queue.iter().map(|t| t.title.clone()).collect()
}

fn at(l: &Listening) -> i64 {
    l.session.as_ref().expect("a session").at
}

fn here(id: &str, moving: bool) -> Output {
    Output::Device { id: id.into(), moving }
}

/// A laptop is playing; its connection goes; the sound is still the
/// laptop's — away, paused where it had got to, and still named.
/// Falsified by an `on_leave` that dropped the output with the device: the
/// output read `Nowhere`.
#[test]
fn the_sound_stays_with_a_device_whose_socket_went() {
    let mut h = House::new();
    h.arrive("laptop", true);
    h.arrive("phone", true);
    h.playing_on("laptop", 40_000);
    h.wait(2_000);
    h.leave("laptop");
    let s = h.state();
    assert_eq!(
        output(&s),
        Output::Away {
            id: "laptop".into(),
            name: "laptop".into()
        },
        "the sound belongs to the laptop"
    );
    assert_eq!(
        clock(&s),
        Clock::Paused { at_ms: 42_000 },
        "and it cannot be playing, there is nobody there"
    );
    assert!(s.device("laptop").is_none(), "gone from who is here");
    assert!(s.device("phone").is_some());
}

/// …and when the laptop comes back it has the sound again, still paused.
/// Falsified by an `on_join` that did not give an away output back: the
/// session still said away.
#[test]
fn the_output_coming_back_has_it_again_paused() {
    let mut h = House::new();
    h.arrive("laptop", true);
    h.playing_on("laptop", 10_000);
    h.leave("laptop");
    h.arrive("laptop", true);
    let s = h.state();
    assert_eq!(output(&s), here("laptop", false));
    assert_eq!(clock(&s), Clock::Paused { at_ms: 10_000 });
}

/// A device that merely turns up takes nothing. Falsified by an `on_join`
/// that handed an away output to whoever joined: the tablet came out the
/// output.
#[test]
fn arriving_is_not_how_a_device_gets_the_sound() {
    let mut h = House::new();
    h.arrive("laptop", true);
    h.playing_on("laptop", 10_000);
    h.arrive("phone", true);
    h.leave("laptop");
    h.arrive("tablet", true);
    assert_eq!(output(&h.state()).id(), Some("laptop"));
    assert!(h.state().device("tablet").is_some(), "the tablet is here");
}

/// A command is a change to the session, which the output obeys: pausing
/// on the phone pauses the laptop's session and leaves the laptop the
/// output. Falsified by a `pause` that handed the sound to the caller: the
/// phone came out the output.
#[test]
fn a_command_is_a_state_change_the_output_obeys() {
    let mut h = House::new();
    h.arrive("laptop", true);
    h.arrive("phone", true);
    h.playing_on("laptop", 0);
    h.wait(5_000);
    h.ok("phone", "pause", &());
    let s = h.state();
    assert_eq!(output(&s).id(), Some("laptop"), "asking did not move it");
    assert_eq!(clock(&s), Clock::Paused { at_ms: 5_000 });
    h.ok("phone", "seek", &Seek { ms: 90_000 });
    assert_eq!(clock(&h.state()), Clock::Paused { at_ms: 90_000 }, "a paused seek stays paused");
    h.ok("phone", "play", &());
    let s = h.state();
    assert_eq!(output(&s).id(), Some("laptop"), "play resumes on the output that is here");
    assert_eq!(clock(&s), Clock::Playing { at_ms: 90_000, since: h.now });
    h.wait(1_000);
    h.ok("phone", "seek", &Seek { ms: 5_000 });
    assert_eq!(
        clock(&h.state()),
        Clock::Playing { at_ms: 5_000, since: h.now },
        "a playing seek plays on from there"
    );
}

/// v4's "the laptop hears the pause": what a device is told when a
/// transition is applied. Waits for D1's volatile machine and its driver,
/// which fan an applied entry out to the instance's connections.
#[test]
#[ignore = "waits for D1: the volatile machine's fan-out of an applied transition"]
fn the_output_hears_a_command() {
    unimplemented!("D1: Output::Fanout of a volatile batch to the laptop's connection")
}

/// …unless the output cannot be reached and the asker can make a sound —
/// and only for a press that means a sound. Falsified by a `seek` that took
/// the sound like `skip`: the phone came out the output.
#[test]
fn only_a_press_that_means_a_sound_takes_it_from_a_device_that_has_gone() {
    let mut h = House::new();
    h.arrive("laptop", true);
    h.arrive("phone", true);
    h.playing_on("laptop", 77_000);
    h.leave("laptop");
    h.ok("phone", "pause", &());
    h.ok("phone", "seek", &Seek { ms: 5 });
    assert_eq!(
        output(&h.state()).id(),
        Some("laptop"),
        "nothing to pause is not a reason to take the music"
    );
    h.ok("phone", "skip", &Skip { by: 1 });
    let s = h.state();
    assert_eq!(
        output(&s),
        here("phone", false),
        "a press that makes a sound, and nothing else can answer"
    );
    let a = s.session.as_ref().unwrap();
    assert_eq!(a.at, 1, "and the press it made");
    assert_eq!(a.clock, Clock::Playing { at_ms: 0, since: h.now });
    assert_eq!(a.queue, queue(), "the queue the laptop had");

    let mut h = House::new();
    h.arrive("laptop", true);
    h.arrive("phone", true);
    h.playing_on("laptop", 30_000);
    h.leave("laptop");
    h.ok("phone", "play", &());
    let s = h.state();
    assert_eq!(output(&s).id(), Some("phone"));
    assert_eq!(clock(&s), Clock::Playing { at_ms: 30_000, since: h.now }, "from where the laptop left it");
}

/// A device that cannot make a sound never takes it. Falsified by dropping
/// `play`'s "nothing here can play it" refusal: the desktop came out the
/// output.
#[test]
fn a_device_that_cannot_be_heard_never_takes_the_sound() {
    let mut h = House::new();
    h.arrive("laptop", true);
    h.arrive("desktop", false);
    h.playing_on("laptop", 0);
    h.leave("laptop");
    let refused = |h: &mut House, t: &str| match t {
        "start" => h.call("desktop", t, &start(&queue(), 0, 0, "")),
        "skip" => h.call("desktop", t, &Skip { by: 1 }),
        "jump" => h.call("desktop", t, &Jump { index: 1 }),
        _ => h.call("desktop", t, &()),
    };
    for t in ["play", "start", "skip", "jump"] {
        assert_eq!(refused(&mut h, t), Err("nothing here can play it".to_string()), "{t}");
    }
    assert_eq!(output(&h.state()).id(), Some("laptop"));

    let mut h = House::new();
    h.arrive("desktop", false);
    assert_eq!(
        h.call("desktop", "start", &start(&queue(), 0, 0, "")),
        Err("nothing here can play it".to_string())
    );

    let mut h = House::new();
    h.arrive("laptop", true);
    h.arrive("desktop", false);
    h.playing_on("laptop", 0);
    h.ok("laptop", "pause", &());
    h.ok("desktop", "start", &start(&queue(), 1, 0, ""));
    let s = h.state();
    assert_eq!(output(&s).id(), Some("laptop"));
    assert!(s.sounding());
}

/// A track pressed goes where the sound is while one is sounding, and here
/// while nothing is. Falsified by a `start` that kept a reachable output
/// whatever its clock: the phone's track went to the paused laptop.
#[test]
fn a_track_pressed_plays_where_the_sound_is_or_here_when_it_is_nowhere() {
    let mut h = House::new();
    h.arrive("laptop", true);
    h.arrive("phone", true);
    h.playing_on("laptop", 0);
    h.ok("phone", "start", &start(&queue(), 1, 0, "playlist/Favorites"));
    let s = h.state();
    assert_eq!(output(&s).id(), Some("laptop"), "something is sounding, so the press goes there");
    assert_eq!(s.session.as_ref().unwrap().at, 1);
    assert_eq!(s.session.as_ref().unwrap().from, "playlist/Favorites");
    h.ok("phone", "pause", &());
    h.ok("phone", "start", &start(&queue(), 0, 61_000, ""));
    let s = h.state();
    assert_eq!(output(&s).id(), Some("phone"), "nothing is sounding, so the press is a sound here");
    assert_eq!(clock(&s), Clock::Playing { at_ms: 61_000, since: h.now });
}

/// Picking a speaker halfway through a track picks up there, connecting
/// until it says it has started. Falsified by a `started` with no "not the
/// output" refusal: the phone's claim ended the kitchen's hand-off.
#[test]
fn picking_a_speaker_picks_up_where_the_track_was() {
    let mut h = House::new();
    h.arrive_as("alice", "phone", "phone", true);
    h.arrive_as("alice", "media_player.kitchen", "speaker", true);
    h.playing_on("phone", 90_000);
    h.wait(1_500);
    h.ok("phone", "transfer", &to("media_player.kitchen"));
    let s = h.state();
    assert_eq!(output(&s), here("media_player.kitchen", true), "still connecting");
    assert_eq!(clock(&s).position(h.now), 91_500, "where the track was");
    assert!(s.moving_to("media_player.kitchen"));
    assert_eq!(h.call("phone", "started", &at_ms(1)), Err("not the output".to_string()));
    h.wait(2_000);
    h.ok("media_player.kitchen", "started", &at_ms(92_100));
    let s = h.state();
    assert_eq!(output(&s), here("media_player.kitchen", false));
    assert_eq!(clock(&s), Clock::Playing { at_ms: 92_100, since: h.now });
    let before = h.state();
    h.ok("phone", "transfer", &to("media_player.kitchen"));
    assert_eq!(h.state(), before, "handing it to the device that has it, settled, is nothing");
}

/// A hand-off is a pause standing where the track was until the device it
/// went to has started. Falsified by a `standing` that ignored `moving`:
/// the pause seven seconds into the hand-off landed at 98 500.
#[test]
fn a_hand_off_stands_still_where_the_track_was_until_it_has_started() {
    let mut h = House::new();
    h.arrive_as("alice", "phone", "phone", true);
    h.arrive_as("alice", "browser", "computer", true);
    h.arrive_as("alice", "media_player.kitchen", "speaker", true);
    h.playing_on("phone", 90_000);
    h.wait(1_500);
    h.ok("phone", "transfer", &to("media_player.kitchen"));
    assert_eq!(clock(&h.state()), Clock::Playing { at_ms: 91_500, since: h.now }, "stopped where it was");
    h.wait(5_000);
    let s = h.state();
    assert_eq!(
        s.session.as_ref().unwrap().position(h.now),
        91_500,
        "standing there while the speaker fetches"
    );
    assert!(!s.outputs("phone"), "the phone goes quiet now");
    h.wait(2_000);
    h.ok("browser", "pause", &());
    assert_eq!(clock(&h.state()), Clock::Paused { at_ms: 91_500 });
    h.ok("browser", "play", &());
    h.wait(3_000);
    assert_eq!(h.state().session.unwrap().position(h.now), 91_500);
    h.ok("phone", "transfer", &to("browser"));
    h.wait(4_000);
    assert_eq!(h.state().session.unwrap().position(h.now), 91_500);
    h.ok("browser", "started", &at_ms(91_520));
    h.wait(1_000);
    let s = h.state();
    assert_eq!(s.session.as_ref().unwrap().position(h.now), 92_520, "running on from where it started");
    assert!(s.outputs("browser") && !s.moving_to("browser"));
    h.ok("phone", "pause", &());
    h.wait(10_000);
    h.ok("phone", "transfer", &to("phone"));
    assert_eq!(clock(&h.state()), Clock::Paused { at_ms: 92_520 });
    h.ok("browser", "play", &());
    h.ok("phone", "transfer", &to("media_player.kitchen"));
    h.wait(6_000);
    h.leave("media_player.kitchen");
    assert_eq!(clock(&h.state()), Clock::Paused { at_ms: 92_520 });
}

/// The output says where it is and the clock follows it, and nothing else
/// does. Falsified by a `report_position` that ignored `since`: the stale
/// report took the session back to 41 200.
#[test]
fn the_output_says_where_it_is_and_the_clock_follows_it_and_nothing_else() {
    let mut h = House::new();
    h.arrive_as("alice", "phone", "phone", true);
    h.arrive_as("alice", "laptop", "computer", false);
    h.arrive_as("alice", "media_player.kitchen", "speaker", true);
    h.playing_on("phone", 40_000);
    let Clock::Playing { since, .. } = clock(&h.state()) else {
        panic!("not playing")
    };
    h.wait(1_000);
    assert_eq!(
        h.call("laptop", "report_position", &Report { at_ms: 40_400, since }),
        Err("not the output".to_string())
    );
    h.ok("phone", "report_position", &Report { at_ms: 40_400, since });
    assert_eq!(
        clock(&h.state()),
        Clock::Playing { at_ms: 40_400, since: h.now },
        "re-anchored where the phone is"
    );
    h.wait(500);
    assert_eq!(h.state().session.unwrap().position(h.now), 40_900, "and playing on from there");
    let Clock::Playing { since: stale, .. } = clock(&h.state()) else {
        panic!("not playing")
    };
    h.wait(200);
    h.ok("laptop", "seek", &Seek { ms: 120_000 });
    let sought = clock(&h.state());
    h.ok("phone", "report_position", &Report { at_ms: 41_200, since: stale });
    assert_eq!(clock(&h.state()), sought, "a report never undoes a seek");
    h.ok("laptop", "pause", &());
    let paused = clock(&h.state());
    let now = h.now;
    h.ok("phone", "report_position", &Report { at_ms: 3_000, since: now });
    assert_eq!(clock(&h.state()), paused);
    h.ok("laptop", "play", &());
    h.playing_on("phone", 90_000);
    h.wait(1_500);
    h.ok("phone", "transfer", &to("media_player.kitchen"));
    let handed = clock(&h.state());
    let Clock::Playing { since, .. } = handed else {
        panic!("not playing: {handed:?}")
    };
    h.ok("media_player.kitchen", "report_position", &Report { at_ms: 5_000, since });
    assert_eq!(clock(&h.state()), handed, "the hand-off still stands where the track was");
    h.wait(2_000);
    assert_eq!(h.state().session.unwrap().position(h.now), 91_500);
}

/// The queue is edited and the track playing plays on. Falsified by an
/// `enqueue` that put "next" tracks at the end: `[a, b, c, d]` where `[a,
/// c, b, d]` was right.
#[test]
fn the_queue_is_edited_and_the_track_playing_plays_on() {
    let mut h = House::new();
    h.arrive_as("alice", "phone", "phone", true);
    h.arrive("desktop", false);
    h.playing_on("phone", 12_000);
    let playing = clock(&h.state());
    let (c, d) = (track("c", 3), track("d", 4));
    h.ok(
        "desktop",
        "enqueue",
        &Enqueue {
            next: true,
            tracks: Queue(vec![c]),
        },
    );
    h.ok(
        "desktop",
        "enqueue",
        &Enqueue {
            next: false,
            tracks: Queue(vec![d]),
        },
    );
    let s = h.state();
    assert_eq!(titles(&s), ["a", "c", "b", "d"]);
    assert_eq!((at(&s), clock(&s)), (0, playing), "the track playing plays on, untouched");
    h.ok("desktop", "move", &Move { from: 3, to: 1 });
    assert_eq!(titles(&h.state()), ["a", "d", "c", "b"]);
    h.ok("desktop", "move", &Move { from: 0, to: 2 });
    let s = h.state();
    assert_eq!(
        (titles(&s), at(&s), clock(&s)),
        (vec!["d".into(), "c".into(), "a".into(), "b".into()], 2, playing)
    );
    h.ok("desktop", "remove", &Remove { index: 0 });
    let s = h.state();
    assert_eq!((titles(&s), at(&s), clock(&s)), (vec!["c".into(), "a".into(), "b".into()], 1, playing));
    h.ok("desktop", "clear_upcoming", &());
    let s = h.state();
    assert_eq!((titles(&s), at(&s), clock(&s)), (vec!["c".into(), "a".into()], 1, playing));
    assert!(s.outputs("phone"), "on the phone all along");
}

/// Taking the track playing out moves on, or stops at the end. Falsified
/// by a `remove` that kept the clock of the track taken out: the next
/// track played from 12 s.
#[test]
fn taking_the_track_playing_out_moves_on_or_stops_at_the_end() {
    let mut h = House::new();
    h.arrive_as("alice", "phone", "phone", true);
    h.playing_on("phone", 12_000);
    h.ok(
        "phone",
        "enqueue",
        &Enqueue {
            next: false,
            tracks: Queue(vec![track("c", 3)]),
        },
    );
    h.wait(1_000);
    h.ok("phone", "remove", &Remove { index: 0 });
    let s = h.state();
    assert_eq!((titles(&s), at(&s)), (vec!["b".into(), "c".into()], 0));
    assert_eq!(clock(&s), Clock::Playing { at_ms: 0, since: h.now }, "the next one, from its start");
    h.ok("phone", "jump", &Jump { index: 1 });
    h.ok("phone", "remove", &Remove { index: 1 });
    let s = h.state();
    assert_eq!((titles(&s), at(&s), clock(&s)), (vec!["b".into()], 0, Clock::Paused { at_ms: 0 }));
    h.ok("phone", "remove", &Remove { index: 0 });
    assert!(h.state().session.is_none(), "nothing queued");
    assert_eq!(h.call("phone", "remove", &Remove { index: 0 }), Err("nothing is queued".to_string()));
}

/// Into nothing queued, `enqueue` starts it; a place not in the queue is
/// refused. Falsified by an `enqueue` with no refusal: the desktop, which
/// cannot play, started a session on itself.
#[test]
fn enqueueing_into_nothing_starts_it_and_a_place_not_in_the_queue_is_refused() {
    let mut h = House::new();
    h.arrive_as("alice", "phone", "phone", true);
    h.arrive("desktop", false);
    assert_eq!(
        h.call(
            "desktop",
            "enqueue",
            &Enqueue {
                next: false,
                tracks: Queue(queue())
            }
        ),
        Err("nothing here can play it".to_string())
    );
    h.ok(
        "phone",
        "enqueue",
        &Enqueue {
            next: true,
            tracks: Queue(queue()),
        },
    );
    let s = h.state();
    assert_eq!((titles(&s), at(&s)), (vec!["a".into(), "b".into()], 0));
    assert!(s.outputs("phone") && s.sounding());
    assert_eq!(
        h.call("desktop", "remove", &Remove { index: 2 }),
        Err("the track is not in the queue".to_string())
    );
    assert_eq!(
        h.call("desktop", "move", &Move { from: 0, to: 2 }),
        Err("the track is not in the queue".to_string())
    );
    assert_eq!(titles(&h.state()), ["a", "b"]);
}

/// The sound is not handed to a device that has gone, that cannot be
/// heard, or to nothing. Falsified by a `transfer` refusal asking only
/// whether the target was here: the mute desktop came out the output.
#[test]
fn the_sound_is_not_handed_to_a_device_that_has_gone_or_is_mute() {
    let mut h = House::new();
    h.arrive("phone", true);
    h.arrive_as("alice", "media_player.kitchen", "speaker", true);
    h.arrive("desktop", false);
    h.playing_on("phone", 0);
    h.leave("media_player.kitchen");
    for d in ["media_player.kitchen", "desktop", "nobody"] {
        assert_eq!(
            h.call("phone", "transfer", &to(d)),
            Err("that device is not here, or cannot play".to_string()),
            "{d}"
        );
        assert_eq!(output(&h.state()), here("phone", false));
    }
    let mut idle = House::new();
    idle.arrive("phone", true);
    idle.arrive("laptop", true);
    assert_eq!(
        idle.call("phone", "transfer", &to("laptop")),
        Err("nothing is playing to move".to_string())
    );
}

/// A device on two connections is one device: its old connection closing
/// does not take the sound. Waits for D1: counting a device's connections
/// and authoring `on_leave` on its last is the driver's.
#[test]
#[ignore = "waits for D1: on_leave authored on a device's last connection, not every one"]
fn a_device_on_a_second_connection_is_still_here_when_the_first_goes() {
    unimplemented!("D1: the hub's per-device connection count")
}

/// What the output says of itself, and nobody else may. Falsified by an
/// `advance` that did not compare `from_track`: the stale one moved the
/// session on a second time.
#[test]
fn the_output_says_what_happens_to_it_and_nobody_else_may() {
    let mut h = House::new();
    h.arrive("laptop", true);
    h.arrive("phone", true);
    h.playing_on("laptop", 0);
    assert_eq!(h.call("phone", "advance", &Advance { from_track: 0 }), Err("not the output".to_string()));
    assert_eq!(h.call("phone", "stalled", &at_ms(1)), Err("not the output".to_string()));
    assert_eq!(h.call("phone", "release", &()), Err("not the output".to_string()));
    h.wait(300_000);
    h.ok("laptop", "advance", &Advance { from_track: 0 });
    let a = h.state().session.unwrap();
    assert_eq!((a.at, a.clock), (1, Clock::Playing { at_ms: 0, since: h.now }));
    let before = h.state();
    h.ok("laptop", "advance", &Advance { from_track: 0 });
    assert_eq!(h.state(), before, "a stale advance is nothing");
    h.ok("laptop", "stalled", &at_ms(12_000));
    assert_eq!(clock(&h.state()), Clock::Paused { at_ms: 12_000 });
    h.ok("laptop", "play", &());
    h.ok("laptop", "advance", &Advance { from_track: 1 });
    let a = h.state().session.unwrap();
    assert_eq!((a.at, a.clock), (1, Clock::Paused { at_ms: 0 }), "the end of the queue");
    assert_eq!(h.call("laptop", "skip", &Skip { by: 1 }), Err("nothing comes after this".to_string()));
    h.ok("laptop", "skip", &Skip { by: -1 });
    assert_eq!(h.state().session.unwrap().at, 0);
    h.ok("laptop", "skip", &Skip { by: -1 });
    assert_eq!(h.state().session.unwrap().at, 0, "before the first is the first again");
    h.ok("laptop", "release", &());
    let a = h.state().session.unwrap();
    assert_eq!(a.output, Output::Nowhere);
    assert!(!a.clock.playing());
    assert_eq!(a.queue, queue(), "the queue stays");
}

/// A restart: the queue as it was, paused where it had got to — never past
/// the end of the track — with no output and nobody here. Which tables a
/// restart keeps is D1's to decide; here the ordered two are carried over
/// by hand, the scrubbers not, and `on_restore` is authored over them.
/// Falsified by an `on_restore` that kept the devices: the restored session
/// still listed the laptop and the phone.
#[test]
fn a_restart_restores_the_session_paused_with_no_output_and_nobody_here() {
    let restart = |h: &House, later: i64| {
        let kept: Vec<(String, Vec<Vec<u8>>)> = ["listening", "listening_device"]
            .iter()
            .map(|t| (t.to_string(), h.store.rows(t).map(|r| r.to_vec()).collect()))
            .collect();
        let mut back = House::new();
        back.store = Store::from_rows(domain().schema.clone(), &kept).unwrap();
        back.now = later;
        back.author("authority", "authority", "on_restore", &()).unwrap();
        back
    };
    let mut h = House::new();
    h.arrive("laptop", true);
    h.arrive("phone", true);
    h.playing_on("laptop", 33_000);
    h.ok("phone", "scrub", &Scrub { at_ms: Some(Ms(50_000)) });
    assert_eq!(h.state().scrub.get("phone"), Some(&50_000));
    h.wait(4_000);
    let back = restart(&h, h.now + 60_000);
    let s = back.state();
    assert!(s.devices.is_empty(), "nobody is here");
    assert!(s.scrub.is_empty());
    let a = s.session.unwrap();
    assert_eq!(a.output, Output::Nowhere);
    assert_eq!(a.clock, Clock::Paused { at_ms: 97_000 }, "where it had got to, the minute away and all");
    assert_eq!(a.queue, queue());

    let mut h = House::new();
    h.arrive("laptop", true);
    h.playing_on("laptop", 0);
    let back = restart(&h, h.now + 3_600_000);
    assert_eq!(clock(&back.state()), Clock::Paused { at_ms: 300_000 }, "never past the end of the track");
}

/// Stopping is the session gone. Falsified by a `stop` that only paused:
/// the session was still active.
#[test]
fn stopping_leaves_nothing() {
    let mut h = House::new();
    h.arrive("laptop", true);
    h.playing_on("laptop", 5_000);
    h.ok("laptop", "stop", &());
    let s = h.state();
    assert!(s.session.is_none());
    assert!(!s.sounding());
    assert_eq!(h.call("laptop", "play", &()), Err("nothing is queued".to_string()));
}

/// One session per account. Falsified by `start` keying the session by
/// `ctx.session`: alice had no session at all, the login its own.
#[test]
fn two_accounts_are_two_sessions() {
    let mut h = House::new();
    h.arrive("laptop", true);
    h.arrive("phone", true);
    h.playing_on("laptop", 0);
    h.arrive_as("bob", "bob-phone", "phone", true);
    let bob = h.state_of("bob");
    assert!(bob.session.is_none());
    assert_eq!(bob.devices.len(), 1);
    assert_eq!(h.state().devices.len(), 2, "alice's laptop and phone, in one");
    assert_eq!(h.state_of("alice").session.map(|a| a.queue.len()), Some(2));
    assert_eq!(h.store.rows("listening").count(), 1, "one session row: alice's");
}

/// A call this module cannot read is refused and changes nothing: no
/// function of that name, or an input of the wrong shape. Falsified by
/// `skip`'s `by` without its refinements: a skip of 0 was taken.
#[test]
fn a_call_nobody_can_read_changes_nothing() {
    let mut h = House::new();
    h.arrive("laptop", true);
    h.playing_on("laptop", 0);
    let before = h.state();
    assert!(domain().module.get("listening.rewind").is_none(), "no transition rewind");
    assert_eq!(
        h.call("laptop", "skip", &Skip { by: 0 }),
        Err("a skip is one track either way".to_string())
    );
    assert!(h.call("laptop", "seek", &()).is_err(), "an input of the wrong shape");
    assert!(h.call("laptop", "start", &start(&[], 0, 0, "")).is_err(), "an empty queue is not one");
    assert_eq!(
        h.call("laptop", "start", &start(&queue(), 2, 0, "")),
        Err("the track is not in the queue".to_string())
    );
    assert_eq!(h.state(), before);
}

/// The tables a function and everything it calls read and write.
fn touches(name: &str) -> (Vec<String>, Vec<String>) {
    let f = domain().lib.get(&domain().get(name)).unwrap();
    let (mut reads, mut writes) = (vec![], vec![]);
    let mut todo = vec![f.f.body.clone()];
    let mut seen = vec![];
    while let Some(e) = todo.pop() {
        match &e {
            Expr::Scan(t) | Expr::Lookup(t, _) => reads.push(t.clone()),
            Expr::Insert(t, _) | Expr::Delete(t, _) => writes.push(t.clone()),
            Expr::Update(t, ..) => {
                reads.push(t.clone());
                writes.push(t.clone())
            }
            Expr::Call(h, _) if !seen.contains(h) => {
                seen.push(*h);
                todo.push(domain().lib.get(h).unwrap().f.body.clone());
            }
            _ => {}
        }
        todo.extend(children(&e).into_iter().cloned());
    }
    (reads, writes)
}

/// `scrub` is the one ephemeral transition, and the two kinds are two
/// tables: it touches nothing but `listening_scrub`, and no ordered one
/// reads that table (v4's verifier rule, "no ordered reads of ephemeral
/// fields", by construction). Falsified by `pause` reading its caller's
/// scrubber: it read `listening_scrub`.
#[test]
fn a_scrub_is_ephemeral_and_nothing_ordered_reads_it() {
    let (reads, writes) = touches("listening.scrub");
    assert!(reads.is_empty() && writes.iter().all(|t| t == "listening_scrub"), "{reads:?} {writes:?}");
    let ordered = [
        "start",
        "play",
        "pause",
        "seek",
        "skip",
        "jump",
        "transfer",
        "stop",
        "enqueue",
        "remove",
        "move",
        "clear_upcoming",
        "started",
        "advance",
        "stalled",
        "release",
        "equip",
        "set_volume",
        "nudge_volume",
        "set_mute",
        "report_volume",
        "join",
        "unjoin",
        "report_group",
        "set_group_volume",
        "nudge_group_volume",
        "set_group_mute",
        "on_join",
        "on_leave",
        "on_restore",
    ];
    for t in ordered {
        let name = format!("listening.{t}");
        assert!(!touches(&name).0.contains(&"listening_scrub".to_string()), "{t} reads the scrubbers");
        assert_eq!(domain().lib.get(&domain().get(&name)).map(|_| ()), Some(()));
    }
    let lives = domain()
        .module
        .bindings
        .iter()
        .filter(|b| b.0.starts_with("listening.") && b.0 != "listening.record_play");
    assert!(lives.clone().count() > ordered.len());

    let mut h = House::new();
    h.arrive("laptop", true);
    h.arrive("phone", true);
    h.playing_on("laptop", 0);
    h.ok("phone", "scrub", &Scrub { at_ms: Some(Ms(120_000)) });
    assert_eq!(h.state().scrub.get("phone"), Some(&120_000));
    h.ok("phone", "scrub", &Scrub { at_ms: None });
    assert!(h.state().scrub.is_empty(), "let go");
    h.ok("phone", "scrub", &Scrub { at_ms: Some(Ms(1)) });
    h.leave("phone");
    assert!(h.state().scrub.is_empty(), "emptied when the phone left");
}

/// v4's relay of a scrub: to everybody else, not back to its author, not
/// sequenced, not in the state's hash. Waits for D1: the volatile machine
/// keeps the ephemeral table's calls as pending intents, relayed coalesced.
#[test]
#[ignore = "waits for D1: the ephemeral relay, coalesced, outside the sequence and the hash"]
fn a_scrub_is_relayed_and_never_sequenced() {
    unimplemented!("D1: relay of listening_scrub calls")
}

// * Volume and groups

/// Nothing has a volume until it says so. Falsified by an `on_join` that
/// gave every device that can play a volume: the phone's `set_volume` was
/// taken.
#[test]
fn nothing_has_a_volume_until_it_says_so() {
    let mut h = House::new();
    h.arrive("phone", true);
    h.arrive("browser", true);
    let s = h.state();
    let phone = s.device("phone").unwrap();
    assert_eq!(
        (phone.can_volume, phone.volume, phone.muted, phone.family.clone(), phone.follows.clone()),
        (false, 100, false, None, None)
    );
    for d in ["phone", "nobody"] {
        assert_eq!(
            h.call("browser", "set_volume", &SetVolume { device: d.into(), level: 30 }),
            Err("that device is not here, or has no volume".to_string()),
            "{d}"
        );
    }
    assert_eq!(
        h.call("phone", "report_volume", &ReportVolume { level: 10, muted: false }),
        Err("not here, or no volume".to_string())
    );
    h.ok(
        "browser",
        "equip",
        &Equip {
            can_volume: true,
            family: None,
            muted: true,
            volume: 64,
        },
    );
    let b = h.state().device("browser").cloned().unwrap();
    assert_eq!((b.can_volume, b.volume, b.muted, b.family), (true, 64, true, None));
    assert_eq!(
        h.call(
            "browser",
            "set_volume",
            &SetVolume {
                device: "browser".into(),
                level: 101
            }
        ),
        Err("between 0 and 100".to_string()),
        "a level is 0 to 100"
    );
}

/// A device's volume, set, nudged and clamped, muted and unmuted. Falsified
/// by `nudge_volume` without its clamp: the browser came out at 130.
#[test]
fn a_volume_is_set_nudged_clamped_and_muted() {
    let mut h = House::new();
    h.arrive("phone", true);
    h.arrive("browser", true);
    h.ok(
        "browser",
        "equip",
        &Equip {
            can_volume: true,
            family: None,
            muted: false,
            volume: 50,
        },
    );
    let nudge = |h: &mut House, delta: i64| {
        h.call(
            "phone",
            "nudge_volume",
            &NudgeVolume {
                delta,
                device: "browser".into(),
            },
        )
    };
    let mute = |h: &mut House, muted: bool| {
        h.ok(
            "phone",
            "set_mute",
            &SetMute {
                device: "browser".into(),
                muted,
            },
        )
    };
    h.ok(
        "phone",
        "set_volume",
        &SetVolume {
            device: "browser".into(),
            level: 30,
        },
    );
    assert_eq!(h.volume("browser"), (30, false));
    nudge(&mut h, 100).unwrap();
    assert_eq!(h.volume("browser"), (100, false), "clamped at the top");
    nudge(&mut h, -7).unwrap();
    assert_eq!(h.volume("browser"), (93, false));
    nudge(&mut h, -100).unwrap();
    assert_eq!(h.volume("browser"), (0, false), "and at the bottom");
    mute(&mut h, true);
    assert_eq!(h.volume("browser"), (0, true));
    nudge(&mut h, 5).unwrap();
    assert_eq!(h.volume("browser"), (5, false), "a volume moved is a volume unmuted");
    mute(&mut h, true);
    mute(&mut h, false);
    assert_eq!(h.volume("browser"), (5, false));
    assert_eq!(nudge(&mut h, 0), Err("a nudge moves the volume".to_string()));
    assert_eq!(h.volume("phone"), (100, false), "the phone's own entry never moved");
}

/// What a device says of its own volume is its own entry's. Falsified by a
/// `report_volume` that wrote every device with a volume: the browser came
/// out at 20 as well.
#[test]
fn a_report_of_a_volume_is_the_callers_own() {
    let mut h = House::new();
    h.arrive("phone", true);
    h.arrive("browser", true);
    h.ok(
        "browser",
        "equip",
        &Equip {
            can_volume: true,
            family: None,
            muted: false,
            volume: 50,
        },
    );
    h.speaker("media_player.kitchen", "sonos", 35);
    h.ok("media_player.kitchen", "report_volume", &ReportVolume { level: 20, muted: true });
    assert_eq!(h.volume("media_player.kitchen"), (20, true));
    assert_eq!(h.volume("browser"), (50, false));
}

/// v4's "the phone hears it, in order": an ordered call's fan-out. Waits
/// for D1's volatile machine.
#[test]
#[ignore = "waits for D1: an ordered volatile entry fanned out in sequence"]
fn a_report_of_a_volume_is_heard_in_order() {
    unimplemented!("D1: Fanout of a sequenced volatile batch")
}

/// A speaker joins the output's group only of its own family and while
/// here. Falsified by a `joinable` that did not compare families: the
/// cast speaker joined the sonos kitchen.
#[test]
fn a_speaker_joins_only_its_own_family_and_only_while_here() {
    let mut h = House::new();
    h.arrive_as("alice", "phone", "phone", true);
    h.speaker("media_player.kitchen", "sonos", 30);
    h.speaker("media_player.study", "sonos", 30);
    h.speaker("media_player.den", "sonos", 30);
    h.speaker("media_player.cast", "cast", 30);
    assert_eq!(
        h.call("phone", "join", &member("media_player.study")),
        Err("nothing is playing to join".to_string())
    );
    h.playing_on("phone", 0);
    assert_eq!(
        h.call("phone", "join", &member("media_player.study")),
        Err("that device cannot join this one".to_string()),
        "a phone has no family"
    );
    h.ok("phone", "transfer", &to("media_player.kitchen"));
    h.ok("media_player.kitchen", "started", &at_ms(0));
    h.leave("media_player.den");
    for who in ["media_player.cast", "phone", "media_player.kitchen", "media_player.den", "nobody"] {
        assert_eq!(
            h.call("phone", "join", &member(who)),
            Err("that device cannot join this one".to_string()),
            "{who}"
        );
    }
    h.ok("phone", "join", &member("media_player.study"));
    assert_eq!(h.follows("media_player.study").as_deref(), Some("media_player.kitchen"));
    assert!(h.state().grouped());
    assert_eq!(h.state().group("media_player.kitchen").len(), 2);
    h.ok("phone", "unjoin", &member("media_player.study"));
    assert_eq!(h.follows("media_player.study"), None);
    for who in ["media_player.study", "media_player.kitchen"] {
        assert_eq!(
            h.call("phone", "unjoin", &member(who)),
            Err("that device is not in a group".to_string()),
            "{who}"
        );
    }
}

/// A group is never in a state Sonos cannot be in. Falsified by a
/// `followed` that did not let go of whoever followed the device joining:
/// the hall went on following the den — a chain.
#[test]
fn a_group_is_never_in_a_state_sonos_cannot_be_in() {
    let mut h = House::new();
    h.arrive_as("alice", "phone", "phone", true);
    for id in ["media_player.kitchen", "media_player.study", "media_player.den", "media_player.hall"] {
        h.speaker(id, "sonos", 40);
    }
    h.playing_in("media_player.kitchen");
    h.ok("phone", "join", &member("media_player.study"));
    h.ok(
        "media_player.den",
        "report_group",
        &ReportGroup {
            members: vec!["media_player.hall".into(), "media_player.study".into()],
        },
    );
    assert_eq!(h.follows("media_player.study").as_deref(), Some("media_player.den"));
    assert_eq!(h.follows("media_player.hall").as_deref(), Some("media_player.den"));
    assert!(!h.state().grouped(), "the kitchen's group lost its only member");
    h.ok("phone", "join", &member("media_player.den"));
    assert_eq!(h.follows("media_player.den").as_deref(), Some("media_player.kitchen"));
    assert_eq!(h.follows("media_player.hall"), None, "a member leads nothing");
    assert_eq!(h.follows("media_player.study"), None);
    h.ok(
        "media_player.den",
        "equip",
        &Equip {
            can_volume: true,
            family: Some(Family("heos".into())),
            muted: false,
            volume: 40,
        },
    );
    assert_eq!(h.follows("media_player.den"), None);
    h.ok("phone", "join", &member("media_player.hall"));
    h.leave("media_player.kitchen");
    assert_eq!(h.follows("media_player.hall"), None, "nobody follows a device that is not here");
    for d in &h.state().devices {
        if let Some(l) = &d.follows {
            assert!(h.state().device(l).is_some_and(|l| l.follows.is_none()), "{} follows a member", d.id);
        }
    }
}

/// The sound handed to a member makes it the leader. Falsified by a
/// `transfer` that left the devices alone: the study came out the output
/// and still following the kitchen.
#[test]
fn the_sound_handed_to_a_member_makes_it_the_leader() {
    let mut h = House::new();
    h.arrive_as("alice", "phone", "phone", true);
    for id in ["media_player.kitchen", "media_player.study", "media_player.den"] {
        h.speaker(id, "sonos", 40);
    }
    h.playing_in("media_player.kitchen");
    h.ok("phone", "join", &member("media_player.study"));
    h.ok("phone", "join", &member("media_player.den"));
    h.ok("phone", "transfer", &to("media_player.study"));
    assert_eq!(output(&h.state()), here("media_player.study", true));
    assert_eq!(h.follows("media_player.study"), None);
    assert_eq!(h.follows("media_player.kitchen").as_deref(), Some("media_player.study"));
    assert_eq!(h.follows("media_player.den").as_deref(), Some("media_player.study"));
    h.ok("media_player.study", "started", &at_ms(0));
    h.ok("phone", "transfer", &to("phone"));
    assert!(!h.state().grouped(), "the phone leads nothing");
    assert_eq!(h.state().followers("media_player.study").len(), 2, "the speakers are still a group");
    h.ok("phone", "transfer", &to("media_player.study"));
    assert!(h.state().grouped(), "a leader keeps its group");
    assert_eq!(h.state().group("media_player.study").len(), 3);
}

/// The group's volume moves every member in proportion. Falsified by a
/// `scaled` that set every member to the level: 20 and 60 at 60 came out
/// 60 and 60, where 30 and 90 is Sonos's.
#[test]
fn a_groups_volume_moves_every_member_in_proportion() {
    let mut h = House::new();
    h.arrive_as("alice", "phone", "phone", true);
    h.speaker("media_player.kitchen", "sonos", 20);
    h.speaker("media_player.study", "sonos", 60);
    h.speaker("media_player.den", "sonos", 70);
    h.playing_in("media_player.kitchen");
    h.ok("phone", "join", &member("media_player.study"));
    assert_eq!(h.state().group_volume("media_player.kitchen"), Some(40));
    h.ok("media_player.study", "report_volume", &ReportVolume { level: 60, muted: true });
    let both = |h: &House| (h.volume("media_player.kitchen"), h.volume("media_player.study"));
    h.ok("phone", "set_group_volume", &GroupVolume { level: 60 });
    assert_eq!(both(&h), ((30, false), (90, false)), "60 over 40, each, and unmuted");
    assert_eq!(h.volume("media_player.den"), (70, false), "not in the group, not moved");
    h.ok("phone", "nudge_group_volume", &GroupNudge { delta: 20 });
    assert_eq!((both(&h).0 .0, both(&h).1 .0), (40, 100), "80 over 60: 40, and 120 clamped");
    h.ok("phone", "nudge_group_volume", &GroupNudge { delta: -100 });
    assert_eq!((both(&h).0 .0, both(&h).1 .0), (0, 0));
    h.ok("phone", "set_group_volume", &GroupVolume { level: 25 });
    assert_eq!((both(&h).0 .0, both(&h).1 .0), (25, 25), "from nothing there is no proportion to keep");
    h.ok("phone", "set_group_mute", &GroupMute { muted: true });
    assert_eq!((both(&h).0 .1, both(&h).1 .1), (true, true));
    assert!(h.state().group_muted("media_player.kitchen"));
    h.ok("phone", "unjoin", &member("media_player.study"));
    h.ok("phone", "set_group_volume", &GroupVolume { level: 55 });
    assert_eq!(h.volume("media_player.kitchen"), (55, false));
    assert_eq!(h.volume("media_player.study"), (25, true));

    let mut h = House::new();
    h.arrive_as("alice", "phone", "phone", true);
    h.playing_on("phone", 0);
    assert_eq!(
        h.call("phone", "set_group_volume", &GroupVolume { level: 10 }),
        Err("nothing playing has a volume".to_string())
    );
}

/// A leader's word on its group. Falsified by a `regrouped` that left a
/// member it no longer named following it: the study stayed in the
/// kitchen's group.
#[test]
fn a_leader_reports_its_group() {
    let mut h = House::new();
    h.arrive_as("alice", "phone", "phone", true);
    h.speaker("media_player.kitchen", "sonos", 20);
    h.speaker("media_player.study", "sonos", 20);
    h.speaker("media_player.den", "sonos", 20);
    h.speaker("media_player.cast", "cast", 20);
    let names = |xs: &[&str]| ReportGroup {
        members: xs.iter().map(|x| x.to_string()).collect(),
    };
    h.ok(
        "media_player.kitchen",
        "report_group",
        &names(&["media_player.study", "media_player.den", "media_player.cast", "media_player.ghost"]),
    );
    let s = h.state();
    let mut members: Vec<&str> = s.followers("media_player.kitchen").iter().map(|d| d.id.as_str()).collect();
    members.sort_unstable();
    assert_eq!(members, ["media_player.den", "media_player.study"]);
    h.ok("media_player.kitchen", "report_group", &names(&["media_player.den"]));
    assert_eq!(h.follows("media_player.study"), None);
    assert_eq!(h.follows("media_player.den").as_deref(), Some("media_player.kitchen"));
    assert_eq!(
        h.call("phone", "report_group", &names(&["media_player.den"])),
        Err("not here, or not a speaker that groups".to_string())
    );
}
