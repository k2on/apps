//! The listening session's rules, as transitions (`docs/plan-live.md`,
//! "Harken's session"): every case the old hand-written desk was tested
//! for, and the rules that replaced its routing, run by the engine's own
//! sans-io server (`ark::live::state::Desk`) — the sequencer, the hooks and
//! the relay a real hub runs, with no socket under them.
//!
//! **Native and Emit agree at every step.** Every event is handed to two
//! desks at once, one running the domain's own Rust (`Module::lives`) and
//! one interpreting the closure it emitted, and what each says to whom,
//! and the state each holds, must be the same bytes after every step. A
//! transition that means one thing natively and another interpreted fails
//! here on the step that parts them — which is what a phone running the
//! module and a server running the Rust would otherwise disagree about in
//! silence.
//!
//! Each test was falsified once by breaking the rule it holds; its doc
//! comment says how, and what failed.

use std::collections::BTreeMap;

use ark::eval::Args;
use ark::ir::ordering::Kind;
use ark::live::state::{Call, Desk, DeviceDecl, Instance, Out, Runner, Said, Who};
use ark::live::ConnId;
use ark::value::{Id, Value};
use harken_domain::listening::read::{input, Clock, Listening, Output, Track, STATE};

/// The server's clock at the start of every test.
const T0: i64 = 1_700_000_000_000;

fn runner(native: bool) -> Runner {
    let m = harken_domain::module();
    let mut r = Runner::of_module(m.build());
    if native {
        r.hold(m.lives());
    }
    r
}

/// A device: its connection, and whose it is.
struct Stand {
    conn: ConnId,
    user: String,
}

/// One server, twice over — natively and interpreted — and the devices
/// standing in it.
struct House {
    desks: [Desk; 2],
    stands: BTreeMap<String, Stand>,
    next_conn: ConnId,
    next_id: u16,
    counter: i64,
    now: i64,
}

impl House {
    fn new() -> House {
        House::of(Desk::new(runner(true)), Desk::new(runner(false)))
    }

    fn of(native: Desk, interpreted: Desk) -> House {
        House {
            desks: [native, interpreted],
            stands: BTreeMap::new(),
            next_conn: 1,
            next_id: 0,
            counter: 0,
            now: T0,
        }
    }

    /// Both desks, the same event; the same answer from each.
    fn both(&mut self, f: impl Fn(&mut Desk) -> Out) -> Out {
        let a = f(&mut self.desks[0]);
        let b = f(&mut self.desks[1]);
        assert_eq!(a, b, "native and interpreted said different things");
        for (i, h) in &self.desks[0].instances {
            assert_eq!(
                Some(&h.value),
                self.desks[1].instances.get(i).map(|h| &h.value),
                "native and interpreted hold different states of {i:?}"
            );
        }
        a
    }

    fn wait(&mut self, ms: i64) {
        self.now += ms;
    }

    /// A device of `user` connects, saying what it is.
    fn arrive_as(&mut self, user: &str, device: &str, kind: &str, can_play: bool) -> ConnId {
        let conn = self.next_conn;
        self.next_conn += 1;
        let who = Who {
            user: user.into(),
            session: format!("{device}-login"),
            roles: Default::default(),
        };
        let decl = DeviceDecl::new(device, kind, device, can_play);
        let now = self.now;
        self.both(|d| d.hello(conn, who.clone(), decl.clone(), now));
        self.stands.insert(device.into(), Stand { conn, user: user.into() });
        conn
    }

    fn arrive(&mut self, device: &str, can_play: bool) -> ConnId {
        self.arrive_as("alice", device, "computer", can_play)
    }

    /// Its connection goes. Not a decision about the music — which is the
    /// whole point of half of these.
    fn leave(&mut self, device: &str) {
        let s = self.stands.remove(device).expect("standing");
        self.drop_conn(s.conn);
    }

