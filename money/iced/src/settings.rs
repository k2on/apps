//! Settings, as harken has them: reached from the account button at the
//! top right (its menu's first entry), a page of sections under one title
//! — the account this window is signed in as and the server it is on, and
//! the banks (`crate::banks`).

use arkui::account::{Account, Who};
use arkui::{hero, style};
use iced::widget::{column, container, scrollable, text};
use iced::{Element, Length, Padding};

use crate::view::{dim, quiet, PAD};
use crate::{live, App, Message, Page};

impl App {
    /// What the account button says and offers: who is signed in, Settings
    /// and Sign out; the demo is alone, and has Settings only.
    pub fn account(&self) -> Option<Account<Message>> {
        let settings = arkui::menu::Entry::run(arkui::glyphs::LIBRARY, "Settings", Message::Go(Page::Settings));
        let Some(server) = &self.server else {
            let mut a = Account::alone("A demo \u{2014} what you do stays in this window");
            a.entries.push(settings);
            return Some(a);
        };
        let login = server.login.as_ref()?;
        let who = Who {
            name: login.user.name.clone(),
            id: login.user.id.clone(),
        };
        let mut a = Account::signed_in(&who, Message::SignOut);
        a.entries.insert(0, settings);
        Some(a)
    }

    /// The session revoked through the domain and forgotten here; the
    /// replica stays, and what is done now is the same person's when they
    /// sign in again.
    pub fn sign_out(&mut self) {
        let Some(s) = &mut self.server else { return };
        if s.login.take().is_some() {
            let _ = self.peer.client.sign_out();
        }
        if let Some(logins) = &s.logins {
            logins.forget(&s.url);
        }
        self.note = "signed out \u{2014} what you do now stays on this device; reload to sign in".into();
    }

    pub fn view_settings(&self) -> Element<'_, Message> {
        let account: Element<'_, Message> = match &self.server {
            None => dim("A demo: no server, no sign-in. Nothing here leaves this window.", 13).into(),
            Some(s) => {
                let who = match &s.login {
                    Some(l) => format!("Signed in as {} on {}", live::who(l), s.url),
                    None => format!("Not signed in to {}", s.url),
                };
                let mut c = column![text(who).size(14).style(style::faint(0.8))].spacing(12);
                if s.login.is_some() {
                    c = c.push(quiet("Sign out", Message::SignOut));
                }
                c.into()
            }
        };
        let page = column![
            container(text("Settings").size(28).font(hero::BOLD)).padding(Padding {
                bottom: 4.0,
                ..Padding::ZERO
            }),
            hero::heading("Banks"),
            self.banks_section(),
            hero::heading("Account"),
            account,
        ]
        .width(Length::Fill);
        scrollable(container(page).padding(PAD).max_width(980))
            .height(Length::Fill)
            .style(style::bars)
            .into()
    }
}
