//! The first run's pages (`ark_app::welcome`, written once for every
//! toolkit) over the sign-in machine ([`crate::onboarding`]): which page
//! shows is where the machine stands, every word on it the machine's own
//! ([`Said`](crate::onboarding::Said)) or the app's ([`WelcomeCopy`]), and
//! a press is taken back into the machine's events. The effects it asks for
//! are the machine's (`Effect`), data the shell carries out as it always
//! has; "Scan a Code" asks the shell's camera, whose answer comes back
//! through [`FirstRun::scanned`].

use ark_app::welcome::{AddDevicePage, ConfirmPage, IntroPage, OpeningPage, ServerPage, SignInPage, WelcomeCopy, WelcomeMsg, WelcomePage};

use crate::onboarding::{display, join_of, AppInfo, Effect, Event, Onboarding, Phase, Qr, Step};

/// What a press asks of the shell.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Asked {
    /// The machine's effects, in order.
    pub effects: Vec<Effect>,
    /// Scan another device's code with the camera; its text to
    /// [`FirstRun::scanned`].
    pub scan: bool,
}

/// An app's first run: the machine, the app's copy, and how far through
/// its pages of what it is the person has come.
#[derive(Debug)]
pub struct FirstRun {
    machine: Onboarding,
    copy: WelcomeCopy,
    /// The page of what the app is that shows; past the last, none.
    intro: usize,
    /// Its pages asked for by their route (`/welcome/`): shown over
    /// wherever the machine stands, until they are walked or skipped.
    asked: bool,
    /// A link for the app itself that came while its first run showed:
    /// handed to the app once the app shows ([`FirstRun::take_link`]).
    link: Option<String>,
}

impl FirstRun {
    /// `app`'s first run, its pages from the start.
    pub fn new(app: AppInfo, copy: WelcomeCopy) -> FirstRun {
        FirstRun {
            machine: Onboarding::new(app),
            copy,
            intro: 0,
            asked: false,
            link: None,
        }
    }

    /// A link for the app itself (a page, a launch) taken, wherever the
    /// first run stands: the app's pages put away, as any link but their
    /// own puts them away, and the link kept until the app shows — so one
    /// that came while the pages or the sign-in showed is not lost. The
    /// newest kept; a sign-in's or a join's link is the machine's, not this.
    pub fn open_link(&mut self, url: String) {
        self.skip_intro();
        self.link = Some(url);
    }

    /// The link kept for the app, once the app itself shows (signed in, or
    /// the demo, no page over it); once. None before: the app asks again as
    /// it comes to show.
    pub fn take_link(&mut self) -> Option<String> {
        match self.page().shows() {
            true => None,
            false => self.link.take(),
        }
    }

    /// The app's pages passed over: a launch that goes straight to a
    /// route, a link or a screenshot's, as a returning person's would.
    pub fn skip_intro(&mut self) {
        self.intro = self.copy.pages.len();
        self.asked = false;
    }

    /// The app's pages from the first, over wherever the machine stands:
    /// their route (`/welcome/`).
    pub fn show_intro(&mut self) {
        self.intro = 0;
        self.asked = !self.copy.pages.is_empty();
    }

    /// The machine, for what it remembers and where it stands.
    pub fn machine(&self) -> &Onboarding {
        &self.machine
    }

    /// One of the machine's events (what an effect came to, a link opened).
    pub fn event(&mut self, e: Event) -> Vec<Effect> {
        // Anything the machine was told past its first page skips the app's.
        // (Unless they were asked for by their route: then they stay.)
        if !self.asked && !matches!(e, Event::Typed { .. }) && !matches!(self.machine.step(), Step::Server { .. }) {
            self.intro = self.copy.pages.len();
        }
        let fx = self.machine.next(e);
        if !self.asked && !matches!(self.machine.step(), Step::Server { .. }) {
            self.intro = self.copy.pages.len();
        }
        fx
    }

