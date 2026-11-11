//! money in the browser: the shell every platform runs
//! (`money_app::shell::MoneyShell`) and the Accounts slice
//! (`money_app::accounts`), run by ark-app here as on the phones and the
//! desktop, drawn by arkui-web (Leptos over the DOM); the screens the
//! shell hosts that are not in Rust's vocabulary yet are this crate's
//! (`screens`), as money/iced has its own.
//!
//! The household is the server's that served the page (`live`): the page
//! signs in through it as the phones do, holds a replica of the budget
//! and syncs over its socket. With `?demo` it is the demo's
//! (`money_core::demo`), in the page's memory, its own authority, with no
//! server: what the screenshots and the public demo run.
//!
//! The address opens any screen with no steps, with the launch arguments
//! every shell reads (`ark_app::shell::Launch`): `?screen=budget`,
//! `?screen=account:checking`, `?screen=category:groceries`,
//! `?screen=transaction:target`, `?screen=review`,
//! `?screen=add-transaction`, `?screen=settings&pane=household`,
//! `?inspector=1`, `?appearance=dark`; and arkui-web's own `?context=<row>`
//! and `?menu=<id>` for a row's context menu and a control's menu,
//! `?popover=<id>` for a field's popover, `?today=2026-11-10` for the day
//! the demo is seeded to.

use std::any::Any;
use std::sync::{Arc, Mutex};

use ark_app::{Ask, Host, Modules, Runtime, Ticket};
use ark_peer::store::Store;
use ark_peer::Peer as Replica;
use arkui_web::{AnyView, WebApp};
use money_app::accounts::{Accounts, AccountsMsg};
use money_app::budget::{Budget, BudgetMsg};
use money_app::shell::view::{commands, money_shell};
use money_app::shell::{MoneyShell, ShellMsg};
use money_app::MoneyRequest;
use money_core::dates::{add_months, days_from_civil, parse_day};
use money_core::*;

pub mod live;
pub mod screens;

/// Everything the page can be told.
#[derive(Clone, Debug)]
pub enum Msg {
    Shell(ShellMsg),
    Accounts(AccountsMsg),
    /// The Budget page's (`money_app::budget`).
    Budget(BudgetMsg),
    /// A category chosen in the budget (its inspector shows it), or none.
    Category(Option<Id<Category>>),
    /// A transaction chosen in a register, or none.
    Txn(Option<Id<Txn>>),
    Month(i64),
    ThisMonth,
    Auto(Auto),
    Assign(Id<Category>, i64),
    /// Approve All asked: the question raised.
    AskApprove,
    Approve(Vec<Id<Txn>>),
    /// A question or a sheet put away.
    Dismiss,
    Cleared(Id<Txn>, bool),
    Categorize(Id<Txn>, Option<Id<Category>>),
    /// The register only what waits for a look, or all of it.
    Review(bool),
    /// New Transaction's sheet raised.
    NewTxn,
    Draft(DraftMsg),
    SaveDraft,
    /// Settings shown, or put away.
    Settings(bool),
    /// Where the address says the page is besides the shell's launch
    /// arguments (`MoneyWeb::route`): a key and its value.
    At(String, String),
    /// The socket has something to say, or the peer's deadline came
    /// (`live`): it is pumped.
    Wake,
    /// What came of one of the sign-in machine's effects, or a press on
    /// the sign-in page (`live`).
    Onboard(ark_auth::onboarding::Event),
    /// A press on the first run's page.
    Welcome(ark_app::welcome::WelcomeMsg),
}

/// A change to the new transaction being written.
#[derive(Clone, Debug)]
pub enum DraftMsg {
    Payee(String),
    Amount(String),
    Date(String),
    Category(Option<Id<Category>>),
    Account(Id<Account>),
    Memo(String),
}

/// A transaction being written.
#[derive(Clone, Debug, Default)]
pub struct Draft {
    pub payee: String,
    pub amount: String,
    pub date: String,
    pub category: Option<Id<Category>>,
    pub account: Option<Id<Account>>,
    pub memo: String,
    pub problem: Option<String>,
}

/// A question asked over the page.
#[derive(Clone, Debug, PartialEq)]
pub enum Asking {
    ApproveAll(usize),
}

