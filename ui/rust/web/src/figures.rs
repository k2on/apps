//! A page of figures (ui/docs/parts.md, "Figures") in the DOM, as
//! `arkui::figures` draws them in iced and `FigureParts` on the phones:
//! the charts are inline SVG in the theme's `[chart]` palette
//! (`--ark-chart-N`), plain shapes where a bar will do.

use leptos::html;
use leptos::prelude::*;

use crate::{glyph, send};

/// What a figure says about itself (`parts::Tint`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint {
    Neutral,
    Positive,
    Caution,
    Negative,
    Accent,
}

impl Tint {
    /// A tint by the name a view gives it (`ark_app::view::HeaderCard`'s
    /// `tint`), as iced's `Tint::named`: neutral unless it is one of the
    /// others.
    pub fn named(name: &str) -> Tint {
        match name {
            "positive" => Tint::Positive,
            "caution" => Tint::Caution,
            "negative" => Tint::Negative,
            "accent" => Tint::Accent,
            _ => Tint::Neutral,
        }
    }

    pub fn class(self) -> &'static str {
        match self {
            Tint::Neutral => "ark-tint-neutral",
            Tint::Positive => "ark-tint-positive",
            Tint::Caution => "ark-tint-caution",
            Tint::Negative => "ark-tint-negative",
            Tint::Accent => "ark-tint-accent",
        }
    }
}

/// A figure in a capsule of its state: wrong filled in the error's colour
/// with its warning glyph, worth a look with a clock, good and the rest
/// in a quiet capsule.
pub fn status_badge(figure: impl Into<String>, tint: Tint) -> AnyView {
    let mark = match tint {
        Tint::Negative => Some(glyph("triangle-alert", 11)),
        Tint::Caution => Some(glyph("clock", 11)),
        _ => None,
    };
    html::span()
        .class(format!("ark-status {}", tint.class()))
        .child((mark, html::span().child(figure.into())))
        .into_any()
}

/// A slim bar under a row's name that fills toward its goal.
pub fn goal_bar(fraction: f32, tint: Tint) -> AnyView {
    let f = (fraction.clamp(0.0, 1.0) * 100.0).round();
    html::div()
        .class(format!("ark-goal {}", tint.class()))
        .role("progressbar")
        .attr("aria-valuenow", f.to_string())
        .child(html::div().class("ark-goal-fill").attr("style", format!("width:{f}%")))
        .into_any()
}

/// A page's large figure: its label over it, a caption under it, and what
/// sits at its end (a button).
pub fn headline(label: &str, figure: impl Into<String>, tint: Tint, caption: &str, trailing: Option<AnyView>) -> AnyView {
    html::div()
        .class("ark-headline-row")
        .child((
            html::div().class("ark-headline-words").child((
                html::div().class("ark-caption").child(label.to_string()),
                html::div().class(format!("ark-headline {}", tint.class())).child(figure.into()),
                (!caption.is_empty()).then(|| html::div().class("ark-caption").child(caption.to_string())),
            )),
            trailing,
        ))
        .into_any()
}

/// A capsule button: bordered, or filled in the accent when prominent.
pub fn capsule_button<M: Clone + 'static>(g: Option<&str>, title: &str, prominent: bool, press: M) -> AnyView {
    crate::wired!(html::button(), crate::tapped(title, &press))
        .class(if prominent { "ark-capsule ark-prominent" } else { "ark-capsule" })
        .on(leptos::ev::click, move |_| send(press.clone()))
        .child((g.map(|g| glyph(g, 13)), html::span().child(title.to_string())))
        .into_any()
}

/// A bar of notice over a list: its glyph, its words, its actions.
pub fn notice_bar(g: &str, words: &str, actions: Vec<AnyView>) -> AnyView {
    html::div()
        .class("ark-notice")
        .role("status")
        .child((
            html::span().class("ark-notice-glyph").child(glyph(g, 16)),
            html::span().class("ark-notice-words").child(words.to_string()),
            html::div().class("ark-notice-actions").child(actions),
        ))
        .into_any()
}

