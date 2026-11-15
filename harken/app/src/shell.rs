//! harken's shell: its sections — Home, Library and Search, each its own stack of
//! the pages it opened — a record's page pushed over the section it was
//! opened from, its title the page's. What each section shows is another
//! runtime's (`hosted`); the host says which page is pushed
//! ([`ShellMsg::Push`], [`ShellMsg::Back`]). Over the sections stands the
//! player (its accessory: the now-playing bar, and the full player it
//! grows into), there while something plays ([`ShellMsg::Playing`], the
//! host's word).

use ark_app::shell::{Household, Launch};
use ark_app::{App, Command, Data};
use harken_domain::{HarkenChange, HarkenView};

use crate::routes::Route;
use crate::HarkenRequest;

pub mod view;

/// The shell, as `ark_app` runs it.
pub struct HarkenShell;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// The section shown (`home`, `library`, `search`); none is Home.
    pub section: String,
    /// The title of the page pushed over it; none while none is.
    pub pushed: Option<String>,
    /// Something plays: the now-playing bar is up.
    pub playing: bool,
    /// The bar has grown into the full player.
    pub player_open: bool,
    /// Settings' pane shown, by its title; none is General.
    pub pane: String,
    /// What the session says of itself, for Settings and the diagnostics.
    pub facts: Facts,
    /// The appearance asked for over the system's: a launch's.
    pub appearance: Option<Appearance>,
}

/// Light or dark, over the system's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ark::twin::Twin)]
pub enum Appearance {
    Light,
    Dark,
}

/// The sections, by their tags, in the order the shell shows them.
pub const SECTIONS: [&str; 3] = ["home", "library", "search"];

/// Settings' panes, by their titles.
pub const PANES: [&str; 2] = ["General", "Diagnostics"];

/// What Settings and the diagnostics say of the session, as the host's
/// session says it.
#[derive(Clone, Debug, Default, PartialEq, ark::twin::Twin)]
pub struct Facts {
    /// The server, as shown; empty alone.
    pub server: String,
    /// "Connected", "Offline", "Connecting", or "On This Device" alone.
    pub link: String,
    pub version: String,
    /// Changes made here not yet synced, counted.
    pub pending: String,
    /// This device as the session names it; "None" alone.
    pub device: String,
    /// Connections opened, counted.
    pub connections: String,
    /// A server is there to say.
    pub on_server: bool,
    /// Who is signed in (the demo's listener alone), and their picture.
    pub who: String,
    pub picture: String,
    /// Their login, as the account sheet says it under their name.
    pub login: String,
    /// The link: up, down, or not known yet; none alone.
    pub linked: Option<bool>,
}

impl Facts {
    /// The facts of `s`.
    pub fn of(s: &ark_app::host::Session) -> Facts {
        let link = match (s.demo, s.linked) {
            (true, _) => "On This Device",
            (false, Some(true)) => "Connected",
            (false, Some(false)) => "Offline",
            (false, None) => "Connecting",
        };
        Facts {
            server: s
                .server
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .trim_end_matches('/')
                .into(),
            link: link.into(),
            version: s.version.clone(),
            pending: s.pending.to_string(),
            device: if s.device.is_empty() { "None".into() } else { s.device.clone() },
            connections: s.epoch.to_string(),
            on_server: !s.demo,
            who: match () {
                _ if s.demo => "Alex".into(),
                _ if !s.name.is_empty() => s.name.clone(),
                _ => s.user.clone(),
            },
            picture: if s.demo { String::new() } else { s.picture.clone() },
            login: if s.demo { String::new() } else { s.user.clone() },
            linked: if s.demo { None } else { s.linked },
        }
    }
}

#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub enum ShellMsg {
    /// The section of this tag.
    Select {
        tag: String,
    },
    /// A page pushed over the section shown, under this title.
    Push {
        title: String,
    },
    /// Back from the page pushed.
    Back,
    /// Whether anything plays, as the host's player says.
    Playing {
        on: bool,
    },
    /// The now-playing bar grown into the full player, and back.
    OpenPlayer,
    ClosePlayer,
    /// The bar swiped, or the menu's Next and Previous: on (1) or back
    /// (-1) a track.
    Skip {
        by: i64,
    },
    /// The menu's Play and Pause.
    Toggle,
    /// Settings' pane of this title.
    Pane {
        title: String,
    },
    /// What the session says of itself now, as the host reads it.
    Facts {
        facts: Facts,
    },
    /// Who is signed in, pressed: Settings, on General.
    Account,
    /// A launch's arguments (`-screen library`, `-appearance dark`,
    /// `-pane diagnostics`): the section and the look they name.
    Launch {
        args: Vec<String>,
    },
    /// A page shown in the system's browser closed: harken opens none.
    PageClosed {
        url: String,
    },
}

