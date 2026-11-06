//! Settings' Banks section: the banks and cards the household connected through
//! Plaid, how each is doing, and what each of their accounts feeds.
//!
//! **Connecting** is an ask of the server's (`banks.connect`,
//! `money_domain::banks`): the bank's login is the server's alone, so this
//! window never sees it. The server answers with the address of a Plaid
//! page, which this window opens as soon as the answer arrives — in the
//! system's browser on the desktop, in a tab opened at the click in a
//! browser ([`crate::browser`]) — and Settings follow the ask until
//! the bank is connected. **Reconnect** is the same for a bank whose login
//! ran out (shown only then), **Add accounts** the same with Plaid's
//! account picker, for an account the bank did not share at first, and
//! **Disconnect** asks the server to forget a bank.
//!
//! A new bank's accounts wait for somebody to say what each feeds, with
//! one control each: a menu of a new account of its kind (a card a credit
//! account), every account here, and nothing — the account another login
//! to the same bank feeds with the same last digits already on the cursor.

use arkui::{glyphs, icon, menu, style};
use iced::widget::{button, column, container, row, text, Column};
use iced::{Alignment, Element, Length, Padding};
use money_domain::*;

use crate::amount::money;
use crate::view::{action, bold, dim, fill, link, quiet};
use crate::{browser, App, Message, Modal, Page};

/// What Settings does.
#[derive(Debug, Clone)]
pub enum Msg {
    Connect,
    Reconnect(String),
    /// Asked whether to, first.
    Disconnect(String),
    Disconnected(String),
    /// A bank's account given an account of its own.
    Open(String),
    /// What it feeds, asked: its menu.
    Pick(String),
    /// Plaid's account picker, for accounts a bank did not share.
    AddAccounts(String),
    Link(String, Id<Account>),
    Ignore(String, bool),
    /// Plaid's page opened again, from its link.
    OpenPage(String),
    Dismiss,
}

impl App {
    /// Settings' Banks section's messages.
    pub fn banks(&mut self, m: Msg) -> iced::Task<Message> {
        match m {
            Msg::Connect => {
                let _ = self.step(Message::Go(Page::Settings));
                self.ask_page("connect a bank", "connect", &());
            }
            Msg::Reconnect(bank) => self.ask_page("reconnect the bank", "reconnect", &BankName { bank }),
            Msg::Disconnect(bank) => self.modal = Some(Modal::Disconnect(bank)),
            Msg::Disconnected(bank) => {
                self.modal = None;
                if self.server.is_none() {
                    self.note = "the demo has no server to disconnect a bank from".into();
                } else if self.author("disconnect the bank", "disconnect", &BankName { bank }) {
                    self.dismissed = None;
                    self.note = "asked the server to disconnect the bank".into();
                }
            }
            Msg::Open(plaid_account_id) => {
                let open = OpenLinked {
                    plaid_account_id,
                    name: String::new(),
                };
                if self.author("open an account for it", "open", &open) {
                    self.note = "opened \u{2014} its history arrives with the bank's next import".into();
                }
            }
            Msg::Pick(plaid) => self.pick(&plaid),
            Msg::AddAccounts(bank) => self.ask_page("add accounts", "add_accounts", &BankName { bank }),
            Msg::Link(plaid_account_id, account_id) => {
                self.modal = None;
                let l = LinkAccount {
                    plaid_account_id,
                    account_id,
                };
                if self.author("link it", "link", &l) {
                    self.note = "linked \u{2014} the bank's next import writes there".into();
                }
            }
            Msg::Ignore(plaid_account_id, ignored) => {
                let i = IgnoreLinked { plaid_account_id, ignored };
                self.author(if ignored { "ignore it" } else { "follow it" }, "ignore", &i);
            }
            Msg::OpenPage(url) => self.open_page(url),
            Msg::Dismiss => self.dismissed = self.peer.asks().first().map(|a| a.id),
        }
        iced::Task::none()
    }

    /// What a bank's account could feed, as its menu's entries: a new
    /// account of its kind, while it feeds none; every open account here
    /// but the one it feeds and those another of its bank's accounts feeds
    /// (one fed by another login to the bank is one it may take over); and
    /// nothing. With the entry the cursor starts on: the account another
    /// login to the same bank feeds from the same account at the bank,
    /// when there is one, else the first.
    pub fn choices(&self, l: &LinkedLine) -> (Vec<menu::Entry<Message>>, usize) {
        let id = || l.plaid_account_id.clone();
        let mut entries = vec![];
        if l.account_id.is_none() {
            let new = format!("New {} account", l.opens_as);
            entries.push(menu::Entry::run(glyphs::PLUS, new, Message::Banks(Msg::Open(id()))));
        }
        if l.ignored && l.account_id.is_some() {
            let again = format!("Feeds {} again", l.account);
            entries.push(menu::Entry::run(glyphs::WALLET, again, Message::Banks(Msg::Ignore(id(), false))));
        }
        let suggested = self.suggested(l);
        let mut at = 0;
        let siblings = |a: Id<Account>| {
            (self.peer.linked().iter()).any(|o| o.bank == l.bank && o.plaid_account_id != l.plaid_account_id && !o.ignored && o.account_id == Some(a))
        };
        for a in self
            .peer
            .accounts()
            .iter()
            .filter(|a| !a.closed && Some(a.id) != l.account_id && !siblings(a.id))
        {
            if Some(a.id) == suggested {
                at = entries.len();
            }
            let feeds = format!("Feeds {}", a.name);
            entries.push(menu::Entry::run(glyphs::WALLET, feeds, Message::Banks(Msg::Link(id(), a.id))));
        }
        if !l.ignored {
            entries.push(menu::Entry::run(glyphs::HIDE, "Ignore", Message::Banks(Msg::Ignore(id(), true))));
        }
        (entries, at)
    }

