//! money's shell, as a standard Mac app is made (ui/docs/apps/money.md):
//! the main window's sections — Budget, Accounts, Transactions, Insights
//! — and, in the sidebar alone, each account under Budget Accounts or
//! Tracking Accounts, as Mail lists its mailboxes; each with its toolbar,
//! its search where it has one and its inspector; the Settings scene's
//! panes; and the menu bar's commands.
//!
//! The shell is an Ark app of its own ([`MoneyShell`]): what it is in the
//! middle of is which section (or account) shows, whether the inspector
//! does, what is typed in the search field and which month the budget is
//! on. An account's tag is `accounts/<id>`, a child of Accounts
//! (`ark_app::shell::section_tab`): its own place in the sidebar, Accounts'
//! tab with its register pushed when compact. What a section shows is a
//! hosted screen (`ark_app::shell::hosted`): Accounts' list is the
//! generated Accounts slice ([`crate::accounts`]), the rest each
//! platform's own screens until they move into Rust. A command only a
//! hosted screen can carry out (a new transaction, the month forward)
//! goes to the platform's shell as a typed [`MoneyRequest`].

use crate::routes::Route;
use ark_app::shell::Launch;
use ark_app::{App, Command, Data};
use money_core::*;

use crate::budget::BudgetMsg;
use crate::MoneyRequest;

pub mod inspect;
pub mod view;

pub use inspect::{CategoryInspector, DetailLine, TransactionInspector};

/// The shell, as `ark_app` runs it.
pub struct MoneyShell;

/// The sections, by tag, in the sidebar's order.
pub const SECTIONS: [&str; 4] = ["budget", "accounts", "transactions", "insights"];

/// Settings' panes, by title, in order.
pub const PANES: [&str; 3] = ["General", "Household", "Connected Banks"];

/// The tag of an account's own place: a child of Accounts.
pub fn account_tag(id: &str) -> String {
    format!("accounts/{id}")
}

/// The account a tag is the place of, by its hex id.
pub fn tagged_account(tag: &str) -> Option<&str> {
    tag.strip_prefix("accounts/")
}

/// What the shell is in the middle of.
#[derive(Clone, Debug, PartialEq)]
pub struct ShellState {
    pub section: String,
    pub inspector: bool,
    pub search: String,
    /// The budget's month (`2026-11`), as the platform's screens have it;
    /// empty until one says.
    pub month: String,
    /// The pane Settings shows, by its title.
    pub pane: String,
    /// An account asked for by name before the accounts were there (a
    /// launch's `-screen account:checking`): shown as soon as one answers
    /// to it, until another place is chosen.
    pub wanted: Option<String>,
    /// The category the budget's inspector shows, by its id or (from a
    /// launch) its name; the transaction a register's inspector shows.
    pub category: Option<String>,
    pub transaction: Option<String>,
    /// The appearance a launch asked for over the system's.
    pub appearance: Option<Appearance>,
}

/// Light or dark, over the system's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Enum))]
pub enum Appearance {
    Light,
    Dark,
}

impl Default for ShellState {
    fn default() -> Self {
        ShellState {
            section: SECTIONS[0].into(),
            inspector: false,
            search: String::new(),
            month: String::new(),
            pane: PANES[0].into(),
            wanted: None,
            category: None,
            transaction: None,
            appearance: None,
        }
    }
}

