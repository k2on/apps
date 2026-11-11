//! The media player: one sound at a time, loaded from a URL at a position,
//! played and paused, sought, at a rate and a volume, with the lock
//! screen's now-playing record. harken's `PhoneSound` and
//! `harken_player::Sound`, generalised. An update drives it with calls
//! ([`load`], [`play`], …, each answered `Done`); what the player did comes
//! back through the [`MediaListener`] the app's shell registers, which
//! turns each [`MediaEvent`] into the app's message.
//!
//! No native implementation yet (README.md).

use std::sync::{Arc, Mutex};

use ark_module::{Modules, Op, Reply};

#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!();

/// What the lock screen and the system's controls show of what plays.
#[derive(Clone, Debug, PartialEq, Default)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct NowPlayingInfo {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// The artwork's address, when there is one.
    pub artwork: Option<String>,
    pub duration_ms: i64,
}

/// What a player did: what the app's update hears from it.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Enum))]
pub enum MediaEvent {
    /// It has the sound and knows how long it is.
    Loaded {
        duration_ms: i64,
    },
    /// Where it is, now and then while it plays.
    Position {
        at_ms: i64,
    },
    Playing,
    Paused,
    /// It reached the end.
    Ended,
    /// The system's controls asked (the lock screen, a headset).
    Remote {
        command: RemoteCommand,
    },
    Failed {
        why: String,
    },
}

/// What the system's controls can ask of a player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Enum))]
pub enum RemoteCommand {
    Play,
    Pause,
    Next,
    Previous,
    SkipForward,
    SkipBack,
}

/// Where a player tells what it did.
#[cfg_attr(feature = "ffi", uniffi::export(with_foreign))]
pub trait MediaListener: Send + Sync {
    fn heard(&self, event: MediaEvent);
}

/// A player of one sound at a time.
#[cfg_attr(feature = "ffi", uniffi::export(with_foreign))]
pub trait MediaPlayer: Send + Sync {
    /// The sound at `url` (a file or a stream), from `at_ms`, paused.
    fn load(&self, url: String, at_ms: i64);
    fn play(&self);
    fn pause(&self);
    fn seek(&self, at_ms: i64);
    /// 1.0 is as recorded.
    fn set_rate(&self, rate: f32);
    /// 0.0 to 1.0.
    fn set_volume(&self, volume: f32);
    fn set_now_playing(&self, info: Option<NowPlayingInfo>);
    fn set_listener(&self, listener: Option<Arc<dyn MediaListener>>);
}

/// What an update asks of the player.
#[derive(Clone, Debug, PartialEq)]
pub enum MediaOp {
    Load { url: String, at_ms: i64 },
    Play,
    Pause,
    Seek { at_ms: i64 },
    Rate(f32),
    Volume(f32),
    NowPlaying(Option<NowPlayingInfo>),
}

impl Op for MediaOp {
    type Module = dyn MediaPlayer;
    type Output = ();
    const MODULE: &'static str = "ark-media-player";
    fn run(&self, m: &dyn MediaPlayer, reply: Reply<()>) {
        match self {
            MediaOp::Load { url, at_ms } => m.load(url.clone(), *at_ms),
            MediaOp::Play => m.play(),
            MediaOp::Pause => m.pause(),
            MediaOp::Seek { at_ms } => m.seek(*at_ms),
            MediaOp::Rate(r) => m.set_rate(*r),
            MediaOp::Volume(v) => m.set_volume(*v),
            MediaOp::NowPlaying(i) => m.set_now_playing(i.clone()),
        }
        reply.done(())
    }
}

pub fn load(url: impl Into<String>, at_ms: i64) -> MediaOp {
    MediaOp::Load { url: url.into(), at_ms }
}
pub fn play() -> MediaOp {
    MediaOp::Play
}
pub fn pause() -> MediaOp {
    MediaOp::Pause
}
pub fn seek(at_ms: i64) -> MediaOp {
    MediaOp::Seek { at_ms }
}
pub fn set_rate(rate: f32) -> MediaOp {
    MediaOp::Rate(rate)
}
pub fn set_volume(volume: f32) -> MediaOp {
    MediaOp::Volume(volume)
}
pub fn set_now_playing(info: Option<NowPlayingInfo>) -> MediaOp {
    MediaOp::NowPlaying(info)
}

#[derive(Default)]
struct Seen {
    asked: Vec<String>,
    listener: Option<Arc<dyn MediaListener>>,
}

/// The player, faked for tests: what it was asked, as words, and events
/// told to its listener when the test says.
#[derive(Clone, Default)]
pub struct Fake {
    seen: Arc<Mutex<Seen>>,
}

impl Fake {
    pub fn new() -> Fake {
        Fake::default()
    }

    fn seen(&self) -> std::sync::MutexGuard<'_, Seen> {
        self.seen.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// This fake registered as the player.
    pub fn register(&self, modules: &mut Modules) {
        modules.register::<dyn MediaPlayer>(Arc::new(self.clone()));
    }

    /// What the player was asked, as words (`load url 0`, `play`, …).
    pub fn asked(&self) -> Vec<String> {
        self.seen().asked.clone()
    }

    /// `event`, as the player would tell it.
    pub fn hear(&self, event: MediaEvent) {
        let l = self.seen().listener.clone();
        if let Some(l) = l {
            l.heard(event)
        }
    }

    fn note(&self, s: String) {
        self.seen().asked.push(s)
    }
}

impl MediaPlayer for Fake {
    fn load(&self, url: String, at_ms: i64) {
        self.note(format!("load {url} {at_ms}"))
    }
    fn play(&self) {
        self.note("play".into())
    }
    fn pause(&self) {
        self.note("pause".into())
    }
    fn seek(&self, at_ms: i64) {
        self.note(format!("seek {at_ms}"))
    }
    fn set_rate(&self, rate: f32) {
        self.note(format!("rate {rate}"))
    }
    fn set_volume(&self, volume: f32) {
        self.note(format!("volume {volume}"))
    }
    fn set_now_playing(&self, info: Option<NowPlayingInfo>) {
        self.note(format!("now playing {}", info.map(|i| i.title).unwrap_or_default()))
    }
    fn set_listener(&self, listener: Option<Arc<dyn MediaListener>>) {
        self.seen().listener = listener
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Log(Mutex<Vec<MediaEvent>>);
    impl MediaListener for Log {
        fn heard(&self, event: MediaEvent) {
            self.0.lock().unwrap().push(event)
        }
    }

    #[test]
    fn calls_reach_the_player_and_events_its_listener() {
        let fake = Fake::new();
        let mut m = Modules::new();
        fake.register(&mut m);
        m.run(&load("file:///a.mp3", 5), |_| {});
        m.run(&play(), |_| {});
        assert_eq!(fake.asked(), vec!["load file:///a.mp3 5", "play"]);
        let log = Arc::new(Log(Mutex::new(vec![])));
        fake.set_listener(Some(log.clone()));
        fake.hear(MediaEvent::Ended);
        assert_eq!(*log.0.lock().unwrap(), vec![MediaEvent::Ended]);
    }
}
