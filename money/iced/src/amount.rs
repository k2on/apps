//! Cents as this window writes them, and back: `ark_format`'s amounts in
//! the desktop's and the browser's number style, the dollar as the United
//! States writes it — the one place that style is chosen. The phones read
//! theirs from the platform's formatter and call the same functions.

use std::sync::LazyLock;

use arkui::format::{self, NumberStyle};

/// How this window writes money.
static STYLE: LazyLock<NumberStyle> = LazyLock::new(NumberStyle::us);

/// `$1,234.56`, `-$12.00`.
pub fn money(cents: i64) -> String {
    format::amount(cents, &STYLE)
}

/// What somebody typed as an amount, in cents; nothing typed is nothing.
pub fn parse(typed: &str) -> Option<i64> {
    format::parse_amount(typed, &STYLE)
}

/// Cents as an amount field shows them for editing: `1234.50`.
pub fn editable(cents: i64) -> String {
    format::amount_editable(cents, &STYLE)
}
