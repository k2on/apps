//! money as an Ark app (ui/docs/app.md): a screen's state, the messages
//! that change it and its update, in Rust, run by `ark_app::Runtime` on
//! every platform — the phones behind their generated library (`money-ffi`,
//! ui/prebuild.py's `ffi`, which writes the UniFFI twins of these records
//! and messages), the desktop's window (arkui's host) and the browser
//! (arkui-web's host), each over [`window`] with its entry point
//! generated from app.toml.
//!
//! The screens moved here: Accounts ([`accounts`]) — the shelves of
//! accounts and what the household is worth, Add Account, renaming,
//! closing and Copy Balance — and Budget ([`budget`]) — the month's Ready
//! to Assign, its groups and categories, and assigning, moving money,
//! targets, new categories and groups. The rest of money is still each
//! shell's own; ui/docs/app.md says what moving it takes.

pub mod components;
pub mod host;
pub mod look;
pub mod routes;
pub mod welcome;
pub mod window;
pub mod words;

#[cfg(test)]
pub(crate) mod testing;

// money's routes, its screens and its declaration, read from `src/app`
// (ark_router's `routes!`; ui/docs/app.md, "The App's Declaration"): each
// route a file, each screen a layout — the shell the root's, the sheets
// `(modals)`', each tab a group of `(tabs)` — and at the crate's root by its
// name (`crate::budget`). Every generated target is written from it (its
// manifest); nothing of it is said in app.toml.
ark_router::routes! {
    /// Somewhere money can be: every route, one path each, on every shell —
    /// the desktop's and the browser's address bar, each phone's back
    /// stack, a deep link ([`routes`] says what money says of them).
    #[derive(Debug, Clone, ark::twin::Twin)]
    #[ark(ffi_eq)]
    pub enum Route in "src/app";
    /// money as every generated target runs it: its part of its host.
    pub type App = host::Money;
}

pub use money_core::{MoneyChange, MoneyView};

/// What money's screens ask of their host that is neither a change, a
/// route nor a device module (`ark_app::App::Request`): typed, so no shell
/// is called by a name. Each is the host's to carry out — the bank sync,
/// whose demo differs per shell; a sheet or a page still drawn by hand on
/// a platform until it moves here (ui/docs/app.md, What Is Left); a
/// message for another of money's runtimes beside it (the shell's toolbar
/// asks the Budget's to auto-assign).
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub enum MoneyRequest {
    /// Ask the banks for anything new (pulled down, Sync with Banks).
    Refresh,
    /// A sheet over the app: a transaction's form, a category chosen, an
    /// account reconciled (the sheets' runtime's).
    Sheet { msg: sheets::SheetsMsg },
    /// A message for the Budget's runtime.
    Budget { msg: budget::BudgetMsg },
    /// A message for the Accounts' runtime.
    Accounts { msg: accounts::AccountsMsg },
    /// The budget's month moved by `by` months.
    StepMonth { by: i32 },
    /// The budget on this month.
    ThisMonth,
    /// A receipt imported from a file.
    ImportReceipt,
    /// A receipt's photo (a page scanned) kept: put on the device and sent
    /// to the server with `AddPhoto`, for the transaction of this hex id
    /// when it is for one. The server reads it.
    KeepPhoto { jpeg: Vec<u8>, txn: Option<String> },
    /// A register's filter bar shown or put away.
    Filter,
    /// A category's month, named by a launch (its name or a word of it):
    /// in the inspector while it shows, pushed otherwise.
    OpenCategory { key: String },
    /// A transaction a row or a launch names (its id, or ui/docs/app.md's
    /// keys: `receipt`, `review`, a number, a payee): Transaction Details in
    /// the inspector while it shows, its form otherwise.
    OpenTransaction { key: String },
    /// What the bank imported, to look over.
    Review,
    /// Settings, on the pane of this title.
    Settings { pane: String },
    /// Signed out of the server, or out of the demo (Settings' Sign Out,
    /// asked about first).
    SignOut,
    /// Add a Device: the shared page (`ark_app::welcome::add_device`) over
    /// a code the host has the server make.
    AddDevice,
}

/// The id a route or a message carries (`Id::hex`), as a row's.
pub fn id_of<T>(hex: &str) -> Option<ark::row::Id<T>> {
    let b: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()))
        .collect::<Option<_>>()?;
    Some(ark::row::Id::new(b.try_into().ok()?))
}
