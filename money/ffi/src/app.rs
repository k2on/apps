//! money's screens as Ark apps on the phones (docs/ark-app.md): the
//! runtime (`ark_app::Runtime`) behind the bridge, over the session's
//! store. A screen's generated SwiftUI or Compose reads its record
//! ([`AccountsApp::page`]) and sends it messages ([`AccountsApp::send`]);
//! the app's update runs here, its changes go to the peer, its routes and
//! the shell's own work to the [`AppShell`] the phone gave, its module
//! calls to the modules the phone registered (`ark_modules`, UniFFI
//! foreign traits the ArkUI libraries implement).

use std::sync::{Arc, Mutex};

use ark_app::{Host, Inbox, Runtime, Ticket};
use ark_ffi::Refusal;
use ark_modules::{Camera, Clipboard, Haptics, ModuleCall, Modules, Share};
use ark_peer::store::Store;
use money_app::accounts::{Accounts, AccountsMsg, AccountsPage};
use money_domain::{self as domain, routes::Route, MoneyChange};

use crate::Session;

/// What only the phone's shell can do for a screen: open a route on its
/// navigator, carry out a native request ("refresh"), and say the time.
#[uniffi::export(with_foreign)]
pub trait AppShell: Send + Sync {
    fn open(&self, route: Route);
    fn native(&self, name: String);
    /// Milliseconds since the epoch: the demo's day in the demo.
    fn now_ms(&self) -> i64;
}

/// The Accounts tab, running.
#[derive(uniffi::Object)]
pub struct AccountsApp {
    session: Arc<ark_ffi::Session<domain::MoneyView>>,
    shell: Arc<dyn AppShell>,
    runtime: Mutex<Runtime<Accounts>>,
    modules: Mutex<Modules>,
    inbox: Inbox,
    _holds: Vec<ark_ffi::Hold<domain::MoneyView>>,
}

/// The phone, as the runtime's host for one message.
struct Phone<'a> {
    app: &'a AccountsApp,
}

impl Host<Accounts> for Phone<'_> {
    fn read<R>(&self, f: impl FnOnce(&Store<domain::MoneyView>) -> R) -> R {
        self.app.session.read(|st, _| f(st))
    }
    fn now_ms(&self) -> i64 {
        self.app.shell.now_ms()
    }
    fn send(&mut self, change: MoneyChange) -> Result<(), String> {
        match self.app.session.send(|p| p.mutate_encoded(change.name(), change.input())) {
            Ok(_) => Ok(()),
            Err(Refusal::Refused { why }) => Err(why),
        }
    }
    fn open(&mut self, route: Route) {
        self.app.shell.open(route)
    }
    fn ask(&mut self, ticket: Ticket, call: ModuleCall) {
        let modules = self.app.modules.lock().unwrap_or_else(|e| e.into_inner()).clone();
        self.app.inbox.ask(&modules, ticket, call)
    }
    fn native(&mut self, name: &str) {
        self.app.shell.native(name.into())
    }
}

impl AccountsApp {
    /// What modules answered since, taken in.
    fn answers(&self, rt: &mut Runtime<Accounts>) {
        loop {
            let answers = self.inbox.take();
            if answers.is_empty() {
                return;
            }
            for (t, a) in answers {
                rt.answer(t, a, &mut Phone { app: self });
            }
        }
    }
}

#[uniffi::export]
impl AccountsApp {
    /// The tab over `session`, its views held for as long as it lives.
    #[uniffi::constructor]
    pub fn new(session: Arc<Session>, shell: Arc<dyn AppShell>) -> Arc<AccountsApp> {
        let inner = session.inner.clone();
        let rt = Runtime::<Accounts>::new();
        let holds = rt.holds().into_iter().map(|k| inner.hold(k)).collect();
        Arc::new(AccountsApp {
            session: inner,
            shell,
            runtime: Mutex::new(rt),
            modules: Mutex::new(Modules::default()),
            inbox: Inbox::default(),
            _holds: holds,
        })
    }

    /// `msg` taken: the state updated, its changes sent, its routes opened.
    pub fn send(&self, msg: AccountsMsg) {
        let mut rt = self.runtime.lock().unwrap_or_else(|e| e.into_inner());
        rt.dispatch(msg, &mut Phone { app: self });
        self.answers(&mut rt);
    }

    /// What the screen shows now.
    pub fn page(&self) -> AccountsPage {
        let rt = self.runtime.lock().unwrap_or_else(|e| e.into_inner());
        rt.screen(&Phone { app: self })
    }

    /// A number that grows with every message taken: with the session's
    /// versions, what says to read [`AccountsApp::page`] again.
    pub fn version(&self) -> u64 {
        self.runtime.lock().unwrap_or_else(|e| e.into_inner()).version()
    }

    /// The phone's modules, registered once at startup; any may be none.
    pub fn use_modules(
        &self,
        haptics: Option<Arc<dyn Haptics>>,
        clipboard: Option<Arc<dyn Clipboard>>,
        share: Option<Arc<dyn Share>>,
        camera: Option<Arc<dyn Camera>>,
    ) {
        let mut m = self.modules.lock().unwrap_or_else(|e| e.into_inner());
        m.haptics = haptics;
        m.clipboard = clipboard;
        m.share = share;
        m.camera = camera;
    }
}
