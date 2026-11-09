//! Every module, faked for tests: what was asked of each is kept to look
//! at, the camera answers when the test says, the secure store is a map.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use crate::*;

/// What the fakes were asked, in order.
#[derive(Default)]
struct Seen {
    haptics: Vec<Haptic>,
    copied: Vec<String>,
    shared: Vec<(String, Option<String>)>,
    scans: Vec<Arc<Reply>>,
    photos: Vec<Arc<Reply>>,
    notices: BTreeMap<String, Notice>,
    secrets: BTreeMap<String, String>,
    media: Vec<String>,
    listener: Option<Arc<dyn MediaListener>>,
}

/// Every module, faked; [`Fakes::modules`] registers them all.
#[derive(Clone, Default)]
pub struct Fakes {
    seen: Arc<Mutex<Seen>>,
}

impl Fakes {
    pub fn new() -> Fakes {
        Fakes::default()
    }

    fn seen(&self) -> std::sync::MutexGuard<'_, Seen> {
        self.seen.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Every module, these fakes.
    pub fn modules(&self) -> Modules {
        let me = Arc::new(self.clone());
        Modules {
            haptics: Some(me.clone()),
            clipboard: Some(me.clone()),
            share: Some(me.clone()),
            camera: Some(me.clone()),
            notifications: Some(me.clone()),
            secure: Some(me.clone()),
            media: Some(me),
        }
    }

    /// The haptics performed.
    pub fn haptics(&self) -> Vec<Haptic> {
        self.seen().haptics.clone()
    }

    /// What was put on the clipboard.
    pub fn copied(&self) -> Vec<String> {
        self.seen().copied.clone()
    }

    /// What was handed to the share sheet.
    pub fn shared(&self) -> Vec<(String, Option<String>)> {
        self.seen().shared.clone()
    }

    /// The oldest scan waiting, answered with `code`.
    pub fn scanned(&self, code: Option<String>) {
        let reply = {
            let mut s = self.seen();
            (!s.scans.is_empty()).then(|| s.scans.remove(0))
        };
        if let Some(r) = reply {
            r.answer(ModuleAnswer::Scanned { code })
        }
    }

    /// The oldest photo waiting, answered with `jpeg`.
    pub fn photographed(&self, jpeg: Option<Vec<u8>>) {
        let reply = {
            let mut s = self.seen();
            (!s.photos.is_empty()).then(|| s.photos.remove(0))
        };
        if let Some(r) = reply {
            r.answer(ModuleAnswer::Photo { jpeg })
        }
    }

    /// The notices scheduled and not cancelled.
    pub fn notices(&self) -> Vec<Notice> {
        self.seen().notices.values().cloned().collect()
    }

    /// What the player was asked, as words (`load url 0`, `play`, …).
    pub fn media(&self) -> Vec<String> {
        self.seen().media.clone()
    }

    /// `event`, as the player would tell it.
    pub fn hear(&self, event: MediaEvent) {
        let l = self.seen().listener.clone();
        if let Some(l) = l {
            l.heard(event)
        }
    }
}

impl Haptics for Fakes {
    fn perform(&self, haptic: Haptic) {
        self.seen().haptics.push(haptic)
    }
}

impl Clipboard for Fakes {
    fn copy_text(&self, text: String) {
        self.seen().copied.push(text)
    }
}

impl Share for Fakes {
    fn share(&self, text: String, url: Option<String>) {
        self.seen().shared.push((text, url))
    }
}

impl Camera for Fakes {
    fn scan_qr(&self, reply: Arc<Reply>) {
        self.seen().scans.push(reply)
    }

    fn take_photo(&self, reply: Arc<Reply>) {
        self.seen().photos.push(reply)
    }
}

impl Notifications for Fakes {
    fn schedule(&self, notice: Notice) {
        self.seen().notices.insert(notice.id.clone(), notice);
    }

    fn cancel(&self, id: String) {
        self.seen().notices.remove(&id);
    }
}

impl SecureStore for Fakes {
    fn get(&self, key: String) -> Option<String> {
        self.seen().secrets.get(&key).cloned()
    }

    fn put(&self, key: String, value: String) {
        self.seen().secrets.insert(key, value);
    }

    fn remove(&self, key: String) {
        self.seen().secrets.remove(&key);
    }
}

impl MediaPlayer for Fakes {
    fn load(&self, url: String, at_ms: i64) {
        self.seen().media.push(format!("load {url} {at_ms}"))
    }

    fn play(&self) {
        self.seen().media.push("play".into())
    }

    fn pause(&self) {
        self.seen().media.push("pause".into())
    }

    fn seek(&self, at_ms: i64) {
        self.seen().media.push(format!("seek {at_ms}"))
    }

    fn set_rate(&self, rate: f32) {
        self.seen().media.push(format!("rate {rate}"))
    }

    fn set_volume(&self, volume: f32) {
        self.seen().media.push(format!("volume {volume}"))
    }

    fn set_now_playing(&self, info: Option<NowPlayingInfo>) {
        self.seen()
            .media
            .push(format!("now playing {}", info.map(|i| i.title).unwrap_or_default()))
    }

    fn set_listener(&self, listener: Option<Arc<dyn MediaListener>>) {
        self.seen().listener = listener
    }
}
