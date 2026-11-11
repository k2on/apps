//! The reconciler over this device's own player ([`MediaSound`]) over the
//! media player's port, faked (`ark_media_player::Fake`): what the session
//! asks reaches the player as calls (a track's URL loaded, played, paused,
//! sought), and what the player and the system say back (loaded, playing,
//! the end of a track, an interruption, the headphones out, a failure, the
//! lock screen's buttons) comes out as the session's transitions, a stall,
//! the next track, or a press.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use ark_media_player::{Fake, MediaControls, MediaEvent, MediaPlayer, NowPlaying, RemoteCommand, RouteChange};
use harken_domain::host::{Kind, Listening};
use harken_domain::*;
use harken_player::{remote_press, MediaSound, Press, Reconciler, Repeat, Say, Sound, View};

const ME: &str = "phone";
const NOW: i64 = 1_700_000_000_000;
const SERVER: &str = "http://harken.local:8080";

fn track(n: u8) -> Track {
    let mut id = [0; 16];
    id[15] = n;
    Track {
        album: "Kind of Blue".into(),
        creator: "Miles Davis".into(),
        duration_ms: 60_000,
        file: format!("music/{n}.mp3"),
        id: Id::new(id),
        title: format!("Track {n}"),
    }
}

fn url(n: u8) -> String {
    format!("{SERVER}/media/music/{n}.mp3")
}

fn device(id: &str) -> Present {
    Present {
        can_play: true,
        can_volume: true,
        family: None,
        follows: None,
        id: id.into(),
        kind: Kind::Phone.name().into(),
        muted: false,
        name: id.into(),
        user: "alice".into(),
        volume: 100,
    }
}

/// The session moving to this device: a hand-off, until it says `started`.
fn handed(n: usize, at: i64, clock: Clock) -> Listening {
    let mut s = session(n, at, clock);
    if let Some(a) = &mut s.session {
        a.output = Output::Device { id: ME.into(), moving: true };
    }
    s
}

fn session(n: usize, at: i64, clock: Clock) -> Listening {
    Listening {
        devices: vec![device(ME)],
        scrub: Default::default(),
        session: Some(Active {
            at,
            clock,
            from: String::new(),
            output: Output::Device {
                id: ME.into(),
                moving: false,
            },
            queue: (0..n as u8).map(track).collect(),
        }),
    }
}

#[derive(Default)]
struct Said(Vec<&'static str>);

impl Say for Said {
    fn call(&mut self, transition: &'static str, _: Vec<u8>, _: Option<i64>, _: &str) -> Result<(), String> {
        self.0.push(transition);
        Ok(())
    }
    fn play(&mut self, _: RecordPlay) -> Result<(), String> {
        Ok(())
    }
    fn progress(&mut self, _: ReportProgress) -> Result<(), String> {
        Ok(())
    }
}

/// The reconciler, this device's player over the fake, the clock and the
/// wakes it asked for.
struct Rig {
    r: Reconciler,
    fake: Fake,
    sound: MediaSound,
    clock: Arc<AtomicI64>,
    wakes: Arc<Mutex<Vec<i64>>>,
}

impl Rig {
    fn new() -> Rig {
        let fake = Fake::new();
        let clock = Arc::new(AtomicI64::new(NOW));
        let wakes = Arc::new(Mutex::new(vec![]));
        let (c, w) = (clock.clone(), wakes.clone());
        let sound = MediaSound::new(
            Arc::new(fake.clone()) as Arc<dyn MediaPlayer>,
            SERVER,
            move |ms| w.lock().unwrap().push(ms),
            move || c.load(Ordering::SeqCst),
        );
        Rig {
            r: Reconciler::new(ME, None),
            fake,
            sound,
            clock,
            wakes,
        }
    }

    fn at(&self, ms: i64) {
        self.clock.store(ms, Ordering::SeqCst)
    }

    fn look(&mut self, s: &Listening) -> Vec<&'static str> {
        let mut said = Said::default();
        let v = View {
            me: ME,
            joined: true,
            epoch: 1,
            listening: s,
            now: self.clock.load(Ordering::SeqCst),
        };
        self.r.reconcile(&v, &mut self.sound, &mut said);
        said.0
    }

    /// Handed `n` tracks from `at`, loaded, and sounding.
    fn sounding(&mut self, n: usize, at: i64) -> Listening {
        let s = handed(n, at, Clock::Playing { at_ms: 0, since: NOW });
        self.look(&s);
        assert_eq!(self.fake.take(), [format!("load {} 0", url(at as u8)), "play".into()]);
        self.fake.hear(MediaEvent::Loaded { duration_ms: 60_000 });
        self.fake.hear(MediaEvent::Playing);
        assert_eq!(self.look(&s), ["started"]);
        session(n, at, Clock::Playing { at_ms: 0, since: NOW })
    }
}

