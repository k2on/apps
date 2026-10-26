//! One line of text that fits whatever width it is laid out in: the value
//! whole when it fits, and otherwise cut, around one ellipsis, as a
//! [`fit::Cut`] says.
use iced::advanced::layout::{self, Layout};
use iced::advanced::text::{self as core_text, Paragraph};
use iced::advanced::widget::{self, tree, Tree, Widget};
use iced::advanced::{mouse, renderer};
use iced::{alignment, Element, Length, Pixels, Rectangle, Size, Theme};

use crate::fit;

/// One line of text cut to whatever width layout gives it ([`fit::within`]):
/// a table cell, or a page header's line beside a docked panel.
///
/// iced's `text` cannot do this: it is told its content before layout, and
/// the width is only known during it. So this holds the whole value, and cuts
/// it in `layout`, where the limits are the column's — remembering the cut
/// against the value and the width, so a table redrawn twenty times a second
/// shapes a cell again only when one of those moves.
pub struct Line<'a> {
    body: String,
    size: f32,
    cut: fit::Cut,
    color: Box<dyn Fn(&Theme) -> iced::Color + 'a>,
    gives_way: bool,
}

impl<'a> Line<'a> {
    /// `body` at `size`, losing the part `cut` says when it does not fit,
    /// drawn in `color`.
    pub fn new(body: impl Into<String>, size: f32, cut: fit::Cut, color: impl Fn(&Theme) -> iced::Color + 'a) -> Self {
        let body: String = body.into();
        Line {
            // What the typeface cannot draw, spelt in what it can.
            body: fit::drawable(&body).into_owned(),
            size,
            cut,
            color: Box::new(color),
            gives_way: false,
        }
    }

    /// Laid out after the rest of its row, so what follows it keeps its room
    /// and this is cut instead — a page's title before its count of tracks.
    /// It is still only as wide as its text, so what follows sits right
    /// after it, as it would after a `text`.
    ///
    /// A row lays its children out in order, each in what the ones before
    /// left, so a line first in a row would take the whole width and leave
    /// the count nothing. Saying it fills is what puts it last; the width it
    /// answers with is what places the rest.
    pub fn gives_way(mut self) -> Self {
        self.gives_way = true;
        self
    }
}

/// What a [`Line`] keeps between layouts: what it was last cut for, the cut,
/// and the cut shaped.
#[derive(Default)]
struct Shaped<P: Paragraph> {
    for_: Option<(String, f32, f32, fit::Cut)>,
    shown: String,
    paragraph: core_text::paragraph::Plain<P>,
}

impl<M, R> Widget<M, Theme, R> for Line<'_>
where
    R: core_text::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Shaped<R::Paragraph>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Shaped::<R::Paragraph>::default())
    }

    fn size(&self) -> Size<Length> {
        let width = match self.gives_way {
            true => Length::Fill,
            false => Length::Shrink,
        };
        Size::new(width, Length::Shrink)
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &R, limits: &layout::Limits) -> layout::Node {
        let cut = tree.state.downcast_mut::<Shaped<R::Paragraph>>();
        let max = limits.max();
        let (size, font) = (self.size, renderer.default_font());

        let key = (self.body.clone(), max.width, self.size, self.cut);
        if cut.for_.as_ref() != Some(&key) {
            cut.shown = fit::within(&self.body, max.width, self.cut, |s| {
                R::Paragraph::with_text(one_line(s, size, font, max.height)).min_bounds().width
            });
            cut.for_ = Some(key);
        }
        let _ = cut.paragraph.update(one_line(&cut.shown, size, font, max.height));
        // As wide as the text and no wider, whatever the limits' minimum: a
        // row gives what fills its whole share as the minimum, and a line
        // that [gives way](Line::gives_way) must still answer with its own
        // width, or what follows it is pushed to the far end.
        let text = cut.paragraph.min_bounds();
        layout::Node::new(Size::new(text.width.min(max.width), text.height.min(max.height)))
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
        let cut = tree.state.downcast_ref::<Shaped<R::Paragraph>>();
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
