//! money's shell laid out, once (ui/docs/apps/money.md): drawn by iced
//! from these functions, written out as SwiftUI and Compose from their
//! recordings (`money_app::screens`, written while nix builds the
//! native projects: nix/ark-app.nix).

use ark_app::shell::*;
use ark_app::view;
use ark_app::view::*;

use super::{AskItem, BankPage, CategoryInspector, ShellMsg, ShellPage, TransactionInspector};

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
    .account(account_item(p.who, p.picture, ShellMsg::Account))
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
            settings_pane("General", "settings", settings_general(p)),
            settings_pane("Household", "users", settings_household(p)),
            settings_pane("Connected Banks", "landmark", settings_banks(p)),
            settings_pane("Appearance", "eye", settings_appearance(p)),
        ),
    )
}

/// General: who is using the budget, the server and whether it can be
/// reached, what waits to sync, Add a Device, the way out (asked about
/// first) and what the app is.
#[view]
pub fn settings_general(p: &ShellPage) -> View<ShellMsg> {
    detail_page((
        card_section(list_row(p.settings.who).subtitle(p.settings.login).tile("users")),
        if p.settings.on_server {
            card_section((
                list_row(p.settings.server).value(p.settings.link).glyph("cloud"),
                if p.settings.waits {
                    list_row("Waiting to Sync").value(p.settings.pending).glyph("clock")
                },
                if p.settings.adds_devices {
                    list_row("Add a Device").glyph("smartphone").pushes().on_tap(ShellMsg::AddDevice)
                },
            ))
            .header("Server")
        },
        if p.settings.leaves {
            card_section(list_row(p.settings.sign_out).glyph("log-out").accent().on_tap(ShellMsg::SignOut)).footer(p.settings.sign_out_note)
        },
        // What a platform adds of its own: the desktop's store logins and
        // the code Add a Device made there; nothing elsewhere.
        hosted("settings.more"),
        card_section((
            list_row("Money").subtitle("Your household's budget, on your own server").tile("wallet"),
            list_row("Version").value(p.settings.version),
        ))
        .header("About"),
        if p.settings.leaving {
            confirmation_dialog(p.settings.leave_question, p.settings.sign_out, ShellMsg::CancelSignOut).action(
                p.settings.sign_out,
                true,
                ShellMsg::ConfirmSignOut,
            )
        },
    ))
}

/// Household: who shares the budget, and Add a Device.
#[view]
pub fn settings_household(p: &ShellPage) -> View<ShellMsg> {
    detail_page(
        card_section((
            list_row(p.settings.who).subtitle(p.settings.login).tile("users"),
            if p.settings.adds_devices {
                list_row("Add a Device").glyph("smartphone").pushes().on_tap(ShellMsg::AddDevice)
            },
        ))
        .header("Shared With")
        .footer(p.settings.household_note),
    )
}

/// Connected Banks: the ask of the moment, each bank with how it is doing
/// (its page pushed when tapped), Connect a Bank and Sync Now.
#[view]
pub fn settings_banks(p: &ShellPage) -> View<ShellMsg> {
    pushes(
        detail_page((
            if let Some(a) = p.settings.ask {
                ask_card(a)
            },
            card_section((
                for b in p.settings.banks {
                    list_row(b.title)
                        .subtitle(b.status)
                        .mark(b.glyph, "checking")
                        .badge(b.waiting)
                        .pushes()
                        .on_tap(ShellMsg::OpenBank { name: b.name })
                },
                if p.settings.can_connect {
                    list_row("Connect a Bank").glyph("circle-plus").accent().on_tap(ShellMsg::ConnectBank)
                },
            ))
            .header("Banks")
            .footer(p.settings.banks_note),
            card_section(list_row("Sync Now").glyph("refresh-cw").accent().on_tap(ShellMsg::Refresh)).footer(p.settings.sync_note),
        )),
        if let Some(b) = p.settings.bank {
            bank_page(b)
        },
        p.settings.showing_bank,
        ShellMsg::CloseBank,
    )
}

