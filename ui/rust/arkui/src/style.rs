//! What the widgets look like, from the palette.
//!
//! [`crate::theme`] answers what a colour *is*; this is where the widgets that
//! would otherwise style themselves are told to use it. It matters because
//! iced's own defaults are not neutral — a `slider` with no style is drawn in
//! the *theme's* primary, and the theme is still iced's, so a seek bar comes
//! out iced's blue-violet in the middle of an app's own accent. Asking
//! [`crate::theme::of`] in our own closures cannot fix that: those closures
//! only run for widgets somebody styled. So every widget is styled here or at
//! its call site, and "it looked fine" is not evidence — the blue only shows up
//! once there is another colour beside it.
//!
//! Nothing below writes a colour down, which is the whole of what makes a dark
//! theme restate all of it.
use crate::text;
use crate::text_input;
use iced::widget::{button, container, progress_bar, rule, scrollable, slider};
use iced::{Background, Border, Color, Theme};

use crate::parts::Tint;
use crate::theme::of;

/// How much of the text colour secondary text is: the least that still reads
/// at 4.5:1 on either page (ui/README.md), 0.6 being a shade under it on white.
const DIM: f32 = 0.65;

/// Secondary text: a clock, a count, a hint.
///
/// In place of `text::secondary`, which reads iced's palette and so is a grey
/// chosen against iced's background rather than the app's.
pub fn dim(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(of(theme).background.base.text.scale_alpha(DIM)),
        ..Default::default()
    }
}

/// Text at some fraction of the text colour, the way every quiet line here is
/// drawn: a hint at 0.5, a heading at 0.55, a dimmed cell at 0.6.
pub fn faint(alpha: f32) -> impl Fn(&Theme) -> text::Style {
    move |theme| text::Style {
        color: Some(of(theme).background.base.text.scale_alpha(alpha)),
        ..Default::default()
    }
}

/// Text in the accent, the way the playing row and a heading inside a table
/// are drawn: *coloured* rather than filled, because a filled stripe is what
/// the cursor means and there can only be one of those.
pub fn accent(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(of(theme).primary.base.color),
        ..Default::default()
    }
}

/// A seek bar: the accent behind the handle, the track's own step of grey
/// ahead of it.
///
/// The two rail backgrounds are *played* and *remaining* in that order, which
/// is why the accent belongs on the first: what has gone by is the part worth
/// colouring, and an accent rail all the way across would say it was over.
pub fn seek(theme: &Theme, status: slider::Status) -> slider::Style {
    let palette = of(theme);
    let lit = match status {
        slider::Status::Active => palette.primary.base.color,
        slider::Status::Hovered => palette.primary.strong.color,
        slider::Status::Dragged => palette.primary.weak.color,
    };
    slider::Style {
        rail: slider::Rail {
            backgrounds: (lit.into(), palette.background.strong.color.into()),
            width: 4.0,
            border: Border {
                radius: 2.0.into(),
                width: 0.0,
                color: Color::TRANSPARENT,
            },
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle { radius: 7.0 },
            background: lit.into(),
            border_color: Color::TRANSPARENT,
            border_width: 0.0,
        },
        // A seek bar has no breakpoints to mark.
        breakpoint: slider::Breakpoint { color: Color::TRANSPARENT },
    }
}

