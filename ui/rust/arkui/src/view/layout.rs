//! The layout primitives (`ark_app::view::layout`, ui/docs/primitives.md)
//! as iced's own: a stack a `column`, `row` or `stack`, a spacer a `Space`
//! filling its stack's axis, a scroll a `scrollable`, words `text` at the
//! desktop's size for their style (`[layout]`'s `type-*`), a glyph the
//! Lucide shape, a filled picture and the derived square drawn to the
//! square they are laid into (iced has no aspect-ratio layout: a square of
//! what it is given is `art-shelf` across, a grid's columns that wide), a
//! grid a wrapped row of cells, and each modifier a `container` around it.
//! Every gap is a `[layout]` step, as on the phones; a card's corner is the
//! desktop's.

use ark_app::view::{Align, Axis, Corner, Edges, Ink, Layout, Measure, Modifier, Part, Prim, Space as Step, Surface, TextStyle, Val, View};
use iced::widget::{column, container, row, scrollable, svg, Space};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme};

use super::{flatten, part, shape};
use crate::layout_tokens as tokens;
use crate::parts::Tint;
use crate::text::{self, text};
use crate::theme::of;

/// `l` drawn, in a stack along `axis` (none: in a part).
pub(super) fn layout<M: Clone + 'static>(l: Layout<M>, axis: Option<Axis>) -> Option<Element<'static, M>> {
    if !l.shown() {
        return None;
    }
    let el: Element<'static, M> = match l.prim {
        Prim::Stack {
            axis: along,
            align,
            spacing,
            content,
        } => {
            let gap = spacing.map(step).unwrap_or(0.0);
            match along {
                Axis::Vertical => column(held(*content, along)).spacing(gap).align_x(alignment(align)).into(),
                Axis::Horizontal => row(held(*content, along)).spacing(gap).align_y(alignment(align)).into(),
                // A square's leaves are drawn to it, round as it is, a
                // hairline over its edge (`style::outline`).
                Axis::Depth => match framed(&l.mods) {
                    Some(f) => {
                        let mut children = FRAME.with(|c| {
                            let was = c.replace(Some(f));
                            let children = held(*content, along);
                            c.set(was);
                            children
                        });
                        children.push(
                            container(Space::new())
                                .width(Length::Fixed(f.side))
                                .height(Length::Fixed(f.side))
                                .style(crate::style::outline(f.radius))
                                .into(),
                        );
                        iced::widget::Stack::with_children(children).into()
                    }
                    None => iced::widget::Stack::with_children(held(*content, along)).into(),
                },
            }
        }
        Prim::Spacer { min } => {
            let least = min.map(step).unwrap_or(0.0);
            match axis {
                Some(Axis::Horizontal) => row![Space::new().width(least), Space::new().width(Length::Fill)]
                    .width(Length::Fill)
                    .into(),
                Some(Axis::Vertical) => column![Space::new().height(least), Space::new().height(Length::Fill)]
                    .height(Length::Fill)
                    .into(),
                _ => Space::new().height(least).into(),
            }
        }
        Prim::Scroll { axis: along, content } => match along {
            Axis::Horizontal => scrollable(row(held(*content, Axis::Horizontal)))
                .direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::new()))
                .width(Length::Fill)
                .into(),
            _ => scrollable(column(held(*content, Axis::Vertical))).height(Length::Fill).into(),
        },
        Prim::Text(w) => {
            let ink = colour(w.ink);
            let mut t = text(w.text.get()).size(size(w.style)).style(move |th: &Theme| text::Style {
                color: Some(ink(th)),
                ..Default::default()
            });
            if w.style.bold() {
                t = t.font(crate::hero::BOLD);
            }
            // Kept to its lines, cut short with an ellipsis: a tile's
            // caption stays inside its cover's width.
            if let Some(n) = w.lines {
                t = t.ellipsize(iced::advanced::text::Ellipsize::End(iced::advanced::text::EllipsizeHeightLimit::Lines(
                    n as usize,
                )));
            }
            if w.centred {
                t = t.align_x(Alignment::Center);
            }
            t.into()
        }
        Prim::Glyph { name, style, ink } => {
            let ink = colour(ink);
            let side = size(style);
            svg(svg::Handle::from_memory(shape(&name.get())))
                .width(Length::Fixed(side))
                .height(Length::Fixed(side))
                .style(move |th, _| svg::Style { color: Some(ink(th)) })
                .into()
        }
        Prim::Image { url, label, fill: false, .. } => part(View::Part(Part::Photo { url, label, zoom: false }))?,
        // What has come of it, covering its square and cut to it; nothing
        // until then (what is under it shows).
        Prim::Image { url, fill: true, .. } => {
            let f = FRAME.with(|c| c.get());
            let (side, radius) = f.map(|f| (Length::Fixed(f.side), f.radius)).unwrap_or((Length::Fill, 0.0));
            match crate::images::remote(&url.get(), "arkui/picture") {
                Some(h) => {
                    // A person — half the side a corner — is made round in
                    // the picture's pixels, so it is a circle on every renderer.
                    let h = match f {
                        Some(f) if f.radius * 2.0 >= f.side - 0.5 => crate::images::round(&h, f.side),
                        _ => h,
                    };
                    iced::widget::image(h)
                        .width(side)
                        .height(side)
                        .content_fit(iced::ContentFit::Cover)
                        .border_radius(radius)
                        .into()
                }
                None => Space::new().width(side).height(side).into(),
            }
        }
        Prim::Swatch { seed } => {
            let f = FRAME.with(|c| c.get()).unwrap_or(Frame {
                side: tokens::ART_SHELF,
                radius: 0.0,
            });
            crate::art::square(&seed.get(), f.side, f.radius)
        }
        // Cells a column's least width each, wrapped: iced lays out no
        // equal columns of what is left.
        Prim::Grid { min, across, down, content } => {
            let w = measure(min);
            let cells: Vec<Element<'static, M>> = held(*content, Axis::Vertical)
                .into_iter()
                .map(|c| container(c).width(Length::Fixed(w)).into())
                .collect();
            row(cells).spacing(step(across)).wrap().vertical_spacing(step(down)).into()
        }
    };
    Some(modified(el, &l.mods))
}

