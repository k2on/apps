//! The crate for the phones, through UniFFI: each function under its own
//! name with `format_` before it (`formatAmount` in Kotlin and Swift),
//! since an app's library puts every exported name in one namespace. Each
//! is the function of the same name above it, with owned arguments.
//!
//! An app's library links it (`money/ffi`, `harken/ffi`); the bindings come
//! with that library's, Kotlin's in `uniffi.ark_format`.

use crate::NumberStyle;

/// [`NumberStyle::us`]: the dollar as the United States writes it.
#[uniffi::export]
pub fn number_style_us() -> NumberStyle {
    NumberStyle::us()
}

/// [`crate::amount`], or [`crate::amount_signed`] when `signed`.
#[uniffi::export]
pub fn format_amount(cents: i64, signed: bool, style: NumberStyle) -> String {
    match signed {
        true => crate::amount_signed(cents, &style),
        false => crate::amount(cents, &style),
    }
}

/// [`crate::amount_whole`].
#[uniffi::export]
pub fn format_amount_whole(cents: i64, style: NumberStyle) -> String {
    crate::amount_whole(cents, &style)
}

/// [`crate::amount_editable`].
#[uniffi::export]
pub fn format_amount_editable(cents: i64, style: NumberStyle) -> String {
    crate::amount_editable(cents, &style)
}

/// [`crate::parse_amount`].
#[uniffi::export]
pub fn parse_amount(typed: String, style: NumberStyle) -> Option<i64> {
    crate::parse_amount(&typed, &style)
}

/// [`crate::percent`].
#[uniffi::export]
pub fn format_percent(part: i64, whole: i64) -> String {
    crate::percent(part, whole)
}

/// [`crate::quantity`].
#[uniffi::export]
pub fn format_quantity(thousandths: i64) -> String {
    crate::quantity(thousandths)
}

/// [`crate::ago`].
#[uniffi::export]
pub fn format_ago(ms: i64, now_ms: i64) -> String {
    crate::ago(ms, now_ms)
}

/// [`crate::ago_alone`].
#[uniffi::export]
pub fn format_ago_alone(ms: i64, now_ms: i64) -> String {
    crate::ago_alone(ms, now_ms)
}

/// [`crate::day_heading`].
#[uniffi::export]
pub fn format_day_heading(day: String, today: String) -> String {
    crate::day_heading(&day, &today)
}

/// [`crate::day_long`].
#[uniffi::export]
pub fn format_day_long(day: String) -> String {
    crate::day_long(&day)
}

/// [`crate::month`].
#[uniffi::export]
pub fn format_month(month: String) -> String {
    crate::month(&month)
}

/// [`crate::month_alone`].
#[uniffi::export]
pub fn format_month_alone(month: String) -> String {
    crate::month_alone(&month)
}

/// [`crate::published`].
#[uniffi::export]
pub fn format_published(ms: i64, now_ms: i64, offset_minutes: i32) -> String {
    crate::published(ms, now_ms, offset_minutes)
}

/// [`crate::clock`].
#[uniffi::export]
pub fn format_clock(ms: i64) -> String {
    crate::clock(ms)
}

/// [`crate::length`].
#[uniffi::export]
pub fn format_length(ms: i64) -> String {
    crate::length(ms)
}

/// [`crate::count`].
#[uniffi::export]
pub fn format_count(n: i64, word: String) -> String {
    crate::count(n, &word)
}

/// [`crate::names`].
#[uniffi::export]
pub fn format_names(names: Vec<String>) -> String {
    crate::names(&names)
}

/// [`crate::facts`].
#[uniffi::export]
pub fn format_facts(parts: Vec<String>) -> String {
    crate::facts(&parts)
}

/// [`crate::span`].
#[uniffi::export]
pub fn format_span(born: i64, died: i64) -> String {
    crate::span(born, died)
}
