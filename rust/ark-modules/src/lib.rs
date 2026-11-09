//! What an Ark app asks of the device, the way Expo's modules do it: each
//! capability one trait, written once here, implemented natively on every
//! platform and registered when the app starts.
//!
//! - [`Haptics`] — the phone's answer in the hand (`docs/motion.md`,
//!   Haptics): a success, a selection step, a warning.
//! - [`Clipboard`] and [`Share`] — text put where the person can paste it,
//!   or handed to the system's share sheet.
//! - [`Camera`] — a QR code scanned, a photo taken (VisionKit and
//!   AVFoundation on the iPhone, CameraX on Android).
//! - [`MediaPlayer`] — a sound loaded, played, paused, sought, at a rate
//!   and a volume, with the lock screen's now playing; what it did comes
//!   back as [`MediaEvent`]s. harken's `PhoneSound` generalised.
//! - [`Notifications`] — a local notification at a time, or none.
//! - [`SecureStore`] — a secret kept in the keychain or the keystore (a
//!   login's token).
//!
//! An app's `update` (`ark_app`) never calls a module itself: it returns a
//! [`ModuleCall`], the runtime hands it to the [`Modules`] the shell
//! registered, and what comes of it ([`ModuleAnswer`]) goes back into the
//! update as a message. So an update is a pure function a test runs with
//! [`fake::Fakes`], and a call is data that crosses the bridge.
//!
//! On the phones the traits are UniFFI foreign traits (feature `ffi`): the
//! Swift and Kotlin classes in the ArkUI libraries implement them. On the
//! desktop and in the browser they are implemented in Rust (`arkui::modules`
//! for iced; the app's web crate over web-sys). A platform that has no such
//! thing (haptics on the desktop) registers none, and the call is answered
//! [`ModuleAnswer::Unavailable`].

use std::sync::{Arc, Mutex};

#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!();

pub mod fake;

// * The calls

/// What a haptic says: `docs/motion.md`'s three, kept for what matters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "ffi", derive(uniffi::Enum))]
pub enum Haptic {
    /// Something was kept: a transaction saved, an account added.
    Success,
    /// A choice stepped: a month, a tick, a detent.
    Selection,
    /// Something went that cannot simply come back: an account closed.
    Warning,
}

/// A local notification: what it says, and when (milliseconds since the
/// epoch; none is now).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct Notice {
    /// Its id: a second notice of one id replaces the first.
    pub id: String,
    pub title: String,
    pub body: String,
    pub at_ms: Option<i64>,
    /// The route it opens, as a path (`/account/…`), when tapped.
    pub opens: Option<String>,
}

/// What the lock screen and the system's controls show of what plays.
#[derive(Clone, Debug, PartialEq, Default)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct NowPlayingInfo {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// The artwork's address, when there is one.
    pub artwork: Option<String>,
    pub duration_ms: i64,
}

/// One thing an update asks of the device.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Enum))]
pub enum ModuleCall {
    Haptic { haptic: Haptic },
    Copy { text: String },
    Share { text: String, url: Option<String> },
    ScanQr,
    TakePhoto,
    Notify { notice: Notice },
    Unnotify { id: String },
    SecretGet { key: String },
    SecretPut { key: String, value: String },
    SecretRemove { key: String },
}

/// What came of a call.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Enum))]
pub enum ModuleAnswer {
    /// It was done (or, for a haptic, asked: nothing says whether it was felt).
    Done,
    /// What a scan read; none when the person backed out.
    Scanned { code: Option<String> },
    /// A photo, as its JPEG's bytes; none when the person backed out.
    Photo { jpeg: Option<Vec<u8>> },
    /// A secret read; none when nothing is kept under the key.
    Secret { value: Option<String> },
    /// The platform has no such module, or it is not registered.
    Unavailable,
    /// It failed, in the platform's words.
    Failed { why: String },
}

// * The modules

/// The phone's answer in the hand. Fired on the change itself, never on a
/// tap that might change nothing (`docs/motion.md`, Haptics).
#[cfg_attr(feature = "ffi", uniffi::export(with_foreign))]
pub trait Haptics: Send + Sync {
    fn perform(&self, haptic: Haptic);
}

