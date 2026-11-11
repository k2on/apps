//! A phone's player (S3b): harken's one reconciler (`harken-player`) run
//! here, over the phone's own peer, against this phone's own player — a
//! queue over the phone's media player (`harken_player::MediaSound` over
//! `ark-media-player`'s `NativeMediaPlayer`: AVPlayer on an iPhone, Media3
//! on Android). The phone attaches its media player ([`Player::attach`])
//! and steps whenever it is told to ([`PlayerShell::wake`]), and draws the
//! view a step answers; a press is [`Player::press`] — of the session while
//! the phone is part of it, of its own queue alone.
//!
//! The lock screen, a headset and the notification are the media player's
//! too: what they ask comes back here and is pressed the same way, and
//! what plays is shown to them from the view each step.

use std::sync::{Arc, Mutex};

use ark_ffi::Peer;
use ark_media_player::phone::NativeMediaPlayer;
use ark_media_player::{MediaControls, NowPlaying};
use harken_domain::{Active, Output};
use harken_player::{remote_press, MediaSound, Press, Repeat, Sound, NOT_PART};
use serde_json::{json, Value};

/// How far an episode's skips go, in seconds: back a little, on a little more.
pub const JOG_BACK_S: u32 = 15;
pub const JOG_FORWARD_S: u32 = 30;

/// A volume, 0 to 100, and its mute.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Loudness {
    pub level: i64,
    pub muted: bool,
}

/// What the phone does for its player: steps again, finds a cover, and
/// keeps the volume the session has for this phone.
#[uniffi::export(with_foreign)]
pub trait PlayerShell: Send + Sync {
    /// Step again in `in_ms` milliseconds (0: now): the player moved, or a
    /// play will count then.
    fn wake(&self, in_ms: i64);
    /// A track's cover by its album and its creator (a show's, for an
    /// episode), as an address the system can fetch; none for none.
    fn cover(&self, album: String, creator: String) -> Option<String>;
    /// The phone's own volume where it has one anybody may set (Android's
    /// media stream), the session's for this phone; none, and the player's
    /// own gain is (an iPhone, whose system volume no app may set).
    fn volume(&self) -> Option<Loudness>;
    fn set_volume(&self, level: i64, muted: bool);
}

/// The shell's volume as the player's knob.
struct Knob(Arc<dyn PlayerShell>);

impl harken_player::Knob for Knob {
    fn volume(&self) -> Option<harken_player::Vol> {
        self.0.volume().map(|v| harken_player::Vol {
            level: v.level,
            muted: v.muted,
        })
    }
    fn set(&self, v: harken_player::Vol) {
        self.0.set_volume(v.level, v.muted)
    }
}

/// A phone's end of the account's listening session.
///
/// A step calls into the peer, which tells the app that something moved —
/// at once, on this thread, where the app drains its session inline (its
/// tests) — and the app steps again: that step, inside this one, answers the
/// view this one last made rather than waiting on itself.
#[derive(uniffi::Object)]
pub struct Player {
    peer: Arc<Peer>,
    inner: Mutex<harken_player::Player>,
    sound: Mutex<Option<MediaSound>>,
    shell: Mutex<Option<Arc<dyn PlayerShell>>>,
    last: Mutex<String>,
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
            shell: Mutex::default(),
            last: Mutex::new("{}".into()),
        })
    }

    /// The phone's media player, playing the tracks of `server`, reconciled
    /// from now on, `shell` told when to step; none, nothing is.
    pub fn attach(&self, media: Option<Arc<dyn NativeMediaPlayer>>, server: String, shell: Option<Arc<dyn PlayerShell>>) {
        *self.shell.lock().unwrap_or_else(|e| e.into_inner()) = shell.clone();
        let knob = shell.clone().map(|sh| Arc::new(Knob(sh)) as Arc<dyn harken_player::Knob>);
        let sound = media.map(|m| {
            let s = MediaSound::new(
                ark_media_player::phone::port(m),
                &server,
                move |ms| {
                    if let Some(s) = &shell {
                        s.wake(ms)
                    }
                },
                ark_peer::now_ms,
            );
            match knob {
                Some(k) => s.with_knob(k),
                None => s,
            }
        });
        let old = std::mem::replace(&mut *self.sound.lock().unwrap_or_else(|e| e.into_inner()), sound);
        // The system shows nothing of a player let go.
        if let Some(old) = old {
            old.show(None, MediaControls::default());
        }
    }

    /// Make the player agree with the session and say what happened to it;
    /// what the system's controls asked, pressed; what plays, as
    /// [`Player::view`] says it, shown to the system too.
    pub fn step(&self) -> String {
        let Some(mut inner) = self.busy() else { return self.last() };
        let mut sound = self.sound.lock().unwrap_or_else(|e| e.into_inner());
        self.peer.with(|p| match sound.as_mut() {
            Some(s) => {
                drop(inner.step(p, s));
                // A car's browsing is the app's own (Android's library service); here, nothing.
                drop(s.take_asks());
                for c in s.take_remote() {
                    let at = Player::shown_active(&inner, p, s).map_or(0, |a| a.position(ark_peer::now_ms()));
                    if let Some(press) = remote_press(c, at) {
                        if inner.press(p, &press, Some(&*s as &dyn Sound)).as_deref() == Some(NOT_PART) {
                            s.alone(&press);
                        }
                    }
                }
            }
            None => drop(inner.refusals(p)),
        });
        let v = self.view_of(&inner, sound.as_ref());
        if let Some(s) = sound.as_ref() {
            self.show(s, &v);
        }
        v.to_string()
    }

    /// A press (`harken_player::Press`, JSON): none when it was taken, else
    /// why not. While the phone is its own session the press is done to its
    /// own queue.
    pub fn press(&self, press: String) -> Option<String> {
        let press: Press = match serde_json::from_str(&press) {
            Ok(p) => p,
            Err(e) => return Some(format!("a press: {e}")),
        };
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut sound = self.sound.lock().unwrap_or_else(|e| e.into_inner());
        let why = self.peer.with(|p| inner.press(p, &press, sound.as_ref().map(|s| s as &dyn Sound)));
        match (why.as_deref(), sound.as_mut()) {
            (Some(NOT_PART), Some(s)) => match s.alone(&press) {
                true => None,
                false => why,
            },
            _ => why,
        }
    }

    /// Alone: what is still to come shuffled, or not.
    pub fn shuffle(&self) {
        if let Some(s) = self.sound.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            s.shuffle(shuffled);
        }
    }

    /// Alone: off, all, one, off.
    pub fn cycle_repeat(&self) {
        if let Some(s) = self.sound.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            s.cycle_repeat();
        }
    }

    /// What plays, as JSON: `{me, joined, linked, epoch, listening, active,
    /// stats, refused}` (`harken_player::Player::view`) and `local`, what
    /// only this phone's own player has: `{buffering, shuffle, repeat,
    /// failed, volume}`, repeat `"off"`, `"all"` or `"one"`, the volume
    /// `{level, muted}` (its gain).
    pub fn view(&self) -> String {
        let Some(inner) = self.busy() else { return self.last() };
        let sound = self.sound.lock().unwrap_or_else(|e| e.into_inner());
        self.view_of(&inner, sound.as_ref()).to_string()
    }
}

