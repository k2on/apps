//! The person's say over the background (ui/docs/app.md, Background And
//! Notifications): one shared setting, "Notify Me", any app embeds where
//! its settings are — the switch, what it says, and what turning it on and
//! off asks of the device.
//!
//! **Off by default, opt-in per app and per person.** An app runs nothing
//! in the background unless its app.toml declares `[background]`; one that
//! does still registers for push and schedules its periodic sync only once
//! the person turns this on, and stops both when they turn it off:
//!
//! - on: the person asked whether the app may notify them
//!   (`notifications::authorize`); yes, the device registered for push
//!   (its endpoint becomes the session's push row, `ArkProcess::notices`)
//!   and the sync scheduled (`background::set`);
//! - off: unregistered (the push row taken away) and the sync stopped;
//! - at launch ([`NotifyMsg::Load`]): what was set before read back from
//!   the device, and, if on, registered again (an endpoint may change).
//!
//! An app embeds a [`Notify`] in its state, wraps [`NotifyMsg`] in its
//! own message ([`NotifyApp::notify`]), hands each to [`update`], and draws
//! [`notify_me`] over [`page`].

use crate::modules::{background, notifications};
use crate::view;
use crate::view::{switch_row, View};
use crate::{Answer, App, Command};

/// What the setting holds: whether it is on, and why not when the person
/// was asked and said no.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Notify {
    pub on: bool,
    /// The person said no to notices in the system's dialog (or turned
    /// them off in the system's settings): the switch says where to go.
    pub refused: bool,
}

/// What the setting is told.
#[derive(Clone, Debug, PartialEq)]
pub enum NotifyMsg {
    /// At launch: what was set before, read back.
    Load,
    Loaded(Answer<Option<u32>>),
    /// The person turned the switch.
    Turn(bool),
    Allowed(Answer<bool>),
}

/// An app that embeds the setting: how its message carries one of the
/// setting's.
pub trait NotifyApp: App {
    fn notify(msg: NotifyMsg) -> Self::Msg;
}

fn loaded<A: NotifyApp>(a: Answer<Option<u32>>) -> A::Msg {
    A::notify(NotifyMsg::Loaded(a))
}

fn allowed<A: NotifyApp>(a: Answer<bool>) -> A::Msg {
    A::notify(NotifyMsg::Allowed(a))
}

/// `msg` taken into `s`: what is to be asked of the device. `every_s` is
/// how often the app syncs in the background once on.
pub fn update<A: NotifyApp>(s: &mut Notify, msg: NotifyMsg, every_s: u32) -> Vec<Command<A>> {
    let on = |every: u32| vec![Command::ask(notifications::register_push()), Command::ask(background::set(Some(every)))];
    match msg {
        NotifyMsg::Load => vec![Command::ask_then(background::wanted(), loaded::<A>)],
        NotifyMsg::Loaded(Ok(Some(_))) => {
            s.on = true;
            on(every_s)
        }
        NotifyMsg::Loaded(_) => {
            s.on = false;
            vec![]
        }
        NotifyMsg::Turn(true) => vec![Command::ask_then(notifications::authorize(), allowed::<A>)],
        NotifyMsg::Allowed(Ok(true)) => {
            (s.on, s.refused) = (true, false);
            on(every_s)
        }
        NotifyMsg::Allowed(_) => {
            (s.on, s.refused) = (false, true);
            vec![]
        }
        NotifyMsg::Turn(false) => {
            (s.on, s.refused) = (false, false);
            vec![Command::ask(notifications::unregister_push()), Command::ask(background::set(None))]
        }
    }
}

/// What the switch shows.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct NotifyPage {
    pub on: bool,
    /// What it notifies about, in the app's words, or why it cannot.
    pub note: String,
}

/// The page of `s`; `about` is what the app notifies about ("New activity
/// in your household").
pub fn page(s: &Notify, about: &str) -> NotifyPage {
    NotifyPage {
        on: s.on,
        note: match s.refused {
            true => "Notifications are off for this app in Settings.".into(),
            false => about.into(),
        },
    }
}

/// The switch: "Notify Me", what about under it.
#[view]
pub fn notify_me(p: &NotifyPage) -> View<NotifyMsg> {
    switch_row("Notify Me", p.note, p.on, NotifyMsg::Turn)
}