    /// A press on a page.
    pub fn send(&mut self, msg: WelcomeMsg) -> Asked {
        let event = match msg {
            WelcomeMsg::Next => {
                self.intro = (self.intro + 1).min(self.copy.pages.len());
                self.asked &= self.intro < self.copy.pages.len();
                return Asked::default();
            }
            WelcomeMsg::Skip => {
                self.skip_intro();
                return Asked::default();
            }
            WelcomeMsg::ScanCode => return Asked { effects: vec![], scan: true },
            WelcomeMsg::Typed { text } => Event::Typed { text },
            WelcomeMsg::Check => Event::Check,
            WelcomeMsg::TryDemo => Event::TryDemo,
            WelcomeMsg::SignIn => Event::OpenSignIn,
            WelcomeMsg::ChangeServer if !self.copy.other_servers => return Asked::default(),
            WelcomeMsg::ChangeServer => Event::ChangeServer,
            WelcomeMsg::Accept => Event::Accept,
            WelcomeMsg::Decline => Event::Decline,
        };
        Asked {
            effects: self.event(event),
            scan: false,
        }
    }

    /// What the camera read: a join link is asked about; anything else, or
    /// nothing, is nothing.
    pub fn scanned(&mut self, text: Option<String>) -> Vec<Effect> {
        match text.and_then(join_of) {
            Some(j) => self.event(Event::Joined {
                server: j.server,
                code: j.code,
            }),
            None => vec![],
        }
    }

    /// The app's page of what it is that shows.
    fn intro_page(&self) -> WelcomePage {
        let c = &self.copy;
        let none = WelcomePage::none();
        let p = &c.pages[self.intro];
        let last = self.intro + 1 == c.pages.len();
        WelcomePage {
            intro: Some(IntroPage {
                glyph: p.glyph.clone(),
                title: p.title.clone(),
                text: p.text.clone(),
                place: (c.pages.len() > 1).then(|| format!("{} of {}", self.intro + 1, c.pages.len())),
                next: if last { "Get Started" } else { "Continue" }.into(),
                skippable: !last,
            }),
            ..none
        }
    }

    /// The page that shows now; none while the app itself does.
    pub fn page(&self) -> WelcomePage {
        let c = &self.copy;
        let said = self.machine.said();
        let none = WelcomePage::none();
        match self.machine.step() {
            _ if self.asked && self.intro < c.pages.len() => self.intro_page(),
            Step::Server { .. } if self.intro < c.pages.len() => self.intro_page(),
            Step::Server { typed, checking, error } => WelcomePage {
                server: Some(ServerPage {
                    glyph: c.glyph.clone(),
                    title: format!("Welcome to {}", c.name),
                    sentence: c.sentence.clone(),
                    prompt: c.placeholder.clone(),
                    note: c.server_note.clone(),
                    can_continue: !checking && !typed.trim().is_empty(),
                    typed,
                    problem: error,
                    checking,
                    editable: !checking,
                    go: said.status.unwrap_or_else(|| "Continue".into()),
                    demo: c.demo.is_some(),
                    demo_note: c.demo.clone(),
                    scan: c.scan,
                }),
                ..none
            },
            Step::SignIn { server, phase, error } => WelcomePage {
                sign_in: Some(SignInPage {
                    glyph: "shield-check".into(),
                    title: "Sign In".into(),
                    sentence: format!("You'll sign in on {}'s own page, and come straight back here.", display(&server)),
                    status: said.status,
                    busy: phase == Phase::Exchanging,
                    problem: error,
                    go: match phase {
                        Phase::Browser => "Open the Sign-In Again",
                        _ => "Sign In",
                    }
                    .into(),
                    can_sign_in: phase != Phase::Exchanging,
                    offers_other: c.other_servers,
                    can_change: phase != Phase::Exchanging,
                }),
                ..none
            },
            Step::Opening { .. } => WelcomePage {
                opening: Some(OpeningPage {
                    glyph: c.glyph.clone(),
                    title: c.name.clone(),
                    sentence: said.status.unwrap_or_default(),
                }),
                ..none
            },
            Step::Confirm { error, .. } => WelcomePage {
                confirm: Some(ConfirmPage {
                    glyph: "link".into(),
                    can_join: error.is_none(),
                    title: said.question.unwrap_or_else(|| "Can't Join".into()),
                    sentence: error.or(said.status).unwrap_or_default(),
                }),
                ..none
            },
            Step::Running { .. } | Step::Demo => none,
        }
    }
}

