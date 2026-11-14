//! This device's own player as a [`Sound`]: a queue, where in it, and the
//! device's media player (`ark_module::media_player`, implemented by
//! `ark-media-player`: AVPlayer on an iPhone, Media3 on Android, the sound
//! card and MPRIS on the desktop) making the sound of one track of it at a
//! time. What phones' `PhoneSound`s each kept in Swift and Kotlin, here
//! once.
//!
//! It plays through its queue by itself, as a phone's player does: a track
//! that ends is followed by the next — or, alone, by itself or the first
//! again, as repeat says — and the reconciler sees the move and says it
//! (`advance`). What the system did to it — an interruption, the
//! headphones out, a stream that failed — is a stall, said once; an
//! interruption over that the player went on from, a resume.
//!
//! What the system's controls asked (the lock screen, a headset, a car,
//! the desktop's media keys) is kept for the shell to take
//! ([`MediaSound::take_remote`]): it is a press, of the session while the
//! device is part of it and of this queue alone ([`MediaSound::alone`]).
//! [`remote_press`] says which press each is.
//!
//! Everything it is told by the player arrives on the player's own thread;
//! the shell is told to step ([`MediaSound::new`]'s `wake`) whenever
//! something moved and when a play would count.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

use ark_module::media_player::{MediaControls, MediaEvent, MediaListener, MediaPlayer, NowPlaying, RemoteCommand, RouteChange};
use harken_domain::{Id, Media, Track};

use crate::{Change, Event, Heard, Level, Place, Press, Sound, Vol};

/// What alone repeats.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

impl Repeat {
    /// Off, all, one, off.
    pub fn next(self) -> Repeat {
        match self {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        }
    }
}

/// How far into a track a previous goes to its start rather than the one before.
pub const RESTART_MS: i64 = 3_000;

/// What it says, and what it was asked, kept as one value.
#[derive(Default)]
struct Local {
    queue: Vec<Track>,
    at: Option<usize>,
    from: String,
    /// Asked to play (a press, the session), whether or not it sounds yet.
    asked: bool,
    /// The player has what it was last loaded with.
    ready: bool,
    waiting: bool,
    sounding: bool,
    /// Where it was last read to be, and when (the clock's).
    ms: i64,
    read_at: i64,
    rate: f32,
    ended: bool,
    events: VecDeque<Event>,
    remote: VecDeque<RemoteCommand>,
    asks: VecDeque<MediaEvent>,
    vol: Vol,
    repeat: Repeat,
    shuffled: bool,
    failed: Option<String>,
    /// What the system was last shown, so a step that changes nothing tells it nothing.
    shown: Option<(Option<NowPlaying>, MediaControls, i64)>,
}

impl Local {
    fn position(&self, now: i64) -> i64 {
        match self.sounding {
            true => self.ms + ((now - self.read_at).max(0) as f32 * self.rate) as i64,
            false => self.ms,
        }
    }

    /// Where it is now, held there.
    fn hold(&mut self, now: i64) {
        self.ms = self.position(now);
        self.read_at = now;
    }

    fn stalled(&mut self, now: i64) {
        if !self.asked {
            return;
        }
        self.hold(now);
        self.asked = false;
        self.sounding = false;
        self.waiting = false;
        self.events.push_back(Event::Stalled { at_ms: self.ms });
    }
}

/// A volume of the device's own that is the session's for it, where it has
/// one anybody may set: a phone's media stream, which its buttons move
/// too. None of it (`volume` none), and the player's own gain is.
pub trait Knob: Send + Sync {
    fn volume(&self) -> Option<Vol>;
    fn set(&self, v: Vol);
}

struct Shared {
    knob: Mutex<Option<Arc<dyn Knob>>>,
    port: Arc<dyn MediaPlayer>,
    /// The server a track's file is under.
    server: String,
    local: Mutex<Local>,
    /// Step again in this many milliseconds (0: now).
    wake: Box<dyn Fn(i64) + Send + Sync>,
    clock: Box<dyn Fn() -> i64 + Send + Sync>,
}

