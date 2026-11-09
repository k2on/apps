//! The clipboard: text put where the person can paste it. An update asks
//! with [`copy`]; the module answers `Done`.
//!
//! - iPhone and Mac: `PhoneClipboard` (swift/: UIPasteboard, NSPasteboard).
//! - Android: `PhoneClipboard` (kotlin/: ClipboardManager).
//! - An iced window, desktop or wasm (feature `desktop`): [`Queued`], whose
//!   texts the window writes with iced's clipboard task after the message.
//! - The browser's DOM (feature `web`, wasm32): [`Browser`], the page's
//!   `navigator.clipboard`.

use std::sync::Arc;

use ark_module::{Call, ModuleAnswer, ModuleCall, ModuleReply};

#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!();

pub mod fake;

/// Text put on the clipboard.
#[cfg_attr(feature = "ffi", uniffi::export(with_foreign))]
pub trait Clipboard: Send + Sync {
    fn copy_text(&self, text: String);
}

/// What an update asks of the clipboard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipboardCall {
    Copy(String),
}

impl Call for ClipboardCall {
    type Module = dyn Clipboard;
    const MODULE: &'static str = "clipboard";
    fn run(&self, m: &dyn Clipboard, reply: Arc<ModuleReply>) {
        match self {
            ClipboardCall::Copy(t) => m.copy_text(t.clone()),
        }
        reply.answer(ModuleAnswer::Done)
    }
}

/// `text` put on the clipboard.
pub fn copy(text: impl Into<String>) -> ModuleCall {
    ClipboardCall::Copy(text.into()).into()
}

#[cfg(feature = "desktop")]
pub use desktop::Queued;

#[cfg(feature = "desktop")]
mod desktop {
    use std::sync::{Arc, Mutex};

    /// The texts an iced window has to write: a call can't run iced's
    /// clipboard task itself, so the host takes them after the message and
    /// writes each (`iced::clipboard::write`).
    #[derive(Clone, Default)]
    pub struct Queued {
        texts: Arc<Mutex<Vec<String>>>,
    }

    impl Queued {
        pub fn new() -> Queued {
            Queued::default()
        }

        /// The texts asked for since, oldest first, taken.
        pub fn take(&self) -> Vec<String> {
            std::mem::take(&mut *self.texts.lock().unwrap_or_else(|e| e.into_inner()))
        }
    }

    impl super::Clipboard for Queued {
        fn copy_text(&self, text: String) {
            self.texts.lock().unwrap_or_else(|e| e.into_inner()).push(text)
        }
    }
}

#[cfg(all(feature = "web", target_arch = "wasm32"))]
pub use web::Browser;

#[cfg(all(feature = "web", target_arch = "wasm32"))]
mod web {
    /// The page's clipboard (`navigator.clipboard.writeText`); it answers
    /// at once, as the browser writes in the background.
    #[derive(Clone, Copy, Default)]
    pub struct Browser;

    impl super::Clipboard for Browser {
        fn copy_text(&self, text: String) {
            if let Some(w) = web_sys::window() {
                let _ = w.navigator().clipboard().write_text(&text);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_module::Modules;

    #[test]
    fn a_copy_reaches_the_registered_module() {
        let fake = fake::Fake::new();
        let mut m = Modules::new();
        fake.register(&mut m);
        m.call(copy("$12.00"), |a| assert_eq!(a, ModuleAnswer::Done));
        assert_eq!(fake.copied(), vec!["$12.00".to_string()]);
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn the_desktop_queues_for_the_window() {
        let q = Queued::new();
        let mut m = Modules::new();
        m.register::<dyn Clipboard>(Arc::new(q.clone()));
        m.call(copy("a"), |_| {});
        assert_eq!(q.take(), vec!["a".to_string()]);
        assert!(q.take().is_empty());
    }
}
