//! What wakes this window, and nothing else does (`docs/plan-reactive.md`).
//!
//! It used to run on `iced::time::every(50ms)`: every tick pumped the peer,
//! pumped the listening session, polled the address bar, announced to the
//! media session, counted the dwell and the quiet, and faded the explorer —
//! twenty updates a second whether anything had happened or not. Now each
//! source says so:
//!
//! - **the sync socket** rings the window's [`Bell`] when it opens, closes
//!   or brings a frame — the log, an ack, the listening session's frames —
//!   through [`crate::client::Replica::set_waker`] ([`waker`]);
//! - **a deadline** the replica or the window asked for — the redial after a
//!   close, the second of quiet before a default playlist — is one timer,
//!   `arkui::wake::at`, keyed by the deadline;
//! - **in a browser, the page**: the back button and a followed fragment
//!   (`popstate`, `hashchange`), a lock-screen button, and the `<audio>`
//!   element starting, pausing or ending ring the bell too ([`page`]);
//! - **a clock** only while something moves by itself — audio playing, a
//!   highlight fading, a submenu's dwell — which is [`crate::App::clock`];
//! - and what a person does, which iced delivers as messages anyway.
//!
//! With nothing playing, nothing fading and nothing arriving, the window
//! runs no timer and `update` is not called.

use arkui::wake::Bell;

/// The peer's wake: ring the bell, from the socket's reader thread natively
/// and from the `WebSocket`'s callbacks in a browser.
pub fn waker(bell: &Bell) -> crate::client::Wake {
    let bell = bell.clone();
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::sync::Arc::new(move || bell.ring())
    }
    #[cfg(target_arch = "wasm32")]
    {
        std::rc::Rc::new(move || bell.ring())
    }
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    /// The bell of the window the page shows. The page's listeners are
    /// installed once for its life, and a window can be replaced under them
    /// (`Message::Started`), so they ring whichever bell is current.
    static PAGE: std::cell::RefCell<Option<Bell>> = const { std::cell::RefCell::new(None) };
    static WIRED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Make `bell` the one the page's own events ring, wiring them the first
/// time: the address bar moving, and the media session's and the
/// `<audio>` element's events (`player.rs`). Nothing on the desktop.
pub fn page(bell: &Bell) {
    #[cfg(target_arch = "wasm32")]
    {
        PAGE.with(|p| *p.borrow_mut() = Some(bell.clone()));
        if !WIRED.with(|w| w.replace(true)) {
            arkui::wake::page_changes(ring_page);
            crate::player::ring_with(ring_page);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = bell;
}

/// Ring the current window's bell.
#[cfg(target_arch = "wasm32")]
pub fn ring_page() {
    PAGE.with(|p| {
        if let Some(b) = &*p.borrow() {
            b.ring();
        }
    });
}
