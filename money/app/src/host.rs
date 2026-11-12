//! money's part of its host (`ark_app::host`), once for every platform:
//! the phones' (ark-ffi's `ArkHost`), the browser's and the desktop's
//! drive it. Over a session it runs every screen's runtime — the shell,
//! Budget, Accounts, Transactions, Insights and the sheets, and where the
//! shell's sections are the window's places the register the shell shows
//! — and carries out what each asks of the app ([`MoneyRequest`]): a
//! message for another runtime, the budget's month moved, the bank asked
//! for anything new (the demo's bank here, a server's importer asked), a
//! category or a transaction named by a launch, the session's facts kept
//! in the shell's Settings. What only the toolkit's shell does is a
//! [`Native`]: a page pushed on a phone's stack, Settings and Add a Device
//! put up, what shows put away for a link, signed out.

use ark_app::host::{Cx, Has, HostedApp, Link, Running, Screen, Session};
use money_core::demo::Bank;
use money_core::{dates, AccountLine, BankLine, BudgetLine, MoneyChange, MoneyView};

use crate::accounts::Accounts;
use crate::budget::{Budget, BudgetMsg};
use crate::insights::{Insights, InsightsMsg};
use crate::register::{Register, RegisterMsg};
use crate::sheets::{Sheets, SheetsMsg};
use crate::shell::settings::SessionFacts;
use crate::shell::{MoneyShell, ShellMsg};
use crate::transactions::{Transactions, TransactionsMsg};
use crate::{MoneyRequest, Route};

/// What only the toolkit's shell does, asked of it in order.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub enum Native {
    /// A page on the section's stack (a phone): an account's register, All
    /// Accounts, Connected Banks, a bank.
    Open { route: Route },
    /// Settings put up (its pane is the shell's, chosen already).
    Settings { pane: String },
    /// Add a Device's page.
    AddDevice,
    /// A link taken: what the shell shows of its own put away (a section's
    /// pages, Settings, a sheet of its own).
    Reset,
    /// Signed out of the server, or out of the demo.
    SignOut,
}

/// Who is using the budget and where, as the account sheet and a tab's face
/// say it.
#[derive(Clone, Debug, Default, PartialEq, Eq, ark::twin::Twin)]
pub struct Household {
    /// Their name ("Alex"); empty while it is not known.
    pub who: String,
    /// Where their picture is loaded from; empty for none, and the demo's.
    pub picture: String,
    /// Under the name: their email or login, or what the demo is.
    pub detail: String,
    /// The server as it is shown (`money.example.com`); none for the demo.
    pub server: Option<String>,
    /// The link to it: up, down, or not known yet.
    pub linked: Option<bool>,
    pub demo: bool,
    /// The banks that want somebody in Settings.
    pub banks_wanting: i64,
    /// The bank is being asked for anything new.
    pub syncing: bool,
}

/// money's runtimes over one session.
pub struct Money {
    pub shell: Screen<MoneyShell>,
    pub accounts: Screen<Accounts>,
    pub budget: Screen<Budget>,
    pub transactions: Screen<Transactions>,
    pub insights: Screen<Insights>,
    pub sheets: Screen<Sheets>,
    /// The register the shell shows, where its sections are the window's
    /// places (the browser, the desktop); a phone pushes its own.
    pub register: Option<Screen<Register>>,
    today: String,
    month: String,
    /// The demo's bank, standing in for Plaid behind Sync.
    bank: Option<Bank>,
    /// What a launch asked that the household had not synced yet: asked
    /// again as it moves.
    pending: Vec<MoneyRequest>,
    /// The facts last told the shell.
    facts: Option<SessionFacts>,
    syncing: bool,
}

/// The time a screen is worked out at: the device's clock on the session's
/// day, and noon of that day when the clock is on another (the demo's fixed
/// day), so what is dated today is dated with the session's day.
pub fn session_now(clock_ms: i64, today: &str) -> i64 {
    if dates::day_of_ms(clock_ms) == today {
        return clock_ms;
    }
    match dates::parse_day(today) {
        Some((y, m, d)) => dates::days_from_civil(y, m, d) * 86_400_000 + 43_200_000,
        None => clock_ms,
    }
}

/// What to call whoever is using the budget: the demo is Alex's.
fn who(s: &Session) -> String {
    match () {
        _ if s.demo => "Alex".into(),
        _ if !s.name.is_empty() => s.name.clone(),
        _ => s.user.clone(),
    }
}

