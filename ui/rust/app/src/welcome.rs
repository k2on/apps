//! An app's first run, any Ark app's: the pages that say what it is, the
//! server's address, signing in, a server from a link or a scanned code
//! asked about first, and what the server has on its way — and, signed in,
//! "Add a Device", the code another device joins with. Written once here,
//! laid out once ([`welcome`], [`add_device`]), drawn by every toolkit.
//!
//! What decides which page shows and what each button does is the sign-in
//! machine every app shares (`ark_auth::onboarding`), an adapter: it
//! worked out as a [`WelcomePage`] by `ark_auth::welcome`, a
//! [`WelcomeMsg`] taken back into its events there. What is the app's own
//! is its [`WelcomeCopy`]: its name, its mark, its words and its pages.
//! The runtime holds none of it: these are records and a view, pure.

use crate::view;
use crate::view::*;

/// What an app says of itself on its first run.
#[derive(Clone, Debug, PartialEq)]
pub struct WelcomeCopy {
    /// Its name as a person reads it: `Money`.
    pub name: String,
    /// Its mark, by ArkUI's glyph names (`wallet`, `headphones`).
    pub glyph: String,
    /// What it is, in a sentence, under "Welcome to Money".
    pub sentence: String,
    /// The address field's placeholder: the usual server's.
    pub placeholder: String,
    /// Under the address field until there is a problem.
    pub server_note: String,
    /// The demo, with what it is in a sentence; none for an app with none.
    pub demo: Option<String>,
    /// Whether a code can be scanned here to join (a camera).
    pub scan: bool,
    /// Whether another server can be chosen: not in a page its own server
    /// served.
    pub other_servers: bool,
    /// The pages before the server's address, in order; none to start there.
    pub pages: Vec<IntroCopy>,
}

/// One page of what the app is, before its server.
#[derive(Clone, Debug, PartialEq)]
pub struct IntroCopy {
    pub glyph: String,
    pub title: String,
    pub text: String,
}

/// A page of what the app is.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct IntroPage {
    pub glyph: String,
    pub title: String,
    pub text: String,
    /// "1 of 3".
    pub place: Option<String>,
    /// "Continue", or "Get Started" on the last.
    pub next: String,
    /// Skip is offered: not the last.
    pub skippable: bool,
}

/// The server's address.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct ServerPage {
    pub glyph: String,
    pub title: String,
    pub sentence: String,
    pub prompt: String,
    pub typed: String,
    pub note: String,
    pub problem: Option<String>,
    /// It is being checked: nothing can be changed.
    pub checking: bool,
    pub editable: bool,
    pub can_continue: bool,
    /// "Continue", or "Checking…".
    pub go: String,
    pub demo: bool,
    pub demo_note: Option<String>,
    pub scan: bool,
}

/// Signing in, on the server's own page.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct SignInPage {
    pub glyph: String,
    pub title: String,
    pub sentence: String,
    pub status: Option<String>,
    pub busy: bool,
    pub problem: Option<String>,
    /// "Sign In", or "Open the Sign-In Again" while the browser is open.
    pub go: String,
    pub can_sign_in: bool,
    /// "Use Another Server" is offered, and can be pressed.
    pub offers_other: bool,
    pub can_change: bool,
}

/// Signed in, and what the server has on its way.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct OpeningPage {
    pub glyph: String,
    pub title: String,
    /// "Getting your budget from money.example.com…"
    pub sentence: String,
}

/// A server from a link or a scanned code, asked about.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct ConfirmPage {
    pub glyph: String,
    /// "Join money.example.com?", or "Can't Join" for a link that can't be used.
    pub title: String,
    /// Who to trust, or why the link can't be used.
    pub sentence: String,
    pub can_join: bool,
}