impl Shared {
    fn local(&self) -> MutexGuard<'_, Local> {
        self.local.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn url(&self, t: &Track) -> String {
        harken_domain::host::url(&self.server, &t.file)
    }

    /// The track at `at` of what is held, from `ms`, played if asked.
    /// Never with the lock held: a player may answer at once.
    fn sound(&self, at: usize, ms: i64) {
        let (url, play) = {
            let mut l = self.local();
            let Some(t) = l.queue.get(at).cloned() else { return };
            let now = (self.clock)();
            l.at = Some(at);
            (l.ms, l.read_at) = (ms, now);
            l.ready = false;
            l.sounding = false;
            l.ended = false;
            l.failed = None;
            l.waiting = l.asked;
            (self.url(&t), l.asked)
        };
        self.port.load(url, ms);
        if play {
            self.port.play();
        }
    }

    /// A track that ended by itself: the next, as repeat says; or the queue's end.
    fn ended(&self) {
        let next = {
            let mut l = self.local();
            let now = (self.clock)();
            let i = l.at.unwrap_or(0);
            match l.repeat {
                Repeat::One => Some(i),
                _ if i + 1 < l.queue.len() => Some(i + 1),
                Repeat::All if !l.queue.is_empty() => Some(0),
                _ => {
                    l.hold(now);
                    l.sounding = false;
                    l.ended = true;
                    None
                }
            }
        };
        if let Some(n) = next {
            self.sound(n, 0);
        }
        (self.wake)(0);
    }
}

/// What the player tells, kept.
struct Hears(Arc<Shared>);

impl MediaListener for Hears {
    fn heard(&self, e: MediaEvent) {
        let s = &self.0;
        let now = (s.clock)();
        {
            let mut l = s.local();
            match e {
                MediaEvent::Loaded { .. } => {
                    l.ready = true;
                    l.read_at = now;
                }
                MediaEvent::Position { at_ms } => {
                    (l.ms, l.read_at) = (at_ms, now);
                    // Said often: the reconciler reads it when it looks.
                    return;
                }
                MediaEvent::Playing => {
                    l.read_at = now;
                    l.ready = true;
                    l.sounding = true;
                    l.waiting = false;
                }
                MediaEvent::Waiting => {
                    l.hold(now);
                    l.sounding = false;
                    l.waiting = l.asked;
                }
                // A pause it was asked for, or one the system made, which
                // the system says on its own (`Interrupted`, `RouteChange`).
                MediaEvent::Paused => {
                    l.hold(now);
                    l.sounding = false;
                    l.waiting = false;
                }
                MediaEvent::Ended => {
                    drop(l);
                    s.ended();
                    return;
                }
                MediaEvent::Remote { command } => l.remote.push_back(command),
                MediaEvent::Failed { why } => {
                    l.stalled(now);
                    l.failed = Some(why);
                }
                MediaEvent::Interrupted
                | MediaEvent::Route {
                    change: RouteChange::Unplugged,
                } => l.stalled(now),
                MediaEvent::Route { .. } => return,
                MediaEvent::Resumed => {
                    if !l.asked && l.at.is_some() {
                        l.asked = true;
                        l.events.push_back(Event::Resumed);
                    }
                }
                e @ (MediaEvent::Browse { .. } | MediaEvent::PlayItem { .. } | MediaEvent::Add { .. }) => l.asks.push_back(e),
            }
        }
        (s.wake)(0);
    }
}

/// This device's own player: a queue over its media player.
pub struct MediaSound {
    s: Arc<Shared>,
}

impl MediaSound {
    /// The player `port`, playing the tracks of the server at `server`;
    /// `wake` told to step again in so many milliseconds whenever something
    /// moved (0) or a play will count, `clock` the server's clock as this
    /// device reads it (`ark_peer::now_ms`).
    pub fn new(
        port: Arc<dyn MediaPlayer>,
        server: &str,
        wake: impl Fn(i64) + Send + Sync + 'static,
        clock: impl Fn() -> i64 + Send + Sync + 'static,
    ) -> MediaSound {
        let s = Arc::new(Shared {
            knob: Mutex::new(None),
            port,
            server: server.into(),
            local: Mutex::new(Local {
                rate: 1.0,
                vol: Vol { level: 100, muted: false },
                ..Local::default()
            }),
            wake: Box::new(wake),
            clock: Box::new(clock),
        });
        s.port.set_listener(Some(Arc::new(Hears(s.clone()))));
        MediaSound { s }
    }

