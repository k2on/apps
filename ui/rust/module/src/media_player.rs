//! The media player's port: one sound at a time, loaded from a URL at a
//! position, played, paused, sought, at a rate and a volume, with what the
//! system shows of it (the lock screen, the notification, Control Center,
//! a headset's buttons, a car's and a watch's now-playing) and a tree a car
//! or a watch browses. Its implementations — AVPlayer on the iPhone, the
//! iPad and the Mac, Media3 in a media session service on Android, an
//! `<audio>` element and the Media Session API in the browser, the
//! desktop's own decoder with MPRIS on Linux — and its fake are the
//! module's (modules/ark-media-player, `ark-media-player`).
//!
//! An update asks with the calls ([`load`], [`play`], [`set_now_playing`],
//! …), each a [`MediaOp`] answered `()` at once. What the player did, and
//! what the system's controls asked of it, comes back through the
//! [`MediaListener`] the shell registers ([`MediaEvent`]), which the shell
//! turns into the app's message.
//!
//! **The player never acts on a control by itself.** A press on the lock
//! screen, a headset's button, a car's skip are each a
//! [`MediaEvent::Remote`], and the app decides what it means: a play may be
//! a transition of an account's session as easily as the player's own
//! `play`. What the player does by itself is what the system does to it:
//! an interruption (a call), the headphones out, the end of the sound —
//! each said ([`MediaEvent::Interrupted`], [`MediaEvent::Route`],
//! [`MediaEvent::Ended`]).

use crate::{Op, Reply};

/// What the system shows of what plays: the lock screen, the notification,
/// a car's and a watch's now-playing. Set for a sound this player makes, or
/// for one made elsewhere (a remote for another device): `position_ms` and
/// `rate` are then what is shown; while this player has a sound loaded, its
/// own position and rate are shown instead.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// The artwork's address (`https:`, `file:`, `content:`), when there is one.
    pub artwork: Option<String>,
    /// What stands in for the artwork while there is none, or none to be
    /// had (no address, or one that cannot be fetched): the seed of arkui's
    /// derived square (`Art.pick` over the palette the app gave the player),
    /// so the system never draws an empty tile. None, and nothing stands in.
    pub placeholder: Option<String>,
    pub duration_ms: i64,
    pub position_ms: i64,
    /// 1.0 playing as recorded, 0.0 paused.
    pub rate: f32,
}

/// Which of the system's controls are offered: the queue's hint (whether
/// there is a next and a previous), skipping by seconds (an episode's), a
/// scrubber, the rate, and adding to the queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct MediaControls {
    pub has_next: bool,
    pub has_previous: bool,
    /// Seconds a skip forward moves; 0 offers none.
    pub skip_forward_s: u32,
    /// Seconds a skip back moves; 0 offers none.
    pub skip_back_s: u32,
    /// The system's scrubber moves the position.
    pub can_seek: bool,
    pub can_rate: bool,
    /// A car's or a watch's controller may add to the queue: each addition
    /// said as a [`MediaEvent::Add`], for the app to do.
    pub can_add: bool,
}

/// What the system's controls ask (the lock screen, a headset, a car, a
/// watch, the desktop's media keys): the app decides what each means.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RemoteCommand {
    Play,
    Pause,
    /// A headset's one button.
    Toggle,
    /// Stopped, as the system says it; most apps mean a pause.
    Stop,
    Next,
    Previous,
    SkipForward {
        seconds: u32,
    },
    SkipBack {
        seconds: u32,
    },
    SeekTo {
        at_ms: i64,
    },
    Rate {
        rate: f32,
    },
}

/// Why the audio's route changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteChange {
    /// The device it played through went (the headphones out, a Bluetooth
    /// speaker gone): the player paused itself, as every phone does.
    Unplugged,
    /// A device came (headphones in): nothing was done.
    Plugged,
    /// Anything else (a car's audio, a category change).
    Other,
}

/// One row of the tree a car or a watch browses: a container to open
/// (`browsable`), something to play (`playable`), or both (an album).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct BrowseItem {
    /// The app's own id: what a [`MediaEvent::Browse`] or
    /// [`MediaEvent::PlayItem`] names it by.
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub artwork: Option<String>,
    pub playable: bool,
    pub browsable: bool,
}

/// The browse tree's root, as a [`MediaEvent::Browse`] names it.
pub const ROOT: &str = "";

