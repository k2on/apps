//! Back and forward, every way an app is asked for them: the browser's own
//! buttons, `^O` and `^I` ([`crate::vim::Action::Jump`]), and two fingers
//! sideways across a trackpad.
//!
//! The pieces are [`crate::route`]'s router (the address bar, or one kept in
//! memory on the desktop) and [`crate::swipe`]'s gesture; what this adds is
//! the wiring every app did the same way, held once:
//!
//! - **One way to move.** A jump and a swipe let go both [`History::go`]: the
//!   bar moves, and the app arrives where it went by its next
//!   [`History::poll`], the same road the browser's back button takes.
//! - **The arrow only where there is somewhere to go.** The wheel is fed with
//!   the router's answer to "is there a back, a forward" ([`History::wheel`]).
//! - **The wheel, sideways, whatever took it** ([`wheel`]): a list scrolls
//!   down and a swipe is sideways, so the two never fight over an event.
//!
//! An app keeps one, records where it is after every update
//! ([`History::sync`]), asks [`History::poll`] where the bar went, feeds it
//! [`wheel`], ticks it on a frame clock while [`History::active`], and draws
//! its page through [`History::view`] for the arrow.
use iced::mouse::ScrollDelta;
use iced::time::Instant;
use iced::{Element, Subscription};

use crate::route::{Route, Router};
use crate::swipe::{self, Swipe};

/// Where an app has been, and the gesture that goes back there.
pub struct History<R> {
    router: Router<R>,
    swipe: Swipe,
}

impl<R: Route> Default for History<R> {
    fn default() -> Self {
        History {
            router: Router::new(),
            swipe: Swipe::default(),
        }
    }
}

impl<R: Route> History<R> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Back (`-1`) or forward (`1`): `^O` and `^I`, a swipe, a menu's
    /// "Back". The app arrives there by its next [`History::poll`].
    pub fn go(&mut self, by: isize) {
        self.router.go(by);
    }

    /// Whether there is anywhere `by` places away.
    pub fn can_go(&self, by: isize) -> bool {
        self.router.can_go(by)
    }

    /// The wheel moved sideways ([`wheel`]): perhaps a swipe.
    pub fn wheel(&mut self, delta: ScrollDelta, now: Instant) {
        self.swipe.allow(self.router.can_go(-1), self.router.can_go(1));
        self.swipe.feed(delta, now);
    }

    /// A frame while [`History::active`]: a swipe let go far enough goes.
    pub fn tick(&mut self, now: Instant) {
        if let Some(by) = self.swipe.tick(now) {
            self.router.go(by);
        }
    }

    /// Whether a swipe is under way, and wants a frame clock.
    pub fn active(&self) -> bool {
        self.swipe.active()
    }

    /// Where the app now is. After every update; nothing else writes the bar.
    pub fn sync(&mut self, at: &R) {
        self.router.sync(at);
    }

    /// Make the next [`History::sync`] replace this place rather than add
    /// one: a cursor walking through what it shows.
    pub fn replace_next(&mut self) {
        self.router.replace_next();
    }

    /// Where the bar went, once there is something to resolve it against
    /// (`ready`): back, forward, or a link somebody was sent.
    pub fn poll(&mut self, ready: bool) -> Option<R> {
        self.router.poll(ready)
    }

    pub fn router(&self) -> &Router<R> {
        &self.router
    }

    /// The page, with a swipe's arrow over it while one is under way.
    pub fn view<'a, M: 'a>(&self, page: Element<'a, M>, now: Instant) -> Element<'a, M> {
        swipe::view(page, &self.swipe, now)
    }
}

/// The wheel when it moves mostly sideways, whichever widget took it — or
/// moves nowhere, which is the fingers lifting where the system says so
/// ([`crate::swipe`]).
pub fn wheel() -> Subscription<ScrollDelta> {
    iced::event::listen_with(|event, _, _| match event {
        iced::Event::Mouse(iced::mouse::Event::WheelScrolled { delta }) => {
            let (x, y) = match delta {
                ScrollDelta::Pixels { x, y } | ScrollDelta::Lines { x, y } => (x, y),
            };
            (x.abs() > y.abs() || (x == 0.0 && y == 0.0)).then_some(delta)
        }
        _ => None,
    })
}