    /// The device's own volume, as the session's for it, in place of the
    /// player's gain (a phone's media stream).
    pub fn with_knob(self, knob: Arc<dyn Knob>) -> MediaSound {
        *self.s.knob.lock().unwrap_or_else(|e| e.into_inner()) = Some(knob);
        self
    }

    fn knob(&self) -> Option<Arc<dyn Knob>> {
        self.s.knob.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The media player it plays on.
    pub fn port(&self) -> &Arc<dyn MediaPlayer> {
        &self.s.port
    }

    /// What the system's controls asked since the last take.
    pub fn take_remote(&self) -> Vec<RemoteCommand> {
        self.s.local().remote.drain(..).collect()
    }

    /// What a car or a watch asked since the last take: a [`MediaEvent::Browse`],
    /// a [`MediaEvent::PlayItem`] or a [`MediaEvent::Add`].
    pub fn take_asks(&self) -> Vec<MediaEvent> {
        self.s.local().asks.drain(..).collect()
    }

    /// Why the last track could not be played, until another is.
    pub fn failed(&self) -> Option<String> {
        self.s.local().failed.clone()
    }

    /// Filling its buffer.
    pub fn buffering(&self) -> bool {
        self.s.local().waiting
    }

    pub fn repeat(&self) -> Repeat {
        self.s.local().repeat
    }

    pub fn shuffled(&self) -> bool {
        self.s.local().shuffled
    }

    /// Off, all, one, off: alone only.
    pub fn cycle_repeat(&self) {
        let mut l = self.s.local();
        l.repeat = l.repeat.next();
    }

    /// What is still to come shuffled by `order` (a permutation of the
    /// tracks after this one, as the shell's random numbers have it), or
    /// back to how it was not: alone only. Shuffled in the queue itself, so
    /// what is drawn is what plays.
    pub fn shuffle(&self, order: impl FnOnce(usize) -> Vec<usize>) {
        let mut l = self.s.local();
        l.shuffled = !l.shuffled;
        if !l.shuffled {
            return;
        }
        let from = l.at.map_or(0, |i| i + 1);
        if from + 1 >= l.queue.len() {
            return;
        }
        let rest: Vec<Track> = l.queue.split_off(from);
        let order = order(rest.len());
        let mut taken: Vec<Option<Track>> = rest.into_iter().map(Some).collect();
        let mut out: Vec<Track> = order.iter().filter_map(|&i| taken.get_mut(i).and_then(Option::take)).collect();
        out.extend(taken.into_iter().flatten());
        l.queue.extend(out);
    }

    /// What the system shows, and which of its controls it offers: told
    /// only when they changed, or the position moved from where the shown
    /// rate would have it by now.
    pub fn show(&self, info: Option<NowPlaying>, controls: MediaControls) {
        let now = (self.s.clock)();
        let changed = {
            let mut l = self.s.local();
            let same = match (&l.shown, &info) {
                (Some((None, c, _)), None) => *c == controls,
                (Some((Some(was), c, at)), Some(info)) => *c == controls && moved_on(was, *at, info, now),
                _ => false,
            };
            if !same {
                l.shown = Some((info.clone(), controls, now));
            }
            !same
        };
        if changed {
            self.s.port.set_controls(controls);
            self.s.port.set_now_playing(info);
        }
    }

    /// A press, done to this queue: the device alone. False for one this
    /// player has no part in (a volume, a hand-off).
    pub fn alone(&mut self, p: &Press) -> bool {
        let now = (self.s.clock)();
        match p {
            Press::Play => self.resume(),
            Press::Pause => self.pause(),
            Press::Toggle => {
                let playing = {
                    let l = self.s.local();
                    l.asked && !l.ended
                };
                if playing {
                    self.pause()
                } else {
                    self.resume()
                }
            }
            Press::Skip { by } => {
                let (len, at, ms, repeat) = {
                    let l = self.s.local();
                    (l.queue.len(), l.at, l.position(now), l.repeat)
                };
                let Some(i) = at else { return true };
                if *by < 0 && ms > RESTART_MS {
                    self.seek(0);
                } else {
                    let to = i as i64 + by;
                    if (0..len as i64).contains(&to) {
                        self.jump(to as usize, 0);
                    } else if *by > 0 && repeat != Repeat::Off && len > 0 {
                        self.jump(0, 0);
                    } else if *by < 0 {
                        self.seek(0);
                    }
                }
            }
            Press::Seek { ms } => self.seek(*ms),
            Press::Jump { index } => {
                let len = self.s.local().queue.len() as i64;
                if (0..len).contains(index) {
                    self.jump(*index as usize, 0)
                }
            }
            Press::Start {
                queue,
                at,
                position_ms,
                from,
                shuffled,
            } => {
                if !queue.is_empty() {
                    let at = (*at).clamp(0, queue.len() as i64 - 1) as usize;
                    self.load(queue, at, *position_ms, true, from);
                    // Shuffle is the queue's: lit for a list started shuffled, out for any other.
                    self.s.local().shuffled = *shuffled;
                }
            }
            Press::Next { tracks } | Press::Append { tracks } => {
                let next = matches!(p, Press::Next { .. });
                let empty = {
                    let mut l = self.s.local();
                    match l.at {
                        Some(i) if !l.queue.is_empty() => {
                            let at = if next { i + 1 } else { l.queue.len() }.min(l.queue.len());
                            l.queue.splice(at..at, tracks.iter().cloned());
                            false
                        }
                        _ => true,
                    }
                };
                if empty && !tracks.is_empty() {
                    self.load(tracks, 0, 0, true, "");
                    self.s.local().shuffled = false;
                }
            }
            Press::Remove { index } => {
                let mut l = self.s.local();
                let i = *index as usize;
                if *index >= 0 && i < l.queue.len() && Some(i) != l.at {
                    l.queue.remove(i);
                    if let Some(at) = l.at.filter(|at| i < *at) {
                        l.at = Some(at - 1);
                    }
                }
            }
            Press::Move { from, to } => {
                let mut l = self.s.local();
                let len = l.queue.len() as i64;
                if !(0..len).contains(from) || !(0..len).contains(to) || from == to {
                    return true;
                }
                let (f, t) = (*from as usize, *to as usize);
                let track = l.queue.remove(f);
                l.queue.insert(t, track);
                if let Some(at) = l.at {
                    l.at = Some(if f == at {
                        t
                    } else if f < at && t >= at {
                        at - 1
                    } else if f > at && t <= at {
                        at + 1
                    } else {
                        at
                    });
                }
            }
            // Its own gain, alone: the volume this device sounds at.
            Press::Loud { level: Level::Here, change } => {
                let v = self.volume().unwrap_or_default();
                self.set_volume(match change {
                    Change::To { level } => Vol { level: *level, muted: false },
                    Change::By { delta } => Vol {
                        level: v.level + delta,
                        muted: false,
                    },
                    Change::Mute { muted } => Vol {
                        level: v.level,
                        muted: *muted,
                    },
                })
            }
            _ => return false,
        }
        (self.s.wake)(0);
        true
    }
}

/// Whether `now` is `was`, shown at `at`, played on to `then`: the same
/// words and picture at the same rate, within a second of where that rate
/// would have it.
fn moved_on(was: &NowPlaying, at: i64, now: &NowPlaying, then: i64) -> bool {
    let words = |n: &NowPlaying| {
        (
            n.title.clone(),
            n.artist.clone(),
            n.album.clone(),
            n.artwork.clone(),
            n.placeholder.clone(),
            n.duration_ms,
        )
    };
    let expected = was.position_ms + ((then - at).max(0) as f32 * was.rate) as i64;
    words(was) == words(now) && was.rate == now.rate && (now.position_ms - expected).abs() < 1_000
}

/// The press a control of the system's asks, as the device would press it:
/// a skip of seconds is a seek from where it is (`at_ms`), a stop a pause
/// (stopping would forget a queue nobody asked to lose). None for what has
/// no press (a rate).
pub fn remote_press(c: RemoteCommand, at_ms: i64) -> Option<Press> {
    Some(match c {
        RemoteCommand::Play => Press::Play,
        RemoteCommand::Pause | RemoteCommand::Stop => Press::Pause,
        RemoteCommand::Toggle => Press::Toggle,
        RemoteCommand::Next => Press::Skip { by: 1 },
        RemoteCommand::Previous => Press::Skip { by: -1 },
        RemoteCommand::SkipForward { seconds } => Press::Seek {
            ms: at_ms + seconds as i64 * 1000,
        },
        RemoteCommand::SkipBack { seconds } => Press::Seek {
            ms: (at_ms - seconds as i64 * 1000).max(0),
        },
        RemoteCommand::SeekTo { at_ms } => Press::Seek { ms: at_ms.max(0) },
        RemoteCommand::Rate { .. } => return None,
    })
}

impl Sound for MediaSound {
    fn heard(&self) -> Heard {
        let now = (self.s.clock)();
        let l = self.s.local();
        let at = l.at.filter(|i| *i < l.queue.len());
        let ms = l.position(now);
        Heard {
            queue: l.queue.iter().map(|t| t.id).collect::<Vec<Id<Media>>>(),
            at,
            playing: l.asked,
            ready: at.is_some() && l.ready,
            place: match (at, l.ready && !l.waiting) {
                (None, _) => Place::Unknown,
                (Some(_), false) => Place::Believed(ms),
                (Some(_), true) => Place::Measured { ms, at: now },
            },
            ended: l.ended,
            from: l.from.clone(),
        }
    }

