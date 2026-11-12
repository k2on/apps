//! money's screens as Ark apps on the phones (ui/docs/app.md): each
//! screen's runtime, run by money's part of its host (`money_app::host`)
//! under ark-ffi's `Hosted`, exported as one object per app.toml `[screens]`
//! entry ([`ark_ffi::hosted_screen!`]). A screen's generated SwiftUI or
//! Compose reads its record (`AccountsApp::page`) and sends it messages
//! (`AccountsApp::send`); what it asks of another runtime, or of the phone's
//! shell, is the host's.

use std::sync::{Arc, Mutex};

use ark_app::host::Screen;
use ark_ffi::hosted::{blank, Hosted};
use ark_ffi::Twin;
use money_app::accounts::Accounts;
use money_app::budget::Budget;
use money_app::insights::Insights;
use money_app::register::{Register, RegisterMsg};
use money_app::sheets::Sheets;
use money_app::shell::MoneyShell;
use money_app::transactions::Transactions;

use crate::host::MoneyPhone;
use crate::*;

ark_ffi::hosted_screen!(
    /// The Accounts tab, running.
    AccountsApp, MoneyPhone, Accounts, AccountsMsgFfi, AccountsPageFfi
);
ark_ffi::hosted_screen!(
    /// The Budget tab, running: its month's views held as the month moves.
    BudgetApp, MoneyPhone, Budget, BudgetMsgFfi, BudgetPageFfi
);
ark_ffi::hosted_screen!(
    /// money's shell, running: its sections, toolbar, inspector, search and
    /// commands (money/app/src/shell). Its generated SwiftUI and Compose
    /// (`MoneyShellView`, `MoneySettingsView`, `MoneyCommands`) read its
    /// page and send it messages.
    ShellApp, MoneyPhone, MoneyShell, ShellMsgFfi, ShellPageFfi
);
ark_ffi::hosted_screen!(
    /// The Transactions tab, running: To Review and All.
    TransactionsApp, MoneyPhone, Transactions, TransactionsMsgFfi, TransactionsPageFfi
);
ark_ffi::hosted_screen!(
    /// Insights, running: where the month's money went.
    InsightsApp, MoneyPhone, Insights, InsightsMsgFfi, InsightsPageFfi
);
ark_ffi::hosted_screen!(
    /// The sheets over the app, running: a transaction's form and its
    /// receipt, a category given, an account reconciled.
    SheetsApp, MoneyPhone, Sheets, SheetsMsgFfi, SheetsPageFfi
);

/// A register, running for as long as the phone shows it (an account's
/// transactions, or every account's): a page pushed on a stack, beside the
/// host's runtimes, over its session.
#[derive(uniffi::Object)]
pub struct RegisterApp {
    host: Arc<Hosted<MoneyPhone>>,
    screen: Mutex<Screen<Register>>,
}

impl RegisterApp {
    pub(crate) fn over(host: Arc<Hosted<MoneyPhone>>, account: Option<String>) -> Option<Arc<RegisterApp>> {
        let mut screen = Screen::new();
        host.beside(&mut screen, |s, cx| s.send(RegisterMsg::Show { account }, cx))?;
        Some(Arc::new(RegisterApp {
            host,
            screen: Mutex::new(screen),
        }))
    }

    fn screen(&self) -> std::sync::MutexGuard<'_, Screen<Register>> {
        self.screen.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Drop for RegisterApp {
    fn drop(&mut self) {
        let mut s = self.screen();
        self.host.close_beside(&mut s);
    }
}

#[uniffi::export]
impl RegisterApp {
    /// `msg` taken.
    pub fn send(&self, msg: RegisterMsgFfi) {
        let mut s = self.screen();
        self.host.beside(&mut s, |s, cx| s.send(RegisterMsg::from_ffi(msg), cx));
    }

    /// What the register shows now.
    pub fn page(&self) -> RegisterPageFfi {
        let s = self.screen();
        self.host
            .read_beside(&s, |s, place, now| s.page(place, now))
            .unwrap_or_else(blank::<Register>)
            .ffi()
    }

    pub fn version(&self) -> u64 {
        self.screen().version()
    }

    pub fn is_idle(&self) -> bool {
        let s = self.screen();
        self.host.read_beside(&s, |s, place, _| s.is_idle(place)).unwrap_or(true)
    }

    /// The page drawn: what waited on it taken; whether any did.
    pub fn drawn(&self) -> bool {
        let mut s = self.screen();
        self.host.beside(&mut s, |s, cx| s.drawn(cx)).unwrap_or(false)
    }

    pub fn awaits_draw(&self) -> bool {
        self.screen().awaits_draw()
    }
}

/// The shell's page before a session runs it: on Budget, nothing held,
/// nobody signed in — what a phone draws until its session opens.
#[uniffi::export]
pub fn shell_blank() -> ShellPageFfi {
    blank::<MoneyShell>().ffi()
}
