//! The Settings scene's panes, the shell's (ui/docs/apps/money.md,
//! Scenes): General — who is using the budget, the server and whether it
//! can be reached, Add a Device, and the way out; Household — who shares
//! the budget; Connected Banks — the ask of the moment, each bank with how
//! it is doing and its own page (when it last synced and what that
//! brought, Reconnect, Disconnect, and what each of its accounts feeds),
//! Connect a Bank; and Appearance. Drawn from [`super::view`] on every
//! toolkit.
//!
//! What only the host has — who signed in where, whether the link is up,
//! what waits to sync — it tells the shell ([`ShellMsg::Session`]). What
//! only the host can do — sign out, make a device's code, open Plaid's
//! page — the shell asks of it ([`MoneyRequest`]). Plaid's page is the
//! shell's to say ([`SettingsPage::plaid`]): asked for, it is the page the
//! server answers the ask with, shown until the ask is over or the person
//! closes it ([`ShellMsg::PageClosed`]).

use ark::row::Id;
use ark_app::{Command, Data};
use ark_format::NumberStyle;
use money_core::banks::{follow, Follow};
use money_core::schema::BankAsk;
use money_core::words::{ask_shown, bank_accounts};
use money_core::*;

use super::{Appearance, MoneyShell, ShellMsg, ShellState};
use crate::words::BankWords;
use crate::MoneyRequest;

/// Who is using the budget and where, as the host knows it: what
/// [`ShellMsg::Session`] carries.
#[derive(Clone, Debug, Default, PartialEq, ark::twin::Twin)]
pub struct SessionFacts {
    /// Their name ("Alex"); empty while it is not known.
    pub who: String,
    /// Their email or login, under the name.
    pub login: String,
    /// The demo's household, on this device alone.
    pub demo: bool,
    /// The server, as it is shown (`money.example.com`).
    pub server: String,
    /// The link to it: up, down, or not known yet (still opening).
    pub linked: Option<bool>,
    /// Changes made here not yet synced.
    pub pending: i64,
    /// The app's version, as the device says it: `0.1 (12)`.
    pub version: String,
    /// This host can sign out or leave the demo (the browser's demo is the
    /// page's alone, and has no way out).
    pub leaves: bool,
    /// This host can make a code for another device (a phone, the desktop).
    pub adds_devices: bool,
}

/// What Settings is in the middle of.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettingsState {
    pub facts: SessionFacts,
    /// Sign Out (or Leave the Demo) asked about.
    pub leaving: bool,
    /// An ask made here for Plaid's page: the newest ask before it (none:
    /// there was none), followed until the server answers it.
    pub awaiting: Option<Option<Id<BankAsk>>>,
    /// Plaid's page opened again from the ask's row.
    pub reopened: Option<String>,
    /// Plaid's page the person closed: not shown again unless asked.
    pub closed: Option<String>,
    /// The answered ask put away.
    pub dismissed: Option<Id<BankAsk>>,
    /// The bank whose page shows, by the name the server keeps it under.
    pub bank: Option<String>,
    /// That bank's Disconnect asked about.
    pub disconnecting: bool,
    /// One of the bank's accounts being given what it feeds (its Plaid id).
    pub choosing: Option<String>,
}

/// The ask of the moment: where it stands in a few words, and in a
/// sentence under its card.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct AskItem {
    /// `Asking the Server`, `Plaid's Page Is Open`, `Connected`, `Not Finished`.
    pub title: String,
    /// How long ago it was asked or answered: `Just now`.
    pub state: String,
    /// What has come of it, and how long Plaid's page stays open.
    pub note: Option<String>,
    /// Plaid's page waits: Open Plaid's Page.
    pub waiting: bool,
    /// Answered: Dismiss.
    pub over: bool,
}

/// A connected bank's row.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct BankItem {
    pub name: String,
    pub title: String,
    pub status: String,
    /// Its login ran out or its import failed: marked.
    pub glyph: String,
    /// Its accounts waiting for a choice.
    pub waiting: Option<i64>,
}

