//! money's screens as Ark apps on the phones (ui/docs/app.md): the
//! runtime (`ark_app::Runtime`) behind the bridge, over the session's
//! store. A screen's generated SwiftUI or Compose reads its record
//! ([`AccountsApp::page`]) and sends it messages ([`AccountsApp::send`]);
//! the app's update runs here, its changes go to the peer, its routes and
//! the shell's own work to the [`AppShell`] the phone gave, its module
//! calls to the modules the phone registered (money/app.toml: each a
//! crate under modules/, a UniFFI foreign trait its swift/ and kotlin/
//! implement).

use std::sync::{Arc, Mutex};

use ark_app::{Host, Inbox, Runtime, Ticket};
use ark_app::{ModuleCall, Modules};
use ark_clipboard::Clipboard;
use ark_ffi::Refusal;
use ark_haptics::Haptics;
use ark_peer::store::Store;
use ark_share::Share;
use money_app::accounts::{Accounts, AccountsMsg, AccountsPage};
use money_app::shell::{MoneyShell, ShellMsg, ShellPage};
use money_domain::{self as domain, routes::Route, MoneyChange};

use crate::Session;

/// What only the phone's shell can do for a screen: open a route on its
/// navigator, carry out a native request ("refresh", "reconcile <id>"),
/// and say the time and the session's day.
#[uniffi::export(with_foreign)]
pub trait AppShell: Send + Sync {
    /// Not `open`: UniFFI writes a vtable field of that name quoted into its C
    /// header for Swift, which clang cannot read.
    fn open_route(&self, route: Route);
    fn native(&self, name: String);
    /// Milliseconds since the epoch, by the device's clock.
    fn now_ms(&self) -> i64;
    /// The session's day (`2026-10-26`): the demo's day in the demo, today
    /// otherwise — what a new account is dated with.
    fn today(&self) -> String;
}

/// The time a screen is worked out at: the device's clock on the session's
/// day, and noon of that day when the clock is on another (the demo's
/// fixed day), so what is dated today is dated with the session's day.
pub fn session_now(clock_ms: i64, today: &str) -> i64 {
    if domain::dates::day_of_ms(clock_ms) == today {
        return clock_ms;
    }
    match domain::dates::parse_day(today) {
        Some((y, m, d)) => domain::dates::days_from_civil(y, m, d) * 86_400_000 + 43_200_000,
        None => clock_ms,
    }
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
        session_now(self.app.shell.now_ms(), &self.app.shell.today())
    }
    fn send(&mut self, change: MoneyChange) -> Result<(), String> {
        match self.app.session.send(|p| p.mutate_encoded(change.name(), change.input())) {
            Ok(_) => Ok(()),
            Err(Refusal::Refused { why }) => Err(why),
        }
    }
    fn open(&mut self, route: Route) {
        self.app.shell.open_route(route)
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

    /// The phone's modules, the ones money opts into (money/app.toml),
    /// registered once at startup; any may be none.
    pub fn use_modules(&self, haptics: Option<Arc<dyn Haptics>>, clipboard: Option<Arc<dyn Clipboard>>, share: Option<Arc<dyn Share>>) {
        let mut m = self.modules.lock().unwrap_or_else(|e| e.into_inner());
        m.set(haptics).set(clipboard).set(share);
    }
}

/// money's shell, running: its sections, toolbar, inspector, search and
/// commands (money/app/src/shell). Its generated SwiftUI and Compose
/// (`MoneyShellView`, `MoneySettingsView`, `MoneyCommands`) read its page
/// and send it messages; a command only a hosted screen can carry out
/// goes to the [`AppShell`]'s `native` by name.
#[derive(uniffi::Object)]
pub struct ShellApp {
    session: Arc<ark_ffi::Session<domain::MoneyView>>,
    shell: Arc<dyn AppShell>,
    runtime: Mutex<Runtime<MoneyShell>>,
    _holds: Vec<ark_ffi::Hold<domain::MoneyView>>,
}

/// The phone, as the shell's host.
struct Window<'a> {
    app: &'a ShellApp,
}

impl Host<MoneyShell> for Window<'_> {
    fn read<R>(&self, f: impl FnOnce(&Store<domain::MoneyView>) -> R) -> R {
        self.app.session.read(|st, _| f(st))
    }
    fn now_ms(&self) -> i64 {
        session_now(self.app.shell.now_ms(), &self.app.shell.today())
    }
    fn send(&mut self, change: MoneyChange) -> Result<(), String> {
        match self.app.session.send(|p| p.mutate_encoded(change.name(), change.input())) {
            Ok(_) => Ok(()),
            Err(Refusal::Refused { why }) => Err(why),
        }
    }
    fn open(&mut self, route: Route) {
        self.app.shell.open_route(route)
    }
    fn ask(&mut self, _: Ticket, _: ModuleCall) {}
    fn native(&mut self, name: &str) {
        self.app.shell.native(name.into())
    }
}

#[uniffi::export]
impl ShellApp {
    /// The shell over `session`, its views held for as long as it lives.
    #[uniffi::constructor]
    pub fn new(session: Arc<Session>, shell: Arc<dyn AppShell>) -> Arc<ShellApp> {
        let inner = session.inner.clone();
        let rt = Runtime::<MoneyShell>::new();
        let holds = rt.holds().into_iter().map(|k| inner.hold(k)).collect();
        Arc::new(ShellApp {
            session: inner,
            shell,
            runtime: Mutex::new(rt),
            _holds: holds,
        })
    }

    /// `msg` taken.
    pub fn send(&self, msg: ShellMsg) {
        let mut rt = self.runtime.lock().unwrap_or_else(|e| e.into_inner());
        rt.dispatch(msg, &mut Window { app: self });
    }

    /// What the shell shows now.
    pub fn page(&self) -> ShellPage {
        let rt = self.runtime.lock().unwrap_or_else(|e| e.into_inner());
        rt.screen(&Window { app: self })
    }

    /// A number that grows with every message taken.
    pub fn version(&self) -> u64 {
        self.runtime.lock().unwrap_or_else(|e| e.into_inner()).version()
    }
}

#[cfg(test)]
mod tests {
    use super::session_now;

    /// On the session's own day the clock is the time; on another (the
    /// demo's fixed day), noon of the session's day. Falsified by the
    /// device's clock alone: an account added in the demo was dated with
    /// the phone's day, not the demo's.
    #[test]
    fn the_time_is_on_the_sessions_day() {
        let noon_26 = session_now(0, "2026-10-26");
        assert_eq!(session_now(noon_26 + 3_600_000, "2026-10-26"), noon_26 + 3_600_000);
        assert_eq!(session_now(noon_26 + 30 * 86_400_000, "2026-10-26"), noon_26);
        assert_eq!(money_domain::dates::day_of_ms(session_now(0, "2026-10-26")), "2026-10-26");
    }
}
