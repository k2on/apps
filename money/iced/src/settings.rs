//! Settings, as harken has them: reached from the account button at the
//! top right (its menu's first entry), a page of sections under one title
//! — the account this window is signed in as and the server it is on, the
//! banks (`crate::banks`), and Amazon and Costco (`crate::stores`).

use arkui::account::{Account, Who};
use arkui::hero;
use iced::widget::column;
use iced::{Element, Length, Task};

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
        let Some(s) = &self.server else { return };
        if s.login.is_some() {
            let _ = self.peer.client.sign_out();
        }
        let _ = self.onboard(ark_auth::onboarding::Event::SignOut);
        self.note = "signed out \u{2014} what you do now stays on this device; reload to sign in".into();
    }

    /// "Add a Device": a one-time code made as this login, on a thread —
    /// the server is asked — for the join link another device scans or
    /// opens (`ark_auth::onboarding::join_link`).
    pub fn add_device(&mut self) -> Task<Message> {
        let Some(s) = &self.server else { return Task::none() };
        let Some(token) = s.login.as_ref().map(|l| l.token.clone()) else {
            return Task::none();
        };
        let server = s.url.clone();
        {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(ark_auth::client::device_code(&server, &token).map(|(code, _)| code));
            });
            Task::perform(
                async move { rx.await.unwrap_or_else(|_| Err("the thread went away".into())) },
                Message::DeviceCode,
            )
        }
    }

    /// The code made, as the link to show; or why not.
    pub fn device_coded(&mut self, made: Result<String, String>) {
        let Some(s) = &mut self.server else { return };
        let app = live::app();
        s.device = Some(made.map(|code| ark_auth::onboarding::join_link(app.scheme, s.url.clone(), Some(code))));
    }

    /// Who is signed in where, as Settings says it (`ShellMsg::Session`):
    /// this window's server and login, or the demo in the window alone.
    pub fn facts(&self) -> money_app::shell::SessionFacts {
        let version = env!("CARGO_PKG_VERSION").to_string();
        let Some(s) = &self.server else {
            return money_app::shell::SessionFacts {
                demo: true,
                today: self.today.clone(),
                version,
                ..Default::default()
            };
        };
        let status = self.peer.client.status();
        money_app::shell::SessionFacts {
            who: s.login.as_ref().map(live::who).unwrap_or_default(),
            login: s.login.as_ref().map(|l| l.user.email.clone()).unwrap_or_default(),
            picture: self.account_picture_url().unwrap_or_default(),
            demo: false,
            today: self.today.clone(),
            server: ark_auth::onboarding::display(&s.url),
            linked: Some(status.linked),
            pending: status.pending as i64,
            version,
            leaves: s.login.is_some(),
            adds_devices: s.login.is_some(),
        }
    }

    /// What the desktop adds to Settings' General (`hosted("settings.more")`):
    /// Add a Device's code once asked for — the shared page
    /// (`ark_app::welcome::add_device`) — its store logins, and how it draws.
    pub fn view_settings(&self) -> Element<'_, Message> {
        let account: Element<'_, Message> = match self.server.as_ref().and_then(|s| s.device.clone()) {
            None => column![].into(),
            Some(made) => {
                let link = made.as_ref().ok().cloned().unwrap_or_default();
                let made = made.and_then(|l| {
                    ark_auth::onboarding::qr_code(l.clone())
                        .map(|q| (l, q))
                        .ok_or_else(|| "the link is too long for a code".to_string())
                });
                let page = ark_auth::first_run::add_device_page(Some(made));
                arkui::view::element(ark_app::welcome::add_device::view(&page).map(move |m| match m {
                    ark_app::welcome::AddDeviceMsg::Share => Message::CopyLink(link.clone()),
                    ark_app::welcome::AddDeviceMsg::Again => Message::AddDevice,
                }))
            }
        };
        let page = column![
            hero::heading("Amazon"),
            self.amazon_section(),
            hero::heading("Costco"),
            self.costco_section(),
            account,
            hero::heading("Display"),
            arkui::renderer::about(),
        ]
        .width(Length::Fill);
        // Inside the pane's own page, which scrolls.
        page.into()
    }
}