/// One of a bank's accounts, and what it feeds here.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct FeedItem {
    pub plaid: String,
    pub title: String,
    /// Its kind and the bank's balance: `Credit Card · the bank says $1,204.10`.
    pub about: String,
    /// `Feeds Chase Sapphire`, `Not imported`, `Waiting for a choice`.
    pub feeds: String,
    pub glyph: String,
    pub kind: String,
    pub waiting: bool,
}

/// An account of the household's that a bank's could feed.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct FeedTarget {
    pub id: String,
    pub name: String,
    pub glyph: String,
    pub kind: String,
}

/// What one of a bank's accounts is to feed, being chosen.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct FeedChoice {
    pub title: String,
    /// `New Credit Card Account`, while it waits for a choice (`can_open`).
    pub can_open: bool,
    pub opens: String,
    pub accounts: Vec<FeedTarget>,
    /// Which accounts here it could feed, or that none is free for it.
    pub accounts_note: String,
    pub ignored: bool,
}

/// A connected bank's own page.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct BankPage {
    pub title: String,
    pub needs_login: bool,
    pub login_note_shown: Option<String>,
    /// What Plaid said when the last import failed, while it is the last
    /// word (`failing`).
    pub failing: bool,
    pub error: String,
    pub failed: String,
    /// Not on a server: Reconnect and Disconnect are not offered.
    pub fixed: bool,
    pub synced: String,
    pub brought: String,
    pub removes: bool,
    pub removed: String,
    pub locks: bool,
    pub locked: String,
    pub feeds: Vec<FeedItem>,
    pub has_feeds: bool,
    pub feeds_note: Option<String>,
    /// The ask of the moment, when it is about this bank.
    pub ask: Option<AskItem>,
    pub connects: bool,
    /// Reconnect under the bank's figures (its login is good), and on the
    /// server.
    pub reconnects: bool,
    pub actions_note: Option<String>,
    pub disconnecting: bool,
    pub disconnect_question: String,
    pub choosing: Option<FeedChoice>,
}

/// What Settings shows.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct SettingsPage {
    pub who: String,
    pub login: String,
    pub demo: bool,
    /// On a server, not the demo: its link.
    pub on_server: bool,
    /// Add a Device is offered: on a server, where the host can make a code.
    pub adds_devices: bool,
    /// Sign Out (Leave the Demo) is offered.
    pub leaves: bool,
    pub server: String,
    /// `Connected`, `Can't Reach It Right Now`, `Opening…`.
    pub link: String,
    /// `3 changes`, while any wait to sync (`waits`).
    pub waits: bool,
    pub pending: String,
    pub sign_out: String,
    pub sign_out_note: Option<String>,
    pub leaving: bool,
    pub leave_question: String,
    pub version: String,
    pub household_note: Option<String>,
    pub ask: Option<AskItem>,
    pub banks: Vec<BankItem>,
    pub has_banks: bool,
    /// A bank can be connected (through a server's importer: not the
    /// demo's), and none is being connected already.
    pub can_connect: bool,
    pub banks_note: Option<String>,
    pub sync_note: Option<String>,
    /// The bank whose page shows.
    pub bank: Option<BankPage>,
    pub showing_bank: bool,
    /// Plaid's page, while it is to show: the host opens it (Safari's
    /// view, a Custom Tab, the browser) and says when it is closed.
    pub plaid: Option<String>,
    /// `system`, `light` or `dark`.
    pub appearance: String,
    pub appearance_note: Option<String>,
}

/// The holds Settings needs: the banks, their accounts, this person's asks.
pub fn holds() -> [MoneyView; 3] {
    [MoneyView::Banks, MoneyView::Linked, MoneyView::Asks]
}

