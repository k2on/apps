//! money in a window its host draws itself (`ark_app::window`): the
//! browser's page now, the desktop's window once it is hosted the same
//! way. What the window shows is every platform's views — the shell's, each
//! screen's, Settings' panes — and what a phone's shell draws of its own
//! for a name the shell hosts is drawn here from money's views too: the
//! month with nothing chosen in the budget's inspector, who is using the
//! budget at the sidebar's foot, a place with nothing chosen.

use ark_app::host::{Cx, Place};
use ark_app::shell::{CommandItem, CommandMenu};
use ark_app::view::{empty_state, View};
use ark_app::window::Windowed;
use money_core::{BudgetLine, MoneyChange, MoneyView, PayeeLine, Summary};

use crate::accounts::AccountsMsg;
use crate::budget::BudgetMsg;
use crate::host::{Money, ShellAsk};
use crate::insights::InsightsMsg;
use crate::register::RegisterMsg;
use crate::sheets::SheetsMsg;
use crate::shell::view::{commands, money_settings, money_shell};
use crate::shell::{tagged_account, Appearance, ShellMsg};
use crate::transactions::TransactionsMsg;
use crate::Route;

pub mod view;

/// A message for one of money's screens, or for its window.
#[derive(Clone, Debug, PartialEq)]
pub enum MoneyMsg {
    Shell(ShellMsg),
    Accounts(AccountsMsg),
    Budget(BudgetMsg),
    Transactions(TransactionsMsg),
    Insights(InsightsMsg),
    /// The register the shell shows.
    Register(RegisterMsg),
    Sheets(SheetsMsg),
    /// Settings' Done: put away.
    CloseSettings,
}

/// The month, as the budget's inspector shows it with nothing chosen.
#[derive(Clone, Debug, Default, PartialEq, ark::twin::Twin)]
pub struct MonthInspector {
    /// "November 2026".
    pub title: String,
    /// What the categories brought from last month.
    pub carried: i64,
    pub assigned: i64,
    pub activity: i64,
    pub available: i64,
    pub income: i64,
    pub overspent_last_month: i64,
}

type Seen<'a> = &'a dyn Place<MoneyView, MoneyChange>;

/// The rows of `key` as `T`, copied.
fn rows<T: Clone + 'static>(place: Seen<'_>, key: &MoneyView) -> Vec<T> {
    let mut out = vec![];
    place.read(&mut |v| out = v.rows::<T>(key).to_vec());
    out
}

impl Money {
    /// The month the budget shows, as its inspector says it with nothing
    /// chosen.
    pub fn month_inspector(&self, place: Seen<'_>) -> MonthInspector {
        let month = self.month().to_string();
        let s: Summary = rows::<Summary>(place, &MoneyView::Summary { month: month.clone() })
            .into_iter()
            .next()
            .unwrap_or_default();
        let lines: Vec<BudgetLine> = rows(place, &MoneyView::Lines { month: month.clone() });
        MonthInspector {
            title: ark_format::month(&month),
            carried: lines.iter().map(|l| l.carried).sum(),
            assigned: s.assigned,
            activity: s.activity,
            available: s.available,
            income: s.income,
            overspent_last_month: s.overspent_last_month,
        }
    }
}

impl Windowed for Money {
    type Msg = MoneyMsg;

    fn take(&mut self, msg: MoneyMsg, cx: &mut Cx<'_, Money>) {
        match msg {
            MoneyMsg::Shell(m) => self.shell.send(m, cx),
            MoneyMsg::Accounts(m) => self.accounts.send(m, cx),
            MoneyMsg::Budget(m) => self.budget.send(m, cx),
            MoneyMsg::Transactions(m) => self.transactions.send(m, cx),
            MoneyMsg::Insights(m) => self.insights.send(m, cx),
            MoneyMsg::Register(m) => {
                if let Some(r) = &mut self.register {
                    r.send(m, cx)
                }
            }
            MoneyMsg::Sheets(m) => self.sheets.send(m, cx),
            MoneyMsg::CloseSettings => self.settings = false,
        }
    }

    fn window(&self, place: Seen<'_>, now_ms: i64) -> View<MoneyMsg> {
        money_shell::view(&self.shell.page(place, now_ms)).map(MoneyMsg::Shell)
    }

