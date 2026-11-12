//! money's host on the phones: its part of the app host every Ark app has
//! (`ark_ffi::ArkHost`, which runs the first run and the session's link),
//! as the desktop's `live.rs` is money's on the desktop. Once a session is
//! open it makes every screen's runtime over it — the shell, Budget,
//! Accounts, Transactions, Insights and the sheets — and carries out what
//! each asks of the app ([`MoneyRequest`]): a message for another
//! runtime, the budget's month moved, the bank asked for anything new, a
//! category or a transaction named by a launch, the session's facts kept
//! in the shell's Settings. What is left for the phone's shell is what it
//! draws itself: a page pushed on a section's stack, Settings and Add a
//! Device ([`NativeAsk`]), and the account sheet ([`Household`]).
//!
//! It replaces what money's phones owned by hand: `Model.swift` and
//! `Bridge.swift`, Android's `Book.kt` and `Start.kt`'s runtime.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use ark_ffi::{ArkHost, Handle, HostApp, Launch, Login, Opened, Peer, Refusal, Twin};
use money_app::budget::BudgetMsg;
use money_app::insights::InsightsMsg;
use money_app::routes::Route;
use money_app::sheets::SheetsMsg;
use money_app::shell::settings::SessionFacts;
use money_app::shell::ShellMsg;
use money_app::transactions::TransactionsMsg;
use money_app::MoneyRequest;
use money_core::{self as domain, BankLine, BudgetLine, MoneyChange};

use crate::app::Router;
use crate::{AccountsApp, BudgetApp, InsightsApp, RegisterApp, SheetsApp, ShellApp, TransactionsApp, WelcomeApp};

type Tag = crate::UniFfiTag;

/// What only the phone's shell does, asked of it in order: taken when it
/// is told something moved ([`MoneyHost::take_asks`]).
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum NativeAsk {
    /// A page on the section's stack: an account's register, All Accounts,
    /// Connected Banks, a bank.
    Open { route: <Route as Twin<Tag>>::Ffi },
    /// Settings, on the pane of this title (empty: as it was).
    Settings { pane: String },
    /// Add a Device's page.
    AddDevice,
    /// A link taken as a launch: what the shell shows of its own put away
    /// (a section's pages, Settings, a sheet of its own).
    Reset,
}

/// Who is using the budget and where, as the account sheet and a tab's
/// face say it.
#[derive(Clone, Debug, Default, PartialEq, Eq, uniffi::Record)]
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

/// The session's runtimes, while one is open.
struct Open {
    peer: Arc<Peer>,
    session: Arc<ark_ffi::Session<domain::MoneyView>>,
    how: Opened,
    today: String,
    month: Mutex<String>,
    bank: Option<Arc<crate::DemoBank>>,
    shell: Arc<ShellApp>,
    accounts: Arc<AccountsApp>,
    budget: Arc<BudgetApp>,
    transactions: Arc<TransactionsApp>,
    insights: Arc<InsightsApp>,
    sheets: Arc<SheetsApp>,
    /// What the host reads beside the screens: the banks (what wants a
    /// look) and the month's budget (a category a launch names).
    holds: Mutex<Vec<ark_ffi::Hold<domain::MoneyView>>>,
    /// The shell's search as last handed to Transactions.
    search: Mutex<String>,
    /// The facts last told the shell.
    facts: Mutex<Option<SessionFacts>>,
}

/// What a runtime asked, carried out once no runtime is mid-message.
enum Job {
    Route(Route),
    Request(MoneyRequest),
}