/// Figures side by side: each its label over it.
pub fn figure_strip(items: Vec<(String, String, Tint)>) -> AnyView {
    let cells: Vec<AnyView> = items
        .into_iter()
        .map(|(l, f, t)| {
            html::div()
                .class("ark-strip-cell")
                .child((
                    html::div().class("ark-caption").child(l),
                    html::div().class(format!("ark-strip-figure {}", t.class())).child(f),
                ))
                .into_any()
        })
        .collect();
    html::div().class("ark-strip").child(cells).into_any()
}

/// A figure on a card, its caption's line kept so tiles are one height.
pub fn stat_tile(label: &str, figure: impl Into<String>, caption: Option<&str>) -> AnyView {
    html::div()
        .class("ark-card ark-stat")
        .child((
            html::div().class("ark-caption").child(label.to_string()),
            html::div().class("ark-stat-figure").child(figure.into()),
            html::div().class("ark-caption").child(caption.unwrap_or("\u{a0}").to_string()),
        ))
        .into_any()
}

/// A chart on a card under its title.
pub fn chart_card(title: &str, chart: AnyView) -> AnyView {
    html::section()
        .class("ark-chart-card")
        .child((
            html::h3().class("ark-chart-title").child(title.to_string()),
            html::div().class("ark-card ark-chart-body").child(chart),
        ))
        .into_any()
}

/// Items in `columns` columns.
pub fn grid(items: Vec<AnyView>, columns: usize, gap: f32) -> AnyView {
    html::div()
        .class("ark-grid")
        .attr(
            "style",
            format!("grid-template-columns:repeat({},minmax(0,1fr));gap:{gap}px", columns.max(1)),
        )
        .child(items)
        .into_any()
}

/// A bar of a labelled bar chart.
#[derive(Clone, Debug)]
pub struct Bar {
    pub label: String,
    pub value: i64,
    pub said: String,
}

/// Bars, each its label, its share of the largest and its figure, in the
/// chart's colours in turn.
pub fn labelled_bars(bars: &[Bar]) -> AnyView {
    let most = bars.iter().map(|b| b.value).max().unwrap_or(1).max(1) as f64;
    let rows: Vec<AnyView> = bars
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let share = (b.value as f64 / most).clamp(0.0, 1.0) * 100.0;
            html::div()
                .class("ark-lbar")
                .child((
                    html::span().class("ark-lbar-label").child(b.label.clone()),
                    html::span().class("ark-lbar-track").child(
                        html::span()
                            .class("ark-lbar-fill")
                            .attr("style", format!("width:{share:.1}%;background:var(--ark-chart-{})", i % 6)),
                    ),
                    html::span().class("ark-lbar-said").child(b.said.clone()),
                ))
                .into_any()
        })
        .collect();
    html::div().class("ark-lbars").child(rows).into_any()
}

/// A series of a column chart.
#[derive(Clone, Debug)]
pub struct Series {
    pub name: String,
}

/// Columns in groups (a month each), a column per series in its colour,
/// the legend over them.
pub fn column_chart(series: &[Series], groups: &[(String, Vec<i64>)], height: f64) -> AnyView {
    let most = groups.iter().flat_map(|g| g.1.iter().copied()).max().unwrap_or(1).max(1) as f64;
    let legend: Vec<AnyView> = series
        .iter()
        .enumerate()
        .map(|(i, s)| {
            html::span()
                .class("ark-legend")
                .child((
                    html::span()
                        .class("ark-legend-swatch")
                        .attr("style", format!("background:var(--ark-chart-{})", i % 6)),
                    s.name.clone(),
                ))
                .into_any()
        })
        .collect();
    let cols: Vec<AnyView> = groups
        .iter()
        .map(|(label, values)| {
            let bars: Vec<AnyView> = values
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let h = (*v as f64 / most).clamp(0.0, 1.0) * 100.0;
                    html::span()
                        .class("ark-column-bar")
                        .attr("style", format!("height:{h:.1}%;background:var(--ark-chart-{})", i % 6))
                        .into_any()
                })
                .collect();
            html::div()
                .class("ark-column-group")
                .child((
                    html::div().class("ark-column-bars").child(bars),
                    html::div().class("ark-caption").child(label.clone()),
                ))
                .into_any()
        })
        .collect();
    html::div()
        .class("ark-columns")
        .child((
            html::div().class("ark-legends").child(legend),
            html::div()
                .class("ark-column-groups")
                .attr("style", format!("height:{height}px"))
                .child(cols),
        ))
        .into_any()
}