/// The session naming this device its output loads the track's file from
/// the server, plays it, and once the player has it and sounds, says
/// `started`; not before, while it fills its buffer.
#[test]
fn a_hand_off_is_loaded_played_and_said_once_it_sounds() {
    let mut rig = Rig::new();
    let s = handed(2, 0, Clock::Playing { at_ms: 0, since: NOW });
    assert!(rig.look(&s).is_empty(), "nothing to say before it is loaded");
    assert_eq!(rig.fake.take(), [format!("load {} 0", url(0)), "play".into()]);
    assert!(rig.look(&s).is_empty(), "still filling its buffer");
    assert_eq!(rig.fake.take(), Vec::<String>::new(), "and not loaded again");
    rig.fake.hear(MediaEvent::Waiting);
    rig.fake.hear(MediaEvent::Loaded { duration_ms: 60_000 });
    rig.fake.hear(MediaEvent::Playing);
    assert_eq!(rig.look(&s), ["started"]);
    assert!(!rig.wakes.lock().unwrap().is_empty(), "the shell was told to step as it moved");
}

/// A session paused, sought, resumed and moved to another track is the
/// player paused, sought, played and loaded with that track.
#[test]
fn the_session_moving_is_told_to_the_player() {
    let mut rig = Rig::new();
    rig.sounding(2, 0);
    rig.at(NOW + 5_000);
    rig.look(&session(2, 0, Clock::Paused { at_ms: 5_000 }));
    assert_eq!(rig.fake.take(), ["pause"]);
    rig.fake.hear(MediaEvent::Paused);
    rig.look(&session(2, 0, Clock::Paused { at_ms: 40_000 }));
    assert_eq!(rig.fake.take(), ["seek 40000"]);
    rig.at(NOW + 6_000);
    rig.look(&session(
        2,
        0,
        Clock::Playing {
            at_ms: 40_000,
            since: NOW + 6_000,
        },
    ));
    assert_eq!(rig.fake.take(), ["play"]);
    rig.look(&session(
        2,
        1,
        Clock::Playing {
            at_ms: 0,
            since: NOW + 6_000,
        },
    ));
    assert_eq!(rig.fake.take(), [format!("load {} 0", url(1)), "play".into()]);
}

/// A track that ends is followed by the next by itself, which the session
/// hears as `advance`; the last one's end is the queue's.
#[test]
fn the_end_of_a_track_plays_the_next_and_is_said_as_advance() {
    let mut rig = Rig::new();
    let s = rig.sounding(2, 0);
    rig.at(NOW + 60_000);
    rig.fake.hear(MediaEvent::Ended);
    assert_eq!(rig.fake.take(), [format!("load {} 0", url(1)), "play".into()]);
    assert_eq!(rig.look(&s), ["advance"]);
    rig.fake.hear(MediaEvent::Loaded { duration_ms: 60_000 });
    rig.fake.hear(MediaEvent::Playing);
    rig.fake.hear(MediaEvent::Ended);
    assert_eq!(rig.fake.take(), Vec::<String>::new(), "nothing after the last");
    assert!(rig.sound.heard().ended);
}