/// What the first run shows: one of its pages.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct WelcomePage {
    pub intro: Option<IntroPage>,
    pub server: Option<ServerPage>,
    pub sign_in: Option<SignInPage>,
    pub opening: Option<OpeningPage>,
    pub confirm: Option<ConfirmPage>,
}

impl WelcomePage {
    /// No page: the app itself shows.
    pub fn none() -> WelcomePage {
        WelcomePage {
            intro: None,
            server: None,
            sign_in: None,
            opening: None,
            confirm: None,
        }
    }

    /// Whether one of its pages shows.
    pub fn shows(&self) -> bool {
        *self != WelcomePage::none()
    }
}

/// Everything the first run's pages can be told.
#[derive(Clone, Debug, PartialEq, Eq, ark::twin::Twin)]
pub enum WelcomeMsg {
    /// The next page of what the app is; past the last, the server's.
    Next,
    /// Straight to the server's address.
    Skip,
    /// The address field, as typed.
    Typed {
        text: String,
    },
    /// "Continue" (or Go) on the server's address: it is checked. (Not
    /// `Continue`: a keyword in Swift.)
    Check,
    TryDemo,
    /// "Scan a Code": the camera, for another device's join code.
    ScanCode,
    /// "Sign In" again, where the page offers it.
    SignIn,
    ChangeServer,
    /// "Join" on a server from a link.
    Accept,
    /// "Cancel" on a server from a link.
    Decline,
}

/// The first run, laid out: its one page showing. Each is a first run's
/// page — the app's mark, a title, a sentence — and what can be done
/// there, the default the accent filled.
#[view]
pub fn welcome(p: &WelcomePage) -> View<WelcomeMsg> {
    (
        if let Some(i) = p.intro {
            first_run_page(
                i.glyph,
                i.title,
                i.text,
                (
                    wide_button(i.next, true, true, false, WelcomeMsg::Next),
                    if i.skippable {
                        wide_button("Skip", false, true, false, WelcomeMsg::Skip)
                    },
                    status_line(i.place, false, None::<String>),
                ),
            )
        },
        if let Some(s) = p.server {
            first_run_page(
                s.glyph,
                s.title,
                s.sentence,
                (
                    address_field(
                        "Server Address",
                        s.prompt,
                        s.typed,
                        s.note,
                        s.problem,
                        s.editable,
                        |text| WelcomeMsg::Typed { text },
                        WelcomeMsg::Check,
                    ),
                    wide_button(s.go, true, s.can_continue, s.checking, WelcomeMsg::Check),
                    if s.demo {
                        (
                            wide_button("Try the Demo", false, s.editable, false, WelcomeMsg::TryDemo),
                            status_line(s.demo_note, false, None::<String>),
                        )
                    },
                    if s.scan {
                        wide_button("Scan a Code", false, s.editable, false, WelcomeMsg::ScanCode)
                    },
                ),
            )
        },
        if let Some(s) = p.sign_in {
            first_run_page(
                s.glyph,
                s.title,
                s.sentence,
                (
                    status_line(s.status, s.busy, s.problem),
                    wide_button(s.go, true, s.can_sign_in, false, WelcomeMsg::SignIn),
                    if s.offers_other {
                        wide_button("Use Another Server", false, s.can_change, false, WelcomeMsg::ChangeServer)
                    },
                ),
            )
        },
        if let Some(o) = p.opening {
            first_run_page(o.glyph, o.title, o.sentence, status_line(None::<String>, true, None::<String>))
        },
        if let Some(c) = p.confirm {
            first_run_page(
                c.glyph,
                c.title,
                c.sentence,
                (
                    if c.can_join {
                        wide_button("Join", true, true, false, WelcomeMsg::Accept)
                    },
                    wide_button("Cancel", false, true, false, WelcomeMsg::Decline),
                ),
            )
        },
    )
        .into()
}

