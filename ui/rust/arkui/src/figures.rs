//! The parts a page of figures is laid out of, the same in every toolkit
//! (ui/docs/parts.md, "Figures"): a figure in a capsule of its state, a
//! page's headline figure, a notice bar with its actions, a strip of
//! labelled figures, a stat tile, a chart's card, and the charts — bars
//! labelled over them, columns in groups, a line over its area, a
//! sparkline. Compose's `Figures.kt` and SwiftUI's `Figures.swift` have the
//! same names and inputs.
//!
//! Nothing here names anything an app is about: a figure is text the app
//! wrote, a state is a [`Tint`]. Every colour is the palette's.

use crate::text::{self, text};
use iced::widget::{button, column, container, row, svg, Space};
use iced::{Alignment, Background, Border, Element, Length, Padding, Theme};

use crate::layout_tokens::PROGRESS_WIDTH;
use crate::parts::{progress_bar, Tint};
use crate::theme::of;
use crate::{glyphs, icon, style};

/// The size of a page's headline figure, and of a tile's.
pub const HEADLINE: f32 = 28.0;
pub const TILE_FIGURE: f32 = 22.0;
/// A slim bar under a row's name.
pub const GOAL_GIRTH: f32 = 4.0;

/// A figure in a capsule of its state: wrong is filled in the error's
/// colour with its warning glyph, worth a look carries a clock, good and
/// nothing are washed in their tint. The one place a page shouts.
pub fn status_badge<'a, M: 'a>(figure: impl Into<String>, tint: Tint) -> Element<'a, M> {
    let glyph = match tint {
        Tint::Negative => Some(glyphs::ALERT),
        Tint::Caution => Some(glyphs::CLOCK),
        _ => None,
    };
    let ink = move |t: &Theme| match tint {
        Tint::Negative => of(t).danger.base.text,
        Tint::Neutral => of(t).background.base.text.scale_alpha(0.6),
        _ => tint.color(t),
    };
    let mut line = row![].spacing(4).align_y(Alignment::Center);
    if let Some(g) = glyph {
        line = line.push(icon::tinted(g, 11.0, move |p| match tint {
            Tint::Negative => p.danger.base.text,
            _ => tint_of(p, tint),
        }));
    }
    line = line.push(text(figure.into()).size(13).font(crate::hero::BOLD).style(move |t| text::Style {
        color: Some(ink(t)),
        ..Default::default()
    }));
    container(line)
        .padding([2, 8])
        .style(move |t: &Theme| container::Style {
            background: Some(Background::Color(match tint {
                Tint::Negative => of(t).danger.base.color,
                Tint::Neutral => of(t).background.base.text.scale_alpha(0.07),
                _ => tint.color(t).scale_alpha(0.16),
            })),
            border: Border {
                radius: 10.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

fn tint_of(p: &iced::theme::palette::Extended, tint: Tint) -> iced::Color {
    match tint {
        Tint::Neutral => p.background.base.text.scale_alpha(0.35),
        Tint::Positive => p.success.base.color,
        Tint::Caution => p.warning.base.color,
        Tint::Negative => p.danger.base.color,
        Tint::Accent => p.primary.base.color,
    }
}

/// A slim bar toward a goal under a row's name, as wide as the theme says
/// beside columns (`progress-width`).
pub fn goal_bar<'a, M: 'a>(fraction: f32, tint: Tint) -> Element<'a, M> {
    container(progress_bar(fraction, tint, GOAL_GIRTH)).width(PROGRESS_WIDTH).into()
}

/// A page's headline figure: what it is (small, quiet), the figure large
/// and bold in its tint, and a caption under it; `trailing` (a menu, an
/// action) at its end.
pub fn headline<'a, M: 'a>(label: &str, figure: impl Into<String>, tint: Tint, caption: &str, trailing: Option<Element<'a, M>>) -> Element<'a, M> {
    let words = column![
        text(label.to_string()).size(12).style(style::dim),
        text(figure.into()).size(HEADLINE).font(crate::hero::BOLD).style(move |t| text::Style {
            color: Some(match tint {
                Tint::Neutral => of(t).background.base.text,
                _ => tint.color(t),
            }),
            ..Default::default()
        }),
        text(caption.to_string()).size(11).style(style::dim),
    ]
    .spacing(1);
    let mut line = row![words, Space::new().width(Length::Fill)].align_y(Alignment::Center);
    if let Some(t) = trailing {
        line = line.push(t);
    }
    line.into()
}

/// A button in a capsule: an action that sits on a page rather than in its
/// toolbar (Auto-Assign by the headline figure, Review on a notice bar).
/// `prominent` fills it with the accent.
pub fn capsule_button<'a, M: Clone + 'a>(glyph: Option<&'static [u8]>, title: &str, prominent: bool, press: M) -> Element<'a, M> {
    let mut label = row![].spacing(6).align_y(Alignment::Center);
    if let Some(g) = glyph {
        label = label.push(icon::tinted(g, 13.0, move |p| match prominent {
            true => p.primary.base.text,
            false => p.background.base.text.scale_alpha(0.8),
        }));
    }
    label = label.push(text(title.to_string()).size(12).style(move |t| text::Style {
        color: Some(match prominent {
            true => of(t).primary.base.text,
            false => of(t).background.base.text,
        }),
        ..Default::default()
    }));
    button(label)
        .padding([3, 10])
        .style(move |t: &Theme, s| {
            let p = of(t);
            let base = match prominent {
                true => p.primary.base.color,
                false => p.background.base.text.scale_alpha(0.07),
            };
            let wash = match s {
                button::Status::Hovered => 0.85,
                button::Status::Pressed => 0.7,
                _ => 1.0,
            };
            button::Style {
                background: Some(Background::Color(base.scale_alpha(wash))),
                text_color: p.background.base.text,
                border: Border {
                    radius: 7.0.into(),
                    width: if prominent { 0.0 } else { 1.0 },
                    color: style::edge_color(t),
                },
                ..button::Style::default()
            }
        })
        .on_press(press)
        .into()
}

/// A notice across the top of a list with what can be done about it: its
/// glyph in the accent, its sentence, and its actions at its end.
pub fn notice_bar<'a, M: 'a>(shape: &'static [u8], words: &str, actions: Vec<Element<'a, M>>) -> Element<'a, M> {
    container(
        row![
            icon::tinted(shape, 16.0, |p| p.primary.base.color),
            container(text(words.to_string()).size(12)).width(Length::Fill),
            row(actions).spacing(6).align_y(Alignment::Center)
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    )
    .padding([6, 14])
    .width(Length::Fill)
    .style(style::surface)
    .into()
}

/// Figures side by side, each its label over it: an account's cleared,
/// uncleared and working balances. A figure below nothing is in the
/// error's colour.
pub fn figure_strip<'a, M: 'a>(items: Vec<(String, String, Tint)>) -> Element<'a, M> {
    row(items.into_iter().map(|(label, figure, tint)| {
        column![
            text(label).size(12).style(style::dim),
            text(figure).size(14).font(crate::hero::BOLD).style(move |t| text::Style {
                color: Some(match tint {
                    Tint::Neutral => of(t).background.base.text,
                    _ => tint.color(t),
                }),
                ..Default::default()
            }),
        ]
        .spacing(1)
        .into()
    }))
    .spacing(22)
    .into()
}

/// A tile with one figure: what it is, the figure large, a caption under
/// it while there is one.
pub fn stat_tile<'a, M: 'a>(label: &str, figure: impl Into<String>, caption: Option<&str>) -> Element<'a, M> {
    let mut c = column![
        text(label.to_string()).size(12).style(style::dim),
        text(figure.into()).size(TILE_FIGURE).font(crate::hero::BOLD),
    ]
    .spacing(2);
    // Every tile has the caption's line, so a row of them is one height
    // whether or not each says something there.
    c = c.push(text(caption.unwrap_or(" ").to_string()).size(11).style(style::dim));
    container(c).padding([10, 12]).width(Length::Fill).style(style::card).into()
}

/// A chart in its card under its title.
pub fn chart_card<'a, M: 'a>(title: &str, chart: impl Into<Element<'a, M>>) -> Element<'a, M> {
    column![
        text(title.to_string()).size(13).font(crate::hero::BOLD),
        container(chart).padding(10).width(Length::Fill).style(style::card)
    ]
    .spacing(6)
    .width(Length::Fill)
    .into()
}

/// Tiles or cards `columns` across, read across, `gap` between them both
/// ways; a short last row keeps its columns where the full ones have them
/// (SwiftUI's `LazyVGrid`, Compose's `FlowRow` of equal weights).
pub fn grid<'a, M: 'a>(items: Vec<Element<'a, M>>, columns: usize, gap: f32) -> Element<'a, M> {
    let columns = columns.max(1);
    let mut rows = column![].spacing(gap);
    let mut items = items.into_iter().peekable();
    while items.peek().is_some() {
        let chunk: Vec<_> = items.by_ref().take(columns).collect();
        let n = chunk.len();
        let mut line = row![].spacing(gap).align_y(Alignment::Start);
        for e in chunk {
            line = line.push(container(e).width(Length::Fill));
        }
        for _ in n..columns {
            line = line.push(Space::new().width(Length::Fill));
        }
        rows = rows.push(line);
    }
    rows.width(Length::Fill).into()
}

/// One bar of [`labelled_bars`]: its label, its value and how it is said.
pub struct Bar {
    pub label: String,
    pub value: i64,
    pub said: String,
}

/// Bars against the largest, each its label over it and its value said at
/// its end: one hue for magnitude (the chart's first), largest first as given.
pub fn labelled_bars<'a, M: 'a>(bars: &[Bar]) -> Element<'a, M> {
    let most = bars.iter().map(|b| b.value).max().unwrap_or(1).max(1) as f32;
    column(bars.iter().map(|b| {
        let share = (b.value as f32 / most).clamp(0.0, 1.0);
        column![
            text(b.label.clone()).size(11).style(style::dim),
            row![
                container(Space::new().width(Length::Fill).height(6))
                    .style(|t: &Theme| container::Style {
                        background: Some(Background::Color(crate::theme::chart(t, 0))),
                        border: Border {
                            radius: 3.0.into(),
                            ..Border::default()
                        },
                        ..container::Style::default()
                    })
                    .width(Length::FillPortion((share * 1000.0).max(1.0) as u16)),
                container(text(b.said.clone()).size(11).style(style::dim))
                    .padding(Padding::ZERO.left(6))
                    .width(Length::FillPortion(((1.0 - share) * 1000.0).max(1.0) as u16 + 260)),
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(1)
        .into()
    }))
    .spacing(6)
    .into()
}

/// A series of [`column_chart`]: its name in the legend; it is drawn in
/// the chart's colour of its place (`theme::chart`).
pub struct Series {
    pub name: String,
}

/// Columns in groups side by side (a month's income beside its spending),
/// a legend over them and each group's label under it; `height` tall.
pub fn column_chart<'a, M: 'a>(series: &[Series], groups: &[(String, Vec<i64>)], height: f32) -> Element<'a, M> {
    let most = groups.iter().flat_map(|g| g.1.iter().copied()).max().unwrap_or(1).max(1) as f32;
    let legend = row(series.iter().enumerate().map(|(i, s)| {
        row![
            container(Space::new().width(8).height(8)).style(move |t: &Theme| container::Style {
                background: Some(Background::Color(crate::theme::chart(t, i))),
                border: Border {
                    radius: 4.0.into(),
                    ..Border::default()
                },
                ..container::Style::default()
            }),
            text(s.name.clone()).size(11).style(style::dim)
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .into()
    }))
    .spacing(12);
    let columns = row(groups.iter().map(|(label, values)| {
        let bars = row(values.iter().enumerate().map(|(i, v)| {
            let h = (*v as f32 / most * height).max(1.0);
            container(Space::new().width(Length::Fill).height(h))
                .width(Length::Fill)
                .style(move |t: &Theme| container::Style {
                    background: Some(Background::Color(crate::theme::chart(t, i))),
                    border: Border {
                        radius: 3.0.into(),
                        ..Border::default()
                    },
                    ..container::Style::default()
                })
                .into()
        }))
        .spacing(6)
        .height(height)
        .align_y(Alignment::End);
        column![
            container(bars).padding([0, 10]),
            container(text(label.clone()).size(11).style(style::dim)).center_x(Length::Fill)
        ]
        .spacing(4)
        .width(Length::Fill)
        .into()
    }))
    .spacing(8);
    column![legend, columns].spacing(8).into()
}

/// A line through `points` (oldest first) over a light area, in the
/// chart's first colour; `height` tall. The area runs down to the chart's
/// foot, whichever side of nothing the line is — never up to nothing from
/// below.
pub fn line_chart<'a, M: 'a>(points: &[i64], height: f32, area: bool) -> Element<'a, M> {
    let svg_doc = line_svg(points, area);
    svg(svg::Handle::from_memory(svg_doc.into_bytes()))
        .width(Length::Fill)
        .height(height)
        .content_fit(iced::ContentFit::Fill)
        .style(move |t, _| svg::Style {
            color: Some(crate::theme::chart(t, 0)),
        })
        .into()
}

/// A small line with no area: a trend beside a figure.
pub fn sparkline<'a, M: 'a>(points: &[i64], height: f32) -> Element<'a, M> {
    line_chart(points, height, false)
}

/// The line as SVG, 100 by 40 units, stretched to its box.
fn line_svg(points: &[i64], area: bool) -> String {
    let (w, h) = (100.0f32, 40.0f32);
    let lo = points.iter().copied().min().unwrap_or(0) as f32;
    let hi = points.iter().copied().max().unwrap_or(0) as f32;
    let span = (hi - lo).max(1.0);
    let n = points.len().max(2) as f32 - 1.0;
    let at: Vec<(f32, f32)> = points
        .iter()
        .enumerate()
        .map(|(i, p)| (i as f32 / n * w, h - 2.0 - (*p as f32 - lo) / span * (h - 4.0)))
        .collect();
    let line: String = at
        .iter()
        .enumerate()
        .map(|(i, (x, y))| format!("{}{x:.2} {y:.2}", if i == 0 { "M" } else { "L" }))
        .collect();
    let mut body = String::new();
    if area && !at.is_empty() {
        body += &format!(r#"<path d="{line}L{w} {h}L0 {h}Z" fill="currentColor" fill-opacity="0.16" stroke="none"/>"#);
    }
    body += &format!(r#"<path d="{line}" fill="none" stroke="currentColor" stroke-width="1.2" vector-effect="non-scaling-stroke"/>"#);
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" preserveAspectRatio="none">{body}</svg>"#)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The area runs to the foot whatever the sign of the figures, so a
    /// negative net worth is not a block filled up to nothing. Falsified by
    /// closing the area at the zero line.
    #[test]
    fn the_area_runs_to_the_foot() {
        let s = line_svg(&[-300, -250, -240], true);
        assert!(s.contains("L100 40L0 40Z"), "{s}");
        assert!(line_svg(&[1, 2], false).matches("<path").count() == 1);
    }
}
