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

/// Every screen of money's laid out in Rust, as the phones' generator takes
/// them (ui/docs/app.md, "Generated, Not Committed"): nix links this with
/// `ark_ui_gen::app::main` and writes the SwiftUI and Compose into the
/// native projects it generates.
#[cfg(feature = "gen")]
pub fn screens() -> ark_ui_gen::app::App {
    use ark_ui_gen::app::{App, Generated, Names};
    let accounts = Names {
        swift: "AccountsView",
        swift_holder: "AccountsScreen",
        kotlin: "AccountsScreen",
        kotlin_holder: "AccountsHolder",
        source: "money/app/src/accounts/view.rs",
        msg: "AccountsMsg",
    };
    let budget = Names {
        swift: "BudgetView",
        swift_holder: "BudgetScreen",
        kotlin: "BudgetScreen",
        kotlin_holder: "BudgetHolder",
        source: "money/app/src/budget/view.rs",
        msg: "BudgetMsg",
    };
    // The shell's main window, Settings and the menu bar's commands.
    let shell = Names {
        swift: "MoneyShellView",
        swift_holder: "MoneyShellScreen",
        kotlin: "MoneyShell",
        kotlin_holder: "MoneyShellHolder",
        source: "money/app/src/shell/view.rs",
        msg: "ShellMsg",
    };
    App {
        // The bridge's (money/bridge), where the messages' twins are.
        namespace: "money_bridge",
        screens: vec![
            Generated::view(accounts::view::accounts::record, accounts),
            Generated::view(budget::view::budget::record, budget),
            Generated::view(shell::view::money_shell::record, shell),
            Generated::view(
                shell::view::money_settings::record,
                Names {
                    swift: "MoneySettingsView",
                    kotlin: "MoneySettings",
                    ..shell
                },
            ),
            Generated::commands(
                shell::view::commands,
                Names {
                    swift: "MoneyCommands",
                    kotlin: "moneyCommands",
                    ..shell
                },
            ),
        ],
    }
}
