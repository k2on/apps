//! What is playing, and the thing that sounds it.
//!
//! The sound is the desktop's media player (`ark-media-player`'s `Desktop`:
//! the sound card through rodio, the stream read by range requests, and
//! MPRIS on the session bus, which the media keys and the desktop's
//! now-playing widgets reach), attached when the window boots
//! ([`Player::attach`]). A window with none attached — the tests, the
//! screenshots' run — sounds nothing: [`Player::audible`] says so, the
//! now-playing bar reads it, and a track selected there is shown rather
//! than silently pretended to play.
//!
//! What the player says of itself arrives on its own thread and is kept
//! here, the window's bell rung so the window reads it; what the media
//! keys ask is kept for the window to press ([`Player::take_remote`]).

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use ark_media_player::{MediaControls, MediaEvent, MediaListener, MediaPlayer, NowPlaying, RemoteCommand, RouteChange};

use crate::rows::Id;

/// Whether this run sounds anything: the window's own, not the tests' nor
/// the screenshots' run, which declare a device that cannot play.
pub fn sounds() -> bool {
    !cfg!(test) && std::env::var_os("ARK_SHOTS").is_none()
}

/// What the element was loaded with: which track of the queue it sounds.
/// Not what the window says is playing — that is `App::now`, which a window
/// watching another device takes from the session — so the bar, the list
/// and the title never read this; the diagnostics do, by its title.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub id: Id,
    pub title: String,
    /// What the library says it runs for. The player reports its own
    /// duration once it has read enough of the stream, and that one is
    /// preferred when it arrives — a transcode is not always the length the
    /// catalogue claims.
    pub ms: i64,
}

/// What the media player said of itself, and what it was last asked.
struct State {
    /// Asked to play, whether or not it sounds yet.
    asked: bool,
    sounding: bool,
    waiting: bool,
    ended: bool,
    /// It stopped by itself (no sound card, a stream that failed, the
    /// headphones out) since the last ask.
    stalled: bool,
    /// Where it was last read to be, and when.
    ms: i64,
    read_at: Instant,
    duration_ms: Option<i64>,
    level: i64,
    muted: bool,
    remote: Vec<RemoteCommand>,
    /// What MPRIS was last shown, and when.
    shown: Option<(Option<NowPlaying>, MediaControls, Instant)>,
}

impl Default for State {
    fn default() -> State {
        State {
            asked: false,
            sounding: false,
            waiting: false,
            ended: false,
            stalled: false,
            ms: 0,
            read_at: Instant::now(),
            duration_ms: None,
            level: 100,
            muted: false,
            remote: vec![],
            shown: None,
        }
    }
}

impl State {
    fn position_ms(&self) -> i64 {
        match self.sounding {
            true => self.ms + self.read_at.elapsed().as_millis() as i64,
            false => self.ms,
        }
    }

    fn hold(&mut self) {
        self.ms = self.position_ms();
        self.read_at = Instant::now();
    }

    fn stall(&mut self) {
        if self.asked {
            self.hold();
            (self.asked, self.sounding, self.waiting, self.stalled) = (false, false, false, true);
        }
    }
}

/// The player's events, kept, and the window rung.
struct Hears {
    s: Arc<Mutex<State>>,
    bell: arkui::wake::Bell,
}

impl MediaListener for Hears {
    fn heard(&self, e: MediaEvent) {
        {
            let mut s = self.s.lock().unwrap_or_else(|e| e.into_inner());
            match e {
                MediaEvent::Loaded { duration_ms } => s.duration_ms = (duration_ms > 0).then_some(duration_ms),
                MediaEvent::Position { at_ms } => (s.ms, s.read_at) = (at_ms, Instant::now()),
                MediaEvent::Playing => {
                    s.read_at = Instant::now();
                    (s.sounding, s.waiting) = (true, false);
                }
                MediaEvent::Waiting => {
                    s.hold();
                    (s.sounding, s.waiting) = (false, s.asked);
                }
                MediaEvent::Paused => {
                    s.hold();
                    (s.sounding, s.waiting) = (false, false);
                }
                MediaEvent::Ended => {
                    s.hold();
                    (s.sounding, s.ended) = (false, true);
                }
                MediaEvent::Failed { .. }
                | MediaEvent::Interrupted
                | MediaEvent::Route {
                    change: RouteChange::Unplugged,
                } => s.stall(),
                MediaEvent::Remote { command } => s.remote.push(command),
                MediaEvent::Resumed | MediaEvent::Route { .. } | MediaEvent::Browse { .. } | MediaEvent::PlayItem { .. } => return,
            }
        }
        self.bell.ring();
    }
}

