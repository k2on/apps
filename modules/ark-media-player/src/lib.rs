//! The media player: one sound at a time, with what the system shows of it
//! and the tree a car or a watch browses. Its port — the [`MediaPlayer`]
//! and [`MediaListener`] traits, the calls ([`load`], [`play`], …, each a
//! [`MediaOp`] whose output is `()`), the [`MediaEvent`]s and the browse
//! tree's [`BrowseItem`] — is the runtime's (`ark_module::media_player`),
//! which an update asks through; here is what implements it:
//!
//! - iPhone, iPad and Mac: `PhoneMediaPlayer` (swift/: AVPlayer, the
//!   playback audio session, `MPNowPlayingInfoCenter` and
//!   `MPRemoteCommandCenter`), through `phone` (feature `ffi`).
//! - Android: `PhoneMediaPlayer` (kotlin/: Media3's ExoPlayer in
//!   `ArkMediaService`, a `MediaLibraryService`), through `phone` (feature `ffi`).
//! - The browser (feature `web`, wasm32): `Browser`, an `<audio>` element
//!   and the Media Session API.
//! - The Linux desktop (feature `desktop`): `Desktop`, a decoder and the
//!   sound card (rodio over symphonia), the stream fetched by range, and
//!   MPRIS on the session bus for the media keys and the desktop's widgets.
//! - Tests: [`fake::Fake`], which keeps what it was asked and tells its
//!   listener what a test says.

pub use ark_module::media_player::*;

#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!();

pub mod fake;
pub use fake::Fake;

#[cfg(all(feature = "web", target_arch = "wasm32"))]
mod web;
#[cfg(all(feature = "web", target_arch = "wasm32"))]
pub use web::Browser;

#[cfg(feature = "desktop")]
mod desktop;
#[cfg(feature = "desktop")]
pub use desktop::Desktop;

/// The phones' player, through UniFFI (`ffi`): the foreign trait Swift's
/// and Kotlin's `PhoneMediaPlayer` implement, the listener they tell, and
/// it registered as the port.
#[cfg(feature = "ffi")]
pub mod phone {
    use std::sync::Arc;

    use super::{BrowseItem, MediaControls, MediaEvent, NowPlaying, RemoteCommand, RouteChange};

    #[uniffi::remote(Record)]
    pub struct NowPlaying {
        pub title: String,
        pub artist: String,
        pub album: String,
        pub artwork: Option<String>,
        pub placeholder: Option<String>,
        pub duration_ms: i64,
        pub position_ms: i64,
        pub rate: f32,
    }

    #[uniffi::remote(Record)]
    pub struct MediaControls {
        pub has_next: bool,
        pub has_previous: bool,
        pub skip_forward_s: u32,
        pub skip_back_s: u32,
        pub can_seek: bool,
        pub can_rate: bool,
    }

    #[uniffi::remote(Enum)]
    pub enum RemoteCommand {
        Play,
        Pause,
        Toggle,
        Stop,
        Next,
        Previous,
        SkipForward { seconds: u32 },
        SkipBack { seconds: u32 },
        SeekTo { at_ms: i64 },
        Rate { rate: f32 },
    }

    #[uniffi::remote(Enum)]
    pub enum RouteChange {
        Unplugged,
        Plugged,
        Other,
    }

    #[uniffi::remote(Record)]
    pub struct BrowseItem {
        pub id: String,
        pub title: String,
        pub subtitle: String,
        pub artwork: Option<String>,
        pub playable: bool,
        pub browsable: bool,
    }

    #[uniffi::remote(Enum)]
    pub enum MediaEvent {
        Loaded { duration_ms: i64 },
        Position { at_ms: i64 },
        Playing,
        Waiting,
        Paused,
        Ended,
        Remote { command: RemoteCommand },
        Failed { why: String },
        Interrupted,
        Resumed,
        Route { change: RouteChange },
        Browse { parent: String },
        PlayItem { id: String },
    }

    /// Where Swift and Kotlin tell what the player did: the app's listener.
    #[uniffi::export(with_foreign)]
    pub trait MediaListener: Send + Sync {
        fn heard(&self, event: MediaEvent);
    }

    /// The phone's player, as Swift and Kotlin implement it (named apart
    /// from Apple's MediaPlayer framework). Every call returns at once; the
    /// implementation does it on its own thread.
    #[uniffi::export(with_foreign)]
    pub trait NativeMediaPlayer: Send + Sync {
        fn load(&self, url: String, at_ms: i64);
        fn play(&self);
        fn pause(&self);
        fn seek(&self, at_ms: i64);
        fn set_rate(&self, rate: f32);
        fn set_volume(&self, volume: f32);
        fn stop(&self);
        fn set_now_playing(&self, info: Option<NowPlaying>);
        fn set_controls(&self, controls: MediaControls);
        fn set_children(&self, parent: String, items: Vec<BrowseItem>);
        fn set_listener(&self, listener: Option<Arc<dyn MediaListener>>);
    }

    struct Phone(Arc<dyn NativeMediaPlayer>);

