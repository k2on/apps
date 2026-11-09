//! The Accounts page, run in Rust once for every platform (money_app,
//! docs/ark-app.md): the window is its host — the store it reads is the
//! window's, a change goes through the window's own [`App::change`], a
//! route becomes a page, and the clipboard is iced's. The page is the view
//! `money_app::accounts::view::accounts` drawn by `arkui::view`.

use ark_app::{Host, ModuleAnswer, ModuleCall, Ticket};
use ark_peer::store::Store;
use iced::{Element, Task};
use money_app::accounts::{Accounts, AccountsMsg};
use money_domain::dates::{days_from_civil, parse_day};
use money_domain::{MoneyChange, MoneyView};

use crate::{App, Message, Route};

/// The window, as the runtime's host for one message.
struct Desk<'a> {
    app: &'a mut App,
    /// What the window does after: the clipboard written.
    tasks: Vec<Task<Message>>,
    answers: Vec<(Ticket, ModuleAnswer)>,
}

/// Noon on the window's day, in milliseconds: the time the page is worked out at.
pub fn now_ms(today: &str) -> i64 {
    let (y, m, d) = parse_day(today).unwrap_or((1970, 1, 1));
    days_from_civil(y, m, d) * 86_400_000 + 43_200_000
}

impl Host<Accounts> for Desk<'_> {
    fn read<R>(&self, f: impl FnOnce(&Store<MoneyView>) -> R) -> R {
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
            self.app.page = page;
        }
    }
    fn ask(&mut self, ticket: Ticket, call: ModuleCall) {
        // The desktop's modules: the clipboard is iced's; there are no
        // haptics, and the rest is not the desktop's yet.
        let answer = match call {
            ModuleCall::Copy { text } => {
                self.tasks.push(iced::clipboard::write(text));
                ModuleAnswer::Done
            }
            _ => ModuleAnswer::Unavailable,
        };
        self.answers.push((ticket, answer));
    }
}

impl App {
    /// A message of the Accounts page taken.
    pub(crate) fn accounts_step(&mut self, msg: AccountsMsg) -> Task<Message> {
        let mut rt = std::mem::take(&mut self.accounts);
        let mut desk = Desk {
            app: self,
            tasks: vec![],
            answers: vec![],
        };
        rt.dispatch(msg, &mut desk);
        while !desk.answers.is_empty() {
            for (t, a) in std::mem::take(&mut desk.answers) {
                rt.answer(t, a, &mut desk);
            }
        }
        let tasks = std::mem::take(&mut desk.tasks);
        self.accounts = rt;
        Task::batch(tasks)
    }

    /// The Accounts page.
    pub(crate) fn view_accounts(&self) -> Element<'_, Message> {
        let page = self.accounts.screen_of(&self.peer.store, now_ms(&self.today));
        money_app::accounts::view::accounts::iced(&page).map(Message::Accounts)
    }
}