/// The selection and the thing that sounds it, together.
pub struct Player {
    track: Option<Track>,
    /// Where in the queue it was put: what tells a track queued twice apart.
    at: usize,
    /// The address it was last loaded from, to start again from at its end.
    url: String,
    port: Option<Arc<dyn MediaPlayer>>,
    s: Arc<Mutex<State>>,
}

impl Default for Player {
    fn default() -> Player {
        Player::new()
    }
}

impl Player {
    /// A player that sounds nothing until one is attached.
    pub fn new() -> Player {
        Player {
            track: None,
            at: 0,
            url: String::new(),
            port: None,
            s: Arc::default(),
        }
    }

    /// Sound through `port` from now on, `bell` rung whenever it says
    /// anything.
    pub fn attach(&mut self, port: Arc<dyn MediaPlayer>, bell: arkui::wake::Bell) {
        port.set_listener(Some(Arc::new(Hears { s: self.s.clone(), bell })));
        self.port = Some(port);
    }

    fn s(&self) -> MutexGuard<'_, State> {
        self.s.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Whether this window can sound anything at all.
    pub fn audible(&self) -> bool {
        self.port.is_some()
    }

    /// Start something. A track with no file is still *selected* — the bar
    /// shows it and says there is nothing to stream, which is what the library
    /// looks like when a song was typed in rather than seeded.
    pub fn play(&mut self, track: Track, at: usize, url: &str) {
        (self.track, self.at) = (Some(track), at);
        if url.is_empty() {
            return self.pause();
        }
        self.url = url.into();
        self.load(0);
    }

    fn load(&mut self, at_ms: i64) {
        {
            let mut s = self.s();
            (s.asked, s.sounding, s.ended, s.stalled) = (true, false, false, false);
            (s.ms, s.read_at, s.duration_ms) = (at_ms, Instant::now(), None);
            s.waiting = self.port.is_some();
        }
        if let Some(p) = &self.port {
            p.load(self.url.clone(), at_ms);
            p.play();
        }
    }

    /// Play, and mean it — and pause, and mean that.
    ///
    /// Two verbs and no toggle, because no caller is in a position to toggle.
    /// The operating system's controller says which of the two it wants; so
    /// does another of this account's devices, which cannot know from here
    /// whether this one is playing; and so, now, does the button in this
    /// window, which asks the *session* what is playing before it decides
    /// which it means. A toggle here would answer a lock screen showing
    /// "paused" by pausing a track something else had already resumed.
    pub fn resume(&mut self) {
        if self.track.is_none() || self.url.is_empty() {
            return;
        }
        let (ended, again, ms) = {
            let s = self.s();
            (s.ended, s.ended || (s.stalled && !s.sounding), s.ms)
        };
        if again {
            // At its end, or stopped by itself: loaded again, from its start or where it stopped.
            return self.load(if ended { 0 } else { ms });
        }
        self.s().asked = true;
        if let Some(p) = &self.port {
            p.play();
        }
    }

    pub fn pause(&mut self) {
        {
            let mut s = self.s();
            s.hold();
            (s.asked, s.sounding, s.waiting) = (false, false, false);
        }
        if let Some(p) = &self.port {
            p.pause();
        }
    }

    pub fn track(&self) -> Option<&Track> {
        self.track.as_ref()
    }

    /// The track it holds is now at `at` in its queue — the queue reordered
    /// around it. Nothing is fetched again: the sound plays on.
    pub fn requeue(&mut self, at: usize) {
        self.at = at;
    }

    /// Where in its queue it was put.
    pub fn at(&self) -> usize {
        self.at
    }

    /// Asked to play — whether or not the stream has started — and not at
    /// its end: what a reconciler hears as playing.
    pub fn asked(&self) -> bool {
        let s = self.s();
        self.track.is_some() && s.asked && !s.ended
    }

    /// Whether it stopped by itself since the last ask, said once.
    pub fn take_stalled(&mut self) -> bool {
        std::mem::take(&mut self.s().stalled)
    }

    pub fn is_playing(&self) -> bool {
        self.s().sounding
    }

    /// Asked to play, and waiting for the stream.
    pub fn buffering(&self) -> bool {
        let s = self.s();
        s.asked && s.waiting
    }

    /// The current track has run out, so the bar should move on.
    pub fn ended(&self) -> bool {
        self.track.is_some() && self.s().ended
    }

    pub fn position(&self) -> f64 {
        self.s().position_ms() as f64 / 1000.0
    }

    /// Seconds, from the stream if it knows and from the library otherwise.
    pub fn duration(&self) -> f64 {
        match self.s().duration_ms {
            Some(ms) => ms as f64 / 1000.0,
            None => self.track.as_ref().map_or(0.0, |t| t.ms as f64 / 1000.0),
        }
    }