pub struct MoneyWeb {
    pub peer: Replica,
    pub store: Store<MoneyView>,
    pub today: String,
    pub month: String,
    pub shell: Runtime<MoneyShell>,
    pub accounts: Runtime<Accounts>,
    pub budget: Runtime<Budget>,
    pub category: Option<Id<Category>>,
    pub txn: Option<Id<Txn>>,
    pub search: String,
    pub review: bool,
    pub draft: Option<Draft>,
    pub asking: Option<Asking>,
    pub settings: bool,
    /// How many changes this page has sent the household.
    pub changes: u64,
    /// Changes counted and not sent (`WebApp::dry_run`).
    pub dry: bool,
    /// The page on its server, signed in there; none for the demo.
    pub live: Option<live::Live>,
}

/// The views the page holds as long as it is open.
const ALWAYS: [MoneyView; 6] = [
    MoneyView::Accounts,
    MoneyView::Groups,
    MoneyView::Payees,
    MoneyView::Tree,
    MoneyView::Banks,
    MoneyView::Register { account: None },
];

/// Noon on `today`, in milliseconds: the time a screen is worked out at.
pub fn noon(today: &str) -> i64 {
    let (y, m, d) = parse_day(today).unwrap_or((1970, 1, 1));
    days_from_civil(y, m, d) * 86_400_000 + 43_200_000
}

impl MoneyWeb {
    pub fn open(today: String) -> MoneyWeb {
        let now = noon(&today);
        // The household seeded once a day and opened afresh from it: the
        // prerenderer opens one for every control it presses.
        thread_local! {
            static SEEDED: std::cell::RefCell<Option<(String, ark_peer::Memory)>> = const { std::cell::RefCell::new(None) };
        }
        let disk = SEEDED.with(|s| {
            let mut s = s.borrow_mut();
            match &*s {
                Some((day, disk)) if *day == today => disk.clone(),
                _ => {
                    let disk = money_seed::seeded_disk(&today, now);
                    *s = Some((today.clone(), disk.clone()));
                    disk
                }
            }
        });
        MoneyWeb::over(money_seed::open(disk), today)
    }

    /// The page over `peer`: its views held, the shell on this month.
    pub fn over(mut peer: Replica, today: String) -> MoneyWeb {
        let mut store = Store::new();
        for v in ALWAYS {
            store.hold(&mut peer, v);
        }
        let month = today[..7].to_string();
        store.hold(&mut peer, MoneyView::Budget { month: month.clone() });
        store.settle(&mut peer);
        let mut app = MoneyWeb {
            peer,
            store,
            today,
            month: month.clone(),
            shell: Runtime::new(),
            accounts: Runtime::new(),
            budget: Runtime::new(),
            category: None,
            txn: None,
            search: String::new(),
            review: false,
            draft: None,
            asking: None,
            settings: false,
            changes: 0,
            dry: false,
            live: None,
        };
        app.shell_step(ShellMsg::Month { month });
        app
    }

    pub fn now_ms(&self) -> i64 {
        noon(&self.today)
    }

    /// A change sent to the household, the views brought up to it.
    fn change(&mut self, change: MoneyChange) -> Result<(), String> {
        self.changes += 1;
        if self.dry {
            return Ok(());
        }
        let r = self.peer.mutate_bytes(change.name(), change.input()).map(|_| ());
        self.store.settle(&mut self.peer);
        r
    }

    fn set_month(&mut self, month: String) {
        if month == self.month {
            return;
        }
        let old = MoneyView::Budget { month: self.month.clone() };
        self.store.hold(&mut self.peer, MoneyView::Budget { month: month.clone() });
        self.store.release(&mut self.peer, &old);
        self.store.settle(&mut self.peer);
        self.month = month.clone();
        self.shell_step(ShellMsg::Month { month });
    }

    pub fn shell_page(&self) -> money_app::shell::ShellPage {
        self.shell.screen_of(&self.store, self.now_ms())
    }

    fn shell_step(&mut self, msg: ShellMsg) {
        let searching = matches!(msg, ShellMsg::Search { .. } | ShellMsg::ShowActivity);
        let launching = matches!(msg, ShellMsg::Launch { .. });
        let mut rt = std::mem::take(&mut self.shell);
        let mut desk = Desk::new(self);
        rt.dispatch(msg, &mut desk);
        desk.answer(&mut rt);
        let natives = std::mem::take(&mut desk.natives);
        self.shell = rt;
        if searching {
            self.search = self.shell.state().search.clone();
        }
        if launching {
            self.appearance();
        }
        for n in natives {
            self.carry(n);
        }
        self.follow_shell();
    }

