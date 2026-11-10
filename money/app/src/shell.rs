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
//! goes to the platform's shell as `Command::Native` with the command's
//! name.

use ark_app::{App, Command, Data};
use money_domain::routes::Route;
use money_domain::*;

pub mod view;

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
}

impl Default for ShellState {
    fn default() -> Self {
        ShellState {
            section: SECTIONS[0].into(),
            inspector: false,
            search: String::new(),
            month: String::new(),
            pane: PANES[0].into(),
        }
    }
}

/// Everything the shell can be told.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Enum))]
pub enum ShellMsg {
    /// Show the section tagged `tag` (the sidebar, a tab, ⌘1…⌘4), or an
    /// account's place (`accounts/<id>`).
    Select { tag: String },
    /// The inspector shown or put away (its close button, a sheet swiped down).
    Inspector { shown: bool },
    /// The inspector the other way (the toolbar's button, ⌥⌘I).
    ToggleInspector,
    /// What is typed in the section's search field.
    Search { text: String },
    /// Back from a pushed account to Accounts.
    Back,
    /// The month the budget is on now (`2026-11`), as the platform moved it.
    Month { month: String },
    /// Settings' pane titled `title` shown.
    Pane { title: String },
    /// A command by name ([`view::commands`]): the shell's own, or the
    /// hosted screen's, which the platform carries out.
    Run { command: String },
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
    type Screen = ShellPage;

    fn holds(_: &ShellState) -> Vec<MoneyView> {
        vec![MoneyView::Accounts]
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
                s.search = text;
                vec![Command::Native(format!("search {}", s.search))]
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
            ShellMsg::Run { command } => match command.strip_prefix("go-") {
                Some(tag) => {
                    select(s, tag.into(), data);
                    vec![]
                }
                None => vec![Command::Native(command)],
            },
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
        let shown = tagged_account(&s.section).and_then(|id| open.iter().find(|a| a.id.hex() == id));
        ShellPage {
            section: s.section.clone(),
            inspector: s.inspector,
            search: s.search.clone(),
            showing: shown.is_some(),
            month_title: ark_format::month(&s.month),
            account_title: shown.map(|a| a.name.clone()).unwrap_or_default(),
            pane: s.pane.clone(),
            to_review: (unapproved > 0).then_some(unapproved),
            budget_accounts: open.iter().filter(|a| a.on_budget).map(place).collect(),
            tracking_accounts: open.iter().filter(|a| !a.on_budget).map(place).collect(),
        }
    }
}

/// Show the section tagged `tag`, or the open account it is the place of:
/// a new place starts with nothing searched.
fn select(s: &mut ShellState, tag: String, data: &Data<'_, MoneyView>) {
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

#[cfg(test)]
mod tests;
