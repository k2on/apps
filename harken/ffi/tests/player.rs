//! A phone's player (S3b) against harken's own session: real peers of the
//! kernel in a [`Hall`] (a harken server in the process, which runs the
//! domain's transitions and hooks), harken's one reconciler behind the
//! phone's FFI ([`Player`]) over the phone's own queue (`MediaSound`), and
//! a stand-in media player ([`Fake`], what AVPlayer and ExoPlayer are to
//! the phones, through `ark-media-player`'s `NativeMediaPlayer`). The phone's reconciler tests
//! (`RemoteTests.kt`), moved here with the reconciler: the output obeying
//! and nobody else sounding, a track's end said once, the session following
//! the phone and the phone following a seek, a play recorded once by the
//! output alone, both branches of coming back, a phone unlinked being its
//! own session, a stall, a press, the phone's own volume, the lock
//! screen's buttons and what it shows; and the
//! desktop's one case the phone's lacked, a hand-off picking up where the
//! track stopped.
//!
//! Each was falsified once by breaking what it holds in the reconciler
//! (`harken/player/src/reconcile.rs`); the comment beside each says how.

use std::sync::{Arc, Mutex};

use ark_ffi::Peer;
use ark_media_player::phone::{MediaListener, NativeMediaPlayer};
use ark_media_player::{BrowseItem, MediaControls, MediaEvent, NowPlaying, RemoteCommand, RouteChange};
use harken_domain::{Active, Clock, Id, Output, Present, Track};
use harken_ffi::{Hall, Player, PlayerShell};
use harken_player::{Change, Level, Press, SYNC_MS};
use serde_json::{json, Value};

const SERVER: &str = "http://harken.test";

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

/// What a media player was told, as a test reads it.
#[derive(Default)]
struct State {
    /// The track loaded, by its title (`music/<title>.mp3`).
    title: Option<String>,
    playing: bool,
    position_ms: i64,
    loads: u32,
    seeks: u32,
    volume: Option<f32>,
    shown: Option<NowPlaying>,
    controls: Option<MediaControls>,
    listener: Option<Arc<dyn MediaListener>>,
}

/// A media player that does what it is told at once and says so, as a
/// phone's would a moment later; and moves by itself only when a test says
/// (a track's end, an interruption, a read of where it is).
struct Fake(Mutex<State>);

impl Fake {
    fn new() -> Arc<Fake> {
        Arc::new(Fake(Mutex::new(State::default())))
    }

    fn s(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap()
    }

    fn tell(&self, e: MediaEvent) {
        let l = self.s().listener.clone();
        if let Some(l) = l {
            l.heard(e)
        }
    }

    /// Where it is, read: as a player says it now and then while it plays.
    fn at(&self, ms: i64) {
        self.s().position_ms = ms;
        self.tell(MediaEvent::Position { at_ms: ms });
    }

    /// The track played to its end: the queue's next follows by itself.
    fn finish(&self) {
        self.s().playing = false;
        self.tell(MediaEvent::Ended);
    }

    /// The system took the audio (a call), at `ms`: it paused.
    fn interrupt(&self, ms: i64) {
        self.at(ms);
        self.s().playing = false;
        self.tell(MediaEvent::Interrupted);
        self.tell(MediaEvent::Paused);
    }

    /// A button of the system's: the lock screen's, a headset's.
    fn remote(&self, c: RemoteCommand) {
        self.tell(MediaEvent::Remote { command: c });
    }

    fn title(&self) -> Option<String> {
        self.s().title.clone()
    }
}

impl NativeMediaPlayer for Fake {
    fn load(&self, url: String, at_ms: i64) {
        {
            let mut s = self.s();
            let title = url.rsplit('/').next().unwrap_or_default().trim_end_matches(".mp3").to_string();
            (s.title, s.position_ms, s.playing) = (Some(title), at_ms, false);
            s.loads += 1;
        }
        self.tell(MediaEvent::Loaded { duration_ms: 100_000 });
    }

    fn play(&self) {
        self.s().playing = true;
        self.tell(MediaEvent::Playing);
    }

