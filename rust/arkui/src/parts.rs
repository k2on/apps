//! The small generic pieces a page is laid out of, with the same names and
//! the same inputs as the phones' (Compose's `dev.arkui` `Parts.kt`,
//! SwiftUI's `Parts.swift`; docs/arkui-parts.md lists them): a page's header
//! about one thing, a section's heading, rows in a card with hairlines
//! between them, a notice across the page, a value in a tinted pill, one of a
//! few to choose, a count, a glyph on a washed square, a bar that fills
//! toward a goal and bars against the largest, the fields a search and a
//! name are typed in, a pager, a scrubber and the bar a finger drags.
//!
//! Nothing here names anything an app is about, and nothing writes a colour
//! down: every colour is the palette's ([`crate::theme::of`]), through the
//! looks in [`crate::style`]. The desktop's measures are the desktop's — a
//! pointer is precise and a window wide — so a card is rounded by 8 here and
//! by 20 on a phone; what is the same is the piece and what it is handed.
use iced::widget::{button, column, container, row, rule, slider, svg, text, text_input, Space};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme};

use crate::theme::of;
use crate::{button as buttons, glyphs, icon, line, style};

/// What a value says about itself: nothing to say, good, worth a look,
/// wrong, or only the app's accent. The phones' `Tint`, with the same five.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tint {
    Neutral,
    Positive,
    Caution,
    Negative,
    Accent,
}

impl Tint {
    /// The tint's colour, from the palette's signals; neutral is the text
    /// colour, faint.
    pub fn color(self, theme: &Theme) -> Color {
        let p = of(theme);
        match self {
            Tint::Neutral => p.background.base.text.scale_alpha(0.35),
            Tint::Positive => p.success.base.color,
            Tint::Caution => p.warning.base.color,
            Tint::Negative => p.danger.base.color,
            Tint::Accent => p.primary.base.color,
        }
    }

    /// Text in this tint: neutral a little stronger than its wash, so it
    /// still reads as text.
    pub fn text(self) -> impl Fn(&Theme) -> text::Style {
        move |theme| text::Style {
            color: Some(match self {
                Tint::Neutral => of(theme).background.base.text.scale_alpha(0.6),
                t => t.color(theme),
            }),
        }
    }
}

/// The header of a page about one thing: its `cover`, what kind of thing it
/// is in small type, its name in bold, who it is by (`by`, a link the
/// caller makes), the facts true of the whole of it, and the page's own
/// `actions` under them.
pub fn page_header<'a, M: 'a>(
    cover: Element<'a, M>,
    kind: &str,
    title: &str,
    facts: &str,
    by: Option<Element<'a, M>>,
    actions: Element<'a, M>,
) -> Element<'a, M> {
    let mut words = column![
        text(kind.to_string()).size(11).style(style::faint(0.55)),
        text(title.to_string()).size(28).font(crate::hero::BOLD),
    ]
    .spacing(4);
    if let Some(by) = by {
        words = words.push(by);
    }
    words = words.push(text(facts.to_string()).size(12).style(style::dim));
    row![cover, column![words, actions].spacing(16)]
        .spacing(24)
        .align_y(Alignment::End)
        .into()
}