    fn drop_conn(&mut self, conn: ConnId) {
        let now = self.now;
        self.both(|d| d.part(conn, now));
    }

    /// Call as `device`: what the server said, or the refusal it sent back.
    fn call(&mut self, device: &str, transition: &str, input: Args) -> Result<Out, String> {
        let s = &self.stands[device];
        let (conn, user) = (s.conn, s.user.clone());
        self.next_id += 1;
        let mut id: Id = [0; 16];
        id[14..].copy_from_slice(&self.next_id.to_be_bytes());
        let counter = match self.desks[0].runner.kind(STATE, transition) {
            Kind::Ephemeral => {
                self.counter += 1;
                Some(self.counter)
            }
            Kind::Ordered => None,
        };
        let call = Call {
            instance: Instance::new(STATE, user),
            transition: transition.into(),
            input,
            id,
            counter,
        };
        let now = self.now;
        let out = self.both(|d| d.call(conn, call.clone(), now));
        match out.iter().find_map(|(c, s)| match s {
            Said::Refused(r) if *c == conn => Some(r.reason.clone()),
            _ => None,
        }) {
            Some(why) => Err(why),
            None => Ok(out),
        }
    }

    fn ok(&mut self, device: &str, transition: &str, input: Args) -> Out {
        self.call(device, transition, input)
            .unwrap_or_else(|why| panic!("{device} {transition}: refused: {why}"))
    }

    fn value(&self, user: &str) -> Value {
        self.desks[0].instances[&Instance::new(STATE, user)].value.clone()
    }

    fn state_of(&self, user: &str) -> Listening {
        let v = self.value(user);
        Listening::from_value(&v).unwrap_or_else(|| panic!("not a listening state: {v:?}"))
    }

    fn state(&self) -> Listening {
        self.state_of("alice")
    }

    /// Something playing on `device`: pressed there, from two tracks in.
    fn playing_on(&mut self, device: &str, position_ms: i64) {
        self.ok(device, "start", input::start(&queue(), 0, position_ms, "album/Brandenburg"));
    }
}

fn track(title: &str, n: u8) -> Track {
    let mut id: Id = [0; 16];
    id[15] = n;
    Track {
        id,
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

fn output(l: &Listening) -> Output {
    l.session.as_ref().expect("a session").output.clone()
}

fn clock(l: &Listening) -> Clock {
    l.session.as_ref().expect("a session").clock
}

/// The bug the desk was written for, as one test: a laptop is playing; its
/// connection goes; the sound is still the laptop's — away, paused where it
/// had got to, and still named, because that is the one row a picker most
/// needs to draw.
///
/// Falsified by an `on_leave` that drops the output with the device (the
/// output read `nowhere`) and, separately, by one that left the clock
/// running (`playing` at 42 s).
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
        "the sound belongs to the laptop; it is simply not answering"
    );
    assert_eq!(
        clock(&s),
        Clock::Paused { at_ms: 42_000 },
        "and it cannot be playing, there is nobody there"
    );
    assert!(s.device("laptop").is_none(), "gone from who is here");
    assert!(s.device("phone").is_some());
}

/// …and when the laptop comes back it has the sound again, still paused:
/// a reconnect is the same device, which may well still be the output.
///
/// Falsified by an `on_join` that did not give an away output back: the
/// laptop came back to a session that still said `away`.
#[test]
fn the_output_coming_back_has_it_again_paused() {
    let mut h = House::new();
    h.arrive("laptop", true);
    h.playing_on("laptop", 10_000);
    h.leave("laptop");
    h.arrive("laptop", true);
    let s = h.state();
    assert_eq!(
        output(&s),
        Output::Device {
            id: "laptop".into(),
            moving: false
        }
    );
    assert_eq!(clock(&s), Clock::Paused { at_ms: 10_000 });
}

/// A device that merely turns up takes nothing: with the laptop away, a
/// tablet opening its app must not become the output just by existing.
///
/// Falsified by an `on_join` that handed an away output to whoever joined:
/// the tablet came out the output.
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