/// Text put on the clipboard.
#[cfg_attr(feature = "ffi", uniffi::export(with_foreign))]
pub trait Clipboard: Send + Sync {
    fn copy_text(&self, text: String);
}

/// The system's share sheet.
#[cfg_attr(feature = "ffi", uniffi::export(with_foreign))]
pub trait Share: Send + Sync {
    fn share(&self, text: String, url: Option<String>);
}

/// What a reply hands its answer to.
type Answering = Box<dyn FnOnce(ModuleAnswer) + Send>;

/// Where an asynchronous module says what came of a call, once.
#[cfg_attr(feature = "ffi", derive(uniffi::Object))]
pub struct Reply {
    to: Mutex<Option<Answering>>,
}

impl Reply {
    /// A reply that hands its answer to `to`.
    pub fn new(to: impl FnOnce(ModuleAnswer) + Send + 'static) -> Arc<Reply> {
        Arc::new(Reply {
            to: Mutex::new(Some(Box::new(to))),
        })
    }
}

#[cfg_attr(feature = "ffi", uniffi::export)]
impl Reply {
    /// What came of the call; only the first answer is heard.
    pub fn answer(&self, answer: ModuleAnswer) {
        let to = self.to.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(to) = to {
            to(answer)
        }
    }
}

/// The camera: a QR code read, a photo taken. Each shows the platform's
/// own camera page and answers when it closes.
#[cfg_attr(feature = "ffi", uniffi::export(with_foreign))]
pub trait Camera: Send + Sync {
    /// Answers [`ModuleAnswer::Scanned`].
    fn scan_qr(&self, reply: Arc<Reply>);
    /// Answers [`ModuleAnswer::Photo`].
    fn take_photo(&self, reply: Arc<Reply>);
}

/// Local notifications.
#[cfg_attr(feature = "ffi", uniffi::export(with_foreign))]
pub trait Notifications: Send + Sync {
    fn schedule(&self, notice: Notice);
    fn cancel(&self, id: String);
}

/// Secrets kept by the system: the keychain, the Android keystore.
#[cfg_attr(feature = "ffi", uniffi::export(with_foreign))]
pub trait SecureStore: Send + Sync {
    fn get(&self, key: String) -> Option<String>;
    fn put(&self, key: String, value: String);
    fn remove(&self, key: String);
}

/// What a player did: what the app's update hears from it.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Enum))]
pub enum MediaEvent {
    /// It has the sound and knows how long it is.
    Loaded {
        duration_ms: i64,
    },
    /// Where it is, now and then while it plays.
    Position {
        at_ms: i64,
    },
    Playing,
    Paused,
    /// It reached the end.
    Ended,
    /// The system's controls asked (the lock screen, a headset).
    Remote {
        command: RemoteCommand,
    },
    Failed {
        why: String,
    },
}

/// What the system's controls can ask of a player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Enum))]
pub enum RemoteCommand {
    Play,
    Pause,
    Next,
    Previous,
    SkipForward,
    SkipBack,
}

/// Where a player tells what it did.
#[cfg_attr(feature = "ffi", uniffi::export(with_foreign))]
pub trait MediaListener: Send + Sync {
    fn heard(&self, event: MediaEvent);
}

/// A player of one sound at a time: harken's `PhoneSound` and
/// `harken_player::Sound`, generalised (docs/ark-app.md, MediaPlayer). The
/// app's update drives it; what it did comes back through the listener.
#[cfg_attr(feature = "ffi", uniffi::export(with_foreign))]
pub trait MediaPlayer: Send + Sync {
    /// The sound at `url` (a file or a stream), from `at_ms`, paused.
    fn load(&self, url: String, at_ms: i64);
    fn play(&self);
    fn pause(&self);
    fn seek(&self, at_ms: i64);
    /// 1.0 is as recorded.
    fn set_rate(&self, rate: f32);
    /// 0.0 to 1.0.
    fn set_volume(&self, volume: f32);
    fn set_now_playing(&self, info: Option<NowPlayingInfo>);
    fn set_listener(&self, listener: Option<Arc<dyn MediaListener>>);
}