    fn pause(&self) {
        self.s().playing = false;
        self.tell(MediaEvent::Paused);
    }

    fn seek(&self, at_ms: i64) {
        {
            let mut s = self.s();
            s.position_ms = at_ms;
            s.seeks += 1;
        }
        self.tell(MediaEvent::Position { at_ms });
    }

    fn set_rate(&self, _: f32) {}

    fn set_volume(&self, volume: f32) {
        self.s().volume = Some(volume);
    }

    fn stop(&self) {
        let mut s = self.s();
        (s.title, s.playing) = (None, false);
    }

    fn set_now_playing(&self, info: Option<NowPlaying>) {
        self.s().shown = info;
    }

    fn set_controls(&self, controls: MediaControls) {
        self.s().controls = Some(controls);
    }

    fn set_children(&self, _: String, _: Vec<BrowseItem>) {}

    fn set_listener(&self, listener: Option<Arc<dyn MediaListener>>) {
        self.s().listener = listener;
    }
}

/// The phone's end of its player: the wake it was last asked for, and a cover for anything.
#[derive(Default)]
struct Shell(Mutex<Option<i64>>);

impl PlayerShell for Shell {
    fn wake(&self, in_ms: i64) {
        if in_ms > 0 {
            *self.0.lock().unwrap() = Some(in_ms);
        }
    }

    fn cover(&self, album: String, _: String) -> Option<String> {
        Some(format!("{SERVER}/art/{album}.jpg"))
    }
}

struct Device<'h> {
    hall: &'h Hall,
    peer: Arc<Peer>,
    player: Arc<Player>,
    sound: Arc<Fake>,
    shell: Arc<Shell>,
    _dir: tempfile::TempDir,
}

