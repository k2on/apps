//! The player, faked for tests: what it was asked, as words, what the
//! system would show and the browse tree it was given, kept to look at;
//! and events told to its listener when the test says.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use ark_module::Modules;

use crate::{BrowseItem, MediaControls, MediaEvent, MediaListener, MediaPlayer, NowPlaying};

#[derive(Default)]
struct Seen {
    asked: Vec<String>,
    now_playing: Option<NowPlaying>,
    controls: MediaControls,
    children: HashMap<String, Vec<BrowseItem>>,
    listener: Option<Arc<dyn MediaListener>>,
}

#[derive(Clone, Default)]
pub struct Fake {
    seen: Arc<Mutex<Seen>>,
}

impl Fake {
    pub fn new() -> Fake {
        Fake::default()
    }

    fn seen(&self) -> MutexGuard<'_, Seen> {
        self.seen.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// This fake registered as the player.
    pub fn register(&self, modules: &mut Modules) {
        modules.register::<dyn MediaPlayer>(Arc::new(self.clone()));
    }

    /// What the player was asked, as words (`load url 0`, `play`, `seek 5`,
    /// `rate 1.5`, `volume 0.5`, `stop`), in order; what it shows and its
    /// tree are [`Fake::now_playing`], [`Fake::controls`], [`Fake::browse`].
    pub fn asked(&self) -> Vec<String> {
        self.seen().asked.clone()
    }

    /// What it was asked since the last take, taken.
    pub fn take(&self) -> Vec<String> {
        std::mem::take(&mut self.seen().asked)
    }

    pub fn now_playing(&self) -> Option<NowPlaying> {
        self.seen().now_playing.clone()
    }

    pub fn controls(&self) -> MediaControls {
        self.seen().controls
    }

    /// What `parent` holds, as a car would ask: the children the app gave,
    /// or none yet — and then the app is asked, as the system would
    /// ([`MediaEvent::Browse`]).
    pub fn browse(&self, parent: &str) -> Option<Vec<BrowseItem>> {
        let got = self.seen().children.get(parent).cloned();
        if got.is_none() {
            self.hear(MediaEvent::Browse { parent: parent.into() })
        }
        got
    }

    /// Whether a listener is registered.
    pub fn listened(&self) -> bool {
        self.seen().listener.is_some()
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
    fn stop(&self) {
        self.note("stop".into())
    }
    fn set_now_playing(&self, info: Option<NowPlaying>) {
        self.seen().now_playing = info
    }
    fn set_controls(&self, controls: MediaControls) {
        self.seen().controls = controls
    }
    fn set_children(&self, parent: String, items: Vec<BrowseItem>) {
        self.seen().children.insert(parent, items);
    }
    fn set_listener(&self, listener: Option<Arc<dyn MediaListener>>) {
        self.seen().listener = listener
    }
}