// * The registry

/// The modules a shell registered at startup; any of them may be missing.
#[derive(Clone, Default)]
pub struct Modules {
    pub haptics: Option<Arc<dyn Haptics>>,
    pub clipboard: Option<Arc<dyn Clipboard>>,
    pub share: Option<Arc<dyn Share>>,
    pub camera: Option<Arc<dyn Camera>>,
    pub notifications: Option<Arc<dyn Notifications>>,
    pub secure: Option<Arc<dyn SecureStore>>,
    pub media: Option<Arc<dyn MediaPlayer>>,
}

impl Modules {
    /// `call` carried out by the module registered for it; what came of it
    /// handed to `reply`, at once or (the camera) when the person is done.
    pub fn call(&self, call: ModuleCall, reply: impl FnOnce(ModuleAnswer) + Send + 'static) {
        let missing = ModuleAnswer::Unavailable;
        match call {
            ModuleCall::Haptic { haptic } => reply(self.haptics.as_ref().map_or(missing, |m| {
                m.perform(haptic);
                ModuleAnswer::Done
            })),
            ModuleCall::Copy { text } => reply(self.clipboard.as_ref().map_or(missing, |m| {
                m.copy_text(text);
                ModuleAnswer::Done
            })),
            ModuleCall::Share { text, url } => reply(self.share.as_ref().map_or(missing, |m| {
                m.share(text, url);
                ModuleAnswer::Done
            })),
            ModuleCall::ScanQr => match &self.camera {
                Some(m) => m.scan_qr(Reply::new(reply)),
                None => reply(missing),
            },
            ModuleCall::TakePhoto => match &self.camera {
                Some(m) => m.take_photo(Reply::new(reply)),
                None => reply(missing),
            },
            ModuleCall::Notify { notice } => reply(self.notifications.as_ref().map_or(missing, |m| {
                m.schedule(notice);
                ModuleAnswer::Done
            })),
            ModuleCall::Unnotify { id } => reply(self.notifications.as_ref().map_or(missing, |m| {
                m.cancel(id);
                ModuleAnswer::Done
            })),
            ModuleCall::SecretGet { key } => reply(self.secure.as_ref().map_or(missing, |m| ModuleAnswer::Secret { value: m.get(key) })),
            ModuleCall::SecretPut { key, value } => reply(self.secure.as_ref().map_or(missing, |m| {
                m.put(key, value);
                ModuleAnswer::Done
            })),
            ModuleCall::SecretRemove { key } => reply(self.secure.as_ref().map_or(missing, |m| {
                m.remove(key);
                ModuleAnswer::Done
            })),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fake::Fakes;

    #[test]
    fn a_call_reaches_its_module_and_answers() {
        let fakes = Fakes::new();
        let got = Arc::new(Mutex::new(vec![]));
        let g = got.clone();
        fakes
            .modules()
            .call(ModuleCall::Copy { text: "$12.00".into() }, move |a| g.lock().unwrap().push(a));
        assert_eq!(fakes.copied(), vec!["$12.00".to_string()]);
        assert_eq!(*got.lock().unwrap(), vec![ModuleAnswer::Done]);
    }

    #[test]
    fn a_missing_module_is_unavailable() {
        let got = Arc::new(Mutex::new(vec![]));
        let g = got.clone();
        Modules::default().call(ModuleCall::Haptic { haptic: Haptic::Success }, move |a| g.lock().unwrap().push(a));
        assert_eq!(*got.lock().unwrap(), vec![ModuleAnswer::Unavailable]);
    }

    #[test]
    fn the_camera_answers_later_and_once() {
        let fakes = Fakes::new();
        let got = Arc::new(Mutex::new(vec![]));
        let g = got.clone();
        fakes.modules().call(ModuleCall::ScanQr, move |a| g.lock().unwrap().push(a));
        assert!(got.lock().unwrap().is_empty());
        fakes.scanned(Some("money://join?server=a".into()));
        fakes.scanned(None);
        assert_eq!(
            *got.lock().unwrap(),
            vec![ModuleAnswer::Scanned {
                code: Some("money://join?server=a".into())
            }]
        );
    }
}
