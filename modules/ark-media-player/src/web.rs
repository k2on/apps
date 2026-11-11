//! The browser's player (feature `web`, wasm32): an `<audio>` element,
//! never attached to the page, and the Media Session API
//! (`navigator.mediaSession`): its metadata, its action handlers — the
//! media keys, a headset, the browser's and the system's media controls —
//! each said as a [`MediaEvent::Remote`], and its position state.
//!
//! The Media Session API is reached by name (`js_sys::Reflect`), so a
//! browser without it plays all the same, with no controls but the page's;
//! web-sys has it only behind `web_sys_unstable_apis`. A browser browses no
//! tree: [`MediaPlayer::set_children`] is kept nowhere.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use js_sys::{Array, Function, Object, Reflect};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::HtmlAudioElement;

use crate::{BrowseItem, MediaControls, MediaEvent, MediaListener, MediaPlayer, NowPlaying, RemoteCommand};

/// A listener or a handler the page calls.
type Callback = Closure<dyn FnMut(JsValue)>;

struct Inner {
    audio: HtmlAudioElement,
    listener: RefCell<Option<Arc<dyn MediaListener>>>,
    controls: RefCell<MediaControls>,
    loaded: RefCell<bool>,
    /// The element's listeners, kept alive as long as the player is.
    held: RefCell<Vec<Callback>>,
    /// The session's handlers, for the controls offered now.
    actions: RefCell<Vec<Callback>>,
}

impl Inner {
    fn tell(&self, e: MediaEvent) {
        let l = self.listener.borrow().clone();
        if let Some(l) = l {
            l.heard(e)
        }
    }

    fn ms(secs: f64) -> i64 {
        if secs.is_finite() {
            (secs * 1000.0).round() as i64
        } else {
            0
        }
    }
}

/// The page's player. One per page: the Media Session is the page's.
#[derive(Clone)]
pub struct Browser {
    inner: Rc<Inner>,
}

// wasm32 without threads has one thread: the port's `Send + Sync` is
// never crossed.
unsafe impl Send for Browser {}
unsafe impl Sync for Browser {}

fn session() -> Option<Object> {
    let nav = web_sys::window()?.navigator();
    let s = Reflect::get(&nav, &"mediaSession".into()).ok()?;
    (!s.is_undefined() && !s.is_null()).then(|| s.unchecked_into())
}

fn set(o: &JsValue, k: &str, v: impl Into<JsValue>) {
    let _ = Reflect::set(o, &k.into(), &v.into());
}

impl Browser {
    /// The page's player; none where the page has no `<audio>`.
    pub fn new() -> Option<Browser> {
        let audio = HtmlAudioElement::new().ok()?;
        audio.set_preload("auto");
        let inner = Rc::new(Inner {
            audio,
            listener: RefCell::new(None),
            controls: RefCell::new(MediaControls::default()),
            loaded: RefCell::new(false),
            held: RefCell::new(vec![]),
            actions: RefCell::new(vec![]),
        });
        let b = Browser { inner };
        b.listen();
        b.handle_actions();
        Some(b)
    }

    fn on(&self, event: &str, f: impl Fn(&Inner, JsValue) + 'static) {
        let weak = Rc::downgrade(&self.inner);
        let c = Closure::<dyn FnMut(JsValue)>::new(move |e: JsValue| {
            if let Some(i) = weak.upgrade() {
                f(&i, e)
            }
        });
        let _ = self.inner.audio.add_event_listener_with_callback(event, c.as_ref().unchecked_ref());
        self.inner.held.borrow_mut().push(c);
    }

