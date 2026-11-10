//! The window as money's shell lays it out (money/app/src/shell,
//! ui/docs/apps/money.md): the sections Budget, Accounts, Transactions and
//! Insights in the sidebar with every account under them (a tab bar at
//! the top of a narrower window, at the foot of a compact one), each its
//! toolbar, its search and its inspector; Settings as the shell's panes;
//! the menu bar's commands as the keys. The shell is
//! `money_app::shell::MoneyShell`, run here as on the phones, drawn by
//! `arkui::view::shell`; what each section shows is this window's own
//! page, hosted by name ([`App::hosted`]).
//!
//! The page is the window's one truth: a section chosen goes to its page,
//! and a page gone to (a key, a link, the back button) is its section's
//! (`sync_shell`).

use ark_app::{Host, ModuleCall, Ticket};
use ark_peer::store::Store;
use arkui::shell::{Frame, Pane, SizeClass};
use arkui::text::text;
use iced::keyboard::{Key, Modifiers};
use iced::widget::{column, container, row};
use iced::{Alignment, Element, Length, Task};
use money_app::shell::view::{commands, money_settings, money_shell};
use money_app::shell::{account_tag, tagged_account, MoneyShell, ShellMsg, ShellPage, SECTIONS};
use money_domain::{Auto, MoneyChange, MoneyView};

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

/// The place a page is in, a register changed from `base` with it: its
/// section, or an account's own place for its register; none for
/// Settings, which is the shell's other scene.
pub fn section_of(page: Page, base: Ledger) -> Option<String> {
    Some(match (page, base) {
        (Page::Budget, _) => SECTIONS[0].into(),
        (Page::Accounts, _) => SECTIONS[1].into(),
        (Page::Ledger(Ledger::Account(a)), _) | (Page::Ledger(Ledger::Adhoc), Ledger::Account(a)) => account_tag(&a.hex()),
        (Page::Ledger(_), _) => SECTIONS[2].into(),
        (Page::Reflect, _) => SECTIONS[3].into(),
        (Page::Settings, _) => return None,
    })
}

/// The page a place opens on.
pub fn page_for(app: &App, tag: &str) -> Option<Page> {
    if let Some(hex) = tagged_account(tag) {
        let a = app.peer.accounts().iter().find(|a| a.id.hex() == hex)?;
        return Some(Page::Ledger(Ledger::Account(a.id)));
    }
    match tag {
        "budget" => Some(Page::Budget),
        "accounts" => Some(Page::Accounts),
        "transactions" => Some(Page::Ledger(Ledger::All)),
        "insights" => Some(Page::Reflect),
        _ => None,
    }
}

