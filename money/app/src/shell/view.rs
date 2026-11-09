//! money's shell laid out, once (ui/docs/apps/money.md): drawn by iced
//! from these functions, written out as SwiftUI and Compose from their
//! recordings (`nix run .#update-ui`).

use ark_app::shell::*;
use ark_app::view;
use ark_app::view::*;

use super::{ShellMsg, ShellPage};

/// The main window: the four sections, each its toolbar over what it
/// shows, its inspector beside it.
#[view]
pub fn money_shell(p: &ShellPage) -> View<ShellMsg> {
    adaptive_tabs(
        p.section,
        |tag| ShellMsg::Select { tag },
        (
            section_tab(
                "Budget",
                "wallet",
                "budget",
                inspector(
                    "Category",
                    p.inspector,
                    |shown| ShellMsg::Inspector { shown },
                    toolbar(
                        "Budget",
                        hosted("budget"),
                        (
                            bar_item("Previous Month", "chevron-left", Placement::Principal, run("previous-month")),
                            bar_item("This Month", "check", Placement::Principal, run("this-month")),
                            bar_item("Next Month", "chevron-right", Placement::Principal, run("next-month")),
                            bar_item("Assign", "plus", Placement::Primary, run("assign")),
                            bar_item("Inspector", "layers", Placement::Primary, ShellMsg::ToggleInspector),
                        ),
                    ),
                    hosted("budget.inspector"),
                ),
            ),
            section_tab(
                "Accounts",
                "landmark",
                "accounts",
                inspector(
                    "Transaction",
                    p.inspector,
                    |shown| ShellMsg::Inspector { shown },
                    toolbar(
                        "Accounts",
                        searchable(
                            "Search",
                            p.search,
                            |text| ShellMsg::Search { text },
                            split_view(hosted("accounts"), hosted("accounts.register"), p.showing, ShellMsg::Back),
                        ),
                        (
                            bar_item("Add Account", "plus", Placement::Primary, run("new-account")),
                            bar_item("Inspector", "layers", Placement::Primary, ShellMsg::ToggleInspector),
                        ),
                    ),
                    hosted("accounts.inspector"),
                ),
            )
            .badge(p.to_review),
            section_tab(
                "Transactions",
                "credit-card",
                "transactions",
                inspector(
                    "Splits and Receipt",
                    p.inspector,
                    |shown| ShellMsg::Inspector { shown },
                    toolbar(
                        "Transactions",
                        searchable(
                            "Search Transactions",
                            p.search,
                            |text| ShellMsg::Search { text },
                            split_view(hosted("transactions"), hosted("transactions.detail"), p.showing, ShellMsg::Back),
                        ),
                        (
                            bar_item("New Transaction", "plus", Placement::Primary, run("new-transaction")),
                            bar_item("Filter", "search", Placement::Secondary, run("filter")),
                            bar_item("Inspector", "layers", Placement::Primary, ShellMsg::ToggleInspector),
                        ),
                    ),
                    hosted("transactions.inspector"),
                ),
            ),
            section_tab(
                "Insights",
                "trending-up",
                "insights",
                inspector(
                    "Trend",
                    p.inspector,
                    |shown| ShellMsg::Inspector { shown },
                    toolbar(
                        "Insights",
                        hosted("insights"),
                        (
                            bar_item("Previous Month", "chevron-left", Placement::Principal, run("previous-month")),
                            bar_item("This Month", "check", Placement::Principal, run("this-month")),
                            bar_item("Next Month", "chevron-right", Placement::Principal, run("next-month")),
                            bar_item("Inspector", "layers", Placement::Primary, ShellMsg::ToggleInspector),
                        ),
                    ),
                    hosted("insights.inspector"),
                ),
            ),
        ),
    )
}

/// The Settings scene: a window of panes on the Mac and the desktop, a
/// list of screens elsewhere.
#[view]
pub fn money_settings(_p: &ShellPage) -> View<ShellMsg> {
    settings_panes((
        settings_pane("Server and Sign-In", "badge-check", hosted("settings.server")),
        settings_pane("Bank Connections", "landmark", hosted("settings.banks")),
        settings_pane("Household", "building-2", hosted("settings.household")),
        settings_pane("Appearance", "circle", hosted("settings.appearance")),
    ))
}

/// A command by name, carried out by the platform's shell.
pub fn run(command: &str) -> ShellMsg {
    ShellMsg::Run { command: command.into() }
}

/// The menu bar's commands (ui/docs/apps/money.md, Scenes): File, View,
/// Go. On the Mac, the menu bar; on an iPad and Android, the hardware
/// keyboard's shortcuts and the app bar's menu; on the desktop, the keys
/// and the command box.
pub fn commands() -> Vec<CommandMenu<ShellMsg>> {
    vec![
        command_menu(
            MenuPlace::File,
            vec![
                command("New Transaction", "n", run("new-transaction")),
                command("New Account", "n", run("new-account")).shift(),
                command_unbound("Import Receipt…", run("import-receipt")),
            ],
        ),
        command_menu(
            MenuPlace::View,
            vec![
                command("Previous Month", "[", run("previous-month")),
                command("Next Month", "]", run("next-month")),
                command("This Month", "t", run("this-month")),
                command("Show Inspector", "i", ShellMsg::ToggleInspector).option(),
                command("Refresh", "r", run("refresh")),
            ],
        ),
        command_menu(
            MenuPlace::Go,
            vec![
                command("Budget", "1", run("go-budget")),
                command("Accounts", "2", run("go-accounts")),
                command("Transactions", "3", run("go-transactions")),
                command("Insights", "4", run("go-insights")),
            ],
        ),
    ]
}
