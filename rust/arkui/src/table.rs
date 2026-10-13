//! A table: rows of single-line cells under headings, in a zebra.
//!
//! Five things about it are load-bearing, and each is a function here:
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
//! - **A column ends in a gap, and its text ends in an ellipsis before the
//!   gap** ([`cell`]). Widths are `FillPortion`s, so the text columns share
//!   whatever is left after the fixed ones — which is only known at layout,
//!   and beside a docked panel or in a small window is far less than the
//!   window's width would suggest. So a cell is cut where it is laid out, to
//!   the width it was actually given less [`SPACING`], and clipped to its
//!   column besides.
use iced::widget::{container, text};
use iced::{Element, Length, Padding};

use crate::line::Line;
use crate::{fit, icon, style};

/// The column a row's one mark sits in — what is playing, say — and the gutter
/// every other row leaves empty. Fixed, so the text lines up whatever is or is
/// not marked.
pub const GUTTER: f32 = 31.0;
/// A row's padding, top and bottom then either side.
pub const ROW_PADDING: [f32; 2] = [3.0, 4.0];
/// The gap at the end of every column the table sizes, so the text of one
/// column never meets the next: a cell's text is cut to its width less this.
pub const SPACING: f32 = 12.0;
/// The size a cell's text is drawn at.
pub const CELL_SIZE: f32 = 13.0;
/// …and a heading's.
pub const HEADING_SIZE: f32 = 11.0;

/// One cell: a single line, ending in an ellipsis where it does not fit, then
/// [`SPACING`], all clipped to `width`.
///
/// The cut is made at layout ([`fit::within`]), against the text as it is
/// shaped and the width the column was given — not an estimate made in
/// characters before layout, which is right at one window width and wrong at
/// every other: beside a docked panel the columns are a few hundred pixels
/// narrower, and a cell cut for the wide table drew `Johann Seba` straight
/// into `Goldberg Variatio`. A value that fits is drawn whole, as it was;
/// one that does not loses its middle ([`fit::Cut::Middle`]), because a
/// cell's value is usually a title whose ends are what identify it — a
/// movement keeps its catalogue number. A column that knows it holds
/// something else says so with [`cell_cut`].
///
/// `on_cursor` is the accent-filled row; `accent` draws the text in the accent
/// (the one playing, a ticked name); `dim` is a secondary column.
pub fn cell<'a, M: 'a>(body: String, width: Length, on_cursor: bool, accent: bool, dim: bool) -> Element<'a, M> {
    cell_cut(body, width, fit::Cut::Middle, on_cursor, accent, dim)
}

/// A [`cell`] that loses the part `cut` says. Which part is the caller's
/// to decide, because only the caller knows what the column holds: a
/// person's name reads from its start (`Johann S…`), where `Joha…ach` is a
/// puzzle, and this table cannot tell a name from a title.
pub fn cell_cut<'a, M: 'a>(body: String, width: Length, cut: fit::Cut, on_cursor: bool, accent: bool, dim: bool) -> Element<'a, M> {
    in_column(
        Line::new(body, CELL_SIZE, cut, move |theme| style::on_row(theme, on_cursor, accent, dim)),
        width,
    )
}

/// A column heading, in the same grid as the cells under it, spaced the way
/// they are. Cut at its end ([`fit::Cut::End`]), not its middle: a heading
/// is a word, and `Perfo…` reads where `Per…er` does not.
pub fn heading<'a, M: 'a>(label: impl Into<String>, width: Length) -> Element<'a, M> {
    let faint = style::faint(0.55);
    in_column(
        Line::new(label.into(), HEADING_SIZE, fit::Cut::End, move |theme| {
            faint(theme).color.unwrap_or_default()
        }),
        width,
    )
}

/// A line in a column of `width`, with the table's gap after it.
///
/// A `Shrink` column is as wide as what is in it, so whatever sized it owns
/// the gap after it — the explorer's grid, which spaces its own — and it gets
/// none here.
fn in_column<'a, M: 'a>(line: Line<'a>, width: Length) -> Element<'a, M> {
    let trailing = match width {
        Length::Shrink => 0.0,
        _ => SPACING,
    };
    container(line)
        .width(width)
        .padding(Padding {
            right: trailing,
            ..Padding::ZERO
        })
        .clip(true)
        .into()
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
