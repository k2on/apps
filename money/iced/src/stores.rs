//! Settings' Amazon and Costco sections: where receipts come from, set up
//! from here and kept on the server alone.
//!
//! Each is a call of the server's own over this window's link
//! (`money_domain::setup`), never an intent: what it carries — Amazon's
//! password, Costco's sign-in — goes to the server and nowhere else, and is
//! in no row, so no device has it, this one included. Every answer is the
//! stores as the server has them, who masked, which the sections draw.
//!
//! - **Amazon**: an email, a password and, for two-step verification, the
//!   authenticator's secret, saved for `amazon-orders`; or removed.
//! - **Costco**: an email and a password, which the server signs in with
//!   itself as Costco's page does, and keeps to sign in again. When Costco
//!   asks for more, "Sign in on Costco's page instead" asks the server for
//!   Costco's own sign-in page and opens it (a tab reserved at the click in
//!   a browser, [`crate::browser`]). Signed in there, Costco sends the
//!   browser on to the address its app takes, `msauth://…?code=…`, which a
//!   browser cannot open; that address is pasted here, and the server
//!   trades it for the sign-in it keeps.
//!
//! The sections ask the server how the stores are each time Settings opens.

use ark::format::Encode;
use iced::widget::{column, container, row, text, text_input};
use iced::{Alignment, Element, Length};
use money_domain::setup::{self as calls, AmazonLogin, CostcoAddress, CostcoPage, CostcoPassword, Stores};

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
    CostcoEmail(String),
    CostcoPassword(String),
    /// The server signs in with the email and password.
    SaveCostcoPassword,
    /// Costco's own page instead, its address pasted back.
    SignInCostco,
    Address(String),
    SaveCostco,
    ForgetCostco,
}

/// A call made, waiting for its answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Asked {
    Stores,
    CostcoPage,
}

