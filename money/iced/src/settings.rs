//! Settings, as harken has them: reached from the account button at the
//! top right (its menu's first entry), a page of sections under one title
//! — the account this window is signed in as and the server it is on, the
//! banks (`crate::banks`), and Amazon and Costco (`crate::stores`).

use arkui::account::{Account, Who};
use arkui::{hero, style};
use iced::widget::{column, container, scrollable, text};
use iced::{Element, Length, Padding, Task};

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

    /// Where the signed-in person's picture is loaded from: the server's
    /// `/auth/picture`, as harken's window asks it. None for the demo, while
    /// nobody is signed in, or when their provider has no picture of them.
    pub fn account_picture_url(&self) -> Option<String> {
        let server = self.server.as_ref()?;
        let login = server.login.as_ref()?;
        ark_auth::picture_url(&server.url, &login.user.id, &login.user.picture)
    }

    /// The picture the account button draws over the initials, once it has
    /// arrived.
    pub fn account_picture(&self) -> Option<&iced::widget::image::Handle> {
        self.pictures.handle(&self.account_picture_url()?)
    }

    /// Ask for the signed-in person's picture; the cache knows it was asked
    /// for, so every update after asks nothing.
    pub fn want_picture(&mut self) -> Task<Message> {
        match self.account_picture_url().and_then(|url| self.pictures.want(url)) {
            Some(t) => t.map(Message::Picture),
            None => Task::none(),
        }
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
            container(crate::view::page_title("Settings")).padding(Padding {
                bottom: 4.0,
                ..Padding::ZERO
            }),
            hero::heading("Banks"),
            self.banks_section(),
            hero::heading("Amazon"),
            self.amazon_section(),
            hero::heading("Costco"),
            self.costco_section(),
            hero::heading("Account"),
            account,
        ]
        .width(Length::Fill);
        scrollable(crate::view::under_face(
            container(page).padding(PAD).max_width(980),
            crate::view::page_ground,
        ))
        .id(crate::look::PAGE_TOP)
        .on_scroll(|v| Message::TopScrolled(v.absolute_offset().y))
        .height(Length::Fill)
        .style(style::bars)
        .into()
    }
}
