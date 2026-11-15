//! What the shell puts up over everything (app.toml's `[shell] over`): an
//! alert, while one is to be said. The status screen says how its last run
//! went in its own page; nothing puts an alert up yet.

pub mod view;

use ark_app::{App, Command, Data};
use devices_core::{DevicesChange, DevicesView};

use crate::Route;

pub struct Notices;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NoticesState {
    pub alert: Option<String>,
}

/// Everything the notices can be told.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub enum NoticesMsg {
    /// Something to say over everything.
    Say { words: String },
    /// The alert put away.
    Dismiss,
}

/// An alert's words.
#[derive(Clone, Debug, Default, PartialEq, ark::twin::Twin)]
pub struct Alert {
    pub words: String,
}

/// What is put up over everything, while anything is.
#[derive(Clone, Debug, Default, PartialEq, ark::twin::Twin)]
pub struct NoticesPage {
    pub alert: Option<Alert>,
}

impl App for Notices {
    type Key = DevicesView;
    type State = NoticesState;
    type Msg = NoticesMsg;
    type Change = DevicesChange;
    type Route = Route;
    type Request = ();
    type Screen = NoticesPage;

    fn holds(_: &NoticesState) -> Vec<DevicesView> {
        vec![]
    }

    fn update(s: &mut NoticesState, msg: NoticesMsg, _: &Data<'_, DevicesView>) -> Vec<Command<Self>> {
        s.alert = match msg {
            NoticesMsg::Say { words } => Some(words),
            NoticesMsg::Dismiss => None,
        };
        vec![]
    }

    fn screen(s: &NoticesState, _: &Data<'_, DevicesView>) -> NoticesPage {
        NoticesPage {
            alert: s.alert.clone().map(|words| Alert { words }),
        }
    }
}