/// What a stack holds, drawn along its axis.
fn held<M: Clone + 'static>(content: View<M>, along: Axis) -> Vec<Element<'static, M>> {
    flatten(content)
        .into_iter()
        .filter_map(|v| match v {
            View::Layout(l) => layout(*l, Some(along)),
            v => part(v),
        })
        .collect()
}

/// `el` in a container for each of `mods`, the first innermost; a corner
/// right after a background rounds it.
fn modified<M: 'static>(mut el: Element<'static, M>, mods: &[Modifier]) -> Element<'static, M> {
    let mut i = 0;
    while i < mods.len() {
        el = match &mods[i] {
            Modifier::Padding(edges, s) => container(el).padding(padding(*edges, step(*s))).into(),
            Modifier::FillWidth(a) => container(el).width(Length::Fill).align_x(alignment(*a)).into(),
            Modifier::FillHeight => container(el).height(Length::Fill).into(),
            Modifier::Background(s) => {
                let radius = match mods.get(i + 1) {
                    Some(Modifier::Corner(c)) => {
                        i += 1;
                        corner(*c)
                    }
                    _ => 0.0,
                };
                let s = *s;
                container(el)
                    .style(move |th: &Theme| container::Style {
                        background: Some(Background::Color(surface(s, th))),
                        border: Border {
                            radius: radius.into(),
                            ..Border::default()
                        },
                        ..container::Style::default()
                    })
                    .into()
            }
            // A corner on nothing filled rounds nothing iced draws; a
            // square's is its leaves' (above).
            Modifier::Corner(_) => el,
            // iced has no screen reader yet.
            Modifier::Combine => el,
            Modifier::Width(m) => container(el).width(measure(*m)).clip(true).into(),
            Modifier::Side(m) => container(el).width(measure(*m)).height(measure(*m)).clip(true).into(),
            // iced has no aspect-ratio layout: a square of what it is given
            // is a shelf's (a grid's columns are that wide).
            Modifier::Square => container(el).width(tokens::ART_SHELF).height(tokens::ART_SHELF).clip(true).into(),
        };
        i += 1;
    }
    el
}

/// A step of `[layout]`, as the phones take it.
fn step(s: Step) -> f32 {
    match s {
        Step::Xs => tokens::SPACE_XS,
        Step::S => tokens::SPACE_S,
        Step::M => tokens::SPACE_M,
        Step::L => tokens::SPACE_L,
        Step::Xl => tokens::SPACE_XL,
        Step::Xxl => tokens::SPACE_XXL,
        Step::Page => tokens::PAGE_MARGIN,
        Step::Section => tokens::SECTION_GAP,
    }
}

