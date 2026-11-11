//! Numbers, days and lengths as people read them: one crate, so that every
//! client — the phones through UniFFI, the desktop and the browser through
//! arkui — prints the same text for the same thing.
//!
//! Everything here is a pure function. Nothing reads the clock or the
//! system's locale: the time is an argument (`now_ms`, a UTC offset), and
//! so is how a place writes money ([`NumberStyle`]), which each platform
//! reads once from its own formatter at startup — `java.text` on Android,
//! `NumberFormatter` on the iPhone, [`NumberStyle::us`] on the desktop and
//! the web — and hands to every call.
//!
//! - **Amounts** ([`amount`], [`amount_signed`], [`amount_whole`],
//!   [`amount_editable`], [`parse_amount`]), a receipt's [`quantity`] and a
//!   share as a [`percent`].
//! - **Days** ([`ago`], [`day_heading`], [`day_long`], [`month`],
//!   [`month_alone`], [`published`]). The words are English, as every other
//!   word of the apps is.
//! - **Lengths** ([`clock`], [`clock_secs`], [`length`]).
//! - **Words** ([`plural`], [`count`], [`names`], [`facts`], [`span`]).
//!
//! The phones call the same functions prefixed `format_` (`formatAmount`,
//! `formatLength` …; ark-format-ffi, an adapter), since their names land in
//! one namespace with everything else of the app's library.

mod days;
mod lengths;
mod numbers;
mod words;

pub use days::{ago, ago_alone, day_heading, day_long, month, month_alone, published};
pub use lengths::{clock, clock_secs, length};
pub use numbers::{amount, amount_editable, amount_or_blank, amount_signed, amount_whole, parse_amount, percent, quantity, NumberStyle};
pub use words::{count, facts, names, plural, span};
