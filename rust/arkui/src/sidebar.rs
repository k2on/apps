//! The sidebar: a column of places down the left of a window, grouped under
//! headings, one of them the page beside it.
//!
//! Lifted from harken's, where the rules were paid for. **One highlight, not
//! two**: the sidebar's cursor *is* what the page is showing, so the row
//! under it is marked whether or not the sidebar has the keyboard, and lit
//! (the accent, [`style::entry`]) only while it has. A heading is drawn when
//! the group changes rather than stored as a row, so the cursor's row `n`
//! and the reader's are the same and the keyboard walks a plain column
//! ([`crate::vim::Grid::column`]).
//!
//! What hangs off a row's right is the app's: harken's tally ([`count`]),
//! money's balance. It is drawn on the row's ground, so it asks
//! [`style::entry_text`] with the same `lit` the row was drawn with
//! ([`lit`]).
use iced::widget::{column, container, mouse_area, row, scrollable, text};
use iced::{Alignment, Element, Length, Padding};

use crate::{icon, style, theme};

/// What a window that has a sidebar usually makes it, edge included.
pub const WIDTH: f32 = 200.0;

/// One place in the sidebar.
pub struct Item<'a, M> {
    pub glyph: &'static [u8],
    pub label: String,
    /// The group it is under. A heading is drawn above the first item of a
    /// run with the same one; `None` draws none.
    pub heading: Option<&'static str>,
    /// Whatever is drawn on the right, where there is something to draw.
    pub trailing: Option<Element<'a, M>>,
    /// What choosing it does.
    pub press: M,
}

/// Whether row `i` is lit: under the cursor while the sidebar has the
/// keyboard. What an app draws on a row's right asks this, as the row does.
pub fn lit(i: usize, cursor: usize, focused: bool) -> bool {
    i == cursor && focused
}

/// A tally on a row's right: how many are in the place.
pub fn count<'a, M: 'a>(n: i64, lit: bool) -> Element<'a, M> {
    text(n.to_string())
        .size(10)
        .style(move |t| {
            let p = theme::of(t);
            text::Style {
                color: Some(match lit {
                    true => p.primary.base.text.scale_alpha(0.7),
                    false => p.background.base.text.scale_alpha(0.5),
                }),
            }
        })
        .into()
}

/// The sidebar `width` wide, edge included, with the cursor on `cursor`.
///
/// `focused` is whether it has the keyboard. `padding` is above the first
/// item and below the last — a window's chrome padding, so the first place
/// lines up with whatever starts the page. `id` is its scrollable's, for
/// [`crate::scroll::reveal`].
pub fn view<'a, M: Clone + 'a>(items: Vec<Item<'a, M>>, cursor: usize, focused: bool, width: f32, padding: f32, id: &'static str) -> Element<'a, M> {
    let mut side = column![].spacing(1).padding(Padding {
        top: padding,
        right: 10.0,
        bottom: padding,
        left: 10.0,
    });
    let mut under: Option<&str> = None;
    for (i, item) in items.into_iter().enumerate() {
        if item.heading.is_some() && item.heading != under {
            side = side.push(container(text(item.heading.unwrap_or_default()).size(10).style(style::faint(0.5))).padding([8, 10]));
        }
        under = item.heading;
        let on_cursor = i == cursor;
        let lit = lit(i, cursor, focused);
        let mut line = row![
            icon::line(item.glyph, lit),
            text(item.label)
                .size(13)
                .width(Length::Fill)
                .wrapping(text::Wrapping::None)
                .style(move |theme| text::Style {
                    color: Some(style::entry_text(theme, lit)),
                }),
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        if let Some(trailing) = item.trailing {
            line = line.push(trailing);
        }
        side = side.push(
            mouse_area(
                container(line)
                    .width(Length::Fill)
                    .padding([5, 10])
                    .style(move |theme| style::entry(theme, on_cursor, focused)),
            )
            .on_press(item.press),
        );
    }
    container(scrollable(side).id(id).style(style::bars))
        // Its edge, a pixel, is part of its width (drawn by the window beside
        // it, `style::edge`), so the page is as wide as the window less this.
        .width(Length::Fixed(width - 1.0))
        .height(Length::Fill)
        .style(style::surface)
        .into()
}
