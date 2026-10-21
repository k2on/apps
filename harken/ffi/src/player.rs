//! A phone's player (S3b): harken's one reconciler (`harken-player`) run
//! here, over the phone's own peer, against a player the phone implements
//! ([`PhoneSound`], a UniFFI callback interface: ExoPlayer on Android). The
//! phone tells [`Player::step`] whenever its peer or its player moved, and
//! draws the view it answers; a press is [`Player::press`].

use std::sync::{Arc, Mutex};

use ark_ffi::Peer;
use harken_domain::{Id, Media, Track};
use harken_player::{Event, Heard, Place, Sound, Vol};

/// A volume, 0 to 100, and its mute.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Loudness {
    pub level: i64,
    pub muted: bool,
}

/// What a phone's player says of itself.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SoundState {
    /// The queue loaded, each track's id (hex).
    pub queue: Vec<String>,
    /// Where in it, or -1 with nothing loaded.
    pub index: i64,
    /// Asked to play (`playWhenReady`), whether or not a buffer is filling.
    pub playing: bool,
    /// Filling a buffer: where it is cannot be said.
    pub buffering: bool,
    pub position_ms: i64,
    /// At the end of the last track of its queue.
    pub ended: bool,
    /// Where the queue was taken from, as whoever loaded it said.
    pub from: String,
}

/// The phone's player, as the reconciler drives it.
#[uniffi::export(with_foreign)]
pub trait PhoneSound: Send + Sync {
    fn state(&self) -> SoundState;
    /// Whether it stopped by itself since the last ask: said once.
    fn take_stalled(&self) -> bool;
    /// The queue loaded, as JSON: a list of `Track`.
    fn tracks(&self) -> String;
    /// Load `queue` (a list of `Track`, JSON), at this track and point.
    fn load(&self, queue: String, at: i64, position_ms: i64, playing: bool, from: String);
    fn jump(&self, at: i64, position_ms: i64);
    fn resume(&self);
    fn pause(&self);
    fn seek(&self, ms: i64);
    /// Step again in `ms`: the moment a play would count.
    fn wake_in(&self, ms: i64);
    /// Its media stream's volume; none where it has none.
    fn volume(&self) -> Option<Loudness>;
    fn set_volume(&self, level: i64, muted: bool);
}

/// A [`PhoneSound`] as a [`Sound`].
struct Phone<'a>(&'a dyn PhoneSound);

impl Sound for Phone<'_> {
    fn heard(&self) -> Heard {
        let s = self.0.state();
        let at = usize::try_from(s.index).ok().filter(|i| *i < s.queue.len());
        Heard {
            queue: s
                .queue
                .into_iter()
                .map(|q| serde_json::from_value::<Id<Media>>(q.into()).unwrap_or_default())
                .collect(),
            at,
            playing: s.playing,
            ready: at.is_some(),
            place: match s.buffering {
                true => Place::Believed(s.position_ms),
                false => Place::Measured {
                    ms: s.position_ms,
                    at: ark_peer::now_ms(),
                },
            },
            ended: s.ended,
            from: s.from,
        }
    }

    fn take(&mut self) -> Option<Event> {
        self.0.take_stalled().then(|| Event::Stalled {
            at_ms: self.0.state().position_ms,
        })
    }

    fn tracks(&self) -> Vec<Track> {
        serde_json::from_str(&self.0.tracks()).unwrap_or_default()
    }

    fn load(&mut self, queue: &[Track], at: usize, position_ms: i64, playing: bool, from: &str) {
        let q = serde_json::to_string(queue).unwrap_or_else(|_| "[]".into());
        self.0.load(q, at as i64, position_ms, playing, from.into());
    }

    fn jump(&mut self, at: usize, position_ms: i64) {
        self.0.jump(at as i64, position_ms);
    }

    fn resume(&mut self) {
        self.0.resume();
    }

    fn pause(&mut self) {
        self.0.pause();
    }

    fn seek(&mut self, ms: i64) {
        self.0.seek(ms);
    }

    fn wake_in(&mut self, ms: i64) {
        self.0.wake_in(ms);
    }

    fn volume(&self) -> Option<Vol> {
        self.0.volume().map(|v| Vol {
            level: v.level,
            muted: v.muted,
        })
    }

    fn set_volume(&mut self, v: Vol) {
        self.0.set_volume(v.level, v.muted);
    }
}

/// A phone's end of the account's listening session.
#[derive(uniffi::Object)]
pub struct Player {
    peer: Arc<Peer>,
    inner: Mutex<harken_player::Player>,
    sound: Mutex<Option<Arc<dyn PhoneSound>>>,
}

#[uniffi::export]
impl Player {
    /// The player of the device `me` (its login's session id) over `peer`,
    /// its plays counted under `dir` (none: in memory).
    #[uniffi::constructor]
    pub fn new(peer: Arc<Peer>, me: String, dir: Option<String>) -> Arc<Player> {
        let counts: Box<dyn ark_peer::Disk> = match dir.map(|d| ark_peer::Files::new(std::path::Path::new(&d).join("player"))) {
            Some(Ok(files)) => Box::new(files),
            _ => Box::new(ark_peer::Memory::default()),
        };
        Arc::new(Player {
            peer,
            inner: Mutex::new(harken_player::Player::new(&me, Some(counts))),
            sound: Mutex::default(),
        })
    }

    /// The player reconciled from now on; none, nothing is.
    pub fn attach(&self, sound: Option<Arc<dyn PhoneSound>>) {
        *self.sound.lock().unwrap_or_else(|e| e.into_inner()) = sound;
    }

    /// Make the player agree with the session and say what happened to it;
    /// what plays, as [`Player::view`] says it.
    pub fn step(&self) -> String {
        let sound = self.sound.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        self.peer.with(|p| match &sound {
            Some(s) => inner.step(p, &mut Phone(&**s)),
            None => drop(inner.refusals(p)),
        });
        self.view_of(&inner, sound.as_deref())
    }

    /// A press (`harken_player::Press`, JSON): none when it was taken, else
    /// why not — "not part of the session" while the phone is its own, when
    /// the caller does it to the player.
    pub fn press(&self, press: String) -> Option<String> {
        let press: harken_player::Press = match serde_json::from_str(&press) {
            Ok(p) => p,
            Err(e) => return Some(format!("a press: {e}")),
        };
        let sound = self.sound.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let local = sound.as_deref().map(Phone);
        self.peer.with(|p| inner.press(p, &press, local.as_ref().map(|l| l as &dyn Sound)))
    }

    /// What plays, as JSON: `{me, joined, linked, epoch, listening, active,
    /// stats, refused}` (`harken_player::Player::view`).
    pub fn view(&self) -> String {
        let sound = self.sound.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        self.view_of(&inner, sound.as_deref())
    }
}

impl Player {
    fn view_of(&self, inner: &harken_player::Player, sound: Option<&dyn PhoneSound>) -> String {
        let local = sound.map(Phone);
        self.peer.with(|p| inner.view(p, local.as_ref().map(|l| l as &dyn Sound))).to_string()
    }
}
