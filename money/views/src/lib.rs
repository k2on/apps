//! money's screens as the phones' generator takes them: each `#[view]`'s
//! recording (money-app's) with its names in Swift and Kotlin, and the menu
//! bar's commands. An adapter of its own (layers.toml), so money-app does
//! not depend on the generator.

/// Every screen of money's laid out in Rust, as the phones' generator takes
/// them (ui/docs/app.md, "Generated, Not Committed"): nix links this with
/// `ark_ui_gen::app::main` (app.toml's `[gen] crate`) and writes the SwiftUI and Compose into the
/// native projects it generates.
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
            Generated::view(money_app::accounts::view::accounts::record, accounts),
            Generated::view(money_app::budget::view::budget::record, budget),
            Generated::view(
                money_app::transactions::view::transactions::record,
                Names {
                    swift: "TransactionsView",
                    swift_holder: "TransactionsScreen",
                    kotlin: "TransactionsScreen",
                    kotlin_holder: "TransactionsHolder",
                    source: "money/app/src/transactions/view.rs",
                    msg: "TransactionsMsg",
                },
            ),
            Generated::view(
                money_app::insights::view::insights::record,
                Names {
                    swift: "InsightsView",
                    swift_holder: "InsightsScreen",
                    kotlin: "InsightsScreen",
                    kotlin_holder: "InsightsHolder",
                    source: "money/app/src/insights/view.rs",
                    msg: "InsightsMsg",
                },
            ),
            Generated::view(
                money_app::register::view::register::record,
                Names {
                    swift: "RegisterView",
                    swift_holder: "RegisterScreen",
                    kotlin: "RegisterScreen",
                    kotlin_holder: "RegisterHolder",
                    source: "money/app/src/register/view.rs",
                    msg: "RegisterMsg",
                },
            ),
            Generated::view(
                money_app::sheets::view::sheets::record,
                Names {
                    swift: "SheetsView",
                    swift_holder: "SheetsScreen",
                    kotlin: "SheetsScreen",
                    kotlin_holder: "SheetsHolder",
                    source: "money/app/src/sheets/view.rs",
                    msg: "SheetsMsg",
                },
            ),
            Generated::view(money_app::shell::view::money_shell::record, shell),
            // The first run's pages, every Ark app's (ark_app::welcome),
            // over the model's FirstRunApp.
            Generated::view(
                ark_app::welcome::welcome::record,
                Names {
                    swift: "WelcomeView",
                    swift_holder: "FirstRunScreen",
                    kotlin: "WelcomeScreen",
                    kotlin_holder: "FirstRunHolder",
                    source: "ui/rust/app/src/welcome.rs",
                    msg: "WelcomeMsg",
                },
            ),
            Generated::view(
                money_app::shell::view::money_settings::record,
                Names {
                    swift: "MoneySettingsView",
                    kotlin: "MoneySettings",
                    ..shell
                },
            ),
            Generated::commands(
                money_app::shell::view::commands,
                Names {
                    swift: "MoneyCommands",
                    kotlin: "moneyCommands",
                    ..shell
                },
            ),
        ],
    }
}
