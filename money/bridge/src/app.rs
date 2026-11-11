//! money's screens as Ark apps on the phones (ui/docs/app.md): the
//! runtime (`ark_app::Runtime`) behind the bridge, over the session's
//! store. A screen's generated SwiftUI or Compose reads its record
//! (`AccountsApp::page`) and sends it messages (`AccountsApp::send`); the
//! app's update runs here, its changes go to the peer, its routes and its
//! typed requests ([`MoneyRequest`]) to the [`AppShell`] the phone gave,
//! its modules' operations to the modules the phone registered
//! ([`Devices`]: money/app.toml's, each a crate under modules/, a UniFFI
//! foreign trait its swift/ and kotlin/ implement).
//!
//! Every screen is the same object over its own app ([`Running`]), written
//! once by [`screen!`]: UniFFI exports no generic type.

use std::sync::{Arc, Mutex};

use ark_app::{App, Ask, Host, Inbox, Modules, Runtime, Ticket};
use ark_camera::Camera;
use ark_clipboard::phone::Clipboard;
use ark_ffi::Refusal;
use ark_haptics::phone::Haptics;
use ark_share::phone::Share;
use money_app::accounts::{Accounts, AccountsMsg};
use money_app::budget::{Budget, BudgetMsg};
use money_app::insights::{Insights, InsightsMsg};
use money_app::receipts::{Receipts, ReceiptsMsg};
use money_app::register::{Register, RegisterMsg};
use money_app::routes::Route;
use money_app::sheets::{Sheets, SheetsMsg};
use money_app::shell::{MoneyShell, ShellMsg};
use money_app::transactions::{Transactions, TransactionsMsg};
use money_core::{self as domain, MoneyChange};

use crate::*;
use ark_ffi::Twin;
use money_app::MoneyRequest;

/// What only the phone's shell can do for a screen: open a route on its
/// navigator, carry out a typed request (the bank sync, a sheet still drawn
/// by hand, a message for another screen's runtime), and say the time and
/// the session's day.
#[uniffi::export(with_foreign)]
pub trait AppShell: Send + Sync {
    /// Not `open`: UniFFI writes a vtable field of that name quoted into its C
    /// header for Swift, which clang cannot read.
    fn open_route(&self, route: RouteFfi);
    fn request(&self, request: MoneyRequestFfi);
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

/// The phone's device modules, the ones money lists (money/app.toml), made
/// once at startup and handed to every screen ([`AccountsApp::use_devices`]
/// …); any may be none. The scanner is the camera's: its eyes and its warp
/// are Rust's (ark-scanner), its frames and shutter the camera module's.
#[derive(uniffi::Object)]
pub struct Devices {
    modules: Modules,
}

#[uniffi::export]
impl Devices {
    #[uniffi::constructor]
    pub fn new(
        haptics: Option<Arc<dyn Haptics>>,
        clipboard: Option<Arc<dyn Clipboard>>,
        share: Option<Arc<dyn Share>>,
        camera: Option<Arc<dyn Camera>>,
    ) -> Arc<Devices> {
        let mut m = Modules::new();
        m.set(haptics.map(ark_haptics::phone::port))
            .set(clipboard.map(ark_clipboard::phone::port))
            .set(share.map(ark_share::phone::port))
            .set(camera.clone())
            .set(camera.map(ark_scanner::over_camera));
        Arc::new(Devices { modules: m })
    }
}

/// One screen's app running over the session: its runtime, the views its
/// state names held (again after every message, as `App::holds` reads the
/// state), the modules registered, and the answers that came later.
pub struct Running<A: App<Key = domain::MoneyView, Change = MoneyChange, Route = Route, Request = MoneyRequest>> {
    session: Arc<ark_ffi::Session<domain::MoneyView>>,
    shell: Arc<dyn AppShell>,
    runtime: Mutex<Runtime<A>>,
    modules: Mutex<Modules>,
    inbox: Inbox<A::Msg>,
    holds: Mutex<(Vec<domain::MoneyView>, Vec<ark_ffi::Hold<domain::MoneyView>>)>,
}

/// The phone, as the runtime's host for one message.
struct Phone<'a, A: App<Key = domain::MoneyView, Change = MoneyChange, Route = Route, Request = MoneyRequest>> {
    app: &'a Running<A>,
}

