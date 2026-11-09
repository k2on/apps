//! money's shell, as a standard Mac app is made (ui/docs/apps/money.md):
//! the main window's sections — Budget, Accounts, Transactions, Insights
//! — each with its toolbar, its search where it has one, a list beside
//! what it opens and an inspector; the Settings scene's panes; and the
//! menu bar's commands.
//!
//! The shell is an Ark app of its own ([`MoneyShell`]): what it is in the
//! middle of is which section shows, whether the inspector does, what is
//! typed in the search field and whether a narrow window has pushed a
//! detail. What a section shows is a hosted screen (`ark_app::shell::hosted`):
//! Accounts' list is the generated Accounts slice ([`crate::accounts`]),
//! the rest each platform's own screens until they move into Rust. A
//! command only a hosted screen can carry out (a new transaction, the
//! month forward) goes to the platform's shell as `Command::Native` with
//! the command's name.

use ark_app::{App, Command, Data};
use money_domain::routes::Route;
use money_domain::*;

pub mod view;

/// The shell, as `ark_app` runs it.
pub struct MoneyShell;

/// The sections, by tag, in the sidebar's order.
pub const SECTIONS: [&str; 4] = ["budget", "accounts", "transactions", "insights"];

/// What the shell is in the middle of.
#[derive(Clone, Debug, PartialEq)]
pub struct ShellState {
    pub section: String,
    pub inspector: bool,
    pub search: String,
    /// A narrow window has pushed the section's detail.
    pub showing: bool,
}

impl Default for ShellState {
    fn default() -> Self {
        ShellState {
            section: SECTIONS[0].into(),
            inspector: false,
            search: String::new(),
            showing: false,
        }
    }
}

/// Everything the shell can be told.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Enum))]
pub enum ShellMsg {
    /// Show the section tagged `tag` (the sidebar, a tab, ⌘1…⌘4).
    Select { tag: String },
    /// The inspector shown or put away (its close button, a sheet swiped down).
    Inspector { shown: bool },
    /// The inspector the other way (the toolbar's button, ⌥⌘I).
    ToggleInspector,
    /// What is typed in the section's search field.
    Search { text: String },
    /// A list opened what it holds: a narrow window pushes the detail.
    Opened,
    /// Back from a pushed detail.
    Back,
    /// A command by name ([`commands`]): the shell's own, or the hosted
    /// screen's, which the platform carries out.
    Run { command: String },
}

/// What the shell shows.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct ShellPage {
    /// The shown section's tag.
    pub section: String,
    pub inspector: bool,
    pub search: String,
    pub showing: bool,
    /// What waits for a look in Accounts: the transactions to review.
    pub to_review: Option<i64>,
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

    fn update(s: &mut ShellState, msg: ShellMsg, _: &Data<'_, MoneyView>) -> Vec<Command<Self>> {
        match msg {
            ShellMsg::Select { tag } => {
                select(s, tag);
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
            ShellMsg::Opened => {
                s.showing = true;
                vec![]
            }
            ShellMsg::Back => {
                s.showing = false;
                vec![]
            }
            ShellMsg::Run { command } => match command.strip_prefix("go-") {
                Some(tag) => {
                    select(s, tag.into());
                    vec![]
                }
                None => vec![Command::Native(command)],
            },
        }
    }

    fn screen(s: &ShellState, data: &Data<'_, MoneyView>) -> ShellPage {
        let accounts: &[AccountLine] = data.rows(&MoneyView::Accounts);
        let unapproved: i64 = accounts.iter().filter(|a| !a.closed).map(|a| a.unapproved).sum();
        ShellPage {
            section: s.section.clone(),
            inspector: s.inspector,
            search: s.search.clone(),
            showing: s.showing,
            to_review: (unapproved > 0).then_some(unapproved),
        }
    }
}

/// Show the section tagged `tag`, if there is one: a new section starts
/// at its list, with nothing searched.
fn select(s: &mut ShellState, tag: String) {
    if SECTIONS.contains(&tag.as_str()) && s.section != tag {
        s.section = tag;
        s.showing = false;
        s.search.clear();
    }
}

#[cfg(test)]
mod tests;
