//! What is playing, and whether anything can be heard.
//!
//! In a browser this is an `<audio>` element the page owns. Handing it a URL
//! buys streaming, buffering, range requests and seeking from the platform —
//! four problems that are the browser's job and would each have to be solved
//! again to play the same bytes from Rust.
//!
//! On the desktop nothing is wired to an audio device. That is a real gap and
//! not a hidden one: [`Player::AUDIBLE`] says so, the now-playing bar reads it,
//! and a track selected there is shown rather than silently pretended to play.
//! Closing it means a decoder and an output device (`rodio`, so `cpal`, so
//! ALSA) plus an HTTP reader to feed them — worth doing when the desktop
//! client has a media store to stream from, which it does not yet.

use crate::rows::Id;

/// What the platform's media controller asked for, if anything.
///
/// Play and pause are separate rather than one toggle, because the operating
/// system says which it means: a lock screen that has been showing "paused"
/// sends `play`, and answering a toggle there would pause a track that a
/// second listener had already resumed.
///
/// Nothing on the desktop can send one, for the same reason nothing there can
/// be heard: there is no media session and no tab to title. So this, and
/// everything that reads it, is the browser build's alone.
#[cfg(target_arch = "wasm32")]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Remote {
    Play,
    Pause,
    Next,
    Previous,
    Seek(f64),
}

#[cfg(target_arch = "wasm32")]
impl Remote {
    /// The mailbox holds a string, because what crosses the wasm boundary
    /// cheaply is a string and there are five of them.
    fn parse(note: &str) -> Option<Remote> {
        Some(match note {
            "play" => Remote::Play,
            "pause" => Remote::Pause,
            "next" => Remote::Next,
            "prev" => Remote::Previous,
            _ => Remote::Seek(note.strip_prefix("seek:")?.parse().ok()?),
        })
    }
}

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

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::Remote;
    use crate::listening::Track;
    use crate::rows::Id;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;
    use web_sys::HtmlAudioElement;

    // `MediaSession` is behind `web_sys_unstable_apis` in web-sys, which is a
    // `RUSTFLAGS` every build of this crate would have to agree on — the nix
    // one, the devshell one and EAS. A snippet is the smaller promise: it
    // travels in the module, wasm-bindgen emits it beside the glue, and
    // nothing outside this file has to know it is there.
    #[wasm_bindgen(inline_js = r#"// The tab's title and the platform's media controller — the same two facts
// twice: what is playing, and whether it is. Written here rather than in
// Rust because both are the page's, the way the <audio> element is.
//
// The remote is a mailbox and a bell. A handler runs on the browser's
// stack, and the app's state lives behind iced's update loop, so what a
// lock-screen button can do is leave a note and ring: the update the ring
// asks for collects it (docs/plan-reactive.md). Last one wins: two presses
// before that update are one instruction, which is what a person pressing
// twice meant anyway. The element rings as well whenever it starts, stops
// or ends on its own — a stall, the end of a stream — or its volume moves,
// so a window with no clock still hears it.
let pending = "";
let wired = false;
let ring = () => {};

export function set_ring(f) {
  ring = f;
}

export function watch(el) {
  for (const name of ["play", "playing", "pause", "ended", "waiting", "seeked", "volumechange"]) {
    el.addEventListener(name, () => ring());
  }
}

function on(name, fn) {
  // An action the browser does not know throws rather than being ignored,
  // and the ones it knows differ per platform, so each is set on its own.
  try {
    navigator.mediaSession.setActionHandler(name, fn);
  } catch (e) {
    /* not on this platform */
  }
}

function wire() {
  if (wired) return;
  wired = true;
  on("play", () => { pending = "play"; ring(); });
  on("pause", () => { pending = "pause"; ring(); });
  on("stop", () => { pending = "pause"; ring(); });
  on("nexttrack", () => { pending = "next"; ring(); });
  on("previoustrack", () => { pending = "prev"; ring(); });
  on("seekto", (d) => {
    if (d && typeof d.seekTime === "number") pending = "seek:" + d.seekTime;
    ring();
  });
}

export function announce(heading, title, artist, album, playing) {
  // Windows draws the tab's title in its own window list, so the tab is the
  // one place this has to be right even where there is no media session.
  document.title = heading;
  if (!("mediaSession" in navigator)) return;
  wire();
  navigator.mediaSession.metadata = title
    ? new MediaMetadata({ title: title, artist: artist, album: album })
    : null;
  navigator.mediaSession.playbackState = !title
    ? "none"
    : playing
      ? "playing"
      : "paused";
}

