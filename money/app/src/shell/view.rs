//! money's shell laid out, once (ui/docs/apps/money.md): drawn by iced
//! from these functions, written out as SwiftUI and Compose from their
//! recordings (`nix run .#update-ui`).

use ark_app::shell::*;
use ark_app::view;
use ark_app::view::*;

use super::{ShellMsg, ShellPage};

/// The main window: the four sections, then each account in the sidebar
/// under Budget Accounts or Tracking Accounts; each its toolbar — what
/// goes back or between months at its leading end, its title, its
/// actions, the inspector's toggle and the search field at its trailing
/// end — over what it shows, its inspector beside it. Who is signed in is
/// at the sidebar's foot, with Settings.
#[view]
pub fn money_shell(p: &ShellPage) -> View<ShellMsg> {
    adaptive_tabs(
        p.section,
        |tag| ShellMsg::Select { tag },
        (
            section_tab(
                "Budget",
                "mail",
                "budget",
                toolbar(
                    p.month_title,
                    inspector(
                        "Category Details",
                        p.inspector,
                        |shown| ShellMsg::Inspector { shown },
                        hosted("budget"),
                        hosted("budget.inspector"),
                    ),
                    (
                        bar_item("Previous Month", "chevron-left", Placement::Navigation, run("previous-month")),
                        bar_item("Next Month", "chevron-right", Placement::Navigation, run("next-month")),
                        bar_item("Auto-Assign", "wand-sparkles", Placement::Primary, run("auto-assign")),
                    ),
                ),
            ),
            section_tab(
                "Accounts",
                "landmark",
                "accounts",
                toolbar(
                    "Accounts",
                    pushes(
                        hosted("accounts"),
                        toolbar(
                            p.account_title,
                            searchable(
                                "Search",
                                p.search,
                                |text| ShellMsg::Search { text },
                                inspector(
                                    "Transaction Details",
                                    p.inspector,
                                    |shown| ShellMsg::Inspector { shown },
                                    hosted("accounts.register"),
                                    hosted("transactions.inspector"),
                                ),
                            ),
                            register_items(),
                        ),
                        p.showing,
                        ShellMsg::Back,
                    ),
                    (
                        bar_item("Sync", "refresh-cw", Placement::Primary, run("refresh")),
                        bar_item("Add Account", "plus", Placement::Primary, run("new-account")),
                    ),
                ),
            ),
            section_tab(
                "Transactions",
                "list",
                "transactions",
                toolbar(
                    "Transactions",
                    searchable(
                        "Payee, category, memo or amount",
                        p.search,
                        |text| ShellMsg::Search { text },
                        inspector(
                            "Transaction Details",
                            p.inspector,
                            |shown| ShellMsg::Inspector { shown },
                            hosted("transactions"),
                            hosted("transactions.inspector"),
                        ),
                    ),
                    (
                        bar_item("Filter", "list-filter", Placement::Primary, run("filter")),
                        bar_item("New Transaction", "plus", Placement::Primary, run("new-transaction")),
                    ),
                ),
            )
            .badge(p.to_review),
            section_tab(
                "Insights",
                "chart-column",
                "insights",
                toolbar(
                    "Insights",
                    hosted("insights"),
                    (
                        bar_item("Previous Month", "chevron-left", Placement::Navigation, run("previous-month")),
                        bar_item("Next Month", "chevron-right", Placement::Navigation, run("next-month")),
                    ),
                ),
            ),
            tab_group(
                "Budget Accounts",
                for a in p.budget_accounts {
                    section_tab(
                        a.title,
                        a.glyph,
                        a.tag,
                        toolbar(
                            a.title,
                            searchable(
                                "Search",
                                p.search,
                                |text| ShellMsg::Search { text },
                                inspector(
                                    "Transaction Details",
                                    p.inspector,
                                    |shown| ShellMsg::Inspector { shown },
                                    hosted("accounts.register"),
                                    hosted("transactions.inspector"),
                                ),
                            ),
                            register_items(),
                        ),
                    )
                    .badge(a.badge)
                },
            ),
            tab_group(
                "Tracking Accounts",
                for a in p.tracking_accounts {
                    section_tab(
                        a.title,
                        a.glyph,
                        a.tag,
                        toolbar(
                            a.title,
                            searchable(
                                "Search",
                                p.search,
                                |text| ShellMsg::Search { text },
                                inspector(
                                    "Transaction Details",
                                    p.inspector,
                                    |shown| ShellMsg::Inspector { shown },
                                    hosted("accounts.register"),
                                    hosted("transactions.inspector"),
                                ),
                            ),
                            register_items(),
                        ),
                    )
                    .badge(a.badge)
                },
            ),
        ),
    )
    .footer(hosted("household"))
    .into()
}

/// An account's register's toolbar: ask the bank, reconcile, add.
fn register_items() -> View<ShellMsg> {
    (
        bar_item("Sync", "refresh-cw", Placement::Primary, run("refresh")),
        bar_item("Reconcile", "badge-check", Placement::Primary, run("reconcile")),
        bar_item("New Transaction", "plus", Placement::Primary, run("new-transaction")),
    )
        .into()
}

/// The Settings scene: a window of panes on the Mac and the desktop, a
/// list of screens elsewhere.
#[view]
pub fn money_settings(p: &ShellPage) -> View<ShellMsg> {
    settings_panes(
        p.pane,
        |title| ShellMsg::Pane { title },
        (
            settings_pane("General", "settings", hosted("settings.general")),
            settings_pane("Household", "users", hosted("settings.household")),
            settings_pane("Connected Banks", "landmark", hosted("settings.banks")),
        ),
    )
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
                command("This Month", "t", run("this-month")).shift(),
                command("Show Inspector", "i", ShellMsg::ToggleInspector).option(),
                command("Find Transactions…", "f", run("find")),
                command("Sync with Banks", "r", run("refresh")),
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
