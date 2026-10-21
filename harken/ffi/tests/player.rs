//! A phone's player (S3b) against harken's own session: real peers of the
//! kernel in a [`Hall`] (a harken server in the process, which runs the
//! domain's transitions and hooks), harken's one reconciler behind the
//! phone's FFI ([`Player`]), and a stand-in player ([`Fake`], what
//! ExoPlayer is to the phone). The phone's reconciler tests
//! (`RemoteTests.kt`), moved here with the reconciler: the output obeying
//! and nobody else sounding, a track's end said once, the session following
//! the phone and the phone following a seek, a play recorded once by the
//! output alone, both branches of coming back, a phone unlinked being its
//! own session, a stall, a press, and the phone's own volume.
//!
//! Each was falsified once by breaking what it holds in the reconciler
//! (`harken/player/src/reconcile.rs`); the comment beside each says how.

use std::sync::{Arc, Mutex};

use ark_ffi::Peer;
use harken_domain::{Active, Clock, Id, Output, Present, Track};
use harken_ffi::{Hall, Loudness, PhoneSound, Player, SoundState};
use harken_player::{Change, Level, Press, SYNC_MS};
use serde_json::{json, Value};

/// A track of a test's queue: `n` names it, and it is `ms` long.
fn track(title: &str, n: u8, ms: i64) -> Track {
    let mut id = [0; 16];
    id[15] = n;
    Track {
        album: "Water Music".into(),
        creator: "Handel".into(),
        duration_ms: ms,
        file: format!("music/{title}.mp3"),
        id: Id::new(id),
        title: title.into(),
    }
}

fn queue() -> Vec<Track> {
    vec![track("a", 1, 100_000), track("b", 2, 100_000)]
}

/// A phone's media stream as `AudioManager` keeps it: fifteen steps and a
/// mute, every write counted.
#[derive(Clone, Copy, Debug)]
struct Stream {
    index: i64,
    muted: bool,
    writes: u32,
}

const STEPS: i64 = 15;

/// A player that does what it is told, and moves only when a test says so.
#[derive(Default)]
struct State {
    queue: Vec<Track>,
    index: i64,
    playing: bool,
    buffering: bool,
    position_ms: i64,
    ended: bool,
    from: String,
    stalled: bool,
    loads: u32,
    jumps: u32,
    seeks: u32,
    wake: Option<i64>,
    stream: Option<Stream>,
}

struct Fake(Mutex<State>);

impl Fake {
    fn new(stream: Option<i64>) -> Arc<Fake> {
        Arc::new(Fake(Mutex::new(State {
            index: -1,
            stream: stream.map(|index| Stream {
                index,
                muted: false,
                writes: 0,
            }),
            ..State::default()
        })))
    }

    fn s(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap()
    }

    /// The track played to its end, and the player went on to the next as
    /// ExoPlayer does — or, at the last, stopped there.
    fn finish(&self) {
        let mut s = self.s();
        if (s.index + 1) < s.queue.len() as i64 {
            s.index += 1;
            s.position_ms = 0;
        } else {
            s.ended = true;
            s.position_ms = s.queue[s.index as usize].duration_ms;
        }
    }
}

impl PhoneSound for Fake {
    fn state(&self) -> SoundState {
        let s = self.s();
        SoundState {
            queue: s.queue.iter().map(|t| t.id.hex()).collect(),
            index: s.index,
            playing: s.playing,
            buffering: s.buffering,
            position_ms: s.position_ms,
            ended: s.ended,
            from: s.from.clone(),
        }
    }

    fn take_stalled(&self) -> bool {
        std::mem::take(&mut self.s().stalled)
    }

    fn tracks(&self) -> String {
        serde_json::to_string(&self.s().queue).unwrap()
    }

