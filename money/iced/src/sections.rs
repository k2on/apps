//! The window as money's shell lays it out (money/app/src/shell,
//! ui/docs/apps/money.md): the sections Budget, Accounts, Transactions and
//! Insights in the sidebar (tabs at the foot of a narrow window), each its
//! toolbar, its search and its inspector, Accounts' list beside the
//! register it opens; Settings as the shell's panes; the menu bar's
//! commands as the keys. The shell is `money_app::shell::MoneyShell`, run
//! here as on the phones, drawn by `arkui::view::shell`; what each section
//! shows is this window's own page, hosted by name ([`App::hosted`]).
//!
//! The page is the window's one truth: a section chosen goes to its page,
//! and a page gone to (a key, a link, the back button) is its section's
//! (`sync_shell`).

use ark_app::{Host, ModuleCall, Ticket};
use ark_peer::store::Store;
use arkui::shell::{Frame, Pane};
use iced::keyboard::{Key, Modifiers};
use iced::widget::{column, container, text};
use iced::{Element, Length, Task};
use money_app::shell::view::{commands, money_settings, money_shell};
use money_app::shell::{MoneyShell, ShellMsg, ShellPage, SECTIONS};
use money_domain::{MoneyChange, MoneyView};

use crate::peer::Ledger;
use crate::{App, Message, Page, Route};

/// The window, as the shell's host for one message.
struct Desk<'a> {
    app: &'a mut App,
    /// What the shell asked of the window's own screens, by name.
    natives: Vec<String>,
}

impl Host<MoneyShell> for Desk<'_> {
    fn read<R>(&self, f: impl FnOnce(&Store<MoneyView>) -> R) -> R {
        f(&self.app.peer.store)
    }
    fn now_ms(&self) -> i64 {
        crate::overview::now_ms(&self.app.today)
    }
    fn send(&mut self, _: MoneyChange) -> Result<(), String> {
        Ok(())
    }
    fn open(&mut self, route: Route) {
        if let Some(page) = self.app.page_of(&route) {
            self.app.page = page;
        }
    }
    fn ask(&mut self, _: Ticket, _: ModuleCall) {}
    fn native(&mut self, name: &str) {
        self.natives.push(name.into());
    }
}

/// The section a page is in, a register changed from `base` with it; none
/// for Settings, which is the shell's other scene.
pub fn section_of(page: Page, base: Ledger) -> Option<&'static str> {
    match (page, base) {
        (Page::Budget, _) => Some("budget"),
        (Page::Accounts | Page::Ledger(Ledger::Account(_)), _) => Some("accounts"),
        (Page::Ledger(Ledger::Adhoc), Ledger::Account(_)) => Some("accounts"),
        (Page::Ledger(_), _) => Some("transactions"),
        (Page::Reflect, _) => Some("insights"),
        (Page::Settings, _) => None,
    }
}

/// The page a section opens on.
pub fn page_for(tag: &str) -> Option<Page> {
    match tag {
        "budget" => Some(Page::Budget),
        "accounts" => Some(Page::Accounts),
        "transactions" => Some(Page::Ledger(Ledger::All)),
        "insights" => Some(Page::Reflect),
        _ => None,
    }
}

impl App {
    /// A message of the shell taken; a section chosen is its page gone to,
    /// a command only this window's screens can carry out is theirs.
    pub(crate) fn shell_step(&mut self, msg: ShellMsg) -> Task<Message> {
        let go = match &msg {
            ShellMsg::Select { tag } => page_for(tag),
            ShellMsg::Back => Some(Page::Accounts),
            ShellMsg::Run { command } => command.strip_prefix("go-").and_then(page_for),
            _ => None,
        };
        let mut rt = std::mem::take(&mut self.shell);
        let mut desk = Desk { app: self, natives: vec![] };
        rt.dispatch(msg, &mut desk);
        let natives = std::mem::take(&mut desk.natives);
        self.shell = rt;
        let mut tasks = vec![];
        if let Some(page) = go.filter(|p| *p != self.page) {
            tasks.push(self.update(Message::Go(page)));
        }
        for name in natives {
            let m = match name.as_str() {
                "new-transaction" => Some(Message::AddTxn),
                "new-account" => Some(Message::Accounts(money_app::accounts::AccountsMsg::Add)),
                "previous-month" => Some(Message::Month(-1)),
                "next-month" => Some(Message::Month(1)),
                "this-month" => Some(Message::ThisMonth),
                "refresh" => Some(Message::Sync),
                "filter" => Some(Message::Go(Page::Ledger(Ledger::All))),
                _ => None,
            };
            if let Some(m) = m {
                tasks.push(self.update(m));
            }
        }
        Task::batch(tasks)
    }