    /// The account another login to the same bank (by its institution)
    /// feeds from an account with the same last digits and type.
    pub fn suggested(&self, l: &LinkedLine) -> Option<Id<Account>> {
        let institution = |bank: &str| {
            (self.peer.banks().iter())
                .find(|b| b.name == bank)
                .map(|b| b.institution.trim().to_lowercase())
                .filter(|i| !i.is_empty())
        };
        let mine = institution(&l.bank)?;
        if l.mask.is_empty() {
            return None;
        }
        (self.peer.linked().iter())
            .filter(|o| o.bank != l.bank && !o.ignored && o.mask == l.mask && o.kind == l.kind && o.subtype == l.subtype)
            .filter(|o| institution(&o.bank).as_ref() == Some(&mine))
            .find_map(|o| o.account_id)
            .filter(|a| self.peer.accounts().iter().any(|x| x.id == *a && !x.closed))
    }

    /// A bank's account's menu, at the pointer, on the entry suggested.
    fn pick(&mut self, plaid: &str) {
        let Some(l) = self.peer.linked().iter().find(|l| l.plaid_account_id == plaid) else {
            return;
        };
        let (entries, at) = self.choices(l);
        let title = l.title();
        let mut m = menu::Menu::open(title, entries, self.pointer, self.window, menu::Anchor::Pointer);
        m.at = at;
        self.ctx.open_menu(m);
    }

    /// An ask for a Plaid page: a tab kept for it now, while the click
    /// counts, and the page opened in it when the server answers.
    fn ask_page(&mut self, what: &str, name: &str, input: &impl ark::format::Encode) {
        if self.server.is_none() {
            self.note = "the demo has no server to connect a bank through".into();
            return;
        }
        browser::reserve();
        let since = self.peer.asks().first().map(|a| a.id);
        if self.author(what, name, input) {
            (self.awaiting, self.dismissed) = (Some(since), None);
            self.note = "asked the server for Plaid's page".into();
        } else {
            browser::release();
        }
    }

    /// The page asked for here, opened once the server has answered with
    /// it; nothing more to wait for once the ask is otherwise answered.
    pub fn follow_ask(&mut self) {
        let Some(since) = self.awaiting else { return };
        match money_domain::banks::follow(self.peer.asks(), since) {
            money_domain::banks::Follow::Wait => {}
            money_domain::banks::Follow::Page { url } => {
                self.awaiting = None;
                self.open_page(url);
            }
            money_domain::banks::Follow::Over => {
                self.awaiting = None;
                browser::release();
            }
        }
    }

    fn open_page(&mut self, url: String) {
        self.note = match browser::open(&url) {
            true => "Plaid's page is open in your browser".into(),
            false => "open Plaid's page from Settings".into(),
        };
        self.opened.push(url);
    }

    /// What wants somebody on Settings: a login run out, or an
    /// account waiting for a choice.
    pub fn banks_wanting(&self) -> usize {
        money_domain::words::banks_wanting(self.peer.banks()) as usize
    }

    /// The ask on show: the newest of this person's, while it is open or
    /// lately answered, unless it was put away.
    fn ask_shown(&self, now: i64) -> Option<AskLine> {
        money_domain::words::ask_shown(self.peer.asks(), self.dismissed, now)
    }