/// What the system does to the player — a call, the headphones out, a
/// stream that failed — is a stall, said once at its point; an
/// interruption the player went on from is a `play`.
#[test]
fn an_interruption_or_the_headphones_out_is_a_stall() {
    let mut rig = Rig::new();
    let s = rig.sounding(1, 0);
    rig.fake.hear(MediaEvent::Position { at_ms: 4_200 });
    rig.fake.hear(MediaEvent::Interrupted);
    rig.fake.hear(MediaEvent::Paused);
    assert_eq!(rig.look(&s), ["stalled"]);
    assert!(rig.look(&s).is_empty(), "said once");
    let paused = session(1, 0, Clock::Paused { at_ms: 4_200 });
    assert!(rig.look(&paused).is_empty());
    rig.fake.hear(MediaEvent::Resumed);
    rig.fake.hear(MediaEvent::Playing);
    assert_eq!(rig.look(&paused), ["play"]);

    let mut rig = Rig::new();
    let s = rig.sounding(1, 0);
    rig.fake.hear(MediaEvent::Route {
        change: RouteChange::Unplugged,
    });
    assert_eq!(rig.look(&s), ["stalled"]);
    rig.fake.hear(MediaEvent::Route {
        change: RouteChange::Plugged,
    });
    assert!(rig.look(&session(1, 0, Clock::Paused { at_ms: 0 })).is_empty(), "plugged in is nothing");

    let mut rig = Rig::new();
    let s = rig.sounding(1, 0);
    rig.fake.hear(MediaEvent::Failed {
        why: "no route to host".into(),
    });
    assert_eq!(rig.look(&s), ["stalled"]);
    assert_eq!(rig.sound.failed().as_deref(), Some("no route to host"));
}

/// A pause the player was asked for is not a stall.
#[test]
fn a_pause_asked_for_is_not_a_stall() {
    let mut rig = Rig::new();
    rig.sounding(1, 0);
    let paused = session(1, 0, Clock::Paused { at_ms: 0 });
    rig.look(&paused);
    assert_eq!(rig.fake.take(), ["pause"]);
    rig.fake.hear(MediaEvent::Paused);
    assert!(rig.look(&paused).is_empty());
}

/// The lock screen's and a headset's buttons are kept for the shell and
/// are presses: a skip of seconds a seek from where it is.
#[test]
fn the_system_s_controls_are_presses() {
    let rig = Rig::new();
    rig.fake.hear(MediaEvent::Remote {
        command: RemoteCommand::Toggle,
    });
    rig.fake.hear(MediaEvent::Remote {
        command: RemoteCommand::SkipForward { seconds: 30 },
    });
    let asked = rig.sound.take_remote();
    assert_eq!(asked, [RemoteCommand::Toggle, RemoteCommand::SkipForward { seconds: 30 }]);
    assert!(rig.sound.take_remote().is_empty(), "taken once");
    assert_eq!(remote_press(asked[0], 0), Some(Press::Toggle));
    assert_eq!(remote_press(asked[1], 10_000), Some(Press::Seek { ms: 40_000 }));
    assert_eq!(remote_press(RemoteCommand::SkipBack { seconds: 15 }, 10_000), Some(Press::Seek { ms: 0 }));
    assert_eq!(remote_press(RemoteCommand::Stop, 0), Some(Press::Pause));
    assert_eq!(remote_press(RemoteCommand::Previous, 0), Some(Press::Skip { by: -1 }));
    assert_eq!(remote_press(RemoteCommand::Rate { rate: 1.5 }, 0), None);
}