/// A command is a change to the session, made by anybody, which the device
/// making the sound obeys — it is told nothing else. Pausing on the phone
/// pauses the laptop's session and leaves the laptop the output; the laptop
/// hears it as every device does, by the `applied` frame.
///
/// Falsified by a `pause` that handed the sound to the caller (the
/// desk's rule four, misapplied): the phone came out the output.
#[test]
fn a_command_is_a_state_change_the_output_obeys() {
    let mut h = House::new();
    let laptop = h.arrive("laptop", true);
    h.arrive("phone", true);
    h.playing_on("laptop", 0);
    h.wait(5_000);
    let out = h.ok("phone", "pause", input::none());
    assert!(
        out.iter()
            .any(|(c, s)| *c == laptop && matches!(s, Said::Applied(a) if a.transition == "pause")),
        "the laptop hears the pause"
    );
    let s = h.state();
    assert_eq!(output(&s).id(), Some("laptop"), "and asking did not move it");
    assert_eq!(clock(&s), Clock::Paused { at_ms: 5_000 });

    h.ok("phone", "seek", input::seek(90_000));
    assert_eq!(clock(&h.state()), Clock::Paused { at_ms: 90_000 }, "a paused seek stays paused");
    h.ok("phone", "play", input::none());
    let s = h.state();
    assert_eq!(output(&s).id(), Some("laptop"), "play resumes on the output that is here");
    assert_eq!(clock(&s), Clock::Playing { at_ms: 90_000, since: h.now });
    h.wait(1_000);
    h.ok("phone", "seek", input::seek(5_000));
    assert_eq!(
        clock(&h.state()),
        Clock::Playing { at_ms: 5_000, since: h.now },
        "a playing seek plays on from there"
    );
}

/// …unless the output cannot be reached, and the asker can make a sound —
/// and only for a press that *means* a sound.
///
/// Falsified by a `seek` that took the sound like `skip` does: the phone
/// came out the output after seeking a session whose laptop had gone.
#[test]
fn only_a_press_that_means_a_sound_takes_it_from_a_device_that_has_gone() {
    let mut h = House::new();
    h.arrive("laptop", true);
    h.arrive("phone", true);
    h.playing_on("laptop", 77_000);
    h.leave("laptop");

    h.ok("phone", "pause", input::none());
    h.ok("phone", "seek", input::seek(5));
    assert_eq!(
        output(&h.state()).id(),
        Some("laptop"),
        "nothing to pause is not a reason to take the music"
    );

    h.ok("phone", "skip", input::skip(1));
    let s = h.state();
    assert_eq!(
        output(&s),
        Output::Device {
            id: "phone".into(),
            moving: false
        },
        "somebody pressed a button that makes a sound and nothing else can answer"
    );
    let a = s.session.as_ref().unwrap();
    assert_eq!(a.at, 1, "and the press it made, so next still means next");
    assert_eq!(a.clock, Clock::Playing { at_ms: 0, since: h.now });
    assert_eq!(
        a.queue,
        queue(),
        "the queue the laptop had: a phone watching all evening holds none of its own"
    );

    // Play is the same press.
    let mut h = House::new();
    h.arrive("laptop", true);
    h.arrive("phone", true);
    h.playing_on("laptop", 30_000);
    h.leave("laptop");
    h.ok("phone", "play", input::none());
    let s = h.state();
    assert_eq!(output(&s).id(), Some("phone"));
    assert_eq!(clock(&s), Clock::Playing { at_ms: 30_000, since: h.now }, "from where the laptop left it");
}

