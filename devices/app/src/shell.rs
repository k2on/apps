//! devices' shell: what the app library's shell (ark-app-android's
//! `ArkShell`) draws around the status screen — one section, who is signed
//! in where, and Settings with its one pane. Its host tells it the
//! session's facts ([`ShellMsg::Session`]).

pub mod view;

use ark_app::shell::Household;
use ark_app::{App, Command, Data};
use devices_core::{DevicesChange, DevicesView};

use crate::Route;

/// The shell's one section.
pub const STATUS: &str = "status";
/// Settings' one pane.
pub const GENERAL: &str = "General";

pub struct DevicesShell;

/// Light or dark, over the system's. devices asks for neither: the system's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ark::twin::Twin)]
pub enum Appearance {
    Light,
    Dark,
}

/// Who is signed in where, as the host reads the session.
#[derive(Clone, Debug, Default, PartialEq, ark::twin::Twin)]
pub struct Facts {
    pub who: String,
    /// Their email or login.
    pub detail: String,
    pub picture: String,
    pub server: String,
    pub linked: Option<bool>,
    pub demo: bool,
    pub version: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShellState {
    pub section: String,
    pub pane: String,
    pub facts: Facts,
}

impl Default for ShellState {
    fn default() -> Self {
        ShellState {
            section: STATUS.into(),
            pane: GENERAL.into(),
            facts: Facts::default(),
        }
    }
}

/// Everything the shell can be told.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub enum ShellMsg {
    /// A section shown (there is one).
    Select { tag: String },
    /// Settings' pane titled `title` shown.
    Pane { title: String },
    /// The session's facts, as the host read them.
    Session { facts: Facts },
    /// A page the shell opened in the browser closed (none is opened yet).
    PageClosed { url: String },
}

/// What the shell shows.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct ShellPage {
    pub section: String,
    pub pane: String,
    /// None: the system's.
    pub appearance: Option<Appearance>,
    /// Who is using the phone and where: the account sheet's.
    pub household: Household,
    /// A page to open in the browser, while one is (none yet).
    pub browse: Option<String>,
    pub who: String,
    pub detail: String,
    pub server: String,
    pub version: String,
}

/// Who is signed in where, as the account sheet draws them.
pub fn household(f: &Facts) -> Household {
    Household {
        who: f.who.clone(),
        picture: f.picture.clone(),
        detail: f.detail.clone(),
        server: (!f.demo && !f.server.is_empty()).then(|| f.server.clone()),
        linked: f.linked,
        demo: f.demo,
        ..Household::default()
    }
}

impl App for DevicesShell {
    type Key = DevicesView;
    type State = ShellState;
    type Msg = ShellMsg;
    type Change = DevicesChange;
    type Route = Route;
    type Request = ();
    type Screen = ShellPage;

    fn holds(_: &ShellState) -> Vec<DevicesView> {
        vec![]
    }

    fn update(s: &mut ShellState, msg: ShellMsg, _: &Data<'_, DevicesView>) -> Vec<Command<Self>> {
        match msg {
            ShellMsg::Select { tag } => s.section = tag,
            ShellMsg::Pane { title } => s.pane = title,
            ShellMsg::Session { facts } => s.facts = facts,
            ShellMsg::PageClosed { .. } => {}
        }
        vec![]
    }

    fn screen(s: &ShellState, _: &Data<'_, DevicesView>) -> ShellPage {
        let f = &s.facts;
        ShellPage {
            section: s.section.clone(),
            pane: s.pane.clone(),
            appearance: None,
            household: household(f),
            browse: None,
            who: f.who.clone(),
            detail: f.detail.clone(),
            server: if f.server.is_empty() { "None".into() } else { f.server.clone() },
            version: f.version.clone(),
        }
    }
}