/// The ask of the moment: where it stands, Open Plaid's Page while the
/// page waits, Dismiss once it is answered; what has come of it under.
#[view]
pub fn ask_card(a: &AskItem) -> View<ShellMsg> {
    card_section((
        list_row(a.title).value(a.state),
        if a.waiting {
            list_row("Open Plaid's Page").glyph("link").accent().on_tap(ShellMsg::OpenAsk)
        },
        if a.over {
            list_row("Dismiss").glyph("x").on_tap(ShellMsg::DismissAsk)
        },
    ))
    .footer(a.note)
    .into()
}

/// A connected bank's page: the ask about it, Log In Again when its login
/// ran out, what its last import failed with, when it last synced and what
/// that brought, each of its accounts and what it feeds (tapped, the
/// choice), Reconnect and Disconnect (asked about first).
#[view]
pub fn bank_page(b: &BankPage) -> View<ShellMsg> {
    toolbar(
        b.title,
        detail_page((
            if let Some(a) = b.ask {
                ask_card(a)
            },
            if b.needs_login {
                card_section(list_row("Reconnect").glyph("refresh-cw").accent().on_tap(ShellMsg::Reconnect))
                    .header("Log In Again")
                    .footer(b.login_note_shown)
            },
            if b.failing {
                card_section((list_row(b.error).glyph("triangle-alert"), list_row("Failed").value(b.failed))).header("Last Import")
            },
            card_section((
                list_row("Last Synced").value(b.synced),
                list_row("Brought In").value(b.brought),
                if b.removes {
                    list_row("Removed").value(b.removed)
                },
                if b.locks {
                    list_row("Left as Reconciled").value(b.locked)
                },
            )),
            if b.has_feeds {
                card_section(for f in b.feeds {
                    list_row(f.title)
                        .subtitle(f.about)
                        .mark(f.glyph, f.kind)
                        .value(f.feeds)
                        .on_tap(ShellMsg::ChooseFeed { plaid: f.plaid })
                })
                .header("Accounts")
                .footer(b.feeds_note)
            },
            card_section((
                if b.reconnects {
                    list_row("Reconnect").glyph("refresh-cw").accent().on_tap(ShellMsg::Reconnect)
                },
                if b.connects {
                    list_row("Disconnect").glyph("circle-minus").accent().on_tap(ShellMsg::Disconnect)
                },
            ))
            .footer(b.actions_note),
            if b.disconnecting {
                confirmation_dialog(
                    b.disconnect_question,
                    "Plaid forgets the login and the bank stops importing. Its accounts here, and what's in them, stay.",
                    ShellMsg::CancelDisconnect,
                )
                .action("Disconnect", true, ShellMsg::ConfirmDisconnect)
            },
            if let Some(c) = b.choosing {
                form_sheet(
                    "What It Feeds",
                    ShellMsg::CancelFeed,
                    (
                        form_section((
                            if c.can_open {
                                list_row(c.opens).glyph("circle-plus").accent().on_tap(ShellMsg::FeedNew)
                            },
                            for a in c.accounts {
                                list_row(a.name).mark(a.glyph, a.kind).on_tap(ShellMsg::FeedAccount { id: a.id })
                            },
                        ))
                        .header(c.title)
                        .footer(c.accounts_note),
                        form_section(if c.ignored {
                            list_row("Import It Again")
                                .glyph("eye")
                                .accent()
                                .on_tap(ShellMsg::FeedIgnored { ignored: false })
                        } else {
                            list_row("Don't Import It")
                                .glyph("eye-off")
                                .accent()
                                .on_tap(ShellMsg::FeedIgnored { ignored: true })
                        }),
                    ),
                )
            },
        )),
        bar_item("Sync", "refresh-cw", Placement::Primary, ShellMsg::Refresh),
    )
}

/// Appearance: the system's, or always light or dark.
#[view]
pub fn settings_appearance(p: &ShellPage) -> View<ShellMsg> {
    detail_page(
        card_section(option_picker(
            "Appearance",
            p.settings.appearance,
            |key| ShellMsg::AppearanceKey { key },
            (
                picker_choice("System", "smartphone", "system"),
                picker_choice("Light", "eye", "light"),
                picker_choice("Dark", "eye-off", "dark"),
            ),
        ))
        .footer(p.settings.appearance_note),
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
