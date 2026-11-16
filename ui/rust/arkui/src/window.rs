//! A window's root: what every arkui app wraps its whole view in, once,
//! and the one subscription that goes with it — so that what a window
//! needs of the platform is arkui's to know, not each app's.
//!
//! - **The renderer, known for certain** (the root module, private to
//!   arkui): the root is drawn every frame by whichever renderer iced
//!   settled on ([`crate::renderer`]).
//! - **The pointer, tracked over every layer** ([`tracked`]): a `stack`
//!   hides the pointer from the layers under the one it is over, so a
//!   `mouse_area` on the page stops hearing it the moment a sheet, a
//!   dialog or a menu is up, and a press read from it is taken from
//!   wherever the pointer last was over the page. Tracked around the root,
//!   it is heard wherever it goes, in the window's own coordinates — the
//!   origin a [`crate::layer::pinned`] layer shares.
//!
//! - **Fingers resting on a trackpad stop a scroll** ([`boot`]): on
//!   Wayland the hold gesture is listened to on the window's own display
//!   ([`crate::touchpad`]), and [`wakes`] rings when the fingers lift so a
//!   band they held springs back.
//!
//! An app's `view` ends in [`view`] (or [`tracked`]), its boot's task has
//! [`boot`] beside it, and its `subscription` has [`wakes`], answered with
//! any message: the update draws a frame. Where there is nothing to do
//! they cost nothing.
use iced::widget::mouse_area;
use iced::{Element, Point, Subscription, Task};

/// The window's root: `content`, drawn as the window's whole view.
pub fn view<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    crate::root::view(content)
}

/// [`view`], and the pointer heard wherever it moves in the window, over
/// every layer: `moved` with where it is, in the window's coordinates.
pub fn tracked<'a, M: Clone + 'a>(content: impl Into<Element<'a, M>>, moved: impl Fn(Point) -> M + 'a) -> Element<'a, M> {
    view(mouse_area(content).on_move(moved))
}

/// Rings when something under the root needs a frame of its own — fingers
/// rested on the trackpad or lifted — for the app to answer with any
/// message.
pub fn wakes() -> Subscription<()> {
    crate::touchpad::subscription()
}

/// What a window starts with, beside an app's own boot task: on Wayland,
/// listening for fingers resting on the trackpad on its display
/// ([`crate::touchpad::listen`]). The window is open by the time an
/// application's boot task runs.
pub fn boot<M: Send + 'static>() -> Task<M> {
    crate::touchpad::listen()
}