/// money's part of its host.
struct Money {
    me: Weak<Money>,
    open: Mutex<Option<Arc<Open>>>,
    host: Mutex<Option<Handle>>,
    jobs: Mutex<VecDeque<Job>>,
    settling: AtomicBool,
    /// What a launch asked that the household had not synced yet: asked
    /// again as it moves.
    pending: Mutex<Vec<MoneyRequest>>,
    asks: Mutex<Vec<NativeAsk>>,
    syncing: AtomicBool,
    /// How many sessions were opened: the phone's holders are made again
    /// when it moves.
    generation: std::sync::atomic::AtomicU64,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// How a screen's runtime reaches money's host.
struct Routes(Weak<Money>);

impl Router for Routes {
    fn open_route(&self, route: Route) {
        if let Some(m) = self.0.upgrade() {
            lock(&m.jobs).push_back(Job::Route(route));
        }
    }
    fn request(&self, request: MoneyRequest) {
        if let Some(m) = self.0.upgrade() {
            lock(&m.jobs).push_back(Job::Request(request));
        }
    }
    fn now_ms(&self) -> i64 {
        ark_peer::now_ms()
    }
    fn today(&self) -> String {
        self.0
            .upgrade()
            .and_then(|m| m.opened())
            .map(|o| o.today.clone())
            .unwrap_or_else(crate::money_today)
    }
    fn settle(&self) {
        if let Some(m) = self.0.upgrade() {
            m.settle()
        }
    }
    fn answered(&self) {
        if let Some(h) = self.0.upgrade().and_then(|m| m.host()) {
            h.moved()
        }
    }
}

impl Money {
    fn opened(&self) -> Option<Arc<Open>> {
        lock(&self.open).clone()
    }

    fn host(&self) -> Option<Handle> {
        lock(&self.host).clone()
    }

    fn ask(&self, a: NativeAsk) {
        lock(&self.asks).push(a);
    }

    /// Every job asked so far carried out, in order — and what they asked
    /// in turn; once, however many runtimes call it meanwhile.
    fn settle(&self) {
        let mut ran = false;
        loop {
            if self.settling.swap(true, Ordering::AcqRel) {
                return;
            }
            while let Some(job) = lock(&self.jobs).pop_front() {
                ran = true;
                match job {
                    Job::Route(r) => self.route(r),
                    Job::Request(r) => self.request(r),
                }
            }
            self.settling.store(false, Ordering::Release);
            // Asked while it let go: taken by this call, not left waiting.
            if lock(&self.jobs).is_empty() {
                break;
            }
        }
        if let Some(o) = self.opened() {
            self.follow(&o);
        }
        if ran {
            if let Some(h) = self.host() {
                h.changed()
            }
        }
    }

    /// What the shell's page says that another runtime follows: its search
    /// is Transactions'.
    fn follow(&self, o: &Open) {
        let search = o.shell.0.screen().search;
        let mut last = lock(&o.search);
        if *last != search {
            *last = search.clone();
            drop(last);
            o.transactions.0.send(TransactionsMsg::Search { text: search });
        }
    }

    /// A route a screen opened: a category's month is Category Details,
    /// the shell's inspector; any other page is the phone's to push.
    fn route(&self, r: Route) {
        let Some(o) = self.opened() else { return };
        match r {
            Route::Category { id, .. } => inspect(&o, id),
            r => self.ask(NativeAsk::Open { route: r.ffi() }),
        }
    }