/// Everything the shell can be told.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Enum))]
pub enum ShellMsg {
    /// Show the section tagged `tag` (the sidebar, a tab, ⌘1…⌘4), or an
    /// account's place (`accounts/<id>`).
    Select {
        tag: String,
    },
    /// The inspector shown or put away (its close button, a sheet swiped down).
    Inspector {
        shown: bool,
    },
    /// The inspector the other way (the toolbar's button, ⌥⌘I).
    ToggleInspector,
    /// What is typed in the section's search field.
    Search {
        text: String,
    },
    /// Back from a pushed account to Accounts.
    Back,
    /// The month the budget is on now (`2026-11`), as the platform moved it.
    Month {
        month: String,
    },
    /// Settings' pane titled `title` shown.
    Pane {
        title: String,
    },
    /// The menu bar's and the toolbars' commands ([`view::commands`]):
    /// the shell's own, or the hosted screens', which the platform's host
    /// carries out ([`MoneyRequest`]).
    NewTransaction,
    NewAccount,
    ImportReceipt,
    PreviousMonth,
    NextMonth,
    ThisMonth,
    /// Find Transactions: the register of every account.
    Find,
    /// Sync with Banks.
    Refresh,
    AutoAssign,
    NewCategory,
    NewGroup,
    ToggleHidden,
    /// A register's filter bar.
    Filter,
    /// The shown account reconciled.
    Reconcile,
    /// The window's launch arguments (`ark_app::shell::Launch`): the place
    /// it opens on, so any screen is opened with no steps.
    Launch {
        args: Vec<String>,
    },
    /// The category (its hex id) the budget's inspector shows, as the
    /// budget's selection moves; none, nothing selected.
    InspectCategory {
        id: Option<String>,
    },
    /// The transaction a register's inspector shows.
    InspectTransaction {
        id: Option<String>,
    },
    /// The inspected category given the rest of its target this month.
    AssignTarget,
    /// Money moved into the inspected, overspent category: the Budget's
    /// Move Money sheet.
    CoverOverspending,
    /// The inspected category's transactions, in Transactions.
    ShowActivity,
}

/// One account's place in the sidebar.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct AccountPlace {
    pub title: String,
    /// Its kind's glyph, by ArkUI's names.
    pub glyph: String,
    /// `accounts/<id>`.
    pub tag: String,
    /// Transactions in it waiting for a look, while there are any.
    pub badge: Option<i64>,
}

/// What the shell shows.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct ShellPage {
    /// The shown section's tag, or the shown account's.
    pub section: String,
    pub inspector: bool,
    pub search: String,
    /// An account is shown: pushed onto Accounts when compact.
    pub showing: bool,
    /// The budget's title: its month, `November 2026`.
    pub month_title: String,
    /// The shown account's name, while one is.
    pub account_title: String,
    /// The pane Settings shows.
    pub pane: String,
    /// The appearance asked for over the system's, while one is.
    pub appearance: Option<Appearance>,
    /// Category Details, while a category is inspected and the month has it.
    pub category: Option<CategoryInspector>,
    /// Transaction Details, while a transaction is inspected.
    pub transaction: Option<TransactionInspector>,
    /// What waits for a look in Transactions: the bank's imports to review.
    pub to_review: Option<i64>,
    /// The open accounts on the budget, then those only tracked.
    pub budget_accounts: Vec<AccountPlace>,
    pub tracking_accounts: Vec<AccountPlace>,
}

impl App for MoneyShell {
    type Key = MoneyView;
    type State = ShellState;
    type Msg = ShellMsg;
    type Change = MoneyChange;
    type Route = Route;
    type Request = MoneyRequest;
    type Screen = ShellPage;

    fn holds(s: &ShellState) -> Vec<MoneyView> {
        let mut v = vec![MoneyView::Accounts, MoneyView::Register { account: None }];
        if !s.month.is_empty() {
            v.push(MoneyView::Lines { month: s.month.clone() });
        }
        v
    }

