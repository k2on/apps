//! One line of text that fits whatever width it is laid out in: the value
//! whole when it fits, and otherwise cut, around one ellipsis, as a
//! [`fit::Cut`] says; and a page's title ([`title`]), which is one of them
//! drawn bold and tracked to its size.
use iced::advanced::layout::{self, Layout};
use iced::advanced::text::{self as core_text, Paragraph};
use iced::advanced::widget::{self, tree, Tree, Widget};
use iced::advanced::{mouse, renderer};
use iced::{alignment, Element, Font, Length, Pixels, Point, Rectangle, Size, Theme};

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
    font: Option<Font>,
    tracking: f32,
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
            font: None,
            tracking: 0.0,
        }
    }

    /// Drawn in `font` rather than the window's default.
    pub fn font(mut self, font: Font) -> Self {
        self.font = Some(font);
        self
    }

    /// `em` of its size between each character and the next, as well as
    /// what the face puts there: below nothing, closer together. See
    /// [`tracking`] for how much a size wants.
    pub fn tracking(mut self, em: f32) -> Self {
        self.tracking = em;
        self
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
/// and the cut shaped — and, tracked, each of its characters shaped alone
/// with where it goes.
#[derive(Default)]
struct Shaped<P: Paragraph> {
    for_: Option<(String, f32, f32, fit::Cut, f32)>,
    shown: String,
    paragraph: core_text::paragraph::Plain<P>,
    letters: Vec<(f32, P)>,
    /// Who it is to a screen reader, for as long as it is on screen.
    a11y: Option<iced_accessibility::Id>,
}

/// How far apart, in `em` of the size, the characters of text `size` pixels
/// tall are drawn beyond what the face spaces them: Apple's rule that
/// tracking is the size's, not one value for every size. Body text and
/// below is left as the face has it; from there it closes up a little more
/// with each pixel, to a fortieth of an em at 28 and over, where letters
/// drawn as far apart as at 13 read as loose.
pub fn tracking(size: f32) -> f32 {
    -0.025 * ((size - 14.0) / 14.0).clamp(0.0, 1.0)
}

/// How big a page's title is: the month on money's budget, the source on
/// harken's list.
pub const TITLE: f32 = 22.0;

/// A page's title, the one both windows draw: bold, at [`TITLE`], closed
/// up as its size wants ([`tracking`]), in the text's colour, losing its
/// end when it does not fit ([`Line::cut`] for another part).
pub fn title<'a>(body: impl Into<String>) -> Line<'a> {
    Line::new(body, TITLE, fit::Cut::End, |theme| crate::theme::of(theme).background.base.text)
        .font(crate::hero::BOLD)
        .tracking(tracking(TITLE))
}

impl Line<'_> {
    /// Losing the part `cut` says, rather than the one it was made with.
    pub fn cut(mut self, cut: fit::Cut) -> Self {
        self.cut = cut;
        self
    }
}

/// Whether every character of `s` is a whole grapheme of its own, so that
/// each can be drawn apart from the rest: nothing that joins onto the one
/// before it — an accent written after its letter, a joiner, a variation
/// selector — and no emoji, which are often several. A line that is not
/// is drawn as the face spaces it, untracked.
fn letters_apart(s: &str) -> bool {
    s.chars().all(|c| {
        !matches!(c as u32,
            0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F | 0x200C | 0x200D | 0xFE00..=0xFE0F | 0x1F000..)
    })
}