/// A message of Settings' taken; `None` for one that is not Settings'.
pub fn update(s: &mut ShellState, msg: &ShellMsg, data: &Data<'_, MoneyView>) -> Option<Vec<Command<MoneyShell>>> {
    let st = &mut s.settings;
    let asks: &[AskLine] = data.rows(&MoneyView::Asks);
    let none = Some(vec![]);
    match msg {
        ShellMsg::Session { facts } => {
            st.facts = facts.clone();
            none
        }
        ShellMsg::SignOut => {
            st.leaving = st.facts.leaves;
            none
        }
        ShellMsg::CancelSignOut => {
            st.leaving = false;
            none
        }
        ShellMsg::ConfirmSignOut if st.leaving => {
            st.leaving = false;
            Some(vec![Command::Request(MoneyRequest::SignOut)])
        }
        ShellMsg::ConfirmSignOut => none,
        ShellMsg::AddDevice if st.facts.adds_devices && !st.facts.demo => Some(vec![Command::Request(MoneyRequest::AddDevice)]),
        ShellMsg::AddDevice => none,
        ShellMsg::ConnectBank => Some(ask_for_page(st, asks, MoneyChange::Connect)),
        ShellMsg::Reconnect => match st.bank.clone() {
            Some(bank) => Some(ask_for_page(st, asks, MoneyChange::Reconnect(BankName { bank }))),
            None => none,
        },
        ShellMsg::Disconnect => {
            st.disconnecting = st.bank.is_some() && !st.facts.demo;
            none
        }
        ShellMsg::CancelDisconnect => {
            st.disconnecting = false;
            none
        }
        ShellMsg::ConfirmDisconnect => {
            st.disconnecting = false;
            st.dismissed = None;
            match st.bank.clone() {
                Some(bank) if !st.facts.demo => Some(vec![Command::send(MoneyChange::Disconnect(BankName { bank }))]),
                _ => none,
            }
        }
        ShellMsg::OpenBank { name } => {
            st.bank = Some(name.clone());
            st.disconnecting = false;
            st.choosing = None;
            none
        }
        ShellMsg::CloseBank => {
            st.bank = None;
            st.disconnecting = false;
            st.choosing = None;
            none
        }
        ShellMsg::OpenAsk => {
            let shown = ask_shown(asks, st.dismissed, data.now_ms);
            if let Some(a) = shown.filter(|a| a.state == AskState::Waiting && !a.url.is_empty()) {
                st.reopened = Some(a.url.clone());
                st.closed = None;
            }
            none
        }
        ShellMsg::PageClosed { url } => {
            st.closed = Some(url.clone());
            st.reopened = None;
            st.awaiting = None;
            none
        }
        ShellMsg::DismissAsk => {
            st.dismissed = asks.first().map(|a| a.id);
            none
        }
        ShellMsg::ChooseFeed { plaid } => {
            st.choosing = Some(plaid.clone());
            none
        }
        ShellMsg::CancelFeed => {
            st.choosing = None;
            none
        }
        ShellMsg::FeedNew => match st.choosing.take() {
            Some(plaid) => Some(vec![Command::send(MoneyChange::OpenLinked(OpenLinked {
                plaid_account_id: plaid,
                name: String::new(),
            }))]),
            None => none,
        },
        ShellMsg::FeedAccount { id } => match (st.choosing.take(), crate::id_of(id)) {
            (Some(plaid), Some(account_id)) => Some(vec![Command::send(MoneyChange::Link(LinkAccount {
                plaid_account_id: plaid,
                account_id,
            }))]),
            _ => none,
        },
        ShellMsg::FeedIgnored { ignored } => match st.choosing.take() {
            Some(plaid) => Some(vec![Command::send(MoneyChange::Ignore(IgnoreLinked {
                plaid_account_id: plaid,
                ignored: *ignored,
            }))]),
            None => none,
        },
        ShellMsg::AppearanceKey { key } => {
            s.appearance = match key.as_str() {
                "light" => Some(Appearance::Light),
                "dark" => Some(Appearance::Dark),
                _ => None,
            };
            none
        }
        _ => None,
    }
}

/// An ask for Plaid's page sent, and followed from the newest ask before
/// it — by the ask's id, not a clock.
fn ask_for_page(st: &mut SettingsState, asks: &[AskLine], change: MoneyChange) -> Vec<Command<MoneyShell>> {
    if st.facts.demo {
        return vec![];
    }
    st.awaiting = Some(asks.first().map(|a| a.id));
    st.dismissed = None;
    st.closed = None;
    st.reopened = None;
    vec![Command::send(change)]
}