    fn listen(&self) {
        self.on("loadedmetadata", |i, _| {
            *i.loaded.borrow_mut() = true;
            i.tell(MediaEvent::Loaded {
                duration_ms: Inner::ms(i.audio.duration()),
            })
        });
        self.on("timeupdate", |i, _| {
            i.tell(MediaEvent::Position {
                at_ms: Inner::ms(i.audio.current_time()),
            });
            position_state(i);
        });
        self.on("seeked", |i, _| {
            i.tell(MediaEvent::Position {
                at_ms: Inner::ms(i.audio.current_time()),
            })
        });
        self.on("playing", |i, _| {
            playback_state("playing");
            i.tell(MediaEvent::Playing)
        });
        self.on("waiting", |i, _| i.tell(MediaEvent::Waiting));
        self.on("pause", |i, _| {
            playback_state("paused");
            i.tell(MediaEvent::Paused)
        });
        self.on("ended", |i, _| i.tell(MediaEvent::Ended));
        self.on("error", |i, _| {
            let why = i
                .audio
                .error()
                .map(|e| match e.message() {
                    m if !m.is_empty() => m,
                    _ => format!("media error {}", e.code()),
                })
                .unwrap_or_else(|| "the sound could not be played".into());
            i.tell(MediaEvent::Failed { why })
        });
    }

    /// Each of the session's actions, said as a remote command; those the
    /// controls do not offer taken away.
    fn handle_actions(&self) {
        let Some(s) = session() else { return };
        let Ok(handler) = Reflect::get(&s, &"setActionHandler".into()) else {
            return;
        };
        let handler: Function = handler.unchecked_into();
        let c = *self.inner.controls.borrow();
        let actions: [(&str, bool); 8] = [
            ("play", true),
            ("pause", true),
            ("stop", true),
            ("nexttrack", c.has_next),
            ("previoustrack", c.has_previous),
            ("seekforward", c.skip_forward_s > 0),
            ("seekbackward", c.skip_back_s > 0),
            ("seekto", c.can_seek),
        ];
        for (action, offered) in actions {
            if !offered {
                let _ = handler.call2(&s, &action.into(), &JsValue::NULL);
                continue;
            }
            let weak = Rc::downgrade(&self.inner);
            let name = action.to_string();
            let f = Closure::<dyn FnMut(JsValue)>::new(move |details: JsValue| {
                let Some(i) = weak.upgrade() else { return };
                let c = *i.controls.borrow();
                let num = |k: &str| Reflect::get(&details, &k.into()).ok().and_then(|v| v.as_f64());
                let command = match name.as_str() {
                    "play" => RemoteCommand::Play,
                    "pause" => RemoteCommand::Pause,
                    "stop" => RemoteCommand::Stop,
                    "nexttrack" => RemoteCommand::Next,
                    "previoustrack" => RemoteCommand::Previous,
                    "seekforward" => RemoteCommand::SkipForward {
                        seconds: num("seekOffset").map_or(c.skip_forward_s, |s| s.round() as u32),
                    },
                    "seekbackward" => RemoteCommand::SkipBack {
                        seconds: num("seekOffset").map_or(c.skip_back_s, |s| s.round() as u32),
                    },
                    _ => RemoteCommand::SeekTo {
                        at_ms: Inner::ms(num("seekTime").unwrap_or(0.0)),
                    },
                };
                i.tell(MediaEvent::Remote { command })
            });
            let _ = handler.call2(&s, &action.into(), f.as_ref());
            self.inner.actions.borrow_mut().push(f);
        }
    }
}

fn playback_state(state: &str) {
    if let Some(s) = session() {
        set(&s, "playbackState", state);
    }
}

/// Where the sound is, for the system's scrubber.
fn position_state(i: &Inner) {
    let Some(s) = session() else { return };
    let d = i.audio.duration();
    if !d.is_finite() || d <= 0.0 {
        return;
    }
    let state = Object::new();
    set(&state, "duration", d);
    set(&state, "playbackRate", i.audio.playback_rate());
    set(&state, "position", i.audio.current_time().clamp(0.0, d));
    if let Ok(f) = Reflect::get(&s, &"setPositionState".into()) {
        let _ = f.unchecked_into::<Function>().call1(&s, &state);
    }
}

impl MediaPlayer for Browser {
    fn load(&self, url: String, at_ms: i64) {
        let a = &self.inner.audio;
        *self.inner.loaded.borrow_mut() = false;
        a.set_src(&url);
        a.load();
        a.set_current_time(at_ms.max(0) as f64 / 1000.0);
    }

