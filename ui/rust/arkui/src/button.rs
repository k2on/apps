//! The buttons both windows draw, one look for each kind: an icon, a link,
//! a word.
//!
//! None has a fill or a border: a page's controls read as part of its text.
//! What a pointer does to one is small and the same everywhere. A word or a
//! link is underlined and keeps its colour ([`underlined`]); an icon gets the
//! faintest tint behind it ([`crate::style::quiet`]). The icon button is
//! money's, a glyph from [`crate::glyphs`] in the text colour at
//! [`crate::icon::SIZE`] with a little room around it, so the same action is
//! the same shape in both apps.
use crate::text::{self, text};
use iced::widget::{button, container, rule, Button};
use iced::{Alignment, Element, Length, Theme};

use crate::{icon, style, theme};

/// How much room an icon button leaves around its glyph.
pub const ICON_PADDING: u16 = 4;

/// An icon button: `shape`, pressed for `press`.
pub fn icon<'a, M: Clone + 'a>(shape: &'static [u8], press: M) -> Element<'a, M> {
    icon_maybe(shape, Some(press))
}

/// An icon button that may have nothing to do: drawn dimmed, and not
/// pressable, when `press` is `None` (back with nowhere to go back to).
pub fn icon_maybe<'a, M: Clone + 'a>(shape: &'static [u8], press: Option<M>) -> Element<'a, M> {
    let dim = press.is_none();
    button(icon::plain(shape, dim))
        .on_press_maybe(press)
        .padding(ICON_PADDING)
        .style(style::quiet)
        .into()
}

/// How soon a second click on the same thing is a double click: AppKit's
/// default, for every window here.
pub const DOUBLE_CLICK: std::time::Duration = std::time::Duration::from_millis(500);

/// A button holding `body` as it is, with no ground and no border: a card,
/// a cover, a glyph drawn some other colour. A pointer over it changes
/// nothing; what it holds says it can be pressed.
pub fn plain<'a, M: Clone + 'a>(body: impl Into<Element<'a, M>>) -> Button<'a, M> {
    button(body).padding(0).style(style::bare)
}

/// `body` with a line under it while the pointer is over it, in `color`:
/// what a link or a name a click opens does when it is pointed at, in place
/// of changing its colour or lighting a ground behind it.
pub fn underlined<'a, M: 'a>(body: impl Into<Element<'a, M>>, color: impl Fn(&Theme) -> iced::Color + 'a) -> Element<'a, M> {
    let line = container(rule::horizontal(1).style(move |t| rule::Style {
        color: color(t),
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }))
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(iced::Padding::ZERO.bottom(1))
    .align_y(Alignment::End);
    iced::widget::hover(body, line)
}

/// Words in the accent that do something: underlined when pointed at.
pub fn link<'a, M: Clone + 'a>(label: impl Into<String>, size: f32, press: M) -> Element<'a, M> {
    let accent = |t: &Theme| theme::of(t).primary.base.color;
    let words = text(label.into()).size(size).style(move |t| text::Style {
        color: Some(accent(t)),
        ..Default::default()
    });
    plain(underlined(words, accent)).on_press(press).into()
}

/// Words in the page's text colour that do something, underlined when
/// pointed at: a button that is a word.
pub fn word<'a, M: Clone + 'a>(label: impl Into<String>, size: f32, press: M) -> Element<'a, M> {
    let ink = |t: &Theme| theme::of(t).background.base.text;
    let words = text(label.into()).size(size).style(move |t| text::Style {
        color: Some(ink(t)),
        ..Default::default()
    });
    plain(underlined(words, ink)).on_press(press).into()
}
