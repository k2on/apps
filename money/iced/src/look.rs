//! money's own looks, beside `arkui::style`'s: the tint the domain's tone
//! of an amount is drawn in ([`Looks`]: `money_domain::words::Tone` in
//! arkui's [`Tint`]s, for `arkui::style::pill` and `progress`), and the box
//! what is ready to assign is drawn in. The
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

/// What an amount says about the money, the domain's (`words::Tone`).
pub use money_domain::words::Tone;

/// A tone as arkui draws it: money's one map, the phones each have the
/// same one.
pub trait Looks {
    /// Nothing is neutral, money with a job positive, short (or on a card)
    /// a caution, below nothing negative.
    fn tint(self) -> Tint;
    /// A bar in this tone: money with a job and nothing yet alike in the
    /// positive, as either is on its way.
    fn bar(self) -> Tint;
    /// Text in this tone.
    fn text(self) -> impl Fn(&Theme) -> iced::widget::text::Style;
}

impl Looks for Tone {
    fn tint(self) -> Tint {
        match self {
            Tone::Neutral => Tint::Neutral,
            Tone::Positive => Tint::Positive,
            Tone::Short | Tone::OnCard => Tint::Caution,
            Tone::Overspent => Tint::Negative,
        }
    }

    fn bar(self) -> Tint {
        match self {
            Tone::Neutral | Tone::Positive => Tint::Positive,
            t => t.tint(),
        }
    }

    fn text(self) -> impl Fn(&Theme) -> iced::widget::text::Style {
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