    fn hosted(&self, name: &str, place: Seen<'_>, now_ms: i64) -> View<MoneyMsg> {
        match name {
            "budget" => crate::budget::view::budget::view(&self.budget.page(place, now_ms)).map(MoneyMsg::Budget),
            "budget.inspector" => view::month_inspector::view(&self.month_inspector(place)).map(MoneyMsg::Budget),
            "accounts" => crate::accounts::view::accounts::view(&self.accounts.page(place, now_ms)).map(MoneyMsg::Accounts),
            "accounts.register" => match (tagged_account(&self.shell.state().section), &self.register) {
                (Some(_), Some(r)) => crate::register::view::register::view(&r.page(place, now_ms)).map(MoneyMsg::Register),
                _ => empty_state("landmark", "No Account Selected", "Choose an account to see its transactions."),
            },
            "transactions" => crate::transactions::view::transactions::view(&self.transactions.page(place, now_ms)).map(MoneyMsg::Transactions),
            "transactions.inspector" => empty_state("panel-right", "No Transaction Selected", "Select a transaction to see its details here."),
            "insights" => crate::insights::view::insights::view(&self.insights.page(place, now_ms)).map(MoneyMsg::Insights),
            "household" => view::household_foot::view(&self.shell.page(place, now_ms).household).map(MoneyMsg::Shell),
            // What a platform adds to Settings' General: the desktop's alone.
            "settings.more" => View::Empty,
            _ => empty_state("panel-right", "Nothing Selected", "Select a row to see its details here."),
        }
    }

    fn over(&self, place: Seen<'_>, now_ms: i64) -> View<MoneyMsg> {
        crate::sheets::view::sheets::view(&self.sheets.page(place, now_ms)).map(MoneyMsg::Sheets)
    }

    fn settings(&self, place: Seen<'_>, now_ms: i64) -> Option<(View<MoneyMsg>, MoneyMsg)> {
        self.settings.then(|| {
            (
                money_settings::view(&self.shell.page(place, now_ms)).map(MoneyMsg::Shell),
                MoneyMsg::CloseSettings,
            )
        })
    }

    fn commands(&self) -> Vec<CommandMenu<MoneyMsg>> {
        commands()
            .into_iter()
            .map(|m| CommandMenu {
                place: m.place,
                items: m
                    .items
                    .into_iter()
                    .map(|c| CommandItem {
                        title: c.title,
                        shortcut: c.shortcut,
                        run: MoneyMsg::Shell(c.run),
                    })
                    .collect(),
            })
            .collect()
    }

    /// The shell's address; Settings' while it shows, its pane by the word
    /// a launch names it with (`/settings/?pane=banks`).
    fn address(&self, place: Seen<'_>, now_ms: i64) -> String {
        if self.settings {
            let pane = &self.shell.state().pane;
            let word = pane.split_whitespace().last().unwrap_or_default().to_lowercase();
            return Route::Settings.address().with("pane", word).url();
        }
        self.shell.page(place, now_ms).address
    }

    fn appearance(&self) -> Option<&'static str> {
        self.shell.state().appearance.map(|a| match a {
            Appearance::Light => "light",
            Appearance::Dark => "dark",
        })
    }

    /// The payees for a payee, and in the search field the payees and the
    /// month's categories that match.
    fn suggest(&self, field: &str, typed: &str, place: Seen<'_>) -> Vec<String> {
        let q = typed.to_lowercase();
        let payees: Vec<PayeeLine> = rows(place, &MoneyView::Payees);
        let mut out: Vec<String> = payees
            .iter()
            .filter(|p| p.name.to_lowercase().contains(&q) && p.name.to_lowercase() != q)
            .map(|p| p.name.clone())
            .collect();
        if field == "search" {
            let lines: Vec<BudgetLine> = rows(
                place,
                &MoneyView::Lines {
                    month: self.month().to_string(),
                },
            );
            out.extend(lines.iter().filter(|l| l.name.to_lowercase().contains(&q)).map(|l| l.name.clone()));
        }
        out.dedup();
        out.truncate(6);
        out
    }

    /// A bank's sign-in on Plaid's page, once the server has answered the
    /// ask with it: the window cannot follow it, so it is taken as closed
    /// once opened.
    fn opens(&self, place: Seen<'_>, now_ms: i64) -> Option<(String, MoneyMsg)> {
        let url = self.shell.page(place, now_ms).settings.plaid?;
        Some((url.clone(), MoneyMsg::Shell(ShellMsg::PageClosed { url })))
    }

    fn native(&mut self, native: ShellAsk, _cx: &mut Cx<'_, Money>) {
        match native {
            ShellAsk::Settings { .. } => self.settings = true,
            ShellAsk::Reset | ShellAsk::SignOut => self.settings = false,
            // A window's sections are its places, never a stack.
            ShellAsk::Open { .. } | ShellAsk::AddDevice => {}
        }
    }
}

#[cfg(test)]
mod tests;
