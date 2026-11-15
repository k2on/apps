//! Which platform the app runs on, for the rare call that must know
//! (Expo's `Platform.OS`): an app is written once and drawn by every
//! toolkit, so a screen never asks; what differs is the toolkit's, or a
//! device module's behind its port. Ask only where the platform itself is
//! the fact — a sentence that names it, a feature one platform lacks that
//! no module stands for — and say why beside the call.

/// A platform an Ark app runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Platform {
    /// The iPhone and the iPad (ArkApp, SwiftUI).
    Ios,
    /// An Android phone or tablet (ark-app-android, Compose).
    Android,
    /// The Mac (ArkApp, SwiftUI).
    Mac,
    /// The desktop's window (arkui, iced): Linux and Windows.
    Desktop,
    /// The browser (arkui-web), and a page written ahead or by a server.
    Web,
}

impl Platform {
    /// Its word, as app.toml's `platforms` spells it (`ios` for the
    /// iPhone's and the iPad's).
    pub fn word(self) -> &'static str {
        match self {
            Platform::Ios => "ios",
            Platform::Android => "android",
            Platform::Mac => "mac",
            Platform::Desktop => "desktop",
            Platform::Web => "web",
        }
    }
}

/// The platform this build of the app runs on: the target it was compiled
/// for. A page written natively (the web export's, a server's) is the
/// browser's too: it is compiled with the `ssr` side of arkui-web, which
/// says so ([`set_web`]).
pub fn platform() -> Platform {
    if cfg!(target_arch = "wasm32") || WEB.load(std::sync::atomic::Ordering::Relaxed) {
        Platform::Web
    } else if cfg!(target_os = "ios") {
        Platform::Ios
    } else if cfg!(target_os = "android") {
        Platform::Android
    } else if cfg!(target_os = "macos") {
        Platform::Mac
    } else {
        Platform::Desktop
    }
}

static WEB: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// This process draws the browser's pages natively (arkui-web's export and
/// a server's pages): [`platform`] is [`Platform::Web`] from now on.
#[doc(hidden)]
pub fn set_web() {
    WEB.store(true, std::sync::atomic::Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test runs natively, on the desktop's platform unless a web host
    /// said otherwise. Falsified by reading the platform from a feature.
    #[test]
    fn a_native_test_is_the_desktops() {
        let expected = match () {
            _ if cfg!(target_os = "macos") => Platform::Mac,
            _ => Platform::Desktop,
        };
        assert_eq!(platform(), expected);
        assert_eq!(Platform::Web.word(), "web");
    }
}
