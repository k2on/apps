//! The desktop's devices of an app's modules (module.toml's `[desktop]`),
//! each a feature of arkui's that an app's generated entry point turns on
//! when its crate depends on the module (ui/prebuild.py's `desktop`), as
//! ark-ffi's features are the phones' (ui/docs/app.md, "The Device"):
//!
//! - `clipboard`: ark-clipboard's `Queued`, whose texts the window writes
//!   after the message with iced's own task.
//! - `scanner`: ark-scanner's `Chosen`: a scan or a photo asked for opens
//!   the desktop's file chooser (`crate::files::choose`), or takes a photo
//!   dropped on the window while it waits.
//! - `media-player`: ark-media-player's `Desktop`, the sound card (rodio
//!   over symphonia) and MPRIS on the session bus, as the app's name, so
//!   the media keys and the desktop's now-playing widgets reach it.
//! - The secure store, always: the Secret Service on Linux, where the
//!   window keeps its logins (`live`) and an app keeps what it asks to.

use std::sync::Arc;

use ark_app::Modules;
use iced::Task;

use super::{Desktop, Msg};

/// The secure store this machine has, as the port an app (and the
/// logins) ask through: the Secret Service on Linux; none elsewhere.
pub fn secure_store(app: &str) -> Modules {
    #[allow(unused_mut)]
    let mut m = Modules::new();
    #[cfg(target_os = "linux")]
    m.register::<dyn ark_secure_store::SecureStore>(Arc::new(ark_secure_store::SecretService::new(app)));
    let _ = app;
    m
}

/// The window's devices.
pub struct Devices {
    modules: Modules,
    /// What a device that answers later rings.
    bell: crate::wake::Bell,
    #[cfg(feature = "clipboard")]
    clipboard: ark_clipboard::Queued,
    #[cfg(feature = "scanner")]
    scanner: ark_scanner::Chosen,
    /// The chooser is open for a scan.
    #[cfg_attr(not(feature = "scanner"), allow(dead_code))]
    choosing: bool,
}

impl Devices {
    pub fn new<W: Desktop>() -> Devices {
        #[allow(unused_mut)]
        let mut modules = secure_store(W::SCHEME);
        #[cfg(feature = "clipboard")]
        let clipboard = ark_clipboard::Queued::new();
        #[cfg(feature = "clipboard")]
        modules.register::<dyn ark_clipboard::Clipboard>(Arc::new(clipboard.clone()));
        #[cfg(feature = "scanner")]
        let scanner = ark_scanner::Chosen::new();
        #[cfg(feature = "scanner")]
        modules.register::<dyn ark_scanner::Scanner>(Arc::new(scanner.clone()));
        #[cfg(feature = "media-player")]
        modules.register::<dyn ark_media_player::MediaPlayer>(Arc::new(ark_media_player::Desktop::new(W::SCHEME, W::NAME, W::SCHEME)));
        Devices {
            modules,
            bell: crate::wake::Bell::new(),
            #[cfg(feature = "clipboard")]
            clipboard,
            #[cfg(feature = "scanner")]
            scanner,
            choosing: false,
        }
    }

    pub fn modules(&self) -> Modules {
        self.modules.clone()
    }

    /// What a device that answers later rings: the window drains its screens.
    pub fn waker(&self) -> ark_app::host::Waker {
        let bell = self.bell.clone();
        Arc::new(move || bell.ring())
    }

    pub fn bell(&self) -> &crate::wake::Bell {
        &self.bell
    }

    /// What the devices were asked during a message, done with the
    /// window's own tasks: the texts written to the clipboard, the chooser
    /// opened for a scan that waits.
    pub fn after<M: Send + 'static>(&mut self) -> Task<Msg<M>> {
        #[allow(unused_mut)]
        let mut tasks: Vec<Task<Msg<M>>> = vec![];
        #[cfg(feature = "clipboard")]
        tasks.extend(self.clipboard.take().into_iter().map(iced::clipboard::write));
        #[cfg(feature = "scanner")]
        if self.scanner.waiting() && !self.choosing {
            self.choosing = true;
            tasks.push(crate::files::choose("A Photo", &["image/*"]).map(Msg::Chosen));
        }
        Task::batch(tasks)
    }

    /// The chooser closed: the scan waiting answered with its file, or none.
    pub fn chosen(&mut self, bytes: Option<Vec<u8>>) {
        self.choosing = false;
        #[cfg(feature = "scanner")]
        self.scanner.chosen(bytes);
        #[cfg(not(feature = "scanner"))]
        let _ = bytes;
        self.bell.ring();
    }

    /// A file dropped on the window: what the scan waiting takes, as if
    /// chosen; nothing while none waits.
    pub fn dropped(&mut self, bytes: Vec<u8>) {
        #[cfg(feature = "scanner")]
        if self.scanner.waiting() {
            self.scanner.chosen(Some(bytes));
            self.bell.ring();
        }
        #[cfg(not(feature = "scanner"))]
        let _ = bytes;
    }
}
