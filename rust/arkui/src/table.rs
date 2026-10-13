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
use iced::advanced::layout::{self, Layout};
use iced::advanced::text::{self as core_text, Paragraph};
use iced::advanced::widget::{self, tree, Tree, Widget};
use iced::advanced::{mouse, renderer};
use iced::widget::{container, text};
use iced::{alignment, Element, Length, Padding, Pixels, Rectangle, Size, Theme};

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
/// one that does not loses its end ([`fit::tail`]), so it ends in the
/// ellipsis that says so.
///
/// `on_cursor` is the accent-filled row; `accent` draws the text in the accent
/// (the one playing, a ticked name); `dim` is a secondary column.
pub fn cell<'a, M: 'a>(body: String, width: Length, on_cursor: bool, accent: bool, dim: bool) -> Element<'a, M> {
    in_column(
        Line::new(body, CELL_SIZE, move |theme| style::on_row(theme, on_cursor, accent, dim)),
        width,
    )
}

/// A column heading, in the same grid as the cells under it, cut and spaced
/// the way they are.
pub fn heading<'a, M: 'a>(label: impl Into<String>, width: Length) -> Element<'a, M> {
    let faint = style::faint(0.55);
    in_column(
        Line::new(label.into(), HEADING_SIZE, move |theme| faint(theme).color.unwrap_or_default()),
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

/// One line of text cut to whatever width layout gives it.
///
/// iced's `text` cannot do this: it is told its content before layout, and
/// the width is only known during it. So this holds the whole value, and cuts
/// it in `layout`, where the limits are the column's — remembering the cut
/// against the value and the width, so a table redrawn twenty times a second
/// shapes a cell again only when one of those moves.
struct Line<'a> {
    body: String,
    size: f32,
    color: Box<dyn Fn(&Theme) -> iced::Color + 'a>,
}

impl<'a> Line<'a> {
    fn new(body: String, size: f32, color: impl Fn(&Theme) -> iced::Color + 'a) -> Self {
        Line {
            body,
            size,
            color: Box::new(color),
        }
    }
}

/// What a [`Line`] keeps between layouts: what it was last cut for, the cut,
/// and the cut shaped.
#[derive(Default)]
struct Cut<P: Paragraph> {
    for_: Option<(String, f32, f32)>,
    shown: String,
    paragraph: core_text::paragraph::Plain<P>,
}

impl<M, R> Widget<M, Theme, R> for Line<'_>
where
    R: core_text::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Cut<R::Paragraph>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Cut::<R::Paragraph>::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &R, limits: &layout::Limits) -> layout::Node {
        let cut = tree.state.downcast_mut::<Cut<R::Paragraph>>();
        let max = limits.max();
        let (size, font) = (self.size, renderer.default_font());

        let key = (self.body.clone(), max.width, self.size);
        if cut.for_.as_ref() != Some(&key) {
            cut.shown = fit::within(&self.body, max.width, |s| {
                R::Paragraph::with_text(one_line(s, size, font, max.height)).min_bounds().width
            });
            cut.for_ = Some(key);
        }
        let _ = cut.paragraph.update(one_line(&cut.shown, size, font, max.height));
        layout::Node::new(limits.resolve(Length::Shrink, Length::Shrink, cut.paragraph.min_bounds()))
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut R,
        theme: &Theme,
        defaults: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let cut = tree.state.downcast_ref::<Cut<R::Paragraph>>();
        widget::text::draw(
            renderer,
            defaults,
            layout.bounds(),
            cut.paragraph.raw(),
            widget::text::Style {
                color: Some((self.color)(theme)),
            },
            viewport,
        );
    }

    fn operate(&mut self, _tree: &mut Tree, layout: Layout<'_>, _renderer: &R, operation: &mut dyn widget::Operation) {
        operation.text(None, layout.bounds(), &self.body);
    }
}

/// `content` as one unwrapped line, for shaping.
fn one_line<F>(content: &str, size: f32, font: F, height: f32) -> core_text::Text<&str, F> {
    core_text::Text {
        content,
        bounds: Size::new(f32::INFINITY, height),
        size: Pixels(size),
        line_height: core_text::LineHeight::default(),
        font,
        align_x: core_text::Alignment::Default,
        align_y: alignment::Vertical::Top,
        shaping: core_text::Shaping::default(),
        wrapping: core_text::Wrapping::None,
    }
}

impl<'a, M: 'a, R> From<Line<'a>> for Element<'a, M, Theme, R>
where
    R: core_text::Renderer + 'a,
{
    fn from(line: Line<'a>) -> Self {
        Element::new(line)
    }
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