impl<A: App<Key = domain::MoneyView, Change = MoneyChange, Route = Route, Request = MoneyRequest>> Host<A> for Phone<'_, A> {
    fn read<R>(&self, f: impl FnOnce(&dyn ark::views::Views<domain::MoneyView>) -> R) -> R {
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
        self.app.shell.open_route(route.ffi())
    }
    fn ask(&mut self, ticket: Ticket, ask: Ask<A::Msg>) {
        let modules = self.app.modules.lock().unwrap_or_else(|e| e.into_inner()).clone();
        self.app.inbox.ask(&modules, ticket, ask)
    }
    fn request(&mut self, r: MoneyRequest) {
        self.app.shell.request(r.ffi())
    }
    /// SwiftUI and Compose draw the page after they read it: what waits
    /// on a draw waits for `drawn`.
    fn draws_between(&self) -> bool {
        true
    }
}

impl<A: App<Key = domain::MoneyView, Change = MoneyChange, Route = Route, Request = MoneyRequest>> Running<A> {
    fn new(session: &Session, shell: Arc<dyn AppShell>) -> Running<A> {
        let app = Running {
            session: session.inner.clone(),
            shell,
            runtime: Mutex::new(Runtime::<A>::new()),
            modules: Mutex::new(Modules::default()),
            inbox: Inbox::default(),
            holds: Mutex::new((vec![], vec![])),
        };
        app.hold(&app.runtime.lock().unwrap_or_else(|e| e.into_inner()));
        app
    }

    /// The views the state names held, and those it no longer names let go.
    fn hold(&self, rt: &Runtime<A>) {
        let want = rt.holds();
        let mut h = self.holds.lock().unwrap_or_else(|e| e.into_inner());
        if h.0 != want {
            let held = want.iter().map(|k| self.session.hold(k.clone())).collect();
            *h = (want, held);
        }
    }

    fn send(&self, msg: A::Msg) {
        let mut rt = self.runtime.lock().unwrap_or_else(|e| e.into_inner());
        rt.dispatch(msg, &mut Phone { app: self });
        self.inbox.drain(&mut rt, &mut Phone { app: self });
        self.hold(&rt);
    }

    fn screen(&self) -> A::Screen {
        let rt = self.runtime.lock().unwrap_or_else(|e| e.into_inner());
        rt.screen(&Phone { app: self })
    }

    fn version(&self) -> u64 {
        self.runtime.lock().unwrap_or_else(|e| e.into_inner()).version()
    }

    fn is_idle(&self) -> bool {
        let rt = self.runtime.lock().unwrap_or_else(|e| e.into_inner());
        self.session.read(|st, _| rt.is_idle(st))
    }

    /// The answers that came since (a camera's, later than its message),
    /// run; whether any did, so the page is read again.
    fn drain(&self) -> bool {
        let mut rt = self.runtime.lock().unwrap_or_else(|e| e.into_inner());
        let before = rt.version();
        self.inbox.drain(&mut rt, &mut Phone { app: self });
        self.hold(&rt);
        rt.version() != before
    }

    /// The page drawn: what waited on it taken ([`Runtime::drawn`]);
    /// whether any did, so the page is read again.
    fn drawn(&self) -> bool {
        let mut rt = self.runtime.lock().unwrap_or_else(|e| e.into_inner());
        let ran = rt.drawn(&mut Phone { app: self });
        self.inbox.drain(&mut rt, &mut Phone { app: self });
        self.hold(&rt);
        ran
    }

    fn use_devices(&self, devices: &Devices) {
        *self.modules.lock().unwrap_or_else(|e| e.into_inner()) = devices.modules.clone();
    }
}

