//! The crate for the phones, through UniFFI: each function under its own
//! name with `format_` before it (`formatAmount` in Kotlin and Swift),
//! since an app's library puts every exported name in one namespace. Each
//! is the function of the same name above it, with owned arguments.
//!
//! An app's library links it (`money/bridge`, `harken/ffi`); the bindings
//! come with that library's, Kotlin's in `uniffi.ark_format` (this crate's
//! namespace is `ark_format`). ark-format itself knows nothing of UniFFI:
//! its `NumberStyle` crosses as a remote record declared here, which a
//! bridge that takes or returns one names with
//! `uniffi::use_remote_type!(ark_format_ffi::NumberStyle)`.

pub use ark_format::NumberStyle;

uniffi::setup_scaffolding!("ark_format");

/// [`NumberStyle`], as UniFFI writes it.
#[uniffi::remote(Record)]
pub struct NumberStyle {
    pub currency: String,
    pub symbol: String,
    pub symbol_first: bool,
    pub symbol_spaced: bool,
    pub decimal: String,
    pub grouping: String,
    pub minus: String,
    pub minus_first: bool,
}

/// [`NumberStyle::us`]: the dollar as the United States writes it.
#[uniffi::export]
pub fn number_style_us() -> NumberStyle {
    NumberStyle::us()
}

/// [`ark_format::amount`], or [`ark_format::amount_signed`] when `with_sign`.
#[uniffi::export]
pub fn format_amount(cents: i64, with_sign: bool, style: NumberStyle) -> String {
    match with_sign {
        true => ark_format::amount_signed(cents, &style),
        false => ark_format::amount(cents, &style),
    }
}

/// [`ark_format::amount_whole`].
#[uniffi::export]
pub fn format_amount_whole(cents: i64, style: NumberStyle) -> String {
    ark_format::amount_whole(cents, &style)
}

/// [`ark_format::amount_editable`].
#[uniffi::export]
pub fn format_amount_editable(cents: i64, style: NumberStyle) -> String {
    ark_format::amount_editable(cents, &style)
}

/// [`ark_format::parse_amount`].
#[uniffi::export]
pub fn parse_amount(typed: String, style: NumberStyle) -> Option<i64> {
    ark_format::parse_amount(&typed, &style)
}

/// [`ark_format::percent`].
#[uniffi::export]
pub fn format_percent(part: i64, whole: i64) -> String {
    ark_format::percent(part, whole)
}

/// [`ark_format::quantity`].
#[uniffi::export]
pub fn format_quantity(thousandths: i64) -> String {
    ark_format::quantity(thousandths)
}

/// [`ark_format::ago`].
#[uniffi::export]
pub fn format_ago(ms: i64, now_ms: i64) -> String {
    ark_format::ago(ms, now_ms)
}

/// [`ark_format::ago_alone`].
#[uniffi::export]
pub fn format_ago_alone(ms: i64, now_ms: i64) -> String {
    ark_format::ago_alone(ms, now_ms)
}

/// [`ark_format::day_heading`].
#[uniffi::export]
pub fn format_day_heading(day: String, today: String) -> String {
    ark_format::day_heading(&day, &today)
}

/// [`ark_format::day_long`].
#[uniffi::export]
pub fn format_day_long(day: String) -> String {
    ark_format::day_long(&day)
}

/// [`ark_format::month`].
#[uniffi::export]
pub fn format_month(month: String) -> String {
    ark_format::month(&month)
}

/// [`ark_format::month_alone`].
#[uniffi::export]
pub fn format_month_alone(month: String) -> String {
    ark_format::month_alone(&month)
}

/// [`ark_format::published`].
#[uniffi::export]
pub fn format_published(ms: i64, now_ms: i64, offset_minutes: i32) -> String {
    ark_format::published(ms, now_ms, offset_minutes)
}

/// [`ark_format::clock`].
#[uniffi::export]
pub fn format_clock(ms: i64) -> String {
    ark_format::clock(ms)
}

/// [`ark_format::length`].
#[uniffi::export]
pub fn format_length(ms: i64) -> String {
    ark_format::length(ms)
}

/// [`ark_format::count`].
#[uniffi::export]
pub fn format_count(n: i64, word: String) -> String {
    ark_format::count(n, &word)
}

/// [`ark_format::names`].
#[uniffi::export]
pub fn format_names(names: Vec<String>) -> String {
    ark_format::names(&names)
}

/// [`ark_format::facts`].
#[uniffi::export]
pub fn format_facts(parts: Vec<String>) -> String {
    ark_format::facts(&parts)
}

/// [`ark_format::span`].
#[uniffi::export]
pub fn format_span(born: i64, died: i64) -> String {
    ark_format::span(born, died)
}