/// A device that cannot make a sound never takes it, however hard it
/// presses: the desktop build has no audio device, so it is a remote. With
/// nothing that can answer, its press is refused — said to it, so the bar
/// that asked can show the press went nowhere — and with a paused output
/// that is here, its track plays there.
///
/// Falsified by dropping `play`'s "nothing here can play it" guard: the
/// desktop came out the output.
#[test]
fn a_device_that_cannot_be_heard_never_takes_the_sound() {
    let mut h = House::new();
    h.arrive("laptop", true);
    h.arrive("desktop", false);
    h.playing_on("laptop", 0);
    h.leave("laptop");
    for t in ["play", "start", "skip", "jump"] {
        let i = match t {
            "start" => input::start(&queue(), 0, 0, ""),
            "skip" => input::skip(1),
            "jump" => input::jump(1),
            _ => input::none(),
        };
        assert_eq!(h.call("desktop", t, i), Err("nothing here can play it".to_string()), "{t}");
    }
    assert_eq!(output(&h.state()).id(), Some("laptop"));

    // An empty session and a desktop: nowhere to play it.
    let mut h = House::new();
    h.arrive("desktop", false);
    assert_eq!(
        h.call("desktop", "start", input::start(&queue(), 0, 0, "")),
        Err("nothing here can play it".to_string())
    );

    // A paused laptop that is here: the desktop's track plays there.
    let mut h = House::new();
    h.arrive("laptop", true);
    h.arrive("desktop", false);
    h.playing_on("laptop", 0);
    h.ok("laptop", "pause", input::none());
    h.ok("desktop", "start", input::start(&queue(), 1, 0, ""));
    let s = h.state();
    assert_eq!(output(&s).id(), Some("laptop"));
    assert!(s.sounding());
}

/// A track pressed goes to the device making the sound while one is — the
/// press on the phone plays on the laptop — and while nothing is sounding,
/// the device it was pressed on takes it. The second half is the decided
/// reconnect rule's other branch: a device that was playing offline comes
/// back to a session with nothing sounding, says `start`, and keeps its own
/// sound.
///
/// Falsified by a `start` that kept a reachable output whatever its clock:
/// the phone's track went to the paused laptop.
#[test]
fn a_track_pressed_plays_where_the_sound_is_or_here_when_it_is_nowhere() {
    let mut h = House::new();
    h.arrive("laptop", true);
    h.arrive("phone", true);
    h.playing_on("laptop", 0);
    h.ok("phone", "start", input::start(&queue(), 1, 0, "playlist/Favorites"));
    let s = h.state();
    assert_eq!(output(&s).id(), Some("laptop"), "something is sounding, so the press goes there");
    assert_eq!(s.session.as_ref().unwrap().at, 1);
    assert_eq!(s.session.as_ref().unwrap().from, "playlist/Favorites");

    h.ok("phone", "pause", input::none());
    h.ok("phone", "start", input::start(&queue(), 0, 61_000, ""));
    let s = h.state();
    assert_eq!(output(&s).id(), Some("phone"), "nothing is sounding, so the press is a sound here");
    assert_eq!(clock(&s), Clock::Playing { at_ms: 61_000, since: h.now });
}

/// Picking a speaker halfway through a track picks up halfway through it,
/// and until the speaker says otherwise it is *connecting*. Only the output
/// can say it has started; saying so re-anchors the clock at where the
/// speaker actually is.
///
/// Falsified by a `started` with no "not the output" guard: the phone's
/// claim to have started ended the kitchen's hand-off.
#[test]
fn picking_a_speaker_picks_up_where_the_track_was() {
    let mut h = House::new();
    h.arrive_as("alice", "phone", "phone", true);
    h.arrive_as("alice", "media_player.kitchen", "speaker", true);
    h.playing_on("phone", 90_000);
    h.wait(1_500);
    h.ok("phone", "transfer", input::transfer("media_player.kitchen"));
    let s = h.state();
    assert_eq!(
        output(&s),
        Output::Device {
            id: "media_player.kitchen".into(),
            moving: true
        },
        "still connecting"
    );
    assert_eq!(clock(&s).position(h.now), 91_500, "where the track was, for the speaker to pick up from");
    assert!(s.moving_to("media_player.kitchen"));

    assert_eq!(h.call("phone", "started", input::at(1)), Err("not the output".to_string()));
    h.wait(2_000);
    h.ok("media_player.kitchen", "started", input::at(92_100));
    let s = h.state();
    assert_eq!(
        output(&s),
        Output::Device {
            id: "media_player.kitchen".into(),
            moving: false
        }
    );
    assert_eq!(clock(&s), Clock::Playing { at_ms: 92_100, since: h.now });

    // Handing it to the device that already has it, settled, is nothing.
    let before = h.value("alice");
    h.ok("phone", "transfer", input::transfer("media_player.kitchen"));
    assert_eq!(h.value("alice"), before);
}