    fn load(&self, queue: String, at: i64, position_ms: i64, playing: bool, from: String) {
        let mut s = self.s();
        s.queue = serde_json::from_str(&queue).unwrap();
        (s.index, s.position_ms, s.playing, s.from, s.ended) = (at, position_ms, playing, from, false);
        s.loads += 1;
    }

    fn jump(&self, at: i64, position_ms: i64) {
        let mut s = self.s();
        (s.index, s.position_ms, s.ended) = (at, position_ms, false);
        s.jumps += 1;
    }

    fn resume(&self) {
        let mut s = self.s();
        (s.playing, s.ended) = (true, false);
    }

    fn pause(&self) {
        self.s().playing = false;
    }

    fn seek(&self, ms: i64) {
        let mut s = self.s();
        (s.position_ms, s.ended) = (ms, false);
        s.seeks += 1;
    }

    fn wake_in(&self, ms: i64) {
        self.s().wake = Some(ms);
    }

    /// The stream's step as the session's 0 to 100.
    fn volume(&self) -> Option<Loudness> {
        self.s().stream.map(|st| Loudness {
            level: ((st.index as f64) * 100.0 / STEPS as f64).round() as i64,
            muted: st.muted,
        })
    }

    /// A level as the stream's nearest step, then the mute.
    fn set_volume(&self, level: i64, muted: bool) {
        let mut s = self.s();
        let Some(st) = &mut s.stream else { return };
        let i = ((level.clamp(0, 100) as f64) * STEPS as f64 / 100.0).round() as i64;
        if i != st.index {
            st.index = i;
            st.writes += 1;
        }
        if muted != st.muted {
            st.muted = muted;
            st.writes += 1;
        }
    }
}

struct Device<'h> {
    hall: &'h Hall,
    peer: Arc<Peer>,
    player: Arc<Player>,
    sound: Arc<Fake>,
    _dir: tempfile::TempDir,
}

fn on<'h>(hall: &'h Hall, id: &str, can_play: bool, kind: &str, stream: Option<i64>) -> Device<'h> {
    let dir = tempfile::tempdir().unwrap();
    let said = json!({ "can_play": can_play, "id": "", "kind": kind, "name": id, "user": "" }).to_string();
    let peer = hall.device(dir.path().to_str().unwrap().into(), "alice".into(), id.into(), said).unwrap();
    let player = Player::new(peer.clone(), id.into(), Some(dir.path().to_str().unwrap().into()));
    let sound = Fake::new(stream);
    player.attach(Some(sound.clone()));
    hall.carry();
    Device {
        hall,
        peer,
        player,
        sound,
        _dir: dir,
    }
}

impl Device<'_> {
    fn reconcile(&self) {
        self.player.step();
        self.hall.carry();
    }

    fn call(&self, transition: &str, input: Value) {
        self.peer
            .call(transition.into(), input.to_string())
            .unwrap_or_else(|e| panic!("{transition}: {e:?}"));
        self.hall.carry();
    }

    fn press(&self, p: Press) {
        let why = self.player.press(serde_json::to_string(&p).unwrap());
        assert_eq!(why, None, "{p:?} was refused");
        self.hall.carry();
    }

    fn view(&self) -> Value {
        serde_json::from_str(&self.player.view()).unwrap()
    }

    /// The session, while this device is part of it.
    fn session(&self) -> Option<Listening> {
        let v = self.view();
        (!v["listening"].is_null()).then(|| Listening {
            devices: serde_json::from_value(v["listening"]["devices"].clone()).unwrap(),
            scrub: serde_json::from_value(v["listening"]["scrub"].clone()).unwrap(),
            session: serde_json::from_value(v["listening"]["session"].clone()).unwrap(),
        })
    }

    fn active(&self) -> Active {
        self.session().unwrap().session.unwrap()
    }

    fn calls(&self) -> u64 {
        self.view()["stats"]["calls"].as_u64().unwrap()
    }

    fn plays(&self) -> Vec<Value> {
        serde_json::from_str::<Vec<Value>>(&self.peer.query("recently_played".into(), "null".into()).unwrap()).unwrap()
    }

    fn me(&self, id: &str) -> Present {
        let held = serde_json::from_str::<Value>(&self.hall.listening("alice".into())).unwrap();
        let ds: Vec<Present> = serde_json::from_value(held["devices"].clone()).unwrap();
        ds.into_iter().find(|d| d.id == id).unwrap()
    }
}

