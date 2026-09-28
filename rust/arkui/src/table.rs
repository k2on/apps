//! A table: rows of single-line cells under headings, in a zebra.
//!
//! Four things about it are load-bearing, and each is a function here:
//!
//! - **A row's background belongs to a container spanning the full width**
//!   ([`row`]), not to a button around the first cell. A stripe that stops
//!   where the text does is not a row. The click comes from a `mouse_area`
//!   around that container, so the whole line is the target.
//! - **Nothing is a literal colour.** Every cell asks [`crate::style::on_row`].
//! - **A row under the cursor is filled with the accent, so text on it has
//!   exactly one legible colour** — the one the accent was paired with. A
//!   dimmed column there is the same hue at lower alpha.
//! - **An empty column is as tall as what it stands in for** ([`gutter`]): an
//!   empty container has no height, so the rows without the mark came out
//!   shorter than the one with it.
//!
//! Widths are `FillPortion`s so the text columns share whatever is left after
//! the fixed ones, and a cell shortens its text to its portion
//! ([`crate::fit::to_width`]) because `Wrapping::None` shortens nothing.
use iced::widget::{container, text};
use iced::{Element, Length, Padding};

use crate::{fit, icon, style};

/// The column a row's one mark sits in — what is playing, say — and the gutter
/// every other row leaves empty. Fixed, so the text lines up whatever is or is
/// not marked.
pub const GUTTER: f32 = 31.0;
/// A row's padding, top and bottom then either side.
pub const ROW_PADDING: [f32; 2] = [3.0, 4.0];

/// One cell: a single line, shortened to its width rather than wrapped.
///
/// `on_cursor` is the accent-filled row; `accent` draws the text in the accent
/// (the one playing, a ticked name); `dim` is a secondary column.
pub fn cell<'a, M: 'a>(body: String, width: Length, on_cursor: bool, accent: bool, dim: bool) -> Element<'a, M> {
    text(fit::to_width(&body, width))
        .size(13)
        .width(width)
        .wrapping(text::Wrapping::None)
        .style(move |theme| text::Style {
            color: Some(style::on_row(theme, on_cursor, accent, dim)),
        })
        .into()
}

/// A column heading, in the same grid as the cells under it.
pub fn heading<'a, M: 'a>(label: impl text::IntoFragment<'a>, width: Length) -> Element<'a, M> {
    text(label).size(11).width(width).style(style::faint(0.55)).into()
}

/// A section heading inside the table: the part of the whole below it.
///
/// Drawn in the accent rather than filled with it, the way the playing row is,
/// because a filled stripe is what the cursor means and there can only be one
/// of those. It takes the whole width, indented by `indent` to where the
/// column it heads starts, so a work with four suites reads as four blocks
/// rather than one list with a repeated column.
pub fn section<'a, M: 'a>(label: String, indent: f32) -> Element<'a, M> {
    container(text(label).size(12).wrapping(text::Wrapping::None).style(style::accent))
        .width(Length::Fill)
        .padding(Padding {
            top: 8.0,
            right: 4.0,
            bottom: 2.0,
            left: indent,
        })
        .into()
}

/// The empty [`GUTTER`], as tall as the glyph that would be in it.
pub fn gutter<'a, M: 'a>() -> Element<'a, M> {
    container(text("")).width(Length::Fixed(GUTTER)).height(Length::Fixed(icon::SIZE)).into()
}

/// A whole row: `line` in a full-width container painted by
/// [`crate::style::row`]. Wrap it in a `mouse_area` for the click.
pub fn row<'a, M: 'a>(line: impl Into<Element<'a, M>>, on_cursor: bool, focused: bool, odd: bool) -> container::Container<'a, M> {
    container(line)
        .width(Length::Fill)
        .padding(Padding::from(ROW_PADDING))
        .style(move |theme| style::row(theme, on_cursor, focused, odd))
}