    /// A request a screen made.
    fn request(&self, r: MoneyRequest) {
        let Some(o) = self.opened() else { return };
        match r {
            MoneyRequest::OpenCategory { key } => match category_named(&o, &key) {
                Some(l) => inspect(&o, l.category_id.hex()),
                None => lock(&self.pending).push(MoneyRequest::OpenCategory { key }),
            },
            // Transaction Details in the inspector while it shows; its form
            // otherwise — tried again as the household arrives when it is
            // not there yet.
            MoneyRequest::OpenTransaction { key } => {
                if o.shell.0.screen().inspector {
                    return o.shell.0.send(ShellMsg::InspectTransaction { id: Some(key) });
                }
                o.sheets.0.send(SheetsMsg::Open { key: key.clone() });
                if o.sheets.0.screen().transaction.is_none() {
                    lock(&self.pending).push(MoneyRequest::OpenTransaction { key });
                }
            }
            MoneyRequest::Sheet { msg } => o.sheets.0.send(msg),
            MoneyRequest::Budget { msg } => o.budget.0.send(msg),
            MoneyRequest::Accounts { msg } => {
                o.shell.0.send(ShellMsg::Select { tag: "accounts".into() });
                o.accounts.0.send(msg);
            }
            // What the bank brought, to look over: Transactions' To Review.
            MoneyRequest::Review => {
                o.shell.0.send(ShellMsg::Select { tag: "transactions".into() });
                o.transactions.0.send(TransactionsMsg::Show { review: true });
            }
            MoneyRequest::StepMonth { by } => {
                let m = domain::dates::add_months(&lock(&o.month), by as i64);
                self.show_month(&o, m)
            }
            MoneyRequest::ThisMonth => self.show_month(&o, o.today[..7.min(o.today.len())].to_string()),
            MoneyRequest::Refresh => self.sync_bank(&o),
            MoneyRequest::Settings { pane } => self.ask(NativeAsk::Settings { pane }),
            MoneyRequest::AddDevice => self.ask(NativeAsk::AddDevice),
            MoneyRequest::SignOut => {
                if let Some(h) = self.host() {
                    h.sign_out()
                }
            }
            // A receipt's photo is kept by the screen's runtime (app.rs);
            // the rest has no host's part on a phone.
            MoneyRequest::KeepPhoto { .. } | MoneyRequest::Filter | MoneyRequest::ImportReceipt => {}
        }
    }

    /// The budget, its spending and the month's figures on `month`, every
    /// runtime that shows a month told.
    fn show_month(&self, o: &Open, month: String) {
        {
            let mut m = lock(&o.month);
            if *m == month {
                return;
            }
            *m = month.clone();
        }
        hold(o);
        o.budget.0.send(BudgetMsg::Month { month: month.clone() });
        o.transactions.0.send(TransactionsMsg::Month { month: month.clone() });
        o.insights.0.send(InsightsMsg::Month { month: month.clone() });
        o.sheets.0.send(SheetsMsg::Month {
            month: month.clone(),
            today: o.today.clone(),
        });
        o.shell.0.send(ShellMsg::Month { month });
    }

    /// Ask the bank for anything new: the demo's bank runs its next import
    /// here; a server's importer is asked to sync now (`banks.sync`, which
    /// any household member may ask), and the link dialled in case it was
    /// resting. What came of it is said.
    fn sync_bank(&self, o: &Open) {
        self.syncing.store(true, Ordering::Release);
        let said = match &o.bank {
            Some(bank) => match bank.sync(o.peer.clone(), Some(o.today.clone())) {
                Ok(s) => s.note,
                Err(Refusal::Refused { why }) => format!("The bank didn't answer: {why}"),
            },
            None => {
                o.peer.dial_now();
                let c = MoneyChange::Sync;
                match o.session.send(|p| p.mutate_encoded(c.name(), c.input())) {
                    Ok(_) => "Asked the bank for anything new".into(),
                    Err(Refusal::Refused { why }) => format!("Couldn't ask the bank: {why}"),
                }
            }
        };
        self.syncing.store(false, Ordering::Release);
        if let Some(h) = self.host() {
            h.notice(said)
        }
    }

    /// Who is signed in where, as Settings says it.
    fn facts(&self, o: &Open, version: String) -> SessionFacts {
        let st = o.peer.status();
        let demo = matches!(o.how, Opened::Demo { .. });
        let login = login_of(o);
        SessionFacts {
            who: who(o),
            login: login
                .as_ref()
                .map(|l| if l.email.is_empty() { l.user.clone() } else { l.email.clone() })
                .unwrap_or_default(),
            demo,
            server: server_of(o)
                .map(|s| ark_ffi::ark_auth::probe::display(&s).to_string())
                .unwrap_or_default(),
            linked: Some(st.linked),
            pending: st.pending as i64,
            version,
            leaves: true,
            adds_devices: !demo,
        }
    }

