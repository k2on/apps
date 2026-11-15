//! harken's shell: its sections — Home and Library, each its own stack of
//! the pages it opened — a record's page pushed over the section it was
//! opened from, its title the page's. What each section shows is another
//! runtime's (`hosted`); the host says which page is pushed
//! ([`ShellMsg::Push`], [`ShellMsg::Back`]).

use ark_app::{App, Command, Data};
use harken_domain::{HarkenChange, HarkenView};

use crate::routes::Route;
use crate::HarkenRequest;

pub mod view;

/// The shell, as `ark_app` runs it.
pub struct HarkenShell;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// The section shown (`home`, `library`); none is Home.
    pub section: String,
    /// The title of the page pushed over it; none while none is.
    pub pushed: Option<String>,
}

#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub enum ShellMsg {
    /// The section of this tag.
    Select { tag: String },
    /// A page pushed over the section shown, under this title.
    Push { title: String },
    /// Back from the page pushed.
    Back,
}

/// Everything the shell shows of its own.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct ShellPage {
    pub section: String,
    pub showing: bool,
    pub page_title: String,
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
        }
    }

    fn screen(s: &State, _: &Data<'_, HarkenView>) -> ShellPage {
        ShellPage {
            section: if s.section.is_empty() { "home".into() } else { s.section.clone() },
            showing: s.pushed.is_some(),
            page_title: s.pushed.clone().unwrap_or_default(),
        }
    }
}