/// Plaid's page while it is to show: the one the server answered this
/// device's ask with, or the one opened again from the ask's row, until
/// the ask is over or the page was closed.
fn plaid(st: &SettingsState, asks: &[AskLine]) -> Option<String> {
    let asked = match st.awaiting.map(|since| follow(asks, since)) {
        Some(Follow::Page { url }) => Some(url),
        _ => None,
    };
    let url = st.reopened.clone().or(asked)?;
    let open = asks.iter().any(|a| a.url == url && a.is_open());
    (open && st.closed.as_deref() != Some(url.as_str())).then_some(url)
}

fn ask_item(a: &AskLine, now: i64) -> AskItem {
    let said = a.said(now);
    AskItem {
        title: match a.state {
            AskState::Asked => "Asking the Server",
            AskState::Waiting => "Plaid's Page Is Open",
            AskState::Done => "Done",
            AskState::Failed | AskState::Expired => "Not Finished",
        }
        .into(),
        note: Some(match a.closes(now) {
            Some(c) => format!("{said} {c}."),
            None => said,
        }),
        state: ark_format::ago_alone(if a.is_open() { a.asked_ms } else { a.answered_ms }, now),
        waiting: a.state == AskState::Waiting && !a.url.is_empty(),
        over: !a.is_open(),
    }
}

fn plural(n: i64, one: &str) -> String {
    match n {
        1 => format!("1 {one}"),
        n => format!("{n} {one}s"),
    }
}

/// What Settings shows of `st`, the banks, their accounts and the asks.
pub fn screen(s: &ShellState, data: &Data<'_, MoneyView>) -> SettingsPage {
    let st = &s.settings;
    let f = &st.facts;
    let now = data.now_ms;
    let banks: &[BankLine] = data.rows(&MoneyView::Banks);
    let linked: &[LinkedLine] = data.rows(&MoneyView::Linked);
    let asks: &[AskLine] = data.rows(&MoneyView::Asks);
    let accounts: &[AccountLine] = data.rows(&MoneyView::Accounts);
    let shown = ask_shown(asks, st.dismissed, now);
    let connects = !f.demo;
    let bank = st.bank.as_deref().and_then(|name| banks.iter().find(|b| b.name == name));
    SettingsPage {
        who: match (f.demo, f.who.is_empty()) {
            (true, _) => "Alex".into(),
            (false, true) => "Signed In".into(),
            (false, false) => f.who.clone(),
        },
        login: if f.demo {
            "The demo's household, on this device alone".into()
        } else {
            f.login.clone()
        },
        demo: f.demo,
        on_server: !f.demo,
        adds_devices: !f.demo && f.adds_devices,
        leaves: f.leaves,
        server: f.server.clone(),
        link: match f.linked {
            None => "Opening\u{2026}".into(),
            Some(true) => "Connected".into(),
            Some(false) => "Can't Reach It Right Now".into(),
        },
        waits: f.pending > 0,
        pending: plural(f.pending, "change"),
        sign_out: if f.demo { "Leave the Demo" } else { "Sign Out" }.into(),
        sign_out_note: Some(
            if f.demo {
                "Nothing in the demo leaves this device, and it starts again from the beginning next time."
            } else {
                "Anything not synced yet stays on this device, and goes when you sign in again."
            }
            .into(),
        ),
        leaving: st.leaving,
        leave_question: if f.demo { "Leave the Demo?" } else { "Sign Out?" }.into(),
        version: f.version.clone(),
        household_note: Some(if f.demo {
            "The demo's household is Alex's alone, on this device.".into()
        } else {
            format!(
                "Everyone who signs in to {} shares this budget. Add a Device signs another of yours in.",
                if f.server.is_empty() { "this server" } else { &f.server }
            )
        }),
        ask: shown.as_ref().map(|a| ask_item(a, now)),
        banks: banks
            .iter()
            .map(|b| {
                let status = b.status(now);
                BankItem {
                    name: b.name.clone(),
                    title: b.title(),
                    status: status.words,
                    glyph: if status.alarm { "triangle-alert" } else { "landmark" }.into(),
                    waiting: (b.waiting > 0).then_some(b.waiting),
                }
            })
            .collect(),
        has_banks: !banks.is_empty(),
        can_connect: connects && !shown.as_ref().is_some_and(|a| a.is_open()),
        banks_note: Some(
            if connects {
                "Banks and cards connect through Plaid. Their logins are kept on your server, never on this device."
            } else {
                "The demo's card comes from a bank of its own. Connecting a real one needs your server."
            }
            .into(),
        ),
        sync_note: Some(
            if connects {
                "The server asks the bank every hour; Sync Now asks it now."
            } else {
                "The demo's bank has a few days of news for its first two syncs."
            }
            .into(),
        ),
        bank: bank.map(|b| bank_page(st, b, linked, accounts, shown.as_ref(), now)),
        showing_bank: bank.is_some(),
        plaid: plaid(st, asks),
        appearance: match s.appearance {
            None => "system",
            Some(Appearance::Light) => "light",
            Some(Appearance::Dark) => "dark",
        }
        .into(),
        appearance_note: Some("System follows this device's light and dark.".into()),
    }
}