    /// The facts told the shell when they moved.
    fn tell_facts(&self, o: &Open) {
        let version = self.host().map(|h| h.version()).unwrap_or_default();
        let facts = self.facts(o, version);
        let mut last = lock(&o.facts);
        if last.as_ref() != Some(&facts) {
            *last = Some(facts.clone());
            drop(last);
            o.shell.0.send(ShellMsg::Session { facts });
        }
    }
}

/// Category Details for the category of this hex id: the shell's
/// inspector, on the budget.
fn inspect(o: &Open, id: String) {
    o.shell.0.send(ShellMsg::Select { tag: "budget".into() });
    o.shell.0.send(ShellMsg::InspectCategory { id: Some(id) });
    o.shell.0.send(ShellMsg::Inspector { shown: true });
}

/// The category a launch names: by its name in any case, or a word of it.
fn category_named(o: &Open, key: &str) -> Option<BudgetLine> {
    let w = key.to_lowercase();
    let month = lock(&o.month).clone();
    let lines: Vec<BudgetLine> = o
        .session
        .read(|st, _| st.rows::<BudgetLine>(&domain::MoneyView::Lines { month }).to_vec());
    lines
        .iter()
        .find(|l| l.name.to_lowercase() == w)
        .or_else(|| lines.iter().find(|l| l.name.to_lowercase().contains(&w)))
        .cloned()
}

/// What the host holds beside the screens: the banks, and the month's budget.
fn hold(o: &Open) {
    let month = lock(&o.month).clone();
    let held = vec![
        o.session.hold(domain::MoneyView::Banks),
        o.session.hold(domain::MoneyView::Budget { month }),
    ];
    *lock(&o.holds) = held;
}

fn login_of(o: &Open) -> Option<Login> {
    match &o.how {
        Opened::Server { login, .. } => Some(o.peer.signed_in().unwrap_or_else(|| login.clone())),
        Opened::Demo { .. } => None,
    }
}

fn server_of(o: &Open) -> Option<String> {
    match &o.how {
        Opened::Server { server, .. } => Some(server.clone()),
        Opened::Demo { .. } => None,
    }
}

/// What to call whoever is using the budget: the demo is Alex's, as the
/// demo window's is.
fn who(o: &Open) -> String {
    match login_of(o) {
        Some(l) if !l.name.is_empty() => l.name,
        Some(l) => l.user,
        None => "Alex".into(),
    }
}

impl HostApp for Money {
    fn info(&self) -> ark_ffi::ark_auth::onboarding::AppInfo {
        ark_ffi::ark_auth::onboarding::AppInfo {
            name: "Money".into(),
            scheme: "money".into(),
            demo: true,
            what: "your budget".into(),
        }
    }

    fn copy(&self, scan: bool, demo: bool) -> ark_app::welcome::WelcomeCopy {
        money_app::welcome::copy(scan, demo)
    }

    fn domain(&self) -> String {
        crate::money_domain()
    }

    fn open(&self, dir: &str, login: Login) -> Result<Arc<Peer>, Refusal> {
        crate::money_process_open(dir.into(), Some(login))
    }

    fn demo(&self, today: Option<String>) -> Result<Arc<Peer>, Refusal> {
        crate::money_demo(today)
    }

    fn opened(&self, peer: Arc<Peer>, how: Opened, host: Handle) {
        *lock(&self.host) = Some(host);
        let today = match &how {
            Opened::Demo { today: Some(t) } => t.clone(),
            _ => crate::money_today(),
        };
        let month = today[..7.min(today.len())].to_string();
        let session = Arc::new(ark_ffi::Session::<domain::MoneyView>::new(peer.clone()));
        let router = || Arc::new(Routes(self.me.clone())) as Arc<dyn Router>;
        let o = Arc::new(Open {
            bank: matches!(how, Opened::Demo { .. }).then(crate::DemoBank::new),
            shell: ShellApp::over(session.clone(), router()),
            accounts: AccountsApp::over(session.clone(), router()),
            budget: BudgetApp::over(session.clone(), router()),
            transactions: TransactionsApp::over(session.clone(), router()),
            insights: InsightsApp::over(session.clone(), router()),
            sheets: SheetsApp::over(session.clone(), router()),
            holds: Mutex::new(vec![]),
            search: Mutex::new(String::new()),
            facts: Mutex::new(None),
            month: Mutex::new(String::new()),
            peer,
            session: session.clone(),
            how,
            today,
        });
        // Whichever views moved, the screens read again: the shell is told.
        let me = self.me.clone();
        session.set_listener(Some(Arc::new(move |_: Vec<domain::MoneyView>| {
            if let Some(h) = me.upgrade().and_then(|m| m.host()) {
                h.changed()
            }
        })));
        *lock(&self.open) = Some(o.clone());
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.show_month(&o, month);
        self.tell_facts(&o);
        self.settle();
    }