impl<M, R> Widget<M, Theme, R> for Line<'_>
where
    R: core_text::Renderer<Font = Font>,
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
        let size = self.size;
        let font = self.font.unwrap_or_else(|| renderer.default_font());
        // Tracked, every gap between two characters is `step` wider.
        let step = match letters_apart(&self.body) {
            true => self.tracking * size,
            false => 0.0,
        };

        let key = (self.body.clone(), max.width, self.size, self.cut, step);
        if cut.for_.as_ref() != Some(&key) {
            cut.shown = fit::within(&self.body, max.width, self.cut, |s| {
                let gaps = s.chars().count().saturating_sub(1) as f32;
                R::Paragraph::with_text(one_line(s, size, font, max.height)).min_bounds().width + step * gaps
            });
            let _ = cut.paragraph.update(one_line(&cut.shown, size, font, max.height));
            // Each character where the face put it in the whole line, so
            // its pairs keep their kerning, and then `step` more for each
            // one before it.
            cut.letters = match step {
                0.0 => Vec::new(),
                _ => {
                    let mut buf = [0u8; 4];
                    let whole = cut.paragraph.raw();
                    cut.shown
                        .chars()
                        .enumerate()
                        .map(|(i, c)| {
                            let x = whole.grapheme_position(0, i).map_or(0.0, |p| p.x) + step * i as f32;
                            (x, R::Paragraph::with_text(one_line(c.encode_utf8(&mut buf), size, font, max.height)))
                        })
                        .collect()
                }
            };
            cut.for_ = Some(key);
        }
        // As wide as the text and no wider, whatever the limits' minimum: a
        // row gives what fills its whole share as the minimum, and a line
        // that [gives way](Line::gives_way) must still answer with its own
        // width, or what follows it is pushed to the far end.
        let _ = cut.a11y.get_or_insert_with(iced_accessibility::Id::unique);
        let text = cut.paragraph.min_bounds();
        let gaps = cut.letters.len().saturating_sub(1) as f32;
        let width = (text.width + step * gaps).max(0.0);
        layout::Node::new(Size::new(width.min(max.width), text.height.min(max.height)))
    }

    /// A label: the whole of what it says, even where it is cut to fit, for
    /// a screen reader (the fork's accesskit).
    fn a11y_nodes(&self, layout: Layout<'_>, tree: &Tree, _cursor: mouse::Cursor) -> iced_accessibility::A11yTree {
        use iced_accessibility::accesskit::{Node, Rect, Role};
        let Some(id) = tree.state.downcast_ref::<Shaped<R::Paragraph>>().a11y.clone() else {
            return iced_accessibility::A11yTree::default();
        };
        let b = layout.bounds();
        let mut node = Node::new(Role::Label);
        node.set_label(self.body.clone().into_boxed_str());
        node.set_bounds(Rect::new(
            f64::from(b.x),
            f64::from(b.y),
            f64::from(b.x + b.width),
            f64::from(b.y + b.height),
        ));
        iced_accessibility::A11yTree::leaf(node, id)
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
        let style = widget::text::Style {
            color: Some((self.color)(theme)),
            ..Default::default()
        };
        let bounds = layout.bounds();
        if cut.letters.is_empty() {
            widget::text::draw(renderer, defaults, bounds, cut.paragraph.raw(), style, viewport);
            return;
        }
        for (x, letter) in &cut.letters {
            let at = Rectangle::new(Point::new(bounds.x + x, bounds.y), letter.min_bounds());
            widget::text::draw(renderer, defaults, at, letter, style, viewport);
        }
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
        line_height: crate::text::LINE_HEIGHT,
        font,
        align_x: core_text::Alignment::Default,
        align_y: alignment::Vertical::Top,
        shaping: core_text::Shaping::default(),
        wrapping: core_text::Wrapping::None,
        // Cut here, by `fit::Cut`, never by the renderer's own ellipsis.
        ellipsize: core_text::Ellipsize::None,
    }
}

impl<'a, M: 'a, R> From<Line<'a>> for Element<'a, M, Theme, R>
where
    R: core_text::Renderer<Font = Font> + 'a,
{
    fn from(line: Line<'a>) -> Self {
        Element::new(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Body text keeps the face's spacing; a title closes up, more the
    /// bigger it is, and never by more than a fortieth of an em. Falsified
    /// by one value for every size: 13 was tightened as much as 28.
    #[test]
    fn tracking_is_the_size_s() {
        assert_eq!(tracking(13.0), 0.0);
        assert!(tracking(TITLE) < 0.0);
        assert!(tracking(28.0) < tracking(TITLE));
        assert_eq!(tracking(40.0), tracking(28.0));
        assert!(tracking(28.0) >= -0.025);
    }

    /// A name with an accent written as its own character, or an emoji, is
    /// not drawn a character at a time. Falsified by a check for ASCII
    /// alone: “Dvořák” (one character for its ř) was drawn untracked.
    #[test]
    fn only_what_is_whole_characters_is_tracked() {
        assert!(letters_apart("November 2026"));
        assert!(letters_apart("Dvořák “matching”"));
        assert!(!letters_apart("Dvor\u{30C}a\u{301}k"));
        assert!(!letters_apart("Favorites \u{1F3B5}"));
    }
}