use harken_domain::host::Listening;

fn start(queue: &[Track], at: i64, position: i64, from: &str) -> Value {
    json!({ "at": at, "from": from, "position": position, "queue": queue })
}

fn drop_link(d: &Device) {
    d.peer.set_online(false);
    d.hall.carry();
}

fn relink(d: &Device) {
    d.peer.set_online(true);
    d.hall.carry();
}

fn now() -> i64 {
    ark_peer::now_ms()
}

/// The one rule a phone lives by: the output's player is made to agree with
/// the session, and every other device is silent. A press on the desktop —
/// a remote, which cannot play — pauses the phone, through nothing but the
/// session; and a hand-off quiets it.
///
/// Falsified by a look that left a device that is not the output alone
/// (`let_go` never asked): the phone went on playing after the sound moved
/// to the laptop.
#[test]
fn the_output_obeys_the_session_and_nobody_else_makes_a_sound() {
    let hall = Hall::new();
    let phone = on(&hall, "phone", true, "phone", None);
    let desk = on(&hall, "desktop", false, "computer", None);
    phone.call("start", start(&queue(), 0, 0, "#album/Water%20Music"));
    phone.reconcile();
    assert_eq!(phone.sound.s().queue, queue(), "the phone loaded what it pressed");
    assert!(phone.sound.s().playing);
    assert_eq!(phone.sound.s().index, 0);
    desk.reconcile();
    assert!(!desk.sound.s().playing, "the desktop makes no sound");
    assert_eq!(desk.sound.s().loads, 0, "and loads nothing");

    desk.call("pause", Value::Null);
    phone.reconcile();
    assert!(!phone.sound.s().playing, "a pause pressed on the desktop is a pause on the phone");
    desk.call("seek", json!({ "ms": 40_000 }));
    phone.reconcile();
    assert_eq!(phone.sound.s().position_ms, 40_000, "and a seek is a seek");
    desk.call("play", Value::Null);
    phone.reconcile();
    assert!(phone.sound.s().playing, "a play pressed on the desktop plays the phone");

    let laptop = on(&hall, "laptop", true, "computer", None);
    desk.call("transfer", json!({ "to": "laptop" }));
    phone.reconcile();
    laptop.reconcile();
    assert!(!phone.sound.s().playing, "the phone lost the sound and is silent");
    assert!(laptop.sound.s().playing && laptop.sound.s().queue == queue());
    let s = laptop.session().unwrap();
    assert!(s.outputs("laptop") && !s.moving_to("laptop"), "the laptop said it had started");
    assert_eq!(phone.sound.s().loads, 1, "the phone loaded once");
}