/// Alone, a press is done to the player's own queue: a track pressed loads
/// it, next and previous move through it (previous a few seconds in is the
/// start again), and the queue's end with repeat on is its start.
#[test]
fn alone_a_press_is_done_to_the_queue() {
    let mut rig = Rig::new();
    let queue: Vec<Track> = (0..3).map(track).collect();
    assert!(rig.sound.alone(&Press::Start {
        queue: queue.clone(),
        at: 1,
        position_ms: 0,
        from: "album/kind-of-blue".into(),
    }));
    assert_eq!(rig.fake.take(), [format!("load {} 0", url(1)), "play".into()]);
    rig.fake.hear(MediaEvent::Loaded { duration_ms: 60_000 });
    rig.fake.hear(MediaEvent::Playing);
    rig.sound.alone(&Press::Skip { by: 1 });
    assert_eq!(rig.fake.take(), [format!("load {} 0", url(2)), "play".into()]);
    rig.fake.hear(MediaEvent::Playing);
    rig.at(NOW + 10_000);
    rig.sound.alone(&Press::Skip { by: -1 });
    assert_eq!(rig.fake.take(), ["seek 0"], "ten seconds in, previous is the start");
    rig.sound.alone(&Press::Skip { by: -1 });
    assert_eq!(rig.fake.take(), [format!("load {} 0", url(1)), "play".into()]);
    rig.sound.alone(&Press::Toggle);
    assert_eq!(rig.fake.take(), ["pause"]);
    assert!(!rig.sound.heard().playing);

    rig.sound.alone(&Press::Jump { index: 2 });
    rig.fake.take();
    rig.sound.alone(&Press::Skip { by: 1 });
    assert_eq!(rig.fake.take(), Vec::<String>::new(), "the end, repeat off");
    rig.sound.cycle_repeat();
    assert_eq!(rig.sound.repeat(), Repeat::All);
    rig.sound.alone(&Press::Skip { by: 1 });
    assert_eq!(rig.fake.take(), [format!("load {} 0", url(0))], "paused, the start again, still paused");

    assert!(!rig.sound.alone(&Press::Transfer { to: "laptop".into() }), "not the player's");
    assert_eq!(rig.sound.heard().from, "album/kind-of-blue");
}

/// Alone, the queue is edited in place around what plays.
#[test]
fn alone_the_queue_is_edited_around_what_plays() {
    let mut rig = Rig::new();
    rig.sound.alone(&Press::Next { tracks: vec![track(0)] });
    assert_eq!(rig.fake.take(), [format!("load {} 0", url(0)), "play".into()], "nothing queued: played");
    rig.sound.alone(&Press::Append { tracks: vec![track(2)] });
    rig.sound.alone(&Press::Next { tracks: vec![track(1)] });
    let ids = |s: &MediaSound| s.tracks().iter().map(|t| t.title.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&rig.sound), ["Track 0", "Track 1", "Track 2"]);
    rig.sound.alone(&Press::Move { from: 0, to: 2 });
    assert_eq!(ids(&rig.sound), ["Track 1", "Track 2", "Track 0"]);
    assert_eq!(rig.sound.heard().at, Some(2), "what plays moved with its row");
    rig.sound.alone(&Press::Remove { index: 2 });
    assert_eq!(ids(&rig.sound).len(), 3, "what plays is not removed");
    rig.sound.alone(&Press::Remove { index: 0 });
    assert_eq!(ids(&rig.sound), ["Track 2", "Track 0"]);
    assert_eq!(rig.sound.heard().at, Some(1));
    assert_eq!(rig.fake.take(), Vec::<String>::new(), "none of it reloads the sound");
}

/// Alone, repeat one plays the same track again at its end.
#[test]
fn repeat_one_plays_the_track_again() {
    let mut rig = Rig::new();
    rig.sound.alone(&Press::Start {
        queue: vec![track(0), track(1)],
        at: 0,
        position_ms: 0,
        from: String::new(),
    });
    rig.fake.take();
    rig.sound.cycle_repeat();
    rig.sound.cycle_repeat();
    assert_eq!(rig.sound.repeat(), Repeat::One);
    rig.fake.hear(MediaEvent::Ended);
    assert_eq!(rig.fake.take(), [format!("load {} 0", url(0)), "play".into()]);
}

/// Shuffling reorders only what is still to come.
#[test]
fn shuffle_reorders_what_is_to_come() {
    let mut rig = Rig::new();
    rig.sound.alone(&Press::Start {
        queue: (0..4).map(track).collect(),
        at: 1,
        position_ms: 0,
        from: String::new(),
    });
    rig.sound.shuffle(|n| (0..n).rev().collect());
    let ids: Vec<String> = rig.sound.tracks().iter().map(|t| t.title.clone()).collect();
    assert_eq!(ids, ["Track 0", "Track 1", "Track 3", "Track 2"]);
    assert!(rig.sound.shuffled());
}