/// A line of `points` (oldest first), its area to the foot when asked,
/// in the accent.
pub fn line_chart(points: &[i64], height: f64, area: bool) -> AnyView {
    html::div()
        .class("ark-line-chart")
        .attr("style", format!("height:{height}px"))
        .inner_html(line_svg(points, area))
        .into_any()
}

/// A line too small for an axis, `width` by `height`.
pub fn sparkline(points: &[i64], width: f64, height: f64) -> AnyView {
    html::div()
        .class("ark-line-chart ark-sparkline")
        .attr("style", format!("width:{width}px;height:{height}px"))
        .inner_html(line_svg(points, false))
        .into_any()
}

/// The line as SVG, 100 by 40 units, stretched to its box: the geometry
/// of `arkui::figures`' line, so the two draw one line.
pub fn line_svg(points: &[i64], area: bool) -> String {
    let (w, h) = (100.0f64, 40.0f64);
    let lo = points.iter().copied().min().unwrap_or(0) as f64;
    let hi = points.iter().copied().max().unwrap_or(0) as f64;
    let span = (hi - lo).max(1.0);
    let n = points.len().max(2) as f64 - 1.0;
    let at: Vec<(f64, f64)> = points
        .iter()
        .enumerate()
        .map(|(i, p)| (i as f64 / n * w, h - 2.0 - (*p as f64 - lo) / span * (h - 4.0)))
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
    body += &format!(r#"<path d="{line}" fill="none" stroke="currentColor" stroke-width="1.5" vector-effect="non-scaling-stroke"/>"#);
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" preserveAspectRatio="none" width="100%" height="100%">{body}</svg>"#)
}

/// A QR code as inline SVG: `size` modules a side, each dark one a square
/// in the ink's colour, the quiet zone of four modules round it on the
/// page's ground (iced's `arkui::qr`).
pub fn qr_svg(size: usize, dark: &[bool]) -> AnyView {
    const QUIET: usize = 4;
    let side = size + 2 * QUIET;
    let mut d = String::new();
    for (i, _) in dark.iter().enumerate().filter(|(_, on)| **on).take(size * size) {
        let (x, y) = (i % size.max(1) + QUIET, i / size.max(1) + QUIET);
        d.push_str(&format!("M{x} {y}h1v1h-1z"));
    }
    leptos::svg::svg()
        .attr("class", "ark-code")
        .attr("viewBox", format!("0 0 {side} {side}"))
        .attr("role", "img")
        .attr("aria-label", "QR Code")
        .attr("shape-rendering", "crispEdges")
        .child((
            leptos::svg::rect()
                .attr("width", side)
                .attr("height", side)
                .attr("class", "ark-code-ground"),
            leptos::svg::path().attr("d", d).attr("class", "ark-code-ink"),
        ))
        .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The area runs to the foot whatever the sign of the figures, as
    /// iced's does. Falsified by closing the area at the zero line.
    #[test]
    fn the_area_runs_to_the_foot() {
        let s = line_svg(&[-300, -250, -240], true);
        assert!(s.contains("L100 40L0 40Z"), "{s}");
        assert_eq!(line_svg(&[1, 2], false).matches("<path").count(), 1);
    }
}