    /// The port's listener, as the phone's.
    struct Tell(Arc<dyn super::MediaListener>);

    impl MediaListener for Tell {
        fn heard(&self, event: MediaEvent) {
            self.0.heard(event)
        }
    }

    impl super::MediaPlayer for Phone {
        fn load(&self, url: String, at_ms: i64) {
            self.0.load(url, at_ms)
        }
        fn play(&self) {
            self.0.play()
        }
        fn pause(&self) {
            self.0.pause()
        }
        fn seek(&self, at_ms: i64) {
            self.0.seek(at_ms)
        }
        fn set_rate(&self, rate: f32) {
            self.0.set_rate(rate)
        }
        fn set_volume(&self, volume: f32) {
            self.0.set_volume(volume)
        }
        fn stop(&self) {
            self.0.stop()
        }
        fn set_now_playing(&self, info: Option<NowPlaying>) {
            self.0.set_now_playing(info)
        }
        fn set_controls(&self, controls: MediaControls) {
            self.0.set_controls(controls)
        }
        fn set_children(&self, parent: String, items: Vec<BrowseItem>) {
            self.0.set_children(parent, items)
        }
        fn set_listener(&self, listener: Option<Arc<dyn super::MediaListener>>) {
            self.0.set_listener(listener.map(|l| Arc::new(Tell(l)) as Arc<dyn MediaListener>))
        }
    }

    /// The phone's player as the port an update asks through.
    pub fn port(m: Arc<dyn NativeMediaPlayer>) -> Arc<dyn super::MediaPlayer> {
        Arc::new(Phone(m))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use ark_module::Modules;

    use super::*;

    struct Log(Mutex<Vec<MediaEvent>>);
    impl MediaListener for Log {
        fn heard(&self, event: MediaEvent) {
            self.0.lock().unwrap().push(event)
        }
    }

    #[test]
    fn calls_reach_the_player_and_events_its_listener() {
        let fake = Fake::new();
        let mut m = Modules::new();
        fake.register(&mut m);
        m.run(&load("file:///a.mp3", 5), |a| assert_eq!(a, Ok(())));
        m.run(&play(), |_| {});
        m.run(&set_rate(1.5), |_| {});
        m.run(&stop(), |_| {});
        assert_eq!(fake.asked(), vec!["load file:///a.mp3 5", "play", "rate 1.5", "stop"]);
        let log = Arc::new(Log(Mutex::new(vec![])));
        fake.set_listener(Some(log.clone()));
        fake.hear(MediaEvent::Ended);
        fake.hear(MediaEvent::Remote {
            command: RemoteCommand::SkipForward { seconds: 30 },
        });
        assert_eq!(
            *log.0.lock().unwrap(),
            vec![
                MediaEvent::Ended,
                MediaEvent::Remote {
                    command: RemoteCommand::SkipForward { seconds: 30 }
                }
            ]
        );
    }

    #[test]
    fn what_the_system_shows_is_kept() {
        let fake = Fake::new();
        let mut m = Modules::new();
        fake.register(&mut m);
        let info = NowPlaying {
            title: "So What".into(),
            artist: "Miles Davis".into(),
            duration_ms: 545_000,
            rate: 1.0,
            ..NowPlaying::default()
        };
        m.run(&set_now_playing(Some(info.clone())), |_| {});
        let c = MediaControls {
            has_next: true,
            can_seek: true,
            ..MediaControls::default()
        };
        m.run(&set_controls(c), |_| {});
        assert_eq!(fake.now_playing(), Some(info));
        assert_eq!(fake.controls(), c);
        m.run(&set_now_playing(None), |_| {});
        assert_eq!(fake.now_playing(), None);
    }

    #[test]
    fn a_browse_is_answered_with_children() {
        let fake = Fake::new();
        let mut m = Modules::new();
        fake.register(&mut m);
        // The app hears a car open the root, and answers what it holds.
        let asked = Arc::new(Mutex::new(vec![]));
        let a = asked.clone();
        struct Opened(Arc<Mutex<Vec<String>>>);
        impl MediaListener for Opened {
            fn heard(&self, e: MediaEvent) {
                if let MediaEvent::Browse { parent } = e {
                    self.0.lock().unwrap().push(parent)
                }
            }
        }
        fake.set_listener(Some(Arc::new(Opened(a))));
        assert_eq!(fake.browse(ROOT), None);
        assert_eq!(*asked.lock().unwrap(), vec![ROOT.to_string()]);
        let albums = BrowseItem {
            id: "albums".into(),
            title: "Albums".into(),
            browsable: true,
            ..BrowseItem::default()
        };
        m.run(&set_children(ROOT, vec![albums.clone()]), |_| {});
        assert_eq!(fake.browse(ROOT), Some(vec![albums]));
    }

    #[test]
    fn a_missing_player_is_unavailable() {
        let m = Modules::new();
        m.run(&play(), |a| assert_eq!(a, Err(ark_module::ModuleError::Unavailable)));
    }
}