    fn update(s: &mut ShellState, msg: ShellMsg, data: &Data<'_, MoneyView>) -> Vec<Command<Self>> {
        match msg {
            ShellMsg::Select { tag } => {
                select(s, tag, data);
                vec![]
            }
            ShellMsg::Inspector { shown } => {
                s.inspector = shown;
                vec![]
            }
            ShellMsg::ToggleInspector => {
                s.inspector = !s.inspector;
                vec![]
            }
            ShellMsg::Search { text } => {
                // What the hosted screens filter by: `ShellPage::search`.
                s.search = text;
                vec![]
            }
            ShellMsg::Back => {
                select(s, SECTIONS[1].into(), data);
                vec![]
            }
            ShellMsg::Month { month } => {
                s.month = month;
                vec![]
            }
            ShellMsg::Pane { title } => {
                if PANES.contains(&title.as_str()) {
                    s.pane = title;
                }
                vec![]
            }
            ShellMsg::Launch { args } => launch(s, Launch::parse(&args), data),
            ShellMsg::InspectCategory { id } => {
                s.category = id;
                vec![]
            }
            ShellMsg::InspectTransaction { id } => {
                s.transaction = id;
                vec![]
            }
            ShellMsg::AssignTarget => match inspected(s, data) {
                Some(l) if l.assignable() && l.target > l.assigned => vec![Command::Send {
                    change: MoneyChange::Assign(Assign {
                        month: s.month.clone(),
                        category_id: l.category_id,
                        amount: l.target,
                    }),
                    then: None,
                }],
                _ => vec![],
            },
            ShellMsg::CoverOverspending => match inspected(s, data) {
                Some(l) => budget(BudgetMsg::Move { id: l.category_id.hex() }),
                None => vec![],
            },
            ShellMsg::ShowActivity => match inspected(s, data).map(|l| l.name.clone()) {
                Some(name) => {
                    select(s, SECTIONS[2].into(), data);
                    s.search = name;
                    vec![]
                }
                None => vec![],
            },
            ShellMsg::Find => {
                select(s, SECTIONS[2].into(), data);
                vec![]
            }
            ShellMsg::Reconcile => match tagged_account(&s.section) {
                Some(id) => vec![Command::Request(MoneyRequest::Reconcile { account: id.into() })],
                None => vec![],
            },
            ShellMsg::NewAccount => vec![Command::Request(MoneyRequest::Accounts {
                msg: crate::accounts::AccountsMsg::Add,
            })],
            ShellMsg::AutoAssign => budget(BudgetMsg::AutoAssign),
            ShellMsg::NewCategory => budget(BudgetMsg::NewCategory { group: String::new() }),
            ShellMsg::NewGroup => budget(BudgetMsg::NewGroup),
            ShellMsg::ToggleHidden => budget(BudgetMsg::ToggleHidden),
            ShellMsg::NewTransaction => vec![Command::Request(MoneyRequest::NewTransaction)],
            ShellMsg::ImportReceipt => vec![Command::Request(MoneyRequest::ImportReceipt)],
            ShellMsg::PreviousMonth => vec![Command::Request(MoneyRequest::StepMonth { by: -1 })],
            ShellMsg::NextMonth => vec![Command::Request(MoneyRequest::StepMonth { by: 1 })],
            ShellMsg::ThisMonth => vec![Command::Request(MoneyRequest::ThisMonth)],
            ShellMsg::Refresh => vec![Command::Request(MoneyRequest::Refresh)],
            ShellMsg::Filter => vec![Command::Request(MoneyRequest::Filter)],
        }
    }

    fn screen(s: &ShellState, data: &Data<'_, MoneyView>) -> ShellPage {
        let accounts: &[AccountLine] = data.rows(&MoneyView::Accounts);
        let open: Vec<&AccountLine> = accounts.iter().filter(|a| !a.closed).collect();
        let unapproved: i64 = open.iter().map(|a| a.unapproved).sum();
        let place = |a: &&AccountLine| AccountPlace {
            title: a.name.clone(),
            glyph: crate::accounts::kind_glyph(&a.kind).into(),
            tag: account_tag(&a.id.hex()),
            badge: (a.unapproved > 0).then_some(a.unapproved),
        };
        let shown = match &s.wanted {
            Some(name) if s.section == SECTIONS[1] => named(&open, name),
            _ => tagged_account(&s.section).and_then(|id| open.iter().find(|a| a.id.hex() == id).copied()),
        };
        ShellPage {
            section: shown.map(|a| account_tag(&a.id.hex())).unwrap_or_else(|| s.section.clone()),
            inspector: s.inspector,
            search: s.search.clone(),
            showing: shown.is_some(),
            month_title: ark_format::month(&s.month),
            account_title: shown.map(|a| a.name.clone()).unwrap_or_default(),
            pane: s.pane.clone(),
            appearance: s.appearance,
            category: inspected(s, data).map(|l| inspect::category(l, &s.month, data.rows(&MoneyView::Register { account: None }))),
            transaction: s
                .transaction
                .as_deref()
                .and_then(|k| inspect::transaction_named(data.rows(&MoneyView::Register { account: None }), k))
                .map(inspect::transaction),
            to_review: (unapproved > 0).then_some(unapproved),
            budget_accounts: open.iter().filter(|a| a.on_budget).map(place).collect(),
            tracking_accounts: open.iter().filter(|a| !a.on_budget).map(place).collect(),
        }
    }
}

/// A message for the Budget's runtime, which the host hands it.
fn budget(msg: BudgetMsg) -> Vec<Command<MoneyShell>> {
    vec![Command::Request(MoneyRequest::Budget { msg })]
}

