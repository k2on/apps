//! money as an Ark app (ui/docs/app.md): a screen's state, the messages
//! that change it and its update, in Rust, run by `ark_app::Runtime` on
//! every platform — the phones behind their bridge (money/ffi), the iced
//! window (money/iced) and the browser (money/web).
//!
//! The screens moved here: Accounts ([`accounts`]) — the shelves of
//! accounts and what the household is worth, Add Account, renaming,
//! closing and Copy Balance — and Budget ([`budget`]) — the month's Ready
//! to Assign, its groups and categories, and assigning, moving money,
//! targets, new categories and groups. The rest of money is still each
//! shell's own; ui/docs/app.md says what moving it takes.

#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!();

pub mod accounts;
pub mod budget;
#[cfg(feature = "ffi")]
pub mod ffi;
pub mod look;
pub mod shell;

pub use money_core::routes::Route;
pub use money_core::{MoneyChange, MoneyView};

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
        namespace: env!("CARGO_CRATE_NAME"),
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
