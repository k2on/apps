//! The Accounts page, run in Rust once for every platform (money_app,
//! ui/docs/app.md): the window is its host — the store it reads is the
//! window's, a change goes through the window's own [`App::change`], a
//! route becomes a page, and the modules are the desktop's (money/app.toml):
//! the clipboard is ark-clipboard's `Queued`, written with iced's own task. The page is the view
//! `money_app::accounts::view::accounts` drawn by `arkui::view`.

use std::sync::Arc;

use ark_app::{Ask, Host, Inbox, Modules, Ticket};
use ark_clipboard::{Clipboard, Queued};
use iced::{Element, Task};
use money_app::accounts::{Accounts, AccountsMsg};
use money_app::budget::{Budget, BudgetMsg};
use money_core::dates::{days_from_civil, parse_day};
use money_core::{MoneyChange, MoneyView};

use crate::{App, Message, Route};

/// The window, as the runtime's host for one message.
struct Desk<'a> {
    app: &'a mut App,
    /// The desktop's modules: the clipboard; no haptics, no share sheet
    /// (an operation of either is answered Unavailable).
    modules: Modules,
    clipboard: Queued,
    accounts: Inbox<AccountsMsg>,
    budget: Inbox<BudgetMsg>,
    /// What the window does itself after the message: its own dialogs.
    natives: Vec<Message>,
}

impl Desk<'_> {
    fn new(app: &mut App) -> Desk<'_> {
        let clipboard = Queued::new();
        let mut modules = Modules::new();
        modules.register::<dyn Clipboard>(Arc::new(clipboard.clone()));
        Desk {
            app,
            modules,
            clipboard,
            accounts: Inbox::default(),
            budget: Inbox::default(),
            natives: vec![],
        }
    }
}

/// Noon on the window's day, in milliseconds: the time the page is worked out at.
pub fn now_ms(today: &str) -> i64 {
    let (y, m, d) = parse_day(today).unwrap_or((1970, 1, 1));
    days_from_civil(y, m, d) * 86_400_000 + 43_200_000
}

impl Host<Accounts> for Desk<'_> {
    fn read<R>(&self, f: impl FnOnce(&dyn ark::views::Views<MoneyView>) -> R) -> R {
        f(&self.app.peer.store)
    }
    fn now_ms(&self) -> i64 {
        now_ms(&self.app.today)
    }
    fn send(&mut self, change: MoneyChange) -> Result<(), String> {
        let r = self.app.peer.client.mutate_bytes(change.name(), change.input());
        if let Err(why) = &r {
            self.app.note = why.clone();
        }
        self.app.peer.refresh();
        r.map(|_| ())
    }
    fn open(&mut self, route: Route) {
        if let Some(page) = self.app.page_of(&route) {
            let _ = self.app.step(Message::Go(page));
        }
    }
    fn request(&mut self, r: money_app::MoneyRequest) {
        // Reconciling is the window's own dialog until it moves into Rust.
        self.natives.extend(self.app.requested(r));
    }
    fn ask(&mut self, ticket: Ticket, ask: Ask<AccountsMsg>) {
        self.accounts.ask(&self.modules, ticket, ask)
    }
}

impl Host<Budget> for Desk<'_> {
    fn read<R>(&self, f: impl FnOnce(&dyn ark::views::Views<MoneyView>) -> R) -> R {
        f(&self.app.peer.store)
    }
    fn now_ms(&self) -> i64 {
        now_ms(&self.app.today)
    }
    fn send(&mut self, change: MoneyChange) -> Result<(), String> {
        <Self as Host<Accounts>>::send(self, change)
    }
    fn open(&mut self, route: Route) {
        // A category's month: Category Details, the inspector's, beside the budget.
        if let Route::Category { id, .. } = &route {
            if let Some(c) = money_app::accounts::account_id(id).map(|a| ark::row::Id::new(a.0)) {
                self.natives.push(Message::Select(Some(c)));
            }
        }
    }
    fn request(&mut self, r: money_app::MoneyRequest) {
        self.natives.extend(self.app.requested(r));
    }
    fn ask(&mut self, ticket: Ticket, ask: Ask<BudgetMsg>) {
        self.budget.ask(&self.modules, ticket, ask)
    }
}

impl App {
    /// A message of the Budget page taken.
    pub(crate) fn budget_step(&mut self, msg: BudgetMsg) -> Task<Message> {
        let mut rt = std::mem::take(&mut self.budget);
        let mut desk = Desk::new(self);
        rt.dispatch(msg, &mut desk);
        let inbox = desk.budget.clone();
        inbox.drain(&mut rt, &mut desk);
        let tasks: Vec<Task<Message>> = std::mem::take(&mut desk.natives).into_iter().map(Task::done).collect();
        self.budget = rt;
        Task::batch(tasks)
    }

    /// The Budget page told the window's month, when it moved.
    pub(crate) fn budget_month(&mut self) -> Task<Message> {
        match self.budget.state().month == self.peer.month {
            true => Task::none(),
            false => self.budget_step(BudgetMsg::Month {
                month: self.peer.month.clone(),
            }),
        }
    }

    /// The Budget page.
    pub(crate) fn view_budget(&self) -> Element<'_, Message> {
        let page = self.budget.screen_of(&self.peer.store, now_ms(&self.today));
        arkui::view::element(money_app::budget::view::budget::view(&page)).map(Message::Budget)
    }

    /// A message of the Accounts page taken.
    pub(crate) fn accounts_step(&mut self, msg: AccountsMsg) -> Task<Message> {
        let mut rt = std::mem::take(&mut self.accounts);
        let mut desk = Desk::new(self);
        rt.dispatch(msg, &mut desk);
        let inbox = desk.accounts.clone();
        inbox.drain(&mut rt, &mut desk);
        // What the window does after: the clipboard written.
        let mut tasks: Vec<Task<Message>> = desk.clipboard.take().into_iter().map(iced::clipboard::write).collect();
        tasks.extend(std::mem::take(&mut desk.natives).into_iter().map(Task::done));
        self.accounts = rt;
        Task::batch(tasks)
    }

    /// The Accounts page.
    pub(crate) fn view_accounts(&self) -> Element<'_, Message> {
        let page = self.accounts.screen_of(&self.peer.store, now_ms(&self.today));
        arkui::view::element(money_app::accounts::view::accounts::view(&page)).map(Message::Accounts)
    }
}
