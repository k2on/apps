//! money as an Ark app (ui/docs/app.md): a screen's state, the messages
//! that change it and its update, in Rust, run by `ark_app::Runtime` on
//! every platform — the phones behind their bridge (money/bridge, which writes the
//! UniFFI twins of these records and messages), the iced
//! window (money/iced) and the browser (money/web).
//!
//! The screens moved here: Accounts ([`accounts`]) — the shelves of
//! accounts and what the household is worth, Add Account, renaming,
//! closing and Copy Balance — and Budget ([`budget`]) — the month's Ready
//! to Assign, its groups and categories, and assigning, moving money,
//! targets, new categories and groups. The rest of money is still each
//! shell's own; ui/docs/app.md says what moving it takes.

pub mod accounts;
pub mod budget;
pub mod look;
pub mod routes;
pub mod shell;
pub mod words;

pub use money_core::{MoneyChange, MoneyView};
pub use routes::Route;

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
    /// The reconcile sheet for the account of this hex id.
    Reconcile { account: String },
    /// A message for the Budget's runtime.
    Budget { msg: budget::BudgetMsg },
    /// A message for the Accounts' runtime.
    Accounts { msg: accounts::AccountsMsg },
    /// The budget's month moved by `by` months.
    StepMonth { by: i32 },
    /// The budget on this month.
    ThisMonth,
    /// The transaction form, for a new one.
    NewTransaction,
    /// A receipt imported from a file.
    ImportReceipt,
    /// A register's filter bar shown or put away.
    Filter,
    /// A category's month, named by a launch (its name or a word of it):
    /// in the inspector while it shows, pushed otherwise.
    OpenCategory { key: String },
    /// A transaction a launch names (ui/docs/app.md's keys: `receipt`,
    /// `review`, a number, a payee): its details, or its form.
    OpenTransaction { key: String },
    /// What the bank imported, to look over.
    Review,
    /// Settings, on the pane of this title.
    Settings { pane: String },
}