    /// What the shell chose to inspect, taken as this page's choice: a
    /// launch's `-screen category:dining -inspector 1` names it to the
    /// shell alone. A category is the shell's Category Details
    /// (`category_inspector`, drawn from `ShellPage::category`); a
    /// transaction is this page's own inspector (`screens::txn_inspector`,
    /// which approves), so it is taken from the shell and the shell's let go.
    fn follow_shell(&mut self) {
        let s = self.shell.state();
        let (category, transaction) = (s.category.clone(), s.transaction.clone());
        if let Some(key) = category {
            self.category = money_app::shell::inspect::category_named(screens::lines(self), &key).map(|l| l.category_id);
        }
        if let Some(key) = transaction {
            self.txn = money_app::shell::inspect::transaction_named(screens::all_txns(self), &key).map(|t| t.id);
            let mut rt = std::mem::take(&mut self.shell);
            let mut desk = Desk::new(self);
            rt.dispatch(ShellMsg::InspectTransaction { id: None }, &mut desk);
            self.shell = rt;
        }
    }

    fn budget_step(&mut self, msg: BudgetMsg) {
        let mut rt = std::mem::take(&mut self.budget);
        let mut desk = Desk::new(self);
        if rt.state().month != desk.app.month {
            let month = desk.app.month.clone();
            rt.dispatch(BudgetMsg::Month { month }, &mut desk);
        }
        rt.dispatch(msg, &mut desk);
        desk.answer(&mut rt);
        let natives = std::mem::take(&mut desk.natives);
        self.budget = rt;
        for n in natives {
            self.carry(n);
        }
    }

    fn accounts_step(&mut self, msg: AccountsMsg) {
        let mut rt = std::mem::take(&mut self.accounts);
        let mut desk = Desk::new(self);
        rt.dispatch(msg, &mut desk);
        desk.answer(&mut rt);
        let natives = std::mem::take(&mut desk.natives);
        self.accounts = rt;
        for n in natives {
            self.carry(n);
        }
    }

    /// What money's runtimes asked of this page ([`MoneyRequest`]), or
    /// what its host made of a route.
    fn carry(&mut self, p: Pending) {
        let r = match p {
            Pending::Select(tag) => return self.shell_step(ShellMsg::Select { tag }),
            Pending::Inspect(id) => {
                let found = screens::lines(self).iter().find(|l| l.category_id.hex() == id).map(|l| l.category_id);
                return self.update(Msg::Category(found));
            }
            Pending::Request(r) => r,
        };
        match r {
            MoneyRequest::NewTransaction => self.update(Msg::NewTxn),
            MoneyRequest::Accounts { msg } => self.accounts_step(msg),
            MoneyRequest::Budget { msg } => self.budget_step(msg),
            MoneyRequest::StepMonth { by } => self.update(Msg::Month(by.into())),
            MoneyRequest::ThisMonth => self.update(Msg::ThisMonth),
            MoneyRequest::Refresh => {
                let _ = self.change(MoneyChange::Sync);
            }
            MoneyRequest::Filter => self.review = !self.review,
            MoneyRequest::Review => self.review = true,
            // The browser's reconcile and receipt import are still to come.
            MoneyRequest::Reconcile { .. } | MoneyRequest::ImportReceipt | MoneyRequest::KeepPhoto { .. } => {}
            MoneyRequest::OpenCategory { key } => {
                let n = key.to_lowercase();
                let found = screens::lines(self)
                    .iter()
                    .find(|l| l.name.to_lowercase().contains(&n))
                    .map(|l| l.category_id);
                self.category = found;
            }
            MoneyRequest::OpenTransaction { key } => {
                // `receipt`: the first with a receipt (the reference's word);
                // else by its payee or its id.
                let n = key.to_lowercase();
                let found = screens::all_txns(self)
                    .iter()
                    .find(|t| match n.as_str() {
                        "receipt" => !t.receipt.is_empty() || t.photos > 0,
                        _ => t.payee.to_lowercase().contains(&n) || t.id.hex() == key,
                    })
                    .map(|t| t.id);
                self.txn = found;
            }
            MoneyRequest::Settings { pane } => {
                self.settings = true;
                if !pane.is_empty() {
                    self.shell_step(ShellMsg::Pane { title: pane });
                }
            }
        }
    }