/// "Add a Device" as it stands: the code being made (`None`), made (its
/// link and QR code), or why it could not be.
pub fn add_device_page(made: Option<Result<(String, Qr), String>>) -> AddDevicePage {
    let blank = AddDevicePage {
        sentence: "Scan this with the other device's camera, or open the link there. It works once, for ten minutes.".into(),
        making: false,
        problem: None,
        shown: false,
        link: String::new(),
        size: 0,
        dark: vec![],
        failed: false,
    };
    match made {
        None => AddDevicePage { making: true, ..blank },
        Some(Ok((link, qr))) => AddDevicePage {
            shown: true,
            link,
            size: qr.size as i64,
            dark: qr.dark,
            ..blank
        },
        Some(Err(why)) => AddDevicePage {
            problem: Some(format!("Couldn't make a code: {}.", why.trim_end_matches('.'))),
            failed: true,
            ..blank
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onboarding::{Found, Remembered};
    use ark_app::welcome::IntroCopy;

    fn copy(pages: usize) -> WelcomeCopy {
        WelcomeCopy {
            name: "Money".into(),
            glyph: "wallet".into(),
            sentence: "Your household's budget.".into(),
            placeholder: "money.example.com".into(),
            server_note: "Where it is kept.".into(),
            demo: Some("A household of two.".into()),
            scan: true,
            other_servers: true,
            pages: (0..pages)
                .map(|i| IntroCopy {
                    glyph: "star".into(),
                    title: format!("Page {i}"),
                    text: "What it is.".into(),
                })
                .collect(),
        }
    }

    fn app() -> AppInfo {
        AppInfo {
            name: "Money".into(),
            scheme: "money".into(),
            demo: true,
            what: "your budget".into(),
        }
    }

    fn launched(f: &mut FirstRun, r: Remembered) -> Vec<Effect> {
        f.event(Event::Launched {
            remembered: r,
            code: None,
            returned: false,
        })
    }

    /// The app's pages, then its server's: Continue walks them, Get
    /// Started ends them, Skip leaves them. Falsified by showing the
    /// server's page first.
    #[test]
    fn the_apps_pages_come_before_its_server() {
        let mut f = FirstRun::new(app(), copy(2));
        launched(&mut f, Remembered::default());
        let i = f.page().intro.unwrap();
        assert_eq!(
            (i.title.as_str(), i.place.as_deref(), i.next.as_str(), i.skippable),
            ("Page 0", Some("1 of 2"), "Continue", true)
        );
        f.send(WelcomeMsg::Next);
        let i = f.page().intro.unwrap();
        assert_eq!((i.next.as_str(), i.skippable), ("Get Started", false));
        f.send(WelcomeMsg::Next);
        let s = f.page().server.unwrap();
        assert_eq!(s.title, "Welcome to Money");
        assert!(s.demo && s.scan && !s.can_continue);
        let mut f = FirstRun::new(app(), copy(3));
        launched(&mut f, Remembered::default());
        f.send(WelcomeMsg::Skip);
        assert!(f.page().server.is_some());
    }

    /// The server's address, checked, found, and the sign-in gone to by
    /// itself: each page the machine's words. Falsified by a page that
    /// says Continue while it checks.
    #[test]
    fn the_server_then_signing_in() {
        let mut f = FirstRun::new(app(), copy(0));
        launched(&mut f, Remembered::default());
        f.send(WelcomeMsg::Typed {
            text: "money.example.com".into(),
        });
        assert!(f.page().server.unwrap().can_continue);
        let asked = f.send(WelcomeMsg::Check);
        assert_eq!(
            asked.effects,
            vec![Effect::FindServer {
                typed: "money.example.com".into()
            }]
        );
        let s = f.page().server.unwrap();
        assert_eq!((s.go.as_str(), s.checking, s.editable, s.can_continue), ("Checking…", true, false, false));
        let fx = f.event(Event::Checked {
            found: Found::Server {
                url: "https://money.example.com".into(),
                newer: false,
            },
        });
        assert!(fx.iter().any(|e| matches!(e, Effect::Browse { .. })));
        let s = f.page().sign_in.unwrap();
        assert_eq!(s.status.as_deref(), Some("Waiting for the sign-in…"));
        assert_eq!(s.go, "Open the Sign-In Again");
        assert!(s.sentence.contains("money.example.com's own page"));
        f.event(Event::Code { code: "c".into() });
        let s = f.page().sign_in.unwrap();
        assert!(s.busy && !s.can_sign_in && !s.can_change);
        assert_eq!(s.status.as_deref(), Some("Signing in…"));
        f.event(Event::SignedIn { login: Default::default() });
        assert_eq!(f.page().opening.unwrap().sentence, "Getting your budget from money.example.com…");
        f.event(Event::Synced);
        assert!(!f.page().shows(), "the app itself");
    }

    /// A scanned join code is asked about; Join signs in with it, Cancel
    /// goes back; a scan that is no join link is nothing. Falsified by
    /// joining without asking.
    #[test]
    fn a_scanned_code_is_asked_about() {
        let mut f = FirstRun::new(app(), copy(2));
        launched(&mut f, Remembered::default());
        assert!(f.send(WelcomeMsg::ScanCode).scan);
        assert!(f.scanned(Some("hello".into())).is_empty());
        assert!(f.page().intro.is_some(), "nothing happened");
        f.scanned(Some("money://join?server=https%3A%2F%2Fmoney.example.com&code=K1".into()));
        let c = f.page().confirm.unwrap();
        assert_eq!(c.title, "Join money.example.com?");
        assert!(c.can_join);
        let asked = f.send(WelcomeMsg::Decline);
        assert!(asked.effects.is_empty());
        assert!(f.page().server.is_some(), "back to the server's address, its pages done");
        f.scanned(Some("money://join?server=http%3A%2F%2Fmoney.example.com".into()));
        let c = f.page().confirm.unwrap();
        assert!(!c.can_join && c.title == "Can't Join" && c.sentence.contains("https"));
    }

    /// A launch that goes straight to a route or a screenshot skips the
    /// pages; their route shows them over the app itself, and walking or
    /// skipping them puts them away. Falsified by the pages standing in
    /// front of the server's address after `skip_intro`.
    #[test]
    fn the_pages_are_skipped_by_a_launch_and_shown_by_their_route() {
        let mut f = FirstRun::new(app(), copy(2));
        f.skip_intro();
        launched(&mut f, Remembered::default());
        assert!(f.page().server.is_some(), "straight to the server's address");
        f.send(WelcomeMsg::TryDemo);
        assert!(!f.page().shows(), "the app itself");
        f.show_intro();
        assert_eq!(f.page().intro.unwrap().title, "Page 0", "over the app");
        f.send(WelcomeMsg::Next);
        assert_eq!(f.page().intro.unwrap().title, "Page 1");
        f.send(WelcomeMsg::Next);
        assert!(!f.page().shows(), "walked: the app again");
        f.show_intro();
        f.send(WelcomeMsg::Skip);
        assert!(!f.page().shows());
    }

    /// A link that comes while the pages show puts them away and waits for
    /// the app: none while the server's address or the sign-in shows, then
    /// the link, once, as the app shows. Falsified by handing it over at
    /// once: the app was not there to take it, and it was lost.
    #[test]
    fn a_link_that_comes_during_the_first_run_waits_for_the_app() {
        let mut f = FirstRun::new(app(), copy(2));
        launched(&mut f, Remembered::default());
        assert!(f.page().intro.is_some(), "the pages show");
        f.open_link("money://budget/".into());
        assert!(f.page().intro.is_none() && f.page().server.is_some(), "the pages put away");
        assert_eq!(f.take_link(), None, "the app is not there yet");
        f.send(WelcomeMsg::TryDemo);
        assert_eq!(f.take_link().as_deref(), Some("money://budget/"), "the app shows: the link");
        assert_eq!(f.take_link(), None, "once");
        // Over the app: their route shows the pages; a link replaces them.
        f.show_intro();
        f.open_link("money://insights/".into());
        assert!(!f.page().shows());
        assert_eq!(f.take_link().as_deref(), Some("money://insights/"));
    }

    #[test]
    fn the_demo_is_no_page() {
        let mut f = FirstRun::new(app(), copy(0));
        launched(&mut f, Remembered::default());
        let fx = f.send(WelcomeMsg::TryDemo).effects;
        assert!(fx.contains(&Effect::OpenDemo));
        assert_eq!(f.page(), WelcomePage::none());
    }

    #[test]
    fn add_a_device_while_made_and_once_made() {
        assert!(add_device_page(None).making);
        let p = add_device_page(Some(Ok((
            "money://join?server=a".into(),
            Qr {
                size: 21,
                dark: vec![true; 441],
            },
        ))));
        assert!(p.shown && p.size == 21 && !p.making && !p.failed);
        let p = add_device_page(Some(Err("Forbidden.".into())));
        assert_eq!(p.problem.as_deref(), Some("Couldn't make a code: Forbidden."));
        assert!(p.failed);
    }
}