export function position(duration, at) {
  if (!("mediaSession" in navigator)) return;
  if (!navigator.mediaSession.setPositionState) return;
  // The dictionary is validated: a position past the duration, or a duration
  // that is not a number yet, throws rather than being clamped.
  if (!(duration > 0) || !(at >= 0) || at > duration) return;
  try {
    navigator.mediaSession.setPositionState({
      duration: duration,
      position: at,
      playbackRate: 1,
    });
  } catch (e) {
    /* the element has not read enough of the stream to agree yet */
  }
}

export function take_remote() {
  const p = pending;
  pending = "";
  return p;
}
"#)]
    extern "C" {
        #[wasm_bindgen(js_name = announce)]
        fn announce_js(heading: &str, title: &str, artist: &str, album: &str, playing: bool);
        #[wasm_bindgen(js_name = position)]
        fn position_js(duration: f64, at: f64);
        #[wasm_bindgen(js_name = take_remote)]
        fn take_remote_js() -> String;
        #[wasm_bindgen(js_name = set_ring)]
        fn set_ring_js(f: &JsValue);
        #[wasm_bindgen(js_name = watch)]
        fn watch_js(el: &HtmlAudioElement);
    }

    /// What the media session's handlers and the element's events call.
    pub fn ring_with(f: fn()) {
        set_ring_js(&Closure::<dyn Fn()>::new(f).into_js_value());
    }

    pub struct Sink {
        el: Option<HtmlAudioElement>,
        /// What the page was last told, so an update that changed nothing
        /// does not rebuild the metadata. The element
        /// decides on its own when it is playing — buffering, a stall, the
        /// end of a stream — so this cannot be kept only where the app
        /// changes something.
        said: Option<(Id, bool)>,
        /// …and the whole second the controller's scrubber was last moved to,
        /// which moves once a second where the rest of it moves on a tap.
        timed: i64,
        /// The volume the element was last given, 0 to 1, and whether it is
        /// muted: kept, so an element made after it was set still has it.
        level: f64,
        muted: bool,
        /// Asked to play — `play()` since the last `pause()` — whatever the
        /// element did since: what tells a stop by itself from one asked for.
        asked: bool,
        /// Whether the element's volume can be set at all, found out once.
        /// iOS keeps a media element's volume at 1 and hands the volume to
        /// the hardware buttons, so there a page can mute and not much else.
        settable: Option<bool>,
    }

    impl Sink {
        pub const AUDIBLE: bool = true;

        pub fn new() -> Sink {
            Sink {
                el: None,
                said: None,
                timed: -1,
                level: 1.0,
                muted: false,
                asked: false,
                settable: None,
            }
        }

        /// One element for the life of the page, reused across tracks. A new
        /// one per track leaks a media element and, in Safari, the first
        /// gesture's permission with it.
        fn el(&mut self) -> Option<&HtmlAudioElement> {
            if self.el.is_none() {
                let el = web_sys::window()?
                    .document()?
                    .create_element("audio")
                    .ok()?
                    .dyn_into::<HtmlAudioElement>()
                    .ok()?;
                el.set_preload("none");
                el.set_volume(self.level);
                el.set_muted(self.muted);
                watch_js(&el);
                self.el = Some(el);
            }
            self.el.as_ref()
        }

        /// The element's volume, 0 to 1, and whether it is muted — `None`
        /// where a page cannot set it.
        pub fn volume(&mut self) -> Option<(f64, bool)> {
            let el = self.el()?.clone();
            let settable = *self.settable.get_or_insert_with(|| {
                // Moved and read back: on a platform that keeps the volume
                // for itself, the read is the old value.
                let was = el.volume();
                el.set_volume(if was > 0.5 { 0.25 } else { 0.75 });
                let moved = (el.volume() - was).abs() > 0.01;
                el.set_volume(was);
                moved
            });
            settable.then(|| (el.volume(), el.muted()))
        }

        /// What the element was last given.
        pub fn level(&self) -> (f64, bool) {
            (self.level, self.muted)
        }

        /// Give the element a volume. Setting a volume or a mute is never
        /// what an autoplay policy refuses — that is starting a sound — so
        /// this needs no gesture.
        pub fn set_volume(&mut self, level: f64, muted: bool) {
            self.level = level.clamp(0.0, 1.0);
            self.muted = muted;
            let (level, muted) = (self.level, self.muted);
            if let Some(el) = self.el() {
                el.set_volume(level);
                el.set_muted(muted);
            }
        }

        pub fn play(&mut self, url: &str) {
            self.asked = true;
            if let Some(el) = self.el() {
                el.set_src(url);
                let _ = el.play();
            }
        }

        pub fn resume(&mut self) {
            self.asked = true;
            if let Some(el) = self.el() {
                let _ = el.play();
            }
        }

        pub fn pause(&mut self) {
            self.asked = false;
            if let Some(el) = self.el() {
                el.pause().ok();
            }
        }

        pub fn is_playing(&self) -> bool {
            self.el.as_ref().is_some_and(|el| !el.paused() && !el.ended())
        }

        /// Asked to play, and not at its end.
        pub fn asked(&self) -> bool {
            self.asked && !self.ended()
        }

        /// Whether it stopped by itself since the last ask — an autoplay
        /// policy, a stream gone, the platform's own pause: said once.
        pub fn take_stalled(&mut self) -> bool {
            let stopped = self.el.as_ref().is_some_and(|el| el.paused() && !el.ended());
            if self.asked && stopped {
                self.asked = false;
                return true;
            }
            false
        }

        /// Asked to play and waiting for data: `readyState` short of
        /// `HAVE_FUTURE_DATA`, so the position is standing still for the
        /// network and not for anybody's reason.
        pub fn buffering(&self) -> bool {
            self.el.as_ref().is_some_and(|el| !el.paused() && !el.ended() && el.ready_state() < 3)
        }

        pub fn ended(&self) -> bool {
            self.el.as_ref().is_some_and(|el| el.ended())
        }

        pub fn position(&self) -> f64 {
            self.el.as_ref().map(|el| el.current_time()).unwrap_or(0.0)
        }

        /// What the stream says it runs for, once enough of it has been read.
        /// `NaN` until then, and infinite for a live one — neither is a length,
        /// so both come back as `None` and the catalogue's figure is used.
        pub fn duration(&self) -> Option<f64> {
            let d = self.el.as_ref()?.duration();
            (d.is_finite() && d > 0.0).then_some(d)
        }

        pub fn seek(&mut self, secs: f64) {
            if let Some(el) = self.el() {
                el.set_current_time(secs);
            }
        }

        /// Say what is playing: the tab's title, `heading`, and the
        /// controller the operating system draws over the lock screen or
        /// beside the clock.
        pub fn announce(&mut self, heading: &str, track: Option<&Track>, playing: bool, duration: f64, at: f64) {
            let now = track.map(|t| (t.id.0, playing));
            if now != self.said {
                self.said = now;
                self.timed = -1;
                match track {
                    Some(t) => announce_js(heading, &t.title, &t.creator, &t.album, playing),
                    None => announce_js(heading, "", "", "", false),
                }
            }
            let whole = at as i64;
            if track.is_some() && whole != self.timed {
                self.timed = whole;
                position_js(duration, at);
            }
        }

        pub fn take_remote(&self) -> Option<Remote> {
            Remote::parse(&take_remote_js())
        }
    }
}

