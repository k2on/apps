//! The Accounts page, run in Rust once for every platform (money_app,
//! ui/docs/app.md): the window is its host — the store it reads is the
//! window's, a change goes through the window's own [`App::change`], a
//! route becomes a page, and the modules are the desktop's (money/modules.toml):
//! the clipboard is ark-clipboard's `Queued`, written with iced's own task. The page is the view
//! `money_app::accounts::view::accounts` drawn by `arkui::view`.

use std::sync::Arc;

use ark_app::{Host, Inbox, ModuleCall, Modules, Ticket};
use ark_clipboard::{Clipboard, Queued};
use ark_peer::store::Store;
use iced::{Element, Task};
use money_app::accounts::{Accounts, AccountsMsg};
use money_domain::dates::{days_from_civil, parse_day};
use money_domain::{MoneyChange, MoneyView};

use crate::{App, Message, Route};

/// The window, as the runtime's host for one message.
struct Desk<'a> {
    app: &'a mut App,
    /// The desktop's modules: the clipboard; no haptics, no share sheet
    /// (a call to either is answered Unavailable).
    modules: Modules,
    clipboard: Queued,
    inbox: Inbox,
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
            inbox: Inbox::default(),
        }
    }
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
        self.inbox.ask(&self.modules, ticket, call)
    }
}

impl App {
    /// A message of the Accounts page taken.
    pub(crate) fn accounts_step(&mut self, msg: AccountsMsg) -> Task<Message> {
        let mut rt = std::mem::take(&mut self.accounts);
        let mut desk = Desk::new(self);
        rt.dispatch(msg, &mut desk);
        loop {
            let answers = desk.inbox.take();
            if answers.is_empty() {
                break;
            }
            for (t, a) in answers {
                rt.answer(t, a, &mut desk);
            }
        }
        // What the window does after: the clipboard written.
        let tasks: Vec<Task<Message>> = desk.clipboard.take().into_iter().map(iced::clipboard::write).collect();
        self.accounts = rt;
        Task::batch(tasks)
    }

    /// The Accounts page.
    pub(crate) fn view_accounts(&self) -> Element<'_, Message> {
        let page = self.accounts.screen_of(&self.peer.store, now_ms(&self.today));
        money_app::accounts::view::accounts::iced(&page).map(Message::Accounts)
    }
}
