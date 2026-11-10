//! money in the browser: the shell every platform runs
//! (`money_app::shell::MoneyShell`) and the Accounts slice
//! (`money_app::accounts`), run by ark-app here as on the phones and the
//! desktop, drawn by arkui-web (Leptos over the DOM); the screens the
//! shell hosts that are not in Rust's vocabulary yet are this crate's
//! (`screens`), as money/iced has its own. The household is the demo's
//! (`money_core::demo`), in the page's memory, its own authority: no server.
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

use ark_app::{Host, ModuleCall, Runtime, Ticket};
use ark_peer::store::Store;
use ark_peer::Peer as Replica;
use arkui_web::{AnyView, WebApp};
use money_app::accounts::{Accounts, AccountsMsg};
use money_app::shell::view::{commands, money_shell};
use money_app::shell::{MoneyShell, ShellMsg};
use money_core::dates::{add_months, day_of_ms, days_from_civil, parse_day};
use money_core::*;

mod screens;

/// Everything the page can be told.
#[derive(Clone, Debug)]
pub enum Msg {
    Shell(ShellMsg),
    Accounts(AccountsMsg),
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
    pub category: Option<Id<Category>>,
    pub txn: Option<Id<Txn>>,
    pub search: String,
    pub review: bool,
    pub draft: Option<Draft>,
    pub asking: Option<Asking>,
    pub settings: bool,
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
    fn open(today: String) -> MoneyWeb {
        let now = noon(&today);
        let mut peer = money_core::demo::open(money_core::demo::seeded_disk(&today, now));
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
            category: None,
            txn: None,
            search: String::new(),
            review: false,
            draft: None,
            asking: None,
            settings: false,
        };
        app.shell_step(ShellMsg::Month { month });
        app
    }

    pub fn now_ms(&self) -> i64 {
        noon(&self.today)
    }

    /// A change sent to the household, the views brought up to it.
    fn change(&mut self, change: MoneyChange) -> Result<(), String> {
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
        let mut rt = std::mem::take(&mut self.shell);
        let mut desk = Desk { app: self, natives: vec![] };
        rt.dispatch(msg, &mut desk);
        let natives = std::mem::take(&mut desk.natives);
        self.shell = rt;
        for n in natives {
            self.native(&n);
        }
    }

    fn accounts_step(&mut self, msg: AccountsMsg) {
        let mut rt = std::mem::take(&mut self.accounts);
        let mut desk = Desk { app: self, natives: vec![] };
        rt.dispatch(msg, &mut desk);
        let natives = std::mem::take(&mut desk.natives);
        self.accounts = rt;
        for n in natives {
            self.native(&n);
        }
    }

    /// What the shell asked of this page's own screens, by name.
    fn native(&mut self, name: &str) {
        let (word, rest) = name.split_once(' ').unwrap_or((name, ""));
        match word {
            "new-transaction" => self.update(Msg::NewTxn),
            "new-account" => self.accounts_step(AccountsMsg::Add),
            "previous-month" => self.update(Msg::Month(-1)),
            "next-month" => self.update(Msg::Month(1)),
            "this-month" => self.update(Msg::ThisMonth),
            "auto-assign" => arkui_web::keys::open_menu("auto-assign"),
            "refresh" => {
                let _ = self.change(MoneyChange::Sync);
            }
            "filter" | "review" => self.review = !self.review || word == "review",
            "find" => {
                self.shell_step(ShellMsg::Select { tag: "transactions".into() });
            }
            "search" => self.search = rest.to_string(),
            "open-category" => {
                let n = rest.to_lowercase();
                let found = screens::lines(self)
                    .iter()
                    .find(|l| l.name.to_lowercase().contains(&n))
                    .map(|l| l.category_id);
                self.category = found;
            }
            "open-transaction" => {
                // `receipt`: the first with a receipt (the reference's word);
                // else by its payee or its id.
                let n = rest.to_lowercase();
                let found = screens::all_txns(self)
                    .iter()
                    .find(|t| match n.as_str() {
                        "receipt" => !t.receipt.is_empty() || t.photos > 0,
                        _ => t.payee.to_lowercase().contains(&n) || t.id.hex() == rest,
                    })
                    .map(|t| t.id);
                self.txn = found;
            }
            "settings" => {
                self.settings = true;
                if !rest.is_empty() {
                    self.shell_step(ShellMsg::Pane { title: rest.to_string() });
                }
            }
            "appearance" => {
                if let Some(root) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element()) {
                    let _ = root.set_attribute("data-theme", rest);
                }
            }
            "select" => self.shell_step(ShellMsg::Select { tag: rest.to_string() }),
            _ => {}
        }
    }
}

/// The page as the runtimes' host for one message.
struct Desk<'a> {
    app: &'a mut MoneyWeb,
    natives: Vec<String>,
}

impl<A: ark_app::App<Key = MoneyView, Change = MoneyChange, Route = money_app::Route>> Host<A> for Desk<'_> {
    fn read<R>(&self, f: impl FnOnce(&Store<MoneyView>) -> R) -> R {
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
            R::Reflect => Some("insights".into()),
            R::Accounts | R::Inbox | R::Filtered | R::Filter { .. } => Some("transactions".into()),
            R::Account { id, .. } => Some(money_app::shell::account_tag(&id)),
            R::Settings | R::Banks | R::Bank { .. } => {
                self.natives.push("settings Connected Banks".into());
                None
            }
            _ => None,
        };
        if let Some(tag) = tag {
            self.natives.push(format!("select {tag}"));
        }
    }
    fn ask(&mut self, _: Ticket, _: ModuleCall) {}
    fn native(&mut self, name: &str) {
        self.natives.push(name.into());
    }
}

impl WebApp for MoneyWeb {
    type Msg = Msg;

    fn view(&self) -> AnyView {
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
            Msg::Category(c) => {
                self.category = c;
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

    fn arrive(&mut self, query: &dyn Fn(&str) -> Option<String>) -> Vec<Msg> {
        let mut args = vec![];
        for key in ["screen", "inspector", "appearance", "pane", "settingsTab"] {
            if let Some(v) = query(key) {
                args.push(format!("-{key}"));
                args.push(v);
            }
        }
        match args.is_empty() {
            true => vec![],
            false => vec![Msg::Shell(ShellMsg::Launch { args })],
        }
    }
}

fn main() {
    let today = arkui_web::query("today")
        .filter(|d| parse_day(d).is_some())
        .unwrap_or_else(|| day_of_ms(ark_peer::now_ms()));
    let app = MoneyWeb::open(today);
    arkui_web::run(app, "green", include_str!("../money.css"));
    if let Some(id) = arkui_web::query("popover") {
        arkui_web::after_draw(move || arkui_web::fields::open_popover(&id, 0));
    }
}
