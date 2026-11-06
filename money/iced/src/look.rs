//! money's own looks, beside `arkui::style`'s: what an amount says about
//! the money ([`Tone`], drawn in arkui's [`Tint`]s by `arkui::style::pill`
//! and `progress`), and the box what is ready to assign is drawn in. The
//! fields, cards, chips and pills are arkui's (`arkui::style::field`,
//! `card`, `chip`, `pill`). Every colour asks [`arkui::theme::of`].

use arkui::parts::Tint;
use arkui::theme::of;
use iced::widget::container;
use iced::{Background, Border, Theme};

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

    /// What arkui draws it in: nothing is neutral, money with a job
    /// positive, short a caution, below nothing negative.
    pub fn tint(self) -> Tint {
        match self {
            Tone::Zero => Tint::Neutral,
            Tone::Funded => Tint::Positive,
            Tone::Short => Tint::Caution,
            Tone::Over => Tint::Negative,
        }
    }

    /// A target's progress in this tone: money with a job and nothing yet
    /// alike in the positive, as either is on its way.
    pub fn bar(self) -> Tint {
        match self {
            Tone::Zero | Tone::Funded => Tint::Positive,
            t => t.tint(),
        }
    }

    /// Text in this tone.
    pub fn text(self) -> impl Fn(&Theme) -> iced::widget::text::Style {
        self.tint().text()
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