impl App {
    /// A message of the shell taken; a place chosen is its page gone to,
    /// a command only this window's screens can carry out is theirs.
    pub(crate) fn shell_step(&mut self, msg: ShellMsg) -> Task<Message> {
        let go = match &msg {
            ShellMsg::Select { tag } => page_for(self, tag),
            ShellMsg::Back => Some(Page::Accounts),
            ShellMsg::Run { command } => command.strip_prefix("go-").and_then(|t| page_for(self, t)),
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
                "filter" => Some(Message::ShowFilter(!self.show_filter)),
                "find" => Some(Message::Go(Page::Ledger(Ledger::All))),
                "reconcile" => self.register_account().map(Message::Reconcile),
                "auto-assign" => Some(Message::AutoMenu),
                _ => name
                    .strip_prefix("search ")
                    .or((name == "search").then_some(""))
                    .map(|typed| Message::Filter(crate::filter::Msg::Edit(crate::filter::Edit::Text(typed.into())))),
            };
            if let Some(m) = m {
                tasks.push(self.update(m));
            }
        }
        Task::batch(tasks)
    }

    /// The shell's place, the page's: what a key, a link or the back
    /// button went to is lit in the sidebar; and the month the budget is on.
    pub(crate) fn sync_shell(&mut self) {
        let mut rt = std::mem::take(&mut self.shell);
        if let Some(tag) = section_of(self.page, self.peer.base) {
            if rt.state().section != tag {
                rt.dispatch(ShellMsg::Select { tag }, &mut Desk { app: self, natives: vec![] });
            }
        }
        if rt.state().month != self.peer.month {
            let month = self.peer.month.clone();
            rt.dispatch(ShellMsg::Month { month }, &mut Desk { app: self, natives: vec![] });
        }
        self.shell = rt;
    }

    /// What the shell shows now.
    pub fn shell_page(&self) -> ShellPage {
        self.shell.screen_of(&self.peer.store, crate::overview::now_ms(&self.today))
    }

    /// The window as the shell draws it: its width and which pane has the keys.
    pub fn frame(&self) -> Frame {
        Frame {
            width: self.window.width,
            focus: self.keyboard.focus,
        }
    }

    /// Whether the sections are a sidebar (`arkui::view::shell`): a wide window.
    pub fn has_sidebar(&self) -> bool {
        self.frame().class() != SizeClass::Compact && self.window.width >= arkui::layout_tokens::SIDEBAR_MIN_WIDTH
    }

    /// The panes on screen, for the keys to walk.
    pub fn panes(&self) -> Vec<Pane> {
        let page = self.shell_page();
        let mut panes = self.frame().panes(false, page.inspector);
        if !self.has_sidebar() {
            panes.retain(|p| *p != Pane::Sidebar);
        }
        panes
    }

    /// The sidebar's places: each its glyph, its title, its heading, its
    /// page — the sections, then the accounts under theirs.
    pub fn places(&self) -> Vec<(&'static [u8], String, Option<&'static str>, Page)> {
        use arkui::glyphs;
        let mut out = vec![
            (glyphs::MAIL, "Budget".into(), None, Page::Budget),
            (glyphs::BANK, "Accounts".into(), None, Page::Accounts),
            (glyphs::LIST, "Transactions".into(), None, Page::Ledger(Ledger::All)),
            (glyphs::CHART, "Insights".into(), None, Page::Reflect),
        ];
        let open: Vec<_> = self.peer.accounts().iter().filter(|a| !a.closed).collect();
        for (heading, on) in [("Budget Accounts", true), ("Tracking Accounts", false)] {
            for a in open.iter().filter(|a| a.on_budget == on) {
                let glyph = glyphs::named(money_app::accounts::kind_glyph(&a.kind)).unwrap_or(glyphs::BANK);
                out.push((glyph, a.name.clone(), Some(heading), Page::Ledger(Ledger::Account(a.id))));
            }
        }
        out
    }

    /// Where the sidebar's cursor is: the place the page is in.
    pub fn sidebar_at(&self) -> usize {
        let tag = section_of(self.page, self.peer.base).unwrap_or_else(|| self.shell.state().section.clone());
        let places = self.places();
        places
            .iter()
            .position(|p| section_of(p.3, self.peer.base).as_deref() == Some(tag.as_str()))
            .unwrap_or(0)
    }

    /// The tag of the sidebar's place `i`.
    pub fn place_tag(&self, i: usize) -> String {
        self.places()
            .get(i)
            .and_then(|p| section_of(p.3, Ledger::All))
            .unwrap_or_else(|| SECTIONS[0].into())
    }

    /// The command `key` with `mods` is the shortcut of (Ctrl, or ⌘ on a Mac).
    pub(crate) fn shortcut(&self, key: &Key, mods: Modifiers) -> Option<ShellMsg> {
        if !(mods.control() || mods.logo()) {
            return None;
        }
        let Key::Character(c) = key else { return None };
        ark_app::shell::command_for(&commands(), c.as_str(), mods.shift(), mods.alt()).map(|c| c.run.clone())
    }

    /// Where a hosted page starts in the window: past the sidebar, under
    /// the toolbar (and the large title, short of a sidebar).
    pub fn page_origin(&self) -> iced::Point {
        use arkui::layout_tokens::{SIDEBAR_WIDTH, TOOLBAR_HEIGHT};
        match self.has_sidebar() {
            true => iced::Point::new(SIDEBAR_WIDTH, TOOLBAR_HEIGHT + 1.0),
            false => iced::Point::new(0.0, TOOLBAR_HEIGHT + LARGE_TITLE_ROW),
        }
    }

    /// The Auto-Assign menu, under the toolbar's wand: what each way would
    /// do to the month.
    pub(crate) fn auto_menu(&mut self) {
        use arkui::menu::{Anchor, Entry, Menu};
        let entries = vec![
            Entry::run(arkui::glyphs::TARGET, "Underfunded", Message::Auto(Auto::Underfunded, vec![])),
            Entry::run(arkui::glyphs::REPEAT, "Assigned Last Month", Message::Auto(Auto::LastMonth, vec![])),
            Entry::run(arkui::glyphs::REOPEN, "Reset Assigned Amounts", Message::Auto(Auto::Reset, vec![])),
        ];
        let right = self.window.width - 12.0 - 30.0 - 2.0;
        let at = iced::Point::new(right, arkui::layout_tokens::TOOLBAR_HEIGHT - 6.0);
        self.ctx
            .open_menu(Menu::open("Auto-Assign", entries, at, self.window, Anchor::RightEdge(right)));
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
            ("budget.inspector", _) => self.view_inspector(),
            ("accounts", _) => self.view_accounts(),
            ("accounts.register", Page::Ledger(l)) => self.view_ledger(l),
            ("transactions", Page::Ledger(l)) => self.view_ledger(l),
            ("transactions", _) => self.view_ledger(Ledger::All),
            ("transactions.inspector", _) => self.view_receipt_side(),
            ("insights", _) => self.view_reflect(),
            ("household", _) => self.view_household(),
            ("settings.general", _) => self.view_settings(),
            ("settings.banks", _) => self.view_banks_pane(),
            ("settings.household", _) => quiet("Who shares this budget, and their roles."),
            ("accounts.register", _) => quiet("Select an Account"),
            _ => quiet("Nothing Selected"),
        }
    }

    /// The sidebar's foot: who is signed in (or that this is the demo), and
    /// Settings beside it — in place of an account button over the toolbar.
    fn view_household(&self) -> Element<'_, Message> {
        let who = self.account();
        let name = match who.as_ref().and_then(|a| a.who.as_ref()) {
            Some(w) => w.full().to_string(),
            None => "Demo Household".into(),
        };
        let face = arkui::account::face(who.as_ref().and_then(|a| a.who.as_ref()), self.account_picture(), 24.0);
        let me = iced::widget::button(row![face, text(name).size(13)].spacing(8).align_y(Alignment::Center))
            .padding([4, 6])
            .style(arkui::style::quiet)
            .on_press(Message::AccountMenu);
        row![
            container(me).width(Length::Fill),
            arkui::button::icon(arkui::glyphs::SETTINGS, Message::Go(Page::Settings))
        ]
        .align_y(Alignment::Center)
        .into()
    }
}

/// The large title's row under the toolbar, short of a sidebar
/// (`arkui::view`'s toolbar): its size's line and its padding.
const LARGE_TITLE_ROW: f32 = 28.0 * 1.3 + 8.0;

/// A word where there is nothing to show yet, quiet and centred.
fn quiet<'a>(words: &'static str) -> Element<'a, Message> {
    container(column![text(words).size(13).style(arkui::style::dim)])
        .center(Length::Fill)
        .into()
}