/// The sound cannot be handed to something that is not answering, which is
/// the one way to lose it entirely — nor to something that cannot be heard,
/// nor to nothing at all.
///
/// Falsified by a `transfer` guard that asked only whether the target was
/// in `devices`: the mute desktop came out the output.
#[test]
fn the_sound_is_not_handed_to_a_device_that_has_gone_or_is_mute() {
    let mut h = House::new();
    h.arrive("phone", true);
    h.arrive_as("alice", "media_player.kitchen", "speaker", true);
    h.arrive("desktop", false);
    h.playing_on("phone", 0);
    h.leave("media_player.kitchen");
    for to in ["media_player.kitchen", "desktop", "nobody"] {
        assert_eq!(
            h.call("phone", "transfer", input::transfer(to)),
            Err("that device is not here, or cannot play".to_string()),
            "{to}"
        );
        assert_eq!(
            output(&h.state()),
            Output::Device {
                id: "phone".into(),
                moving: false
            }
        );
    }
    let mut idle = House::new();
    idle.arrive("phone", true);
    idle.arrive("laptop", true);
    assert_eq!(
        idle.call("phone", "transfer", input::transfer("laptop")),
        Err("nothing is playing to move".to_string())
    );
}

/// A device on two connections at once — a tab reloaded before its old
/// socket was noticed gone — is one device, and the old connection closing
/// does not take the sound away from it.
///
/// Falsified by running `on_leave` on every connection's close rather than
/// on the device's last: the laptop came out away. (That is the engine's
/// rule, `Desk::part`; this holds harken's session to it.)
#[test]
fn a_device_on_a_second_connection_is_still_here_when_the_first_goes() {
    let mut h = House::new();
    let old = h.arrive("laptop", true);
    h.arrive("phone", true);
    h.playing_on("laptop", 5_000);
    h.arrive("laptop", true);
    h.drop_conn(old);
    let s = h.state();
    assert!(s.device("laptop").is_some());
    assert_eq!(output(&s).id(), Some("laptop"));
    assert!(s.sounding(), "still playing: nothing left");
}