    fn closed(&self) {
        let gone = lock(&self.open).take();
        if let Some(o) = gone {
            o.session.set_listener(None);
        }
        lock(&self.jobs).clear();
        lock(&self.pending).clear();
        lock(&self.asks).clear();
    }

    fn moved(&self) {
        let Some(o) = self.opened() else { return };
        // What a device module answered since (the camera's photo), taken
        // by the runtime that asked; the host says the app moved after.
        o.shell.0.drain();
        o.accounts.0.drain();
        o.budget.0.drain();
        o.transactions.0.drain();
        o.insights.0.drain();
        o.sheets.0.drain();
        self.tell_facts(&o);
        // What a launch asked that had not synced yet, asked again.
        let waiting = std::mem::take(&mut *lock(&self.pending));
        lock(&self.jobs).extend(waiting.into_iter().map(Job::Request));
        self.settle();
    }

    fn link(&self, args: Option<Vec<String>>, address: String) {
        let Some(o) = self.opened() else { return };
        o.sheets.0.send(SheetsMsg::Dismiss);
        lock(&self.pending).clear();
        self.ask(NativeAsk::Reset);
        // The shell puts its inspector away first and moves once the page
        // without it is drawn (Command::AfterDraw).
        match args {
            Some(args) => o.shell.0.send(ShellMsg::Launch { args }),
            None => o.shell.0.send(ShellMsg::Address { address }),
        }
    }

    fn launched(&self, args: Vec<String>) {
        if let Some(o) = self.opened() {
            o.shell.0.send(ShellMsg::Launch { args })
        }
    }

    fn idle(&self) -> bool {
        self.opened().is_none_or(|o| {
            o.shell.0.is_idle()
                && o.accounts.0.is_idle()
                && o.budget.0.is_idle()
                && o.transactions.0.is_idle()
                && o.insights.0.is_idle()
                && o.sheets.0.is_idle()
        })
    }

    fn version(&self) -> u64 {
        self.opened().map_or(0, |o| {
            o.shell.0.version()
                + o.accounts.0.version()
                + o.budget.0.version()
                + o.transactions.0.version()
                + o.insights.0.version()
                + o.sheets.0.version()
        })
    }
}

/// money's host, as the phone holds it: the app host every Ark app has
/// ([`ArkHost`], [`MoneyHost::host`]), the first run's page
/// ([`MoneyHost::welcome`]), and money's screens' runtimes while a session
/// is open.
#[derive(uniffi::Object)]
pub struct MoneyHost {
    host: Arc<ArkHost>,
    money: Arc<Money>,
}

#[uniffi::export]
impl MoneyHost {
    /// money's host as the phone launched it, its login kept in `store`
    /// (ark-secure-store's `PhoneSecureStore`; none: for this run alone).
    #[uniffi::constructor]
    pub fn new(launch: Launch, store: Option<Arc<dyn ark_secure_store::phone::SecureStore>>) -> Arc<MoneyHost> {
        let money = Arc::new_cyclic(|me| Money {
            me: me.clone(),
            open: Mutex::new(None),
            host: Mutex::new(None),
            jobs: Mutex::new(VecDeque::new()),
            settling: AtomicBool::new(false),
            pending: Mutex::new(vec![]),
            asks: Mutex::new(vec![]),
            syncing: AtomicBool::new(false),
            generation: std::sync::atomic::AtomicU64::new(0),
        });
        let host = ArkHost::new(money.clone(), launch, store);
        Arc::new(MoneyHost { host, money })
    }