/// The phone's player holds the queue and moves on through it by itself:
/// that is the end of a track, said once as `advance`, and the player is not
/// dragged back while it lands — nor the next track loaded again. The last
/// one ending leaves the session paused at its start, said once however
/// often a player still at its end is reconciled; play there plays it
/// again.
///
/// Falsified by treating the player's own move as somebody else's (the
/// `Expect` arm taken away): the reconciler jumped it back, and the session
/// was still on the first track.
#[test]
fn a_track_that_ends_is_said_once_and_the_next_one_plays() {
    let hall = Hall::new();
    let phone = on(&hall, "phone", true, "phone", None);
    phone.call("start", start(&queue(), 0, 0, ""));
    phone.reconcile();
    phone.sound.finish();
    let calls = phone.calls();
    phone.reconcile();
    phone.reconcile();
    assert_eq!(phone.active().at, 1, "one track on");
    assert_eq!(phone.calls(), calls + 1, "said once");
    assert_eq!(phone.sound.s().loads, 1, "the player was not loaded again");
    assert_eq!(phone.sound.s().jumps, 0, "nor dragged back");
    assert!(phone.sound.s().playing);

    phone.sound.finish();
    let before = phone.calls();
    for _ in 0..3 {
        phone.reconcile();
    }
    assert_eq!(phone.calls(), before + 1, "the end of the queue, said once");
    let a = phone.active();
    assert_eq!(a.at, 1);
    assert_eq!(a.clock, Clock::Paused { at_ms: 0 });
    assert!(!phone.sound.s().playing);
    phone.call("play", Value::Null);
    phone.reconcile();
    assert!(phone.sound.s().playing, "and played again from its start");
    assert_eq!(phone.sound.s().position_ms, 0);
}

/// The phone is right about where it is. ExoPlayer a second behind a clock
/// nobody touched is followed: the session is told where the player is
/// (`report_position`), and the player is not sent a second ahead. Within
/// the bound nothing is said; filling a buffer, nothing either; a seek on
/// another device is the session moving, and the player goes there.
///
/// Falsified by the old rule, the player always sent where the clock counts
/// (the `moved` test taken out): "not sent ahead" failed.
#[test]
fn the_session_follows_the_phone_and_the_phone_follows_a_seek() {
    let hall = Hall::new();
    let phone = on(&hall, "phone", true, "phone", None);
    let desk = on(&hall, "desktop", false, "computer", None);
    phone.call("start", start(&queue(), 0, 40_000, ""));
    phone.reconcile();
    let loaded = phone.sound.s().position_ms;
    assert!((40_000..40_100).contains(&loaded), "loaded where it was pressed: {loaded}");
    let counted = || phone.active().position(now());

    // A second on the clock, and the player has played none of it.
    std::thread::sleep(std::time::Duration::from_millis(1_000));
    let seeks = phone.sound.s().seeks;
    phone.reconcile();
    assert_eq!(phone.sound.s().position_ms, loaded, "not sent ahead to where the clock counts");
    assert_eq!(phone.sound.s().seeks, seeks);
    let at = counted();
    assert!((at - loaded).abs() <= SYNC_MS, "the session is where the player is: {at}");
    let calls = phone.calls();
    phone.reconcile();
    assert_eq!(phone.calls(), calls, "and says it once");

    // Within the bound, nothing either way; filling a buffer, nothing either.
    let c = counted();
    phone.sound.s().position_ms = c - SYNC_MS / 2;
    phone.reconcile();
    assert_eq!(phone.calls(), calls);
    assert_eq!(phone.sound.s().seeks, seeks);
    let c = counted();
    {
        let mut s = phone.sound.s();
        s.position_ms = c - 3_000;
        s.buffering = true;
    }
    phone.reconcile();
    assert_eq!(phone.calls(), calls);
    assert_eq!(phone.sound.s().seeks, seeks);
    let c = counted();
    {
        let mut s = phone.sound.s();
        s.buffering = false;
        s.position_ms = c;
    }

    // A seek on the desktop is the session moving: the player goes.
    desk.call("seek", json!({ "ms": 100_000 }));
    phone.reconcile();
    let p = phone.sound.s().position_ms;
    assert!((p - 100_000).abs() <= SYNC_MS, "sought where the session is: {p}");
    assert_eq!(phone.calls(), calls, "and nothing said back");
}