/// Ring the window with `f` whenever the media session or the `<audio>`
/// element says something happened (`wake.rs`).
#[cfg(target_arch = "wasm32")]
pub use imp::ring_with;

#[cfg(not(target_arch = "wasm32"))]
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

    /// Say what is playing, where the platform shows such things: the tab's
    /// title, `heading`, and the controller Windows draws beside the clock —
    /// what the window says is playing (`App::now`: its track, whether it
    /// plays, its duration and where it is; `App::title`), which is not
    /// always this element: a window watching another device says that
    /// device's track.
    ///
    /// Called after every update rather than from each place that changes
    /// something, because the element changes it too — a stream that stalls
    /// or runs out was nobody's button press, and a controller still showing
    /// "playing" for it is worse than one that is a frame behind. The
    /// element rings the window when it does (`watch` above), and while it
    /// plays the window's clock moves the controller's scrubber.
    #[cfg(target_arch = "wasm32")]
    pub fn announce(&mut self, heading: &str, now: Option<(crate::listening::Track, bool, f64, f64)>) {
        match &now {
            Some((track, playing, duration, at)) => self.sink.announce(heading, Some(track), *playing, *duration, *at),
            None => self.sink.announce(heading, None, false, 0.0, 0.0),
        }
    }

    /// What a lock-screen button asked for since the last update, if anything.
    #[cfg(target_arch = "wasm32")]
    pub fn take_remote(&self) -> Option<Remote> {
        self.sink.take_remote()
    }

    pub fn track(&self) -> Option<&Track> {
        self.track.as_ref()
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