/// Who is signed in where, as Settings says it.
pub fn facts(s: &Session) -> SessionFacts {
    SessionFacts {
        who: who(s),
        login: match () {
            _ if s.demo => String::new(),
            _ if !s.email.is_empty() => s.email.clone(),
            _ => s.user.clone(),
        },
        picture: s.picture.clone(),
        demo: s.demo,
        server: s.server.clone(),
        linked: s.linked,
        pending: s.pending,
        version: s.version.clone(),
        leaves: s.leaves,
        adds_devices: s.adds_devices,
    }
}

impl Money {
    /// The month the budget shows (`2026-10`).
    pub fn month(&self) -> &str {
        &self.month
    }

    /// The session's day (`2026-10-26`).
    pub fn today(&self) -> &str {
        &self.today
    }

    /// Who is using the budget and where: the account sheet's.
    pub fn household(&self, cx: &Cx<'_, Money>) -> Household {
        let s = cx.place.session();
        let banks: Vec<BankLine> = cx.rows(&MoneyView::Banks);
        Household {
            who: who(&s),
            picture: s.picture.clone(),
            detail: match () {
                _ if s.demo => "The demo's household, on this phone alone".into(),
                _ if !s.email.is_empty() => s.email.clone(),
                _ => s.user.clone(),
            },
            server: (!s.demo && !s.server.is_empty()).then(|| s.server.clone()),
            linked: s.linked,
            demo: s.demo,
            banks_wanting: money_core::words::banks_wanting(&banks),
            syncing: self.syncing,
        }
    }

    /// The budget, its spending and the month's figures on `month`, every
    /// runtime that shows a month told.
    fn show_month(&mut self, month: String, cx: &mut Cx<'_, Money>) {
        if self.month == month {
            return;
        }
        self.month = month.clone();
        self.budget.send(BudgetMsg::Month { month: month.clone() }, cx);
        self.transactions.send(TransactionsMsg::Month { month: month.clone() }, cx);
        self.insights.send(InsightsMsg::Month { month: month.clone() }, cx);
        self.sheets.send(
            SheetsMsg::Month {
                month: month.clone(),
                today: self.today.clone(),
            },
            cx,
        );
        self.shell.send(ShellMsg::Month { month }, cx);
    }

    /// Category Details for the category of this hex id: the shell's
    /// inspector, on the budget.
    fn inspect(&mut self, id: String, cx: &mut Cx<'_, Money>) {
        self.shell.send(ShellMsg::Select { tag: "budget".into() }, cx);
        self.shell.send(ShellMsg::InspectCategory { id: Some(id) }, cx);
        if !self.shell.state().inspector {
            self.shell.send(ShellMsg::Inspector { shown: true }, cx);
        }
    }

    /// Ask the bank for anything new: the demo's bank runs its next import
    /// here, as a server's importer would; a server's importer is asked to
    /// sync now (`banks.sync`, which any household member may ask), and the
    /// link dialled in case it was resting. What came of it is said.
    fn sync_bank(&mut self, cx: &mut Cx<'_, Money>) {
        self.syncing = true;
        let said = match &mut self.bank {
            Some(bank) => {
                let lines: Vec<AccountLine> = cx.rows(&MoneyView::Accounts);
                let owed = lines.iter().find(|a| a.bank == money_core::demo::BANK_NAME).map_or(0, |a| a.cleared);
                let import = bank.next(&self.today, owed);
                let (n, u) = (import.upserts.len(), import.removed.len());
                match cx.place.send(MoneyChange::ImportBank(import)) {
                    Ok(()) => match (n, bank.syncs) {
                        (0, _) => "Chase: nothing new at the bank".into(),
                        (_, 1) => format!("Chase: {n} new from the bank \u{2014} one still pending"),
                        _ => format!("Chase: {n} from the bank, {u} pending one posted"),
                    },
                    Err(why) => format!("The bank didn't answer: {why}"),
                }
            }
            None => {
                cx.place.dial();
                match cx.place.send(MoneyChange::Sync) {
                    Ok(()) => "Asked the bank for anything new".into(),
                    Err(why) => format!("Couldn't ask the bank: {why}"),
                }
            }
        };
        self.syncing = false;
        cx.place.notice(said)
    }

    /// The register the shell shows follows it: the account its section
    /// names, its search, its transaction.
    fn follow_register(&mut self, cx: &mut Cx<'_, Money>) {
        let Some(reg) = &mut self.register else { return };
        let s = self.shell.state();
        let account = crate::shell::tagged_account(&s.section).map(String::from);
        let (search, selected) = (s.search.clone(), s.transaction.clone());
        if reg.state().account != account {
            reg.send(RegisterMsg::Show { account }, cx);
        }
        if reg.state().search != search {
            reg.send(RegisterMsg::Search { text: search }, cx);
        }
        if reg.state().selected != selected {
            reg.send(RegisterMsg::Select { id: selected }, cx);
        }
    }
}