    /// Whether nothing is left to come on the page: no runtime waits on
    /// an operation and each holds every view it needs
    /// (`Runtime::is_idle`); with [`MoneyWeb::version`] unchanged, what a
    /// screenshot driver waits for.
    pub fn is_idle(&self) -> bool {
        self.shell.is_idle(&self.store) && self.budget.is_idle(&self.store) && self.accounts.is_idle(&self.store)
    }

    /// The runtimes' versions summed: grows with every message any took.
    pub fn version(&self) -> u64 {
        self.shell.version() + self.budget.version() + self.accounts.version()
    }

    /// The ready signal on the page's root, read by the screenshot
    /// drivers (ui/docs/app.md, Effects and Ports): `data-ark-idle` is `1`
    /// once nothing is left to come, `data-ark-version` the version drawn.
    fn say_idle(&self) {
        #[cfg(target_arch = "wasm32")]
        if let Some(root) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element()) {
            let _ = root.set_attribute("data-ark-idle", if self.is_idle() { "1" } else { "0" });
            let _ = root.set_attribute("data-ark-version", &self.version().to_string());
        }
    }

    /// The appearance the shell says, over the system's.
    fn appearance(&self) {
        #[cfg(target_arch = "wasm32")]
        if let Some(a) = self.shell.state().appearance {
            use money_app::shell::Appearance;
            if let Some(root) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element()) {
                let _ = root.set_attribute("data-theme", if a == Appearance::Dark { "dark" } else { "light" });
            }
        }
    }
}

/// What the page's host made of a command, carried out after the message.
enum Pending {
    /// A section shown, by its tag.
    Select(String),
    /// A category's month in Category Details, by its hex id.
    Inspect(String),
    /// What a runtime asked.
    Request(MoneyRequest),
}

/// What the address's path says after the section, each a key and its
/// value (`MoneyWeb::route`): what the page shows.
const PATH_KEYS: [&str; 3] = ["category", "txn", "settings"];

/// What its query says: the UI's state over it, in the order read back.
const QUERY_KEYS: [&str; 11] = [
    "review",
    "search",
    "ask",
    "sheet",
    "payee",
    "amount",
    "date",
    "draft-category",
    "account",
    "memo",
    "month",
];

impl MoneyWeb {
    /// Where the page is, as its address (ui/docs/app.md, "The Browser's
    /// Modes"). The path is what it shows: the section's tag
    /// (`budget`, `accounts/<id>`), the month when it is not this one, the
    /// category or the transaction chosen, Settings and its pane —
    /// `/budget/2026-10/category/<id>/`. The query is the UI over it: the
    /// inspector, the review, the search, the question, the sheet and what
    /// is written in it. `arrive` reads it back.
    pub fn route(&self) -> arkui_web::Address {
        let s = self.shell.state();
        let mut path: Vec<String> = s.section.split('/').filter(|p| !p.is_empty()).map(String::from).collect();
        if self.month != self.today[..7] {
            path.push(self.month.clone());
        }
        if let Some(c) = self.category {
            path.extend(["category".into(), c.hex()]);
        }
        if let Some(t) = self.txn {
            path.extend(["txn".into(), t.hex()]);
        }
        if self.settings {
            path.extend(["settings".into(), s.pane.clone()]);
        }
        let mut r = arkui_web::Address::at(path);
        if s.inspector {
            r = r.with("inspector", "1");
        }
        if self.review {
            r = r.with("review", "1");
        }
        if !self.search.is_empty() {
            r = r.with("search", self.search.clone());
        }
        if let Some(Asking::ApproveAll(_)) = self.asking {
            r = r.with("ask", "approve-all");
        }
        if let Some(d) = &self.draft {
            r = r.with("sheet", "add-transaction");
            for (k, v) in [("payee", &d.payee), ("amount", &d.amount), ("date", &d.date), ("memo", &d.memo)] {
                if !v.is_empty() {
                    r = r.with(k, v.clone());
                }
            }
            if let Some(c) = d.category {
                r = r.with("draft-category", c.hex());
            }
            if let Some(a) = d.account {
                r = r.with("account", a.hex());
            }
        }
        r
    }