    /// The app host every Ark app has: its first run, its link, the shell's
    /// notices.
    pub fn host(&self) -> Arc<ArkHost> {
        self.host.clone()
    }

    /// The first run's pages, as the generated first run reads them.
    pub fn welcome(&self) -> Arc<WelcomeApp> {
        WelcomeApp::new(self.host.clone())
    }

    /// money's shell: its sections, toolbar, inspector, search, Settings and
    /// commands; none until a session is open.
    pub fn shell(&self) -> Option<Arc<ShellApp>> {
        self.money.opened().map(|o| o.shell.clone())
    }

    pub fn accounts(&self) -> Option<Arc<AccountsApp>> {
        self.money.opened().map(|o| o.accounts.clone())
    }

    pub fn budget(&self) -> Option<Arc<BudgetApp>> {
        self.money.opened().map(|o| o.budget.clone())
    }

    pub fn transactions(&self) -> Option<Arc<TransactionsApp>> {
        self.money.opened().map(|o| o.transactions.clone())
    }

    pub fn insights(&self) -> Option<Arc<InsightsApp>> {
        self.money.opened().map(|o| o.insights.clone())
    }

    pub fn sheets(&self) -> Option<Arc<SheetsApp>> {
        self.money.opened().map(|o| o.sheets.clone())
    }

    /// A register's runtime, for as long as the phone shows it: the account
    /// of this hex id's, or every account's for none.
    pub fn register(&self, account: Option<String>) -> Option<Arc<RegisterApp>> {
        let o = self.money.opened()?;
        let r = RegisterApp::over(o.session.clone(), Arc::new(Routes(Arc::downgrade(&self.money))));
        r.0.send(money_app::register::RegisterMsg::Show { account });
        Some(r)
    }

    /// The session the screens' runtimes are over: grows with every session
    /// opened, so the phone makes its holders again; none open is zero.
    pub fn generation(&self) -> u64 {
        match self.money.opened() {
            Some(_) => self.money.generation.load(Ordering::Acquire),
            None => 0,
        }
    }

    /// What the phone's shell is asked to do, in order, taken.
    pub fn take_asks(&self) -> Vec<NativeAsk> {
        std::mem::take(&mut *lock(&self.money.asks))
    }

    /// Who is using the budget and where: the account sheet's.
    pub fn household(&self) -> Household {
        let Some(o) = self.money.opened() else { return Household::default() };
        let demo = matches!(o.how, Opened::Demo { .. });
        let login = login_of(&o);
        let server = server_of(&o);
        let banks: Vec<BankLine> = o.session.read(|st, _| st.rows::<BankLine>(&domain::MoneyView::Banks).to_vec());
        Household {
            who: who(&o),
            picture: match (&login, &server) {
                (Some(l), Some(s)) => ark_ffi::ark_auth::picture_url(s, &l.user, &l.picture).unwrap_or_default(),
                _ => String::new(),
            },
            detail: match (&login, demo) {
                (_, true) => "The demo's household, on this phone alone".into(),
                (Some(l), _) if !l.email.is_empty() => l.email.clone(),
                (Some(l), _) => l.user.clone(),
                (None, _) => String::new(),
            },
            server: server.map(|s| ark_ffi::ark_auth::probe::display(&s).to_string()),
            linked: Some(o.peer.status().linked),
            demo,
            banks_wanting: money_core::words::banks_wanting(&banks),
            syncing: self.money.syncing.load(Ordering::Acquire),
        }
    }

    /// The session's day (`2026-10-26`): the demo's in the demo.
    pub fn today(&self) -> String {
        self.money.opened().map(|o| o.today.clone()).unwrap_or_else(crate::money_today)
    }

    /// The month the budget shows (`2026-10`).
    pub fn month(&self) -> String {
        self.money.opened().map(|o| lock(&o.month).clone()).unwrap_or_default()
    }

    /// Ask the bank for anything new (pulled down): what came of it said.
    pub fn sync_bank(&self) {
        lock(&self.money.jobs).push_back(Job::Request(MoneyRequest::Refresh));
        self.money.settle();
    }
}
