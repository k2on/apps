//! money as an Ark app (ui/docs/app.md): a screen's state, the messages
//! that change it and its update, in Rust, run by `ark_app::Runtime` on
//! every platform — the phones behind their bridge (money/ffi), the iced
//! window (money/iced) and the browser (money/web).
//!
//! The first screen moved here is Accounts ([`accounts`]): the shelves of
//! accounts and what the household is worth, All Accounts, a row opening
//! its register, Add Account's form with the domain's own check, renaming,
//! closing and reopening, and Copy Balance. The rest of money is still each
//! shell's own; ui/docs/app.md says what moving it takes.

#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!();

pub mod accounts;
pub mod shell;

pub use money_domain::routes::Route;
pub use money_domain::{MoneyChange, MoneyView};