/// Everything the shell shows of its own.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct ShellPage {
    pub section: String,
    pub showing: bool,
    pub page_title: String,
    pub playing: bool,
    pub player_open: bool,
    pub pane: String,
    pub facts: Facts,
    /// Light or dark over the system's, as a launch asked.
    pub appearance: Option<Appearance>,
    /// Who is signed in, as the account face and its sheet say it.
    pub household: Household,
    /// A page shown in the system's browser: harken opens none.
    pub browse: Option<String>,
}

/// Who is listening and where, of the facts the host told the shell.
fn household(f: &Facts) -> Household {
    Household {
        who: f.who.clone(),
        picture: f.picture.clone(),
        detail: if f.on_server {
            f.login.clone()
        } else {
            "The demo's library, on this device alone".into()
        },
        server: (f.on_server && !f.server.is_empty()).then(|| f.server.clone()),
        linked: f.linked,
        demo: !f.on_server,
        ..Household::default()
    }
}

/// A launch's section, pane and look taken.
fn launch(s: &mut State, l: Launch) -> Vec<Command<HarkenShell>> {
    if let Some(a) = l.appearance.as_deref() {
        s.appearance = match a {
            "light" => Some(Appearance::Light),
            "dark" => Some(Appearance::Dark),
            _ => None,
        };
    }
    if let Some(p) = l.pane.as_deref() {
        if let Some(t) = PANES.iter().find(|t| t.eq_ignore_ascii_case(p)) {
            s.pane = (*t).into();
        }
    }
    match l.screen.as_deref() {
        Some("settings") => vec![Command::Request(HarkenRequest::Settings { pane: s.pane.clone() })],
        Some(tag) if SECTIONS.contains(&tag) => {
            if tag != s.section {
                s.pushed = None;
            }
            s.section = tag.into();
            vec![]
        }
        // One of the library's kinds (`-screen albums`): its index, the
        // library's way.
        Some(kind) => match Route::of_kind(kind) {
            Some(r) => vec![Command::Open(r)],
            None => vec![],
        },
        None => vec![],
    }
}

impl App for HarkenShell {
    type Key = HarkenView;
    type State = State;
    type Msg = ShellMsg;
    type Change = HarkenChange;
    type Route = Route;
    type Request = HarkenRequest;
    type Screen = ShellPage;

    fn holds(_: &State) -> Vec<HarkenView> {
        vec![]
    }

    fn update(s: &mut State, msg: ShellMsg, _: &Data<'_, HarkenView>) -> Vec<Command<Self>> {
        match msg {
            // A tab switch finds its section as it was left, but a page
            // pushed belongs to the section it was opened from.
            ShellMsg::Select { tag } => {
                if tag != s.section {
                    s.pushed = None;
                }
                s.section = tag;
                vec![]
            }
            ShellMsg::Push { title } => {
                s.pushed = Some(title);
                vec![]
            }
            ShellMsg::Back => {
                s.pushed = None;
                vec![Command::Back]
            }
            // Nothing plays: nothing to have grown.
            ShellMsg::Playing { on } => {
                s.playing = on;
                s.player_open &= on;
                vec![]
            }
            ShellMsg::OpenPlayer => {
                s.player_open = s.playing;
                vec![]
            }
            ShellMsg::ClosePlayer => {
                s.player_open = false;
                vec![]
            }
            ShellMsg::Skip { by } => vec![Command::Request(HarkenRequest::Skip { by })],
            ShellMsg::Toggle => vec![Command::Request(HarkenRequest::Toggle)],
            ShellMsg::Pane { title } => {
                s.pane = title;
                vec![]
            }
            ShellMsg::Facts { facts } => {
                s.facts = facts;
                vec![]
            }
            ShellMsg::Account => {
                s.pane = PANES[0].into();
                vec![Command::Request(HarkenRequest::Settings { pane: PANES[0].into() })]
            }
            ShellMsg::Launch { args } => launch(s, Launch::parse(&args)),
            ShellMsg::PageClosed { .. } => vec![],
        }
    }

    fn screen(s: &State, _: &Data<'_, HarkenView>) -> ShellPage {
        ShellPage {
            section: if s.section.is_empty() { "home".into() } else { s.section.clone() },
            showing: s.pushed.is_some(),
            page_title: s.pushed.clone().unwrap_or_default(),
            playing: s.playing,
            player_open: s.player_open,
            pane: if s.pane.is_empty() { PANES[0].into() } else { s.pane.clone() },
            facts: s.facts.clone(),
            appearance: s.appearance,
            household: household(&s.facts),
            browse: None,
        }
    }
}