/// A section's heading over its card: its name, quiet, and what it adds up
/// to — or an action — at its end.
pub fn section_header<'a, M: 'a>(title: &str, trailing: Option<Element<'a, M>>) -> Element<'a, M> {
    let mut line = row![
        text(title.to_string()).size(13).font(crate::hero::BOLD).style(style::faint(0.55)),
        Space::new().width(Length::Fill)
    ]
    .align_y(Alignment::Center);
    if let Some(t) = trailing {
        line = line.push(t);
    }
    container(line)
        .padding(Padding {
            top: 16.0,
            right: 4.0,
            bottom: 6.0,
            left: 4.0,
        })
        .into()
}

/// Rows in a card, as a grouped list draws a section: a [`row_divider`]
/// between each.
pub fn group_card<'a, M: 'a>(rows: Vec<Element<'a, M>>) -> Element<'a, M> {
    let mut list = column![].spacing(0).width(Length::Fill);
    for (i, r) in rows.into_iter().enumerate() {
        if i > 0 {
            list = list.push(row_divider());
        }
        list = list.push(container(r).padding([8, 0]));
    }
    container(list).padding([4, 14]).width(Length::Fill).style(style::card).into()
}

/// The hairline between two rows of a [`group_card`].
pub fn row_divider<'a, M: 'a>() -> Element<'a, M> {
    rule::horizontal(1).style(style::hairline).into()
}

/// A notice across the page — the server cannot be reached, say: a quiet
/// glyph beside a sentence, on the surface's step. It says; it does not ask.
pub fn banner<'a, M: 'a>(shape: &'static [u8], words: &str) -> Element<'a, M> {
    container(
        row![icon::plain(shape, true), text(words.to_string()).size(12).style(style::dim)]
            .spacing(10)
            .align_y(Alignment::Center),
    )
    .padding(12)
    .width(Length::Fill)
    .style(style::card)
    .into()
}

/// A value in a pill of its tint: `content` (an amount, a count) on a wash
/// of the tint, in the tint.
pub fn pill<'a, M: 'a>(tint: Tint, content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    container(content).padding([3, 10]).style(style::pill(tint)).into()
}

/// One of a few to choose from: the chosen one's word in the accent, the
/// rest in the text colour, no fill and no border on either.
pub fn chip<'a, M: Clone + 'a>(label: &str, chosen: bool, press: M) -> Element<'a, M> {
    button(text(label.to_string()).size(13))
        .on_press(press)
        .padding([5, 10])
        .style(style::chip(chosen))
        .into()
}

/// A count in the accent's bubble: what needs looking at.
pub fn badge<'a, M: 'a>(count: usize) -> Element<'a, M> {
    container(text(count.to_string()).size(10).style(move |theme| text::Style {
        color: Some(of(theme).primary.base.text),
    }))
    .padding([1, 6])
    .style(move |theme| container::Style {
        background: Some(Background::Color(of(theme).primary.base.color)),
        border: Border {
            radius: 8.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    })
    .into()
}

/// A glyph in `color` on a rounded square washed in it, `side` across.
pub fn tile<'a, M: 'a>(shape: &'static [u8], color: impl Fn(&Theme) -> Color + Copy + 'a, side: f32) -> Element<'a, M> {
    let mark = svg(svg::Handle::from_memory(shape))
        .width(Length::Fixed(side * 0.56))
        .height(Length::Fixed(side * 0.56))
        .style(move |t, _| svg::Style { color: Some(color(t)) });
    container(mark)
        .width(Length::Fixed(side))
        .height(Length::Fixed(side))
        .center_x(Length::Fixed(side))
        .center_y(Length::Fixed(side))
        .style(move |t| container::Style {
            background: Some(Background::Color(color(t).scale_alpha(0.16))),
            border: Border {
                radius: (side * 0.26).into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

/// A bar filling toward a goal: `fraction` of the way, in the tint over a
/// faint rail, `girth` thick.
pub fn progress_bar<'a, M: 'a>(fraction: f32, tint: Tint, girth: f32) -> Element<'a, M> {
    iced::widget::progress_bar(0.0..=1.0, fraction.clamp(0.0, 1.0))
        .girth(Length::Fixed(girth))
        .style(style::progress(tint))
        .into()
}

/// One part of [`bars`]: what it is and how much.
#[derive(Debug, Clone)]
pub struct Part {
    pub label: String,
    pub value: i64,
}

/// `parts` as bars, each against the largest, its label before it and its
/// value (`value` writes it) after: a legend and a comparison at once.
pub fn bars<'a, M: 'a>(parts: &[Part], value: impl Fn(i64) -> String, tint: Tint) -> Element<'a, M> {
    let most = parts.iter().map(|p| p.value).max().unwrap_or(1).max(1) as f32;
    let mut all = column![].spacing(10);
    for p in parts {
        all = all.push(
            row![
                text(p.label.clone()).size(13).width(Length::Fixed(160.0)),
                progress_bar(p.value as f32 / most, tint, 14.0),
                text(value(p.value)).size(13).width(Length::Fixed(110.0)),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        );
    }
    all.into()
}

/// The field a search is typed in: the field's look, `placeholder` while it
/// is empty. The caller gives it its id, what typing says and what Enter
/// does.
pub fn search_field<'a, M: Clone + 'a>(placeholder: &str, query: &str) -> text_input::TextInput<'a, M> {
    text_input(placeholder, query).padding([6, 10]).size(13).style(style::field)
}

/// The field a new thing's name is typed in: the field's look, `placeholder`
/// while it is empty.
pub fn name_field<'a, M: Clone + 'a>(placeholder: &str, name: &str) -> text_input::TextInput<'a, M> {
    text_input(placeholder, name).padding([6, 10]).size(13).style(style::field)
}

/// A title stepped through something ordered — a month, a week — with a
/// button either side, and, when it is elsewhere, the way back to where it
/// started (`home`, its label and what it says). Stepped from the keyboard
/// all day, so it does not move.
pub fn pager<'a, M: Clone + 'a>(title: impl Into<String>, back: M, forward: M, home: Option<(&str, M)>) -> Element<'a, M> {
    let mut steps = row![
        buttons::icon(glyphs::BACK, back),
        Element::from(line::title(title)),
        buttons::icon(glyphs::CHEVRON, forward)
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    if let Some((label, go)) = home {
        steps = steps.push(buttons::link(label.to_string(), 12.0, go));
    }
    steps.into()
}

/// How wide a [`scrubber`]'s clocks are: room for `1:23:45` at 11px.
pub const CLOCK: f32 = 48.0;

/// A position through something that runs: what has gone (`left`) before
/// the [`bar`] and what it runs to (`right`) after it, each as wide as the
/// longest it can say, so the bar does not shift as the seconds go by.
pub fn scrubber<'a, M: Clone + 'a>(
    left: String,
    right: String,
    total: f32,
    at: f32,
    on_change: impl Fn(f32) -> M + 'a,
    on_release: M,
) -> Element<'a, M> {
    row![
        text(left).size(11).style(style::dim).width(Length::Fixed(CLOCK)).align_x(Alignment::End),
        bar(0.0..=total, at, on_change).on_release(on_release).width(Length::Fill),
        text(right).size(11).style(style::dim).width(Length::Fixed(CLOCK)),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}

/// The bar a finger or a pointer drags along `range` — a seek, a volume:
/// the accent behind the handle ([`style::seek`]). The caller sets its width
/// and what letting go says.
pub fn bar<'a, M: Clone + 'a>(range: std::ops::RangeInclusive<f32>, at: f32, on_change: impl Fn(f32) -> M + 'a) -> slider::Slider<'a, f32, M> {
    slider(range, at, on_change).style(style::seek)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A neutral tint's text is stronger than its colour, which is a wash's;
    /// the rest are their colour.
    #[test]
    fn neutral_text_is_stronger_than_its_wash() {
        let t = Theme::Light;
        let c = Tint::Neutral.color(&t);
        let w = Tint::Neutral.text()(&t).color.unwrap();
        assert!(w.a > c.a);
        assert_eq!(Tint::Negative.text()(&t).color.unwrap(), Tint::Negative.color(&t));
    }
}
