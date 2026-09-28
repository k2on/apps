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
use iced::widget::{button, container, scrollable, slider, text};
use iced::{Background, Border, Color, Theme};

use crate::panel::ENTRY_RADIUS;
use crate::theme::of;

/// Secondary text: a clock, a count, a hint.
///
/// In place of `text::secondary`, which reads iced's palette and so is a grey
/// chosen against iced's background rather than the app's.
pub fn dim(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(of(theme).background.base.text.scale_alpha(0.6)),
    }
}

/// Text at some fraction of the text colour, the way every quiet line here is
/// drawn: a hint at 0.5, a heading at 0.55, a dimmed cell at 0.6.
pub fn faint(alpha: f32) -> impl Fn(&Theme) -> text::Style {
    move |theme| text::Style {
        color: Some(of(theme).background.base.text.scale_alpha(alpha)),
    }
}

/// Text in the accent, the way the playing row and a heading inside a table
/// are drawn: *coloured* rather than filled, because a filled stripe is what
/// the cursor means and there can only be one of those.
pub fn accent(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(of(theme).primary.base.color),
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
    }
}

/// A button somebody is meant to press: signing in, going offline.
///
/// Filled with the accent, and its text is the one colour that accent was
/// paired with — the same rule the cursor's row follows.
pub fn action(theme: &Theme, status: button::Status) -> button::Style {
    let palette = of(theme);
    let (background, text_color) = match status {
        button::Status::Active => (palette.primary.base.color, palette.primary.base.text),
        button::Status::Hovered => (palette.primary.strong.color, palette.primary.strong.text),
        button::Status::Pressed => (palette.primary.weak.color, palette.primary.weak.text),
        button::Status::Disabled => (palette.background.weak.color, palette.background.base.text.scale_alpha(0.4)),
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color,
        border: Border {
            radius: 6.0.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
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
    scrollable::Style {
        vertical_rail: v,
        horizontal_rail: h,
        ..scrollable::default(theme, status)
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
        background: Some(Background::Color(palette.background.base.color)),
        text_color: Some(palette.background.base.text),
        ..container::Style::default()
    }
}

/// What a row of a list or a table is painted.
///
/// Three states, and they are deliberately not three shades of one idea: the
/// cursor is the accent when its pane has the keyboard and a plain strong grey
/// when it does not — the way a native list dims its selection when you click
/// away — and everything else is the zebra, the page's own background
/// alternating with the faintest step up from it.
pub fn row(theme: &Theme, on_cursor: bool, focused: bool, odd: bool) -> container::Style {
    let palette = of(theme);
    let background = if on_cursor {
        Some(match focused {
            true => palette.primary.base.color,
            false => palette.background.strong.color,
        })
    } else if odd {
        // A wash of the text colour rather than `background.weak`: from a
        // base this dark iced's generated step goes a long way, which reads as
        // a striped table rather than a list you can follow across. Alpha, so
        // the same number is a lift on a dark theme and a shade on a light one.
        Some(palette.background.base.text.scale_alpha(0.045))
    } else {
        None
    };
    container::Style {
        background: background.map(Background::Color),
        ..container::Style::default()
    }
}

/// What one entry inside a panel is painted: a rounded rectangle floating
/// inside the panel's padding, rather than a stripe painted onto its edge.
///
/// `focused` is `false` on a panel whose keyboard has gone somewhere else — a
/// menu with its submenu up, or a submenu opened by pointing that the keys
/// have not followed into. It dims to `background.strong` rather than going
/// out, the same rule [`row`] has and for the same reason: the entry is still
/// the one the panel is about.
pub fn entry(theme: &Theme, on_cursor: bool, focused: bool) -> container::Style {
    let palette = of(theme);
    container::Style {
        background: on_cursor.then_some(Background::Color(match focused {
            true => palette.primary.base.color,
            false => palette.background.strong.color,
        })),
        border: Border {
            radius: ENTRY_RADIUS.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// The one colour text on such an entry is legible in: the colour the accent
/// was paired with when it is lit, the page's text when it is not.
pub fn entry_text(theme: &Theme, lit: bool) -> Color {
    let palette = of(theme);
    match lit {
        true => palette.primary.base.text,
        false => palette.background.base.text,
    }
}

/// Text on a row that may be under the cursor.
///
/// On the cursor the row is filled with the accent, so there is exactly one
/// colour text on it can be — the one that accent was paired with — and a
/// dimmed column gets the same hue at lower alpha, never a grey that was
/// chosen against the window instead. Off it, `accent` is the accent colour
/// (the one playing, a ticked name) and `dim` is the text at 0.6.
pub fn on_row(theme: &Theme, on_cursor: bool, accent: bool, dim: bool) -> Color {
    let palette = of(theme);
    if on_cursor {
        let text = palette.primary.base.text;
        match dim {
            true => text.scale_alpha(0.75),
            false => text,
        }
    } else if accent {
        palette.primary.base.color
    } else if dim {
        palette.background.base.text.scale_alpha(0.6)
    } else {
        palette.background.base.text
    }
}