    /// One of the address's keys taken (`route`'s, read back).
    fn at(&mut self, key: &str, v: String) {
        let category = |app: &MoneyWeb| screens::lines(app).iter().find(|l| l.category_id.hex() == v).map(|l| l.category_id);
        match key {
            "month" if dates_ok(&v) => self.set_month(v),
            "category" => {
                let c = category(self);
                self.category = c;
                self.shell_step(ShellMsg::InspectCategory { id: c.map(|c| c.hex()) });
            }
            "draft-category" => {
                let c = category(self);
                if let Some(d) = &mut self.draft {
                    d.category = c;
                }
            }
            "txn" => self.txn = screens::all_txns(self).iter().find(|t| t.id.hex() == v).map(|t| t.id),
            "review" => self.review = v == "1",
            "search" => self.search = v,
            "settings" => {
                self.settings = true;
                if !v.is_empty() {
                    self.shell_step(ShellMsg::Pane { title: v });
                }
            }
            "ask" if v == "approve-all" => self.update(Msg::AskApprove),
            "sheet" if v == "add-transaction" => {
                if self.draft.is_none() {
                    self.update(Msg::NewTxn);
                }
            }
            "account" => {
                let a = screens::open_accounts(self).into_iter().find(|a| a.id.hex() == v).map(|a| a.id);
                if let (Some(d), Some(a)) = (&mut self.draft, a) {
                    d.account = Some(a);
                }
            }
            k @ ("payee" | "amount" | "date" | "memo") => {
                if let Some(d) = &mut self.draft {
                    match k {
                        "payee" => d.payee = v,
                        "amount" => d.amount = v,
                        "date" => d.date = v,
                        _ => d.memo = v,
                    }
                }
            }
            _ => {}
        }
    }
}

fn dates_ok(month: &str) -> bool {
    month.len() == 7 && parse_day(&format!("{month}-01")).is_some()
}

/// The page as the runtimes' host for one message.
struct Desk<'a> {
    app: &'a mut MoneyWeb,
    natives: Vec<Pending>,
    /// What the operations asked became, each its runtime's message
    /// (`Option<A::Msg>`), handed back after the message ([`Desk::answer`]).
    answered: Arc<Mutex<Answered>>,
}

/// Each ticket and what its operation became, its type its runtime's.
type Answered = Vec<(Ticket, Box<dyn Any + Send>)>;

impl<'a> Desk<'a> {
    fn new(app: &'a mut MoneyWeb) -> Desk<'a> {
        Desk {
            app,
            natives: vec![],
            answered: Arc::default(),
        }
    }

    /// Every operation's answer taken into `rt`, so it waits on none.
    fn answer<A: ark_app::App<Key = MoneyView, Change = MoneyChange, Route = money_app::Route, Request = MoneyRequest>>(
        &mut self,
        rt: &mut Runtime<A>,
    ) {
        loop {
            let answered = std::mem::take(&mut *self.answered.lock().unwrap_or_else(|e| e.into_inner()));
            if answered.is_empty() {
                return;
            }
            for (t, m) in answered {
                if let Ok(m) = m.downcast::<Option<A::Msg>>() {
                    rt.answer(t, *m, self);
                }
            }
        }
    }
}

impl<A: ark_app::App<Key = MoneyView, Change = MoneyChange, Route = money_app::Route, Request = MoneyRequest>> Host<A> for Desk<'_> {
    fn read<R>(&self, f: impl FnOnce(&dyn ark::views::Views<MoneyView>) -> R) -> R {
        f(&self.app.store)
    }
    fn now_ms(&self) -> i64 {
        self.app.now_ms()
    }
    fn send(&mut self, change: MoneyChange) -> Result<(), String> {
        self.app.change(change)
    }
    fn open(&mut self, route: money_app::Route) {
        use money_app::Route as R;
        let tag = match route {
            R::Budget => Some("budget".to_string()),
            R::Insights => Some("insights".into()),
            R::Accounts | R::Inbox | R::Filtered | R::Filter { .. } => Some("transactions".into()),
            R::Account { id, .. } => Some(money_app::shell::account_tag(&id)),
            // A category's month: Category Details, beside the budget.
            R::Category { id, .. } => {
                self.natives.push(Pending::Inspect(id));
                None
            }
            R::Settings | R::Banks | R::Bank { .. } => {
                self.natives.push(Pending::Request(MoneyRequest::Settings {
                    pane: "Connected Banks".into(),
                }));
                None
            }
            _ => None,
        };
        if let Some(tag) = tag {
            self.natives.push(Pending::Select(tag));
        }
    }
    fn ask(&mut self, ticket: Ticket, ask: Ask<A::Msg>) {
        // The page registers no module yet: answered Unavailable.
        let answered = self.answered.clone();
        ask.run(&Modules::new(), move |m| {
            answered.lock().unwrap_or_else(|e| e.into_inner()).push((ticket, Box::new(m)))
        })
    }
    fn request(&mut self, r: MoneyRequest) {
        self.natives.push(Pending::Request(r));
    }
}