/// What a player did, and what the system asked of it: what the app's
/// update hears.
#[derive(Clone, Debug, PartialEq)]
pub enum MediaEvent {
    /// It has the sound and knows how long it is (0: a stream with no end).
    Loaded {
        duration_ms: i64,
    },
    /// Where it is: now and then while it plays, and after a seek.
    Position {
        at_ms: i64,
    },
    /// Sounding.
    Playing,
    /// Asked to play, and filling its buffer.
    Waiting,
    Paused,
    /// It reached the end of the sound.
    Ended,
    /// The system's controls asked.
    Remote {
        command: RemoteCommand,
    },
    /// It could not load or go on, in the platform's words.
    Failed {
        why: String,
    },
    /// Another sound took the audio (a call, an alarm, another app): the
    /// player was paused or ducked by the system.
    Interrupted,
    /// The interruption is over, and the player went on by itself.
    Resumed,
    /// The audio's route changed.
    Route {
        change: RouteChange,
    },
    /// A car or a watch opened `parent` ([`ROOT`] first): the app answers
    /// with [`set_children`].
    Browse {
        parent: String,
    },
    /// A car or a watch chose `id` to play.
    PlayItem {
        id: String,
    },
    /// A car or a watch added `ids` (the app's ids, as it handed them) to
    /// the queue: right after what plays (`next`), or at its end. Offered
    /// only while the controls say [`MediaControls::can_add`].
    Add {
        ids: Vec<String>,
        next: bool,
    },
}

/// Where a player tells what it did.
pub trait MediaListener: Send + Sync {
    fn heard(&self, event: MediaEvent);
}

/// A player of one sound at a time.
pub trait MediaPlayer: Send + Sync {
    /// The sound at `url` (a file or a stream), from `at_ms`, paused.
    fn load(&self, url: String, at_ms: i64);
    fn play(&self);
    fn pause(&self);
    fn seek(&self, at_ms: i64);
    /// 1.0 is as recorded.
    fn set_rate(&self, rate: f32);
    /// 0.0 to 1.0: this player's own gain, never the system's volume.
    fn set_volume(&self, volume: f32);
    /// Nothing loaded any more, and the audio let go.
    fn stop(&self);
    /// What the system shows; none, nothing is.
    fn set_now_playing(&self, info: Option<NowPlaying>);
    fn set_controls(&self, controls: MediaControls);
    /// What `parent` holds, as a [`MediaEvent::Browse`] asked.
    fn set_children(&self, parent: String, items: Vec<BrowseItem>);
    fn set_listener(&self, listener: Option<std::sync::Arc<dyn MediaListener>>);
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
    Stop,
    NowPlaying(Option<NowPlaying>),
    Controls(MediaControls),
    Children { parent: String, items: Vec<BrowseItem> },
}

impl MediaOp {
    /// `self` done to `m`.
    pub fn apply(&self, m: &dyn MediaPlayer) {
        match self {
            MediaOp::Load { url, at_ms } => m.load(url.clone(), *at_ms),
            MediaOp::Play => m.play(),
            MediaOp::Pause => m.pause(),
            MediaOp::Seek { at_ms } => m.seek(*at_ms),
            MediaOp::Rate(r) => m.set_rate(*r),
            MediaOp::Volume(v) => m.set_volume(*v),
            MediaOp::Stop => m.stop(),
            MediaOp::NowPlaying(i) => m.set_now_playing(i.clone()),
            MediaOp::Controls(c) => m.set_controls(*c),
            MediaOp::Children { parent, items } => m.set_children(parent.clone(), items.clone()),
        }
    }
}

impl Op for MediaOp {
    type Module = dyn MediaPlayer;
    type Output = ();
    const MODULE: &'static str = "ark-media-player";
    fn run(&self, m: &dyn MediaPlayer, reply: Reply<()>) {
        self.apply(m);
        reply.done(())
    }
}

/// The sound at `url`, from `at_ms`, paused.
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
pub fn stop() -> MediaOp {
    MediaOp::Stop
}
pub fn set_now_playing(info: Option<NowPlaying>) -> MediaOp {
    MediaOp::NowPlaying(info)
}
pub fn set_controls(controls: MediaControls) -> MediaOp {
    MediaOp::Controls(controls)
}
/// What `parent` holds, answering a [`MediaEvent::Browse`].
pub fn set_children(parent: impl Into<String>, items: Vec<BrowseItem>) -> MediaOp {
    MediaOp::Children {
        parent: parent.into(),
        items,
    }
}