/// The desktop's size for words in `s` (`[layout]`'s `type-*`).
fn size(s: TextStyle) -> f32 {
    match s {
        TextStyle::LargeTitle => tokens::TYPE_LARGE_TITLE,
        TextStyle::Title => tokens::TYPE_TITLE,
        TextStyle::Headline => tokens::TYPE_HEADLINE,
        TextStyle::Body => tokens::TYPE_BODY,
        TextStyle::Subheadline => tokens::TYPE_SUBHEADLINE,
        TextStyle::Footnote => tokens::TYPE_FOOTNOTE,
        TextStyle::Caption => tokens::TYPE_CAPTION,
        TextStyle::Figure => tokens::TYPE_FIGURE,
    }
}

/// The desktop's corner: a card is rounded by `corner` here, as
/// `style::card` is (ui/docs/parts.md: a pointer's measures).
fn corner(c: Corner) -> f32 {
    match c {
        Corner::Card | Corner::Small => tokens::CORNER,
        // Round: as round as anything iced rounds gets.
        Corner::Circle => 1.0e4,
    }
}

fn alignment(a: Align) -> Alignment {
    match a {
        Align::Start => Alignment::Start,
        Align::Center => Alignment::Center,
        Align::End => Alignment::End,
    }
}

fn padding(e: Edges, s: f32) -> Padding {
    let z = Padding::ZERO;
    match e {
        Edges::All => Padding::new(s),
        Edges::Horizontal => z.left(s).right(s),
        Edges::Vertical => z.top(s).bottom(s),
        Edges::Top => z.top(s),
        Edges::Bottom => z.bottom(s),
        // iced lays nothing out right to left yet.
        Edges::Leading => z.left(s),
        Edges::Trailing => z.right(s),
    }
}

fn surface(s: Surface, th: &Theme) -> Color {
    match s {
        Surface::Card => crate::style::surface_color(th),
        Surface::Raised => of(th).background.strong.color,
    }
}

/// What an `Ink` is on the desktop's theme.
fn colour(i: Ink) -> impl Fn(&Theme) -> Color + Copy + 'static {
    let tint = |v: &Val<String>| Tint::named(&v.get());
    let (kind, t) = match &i {
        Ink::Ink => (0, Tint::Neutral),
        Ink::Quiet => (1, Tint::Neutral),
        Ink::Accent => (2, Tint::Accent),
        Ink::Tint(v) => (3, tint(v)),
        Ink::Figure(v) => (4, tint(v)),
    };
    move |th: &Theme| {
        let p = of(th);
        match (kind, t) {
            (1, _) => p.background.base.text.scale_alpha(crate::style::DIM),
            (2, _) => p.primary.base.color,
            (3, t) => t.color(th),
            (4, Tint::Neutral) | (0, _) => p.background.base.text,
            (_, t) => t.color(th),
        }
    }
}

/// A `[layout]` measure (`art-*`).
fn measure(m: Measure) -> f32 {
    match m {
        Measure::ArtRow => tokens::ART_ROW,
        Measure::ArtShelf => tokens::ART_SHELF,
        Measure::ArtHeader => tokens::ART_HEADER,
    }
}

/// The square a `zstack` is laid into and its corner, which its leaves (a
/// filled picture, the derived square) are drawn to.
#[derive(Clone, Copy)]
struct Frame {
    side: f32,
    radius: f32,
}

thread_local! {
    /// The square the leaves being drawn are in, while a framed stack's
    /// content is drawn.
    static FRAME: std::cell::Cell<Option<Frame>> = const { std::cell::Cell::new(None) };
}

/// The square `mods` lay a stack into, if they do: a side, or a square of
/// what it is given (a shelf's here), and its corner.
fn framed(mods: &[Modifier]) -> Option<Frame> {
    let side = mods.iter().find_map(|m| match m {
        Modifier::Side(m) => Some(measure(*m)),
        Modifier::Square => Some(tokens::ART_SHELF),
        _ => None,
    })?;
    let radius = mods
        .iter()
        .find_map(|m| match m {
            Modifier::Corner(Corner::Circle) => Some(side / 2.0),
            Modifier::Corner(c) => Some(corner(*c)),
            _ => None,
        })
        .unwrap_or(0.0);
    Some(Frame { side, radius })
}
