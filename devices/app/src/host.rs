//! devices' part of its host (`ark_app::host`): the phones' (ark-ffi's
//! `ArkHost`) drives it. Over a session it runs the shell, the status
//! screen and the notices, keeps the shell told who is signed in where, and
//! takes the OS's wakes to the status screen (`HostedApp::woken`'s default:
//! each screen's app is offered them, and only the status screen's has a
//! message for them).

use ark_app::host::{Cx, Has, HostedApp, Link, Running, Screen, Session, ShellAsk};
use devices_core::{DevicesChange, DevicesView, Policy};

use crate::notices::Notices;
use crate::policy::policy_of;
use crate::shell::{DevicesShell, Facts, ShellMsg, GENERAL, STATUS};
use crate::status::{Phase, Status, StatusMsg};
use crate::Route;

/// devices' runtimes over one session.
pub struct Devices {
    pub shell: Screen<DevicesShell>,
    pub status: Screen<Status>,
    pub notices: Screen<Notices>,
    /// The facts last told the shell.
    facts: Option<Facts>,
}

/// Who is signed in where, as the shell says it.
pub fn facts(s: &Session) -> Facts {
    Facts {
        who: if s.name.is_empty() { s.user.clone() } else { s.name.clone() },
        detail: if s.email.is_empty() { s.user.clone() } else { s.email.clone() },
        picture: s.picture.clone(),
        server: s.server.clone(),
        linked: s.linked,
        demo: s.demo,
        version: s.version.clone(),
    }
}

impl Has<DevicesShell> for Devices {
    fn screen(&self) -> &Screen<DevicesShell> {
        &self.shell
    }
    fn screen_mut(&mut self) -> &mut Screen<DevicesShell> {
        &mut self.shell
    }
}

impl Has<Status> for Devices {
    fn screen(&self) -> &Screen<Status> {
        &self.status
    }
    fn screen_mut(&mut self) -> &mut Screen<Status> {
        &mut self.status
    }
}

impl Has<Notices> for Devices {
    fn screen(&self) -> &Screen<Notices> {
        &self.notices
    }
    fn screen_mut(&mut self) -> &mut Screen<Notices> {
        &mut self.notices
    }
}

impl HostedApp for Devices {
    type Key = DevicesView;
    type Change = DevicesChange;
    type Route = Route;
    type Request = ();

    const KEEPS: &'static str = "this phone's policy";

    fn welcome(scan: bool, demo: bool) -> ark_app::welcome::WelcomeCopy {
        crate::welcome::copy(scan, demo)
    }

    fn encode(change: &DevicesChange) -> (&'static str, Vec<u8>) {
        (change.name(), change.input())
    }

    fn open(_: &Session, _: &mut Cx<'_, Self>) -> Devices {
        Devices {
            shell: Screen::new(),
            status: Screen::new(),
            notices: Screen::new(),
            facts: None,
        }
    }

    fn screens(&mut self) -> Vec<&mut dyn Running<Self>> {
        vec![&mut self.shell, &mut self.status, &mut self.notices]
    }

    fn screens_ref(&self) -> Vec<&dyn Running<Self>> {
        vec![&self.shell, &self.status, &self.notices]
    }

    /// What the status screen reads, held from the session's start: a wake
    /// runs inside the screen's first message, before the screen's own
    /// holds would be.
    fn holds(&self) -> Vec<DevicesView> {
        vec![DevicesView::Assigned, DevicesView::Own]
    }

    fn route(&mut self, route: Route, cx: &mut Cx<'_, Self>) {
        match route {
            Route::Status => self.shell.send(ShellMsg::Select { tag: STATUS.into() }, cx),
            Route::Settings => cx.native(ShellAsk::Settings { pane: GENERAL.into() }),
        }
    }

    fn request(&mut self, _: (), _: &mut Cx<'_, Self>) {}

    /// The session's facts told the shell whenever they moved.
    fn follow(&mut self, cx: &mut Cx<'_, Self>) {
        let facts = facts(&cx.place.session());
        if self.facts.as_ref() != Some(&facts) {
            self.facts = Some(facts.clone());
            self.shell.send(ShellMsg::Session { facts }, cx);
        }
    }

    /// The store moved: a policy that came in after the last run (the
    /// replica caught up after it read, or the server changed it while the
    /// app runs) is applied at once, without waiting for a wake.
    fn moved(&mut self, cx: &mut Cx<'_, Self>) {
        let assigned = cx.rows::<Policy>(&DevicesView::Assigned).into_iter().next();
        let s = self.status.state();
        if s.phase == Phase::Idle && s.ran() && &policy_of(assigned.as_ref()) != s.wanted() {
            self.status.send(StatusMsg::Refresh, cx);
        }
    }

    /// A link: what shows put away, then the status (devices has one place).
    fn link(&mut self, _: Link, cx: &mut Cx<'_, Self>) {
        cx.native(ShellAsk::Reset);
        self.shell.send(ShellMsg::Select { tag: STATUS.into() }, cx);
    }

    fn launched(&mut self, _: Vec<String>, _: &mut Cx<'_, Self>) {}
}