impl WebApp for MoneyWeb {
    type Msg = Msg;

    fn view(&self) -> AnyView {
        self.say_idle();
        if let Some(live) = self.live.as_ref().filter(|l| !l.running()) {
            return self.welcome(live);
        }
        let page = self.shell_page();
        let hosts = |name: &'static str| screens::hosted(self, name);
        let window = arkui_web::shell(money_shell::view(&page).map(Msg::Shell), &hosts);
        let over = screens::over(self, &page);
        leptos::prelude::IntoAny::into_any((window, over))
    }

    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Shell(ShellMsg::Select { tag }) => {
                self.txn = None;
                self.shell_step(ShellMsg::Select { tag });
            }
            Msg::Shell(m) => self.shell_step(m),
            Msg::Accounts(m) => self.accounts_step(m),
            Msg::Budget(m) => self.budget_step(m),
            Msg::Category(c) => {
                self.category = c;
                // The shell's Category Details shows it (`follow_shell`).
                self.shell_step(ShellMsg::InspectCategory { id: c.map(|c| c.hex()) });
                if c.is_some() && !self.shell.state().inspector {
                    self.shell_step(ShellMsg::Inspector { shown: true });
                }
            }
            Msg::Txn(t) => {
                self.txn = t;
                if t.is_some() && !self.shell.state().inspector {
                    self.shell_step(ShellMsg::Inspector { shown: true });
                }
            }
            Msg::Month(d) => self.set_month(add_months(&self.month, d)),
            Msg::ThisMonth => self.set_month(self.today[..7].to_string()),
            Msg::Auto(how) => {
                let _ = self.change(MoneyChange::AutoAssign(AutoAssign {
                    month: self.month.clone(),
                    how,
                    only: vec![],
                }));
            }
            Msg::Assign(category_id, amount) => {
                let _ = self.change(MoneyChange::Assign(Assign {
                    month: self.month.clone(),
                    category_id,
                    amount,
                }));
            }
            Msg::AskApprove => {
                let n = screens::all_txns(self).iter().filter(|t| !t.approved).count();
                self.asking = Some(Asking::ApproveAll(n));
            }
            Msg::Approve(ids) => {
                self.asking = None;
                let _ = self.change(MoneyChange::Approve(Txns { ids }));
            }
            Msg::Dismiss => {
                self.asking = None;
                self.draft = None;
            }
            Msg::Cleared(id, cleared) => {
                let _ = self.change(MoneyChange::SetCleared(SetCleared { id, cleared }));
            }
            Msg::Categorize(id, category_id) => {
                let _ = self.change(MoneyChange::Categorize(Categorize { ids: vec![id], category_id }));
            }
            Msg::Review(on) => self.review = on,
            Msg::NewTxn => {
                let account = screens::open_accounts(self).first().map(|a| a.id);
                self.draft = Some(Draft {
                    date: self.today.clone(),
                    account,
                    ..Draft::default()
                });
            }
            Msg::Draft(d) => {
                if let Some(f) = &mut self.draft {
                    match d {
                        DraftMsg::Payee(p) => {
                            // A payee known, its usual category with it.
                            let usual = self
                                .store
                                .rows::<PayeeLine>(&MoneyView::Payees)
                                .iter()
                                .find(|l| l.name == p)
                                .and_then(|l| l.category_id);
                            if f.category.is_none() {
                                f.category = usual;
                            }
                            f.payee = p;
                        }
                        DraftMsg::Amount(a) => f.amount = a,
                        DraftMsg::Date(d) => f.date = d,
                        DraftMsg::Category(c) => f.category = c,
                        DraftMsg::Account(a) => f.account = Some(a),
                        DraftMsg::Memo(m) => f.memo = m,
                    }
                    f.problem = None;
                }
            }
            Msg::SaveDraft => {
                let Some(f) = self.draft.clone() else { return };
                let cents = ark_format::parse_amount(&f.amount, &ark_format::NumberStyle::us());
                let (Some(account_id), Some(cents)) = (f.account, cents) else {
                    if let Some(d) = &mut self.draft {
                        d.problem = Some("An amount, as 12.50".into());
                    }
                    return;
                };
                let r = self.change(MoneyChange::AddTransaction(AddTransaction {
                    account_id,
                    date: f.date,
                    payee: f.payee,
                    category_id: f.category,
                    memo: f.memo,
                    amount: -cents.abs(),
                    cleared: false,
                    transfer_to: None,
                }));
                match r {
                    Ok(()) => self.draft = None,
                    Err(why) => {
                        if let Some(d) = &mut self.draft {
                            d.problem = Some(why);
                        }
                    }
                }
            }
            Msg::Settings(on) => self.settings = on,
            Msg::At(k, v) => self.at(&k, v),
            Msg::Wake => self.wake(),
            Msg::Onboard(e) => self.onboard(e),
            Msg::Welcome(m) => self.first_run(m),
        }
    }

    fn commands(&self) -> Vec<ark_app::shell::CommandMenu<Msg>> {
        commands()
            .into_iter()
            .map(|m| ark_app::shell::CommandMenu {
                place: m.place,
                items: m
                    .items
                    .into_iter()
                    .map(|c| ark_app::shell::CommandItem {
                        title: c.title,
                        shortcut: c.shortcut,
                        run: Msg::Shell(c.run),
                    })
                    .collect(),
            })
            .collect()
    }

    fn suggest(&self, field: &str, typed: &str) -> Vec<String> {
        self.suggestions(field, typed)
    }

    fn arrive(&mut self, route: &arkui_web::Address) -> Vec<Msg> {
        // The launch arguments every shell reads, as the query says them
        // (`?screen=budget&inspector=1`).
        let mut args = vec![];
        for key in ["screen", "inspector", "appearance", "pane", "settingsTab"] {
            if let Some(v) = route
                .get(key)
                .map(str::to_string)
                .or_else(|| arkui_web::page::args().into_iter().find(|(k, _)| k == key).map(|(_, v)| v))
            {
                args.push(format!("-{key}"));
                args.push(v);
            }
        }
        let mut out = vec![];
        // The path: the section's tag, then what it shows.
        let mut segs = route.path.iter().map(String::as_str).peekable();
        let section = match segs.next() {
            Some("accounts") if segs.peek().is_some_and(|s| !PATH_KEYS.contains(s)) => segs.next().map(money_app::shell::account_tag),
            Some(s) => Some(s.to_string()),
            None => None,
        };
        if let Some(tag) = section {
            out.push(Msg::Shell(ShellMsg::Select { tag }));
        }
        if !args.is_empty() {
            out.push(Msg::Shell(ShellMsg::Launch { args }));
        }
        while let Some(seg) = segs.next() {
            match seg {
                k if PATH_KEYS.contains(&k) => out.push(Msg::At(k.into(), segs.next().unwrap_or_default().into())),
                m if dates_ok(m) => out.push(Msg::At("month".into(), m.into())),
                _ => {}
            }
        }
        for key in QUERY_KEYS {
            if let Some(v) = route.get(key) {
                out.push(Msg::At(key.into(), v.into()));
            }
        }
        out
    }

    fn route(&self) -> arkui_web::Address {
        self.route()
    }

    fn changes(&self) -> u64 {
        self.changes
    }

    fn dry_run(&mut self, on: bool) {
        self.dry = on;
    }

    fn rewind(&mut self) -> bool {
        self.set_month(self.today[..7].to_string());
        self.shell = Runtime::new();
        (self.category, self.txn, self.draft, self.asking) = (None, None, None, None);
        (self.search, self.review, self.settings) = (String::new(), false, false);
        self.accounts = Runtime::new();
        self.budget = Runtime::new();
        let month = self.month.clone();
        self.shell_step(ShellMsg::Month { month });
        true
    }
}

/// money's own styles, after ArkUI's.
pub const CSS: &str = include_str!("../money.css");
