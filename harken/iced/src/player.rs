//! What is playing, and whether anything can be heard.
//!
//! On the desktop nothing is wired to an audio device. That is a real gap and
//! not a hidden one: [`Player::AUDIBLE`] says so, the now-playing bar reads it,
//! and a track selected there is shown rather than silently pretended to play.
//! Closing it means a decoder and an output device (`rodio`, so `cpal`, so
//! ALSA) plus an HTTP reader to feed them — worth doing when the desktop
//! client has a media store to stream from, which it does not yet.

use crate::rows::Id;

/// What the element was loaded with: which track of the queue it sounds.
/// Not what the window says is playing — that is `App::now`, which a window
/// watching another device takes from the session — so the bar, the list
/// and the title never read this; the diagnostics do, by its title.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub id: Id,
    pub title: String,
    /// What the library says it runs for. The element reports its own duration
    /// once it has read enough of the stream, and that one is preferred when
    /// it arrives — a transcode is not always the length the catalogue claims.
    pub ms: i64,
}

mod imp {
    /// The desktop has no audio device wired up. Everything here is a no-op on
    /// purpose rather than by omission: the bar asks `AUDIBLE` and says so.
    pub struct Sink;

    impl Sink {
        pub const AUDIBLE: bool = false;

        pub fn new() -> Sink {
            Sink
        }
        pub fn play(&mut self, _url: &str) {}
        pub fn resume(&mut self) {}
        pub fn pause(&mut self) {}
        pub fn is_playing(&self) -> bool {
            false
        }
        pub fn asked(&self) -> bool {
            false
        }
        pub fn take_stalled(&mut self) -> bool {
            false
        }
        pub fn buffering(&self) -> bool {
            false
        }
        pub fn ended(&self) -> bool {
            false
        }
        pub fn position(&self) -> f64 {
            0.0
        }
        pub fn duration(&self) -> Option<f64> {
            None
        }
        pub fn seek(&mut self, _secs: f64) {}
        /// Nothing to turn up or down.
        pub fn volume(&mut self) -> Option<(f64, bool)> {
            None
        }
        pub fn set_volume(&mut self, _level: f64, _muted: bool) {}
        pub fn level(&self) -> (f64, bool) {
            (1.0, false)
        }
    }
}

/// The selection and the thing that sounds it, together.
pub struct Player {
    track: Option<Track>,
    /// Where in the queue it was put: what tells a track queued twice apart.
    at: usize,
    sink: imp::Sink,
}

impl Player {
    /// Whether this build can sound anything at all. False on the desktop.
    pub const AUDIBLE: bool = imp::Sink::AUDIBLE;

    pub fn new() -> Player {
        Player {
            track: None,
            at: 0,
            sink: imp::Sink::new(),
        }
    }

    /// Start something. A track with no file is still *selected* — the bar
    /// shows it and says there is nothing to stream, which is what the library
    /// looks like when a song was typed in rather than seeded.
    pub fn play(&mut self, track: Track, at: usize, url: &str) {
        (self.track, self.at) = (Some(track), at);
        if url.is_empty() {
            self.sink.pause();
        } else {
            self.sink.play(url);
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
        self.sink.resume();
    }

    pub fn pause(&mut self) {
        self.sink.pause();
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
        self.track.is_some() && self.sink.asked()
    }

    /// Whether it stopped by itself since the last ask, said once.
    pub fn take_stalled(&mut self) -> bool {
        self.sink.take_stalled()
    }

    pub fn is_playing(&self) -> bool {
        self.sink.is_playing()
    }

    /// Asked to play, and waiting for the stream.
    pub fn buffering(&self) -> bool {
        self.sink.buffering()
    }

    /// The current track has run out, so the bar should move on.
    pub fn ended(&self) -> bool {
        self.track.is_some() && self.sink.ended()
    }

    pub fn position(&self) -> f64 {
        self.sink.position()
    }

    /// Seconds, from the stream if it knows and from the library otherwise.
    pub fn duration(&self) -> f64 {
        self.sink
            .duration()
            .unwrap_or_else(|| self.track.as_ref().map_or(0.0, |t| t.ms as f64 / 1000.0))
    }

    pub fn seek(&mut self, secs: f64) {
        self.sink.seek(secs);
    }

    /// The volume this device sounds at, 0 to 100, and whether it is muted
    /// — `None` where this build cannot set one: the desktop, which sounds
    /// nothing, and a browser that keeps the volume for itself.
    pub fn volume(&mut self) -> Option<(i64, bool)> {
        self.sink.volume().map(|(level, muted)| ((level * 100.0).round() as i64, muted))
    }

    pub fn set_volume(&mut self, level: i64, muted: bool) {
        self.sink.set_volume(level.clamp(0, 100) as f64 / 100.0, muted);
    }

    /// The volume it was last given, 0 to 100, and its mute: what a bar
    /// draws for a device that is its own session.
    pub fn level(&self) -> (i64, bool) {
        let (level, muted) = self.sink.level();
        ((level * 100.0).round() as i64, muted)
    }
}