impl HostedApp for Money {
    type Key = MoneyView;
    type Change = MoneyChange;
    type Route = Route;
    type Request = MoneyRequest;
    type Native = Native;

    fn open(session: &Session, cx: &mut Cx<'_, Money>) -> Money {
        let today = session.today.clone();
        let mut m = Money {
            shell: Screen::new(),
            accounts: Screen::new(),
            budget: Screen::new(),
            transactions: Screen::new(),
            insights: Screen::new(),
            sheets: Screen::new(),
            register: (!cx.place.stacks()).then(Screen::new),
            month: String::new(),
            bank: session.demo.then(Bank::default),
            pending: vec![],
            facts: None,
            syncing: false,
            today: today.clone(),
        };
        cx.now_ms = session_now(cx.place.now_ms(), &today);
        m.show_month(today[..7.min(today.len())].to_string(), cx);
        m
    }

    fn screens(&mut self) -> Vec<&mut dyn Running<Money>> {
        let mut v: Vec<&mut dyn Running<Money>> = vec![
            &mut self.shell,
            &mut self.accounts,
            &mut self.budget,
            &mut self.transactions,
            &mut self.insights,
            &mut self.sheets,
        ];
        if let Some(r) = &mut self.register {
            v.push(r);
        }
        v
    }

    fn screens_ref(&self) -> Vec<&dyn Running<Money>> {
        let mut v: Vec<&dyn Running<Money>> = vec![
            &self.shell,
            &self.accounts,
            &self.budget,
            &self.transactions,
            &self.insights,
            &self.sheets,
        ];
        if let Some(r) = &self.register {
            v.push(r);
        }
        v
    }

    /// The banks (what wants a look, the account sheet's), the accounts
    /// (the demo's bank), and the month's budget (a category a launch names).
    fn holds(&self) -> Vec<MoneyView> {
        vec![MoneyView::Banks, MoneyView::Accounts, MoneyView::Budget { month: self.month.clone() }]
    }

    fn now_ms(&self, clock_ms: i64) -> i64 {
        session_now(clock_ms, &self.today)
    }

    /// A route a screen opened: a category's month is Category Details, the
    /// shell's inspector; any other page a phone pushes on its stack, and a
    /// window shows as one of its places.
    fn route(&mut self, r: Route, cx: &mut Cx<'_, Money>) {
        if let Route::Category { id, .. } = r {
            return self.inspect(id, cx);
        }
        if cx.place.stacks() {
            return cx.native(Native::Open { route: r });
        }
        let tag = match r {
            Route::Budget => "budget".to_string(),
            Route::Insights => "insights".into(),
            Route::Accounts | Route::Inbox | Route::Filtered | Route::Filter { .. } => "transactions".into(),
            Route::Account { id, .. } => crate::shell::account_tag(&id),
            Route::Settings | Route::Banks | Route::Bank { .. } => {
                return cx.request(MoneyRequest::Settings {
                    pane: "Connected Banks".into(),
                })
            }
            _ => return,
        };
        self.shell.send(ShellMsg::Select { tag }, cx)
    }