fn bank_page(st: &SettingsState, b: &BankLine, linked: &[LinkedLine], accounts: &[AccountLine], ask: Option<&AskLine>, now: i64) -> BankPage {
    let title = b.title();
    let us = NumberStyle::us();
    let mine = bank_accounts(&b.name, linked);
    let feeds: Vec<FeedItem> = mine
        .iter()
        .map(|l| {
            let kind = if l.opens_as.is_empty() { l.kind.clone() } else { l.opens_as.clone() };
            FeedItem {
                plaid: l.plaid_account_id.clone(),
                title: l.title(),
                about: match l.balance {
                    Some(c) => format!("{} \u{00b7} the bank says {}", l.kind_words(), ark_format::amount(c, &us)),
                    None => l.kind_words(),
                },
                feeds: match (l.waiting, l.ignored) {
                    (true, _) => "Waiting for a choice".into(),
                    (_, true) => "Not imported".into(),
                    _ => format!("Feeds {}", l.account),
                },
                glyph: crate::accounts::kind_glyph(&kind).into(),
                kind,
                waiting: l.waiting,
            }
        })
        .collect();
    let choosing = st
        .choosing
        .as_deref()
        .and_then(|p| mine.iter().find(|l| l.plaid_account_id == p))
        .map(|l| {
            let free = money_core::accounts::linkable(accounts, &l.opens_as);
            FeedChoice {
                title: l.title(),
                can_open: l.waiting,
                opens: l.opens_words(),
                accounts_note: if free.is_empty() {
                    "Every open account is fed by a bank already.".into()
                } else {
                    "Or an account you keep here already, which takes what the bank has from the day it was connected.".into()
                },
                accounts: free
                    .iter()
                    .map(|a| FeedTarget {
                        id: a.id.hex(),
                        name: a.name.clone(),
                        glyph: crate::accounts::kind_glyph(&a.kind).into(),
                        kind: a.kind.clone(),
                    })
                    .collect(),
                ignored: l.ignored,
            }
        });
    let connects = !st.facts.demo;
    BankPage {
        needs_login: b.needs_login,
        login_note_shown: Some(format!("{title} wants its login again before it sends anything new.")),
        failing: !b.needs_login && !b.error.is_empty(),
        error: b.error.clone(),
        failed: ark_format::ago_alone(b.failed_ms, now),
        synced: ark_format::ago_alone(b.synced_ms, now),
        brought: format!("{} new, {} changed", b.added, b.updated),
        removes: b.deleted > 0,
        removed: b.deleted.to_string(),
        locks: b.locked > 0,
        locked: b.locked.to_string(),
        has_feeds: !feeds.is_empty(),
        feeds,
        feeds_note: Some(
            if b.waiting > 0 {
                "Choose what each account feeds: a new account here, one you already keep, or nothing. One you kept by hand takes what the bank has from the day it was connected."
            } else {
                "Each of the bank's accounts feeds an account here, or nothing."
            }
            .into(),
        ),
        ask: ask.filter(|a| a.bank == b.name).map(|a| ask_item(a, now)),
        connects,
        fixed: !connects,
        reconnects: connects && !b.needs_login,
        actions_note: (!connects).then(|| "The demo's bank stays connected: there is no server to ask.".into()),
        disconnecting: st.disconnecting,
        disconnect_question: format!("Disconnect {title}?"),
        choosing,
        title,
    }
}

#[cfg(test)]
mod tests;