/// What the sections hold: the stores as the server last said, what is
/// typed, and the calls not answered yet.
#[derive(Default)]
pub struct Settings {
    pub stores: Option<Stores>,
    pub user: String,
    pub password: String,
    pub otp: String,
    pub costco_email: String,
    pub costco_password: String,
    pub address: String,
    /// A Costco sign-in begun here: the address is asked for.
    pub signing_in: bool,
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
            Msg::CostcoEmail(v) => s.costco_email = v,
            Msg::CostcoPassword(v) => s.costco_password = v,
            Msg::SaveCostcoPassword => {
                let login = CostcoPassword {
                    email: s.costco_email.clone(),
                    password: s.costco_password.clone(),
                };
                self.call("costco", calls::COSTCO_PASSWORD, &login, Asked::Stores);
            }
            Msg::SignInCostco => {
                browser::reserve();
                self.call("costco", calls::COSTCO_SIGN_IN, &(), Asked::CostcoPage);
            }
            Msg::Address(v) => s.address = v,
            Msg::SaveCostco => {
                let address = CostcoAddress { address: s.address.clone() };
                self.call("costco", calls::COSTCO, &address, Asked::Stores);
            }
            Msg::ForgetCostco => self.call("costco", calls::FORGET_COSTCO, &(), Asked::Stores),
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
            Err(why) => {
                if asked == Asked::CostcoPage {
                    browser::release();
                }
                self.settings.refused = Some((section, why));
            }
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
                    Asked::CostcoPage => CostcoPage::from_bytes(&b).map(|p| (None, Some(p.url))),
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
                        if new.costco.signed_in && section == "costco" {
                            (s.address, s.signing_in, s.costco_password) = (String::new(), false, String::new());
                        }
                        s.stores = Some(new);
                    }
                    if let Some(url) = page {
                        self.settings.signing_in = true;
                        self.note = match browser::open(&url) {
                            true => "Costco's sign-in is open in your browser".into(),
                            false => "open Costco's sign-in from Settings".into(),
                        };
                        self.opened.push(url);
                    }
                }
                Err(why) => {
                    if asked == Asked::CostcoPage {
                        browser::release();
                    }
                    self.settings.refused = Some((section, why));
                }
            }
        }
    }

    fn refused_in(&self, section: &str) -> Option<Element<'_, Message>> {
        let (s, why) = self.settings.refused.as_ref()?;
        (*s == section || s.is_empty()).then(|| text(why.as_str()).size(12).style(look::Tone::Over.text()).into())
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
                .style(look::input)
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
        if let Some(e) = self.refused_in("amazon") {
            c = c.push(e);
        }
        c = c.push(dim(
            "Your Amazon login goes to the server and is kept there, in a file only it reads. It is never part of \
             the budget, so it is not synced to this or any device.",
            11,
        ));
        card(c)
    }

    pub fn costco_section(&self) -> Element<'_, Message> {
        let s = &self.settings;
        let co = s.stores.as_ref().map(|s| &s.costco);
        let state = match co {
            _ if self.server.is_none() => "This is the demo: it has no server, so there is nothing to set up.".to_string(),
            None => "Asking the server\u{2026}".into(),
            Some(c) if c.file => "Signed in by a token file the server was given.".into(),
            Some(c) => match (&c.who, c.signed_in) {
                (Some(who), true) => format!("Signed in as {who}."),
                (None, true) => "Signed in.".into(),
                _ => "Not set up.".into(),
            },
        };
        let mut c = column![
            dim(
                "At each sync the server reads the last 90 days of warehouse and gas receipts from Costco. It signs \
                 in with your Costco email and password, as costco.com's sign-in page does.",
                12
            ),
            bold(state, 13),
        ]
        .spacing(10);
        if self.server.is_some() {
            let field = |placeholder: &str, value: &str, on: fn(String) -> Msg, secure: bool| {
                text_input(placeholder, value)
                    .on_input(move |v| Message::Stores(on(v)))
                    .on_submit(Message::Stores(Msg::SaveCostcoPassword))
                    .secure(secure)
                    .size(13)
                    .padding([6, 8])
                    .style(look::input)
            };
            let signed_in = co.is_some_and(|c| c.signed_in);
            let mut buttons = row![
                action(
                    if signed_in { "Sign in again" } else { "Sign in" },
                    Message::Stores(Msg::SaveCostcoPassword)
                ),
                quiet("Sign in on Costco's page instead", Message::Stores(Msg::SignInCostco)),
            ]
            .spacing(12)
            .align_y(Alignment::Center);
            if signed_in {
                buttons = buttons.push(quiet("Sign out", Message::Stores(Msg::ForgetCostco)));
            }
            c = c
                .push(field("Email", &s.costco_email, Msg::CostcoEmail, false))
                .push(field("Password", &s.costco_password, Msg::CostcoPassword, true))
                .push(buttons)
                .push(dim(
                    "The password is kept on the server, in a file only it reads, so it can sign in again if Costco \
                     ends the sign-in. If Costco asks for a code as well, sign in on Costco's page instead: then the \
                     password never reaches the server.",
                    11,
                ));
        }
        if s.signing_in {
            c = c
                .push(dim(
                    "Before you sign in on the Costco tab, open its developer tools (F12, or \u{2325}\u{2318}I on a Mac) at \
                     Console. Once you're signed in, Costco tries to hand you to its app and the page seems to stop. \
                     The console then says Failed to launch 'msauth://com.costco.app.android/\u{2026}code=\u{2026}': copy \
                     that address and paste it here within 15 minutes. If it isn't there, use Network with Preserve log \
                     on, sign in again, and copy the location of the last redirect.",
                    11,
                ))
                .push(
                    text_input("msauth://com.costco.app.android/\u{2026}?code=\u{2026}", &s.address)
                        .on_input(|v| Message::Stores(Msg::Address(v)))
                        .on_submit(Message::Stores(Msg::SaveCostco))
                        .size(13)
                        .padding([6, 8])
                        .style(look::input),
                )
                .push(action("Save", Message::Stores(Msg::SaveCostco)));
        }
        if let Some(e) = self.refused_in("costco") {
            c = c.push(e);
        }
        card(c)
    }
}

fn card<'a>(c: iced::widget::Column<'a, Message>) -> Element<'a, Message> {
    container(c).padding(16).width(Length::Fill).style(look::card).into()
}

/// What is typed is left out: a password is said nowhere.
impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("Settings")
            .field("stores", &self.stores)
            .field("signing_in", &self.signing_in)
            .field("refused", &self.refused)
            .field("waiting", &self.waiting.len())
            .finish()
    }
}
