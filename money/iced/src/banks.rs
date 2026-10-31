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
//! ran out, and **Disconnect** asks the server to forget a bank.
//!
//! A new bank's accounts wait for somebody to say what each feeds: an
//! account opened for it (a card a credit account), one already here, or
//! nothing.

use arkui::{glyphs, icon, style};
use iced::widget::{button, column, container, row, scrollable, text, Column};
use iced::{Alignment, Element, Length, Padding};
use money_domain::*;

use crate::amount::money;
use crate::view::{action, bold, dim, fill, link, quiet};
use crate::{browser, look, App, Message, Modal, Page};

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
    /// Which account here it is to feed, asked.
    LinkTo(String),
    Link(String, Id<Account>),
    Ignore(String, bool),
    /// Plaid's page opened again, from its link.
    OpenPage(String),
    Dismiss,
}

/// How long an answered ask stays on the page.
const SHOWN_MS: i64 = 15 * 60 * 1000;

/// A time ago, in words.
fn ago(then_ms: i64, now_ms: i64) -> String {
    let m = (now_ms - then_ms).max(0) / 60_000;
    match m {
        0 => "just now".into(),
        1..=59 => format!("{m} min ago"),
        60..=2879 => format!("{} h ago", m / 60),
        _ => format!("{} days ago", m / 1440),
    }
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
            Msg::LinkTo(plaid) => self.modal = Some(Modal::LinkTo(plaid)),
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
            Msg::Dismiss => self.dismissed = self.peer.asks.rows.first().map(|a| a.id),
        }
        iced::Task::none()
    }

    /// An ask for a Plaid page: a tab kept for it now, while the click
    /// counts, and the page opened in it when the server answers.
    fn ask_page(&mut self, what: &str, name: &str, input: &impl ark::format::Encode) {
        if self.server.is_none() {
            self.note = "the demo has no server to connect a bank through".into();
            return;
        }
        browser::reserve();
        let now = ark_peer::now_ms();
        if self.author(what, name, input) {
            (self.awaiting, self.dismissed) = (Some(now), None);
            self.note = "asked the server for Plaid's page".into();
        } else {
            browser::release();
        }
    }

    /// The page asked for here, opened once the server has answered with
    /// it; nothing more to wait for once the ask is otherwise answered.
    pub fn follow_ask(&mut self) {
        let Some(since) = self.awaiting else { return };
        let Some(a) = self.peer.asks.rows.first().filter(|a| a.asked_ms >= since - 1_000) else {
            return;
        };
        match a.state {
            AskState::Asked => {}
            AskState::Waiting if !a.url.is_empty() => {
                let url = a.url.clone();
                self.awaiting = None;
                self.open_page(url);
            }
            _ => {
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
        self.peer.banks.rows.iter().map(|b| b.waiting as usize + b.needs_login as usize).sum()
    }

    /// The ask on show: the newest of this person's, while it is open or
    /// lately answered, unless it was put away.
    fn ask_shown(&self, now: i64) -> Option<&AskLine> {
        let a = self.peer.asks.rows.first()?;
        let open = matches!(a.state, AskState::Asked | AskState::Waiting);
        (Some(a.id) != self.dismissed && (open || now - a.answered_ms < SHOWN_MS)).then_some(a)
    }

    fn view_ask<'a>(&'a self, a: &'a AskLine, now: i64) -> Element<'a, Message> {
        let name = match (a.institution.as_str(), a.bank.as_str()) {
            ("", "") => "the bank".to_string(),
            ("", b) => b.to_string(),
            (i, _) => i.to_string(),
        };
        let mut c = column![].spacing(8);
        let said = match (a.state, a.what.as_str()) {
            (AskState::Asked, _) if now - a.asked_ms > 30_000 => "The server has not answered yet. Does it have Plaid's keys?".to_string(),
            (AskState::Asked, "disconnect") => format!("Asking the server to disconnect {name}\u{2026}"),
            (AskState::Asked, _) => "Asking the server for Plaid's page\u{2026}".into(),
            (AskState::Waiting, _) => "Plaid's page is open in your browser. Log in to your bank there; this page follows.".into(),
            (AskState::Done, "connect") => format!("Connected {name}. Say below what each of its accounts feeds."),
            (AskState::Done, "reconnect") => format!("Logged in to {name} again."),
            (AskState::Done, _) => format!("Disconnected {name}. Its accounts here stay, kept by hand from now on."),
            (AskState::Failed, _) => format!("Could not {} {name}: {}", a.what, a.message),
            (AskState::Expired, _) => a.message.clone(),
        };
        c = c.push(text(said).size(13));
        if a.state == AskState::Waiting {
            c = c.push(
                row![
                    link("Open Plaid's page", Message::Banks(Msg::OpenPage(a.url.clone()))),
                    dim(ago_or_in(a.expires_ms, now), 11)
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
            .style(look::card)
            .into()
    }

    /// Settings' Banks section: what it is, Connect and Sync, the ask on
    /// show, and each connection with its accounts.
    pub fn banks_section(&self) -> Element<'_, Message> {
        let now = ark_peer::now_ms();
        let mut tools = row![action("+ Connect a bank", Message::Banks(Msg::Connect))]
            .spacing(10)
            .align_y(Alignment::Center);
        if !self.peer.banks.rows.is_empty() {
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
        for b in &self.peer.banks.rows {
            body = body.push(self.view_bank(b, now));
        }
        if self.peer.banks.rows.is_empty() {
            body = body.push(dim(
                "No banks yet. Connect one: Plaid's page opens in your browser, and the login stays on the server.",
                13,
            ));
        }
        container(body).padding(Padding { top: 8.0, ..Padding::ZERO }).into()
    }

    fn view_bank<'a>(&'a self, b: &'a BankLine, now: i64) -> Element<'a, Message> {
        let title = match b.institution.is_empty() {
            true => b.name.clone(),
            false => b.institution.clone(),
        };
        let status: Element<'a, Message> = if b.needs_login {
            text("The bank wants its login again: Reconnect.")
                .size(12)
                .style(|theme| text::Style {
                    color: Some(arkui::theme::of(theme).danger.base.color),
                })
                .into()
        } else if !b.error.is_empty() {
            text(format!("The last import failed {}: {}", ago(b.failed_ms, now), b.error))
                .size(12)
                .style(|theme| text::Style {
                    color: Some(arkui::theme::of(theme).danger.base.color),
                })
                .into()
        } else if b.synced_ms == 0 {
            dim("Waiting for its first import", 12).into()
        } else {
            dim(
                format!("Synced {} \u{2014} {} new, {} updated", ago(b.synced_ms, now), b.added, b.updated),
                12,
            )
            .into()
        };
        let reconnect = match b.needs_login {
            true => action("Reconnect", Message::Banks(Msg::Reconnect(b.name.clone()))),
            false => quiet("Reconnect", Message::Banks(Msg::Reconnect(b.name.clone()))),
        };
        let head = row![
            icon::plain(glyphs::BANK, false),
            column![bold(title, 15), status].spacing(2).width(Length::Fill),
            reconnect,
            quiet("Disconnect", Message::Banks(Msg::Disconnect(b.name.clone()))),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        let mut accounts = column![].spacing(6);
        for l in self.peer.linked.rows.iter().filter(|l| l.bank == b.name) {
            accounts = accounts.push(self.view_linked(l));
        }
        container(column![head, accounts].spacing(12))
            .padding(14)
            .width(Length::Fill)
            .style(look::card)
            .into()
    }

    fn view_linked<'a>(&'a self, l: &'a LinkedLine) -> Element<'a, Message> {
        let id = || l.plaid_account_id.clone();
        let name = match l.mask.is_empty() {
            true => l.name.clone(),
            false => format!("{} \u{2022}\u{2022}{}", l.name, l.mask),
        };
        let what = match l.subtype.is_empty() {
            true => l.kind.clone(),
            false => l.subtype.clone(),
        };
        let about = column![
            text(name).size(13),
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
        let choice: Element<'a, Message> = if l.waiting {
            row![
                dim("Feeds?", 12),
                action(&format!("New {} account", l.opens_as), Message::Banks(Msg::Open(id()))),
                quiet("An existing account\u{2026}", Message::Banks(Msg::LinkTo(id()))),
                quiet("Ignore", Message::Banks(Msg::Ignore(id(), true))),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into()
        } else if l.ignored {
            row![dim("Ignored", 12), link("Follow", Message::Banks(Msg::Ignore(id(), false)))]
                .spacing(10)
                .align_y(Alignment::Center)
                .into()
        } else {
            let to = l.account_id.map(|a| Message::Go(Page::Ledger(crate::peer::Ledger::Account(a))));
            let mut r = row![dim("Feeds", 12)].spacing(10).align_y(Alignment::Center);
            r = match to {
                Some(go) => r.push(link(l.account.clone(), go)),
                None => r.push(dim(l.account.clone(), 12)),
            };
            r.push(link("Change\u{2026}", Message::Banks(Msg::LinkTo(id()))))
                .push(link("Ignore", Message::Banks(Msg::Ignore(id(), true))))
                .into()
        };
        container(row![about, choice].spacing(12).align_y(Alignment::Center))
            .padding([8, 10])
            .width(Length::Fill)
            .style(look::group_card)
            .into()
    }

    /// The accounts a bank's account could feed: open, and fed by no bank.
    pub fn view_link_to<'a>(&'a self, plaid: &'a str) -> Element<'a, Message> {
        let l = self.peer.linked.rows.iter().find(|l| l.plaid_account_id == plaid);
        let name = l.map(|l| l.name.clone()).unwrap_or_default();
        let mut list = column![].spacing(1);
        let free = self.peer.accounts.rows.iter().filter(|a| !a.closed && a.bank.is_empty());
        let mut any = false;
        for a in free {
            any = true;
            list = list.push(
                button(row![text(a.name.clone()).size(13).width(Length::Fill), dim(a.kind.clone(), 11)].align_y(Alignment::Center))
                    .on_press(Message::Banks(Msg::Link(plaid.to_string(), a.id)))
                    .padding([6, 8])
                    .width(Length::Fill)
                    .style(look::choice(false)),
            );
        }
        if !any {
            list = list.push(dim("Every open account here is fed by a bank already.", 12));
        }
        column![
            bold(format!("What does {name} feed?"), 16),
            dim(
                "An account you have kept by hand takes what the bank has from the day it was connected.",
                11
            ),
            scrollable(list).height(Length::Shrink),
            row![fill(), quiet("Cancel", Message::CloseModal)],
        ]
        .spacing(10)
        .into()
    }

    pub fn view_disconnect<'a>(&'a self, bank: &'a str) -> Element<'a, Message> {
        let b = self.peer.banks.rows.iter().find(|b| b.name == bank);
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

/// When a page closes: in so many minutes.
fn ago_or_in(then_ms: i64, now_ms: i64) -> String {
    let m = (then_ms - now_ms).max(0) / 60_000;
    match m {
        0 => "it closes in under a minute".into(),
        _ => format!("it closes in {m} min"),
    }
}

#[cfg(test)]
mod tests {
    use super::ago;

    #[test]
    fn times_ago_read() {
        assert_eq!(ago(0, 30_000), "just now");
        assert_eq!(ago(0, 5 * 60_000), "5 min ago");
        assert_eq!(ago(0, 3 * 3_600_000), "3 h ago");
        assert_eq!(ago(0, 3 * 86_400_000), "3 days ago");
    }
}