/// "Add a Device": the code another device scans to join this server
/// signed in, while it is made, and why it could not be.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct AddDevicePage {
    /// "Scan this with the other device's camera, or open the link there…"
    pub sentence: String,
    /// The code is being made (the server is asked).
    pub making: bool,
    pub problem: Option<String>,
    /// The code made: its link, and its QR code's modules.
    pub shown: bool,
    pub link: String,
    pub size: i64,
    pub dark: Vec<bool>,
    /// The code could not be made: Try Again.
    pub failed: bool,
}

/// What "Add a Device" can be told.
#[derive(Clone, Debug, PartialEq, Eq, ark::twin::Twin)]
pub enum AddDeviceMsg {
    /// The link to the share sheet.
    Share,
    /// Another code made.
    Again,
}

/// "Add a Device", laid out: the code once made, under it what it is for
/// and the link to share; while it is made, a spinner; why not, and Try
/// Again.
#[view]
pub fn add_device(p: &AddDevicePage) -> View<AddDeviceMsg> {
    first_run_page(
        "smartphone",
        "Add a Device",
        p.sentence,
        (
            if p.shown {
                (
                    code_image(p.size, p.dark),
                    wide_button("Share Link", true, true, false, AddDeviceMsg::Share),
                )
            },
            status_line(None::<String>, p.making, p.problem),
            if p.failed {
                wide_button("Try Again", true, true, false, AddDeviceMsg::Again)
            },
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names<M>(v: &View<M>, out: &mut Vec<&'static str>) {
        match v {
            View::List(vs) => vs.iter().for_each(|v| names(v, out)),
            View::Part(p) => {
                let c = p.call();
                out.push(c.name);
                for (_, s) in c.slots {
                    names(s, out)
                }
            }
            View::Each { body, .. } => names(body, out),
            View::When { yes, no, .. } | View::WhenSome { yes, no, .. } => {
                names(yes, out);
                names(no, out)
            }
            View::Layout(l) => l.slots().into_iter().for_each(|s| names(s, out)),
            View::Empty => {}
        }
    }

    fn server() -> ServerPage {
        ServerPage {
            glyph: "wallet".into(),
            title: "Welcome to Money".into(),
            sentence: "A budget.".into(),
            prompt: "money.example.com".into(),
            typed: String::new(),
            note: "Where it is kept.".into(),
            problem: None,
            checking: false,
            editable: true,
            can_continue: false,
            go: "Continue".into(),
            demo: true,
            demo_note: Some("A household of two.".into()),
            scan: false,
        }
    }

    /// The server's page: its field, Continue, the demo and its note; no
    /// Scan a Code where there is no camera. Falsified by drawing the scan
    /// button whatever `scan` says.
    #[test]
    fn the_server_page_offers_what_the_app_has() {
        let p = WelcomePage {
            server: Some(server()),
            ..WelcomePage::none()
        };
        let mut n = vec![];
        names(&welcome::view(&p), &mut n);
        assert_eq!(n, ["FirstRunPage", "AddressField", "WideButton", "WideButton", "StatusLine"]);
        let p = WelcomePage {
            server: Some(ServerPage {
                scan: true,
                demo: false,
                ..server()
            }),
            ..WelcomePage::none()
        };
        let mut n = vec![];
        names(&welcome::view(&p), &mut n);
        assert_eq!(n, ["FirstRunPage", "AddressField", "WideButton", "WideButton"]);
        assert!(p.shows() && !WelcomePage::none().shows());
    }

    /// Every page is recorded for the phones, each behind its `if let`.
    #[test]
    fn the_recording_has_every_page() {
        let mut n = vec![];
        names(&welcome::record(), &mut n);
        assert_eq!(n.iter().filter(|x| **x == "FirstRunPage").count(), 5);
        assert!(n.contains(&"AddressField") && n.contains(&"StatusLine"));
        let mut n = vec![];
        names(&add_device::record(), &mut n);
        assert_eq!(n, ["FirstRunPage", "CodeImage", "WideButton", "StatusLine", "WideButton"]);
    }
}