    fn view_ask<'a>(&'a self, a: AskLine, now: i64) -> Element<'a, Message> {
        let mut c = column![].spacing(8);
        c = c.push(text(a.said(now)).size(13));
        if a.state == AskState::Waiting {
            c = c.push(
                row![
                    link("Open Plaid's page", Message::Banks(Msg::OpenPage(a.url.clone()))),
                    dim(a.closes(now).unwrap_or_default(), 11)
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            );
        }
        let close: Element<'a, Message> = match matches!(a.state, AskState::Asked | AskState::Waiting) {
            true => fill().into(),
            false => quiet("Dismiss", Message::Banks(Msg::Dismiss)),
        };
        container(row![c.width(Length::Fill), close].align_y(Alignment::Center))
            .padding(14)
            .width(Length::Fill)
            .style(arkui::style::card)
            .into()
    }

    /// Settings' Banks section: what it is, Connect and Sync, the ask on
    /// show, and each connection with its accounts.
    pub fn banks_section(&self) -> Element<'_, Message> {
        let now = ark_peer::now_ms();
        let mut tools = row![action("+ Connect a bank", Message::Banks(Msg::Connect))]
            .spacing(10)
            .align_y(Alignment::Center);
        if !self.peer.banks().is_empty() {
            tools = tools.push(quiet("Sync", Message::Sync));
        }
        let explain = "Banks and cards connected through Plaid. Their logins are kept on the server, never here; \
                       each of their accounts feeds an account here, or nothing.";
        let mut body: Column<'_, Message> = column![
            text(explain)
                .size(14)
                .line_height(1.45)
                .style(style::faint(0.8))
                .wrapping(text::Wrapping::Word),
            tools
        ]
        .spacing(14);
        if let Some(a) = self.ask_shown(now) {
            body = body.push(self.view_ask(a, now));
        }
        for b in self.peer.banks() {
            body = body.push(self.view_bank(b, now));
        }
        if self.peer.banks().is_empty() {
            body = body.push(dim(
                "No banks yet. Connect one: Plaid's page opens in your browser, and the login stays on the server.",
                13,
            ));
        }
        container(body).padding(Padding { top: 8.0, ..Padding::ZERO }).into()
    }

    fn view_bank<'a>(&'a self, b: &'a BankLine, now: i64) -> Element<'a, Message> {
        let title = b.title();
        let s = b.status(now);
        let status: Element<'a, Message> = match s.alarm {
            true => text(s.words)
                .size(12)
                .style(|theme| text::Style {
                    color: Some(arkui::theme::of(theme).danger.base.color),
                })
                .into(),
            false => dim(s.words, 12).into(),
        };
        let mut head = row![
            icon::plain(glyphs::BANK, false),
            column![bold(title, 15), status].spacing(2).width(Length::Fill),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        if b.needs_login {
            head = head.push(action("Reconnect", Message::Banks(Msg::Reconnect(b.name.clone()))));
        }
        head = head
            .push(quiet("Add accounts", Message::Banks(Msg::AddAccounts(b.name.clone()))))
            .push(quiet("Disconnect", Message::Banks(Msg::Disconnect(b.name.clone()))));
        let mut accounts = column![].spacing(6);
        for l in self.peer.linked().iter().filter(|l| l.bank == b.name) {
            accounts = accounts.push(self.view_linked(l));
        }
        container(column![head, accounts].spacing(12))
            .padding(14)
            .width(Length::Fill)
            .style(arkui::style::card)
            .into()
    }

    /// A bank's account: what it is at the bank, and one control for what
    /// it feeds — its menu ([`App::choices`]), saying what it feeds now or,
    /// while it waits, what its menu suggests.
    fn view_linked<'a>(&'a self, l: &'a LinkedLine) -> Element<'a, Message> {
        let what = match l.subtype.is_empty() {
            true => l.kind.clone(),
            false => l.subtype.clone(),
        };
        let about = column![
            text(l.title()).size(13),
            dim(
                match l.balance {
                    Some(b) => format!("{what} \u{2014} the bank says {}", money(b)),
                    None => what,
                },
                11
            )
        ]
        .spacing(1)
        .width(Length::Fill);
        let label = if l.waiting {
            let (entries, at) = self.choices(l);
            entries.get(at).map(|e| e.label.clone()).unwrap_or_else(|| "Choose".into())
        } else if l.ignored {
            "Ignored".into()
        } else {
            format!("Feeds {}", l.account)
        };
        let pick = button(
            row![text(label).size(13), icon::line(glyphs::DOWN, false)]
                .spacing(6)
                .align_y(Alignment::Center),
        )
        .on_press(Message::Banks(Msg::Pick(l.plaid_account_id.clone())))
        .padding([5, 10]);
        let pick = match l.waiting {
            true => pick.style(style::action),
            false => pick.style(arkui::style::chip(false)),
        };
        let mut choice = row![].spacing(8).align_y(Alignment::Center);
        if l.waiting {
            choice = choice.push(dim("Feeds?", 12));
        }
        choice = choice.push(pick);
        if let Some(a) = l.account_id.filter(|_| !l.ignored) {
            choice = choice.push(link("Open", Message::Go(Page::Ledger(crate::peer::Ledger::Account(a)))));
        }
        container(row![about, choice].spacing(12).align_y(Alignment::Center))
            .padding([8, 10])
            .width(Length::Fill)
            .into()
    }

    pub fn view_disconnect<'a>(&'a self, bank: &'a str) -> Element<'a, Message> {
        let b = self.peer.banks().iter().find(|b| b.name == bank);
        let name = b
            .map(|b| {
                if b.institution.is_empty() {
                    b.name.clone()
                } else {
                    b.institution.clone()
                }
            })
            .unwrap_or_default();
        column![
            bold(format!("Disconnect {name}?"), 16),
            text("Plaid forgets the login and the bank stops importing. Its accounts here, and what is in them, stay.").size(13),
            row![
                fill(),
                quiet("Cancel", Message::CloseModal),
                action("Disconnect", Message::Banks(Msg::Disconnected(bank.to_string())))
            ]
            .spacing(8),
        ]
        .spacing(10)
        .into()
    }
}
