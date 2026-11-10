//! money's shell laid out, once (ui/docs/apps/money.md): drawn by iced
//! from these functions, written out as SwiftUI and Compose from their
//! recordings (`money_app::screens`, written while nix builds the
//! native projects: nix/ark-app.nix).

use ark_app::shell::*;
use ark_app::view;
use ark_app::view::*;

use super::{CategoryInspector, ShellMsg, ShellPage, TransactionInspector};

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
                        if let Some(c) = p.category {
                            category_inspector(c)
                        } else {
                            hosted("budget.inspector")
                        },
                    ),
                    (
                        bar_item("Previous Month", "chevron-left", Placement::Navigation, ShellMsg::PreviousMonth),
                        bar_item("Next Month", "chevron-right", Placement::Navigation, ShellMsg::NextMonth),
                        bar_item("Auto-Assign", "wand-sparkles", Placement::Primary, ShellMsg::AutoAssign),
                        bar_item("New Transaction", "plus", Placement::Primary, ShellMsg::NewTransaction),
                        bar_item("New Category", "circle-plus", Placement::Secondary, ShellMsg::NewCategory),
                        bar_item("New Group", "folder", Placement::Secondary, ShellMsg::NewGroup),
                        bar_item("Show Hidden Categories", "eye", Placement::Secondary, ShellMsg::ToggleHidden),
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
                                    if let Some(t) = p.transaction {
                                        transaction_inspector(t)
                                    } else {
                                        hosted("transactions.inspector")
                                    },
                                ),
                            ),
                            register_items(),
                        ),
                        p.showing,
                        ShellMsg::Back,
                    ),
                    (
                        bar_item("Sync", "refresh-cw", Placement::Primary, ShellMsg::Refresh),
                        bar_item("Add Account", "plus", Placement::Primary, ShellMsg::NewAccount),
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
                            if let Some(t) = p.transaction {
                                transaction_inspector(t)
                            } else {
                                hosted("transactions.inspector")
                            },
                        ),
                    ),
                    (
                        bar_item("Filter", "list-filter", Placement::Primary, ShellMsg::Filter),
                        bar_item("New Transaction", "plus", Placement::Primary, ShellMsg::NewTransaction),
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
                        bar_item("Previous Month", "chevron-left", Placement::Navigation, ShellMsg::PreviousMonth),
                        bar_item("Next Month", "chevron-right", Placement::Navigation, ShellMsg::NextMonth),
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
                                    if let Some(t) = p.transaction {
                                        transaction_inspector(t)
                                    } else {
                                        hosted("transactions.inspector")
                                    },
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
                                    if let Some(t) = p.transaction {
                                        transaction_inspector(t)
                                    } else {
                                        hosted("transactions.inspector")
                                    },
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

/// Category Details (the reference's): a card of what it has, what needs
/// attention and its group, with Assign and Cover Overspending; the month's
/// figures; its goal and how far toward it; the month's transactions in it
/// and the way to all of them.
#[view]
pub fn category_inspector(c: &CategoryInspector) -> View<ShellMsg> {
    detail_page((
        header_card(c.name, c.figure, c.tint).badge().line(c.attention).caption(c.group).actions((
            if c.can_assign {
                list_row(c.assign).glyph("target").accent().on_tap(ShellMsg::AssignTarget)
            },
            if c.cover {
                list_row("Cover Overspending From")
                    .glyph("arrow-left-right")
                    .accent()
                    .on_tap(ShellMsg::CoverOverspending)
            },
        )),
        card_section((
            list_row("Carried Over").amount(c.carried).quiet(),
            list_row("Assigned").amount(c.assigned),
            list_row("Activity").amount(c.activity).quiet(),
            list_row("Available").amount(c.available),
        ))
        .header(c.month),
        card_section((
            list_row("Goal").value(c.goal),
            if c.has_goal {
                (
                    list_row("Target").amount(c.target),
                    goal_progress(c.progress, c.progress_tint, c.progress_note),
                )
            },
        ))
        .header("Goal"),
        card_section((
            for t in c.activity_lines {
                list_row(t.title).subtitle(t.subtitle).amount(t.cents)
            },
            if c.quiet_month {
                list_row("No transactions this month.").quiet()
            },
            list_row("Show All in Transactions").accent().on_tap(ShellMsg::ShowActivity),
        ))
        .header("Activity"),
    ))
}

/// Transaction Details: who and how much, in a card; where, when, its
/// status and its memo.
#[view]
pub fn transaction_inspector(t: &TransactionInspector) -> View<ShellMsg> {
    detail_page((
        header_card(t.payee, t.figure, t.tint).caption(t.category),
        card_section((
            list_row("Account").value(t.account),
            list_row("Date").value(t.date),
            list_row("Status").value(t.status),
            if t.has_memo {
                list_row("Memo").value(t.memo)
            },
        )),
    ))
}

/// An account's register's toolbar: ask the bank, reconcile, add.
fn register_items() -> View<ShellMsg> {
    (
        bar_item("Sync", "refresh-cw", Placement::Primary, ShellMsg::Refresh),
        bar_item("Reconcile", "badge-check", Placement::Primary, ShellMsg::Reconcile),
        bar_item("New Transaction", "plus", Placement::Primary, ShellMsg::NewTransaction),
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

/// A section shown, by its tag.
pub fn go(tag: &str) -> ShellMsg {
    ShellMsg::Select { tag: tag.into() }
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
                command("New Transaction", "n", ShellMsg::NewTransaction),
                command("New Account", "n", ShellMsg::NewAccount).shift(),
                command_unbound("Import Receipt…", ShellMsg::ImportReceipt),
            ],
        ),
        command_menu(
            MenuPlace::View,
            vec![
                command("Previous Month", "[", ShellMsg::PreviousMonth),
                command("Next Month", "]", ShellMsg::NextMonth),
                command("This Month", "t", ShellMsg::ThisMonth).shift(),
                command("Show Inspector", "i", ShellMsg::ToggleInspector).option(),
                command("Find Transactions…", "f", ShellMsg::Find),
                command("Sync with Banks", "r", ShellMsg::Refresh),
            ],
        ),
        command_menu(
            MenuPlace::Go,
            vec![
                command("Budget", "1", go("budget")),
                command("Accounts", "2", go("accounts")),
                command("Transactions", "3", go("transactions")),
                command("Insights", "4", go("insights")),
            ],
        ),
    ]
}