/// The volume is the player's own gain, muted as nothing.
#[test]
fn a_volume_is_the_player_s_gain() {
    let mut rig = Rig::new();
    rig.sound.set_volume(harken_player::Vol { level: 40, muted: false });
    rig.sound.set_volume(harken_player::Vol { level: 40, muted: true });
    assert_eq!(rig.fake.take(), ["volume 0.4", "volume 0"]);
    // Alone, a volume pressed for this device is the same gain.
    use harken_player::{Change, Level};
    rig.sound.alone(&Press::Loud {
        level: Level::Here,
        change: Change::To { level: 70 },
    });
    rig.sound.alone(&Press::Loud {
        level: Level::Here,
        change: Change::By { delta: -20 },
    });
    assert_eq!(rig.fake.take(), ["volume 0.7", "volume 0.5"]);
    assert_eq!(rig.sound.volume(), Some(harken_player::Vol { level: 50, muted: false }));
}

/// What the system shows is told when it changes, not at every step: a
/// position that only played on is not news.
#[test]
fn what_the_system_shows_is_told_when_it_changes() {
    let rig = Rig::new();
    let info = |ms: i64, rate: f32| NowPlaying {
        title: "So What".into(),
        artist: "Miles Davis".into(),
        album: "Kind of Blue".into(),
        artwork: None,
        duration_ms: 545_000,
        position_ms: ms,
        rate,
    };
    let c = MediaControls {
        has_next: true,
        can_seek: true,
        ..MediaControls::default()
    };
    rig.sound.show(Some(info(0, 1.0)), c);
    assert_eq!(rig.fake.now_playing(), Some(info(0, 1.0)));
    rig.at(NOW + 5_000);
    rig.sound.show(Some(info(5_000, 1.0)), c);
    assert_eq!(rig.fake.now_playing(), Some(info(0, 1.0)), "played on: not told");
    rig.sound.show(Some(info(30_000, 1.0)), c);
    assert_eq!(rig.fake.now_playing(), Some(info(30_000, 1.0)), "sought: told");
    rig.sound.show(Some(info(30_000, 0.0)), c);
    assert_eq!(rig.fake.now_playing(), Some(info(30_000, 0.0)), "paused: told");
    rig.sound.show(None, c);
    assert_eq!(rig.fake.now_playing(), None);
    assert_eq!(rig.fake.controls(), c);
}

/// A car's browse and its choice are kept for the shell.
#[test]
fn a_car_s_asks_are_kept_for_the_shell() {
    let rig = Rig::new();
    rig.fake.hear(MediaEvent::Browse { parent: String::new() });
    rig.fake.hear(MediaEvent::PlayItem { id: "album/1".into() });
    assert_eq!(
        rig.sound.take_asks(),
        [
            MediaEvent::Browse { parent: String::new() },
            MediaEvent::PlayItem { id: "album/1".into() }
        ]
    );
}

/// A paused player is idle: once the pause has landed, looking again and
/// again says nothing, asks the player nothing, shows the system nothing
/// new and asks for no step later — nothing keeps a screen that waits for
/// quiet (the screenshots' ready signal) from being quiet.
#[test]
fn a_paused_player_is_idle() {
    let mut rig = Rig::new();
    rig.sounding(1, 0);
    rig.at(NOW + 7_000);
    let paused = session(1, 0, Clock::Paused { at_ms: 7_000 });
    rig.look(&paused);
    assert_eq!(rig.fake.take(), ["pause"]);
    rig.fake.hear(MediaEvent::Paused);
    let info = NowPlaying {
        title: "Track 0".into(),
        duration_ms: 60_000,
        position_ms: 7_000,
        rate: 0.0,
        ..NowPlaying::default()
    };
    rig.sound.show(Some(info.clone()), MediaControls::default());
    rig.wakes.lock().unwrap().clear();
    for s in 1..=10 {
        rig.at(NOW + 7_000 + s * 1_000);
        assert!(rig.look(&paused).is_empty(), "nothing said");
        rig.sound.show(Some(info.clone()), MediaControls::default());
    }
    assert_eq!(rig.fake.take(), Vec::<String>::new(), "nothing asked of the player");
    assert_eq!(*rig.wakes.lock().unwrap(), Vec::<i64>::new(), "no step asked for");
    assert_eq!(rig.sound.heard().place.ms(), Some(7_000), "and it stands where it paused");
}