    /// The shell's section, the page's: what a key, a link or the back
    /// button went to is lit in the sidebar.
    pub(crate) fn sync_shell(&mut self) {
        let Some(tag) = section_of(self.page, self.peer.base) else { return };
        let mut rt = std::mem::take(&mut self.shell);
        if rt.state().section != tag {
            rt.dispatch(ShellMsg::Select { tag: tag.into() }, &mut Desk { app: self, natives: vec![] });
        }
        self.shell = rt;
    }

    /// What the shell shows now.
    pub fn shell_page(&self) -> ShellPage {
        let mut page = self.shell.screen_of(&self.peer.store, crate::overview::now_ms(&self.today));
        // A register opened from Accounts is that section's detail.
        page.showing = matches!(self.page, Page::Ledger(Ledger::Account(_)));
        page
    }

    /// The window as the shell draws it: its width and which pane has the keys.
    pub fn frame(&self) -> Frame {
        Frame {
            width: self.window.width,
            focus: self.keyboard.focus,
        }
    }

    /// The panes on screen, for the keys to walk.
    pub fn panes(&self) -> Vec<Pane> {
        let page = self.shell_page();
        self.frame().panes(page.showing, page.inspector)
    }

    /// The sidebar's sections: each its glyph, its title, no heading, its page.
    pub fn places(&self) -> Vec<(&'static [u8], String, Option<&'static str>, Page)> {
        use arkui::glyphs;
        vec![
            (glyphs::HOME, "Budget".into(), None, Page::Budget),
            (glyphs::BANK, "Accounts".into(), None, Page::Accounts),
            (glyphs::PLAYLIST, "Transactions".into(), None, Page::Ledger(Ledger::All)),
            (glyphs::LIBRARY, "Insights".into(), None, Page::Reflect),
        ]
    }

    /// Where the sidebar's cursor is: the section the page is in.
    pub fn sidebar_at(&self) -> usize {
        let tag = section_of(self.page, self.peer.base).unwrap_or(self.shell.state().section.as_str());
        SECTIONS.iter().position(|s| *s == tag).unwrap_or(0)
    }

    /// The command `key` with `mods` is the shortcut of (Ctrl, or ⌘ on a Mac).
    pub(crate) fn shortcut(&self, key: &Key, mods: Modifiers) -> Option<ShellMsg> {
        if !(mods.control() || mods.logo()) {
            return None;
        }
        let Key::Character(c) = key else { return None };
        ark_app::shell::command_for(&commands(), c.as_str(), mods.shift(), mods.alt()).map(|c| c.run.clone())
    }

    /// Where a hosted page starts in the window: past the sidebar and, for
    /// the register beside Accounts' list, the list; under the toolbar.
    pub fn page_origin(&self) -> iced::Point {
        use arkui::layout_tokens::{CONTENT_WIDTH, SIDEBAR_WIDTH, TOOLBAR_HEIGHT};
        use arkui::shell::SizeClass;
        let class = self.frame().class();
        let mut x = match class {
            SizeClass::Compact => 0.0,
            _ => SIDEBAR_WIDTH,
        };
        if class == SizeClass::Expanded && matches!(self.page, Page::Ledger(Ledger::Account(_))) {
            x += CONTENT_WIDTH + 1.0;
        }
        iced::Point::new(x, TOOLBAR_HEIGHT + 1.0)
    }

    /// The window's content: the shell's sections, or Settings' panes.
    pub fn view_shell(&self) -> Element<'_, Message> {
        let page = self.shell_page();
        let view = match self.page {
            Page::Settings => money_settings::view(&page),
            _ => money_shell::view(&page),
        };
        arkui::view::shell(view.map(Message::Shell), self.frame(), &mut |name| self.hosted(name))
    }

    /// A screen the shell hosts: this window's own pages.
    fn hosted(&self, name: &'static str) -> Element<'_, Message> {
        match (name, self.page) {
            ("budget", _) => self.view_budget(),
            ("accounts", _) => self.view_accounts(),
            ("accounts.register", Page::Ledger(l)) => self.view_ledger(l),
            ("transactions", Page::Ledger(l)) => self.view_ledger(l),
            ("transactions", _) => self.view_ledger(Ledger::All),
            ("insights", _) => self.view_reflect(),
            ("settings.server", _) => self.view_settings(),
            ("accounts.register", _) => quiet("Select an Account"),
            ("settings.banks", _) => quiet("Bank Connections are in Server and Sign-In, under Banks."),
            ("settings.household", _) => quiet("Who shares this budget, and their roles."),
            ("settings.appearance", _) => quiet("Follows the system's light and dark."),
            _ => quiet("Nothing Selected"),
        }
    }
}

/// A word where there is nothing to show yet, quiet and centred.
fn quiet<'a>(words: &'static str) -> Element<'a, Message> {
    container(column![text(words).size(13).style(arkui::style::dim)])
        .center(Length::Fill)
        .into()
}
