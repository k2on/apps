//! Context windows as layers of one `stack!` over the page.
//!
//! A menu or a picker is about something on the page, so it goes *over* the
//! page rather than in place of it: something that replaces the rows hides the
//! one it is about. Each layer is a backdrop and the panel on it.
//!
//! **A backdrop closes what is on it, and swallows the wheel.** A click away is
//! how people dismiss a menu, so a menu that only answers to the key that
//! opened it is one they click around. And the wheel is not a nicety: a panel
//! is pinned at a window coordinate, so a list that went on scrolling under it
//! would leave it hanging beside a row it is not about. AppKit answers that by
//! having an open menu take the event stream outright; the way to say it in
//! iced is to *capture* the event, which `mouse_area`'s `on_scroll` does — so
//! a backdrop sends `swallow` and the app does nothing with it. What is above a
//! backdrop still scrolls, which is why a long picker's own list still does.
//!
//! **A submenu has no backdrop of its own.** The menu's is already under both,
//! so the click-away and the wheel are answered — and a second full-window
//! layer *over* the menu would eat every click on the menu's own entries.
//!
//! **A backdrop cannot swallow a hover**, which is a separate rule: a hover is
//! published by the row itself and falls through every layer. So an app
//! refuses its rows' hover while [`crate::context::Context::focus`] is `Some`,
//! or the cursor the menu is about creeps away under it.
use iced::widget::{container, mouse_area, pin, text};
use iced::{Background, Element, Length, Point};

use crate::theme::of;

/// A clear backdrop the size of the window: a click or a right click closes,
/// the wheel is swallowed.
pub fn backdrop<'a, M: Clone + 'a>(close: M, swallow: M) -> Element<'a, M> {
    mouse_area(container(text("")).width(Length::Fill).height(Length::Fill))
        .on_press(close.clone())
        .on_right_press(close)
        .on_scroll(move |_| swallow.clone())
        .into()
}

/// The same, dimming the page: for a panel that stands alone in the middle of
/// the window, with no parent and no pointer to sit under.
pub fn dimmed<'a, M: Clone + 'a>(close: M, swallow: M) -> Element<'a, M> {
    mouse_area(
        container(text(""))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|theme| container::Style {
                background: Some(Background::Color(of(theme).background.base.color.scale_alpha(0.72))),
                ..container::Style::default()
            }),
    )
    .on_press(close.clone())
    .on_right_press(close)
    .on_scroll(move |_| swallow.clone())
    .into()
}

/// A panel pinned at a window coordinate. `pin` clips rather than scrolls, so
/// the origin has to have been fitted to the window first — which is what every
/// placement in [`crate::menu`] and [`crate::panel`] does.
pub fn pinned<'a, M: 'a>(content: impl Into<Element<'a, M>>, at: Point) -> Element<'a, M> {
    pin(content).x(at.x).y(at.y).into()
}

/// A panel in the middle of the window.
pub fn centred<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    container(content).center_x(Length::Fill).center_y(Length::Fill).into()
}