/// A screen's app as UniFFI exports it: `$name` over `$app`, its messages
/// `$msg` (whose twin is `$msg_ffi`) and its page's twin `$page`.
macro_rules! screen {
    ($(#[$doc:meta])* $name:ident, $app:ty, $msg:ty, $msg_ffi:ty, $page:ty) => {
        $(#[$doc])*
        #[derive(uniffi::Object)]
        pub struct $name(Running<$app>);

        #[uniffi::export]
        impl $name {
            /// The screen over `session`, its views held while it lives.
            #[uniffi::constructor]
            pub fn new(session: Arc<Session>, shell: Arc<dyn AppShell>) -> Arc<$name> {
                Arc::new($name(Running::new(&session, shell)))
            }

            /// `msg` taken: the state updated, its changes sent, its routes
            /// opened, the views it names held.
            pub fn send(&self, msg: $msg_ffi) {
                self.0.send(<$msg>::from_ffi(msg))
            }

            /// What the screen shows now.
            pub fn page(&self) -> $page {
                self.0.screen().ffi()
            }

            /// A number that grows with every message taken: with the
            /// session's versions, what says to read the page again.
            pub fn version(&self) -> u64 {
                self.0.version()
            }

            /// Whether nothing is left to come (`Runtime::is_idle`): what a
            /// screenshot driver waits on, with the version unchanged.
            pub fn is_idle(&self) -> bool {
                self.0.is_idle()
            }

            /// What a module answered later (the camera, when the person is
            /// done) taken; whether the page moved.
            pub fn drain(&self) -> bool {
                self.0.drain()
            }

            /// The page as last read has been drawn: what waited on that
            /// (`Command::AfterDraw`) taken; whether any did, so the page
            /// is read again. Asked while `awaits_draw`.
            pub fn drawn(&self) -> bool {
                self.0.drawn()
            }

            /// Whether something waits for the page to be drawn.
            pub fn awaits_draw(&self) -> bool {
                self.0.runtime.lock().unwrap_or_else(|e| e.into_inner()).awaits_draw()
            }

            /// The phone's device modules, registered once at startup.
            pub fn use_devices(&self, devices: Arc<Devices>) {
                self.0.use_devices(&devices)
            }
        }
    };
}

screen!(
    /// The Accounts tab, running.
    AccountsApp, Accounts, AccountsMsg, AccountsMsgFfi, AccountsPageFfi
);
screen!(
    /// The Budget tab, running: its month's views held as the month moves.
    BudgetApp, Budget, BudgetMsg, BudgetMsgFfi, BudgetPageFfi
);
screen!(
    /// money's shell, running: its sections, toolbar, inspector, search and
    /// commands (money/app/src/shell). Its generated SwiftUI and Compose
    /// (`MoneyShellView`, `MoneySettingsView`, `MoneyCommands`) read its
    /// page and send it messages; a command only a hosted screen can carry
    /// out goes to the [`AppShell`]'s `request`, typed.
    ShellApp, MoneyShell, ShellMsg, ShellMsgFfi, ShellPageFfi
);
screen!(
    /// The Transactions tab, running (money/app/src/transactions): To
    /// Review and All, approving and categorizing.
    TransactionsApp, Transactions, TransactionsMsg, TransactionsMsgFfi, TransactionsPageFfi
);
screen!(
    /// Insights, running (money/app/src/insights): where the month's money went.
    InsightsApp, Insights, InsightsMsg, InsightsMsgFfi, InsightsPageFfi
);
screen!(
    /// A register, running (money/app/src/register): an account's
    /// transactions, or every account's, as a table.
    RegisterApp, Register, RegisterMsg, RegisterMsgFfi, RegisterPageFfi
);
screen!(
    /// The sheets over the app, running (money/app/src/sheets): a
    /// transaction's form, a category given, an account reconciled.
    SheetsApp, Sheets, SheetsMsg, SheetsMsgFfi, SheetsPageFfi
);
screen!(
    /// A transaction's receipt, being given (money/app/src/receipts): a
    /// page scanned or a photo chosen, kept and sent to the server, which
    /// reads it.
    ReceiptsApp, Receipts, ReceiptsMsg, ReceiptsMsgFfi, ReceiptsPageFfi
);

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
        assert_eq!(money_core::dates::day_of_ms(session_now(0, "2026-10-26")), "2026-10-26");
    }
}