fn on<'h>(hall: &'h Hall, id: &str, can_play: bool, kind: &str) -> Device<'h> {
    let dir = tempfile::tempdir().unwrap();
    let said = json!({ "can_play": can_play, "id": "", "kind": kind, "name": id, "user": "" }).to_string();
    let peer = hall.device(dir.path().to_str().unwrap().into(), "alice".into(), id.into(), said).unwrap();
    let player = Player::new(peer.clone(), id.into(), Some(dir.path().to_str().unwrap().into()));
    let sound = Fake::new();
    let shell = Arc::new(Shell::default());
    player.attach(Some(sound.clone()), SERVER.into(), Some(shell.clone()));
    hall.carry();
    Device {
        hall,
        peer,
        player,
        sound,
        shell,
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

    /// What plays as the phone draws it: the session's, or its own alone.
    fn drawn(&self) -> Active {
        serde_json::from_value(self.view()["active"].clone()).unwrap()
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
    let phone = on(&hall, "phone", true, "phone");
    let desk = on(&hall, "desktop", false, "computer");
    phone.call("start", start(&queue(), 0, 0, "#album/Water%20Music"));
    phone.reconcile();
    assert_eq!(phone.sound.title().as_deref(), Some("a"), "the phone loaded what it pressed");
    assert!(phone.sound.s().playing);
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

    let laptop = on(&hall, "laptop", true, "computer");
    desk.call("transfer", json!({ "to": "laptop" }));
    phone.reconcile();
    laptop.reconcile();
    assert!(!phone.sound.s().playing, "the phone lost the sound and is silent");
    assert!(laptop.sound.s().playing && laptop.sound.title().as_deref() == Some("a"));
    let s = laptop.session().unwrap();
    assert!(s.outputs("laptop") && !s.moving_to("laptop"), "the laptop said it had started");
    assert_eq!(phone.sound.s().loads, 1, "the phone loaded once");
}

/// A hand-off between two players picks up where the track stopped: while
/// the sound is on its way the session stands where it was, not counting a
/// sound nobody hears; the laptop, slow to reconcile, loads the track there
/// and says `started` there; and nobody's position goes back. (The desktop's
/// own reconciler's case, ported to the one reconciler.)
///
/// Falsified by loading where the session's clock counts rather than where
/// the session stands (`a.clock.position(v.now)` for `a.position(v.now)` in
/// `drive`): the laptop, a second and a half late, loaded 1,5xx ms on.
#[test]
fn a_hand_off_picks_up_where_the_track_stopped() {
    let hall = Hall::new();
    let phone = on(&hall, "phone", true, "phone");
    let laptop = on(&hall, "laptop", true, "computer");
    phone.call("start", start(&queue(), 0, 40_000, ""));
    phone.reconcile();
    assert!(phone.sound.s().playing);
    phone.call("transfer", json!({ "to": "laptop" }));
    phone.reconcile();
    assert!(!phone.sound.s().playing, "silent once the sound is handed on");
    let handed = phone.active().position(now());
    assert!((40_000..40_500).contains(&handed), "{handed}");
    std::thread::sleep(std::time::Duration::from_millis(1_500));
    assert_eq!(phone.active().position(now()), handed, "standing where it was");
    laptop.reconcile();
    assert_eq!(laptop.sound.s().position_ms, handed, "loaded where the track stopped");
    laptop.sound.at(handed);
    laptop.reconcile();
    let s = laptop.session().unwrap();
    assert!(s.outputs("laptop") && !s.moving_to("laptop"), "and said it had started");
    let a = s.session.unwrap();
    assert!(
        matches!(a.clock, Clock::Playing { at_ms, .. } if (at_ms - handed).abs() < 100),
        "started there: {:?}",
        a.clock
    );
    assert!(laptop.active().position(now()) >= handed, "and never back");
    phone.reconcile();
    assert!(!phone.sound.s().playing, "and the phone stays quiet");
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
    let phone = on(&hall, "phone", true, "phone");
    phone.call("start", start(&queue(), 0, 0, ""));
    phone.reconcile();
    phone.sound.finish();
    assert_eq!(phone.sound.title().as_deref(), Some("b"), "the next track, by itself");
    let calls = phone.calls();
    phone.reconcile();
    phone.reconcile();
    assert_eq!(phone.active().at, 1, "one track on");
    assert_eq!(phone.calls(), calls + 1, "said once");
    assert_eq!(phone.sound.s().loads, 2, "the next loaded once, by the player itself");
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

/// The phone is right about where it is. A player a second behind a clock
/// nobody touched is followed: the session is told where the player is
/// (`report_position`), and the player is not sent a second ahead. Within
/// the bound nothing is said; a seek on another device is the session
/// moving, and the player goes there.
///
/// Falsified by the old rule, the player always sent where the clock counts
/// (the `moved` test taken out): "not sent ahead" failed.
#[test]
fn the_session_follows_the_phone_and_the_phone_follows_a_seek() {
    let hall = Hall::new();
    let phone = on(&hall, "phone", true, "phone");
    let desk = on(&hall, "desktop", false, "computer");
    phone.call("start", start(&queue(), 0, 40_000, ""));
    phone.reconcile();
    let loaded = phone.sound.s().position_ms;
    assert!((40_000..40_100).contains(&loaded), "loaded where it was pressed: {loaded}");
    let counted = || phone.active().position(now());

    // A second on the clock, and the player has played none of it.
    std::thread::sleep(std::time::Duration::from_millis(1_000));
    phone.sound.at(loaded);
    let seeks = phone.sound.s().seeks;
    phone.reconcile();
    assert_eq!(phone.sound.s().position_ms, loaded, "not sent ahead to where the clock counts");
    assert_eq!(phone.sound.s().seeks, seeks);
    let at = counted();
    assert!((at - loaded).abs() <= SYNC_MS, "the session is where the player is: {at}");
    let calls = phone.calls();
    phone.sound.at(counted());
    phone.reconcile();
    assert_eq!(phone.calls(), calls, "and says it once");

    // Within the bound, nothing either way.
    phone.sound.at(counted() - SYNC_MS / 2);
    phone.reconcile();
    assert_eq!(phone.calls(), calls);
    assert_eq!(phone.sound.s().seeks, seeks);
    phone.sound.at(counted());

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
/// the shell is asked to step when it will be reached.
///
/// Falsified by counting on every look past the threshold (the `counted`
/// flag never written): the second look authored a second play.
#[test]
fn a_play_is_recorded_once_by_the_output_alone() {
    let hall = Hall::new();
    let phone = on(&hall, "phone", true, "phone");
    let laptop = on(&hall, "laptop", true, "computer");
    phone.call("start", start(&queue(), 0, 29_000, "#album/Water%20Music"));
    phone.reconcile();
    assert_eq!(phone.plays().len(), 0, "not yet: half of a hundred seconds is fifty, thirty is first");
    let wake = phone.shell.0.lock().unwrap().unwrap_or(0);
    assert!((900..=1_100).contains(&wake), "a wake at the threshold: {wake}");
    phone.sound.at(30_500);
    phone.call("seek", json!({ "ms": 30_500 }));
    phone.reconcile();
    assert_eq!(phone.plays().len(), 1, "thirty seconds in: a play");
    phone.reconcile();
    assert_eq!(phone.plays().len(), 1, "once per play");
    laptop.sound.at(40_000);
    laptop.reconcile();
    assert_eq!(laptop.view()["stats"]["plays"], 0, "a device watching records nothing");
    assert_eq!(phone.plays()[0]["from"], "#album/Water%20Music");
    assert_eq!(phone.peer.status().pending, 0, "the play reached the server");

    // Offline the phone is its own session, and a play is the log's all the same.
    drop_link(&phone);
    phone.press(Press::Skip { by: 1 });
    phone.reconcile();
    phone.sound.at(31_000);
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
    let phone = on(&hall, "phone", true, "phone");
    let laptop = on(&hall, "laptop", true, "computer");
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
    let phone = on(&hall, "phone", true, "phone");
    phone.call("start", start(&queue(), 0, 0, "#playlist/Favorites"));
    phone.reconcile();

    drop_link(&phone);
    // On its own it moved to the second track and on into it.
    phone.press(Press::Skip { by: 1 });
    phone.sound.at(12_000);
    phone.reconcile();
    relink(&phone);
    phone.sound.at(12_000);
    phone.reconcile();
    phone.reconcile();
    assert!(phone.sound.s().playing, "its own sound goes on");
    let s = phone.session().unwrap();
    let a = s.session.clone().unwrap();
    assert!(s.outputs("phone") && s.sounding());
    assert_eq!(a.at, 1);
    assert_eq!(a.from, "#playlist/Favorites");
    assert!(
        matches!(a.clock, Clock::Playing { at_ms, .. } if (12_000..12_500).contains(&at_ms)),
        "{:?}",
        a.clock
    );
    assert_eq!(phone.view()["stats"]["came_back"], 1);
    assert_eq!(phone.sound.s().loads, 2, "the skip's load, and not loaded again");
}

/// Unlinked, nothing is called: a call made then would be said on the next
/// connection, long after it meant anything; and the stale session, which
/// says playing, is not obeyed — while what plays is drawn from the player
/// itself, as its own session, and a press is done to it.
///
/// Falsified by `joined` not asking whether the peer is linked: the offline
/// look drove the player from the stale session and resumed it.
#[test]
fn a_phone_unlinked_is_its_own_session() {
    let hall = Hall::new();
    let phone = on(&hall, "phone", true, "phone");
    phone.call("start", start(&queue(), 0, 0, ""));
    phone.reconcile();
    drop_link(&phone);
    let calls = phone.calls();
    assert_eq!(phone.player.press(r#""Pause""#.into()), None, "a press alone is the player's own");
    phone.reconcile();
    assert!(!phone.sound.s().playing, "the stale session is not obeyed");
    assert!(phone.session().is_none());
    assert_eq!(phone.calls(), calls, "and nothing is called");
    let own = phone.drawn();
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

/// The player stopped by the system — focus taken by a call, the
/// headphones out — is the session pausing there, said once; the phone does
/// not fight it by resuming.
///
/// Falsified by not asking the player whether it stalled (`take` never
/// read): the reconciler resumed it, and the session went on saying playing.
#[test]
fn a_stall_is_said_and_not_fought() {
    let hall = Hall::new();
    let phone = on(&hall, "phone", true, "phone");
    phone.call("start", start(&queue(), 0, 5_000, ""));
    phone.reconcile();
    phone.sound.interrupt(7_000);
    phone.reconcile();
    phone.reconcile();
    assert!(!phone.sound.s().playing, "not resumed");
    assert_eq!(phone.active().clock, Clock::Paused { at_ms: 7_000 });

    // The headphones out: the same.
    phone.call("play", Value::Null);
    phone.reconcile();
    assert!(phone.sound.s().playing);
    phone.sound.at(9_000);
    phone.sound.s().playing = false;
    phone.sound.tell(MediaEvent::Route {
        change: RouteChange::Unplugged,
    });
    phone.reconcile();
    assert!(!phone.sound.s().playing);
    assert!(matches!(phone.active().clock, Clock::Paused { at_ms } if (9_000..9_200).contains(&at_ms)));
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
    let phone = on(&hall, "phone", true, "phone");
    let laptop = on(&hall, "laptop", true, "computer");
    // Alone first (offline): this phone's own queue, at its second track.
    drop_link(&phone);
    phone.press(Press::Start {
        queue: queue(),
        at: 1,
        position_ms: 0,
        from: "#library".into(),
    });
    phone.press(Press::Pause);
    relink(&phone);
    phone.reconcile();
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

/// The lock screen's buttons and a headset's are presses of the session
/// while the phone is part of it — a pause there pauses whichever device
/// plays — and of its own queue alone; a skip of seconds a seek from where
/// the session stands.
///
/// Falsified by doing a button to the player itself (the `remote_press`
/// loop taken out of `step`): the session went on playing on the laptop.
#[test]
fn the_lock_screens_buttons_are_presses() {
    let hall = Hall::new();
    let phone = on(&hall, "phone", true, "phone");
    let laptop = on(&hall, "laptop", true, "computer");
    laptop.call("start", start(&queue(), 0, 10_000, ""));
    laptop.reconcile();
    phone.reconcile();
    phone.sound.remote(RemoteCommand::Pause);
    phone.reconcile();
    assert!(!phone.session().unwrap().sounding(), "a pause on the lock screen pauses the laptop");
    phone.sound.remote(RemoteCommand::SkipForward { seconds: 30 });
    phone.reconcile();
    assert!(
        matches!(phone.active().clock, Clock::Paused { at_ms } if (40_000..40_200).contains(&at_ms)),
        "{:?}",
        phone.active().clock
    );
    phone.sound.remote(RemoteCommand::Next);
    phone.reconcile();
    assert_eq!(phone.active().at, 1);
    assert_eq!(phone.sound.s().loads, 0, "the phone made no sound of it");

    drop_link(&phone);
    phone.press(Press::Start {
        queue: queue(),
        at: 0,
        position_ms: 0,
        from: String::new(),
    });
    phone.sound.remote(RemoteCommand::Next);
    phone.reconcile();
    assert_eq!(phone.sound.title().as_deref(), Some("b"), "alone, next is the phone's own next");
}

/// What the system shows is what plays: the track, its cover, where it is,
/// whether it moves; while the sound is on another device, where it is;
/// and the controls a track has.
#[test]
fn the_lock_screen_shows_what_plays() {
    let hall = Hall::new();
    let phone = on(&hall, "phone", true, "phone");
    let laptop = on(&hall, "laptop", true, "computer");
    laptop.call("start", start(&queue(), 0, 10_000, ""));
    laptop.reconcile();
    phone.reconcile();
    let shown = phone.sound.s().shown.clone().expect("something shown");
    assert_eq!(shown.title, "a");
    assert_eq!(shown.artist, "Handel · Playing on laptop");
    assert_eq!(shown.album, "Water Music");
    assert_eq!(shown.artwork.as_deref(), Some("http://harken.test/art/Water Music.jpg"));
    assert_eq!(shown.rate, 1.0);
    let c = phone.sound.s().controls.unwrap();
    assert!(c.has_next && c.can_seek && c.skip_forward_s == 0, "{c:?}");
    laptop.call("skip", json!({ "by": 1 }));
    phone.reconcile();
    assert!(!phone.sound.s().controls.unwrap().has_next, "the last track has no next");
}

/// The phone's own volume is its entry in the session: said once a
/// connection (`equip`) — a volume anybody can move, no family, at its
/// player's gain — and followed whoever moves it, never said back.
///
/// Falsified by a look that compared the gain with the level the session
/// asked for rather than with what it read after its own write (`seen` set
/// to `want`): the phone said its own level back.
#[test]
fn the_phones_volume_is_its_entry_and_only_a_hand_is_said() {
    let hall = Hall::new();
    let phone = on(&hall, "phone", true, "phone");
    let desk = on(&hall, "desktop", false, "computer");
    assert!(!phone.me("phone").can_volume, "nothing has a volume until it says so");
    phone.reconcile();
    let me = phone.me("phone");
    assert!(me.can_volume, "said once it joined");
    assert_eq!(me.volume, 100, "at its player's gain");
    assert_eq!(me.family, None, "and no family: a phone joins no group");
    let calls = phone.calls();
    for _ in 0..3 {
        phone.reconcile();
    }
    assert_eq!(phone.calls(), calls, "once a connection, not once a look");
    assert_eq!(phone.sound.s().volume, None, "and nothing written to the player");

    let set = |p: Press| desk.press(p);
    set(Press::Loud {
        level: Level::Device { id: "phone".into() },
        change: Change::To { level: 30 },
    });
    phone.reconcile();
    assert_eq!(phone.sound.s().volume, Some(0.3), "a level set on the desktop is the player's gain");
    for _ in 0..3 {
        phone.reconcile();
    }
    assert_eq!(phone.me("phone").volume, 30, "and not said back");
    assert_eq!(phone.calls(), calls);
    set(Press::Loud {
        level: Level::Device { id: "phone".into() },
        change: Change::Mute { muted: true },
    });
    phone.reconcile();
    assert_eq!(phone.sound.s().volume, Some(0.0), "a mute is silence");

    // A new connection is a room that has never heard what this phone can do.
    drop_link(&phone);
    relink(&phone);
    assert!(!phone.me("phone").can_volume);
    phone.reconcile();
    assert!(phone.me("phone").can_volume, "said again on the new connection");
    assert_eq!(phone.me("phone").volume, 30);
    assert_eq!(phone.calls(), calls + 1);
}

/// The peer tells the app that something moved at once, on the thread that
/// moved it, and an app that drains inline (its tests do) steps again from
/// inside the step that moved it: that step answers the view last made, and
/// nothing waits on itself.
///
/// Falsified by a step that took its lock by waiting (`lock` for
/// `try_lock`): the nested step never returned.
#[test]
fn a_step_told_of_its_own_change_does_not_wait_on_itself() {
    struct Again(Mutex<Option<Arc<Player>>>);
    impl ark_ffi::Listener for Again {
        fn changed(&self) {
            let p = self.0.lock().unwrap().clone();
            if let Some(p) = p {
                p.step();
            }
        }
    }
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let hall = Hall::new();
        let phone = on(&hall, "phone", true, "phone");
        let again = Arc::new(Again(Mutex::new(Some(phone.player.clone()))));
        // A play recorded is a change of the log, which a view the app keeps sees.
        let _plays = phone.peer.view("recently_played".into(), "null".into()).unwrap();
        phone.peer.set_listener(Some(again.clone()));
        phone.call("start", start(&queue(), 0, 31_000, ""));
        phone.player.step();
        *again.0.lock().unwrap() = None;
        let _ = done.send(phone.sound.s().loads);
    });
    let loads = finished
        .recv_timeout(std::time::Duration::from_secs(20))
        .expect("a step waited on itself");
    assert_eq!(loads, 1);
}