/// A button somebody is meant to press: signing in, going offline.
///
/// Words in the accent, with no fill and no border: a button reads as part
/// of the page's text, and the accent alone says it is the one to press.
/// Pointing at it changes nothing about its colour; what a pointer does to
/// a control is [`crate::button`]'s underline or [`quiet`]'s faint tint.
/// Pressed, the moment the button goes down and not when it comes up, the
/// faint ground of a pressed control is behind it ([`PRESS_ALPHA`]).
pub fn action(theme: &Theme, status: button::Status) -> button::Style {
    let palette = of(theme);
    let text_color = match status {
        button::Status::Disabled => palette.background.base.text.scale_alpha(0.4),
        _ => palette.primary.base.color,
    };
    button::Style {
        background: pressed(theme, status),
        text_color,
        border: Border {
            radius: FOCUS_RADIUS.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

/// The ground behind a button held down: the text colour at
/// [`PRESS_ALPHA`], from the moment it goes down — what a press does is
/// seen while the hand is still on it, not only once it lets go.
fn pressed(theme: &Theme, status: button::Status) -> Option<Background> {
    matches!(status, button::Status::Pressed).then(|| Background::Color(of(theme).background.base.text.scale_alpha(PRESS_ALPHA)))
}

/// A button that reads as a label until it is pointed at: an icon, a word,
/// the account at the end of a top bar. No border, and pointed at, only the
/// faintest tint behind it ([`HOVER_ALPHA`]), and held down, twice that
/// ([`PRESS_ALPHA`]); its text keeps its colour.
pub fn quiet(theme: &Theme, status: button::Status) -> button::Style {
    let palette = of(theme);
    let background = match status {
        button::Status::Hovered => Some(Background::Color(palette.background.base.text.scale_alpha(HOVER_ALPHA))),
        button::Status::Pressed => pressed(theme, status),
        button::Status::Active | button::Status::Disabled => None,
    };
    let text_color = match status {
        button::Status::Disabled => palette.background.base.text.scale_alpha(0.4),
        _ => palette.background.base.text,
    };
    button::Style {
        background,
        text_color,
        border: Border {
            radius: FOCUS_RADIUS.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

/// A button that is only what it holds: no fill, no border, its text in the
/// page's colour whatever the pointer does. A link's or a name's, whose
/// pointer is the underline [`crate::button::underlined`] draws. Held
/// down, it has a pressed control's faint ground behind it, as every
/// button does ([`PRESS_ALPHA`]).
pub fn bare(theme: &Theme, status: button::Status) -> button::Style {
    let palette = of(theme);
    button::Style {
        background: pressed(theme, status),
        text_color: match status {
            button::Status::Disabled => palette.background.base.text.scale_alpha(0.4),
            _ => palette.background.base.text,
        },
        border: Border {
            radius: FOCUS_RADIUS.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

/// How much of the text colour a control pointed at is tinted with: enough
/// to say "this one", not so much that the pointer passing over a page
/// lights it up as it goes.
pub const HOVER_ALPHA: f32 = 0.05;

/// How much of it a control held down is tinted with: twice the pointer's,
/// so going down is seen as a change even under the pointer, and still
/// only a wash.
pub const PRESS_ALPHA: f32 = 0.10;

/// How round the focus's ground is ([`focus`]).
pub const FOCUS_RADIUS: f32 = 6.0;

/// The ground behind what has the focus: the cursor's row, entry or cell.
/// A low wash of the text colour, rounded, rather than the accent: the
/// focus is where the keys are, not something to press, and an accent
/// stripe drowns the row it is on. A pane without the keyboard keeps a
/// fainter one, the way a native list dims its selection when you click
/// away, so the row the page is about is still seen.
pub fn focus(theme: &Theme, focused: bool) -> Color {
    of(theme).background.base.text.scale_alpha(if focused { 0.08 } else { 0.05 })
}

/// The scrollbars. **A scrollbar is not a thing to accent.**
///
/// iced's default draws a hovered scroller in `primary.strong`, so the bar
/// goes the accent when you reach for it — and the accent means "this is
/// playing, this is ticked, press this". A bar you reached for is none of
/// those. It is the text colour instead, which is white on a dark theme and
/// near-black on a light one from the same line.
pub fn bars(theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let palette = of(theme);
    let rail = |hovered: bool| scrollable::Rail {
        background: Some(Background::Color(palette.background.weak.color)),
        border: Border {
            radius: 2.0.into(),
            ..Border::default()
        },
        scroller: scrollable::Scroller {
            background: Background::Color(if hovered {
                palette.background.base.text.scale_alpha(0.85)
            } else {
                palette.background.strongest.color
            }),
            border: Border {
                radius: 2.0.into(),
                ..Border::default()
            },
        },
    };
    let (v, h) = match status {
        scrollable::Status::Hovered {
            is_vertical_scrollbar_hovered,
            is_horizontal_scrollbar_hovered,
            ..
        } => (rail(is_vertical_scrollbar_hovered), rail(is_horizontal_scrollbar_hovered)),
        scrollable::Status::Dragged {
            is_vertical_scrollbar_dragged,
            is_horizontal_scrollbar_dragged,
            ..
        } => (rail(is_vertical_scrollbar_dragged), rail(is_horizontal_scrollbar_dragged)),
        _ => (rail(false), rail(false)),
    };
    // An `elastic` banding draws this bar itself, unmoved and shorter.
    let v = match crate::scroll::bar_hidden() {
        true => scrollable::Rail {
            background: None,
            border: Border::default(),
            scroller: scrollable::Scroller {
                background: Background::Color(Color::TRANSPARENT),
                border: Border::default(),
            },
        },
        false => v,
    };
    scrollable::Style {
        vertical_rail: v,
        horizontal_rail: h,
        ..scrollable::default(theme, status)
    }
}

/// The hairline under an entry of a list, and between sections: the text
/// colour at a fifth, so it reads as a line on either ground — iced's own
/// rule is its palette's grey, which on harken's ground all but vanishes.
pub fn hairline(theme: &Theme) -> rule::Style {
    rule::Style {
        color: of(theme).background.base.text.scale_alpha(0.2),
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }
}

/// The page's own ground.
///
/// **The one thing a style closure cannot otherwise reach.** With no `Theme`
/// of the app's own, iced paints the window from *its* palette, and every row
/// that draws no background — half of them, since the zebra is a wash over
/// whatever is behind — shows that through. So the root container paints
/// itself with this, and the app's background is what is actually on screen.
pub fn page(theme: &Theme) -> container::Style {
    let palette = of(theme);
    container::Style {
        background: Some(Background::Color(page_color(theme))),
        text_color: Some(palette.background.base.text),
        ..container::Style::default()
    }
}

/// The colour of a [`page`]'s ground: what a scroll edge over the page
/// lays down ([`crate::edge`]).
pub fn page_color(theme: &Theme) -> Color {
    of(theme).background.base.color
}

/// How far a [`surface`] steps from the page toward the text colour.
pub const SURFACE_STEP: f32 = 0.04;

/// The ground of a pane beside the page — a sidebar, a play bar: the page's
/// own colour a step toward its text, so it reads as a lighter shelf on a
/// dark theme and a greyer one on a light theme. In place of a rule between
/// the two: a line is one more thing to read, and a change of ground says
/// "another place" without being looked at.
pub fn surface(theme: &Theme) -> container::Style {
    let palette = of(theme);
    container::Style {
        background: Some(Background::Color(surface_color(theme))),
        text_color: Some(palette.background.base.text),
        ..container::Style::default()
    }
}

/// How far toward the text an [`edge`] or an [`outline`] is drawn: enough to
/// see where one thing stops, not so much that it is one more thing to read.
pub const EDGE_STEP: f32 = 0.12;

/// The line where a [`surface`] meets the page — the sidebar's right, the
/// play bar's top — so each reads as a pane of its own and not only a
/// change of ground.
pub fn edge(theme: &Theme) -> rule::Style {
    rule::Style {
        color: edge_color(theme),
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }
}

/// The colour of an [`edge`] and an [`outline`].
pub fn edge_color(theme: &Theme) -> Color {
    let palette = of(theme);
    mix(palette.background.base.color, palette.background.base.text, EDGE_STEP)
}

/// A picture's hairline, `corner` round to match it: a pale cover on a pale
/// page, or a dark one on a dark page, still has an edge.
pub fn outline(corner: f32) -> impl Fn(&Theme) -> container::Style {
    move |theme| container::Style {
        border: Border {
            color: Color { a: 0.6, ..edge_color(theme) },
            width: 1.0,
            radius: corner.into(),
        },
        ..container::Style::default()
    }
}

/// The colour a [`surface`] is painted: opaque, so what is laid over it — a
/// row's wash, a lit entry — composes the same as it does on the page.
pub fn surface_color(theme: &Theme) -> Color {
    let palette = of(theme);
    mix(palette.background.base.color, palette.background.base.text, SURFACE_STEP)
}

/// `from` moved `by` of the way to `to`.
fn mix(from: Color, to: Color, by: f32) -> Color {
    Color {
        r: from.r + (to.r - from.r) * by,
        g: from.g + (to.g - from.g) * by,
        b: from.b + (to.b - from.b) * by,
        a: 1.0,
    }
}

/// What a row of a list or a table is painted: nothing, but under the
/// cursor, the [`focus`]'s rounded ground. No zebra and no accent fill: a
/// list reads as its text, and the one row with a ground is the one the
/// keys are on. `odd` is kept for the callers that still say it.
pub fn row(theme: &Theme, on_cursor: bool, focused: bool, _odd: bool) -> container::Style {
    container::Style {
        background: on_cursor.then(|| Background::Color(focus(theme, focused))),
        border: Border {
            radius: FOCUS_RADIUS.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// What a row of a Mac's table is painted: [`row`]'s cursor, and every
/// other row a faint stripe, so a wide row is followed across.
pub fn striped(theme: &Theme, on_cursor: bool, focused: bool, odd: bool) -> container::Style {
    let ground = match (on_cursor, odd) {
        (true, _) => Some(Background::Color(focus(theme, focused))),
        (false, true) => Some(Background::Color(of(theme).background.base.text.scale_alpha(0.035))),
        (false, false) => None,
    };
    container::Style {
        background: ground,
        border: Border {
            radius: 4.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// What one entry inside a panel or a sidebar is painted: the [`focus`]'s
/// rounded ground under the cursor, fainter while its pane does not have
/// the keyboard (a menu with its submenu up), the same rule as [`row`].
pub fn entry(theme: &Theme, on_cursor: bool, focused: bool) -> container::Style {
    row(theme, on_cursor, focused, false)
}

/// The colour of text on such an entry: the page's text, lit or not, since
/// the focus's ground is a wash the text reads on either way.
pub fn entry_text(theme: &Theme, _lit: bool) -> Color {
    of(theme).background.base.text
}

/// Text on a row that may be under the cursor: the same whether it is or
/// not, the cursor being a wash under it ([`row`]). `accent` is the accent
/// colour (the one playing, a ticked name) and `dim` is the text at [`DIM`].
pub fn on_row(theme: &Theme, _on_cursor: bool, accent: bool, dim: bool) -> Color {
    let palette = of(theme);
    if accent {
        palette.primary.base.color
    } else if dim {
        palette.background.base.text.scale_alpha(DIM)
    } else {
        palette.background.base.text
    }
}

/// A [`crate::parts::pill`]: a wash of its tint, the text in the tint itself
/// (neutral's a little stronger), fully rounded at a figure's height — the
/// same on the cursor's row as off it, which a tint reads on either way.
pub fn pill(tint: Tint) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let c = tint.color(theme);
        container::Style {
            background: Some(Background::Color(c.scale_alpha(if tint == Tint::Neutral { 0.18 } else { 0.2 }))),
            text_color: Some(match tint {
                Tint::Neutral => of(theme).background.base.text.scale_alpha(0.75),
                _ => c,
            }),
            border: Border {
                radius: 10.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        }
    }
}

/// A card ([`crate::parts::group_card`], an inspector's box): the surface's
/// step, rounded.
pub fn card(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(surface_color(theme))),
        text_color: Some(of(theme).background.base.text),
        border: Border {
            radius: 8.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// The capsule a regular width's tab bar at the top sits in (an iPad
/// upright's): the surface's step, fully rounded.
pub fn capsule_group(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(surface_color(theme))),
        border: Border {
            radius: 20.0.into(),
            width: 1.0,
            color: edge_color(theme),
        },
        ..container::Style::default()
    }
}

/// One section in that bar: lit, a wash of the accent behind its title;
/// pointed at, the faintest tint.
pub fn capsule(theme: &Theme, status: button::Status, on: bool) -> button::Style {
    let p = of(theme);
    let background = match (on, status) {
        (true, _) => Some(Background::Color(p.primary.base.color.scale_alpha(0.14))),
        (false, button::Status::Hovered) => Some(Background::Color(p.background.base.text.scale_alpha(HOVER_ALPHA))),
        (false, button::Status::Pressed) => pressed(theme, status),
        _ => None,
    };
    button::Style {
        background,
        text_color: p.background.base.text,
        border: Border {
            radius: 16.0.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

/// A sheet over a compact window: the page's ground, its top corners rounded.
pub fn sheet(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(of(theme).background.base.color)),
        text_color: Some(of(theme).background.base.text),
        border: Border {
            radius: iced::border::Radius {
                top_left: 12.0,
                top_right: 12.0,
                bottom_right: 0.0,
                bottom_left: 0.0,
            },
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// A field ([`crate::parts::search_field`], [`crate::parts::name_field`]):
/// the page's ground with a hairline, the accent's when it has the keys.
pub fn field(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let p = of(theme);
    let edge = match status {
        text_input::Status::Focused { .. } => p.primary.base.color,
        text_input::Status::Hovered => p.background.base.text.scale_alpha(0.35),
        _ => p.background.base.text.scale_alpha(0.2),
    };
    text_input::Style {
        background: Background::Color(p.background.base.color),
        border: Border {
            radius: 5.0.into(),
            width: 1.0,
            color: edge,
        },
        icon: p.background.base.text.scale_alpha(0.6),
        placeholder: p.background.base.text.scale_alpha(0.4),
        value: p.background.base.text,
        selection: p.primary.weak.color,
    }
}

/// A [`crate::parts::chip`]: the chosen one's words in the accent, the rest
/// in the text colour; no fill and no border on either.
pub fn chip(chosen: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| match chosen {
        true => action(theme, status),
        false => quiet(theme, status),
    }
}

/// A [`crate::parts::progress_bar`]: the tint's colour over a faint rail.
pub fn progress(tint: Tint) -> impl Fn(&Theme) -> progress_bar::Style {
    move |theme| progress_bar::Style {
        background: Background::Color(of(theme).background.base.text.scale_alpha(0.1)),
        bar: Background::Color(tint.color(theme)),
        border: Border {
            radius: 2.0.into(),
            ..Border::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A surface is a step off the page on either side — toward the text, so
    /// lighter on dark and darker on light — and only a step.
    #[test]
    fn a_surface_is_a_step_off_the_page() {
        for (theme, lighter) in [(Theme::Dark, true), (Theme::Light, false)] {
            let page = of(&theme).background.base.color;
            let surface = surface_color(&theme);
            let step = surface.r - page.r;
            assert_eq!(step > 0.0, lighter, "{theme:?}: {page:?} to {surface:?}");
            assert!(step.abs() > 0.01 && step.abs() < 0.05, "{theme:?}: a step of {step}");
        }
    }
}