/// A play counts once it has played thirty seconds or half the track, once
/// per load of a track, and only on the device making the sound — offline
/// too, as a pending intent the server gets later. Short of the threshold
/// the player is asked to wake the reconciler when it will be reached.
///
/// Falsified by counting on every look past the threshold (the `counted`
/// flag never written): the second look authored a second play.
#[test]
fn a_play_is_recorded_once_by_the_output_alone() {
    let hall = Hall::new();
    let phone = on(&hall, "phone", true, "phone", None);
    let laptop = on(&hall, "laptop", true, "computer", None);
    phone.call("start", start(&queue(), 0, 29_000, "#album/Water%20Music"));
    phone.reconcile();
    assert_eq!(phone.plays().len(), 0, "not yet: half of a hundred seconds is fifty, thirty is first");
    let wake = phone.sound.s().wake.unwrap_or(0);
    assert!((900..=1_100).contains(&wake), "a wake at the threshold: {wake}");
    phone.sound.s().position_ms = 30_500;
    phone.call("seek", json!({ "ms": 30_500 }));
    phone.reconcile();
    assert_eq!(phone.plays().len(), 1, "thirty seconds in: a play");
    phone.reconcile();
    assert_eq!(phone.plays().len(), 1, "once per play");
    laptop.sound.s().position_ms = 40_000;
    laptop.reconcile();
    assert_eq!(laptop.view()["stats"]["plays"], 0, "a device watching records nothing");
    assert_eq!(phone.plays()[0]["from"], "#album/Water%20Music");
    assert_eq!(phone.peer.status().pending, 0, "the play reached the server");

    // Offline the phone is its own session, and a play is the log's all the same.
    drop_link(&phone);
    phone.sound.jump(1, 0);
    phone.reconcile();
    phone.sound.s().position_ms = 31_000;
    phone.reconcile();
    assert_eq!(phone.peer.status().pending, 1, "a play made offline, pending");
    assert_eq!(phone.plays().len(), 2);
}

/// The decided rule for coming back, the first way: something is playing
/// elsewhere when the phone's link returns, so its own sound — it went on
/// playing while it was away — stops, and it says nothing.
///
/// Falsified by a look that says `start` whenever it comes back playing
/// (the `sounding` test taken out): the phone took the sound from the
/// laptop.
#[test]
fn coming_back_to_a_session_sounding_elsewhere_stops_this_phone() {
    let hall = Hall::new();
    let phone = on(&hall, "phone", true, "phone", None);
    let laptop = on(&hall, "laptop", true, "computer", None);
    phone.call("start", start(&queue(), 0, 0, ""));
    phone.reconcile();
    laptop.reconcile();

    drop_link(&phone);
    phone.reconcile();
    assert!(phone.sound.s().playing, "offline, its own session: it plays on");
    laptop.call("skip", json!({ "by": 1 }));
    laptop.reconcile();
    assert!(laptop.sound.s().playing, "the laptop took the sound: the phone that had it is away");

    relink(&phone);
    phone.reconcile();
    assert!(!phone.sound.s().playing, "something is playing elsewhere: this phone's own sound stops");
    let s = phone.session().unwrap();
    assert!(s.outputs("laptop") && s.sounding(), "and the laptop plays on");
    assert_eq!(phone.view()["stats"]["came_back"], 0);
}

/// …and the other way: nothing is sounding when it comes back, so what it
/// was doing on its own becomes the session — its queue, its track and
/// where it had got to, on this phone — and its own sound goes on without a
/// reload.
///
/// Falsified by a look that only ever stops its sound on coming back (the
/// `start` branch taken out): the session stayed paused and away.
#[test]
fn coming_back_playing_to_a_quiet_session_makes_it_this_one() {
    let hall = Hall::new();
    let phone = on(&hall, "phone", true, "phone", None);
    phone.call("start", start(&queue(), 0, 0, ""));
    phone.reconcile();

    drop_link(&phone);
    // On its own it moved to the second track and on into it.
    phone.sound.jump(1, 12_000);
    phone.sound.s().from = "#playlist/Favorites".into();
    phone.reconcile();
    relink(&phone);
    phone.reconcile();
    phone.reconcile();
    assert!(phone.sound.s().playing, "its own sound goes on");
    let s = phone.session().unwrap();
    let a = s.session.clone().unwrap();
    assert!(s.outputs("phone") && s.sounding());
    assert_eq!(a.at, 1);
    assert_eq!(a.from, "#playlist/Favorites");
    assert!(matches!(a.clock, Clock::Playing { at_ms: 12_000, .. }), "{:?}", a.clock);
    assert_eq!(phone.view()["stats"]["came_back"], 1);
    assert_eq!(phone.sound.s().loads, 1, "not loaded again");
}