    pub fn seek(&mut self, secs: f64) {
        let ms = (secs.max(0.0) * 1000.0) as i64;
        {
            let mut s = self.s();
            (s.ms, s.read_at, s.ended) = (ms, Instant::now(), false);
        }
        if let Some(p) = &self.port {
            p.seek(ms);
        }
    }

    /// The volume this device sounds at, 0 to 100, and whether it is muted
    /// — `None` where this window sounds nothing.
    pub fn volume(&mut self) -> Option<(i64, bool)> {
        self.port.is_some().then(|| self.level())
    }

    pub fn set_volume(&mut self, level: i64, muted: bool) {
        let level = level.clamp(0, 100);
        {
            let mut s = self.s();
            (s.level, s.muted) = (level, muted);
        }
        if let Some(p) = &self.port {
            p.set_volume(if muted { 0.0 } else { level as f32 / 100.0 });
        }
    }

    /// The volume it was last given, 0 to 100, and its mute: what a bar
    /// draws for a device that is its own session.
    pub fn level(&self) -> (i64, bool) {
        let s = self.s();
        (s.level, s.muted)
    }

    /// What the media keys and the desktop's widgets asked since the last take.
    pub fn take_remote(&mut self) -> Vec<RemoteCommand> {
        std::mem::take(&mut self.s().remote)
    }

    /// What the desktop's widgets show (MPRIS), and which of its controls
    /// it offers: told only when it changed, or the position moved from
    /// where the shown rate would have it by now.
    pub fn show(&mut self, info: Option<NowPlaying>, controls: MediaControls) {
        let Some(p) = &self.port else { return };
        {
            let mut s = self.s();
            let same = match (&s.shown, &info) {
                (Some((None, c, _)), None) => *c == controls,
                (Some((Some(was), c, at)), Some(now)) => {
                    let expected = was.position_ms + (at.elapsed().as_millis() as f32 * was.rate) as i64;
                    *c == controls
                        && (&was.title, &was.artist, &was.album, &was.artwork, was.duration_ms, was.rate)
                            == (&now.title, &now.artist, &now.album, &now.artwork, now.duration_ms, now.rate)
                        && (now.position_ms - expected).abs() < 1_000
                }
                _ => false,
            };
            if same {
                return;
            }
            s.shown = Some((info.clone(), controls, Instant::now()));
        }
        p.set_controls(controls);
        p.set_now_playing(info);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> Track {
        Track {
            id: Id::default(),
            title: "a".into(),
            ms: 60_000,
        }
    }

    /// With no media player the window sounds nothing and says so; with
    /// one, a track pressed is loaded and played, the player's own word on
    /// where it is and how long it is kept, its end said, and a media key
    /// kept for the window to press.
    #[test]
    fn the_desktop_player_drives_the_media_player_and_hears_it() {
        let mut silent = Player::new();
        assert!(!silent.audible());
        assert_eq!(silent.volume(), None);

        let fake = ark_media_player::Fake::new();
        let mut p = Player::new();
        let bell = arkui::wake::Bell::new();
        p.attach(Arc::new(fake.clone()), bell.clone());
        assert!(p.audible());
        p.play(track(), 0, "http://h/media/a.mp3");
        assert_eq!(fake.take(), ["load http://h/media/a.mp3 0", "play"]);
        assert!(p.asked() && p.buffering() && !p.is_playing());
        fake.hear(MediaEvent::Loaded { duration_ms: 61_000 });
        fake.hear(MediaEvent::Playing);
        assert!(p.is_playing() && !p.buffering());
        assert_eq!(p.duration(), 61.0, "the stream's own length");
        fake.hear(MediaEvent::Position { at_ms: 30_000 });
        assert!(p.position() >= 30.0);
        fake.hear(MediaEvent::Remote {
            command: RemoteCommand::Toggle,
        });
        assert_eq!(p.take_remote(), [RemoteCommand::Toggle]);
        assert!(bell.rung() >= 4, "the window was rung for each");
        fake.hear(MediaEvent::Ended);
        assert!(p.ended() && !p.asked());
        p.resume();
        assert_eq!(fake.take(), ["load http://h/media/a.mp3 0", "play"], "at its end, from its start again");

        fake.hear(MediaEvent::Failed { why: "no sound card".into() });
        assert!(p.take_stalled() && !p.take_stalled(), "a failure is a stall, said once");
        p.set_volume(40, true);
        assert_eq!(fake.take(), ["volume 0"]);
        assert_eq!(p.volume(), Some((40, true)));
    }
}