    fn take(&mut self) -> Option<Event> {
        self.s.local().events.pop_front()
    }

    fn tracks(&self) -> Vec<Track> {
        self.s.local().queue.clone()
    }

    fn load(&mut self, queue: &[Track], at: usize, position_ms: i64, playing: bool, from: &str) {
        {
            let mut l = self.s.local();
            l.queue = queue.to_vec();
            l.from = from.into();
            l.asked = playing;
            l.at = None;
        }
        if at < queue.len() {
            self.s.sound(at, position_ms)
        }
    }

    fn jump(&mut self, at: usize, position_ms: i64) {
        self.s.sound(at, position_ms)
    }

    fn resume(&mut self) {
        let restart = {
            let mut l = self.s.local();
            if l.at.is_none() {
                return;
            }
            l.asked = true;
            match l.ended || l.failed.is_some() {
                true => Some((l.at.unwrap_or(0), if l.ended { 0 } else { l.ms })),
                false => {
                    l.waiting = !l.sounding;
                    None
                }
            }
        };
        match restart {
            Some((at, ms)) => self.s.sound(at, ms),
            None => self.s.port.play(),
        }
    }

    fn pause(&mut self) {
        {
            let now = (self.s.clock)();
            let mut l = self.s.local();
            l.hold(now);
            l.asked = false;
            l.sounding = false;
            l.waiting = false;
        }
        self.s.port.pause()
    }

    fn seek(&mut self, ms: i64) {
        {
            let now = (self.s.clock)();
            let mut l = self.s.local();
            (l.ms, l.read_at) = (ms.max(0), now);
            l.ended = false;
        }
        self.s.port.seek(ms.max(0))
    }

    fn wake_in(&mut self, ms: i64) {
        (self.s.wake)(ms)
    }

    /// The device's own volume where it has one, else the player's gain.
    fn volume(&self) -> Option<Vol> {
        self.knob().and_then(|k| k.volume()).or(Some(self.s.local().vol))
    }

    fn set_volume(&mut self, v: Vol) {
        let v = Vol {
            level: v.level.clamp(0, 100),
            muted: v.muted,
        };
        if let Some(k) = self.knob().filter(|k| k.volume().is_some()) {
            return k.set(v);
        }
        self.s.local().vol = v;
        self.s.port.set_volume(if v.muted { 0.0 } else { v.level as f32 / 100.0 })
    }
}

impl Drop for MediaSound {
    fn drop(&mut self) {
        self.s.port.set_listener(None)
    }
}