    fn play(&self) {
        let Ok(p) = self.inner.audio.play() else { return };
        // Refused (a page nobody has touched yet may not sound): said. An
        // abort is a pause or a load that came first, and nothing.
        // Once: it frees itself when called (and is a few bytes kept when not).
        let weak = Rc::downgrade(&self.inner);
        let refused = Closure::<dyn FnMut(JsValue)>::once_into_js(move |e: JsValue| {
            let name = Reflect::get(&e, &"name".into()).ok().and_then(|n| n.as_string()).unwrap_or_default();
            if name == "AbortError" {
                return;
            }
            let why = Reflect::get(&e, &"message".into()).ok().and_then(|m| m.as_string()).unwrap_or(name);
            if let Some(i) = weak.upgrade() {
                i.tell(MediaEvent::Failed { why })
            }
        });
        if let Ok(catch) = Reflect::get(&p, &"catch".into()) {
            let _ = catch.unchecked_into::<Function>().call1(&p, &refused);
        }
    }

    fn pause(&self) {
        let _ = self.inner.audio.pause();
    }

    fn seek(&self, at_ms: i64) {
        self.inner.audio.set_current_time(at_ms.max(0) as f64 / 1000.0);
    }

    fn set_rate(&self, rate: f32) {
        self.inner.audio.set_playback_rate(rate as f64);
    }

    fn set_volume(&self, volume: f32) {
        self.inner.audio.set_volume(volume.clamp(0.0, 1.0) as f64);
    }

    fn stop(&self) {
        let a = &self.inner.audio;
        let _ = a.pause();
        let _ = a.remove_attribute("src");
        a.load();
        playback_state("none");
    }

    fn set_now_playing(&self, info: Option<NowPlaying>) {
        let Some(s) = session() else { return };
        let Some(info) = info else {
            set(&s, "metadata", JsValue::NULL);
            playback_state("none");
            return;
        };
        let init = Object::new();
        set(&init, "title", info.title.as_str());
        set(&init, "artist", info.artist.as_str());
        set(&init, "album", info.album.as_str());
        if let Some(art) = &info.artwork {
            let image = Object::new();
            set(&image, "src", art.as_str());
            set(&init, "artwork", Array::of1(&image));
        }
        let Some(window) = web_sys::window() else { return };
        if let Ok(ctor) = Reflect::get(&window, &"MediaMetadata".into()) {
            if let Ok(m) = Reflect::construct(&ctor.unchecked_into::<Function>(), &Array::of1(&init)) {
                set(&s, "metadata", m);
            }
        }
        // Shown for a sound made elsewhere; this player's own says itself.
        if !*self.inner.loaded.borrow() && info.duration_ms > 0 {
            let state = Object::new();
            let d = info.duration_ms as f64 / 1000.0;
            set(&state, "duration", d);
            set(&state, "playbackRate", if info.rate > 0.0 { info.rate as f64 } else { 1.0 });
            set(&state, "position", (info.position_ms as f64 / 1000.0).clamp(0.0, d));
            if let Ok(f) = Reflect::get(&s, &"setPositionState".into()) {
                let _ = f.unchecked_into::<Function>().call1(&s, &state);
            }
            playback_state(if info.rate > 0.0 { "playing" } else { "paused" });
        }
    }

    fn set_controls(&self, controls: MediaControls) {
        if *self.inner.controls.borrow() == controls {
            return;
        }
        *self.inner.controls.borrow_mut() = controls;
        // The new set replaces each handler; the old ones go after.
        let old = std::mem::take(&mut *self.inner.actions.borrow_mut());
        self.handle_actions();
        drop(old);
    }

    fn set_children(&self, _parent: String, _items: Vec<BrowseItem>) {}

    fn set_listener(&self, listener: Option<Arc<dyn MediaListener>>) {
        *self.inner.listener.borrow_mut() = listener;
    }
}