/// A permutation of `n`, from the clock's low bits: a shuffle needs no more.
fn shuffled(n: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..n).collect();
    let mut x = (ark_peer::now_ms() as u64) | 1;
    for i in (1..n).rev() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        order.swap(i, (x % (i as u64 + 1)) as usize);
    }
    order
}

impl Player {
    /// The player, unless a step is under way on this thread (or another).
    fn busy(&self) -> Option<std::sync::MutexGuard<'_, harken_player::Player>> {
        match self.inner.try_lock() {
            Ok(g) => Some(g),
            Err(std::sync::TryLockError::Poisoned(e)) => Some(e.into_inner()),
            Err(std::sync::TryLockError::WouldBlock) => None,
        }
    }

    fn last(&self) -> String {
        self.last.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// What plays: the session's while joined, this phone's own alone.
    fn shown_active(inner: &harken_player::Player, p: &ark_peer::Peer, s: &MediaSound) -> Option<Active> {
        let v = inner.view(p, Some(s as &dyn Sound));
        serde_json::from_value(v["active"].clone()).ok()
    }

    fn view_of(&self, inner: &harken_player::Player, sound: Option<&MediaSound>) -> Value {
        let mut v = self.peer.with(|p| inner.view(p, sound.map(|s| s as &dyn Sound)));
        if let Some(s) = sound {
            v["local"] = json!({
                "buffering": s.buffering(),
                "shuffle": s.shuffled(),
                "repeat": match s.repeat() { Repeat::Off => "off", Repeat::All => "all", Repeat::One => "one" },
                "failed": s.failed(),
                "volume": s.volume().map(|v| json!({ "level": v.level, "muted": v.muted })),
            });
        }
        *self.last.lock().unwrap_or_else(|e| e.into_inner()) = v.to_string();
        v
    }

    /// What plays, shown to the system — the lock screen, the notification,
    /// a headset, a watch — with the controls it offers: an episode's skips
    /// of seconds, a track's next and previous. While the sound is on
    /// another device, the artist says where ("Handel · Playing on Laptop").
    fn show(&self, s: &MediaSound, v: &Value) {
        let a = match serde_json::from_value::<Active>(v["active"].clone()) {
            Ok(a) if !a.queue.is_empty() => a,
            _ => return s.show(None, MediaControls::default()),
        };
        let t = a.now().clone();
        let episode = self
            .peer
            .with(|p| ark::db::read::<harken_domain::Harken, _>(&p.log.store, |db| db.episode.has(t.id)));
        let me = v["me"].as_str().unwrap_or_default();
        let elsewhere = match &a.output {
            Output::Device { id, .. } if id != me => Some(
                v["listening"]["devices"]
                    .as_array()
                    .and_then(|ds| ds.iter().find(|d| d["id"] == id.as_str()))
                    .and_then(|d| d["name"].as_str())
                    .unwrap_or(id)
                    .to_string(),
            ),
            Output::Away { name, .. } => Some(name.clone()),
            _ => None,
        };
        let counting = matches!(a.clock, harken_domain::Clock::Playing { .. }) && !matches!(a.output, Output::Device { moving: true, .. });
        let shell = self.shell.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let cover = shell.and_then(|sh| sh.cover(t.album.clone(), t.creator.clone()));
        let at = a.at.max(0) as usize;
        let info = NowPlaying {
            title: t.title.clone(),
            artist: match elsewhere {
                Some(on) => format!("{} · Playing on {on}", t.creator),
                None => t.creator.clone(),
            },
            album: t.album.clone(),
            artwork: cover,
            duration_ms: t.duration_ms,
            position_ms: a.position(ark_peer::now_ms()),
            rate: if counting { 1.0 } else { 0.0 },
        };
        let controls = MediaControls {
            has_next: at + 1 < a.queue.len(),
            has_previous: true,
            skip_forward_s: if episode { JOG_FORWARD_S } else { 0 },
            skip_back_s: if episode { JOG_BACK_S } else { 0 },
            can_seek: t.duration_ms > 0,
            can_rate: false,
        };
        s.show(Some(info), controls);
    }
}
