//! Settings' Amazon and Costco sections: where receipts come from, set up
//! from here and kept on the server alone.
//!
//! Each is a call of the server's own over this window's link
//! (`money_domain::setup`), never an intent: what it carries — Amazon's
//! password — goes to the server and nowhere else, and is in no row, so no
//! device has it, this one included. Every answer is the stores as the
//! server has them, who masked, which the sections draw.
//!
//! - **Amazon**: an email, a password and, for two-step verification, the
//!   authenticator's secret, saved for `amazon-orders`; or removed.
//! - **Costco**: read in the server's browser, signed in to costco.com by a
//!   person. "Sign in to Costco" has the server open costco.com's sign-in
//!   there and answers with the browser's page, which this window opens (a
//!   tab reserved at the click in a browser, [`crate::browser`]).
//!
//! The sections ask the server how the stores are each time Settings opens.

use ark::format::Encode;
use arkui::text::text;
use arkui::text_input::text_input;
use iced::widget::{column, container, row};
use iced::{Alignment, Element, Length};
use money_domain::setup::{self as calls, AmazonLogin, BrowserPage, Stores};

use crate::view::{action, bold, dim, quiet};
use crate::{browser, look, App, Message};

/// What the sections do.
#[derive(Debug, Clone)]
pub enum Msg {
    User(String),
    Password(String),
    Otp(String),
    SaveAmazon,
    ForgetAmazon,
    /// costco.com's sign-in opened in the server's browser, and its page here.
    SignInCostco,
}

/// A call made, waiting for its answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Asked {
    Stores,
    BrowserPage,
}

/// What the sections hold: the stores as the server last said, what is
/// typed, and the calls not answered yet.
#[derive(Default)]
pub struct Settings {
    pub stores: Option<Stores>,
    pub user: String,
    pub password: String,
    pub otp: String,
    /// What the server last refused, said under its section.
    pub refused: Option<(&'static str, String)>,
    waiting: Vec<(u64, Asked, &'static str)>,
}

impl App {
    /// The sections' messages.
    pub fn stores(&mut self, m: Msg) {
        let s = &mut self.settings;
        match m {
            Msg::User(v) => s.user = v,
            Msg::Password(v) => s.password = v,
            Msg::Otp(v) => s.otp = v,
            Msg::SaveAmazon => {
                let login = AmazonLogin {
                    username: s.user.clone(),
                    password: s.password.clone(),
                    otp_secret: s.otp.clone(),
                };
                self.call("amazon", calls::AMAZON, &login, Asked::Stores);
            }
            Msg::ForgetAmazon => self.call("amazon", calls::FORGET_AMAZON, &(), Asked::Stores),
            Msg::SignInCostco => self.call("costco", calls::COSTCO_SIGN_IN, &(), Asked::BrowserPage),
        }
    }

    /// How the stores are, asked as Settings opens.
    pub fn ask_stores(&mut self) {
        if self.server.is_some() {
            self.call("", calls::STATUS, &(), Asked::Stores);
        }
    }

    fn call(&mut self, section: &'static str, name: &str, input: &impl Encode, asked: Asked) {
        if self.server.is_none() {
            self.note = "the demo has no server: Amazon and Costco are set up on yours".into();
            return;
        }
        match self.peer.client.request(name, input.to_bytes()) {
            Ok(id) => {
                self.settings.refused = None;
                self.settings.waiting.push((id, asked, section));
            }
            Err(why) => self.settings.refused = Some((section, why)),
        }
    }

    /// The server's answers come in: drawn, a page opened, a refusal said.
    /// Settings open before the link was up ask again once it is.
    pub fn stores_answered(&mut self) {
        let s = &self.settings;
        if self.page == crate::Page::Settings && s.stores.is_none() && s.waiting.is_empty() && self.peer.client.linked {
            self.ask_stores();
        }
        let waiting = std::mem::take(&mut self.settings.waiting);
        for (id, asked, section) in waiting {
            let Some(answer) = self.peer.client.take_reply(id) else {
                self.settings.waiting.push((id, asked, section));
                continue;
            };
            let read = answer.and_then(|b| {
                match asked {
                    Asked::Stores => Stores::from_bytes(&b).map(|s| (Some(s), None)),
                    Asked::BrowserPage => BrowserPage::from_bytes(&b).map(|p| (None, Some(p.url))),
                }
                .map_err(|e| format!("an answer this window does not read: {e}"))
            });
            match read {
                Ok((stores, page)) => {
                    let s = &mut self.settings;
                    if let Some(new) = stores {
                        // A login kept is not kept typed here.
                        if new.amazon.who.is_some() && section == "amazon" {
                            (s.password, s.otp) = (String::new(), String::new());
                        }
                        s.stores = Some(new);
                    }
                    if let Some(url) = page {
                        self.note = match browser::open(&url) {
                            true => "the server's browser is open: sign in to costco.com there".into(),
                            false => format!("open the server's browser at {url} and sign in to costco.com there"),
                        };
                        self.opened.push(url);
                    }
                }
                Err(why) => self.settings.refused = Some((section, why)),
            }
        }
    }

    fn refused_in(&self, section: &str) -> Option<Element<'_, Message>> {
        let (s, why) = self.settings.refused.as_ref()?;
        (*s == section || s.is_empty()).then(|| text(why.as_str()).size(12).style(look::Looks::text(look::Tone::Overspent)).into())
    }

    /// What `store`'s last import did, when there was one.
    fn last(&self, store: &str) -> Option<Element<'_, Message>> {
        let (_, how) = self.settings.stores.as_ref()?.last.iter().find(|(s, _)| s == store)?;
        let line = text(format!("Last sync: {how}")).size(12);
        Some(match how.starts_with("failed") {
            true => line.style(look::Looks::text(look::Tone::Overspent)).into(),
            false => line.into(),
        })
    }