/// Show the section tagged `tag`, or the open account it is the place of:
/// a new place starts with nothing searched.
fn select(s: &mut ShellState, tag: String, data: &Data<'_, MoneyView>) {
    s.wanted = None;
    let known = match tagged_account(&tag) {
        Some(id) => data
            .rows::<AccountLine>(&MoneyView::Accounts)
            .iter()
            .any(|a| !a.closed && a.id.hex() == id),
        None => SECTIONS.contains(&tag.as_str()),
    };
    if known && s.section != tag {
        s.section = tag;
        s.search.clear();
    }
}

/// The category the budget's inspector shows: the month's line its key
/// names, while the month has one.
fn inspected<'a>(s: &ShellState, data: &Data<'a, MoneyView>) -> Option<&'a BudgetLine> {
    let key = s.category.as_deref()?;
    inspect::category_named(data.rows(&MoneyView::Lines { month: s.month.clone() }), key)
}

/// The open account `name` names: its id, its name in any case, or a
/// word of its name (`checking`, `Everyday Checking`).
fn named<'a>(open: &[&'a AccountLine], name: &str) -> Option<&'a AccountLine> {
    let n = name.to_lowercase();
    open.iter()
        .find(|a| a.id.hex() == name || a.name.to_lowercase() == n)
        .or_else(|| open.iter().find(|a| a.name.to_lowercase().split_whitespace().any(|w| w == n)))
        .or_else(|| open.iter().find(|a| a.name.to_lowercase().contains(&n)))
        .copied()
}

/// Settings' pane a launch's word names (`general`, `household`,
/// `banks`), by its title.
fn pane_named(word: &str) -> Option<&'static str> {
    let w = word.to_lowercase();
    PANES
        .iter()
        .copied()
        .find(|p| p.to_lowercase() == w || p.to_lowercase().split_whitespace().any(|x| x == w))
}

/// Where the launch arguments open: the shell's own places and the
/// appearance here, and what only the platform's hosted screens can show
/// yet (a category, a transaction, the review, a sheet, Settings) asked
/// of its host ([`MoneyRequest`]).
fn launch(s: &mut ShellState, l: Launch, data: &Data<'_, MoneyView>) -> Vec<Command<MoneyShell>> {
    let mut out = Vec::new();
    match l.appearance.as_deref() {
        Some("dark") => s.appearance = Some(Appearance::Dark),
        Some("light") => s.appearance = Some(Appearance::Light),
        _ => {}
    }
    if let Some(shown) = l.inspector {
        s.inspector = shown;
    }
    if let Some(p) = l.pane.as_deref().and_then(pane_named) {
        s.pane = p.into();
    }
    let item = l.item.clone().unwrap_or_default();
    match l.screen.as_deref() {
        Some(tag @ ("budget" | "accounts" | "transactions" | "insights")) => select(s, tag.into(), data),
        Some("account") => {
            let accounts: &[AccountLine] = data.rows(&MoneyView::Accounts);
            let open: Vec<&AccountLine> = accounts.iter().filter(|a| !a.closed).collect();
            match named(&open, &item) {
                Some(a) => select(s, account_tag(&a.id.hex()), data),
                None => {
                    select(s, SECTIONS[1].into(), data);
                    s.wanted = Some(item);
                }
            }
        }
        // In the inspector when it is shown (Category Details, Transaction
        // Details); pushed, or in its editor, by the platform otherwise.
        Some("category") => {
            select(s, SECTIONS[0].into(), data);
            s.category = Some(item.clone());
            if !s.inspector {
                out.push(Command::Request(MoneyRequest::OpenCategory { key: item }));
            }
        }
        Some("transaction") => {
            select(s, SECTIONS[2].into(), data);
            s.transaction = Some(item.clone());
            if !s.inspector {
                out.push(Command::Request(MoneyRequest::OpenTransaction { key: item }));
            }
        }
        Some("review") => {
            select(s, SECTIONS[2].into(), data);
            out.push(Command::Request(MoneyRequest::Review));
        }
        Some("add-transaction") => {
            select(s, SECTIONS[2].into(), data);
            out.push(Command::Request(MoneyRequest::NewTransaction));
        }
        Some("settings") => out.push(Command::Request(MoneyRequest::Settings { pane: s.pane.clone() })),
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests;
