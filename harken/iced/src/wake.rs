//! What wakes this window, and nothing else does (`docs/plan-reactive.md`).
//!
//! It used to run on `iced::time::every(50ms)`: every tick pumped the peer,
//! pumped the listening session, polled the address bar, announced to the
//! media session, and counted the dwell and the quiet —
//! twenty updates a second whether anything had happened or not. Now each
//! source says so:
//!
//! - **the sync socket** rings the window's [`Bell`] when it opens, closes
//!   or brings a frame — the log, an ack, the listening session's frames —
//!   through [`crate::client::Replica::set_waker`] ([`waker`]);
//! - **a deadline** the replica or the window asked for — the redial after a
//!   close — is one timer,
//!   `arkui::wake::at`, keyed by the deadline;
//! - **a clock** only while something moves by itself — audio playing and a
//!   submenu's dwell — which is [`crate::App::clock`];
//! - and what a person does, which iced delivers as messages anyway.
//!
//! With nothing playing, nothing dwelling and nothing arriving, the window
//! runs no timer and `update` is not called.

use arkui::wake::Bell;

/// The peer's wake: ring the bell, from the socket's reader thread.
pub fn waker(bell: &Bell) -> crate::client::Wake {
    let bell = bell.clone();
    std::sync::Arc::new(move || bell.ring())
}
