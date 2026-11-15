//! harken's shell: its sections — Home, Library and Search, each its own stack of
//! the pages it opened — a record's page pushed over the section it was
//! opened from, its title the page's. What each section shows is another
//! runtime's (`hosted`); the host says which page is pushed
//! ([`ShellMsg::Push`], [`ShellMsg::Back`]). Over the sections stands the
//! player (its accessory: the now-playing bar, and the full player it
//! grows into), there while something plays ([`ShellMsg::Playing`], the
//! host's word).

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
    /// The bar swiped: on (1) or back (-1) a track.
    Skip {
        by: i64,
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
        }
    }

    fn screen(s: &State, _: &Data<'_, HarkenView>) -> ShellPage {
        ShellPage {
            section: if s.section.is_empty() { "home".into() } else { s.section.clone() },
            showing: s.pushed.is_some(),
            page_title: s.pushed.clone().unwrap_or_default(),
            playing: s.playing,
            player_open: s.player_open,
        }
    }
}
