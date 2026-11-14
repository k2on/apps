//! A type's shape for an adapter to write its twin from (ui/docs/app.md,
//! Layers): what crosses to the phones is written in the adapter, never in
//! the domain's or the app's crate.
//!
//! `#[ark(ffi)]` on a row, an input or any `#[derive(Describe)]` type, or
//! `#[derive(Twin)]` on a plain one, writes beside the type each field's
//! type by position ([`Field`]) and a hidden macro that writes the twin.
//! The adapter writes them all with [`twins!`](crate::twins):
//!
//! ```ignore
//! ark::twins!(money_core: AccountLine, BudgetLine, Tone);
//! ```
//!
//! Neither the type's crate nor this one depends on UniFFI or the bridge.

pub use ark_macros::{twins, Twin};

/// The type of field `F` of variant `V` (a struct's fields are variant 0),
/// so a twin written in another crate names it without its path.
pub trait Field<const V: usize, const F: usize> {
    type Ty;
}