/// What the output says of itself: the track ended (the next, or paused at
/// the start of the last when there is no next); a track that is no longer
/// the one playing ended (nothing, since somebody moved the session first);
/// it stopped by itself (paused there); it lets the sound go (the queue
/// stays, paused, nowhere). Nobody but the output may say any of them.
///
/// Falsified by an `advance` that did not compare `from_track`: the stale
/// one moved the session on a second time.
#[test]
fn the_output_says_what_happens_to_it_and_nobody_else_may() {
    let mut h = House::new();
    h.arrive("laptop", true);
    h.arrive("phone", true);
    h.playing_on("laptop", 0);
    for t in ["advance", "stalled", "release"] {
        let i = match t {
            "advance" => input::advance(0),
            "stalled" => input::at(1),
            _ => input::none(),
        };
        assert_eq!(h.call("phone", t, i), Err("not the output".to_string()), "{t}");
    }
    h.wait(300_000);
    h.ok("laptop", "advance", input::advance(0));
    let a = h.state().session.unwrap();
    assert_eq!((a.at, a.clock), (1, Clock::Playing { at_ms: 0, since: h.now }));
    let before = h.value("alice");
    h.ok("laptop", "advance", input::advance(0));
    assert_eq!(h.value("alice"), before, "a stale advance is nothing");

    h.ok("laptop", "stalled", input::at(12_000));
    assert_eq!(clock(&h.state()), Clock::Paused { at_ms: 12_000 });
    h.ok("laptop", "play", input::none());
    h.ok("laptop", "advance", input::advance(1));
    let a = h.state().session.unwrap();
    assert_eq!((a.at, a.clock), (1, Clock::Paused { at_ms: 0 }), "the end of the queue");
    assert_eq!(h.call("laptop", "skip", input::skip(1)), Err("nothing comes after this".to_string()));
    h.ok("laptop", "skip", input::skip(-1));
    assert_eq!(h.state().session.unwrap().at, 0);
    h.ok("laptop", "skip", input::skip(-1));
    assert_eq!(h.state().session.unwrap().at, 0, "before the first is the first again");

    h.ok("laptop", "release", input::none());
    let a = h.state().session.unwrap();
    assert_eq!(a.output, Output::Nowhere);
    assert!(!a.clock.playing());
    assert_eq!(a.queue, queue(), "the queue stays");
}

/// A restart: the queue as it was, paused where it had got to — never past
/// the end of the track — with no output, and nobody here. What the server
/// keeps is the ordered part only: nobody's scrubber survives. Restoring
/// moves the sequence by one, so a client resyncs onto the restored state.
///
/// Falsified by an `on_restore` that kept `devices`: the restored session
/// still listed the laptop and the phone.
#[test]
fn a_restart_restores_the_session_paused_with_no_output_and_nobody_here() {
    let mut h = House::new();
    h.arrive("laptop", true);
    h.arrive("phone", true);
    h.playing_on("laptop", 33_000);
    h.ok("phone", "scrub", input::scrub(Some(50_000)));
    assert_eq!(h.state().scrub.get("phone"), Some(&50_000));
    h.wait(4_000);
    let kept = h.desks[0].kept();
    assert_eq!(kept, h.desks[1].kept());
    let (i, seq, ordered) = kept.into_iter().find(|(i, ..)| i.key == "alice").expect("kept");
    assert!(
        !matches!(ordered.as_struct().get("scrub"), Some(Value::Struct(m)) if !m.is_empty()),
        "a scrubber is not kept: {ordered:?}"
    );

    let mut back = House::of(Desk::new(runner(true)), Desk::new(runner(false)));
    back.now = h.now + 60_000;
    let now = back.now;
    for d in &mut back.desks {
        d.restore(i.clone(), seq, ordered.clone(), now);
    }
    assert_eq!(back.value("alice"), back.desks[1].instances[&i].value);
    assert_eq!(back.desks[0].instances[&i].seq, seq + 1, "on_restore moved it");
    let s = back.state();
    assert!(s.devices.is_empty(), "nobody is here");
    assert!(s.scrub.is_empty());
    let a = s.session.unwrap();
    assert_eq!(a.output, Output::Nowhere);
    assert_eq!(a.clock, Clock::Paused { at_ms: 97_000 }, "where it had got to, the minute away and all");
    assert_eq!(a.queue, queue());

    // Never past the end of the track.
    let mut h = House::new();
    h.arrive("laptop", true);
    h.playing_on("laptop", 0);
    let (i, seq, ordered) = h.desks[0].kept().into_iter().next().unwrap();
    let mut back = House::of(Desk::new(runner(true)), Desk::new(runner(false)));
    let later = h.now + 3_600_000;
    for d in &mut back.desks {
        d.restore(i.clone(), seq, ordered.clone(), later);
    }
    assert_eq!(clock(&back.state()), Clock::Paused { at_ms: 300_000 });
}

