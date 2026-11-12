//! An app drawn in a window its host draws itself (ui/docs/app.md, "The
//! App's Part of Its Host"): the browser's page (`arkui_web::host`), and the
//! desktop's window once it is hosted the same way. A phone's shell is its
//! toolkit's own and hosts the generated screens by name; a window's host
//! has no shell of its own, so the app's part says the whole window, once,
//! in the views every toolkit draws: its shell's view, what each name it
//! hosts shows, what goes over it (its sheets, its Settings), its address,
//! its appearance, the menu bar's commands and what a field suggests.
//!
//! One message type reaches every screen ([`Windowed::Msg`], the app's own
//! enum of its screens' messages), and the app takes it ([`Windowed::take`])
//! inside the host's loop, as a phone's object per screen sends into
//! `Hosting`. What the toolkit's shell is asked ([`HostedApp::Native`]) a
//! window's host carries out by asking the app again ([`Windowed::native`]):
//! Settings shown is the app's state here, not a scene of the toolkit's.

use std::fmt::Debug;

use crate::host::{Cx, HostedApp, Place};
use crate::shell::CommandMenu;
use crate::view::View;

/// The app's part, as a window's host draws it.
pub trait Windowed: HostedApp {
    /// A message for one of its screens, or for the window (Settings put
    /// away).
    type Msg: Clone + Debug + 'static;

    /// `msg` taken by the screen it is for.
    fn take(&mut self, msg: Self::Msg, cx: &mut Cx<'_, Self>);
    /// The window: its shell's view, whose hosted screens are
    /// [`Windowed::hosted`]'s.
    fn window(&self, place: &dyn Place<Self::Key, Self::Change>, now_ms: i64) -> View<Self::Msg>;
    /// What the shell's `hosted(name)` shows.
    fn hosted(&self, name: &str, place: &dyn Place<Self::Key, Self::Change>, now_ms: i64) -> View<Self::Msg>;
    /// What goes over the window: its sheets and questions.
    fn over(&self, _place: &dyn Place<Self::Key, Self::Change>, _now_ms: i64) -> View<Self::Msg> {
        View::Empty
    }
    /// Settings, while it shows (its panes' view, whose hosted names are
    /// [`Windowed::hosted`]'s), and the message that puts it away.
    fn settings(&self, _place: &dyn Place<Self::Key, Self::Change>, _now_ms: i64) -> Option<(View<Self::Msg>, Self::Msg)> {
        None
    }
    /// The menu bar's commands: their shortcuts are the keyboard's.
    fn commands(&self) -> Vec<CommandMenu<Self::Msg>> {
        vec![]
    }
    /// Where the window is, as the address that brings it back there
    /// (`HostedApp::link` reads it, `ark_router::Address`'s conventions).
    fn address(&self, place: &dyn Place<Self::Key, Self::Change>, now_ms: i64) -> String;
    /// The appearance asked for over the system's (`light`, `dark`), while
    /// one is.
    fn appearance(&self) -> Option<&'static str> {
        None
    }
    /// What the field `field` suggests for what is typed in it.
    fn suggest(&self, _field: &str, _typed: &str, _place: &dyn Place<Self::Key, Self::Change>) -> Vec<String> {
        vec![]
    }
    /// A page to open in a tab or a window of its own (a bank's sign-in),
    /// and the message that says it was opened: what the window cannot
    /// follow is taken as closed once opened.
    fn opens(&self, _place: &dyn Place<Self::Key, Self::Change>, _now_ms: i64) -> Option<(String, Self::Msg)> {
        None
    }
    /// What the toolkit's shell was asked, carried out by the app's part in
    /// a window (Settings shown); signing out is the host's own first.
    fn native(&mut self, _native: Self::Native, _cx: &mut Cx<'_, Self>) {}
}