    fn request(&mut self, r: MoneyRequest, cx: &mut Cx<'_, Money>) {
        match r {
            MoneyRequest::OpenCategory { key } => {
                let lines: Vec<BudgetLine> = cx.rows(&MoneyView::Lines { month: self.month.clone() });
                match crate::shell::inspect::category_named(&lines, &key) {
                    Some(l) => self.inspect(l.category_id.hex(), cx),
                    None => self.pending.push(MoneyRequest::OpenCategory { key }),
                }
            }
            // Transaction Details in the inspector while it shows; its form
            // otherwise — tried again as the household arrives when it is not
            // there yet.
            MoneyRequest::OpenTransaction { key } => {
                if self.shell.state().inspector {
                    return self.shell.send(ShellMsg::InspectTransaction { id: Some(key) }, cx);
                }
                self.sheets.send(SheetsMsg::Open { key: key.clone() }, cx);
                if self.sheets.page(&*cx.place, cx.now_ms).transaction.is_none() {
                    self.pending.push(MoneyRequest::OpenTransaction { key });
                }
            }
            MoneyRequest::Sheet { msg } => self.sheets.send(msg, cx),
            MoneyRequest::Budget { msg } => self.budget.send(msg, cx),
            MoneyRequest::Accounts { msg } => {
                self.shell.send(ShellMsg::Select { tag: "accounts".into() }, cx);
                self.accounts.send(msg, cx);
            }
            // What the bank brought, to look over: Transactions' To Review.
            MoneyRequest::Review => {
                self.shell.send(ShellMsg::Select { tag: "transactions".into() }, cx);
                self.transactions.send(TransactionsMsg::Show { review: true }, cx);
            }
            MoneyRequest::StepMonth { by } => {
                let m = dates::add_months(&self.month, by as i64);
                self.show_month(m, cx)
            }
            MoneyRequest::ThisMonth => self.show_month(self.today[..7.min(self.today.len())].to_string(), cx),
            MoneyRequest::Refresh => self.sync_bank(cx),
            MoneyRequest::Settings { pane } => {
                if !pane.is_empty() {
                    self.shell.send(ShellMsg::Pane { title: pane.clone() }, cx);
                }
                cx.native(Native::Settings { pane })
            }
            MoneyRequest::AddDevice => cx.native(Native::AddDevice),
            MoneyRequest::SignOut => cx.native(Native::SignOut),
            // A receipt's photo kept: on the device at once, and sent to the
            // server (which reads it) on this connection or the next. A device
            // whose files cannot be written keeps nothing.
            MoneyRequest::KeepPhoto { jpeg, txn } => {
                if let Some(file) = cx.place.keep(&jpeg, crate::receipts::JPEG) {
                    let txn_id = txn.as_deref().and_then(crate::id_of);
                    let _ = cx.place.send(MoneyChange::AddPhoto(money_core::AddPhoto { file, txn_id }));
                }
            }
            // The desktop's filter bar and receipt import are its own still.
            MoneyRequest::Filter | MoneyRequest::ImportReceipt => {}
        }
    }

    /// What the shell's page says that another runtime follows: its search
    /// is Transactions', its section the register's; and who is signed in
    /// where, told the shell when it moved.
    fn follow(&mut self, cx: &mut Cx<'_, Money>) {
        let search = self.shell.state().search.clone();
        if self.transactions.state().search != search {
            self.transactions.send(TransactionsMsg::Search { text: search }, cx);
        }
        self.follow_register(cx);
        let session = cx.place.session();
        let facts = facts(&session);
        if self.facts.as_ref() != Some(&facts) {
            self.facts = Some(facts.clone());
            self.shell.send(ShellMsg::Session { facts }, cx);
        }
        // Who is using the budget, for "Entered by": the demo's is Alex,
        // whose entries carry the demo's id.
        let user = if session.demo { money_core::demo::ME.to_string() } else { session.user.clone() };
        let name = who(&session);
        let sheets = self.sheets.state();
        if sheets.me != user || sheets.my_name != name {
            self.sheets.send(SheetsMsg::Me { user, name }, cx);
        }
    }

    /// What a launch asked that had not synced yet, asked again.
    fn moved(&mut self, cx: &mut Cx<'_, Money>) {
        for r in std::mem::take(&mut self.pending) {
            cx.request(r)
        }
    }

    /// A link taken while the app shows: the sheets and the shell's own
    /// pages put away, then the shell goes where it says; the shell puts its
    /// inspector away first and moves once the page without it is drawn
    /// (`Command::AfterDraw`).
    fn link(&mut self, link: Link, cx: &mut Cx<'_, Money>) {
        self.sheets.send(SheetsMsg::Dismiss, cx);
        self.pending.clear();
        cx.native(Native::Reset);
        match link {
            Link::Launch(args) => self.shell.send(ShellMsg::Launch { args }, cx),
            Link::Address(address) => self.shell.send(ShellMsg::Address { address }, cx),
        }
    }

    fn launched(&mut self, args: Vec<String>, cx: &mut Cx<'_, Money>) {
        self.shell.send(ShellMsg::Launch { args }, cx)
    }
}

macro_rules! has {
    ($($app:ty => $field:ident),*) => {$(
        impl Has<$app> for Money {
            fn screen(&self) -> &Screen<$app> {
                &self.$field
            }
            fn screen_mut(&mut self) -> &mut Screen<$app> {
                &mut self.$field
            }
        }
    )*};
}

has!(MoneyShell => shell, Accounts => accounts, Budget => budget, Transactions => transactions, Insights => insights, Sheets => sheets);

#[cfg(test)]
mod tests;