/// Unlinked, nothing is called: a call made then would be said on the next
/// connection, long after it meant anything; and the stale session, which
/// says playing, is not obeyed — while what plays is drawn from the player
/// itself, as its own session.
///
/// Falsified by `joined` not asking whether the peer is linked: the offline
/// look drove the player from the stale session and resumed it.
#[test]
fn a_phone_unlinked_is_its_own_session() {
    let hall = Hall::new();
    let phone = on(&hall, "phone", true, "phone", None);
    phone.call("start", start(&queue(), 0, 0, ""));
    phone.reconcile();
    drop_link(&phone);
    phone.sound.pause();
    let calls = phone.calls();
    phone.reconcile();
    assert!(!phone.sound.s().playing, "the stale session is not obeyed");
    assert!(phone.session().is_none());
    assert_eq!(phone.calls(), calls, "and nothing is called");
    assert_eq!(phone.player.press(r#""Pause""#.into()).as_deref(), Some(harken_player::NOT_PART));
    let own: Active = serde_json::from_value(phone.view()["active"].clone()).unwrap();
    assert_eq!((own.at, own.queue), (0, queue()), "what plays is the player's own");
    assert!(matches!(own.clock, Clock::Paused { at_ms } if at_ms < 100), "{:?}", own.clock);
    assert_eq!(
        own.output,
        Output::Device {
            id: "phone".into(),
            moving: false
        }
    );
}

/// The player stopping by itself — focus taken by a call, the headphones
/// out — is the session pausing there, said once; the phone does not fight
/// it by resuming.
///
/// Falsified by not asking the player whether it stalled (`take` never
/// read): the reconciler resumed it, and the session went on saying playing.
#[test]
fn a_stall_is_said_and_not_fought() {
    let hall = Hall::new();
    let phone = on(&hall, "phone", true, "phone", None);
    phone.call("start", start(&queue(), 0, 5_000, ""));
    phone.reconcile();
    {
        let mut s = phone.sound.s();
        (s.playing, s.stalled, s.position_ms) = (false, true, 7_000);
    }
    phone.reconcile();
    phone.reconcile();
    assert!(!phone.sound.s().playing, "not resumed");
    assert_eq!(phone.active().clock, Clock::Paused { at_ms: 7_000 });
}

/// A press is a transition, and the ones that need reading are read the
/// desktop's way: the one button pauses what sounds and otherwise plays;
/// with nothing queued it plays what this phone had; "play next" puts the
/// track after this one; a scrub reaches the others.
///
/// Falsified by a toggle that always said `play`: the sounding session did
/// not pause.
#[test]
fn a_press_is_a_transition() {
    let hall = Hall::new();
    let phone = on(&hall, "phone", true, "phone", None);
    let laptop = on(&hall, "laptop", true, "computer", None);
    phone.sound.load(serde_json::to_string(&queue()).unwrap(), 1, 0, true, "#library".into());
    phone.press(Press::Play);
    assert_eq!(phone.active().at, 1, "nothing queued: this phone's own, pressed again");
    laptop.press(Press::Toggle);
    assert!(!phone.session().unwrap().sounding(), "the one button pauses what sounds");
    laptop.press(Press::Toggle);
    assert!(phone.session().unwrap().sounding(), "and plays what does not");
    laptop.press(Press::Next {
        tracks: vec![track("c", 3, 100_000)],
    });
    let a = phone.active();
    assert_eq!(a.queue.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
    assert_eq!(a.at, 1);
    assert!(phone.session().unwrap().outputs("phone"));
    laptop.press(Press::Scrub { at_ms: Some(42_000) });
    assert_eq!(phone.session().unwrap().scrub.get("laptop"), Some(&42_000), "a scrub reaches the phone");
    phone.press(Press::Here);
    assert!(phone.session().unwrap().outputs("phone"), "the sound here already: nothing asked");
}

/// The phone's own volume is its entry in the session: said once a
/// connection (`equip`) — a volume anybody can move, no family, at its
/// media stream's level — and followed whoever moves it; the stream moved by
/// anything else — the buttons, the system's panel — is said once, as
/// `report_volume`. Never what the phone set itself: a stream has fifteen
/// steps, so the 30 the desktop asks for is the fifth, and the 33 that reads
/// back is where the phone left it, not a hand.
///
/// Falsified by a look that compared the stream with the level the session
/// asked for rather than with what it read after its own write (`seen` set
/// to `want`): the phone said 33 back ("and the 33 it reads is not said
/// back").
#[test]
fn the_phones_volume_is_its_entry_and_only_a_hand_is_said() {
    let hall = Hall::new();
    let phone = on(&hall, "phone", true, "phone", Some(9));
    let desk = on(&hall, "desktop", false, "computer", None);
    assert!(!phone.me("phone").can_volume, "nothing has a volume until it says so");
    phone.reconcile();
    let me = phone.me("phone");
    assert!(me.can_volume, "said once it joined");
    assert_eq!(me.volume, 60, "at its stream's level: the ninth of fifteen steps");
    assert_eq!(me.family, None, "and no family: a phone joins no group");
    let calls = phone.calls();
    for _ in 0..3 {
        phone.reconcile();
    }
    assert_eq!(phone.calls(), calls, "once a connection, not once a look");
    assert_eq!(phone.sound.s().stream.unwrap().writes, 0, "and nothing written to the stream");

    let set = |p: Press| desk.press(p);
    set(Press::Loud {
        level: Level::Device { id: "phone".into() },
        change: Change::To { level: 30 },
    });
    phone.reconcile();
    assert_eq!(
        phone.sound.s().stream.unwrap().index,
        5,
        "a level set on the desktop is the stream's nearest step"
    );
    for _ in 0..3 {
        phone.reconcile();
    }
    assert_eq!(phone.me("phone").volume, 30, "and the 33 it reads is not said back");
    assert_eq!(phone.calls(), calls);
    set(Press::Loud {
        level: Level::Device { id: "phone".into() },
        change: Change::Mute { muted: true },
    });
    phone.reconcile();
    assert!(phone.sound.s().stream.unwrap().muted, "a mute is a mute");
    assert_eq!(phone.sound.s().stream.unwrap().index, 5, "the step left where it was");

    // The buttons: the stream moved by a hand.
    {
        let mut s = phone.sound.s();
        let st = s.stream.as_mut().unwrap();
        (st.index, st.muted) = (12, false);
    }
    for _ in 0..3 {
        phone.reconcile();
    }
    assert_eq!(phone.calls(), calls + 1, "said once");
    let me = phone.me("phone");
    assert_eq!((me.volume, me.muted), (80, false));
    let heard = desk.session().unwrap();
    assert_eq!(heard.device("phone").unwrap().volume, 80, "and the desktop hears it");
    assert_eq!(phone.sound.s().stream.unwrap().index, 12, "not fought");

    // A new connection is a room that has never heard what this phone can do.
    drop_link(&phone);
    relink(&phone);
    assert!(!phone.me("phone").can_volume);
    phone.reconcile();
    assert!(phone.me("phone").can_volume, "said again on the new connection");
    assert_eq!(phone.me("phone").volume, 80);
    assert_eq!(phone.calls(), calls + 2);
}