    pub fn amazon_section(&self) -> Element<'_, Message> {
        let s = &self.settings;
        let a = s.stores.as_ref().map(|s| &s.amazon);
        let state = match a {
            _ if self.server.is_none() => "This is the demo: it has no server, so there is nothing to set up.".to_string(),
            None => "Asking the server\u{2026}".into(),
            Some(a) => match &a.who {
                Some(who) if a.otp => format!("Signed in as {who}, with an authenticator secret."),
                Some(who) => format!("Signed in as {who}."),
                None => "Not set up.".into(),
            },
        };
        let mut c = column![
            dim(
                "At each sync the server signs in to Amazon with amazon-orders and brings the last 30 days of \
                 orders in as receipts, and how each was charged: each item filed, and each card charge split by them.",
                12
            ),
            bold(state, 13),
        ]
        .spacing(10);
        if a.is_some_and(|a| !a.program) {
            c = c.push(dim(
                "This server has no amazon-orders program yet (services.money.amazon.package), so nothing is read until it does.",
                11,
            ));
        }
        let field = |placeholder: &str, value: &str, on: fn(String) -> Msg, secure: bool| {
            text_input(placeholder, value)
                .on_input(move |v| Message::Stores(on(v)))
                .on_submit(Message::Stores(Msg::SaveAmazon))
                .secure(secure)
                .size(13)
                .padding([6, 8])
                .style(arkui::style::field)
        };
        if self.server.is_some() {
            c = c
                .push(field("Email", &s.user, Msg::User, false))
                .push(field("Password", &s.password, Msg::Password, true))
                .push(field("Authenticator secret (optional)", &s.otp, Msg::Otp, true))
                .push(dim(
                    "The secret is the key Amazon shows when you add an authenticator app (Login & security \u{2192} \
                     2-Step Verification), so the server can make its own codes. A code texted to your phone can't be used.",
                    11,
                ));
            let mut buttons = row![action("Save", Message::Stores(Msg::SaveAmazon))]
                .spacing(12)
                .align_y(Alignment::Center);
            if a.is_some_and(|a| a.who.is_some()) {
                buttons = buttons.push(quiet("Remove login", Message::Stores(Msg::ForgetAmazon)));
            }
            c = c.push(buttons);
        }
        if let Some(l) = self.last("amazon") {
            c = c.push(l);
        }
        if let Some(e) = self.refused_in("amazon") {
            c = c.push(e);
        }
        c = c.push(dim(
            "Your Amazon login goes to the server and is kept there, in a file only it reads. It is never part of \
             the budget, so it is not synced to this or any device. When Amazon checks the sign-in with a \
             challenge, the server's browser runs it.",
            11,
        ));
        card(c)
    }

    pub fn costco_section(&self) -> Element<'_, Message> {
        let co = self.settings.stores.as_ref().map(|s| &s.costco);
        let state = match co {
            _ if self.server.is_none() => "This is the demo: it has no server, so there is nothing to set up.".to_string(),
            None => "Asking the server\u{2026}".into(),
            Some(c) if !c.browser => "This server has no browser for Costco yet (services.money.browser).".into(),
            Some(_) => "Read in the server's browser, signed in to costco.com there.".into(),
        };
        let mut c = column![
            dim(
                "At each sync the server opens costco.com's Orders & Purchases in its browser and reads the last 90 \
                 days of warehouse and gas receipts there, item by item.",
                12
            ),
            bold(state, 13),
        ]
        .spacing(10);
        if co.is_some_and(|c| c.browser) {
            c = c
                .push(row![action("Sign in to Costco", Message::Stores(Msg::SignInCostco))].align_y(Alignment::Center))
                .push(dim(
                    "This opens costco.com's sign-in in the server's browser, and that browser's page here. Sign in \
                     there once: the browser stays signed in to Costco, and your password goes to Costco alone.",
                    11,
                ));
        }
        if let Some(l) = self.last("costco") {
            c = c.push(l);
        }
        if let Some(e) = self.refused_in("costco") {
            c = c.push(e);
        }
        card(c)
    }
}

fn card<'a>(c: iced::widget::Column<'a, Message>) -> Element<'a, Message> {
    container(c).padding(16).width(Length::Fill).style(arkui::style::card).into()
}

/// What is typed is left out: a password is said nowhere.
impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("Settings")
            .field("stores", &self.stores)
            .field("refused", &self.refused)
            .field("waiting", &self.waiting.len())
            .finish()
    }
}