/// Stopping is the session gone: nothing queued, nothing playing, nowhere.
///
/// Falsified by a `stop` that only paused: the session was still active.
#[test]
fn stopping_leaves_nothing() {
    let mut h = House::new();
    h.arrive("laptop", true);
    h.playing_on("laptop", 5_000);
    h.ok("laptop", "stop", input::none());
    let s = h.state();
    assert!(s.session.is_none());
    assert!(!s.sounding());
    assert_eq!(h.call("laptop", "play", input::none()), Err("nothing is queued".to_string()));
}

/// One instance per account: two people's sessions never meet, however
/// many devices each of them has.
///
/// Falsified by a scope of `ctx.session` (one per login): there was no
/// instance of alice's at all, each of her logins holding its own.
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
    assert_eq!(h.desks[0].instances.len(), 2);
}

/// A call this module cannot read is refused, to its caller, and changes
/// nothing: no transition of that name, or an input of the wrong shape.
///
/// Falsified by `skip`'s `by` without its refinement: a skip of 0 was
/// taken, and changed nothing — which is also why it is refused.
#[test]
fn a_call_nobody_can_read_changes_nothing() {
    let mut h = House::new();
    h.arrive("laptop", true);
    h.playing_on("laptop", 0);
    let before = h.value("alice");
    assert_eq!(
        h.call("laptop", "rewind", input::none()),
        Err("listening: no transition rewind".to_string())
    );
    assert_eq!(
        h.call("laptop", "skip", input::skip(0)),
        Err("a skip is one track either way".to_string())
    );
    assert!(h.call("laptop", "seek", input::none()).is_err());
    assert!(
        h.call("laptop", "start", input::start(&[], 0, 0, "")).is_err(),
        "an empty queue is not one"
    );
    assert_eq!(
        h.call("laptop", "start", input::start(&queue(), 2, 0, "")),
        Err("the track is not in the queue".to_string())
    );
    assert_eq!(h.value("alice"), before);
}

/// `scrub` is the one ephemeral transition: relayed to everybody else, not
/// sequenced, left out of the state's hash, and emptied when its device
/// leaves. Every other transition is ordered, and none reads it — which the
/// verifier would refuse by name.
///
/// Falsified by `pause` reading the scrub slot of its caller to pick where
/// to pause: the module no longer verified ("reads the ephemeral field
/// scrub").
#[test]
fn a_scrub_is_ephemeral_relayed_and_never_kept() {
    let m = harken_domain::module();
    let c = ark::hash::live_closure(m.build(), m.build().lookup_state(STATE).unwrap());
    for t in [
        "start", "play", "pause", "seek", "skip", "jump", "transfer", "stop", "started", "advance", "stalled", "release",
    ] {
        assert_eq!(c.kind(t), Kind::Ordered, "{t}");
    }
    assert_eq!(c.kind("scrub"), Kind::Ephemeral);

    let mut h = House::new();
    let laptop = h.arrive("laptop", true);
    let phone = h.arrive("phone", true);
    h.playing_on("laptop", 0);
    let hash = h.desks[0].runner.hash(STATE, &h.value("alice"));
    let seq = h.desks[0].instances[&Instance::new(STATE, "alice")].seq;
    let out = h.ok("phone", "scrub", input::scrub(Some(120_000)));
    assert!(out.iter().any(|(c, _)| *c == laptop), "relayed to the laptop");
    assert!(out.iter().all(|(c, _)| *c != phone), "and not back to its author");
    assert_eq!(h.state().scrub.get("phone"), Some(&120_000));
    assert_eq!(h.desks[0].runner.hash(STATE, &h.value("alice")), hash, "not in the hash");
    assert_eq!(h.desks[0].instances[&Instance::new(STATE, "alice")].seq, seq, "not sequenced");
    h.leave("phone");
    assert!(h.state().scrub.is_empty(), "emptied when the phone left");
}
