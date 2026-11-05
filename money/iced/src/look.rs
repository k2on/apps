//! money's own looks, beside `arkui::style`'s: the fields a form is typed
//! into, the pill an amount of money sits in, the box what is ready to
//! assign is drawn in. Every colour asks [`arkui::theme::of`].

use arkui::theme::of;
use iced::widget::{button, container, progress_bar, text_input};
use iced::{Background, Border, Color, Theme};

/// The text boxes a task focuses, by id.
pub const ASSIGNED: &str = "assigned";
pub const NEW_GROUP: &str = "new-group";
pub const NEW_CATEGORY: &str = "new-category";
/// The inspector's box for a new kind of its category.
pub const NEW_KIND: &str = "new-kind";
pub const PAYEE: &str = "payee";
pub const RENAME: &str = "rename";
/// The category dropdown's box and list.
pub const DROP_INPUT: &str = "drop-input";
pub const DROP_LIST: &str = "drop-list";
/// The register's scrollable.
pub const REGISTER: &str = "register";
/// The register's filter's text, and a filter's name as it is saved.
pub const FILTER_TEXT: &str = "filter-text";
pub const FILTER_NAME: &str = "filter-name";

/// The budget's scrollable, which the cursor keeps itself in.
pub const BUDGET: &str = "budget";
/// Reflect's scrollable, and Settings': whichever is open, for the small
/// header's way back to the top.
pub const PAGE_TOP: &str = "page-top";

/// What an amount says about the money: the colour of its pill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Nothing, neither good nor bad.
    Zero,
    /// Money with a job.
    Funded,
    /// Short of what its target asks.
    Short,
    /// Below nothing.
    Over,
}

impl Tone {
    /// A category's available amount, read against what its target asks
    /// the month to set aside.
    pub fn of(available: i64, target: i64, assigned: i64) -> Tone {
        match available {
            a if a < 0 => Tone::Over,
            _ if assigned < target => Tone::Short,
            0 => Tone::Zero,
            _ => Tone::Funded,
        }
    }

    fn color(self, theme: &Theme) -> Color {
        let p = of(theme);
        match self {
            Tone::Zero => p.background.base.text.scale_alpha(0.35),
            Tone::Funded => p.success.base.color,
            Tone::Short => p.warning.base.color,
            Tone::Over => p.danger.base.color,
        }
    }

    /// Text in this tone.
    pub fn text(self) -> impl Fn(&Theme) -> iced::widget::text::Style {
        move |theme| iced::widget::text::Style {
            color: Some(match self {
                Tone::Zero => of(theme).background.base.text.scale_alpha(0.6),
                t => t.color(theme),
            }),
        }
    }
}

/// The pill an available amount is drawn in: a wash of its tone, the text
/// in the tone itself, the same on the cursor's row as off it (the cursor
/// is a faint wash, which a tone reads on either way).
pub fn pill(tone: Tone, _lit: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let c = tone.color(theme);
        container::Style {
            background: Some(Background::Color(c.scale_alpha(if tone == Tone::Zero { 0.18 } else { 0.2 }))),
            text_color: Some(match tone {
                Tone::Zero => of(theme).background.base.text.scale_alpha(0.75),
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

/// The box across the top of the budget: green while there is money to
/// assign, red when more was assigned than there is, quiet at nothing.
pub fn ready(cents: i64) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = of(theme);
        let c = match cents {
            0 => p.background.base.text.scale_alpha(0.3),
            c if c < 0 => p.danger.base.color,
            _ => p.success.base.color,
        };
        container::Style {
            background: Some(Background::Color(c.scale_alpha(0.16))),
            text_color: Some(p.background.base.text),
            border: Border {
                radius: 8.0.into(),
                width: 1.0,
                color: c.scale_alpha(0.5),
            },
            ..container::Style::default()
        }
    }
}

/// A box of the inspector: the surface's step, rounded.
pub fn card(theme: &Theme) -> container::Style {
    let p = of(theme);
    container::Style {
        background: Some(Background::Color(arkui::style::surface_color(theme))),
        text_color: Some(p.background.base.text),
        border: Border {
            radius: 8.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// A field: the page's ground with a hairline, the accent's when it has the
/// keys.
pub fn input(theme: &Theme, status: text_input::Status) -> text_input::Style {
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

/// A button one of a few is chosen in: the chosen one's words in the
/// accent, the rest in the text colour; no fill and no border on either.
pub fn choice(chosen: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| match chosen {
        true => arkui::style::action(theme, status),
        false => arkui::style::quiet(theme, status),
    }
}

/// A target's progress: the tone's colour over a faint rail.
pub fn progress(tone: Tone) -> impl Fn(&Theme) -> progress_bar::Style {
    move |theme| progress_bar::Style {
        background: Background::Color(of(theme).background.base.text.scale_alpha(0.1)),
        bar: Background::Color(match tone {
            Tone::Zero | Tone::Funded => of(theme).success.base.color,
            t => t.color(theme),
        }),
        border: Border {
            radius: 2.0.into(),
            ..Border::default()
        },
    }
}

/// A group's heading in the budget: its words in bold say what it is, on
/// no ground of its own, as no row has one but the cursor's.
pub fn group_card(_theme: &Theme) -> container::Style {
    container::Style::default()
}
