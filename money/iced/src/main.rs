//! money, in iced — on the desktop and in a browser: a household's budget.
//!
//! Two builds of one window. Without features it is the household's
//! client: it signs in to a money server and dials it (`live.rs`), and
//! "Sync" asks the server's importer to ask the banks now. With `demo` it
//! is the demo: no server and no sign-in, a seeded household (`money_domain::seed`)
//! on a peer that is its own authority, and the card's bank standing in
//! for Plaid behind "Sync" — what GitHub Pages publishes, and a separate
//! build on purpose, as harken's is: a flag could be set by accident, a
//! feature has to be asked for at build time.
//!
//! The window is laid out the way YNAB taught people to read a budget —
//! the accounts down the side, the month's categories with what each was
//! assigned, spent and has, what is ready to assign across the top, and the
//! selected category's story on the right.
//!
//! None of the budget's arithmetic is here. Every figure on screen is a
//! view of the domain (`money/domain`), kept up to date by the rows a
//! change touched; every button is a mutation by name, and a refusal is
//! the domain's own sentence, said at the bottom of the sidebar.
//!
//! Most of what a window is lives in `arkui`: the palette and the rule that
//! nothing writes a colour down, the table's cells and rows, the glyphs,
//! the layers a panel floats on. What is here is money's: its pages, its
//! messages, its forms.

mod amount;
mod client;
mod live;
mod look;
mod palette;
mod peer;
mod view;

#[cfg(test)]
mod tests;

use iced::keyboard::{key::Named, Key};
use iced::{Subscription, Task};
use money_domain::dates::{add_months, day_of_ms, month_of};
#[cfg(any(feature = "demo", test))]
use money_domain::seed;
use money_domain::*;

use peer::{Ledger, Peer};

/// What fills the page beside the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Budget,
    Reflect,
    Ledger(Ledger),
}

/// A field of the transaction form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Date,
    Payee,
    Memo,
    Outflow,
    Inflow,
}

/// A transaction being entered or corrected.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TxnForm {
    /// The one corrected; none for a new one.
    pub editing: Option<Id<Txn>>,
    pub account: Option<Id<Account>>,
    pub date: String,
    pub payee: String,
    pub category: Option<Id<Category>>,
    pub transfer_to: Option<Id<Account>>,
    pub memo: String,
    pub outflow: String,
    pub inflow: String,
    pub cleared: bool,
    /// Who entered the one corrected.
    pub by: String,
}

/// What a category chooser is choosing for.
#[derive(Debug, Clone, PartialEq)]
pub enum Choosing {
    /// The form's category (or transfer).
    Form,
    /// These transactions' category.
    Txns(Vec<Id<Txn>>),
    /// Where the inspector's category's money goes (or comes from).
    Move { from: Option<Id<Category>>, amount: String },
}

/// What a chooser offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Category(Option<Id<Category>>),
    Transfer(Id<Account>),
}

/// A panel over the page.
#[derive(Debug, Clone, PartialEq)]
pub enum Modal {
    AddAccount {
        name: String,
        kind: &'static str,
        balance: String,
        on_budget: bool,
    },
    Reconcile {
        account: Id<Account>,
        /// Whether the cleared balance was said not to be the bank's.
        differs: bool,
        balance: String,
    },
    Choose(Choosing),
}

#[derive(Debug, Clone)]
pub enum Message {
    Go(Page),
    /// A month earlier or later; zero is this one.
    Month(i64),
    ThisMonth,
    Select(Option<Id<Category>>),
    EditAssigned(Id<Category>),
    AssignedTyped(String),
    AssignedDone,
    Auto(Auto, Vec<Id<Category>>),
    /// Assign a category exactly this much this month.
    AssignTo(Id<Category>, i64),
    TargetTyped(String),
    TargetDone,
    Collapse(Id<Group>),
    NewGroup,
    NewGroupTyped(String),
    NewGroupDone,
    NewCategory(Id<Group>),
    NewCategoryTyped(String),
    NewCategoryDone,
    HideCategory(Id<Category>, bool),
    DeleteCategory(Id<Category>),
    ShowHidden(bool),
    MoveMoney,
    MoveTyped(String),
    AddTxn,
    EditTxn(Id<Txn>),
    Form(Field, String),
    FormCleared(bool),
    FormAccount(Id<Account>),
    SaveTxn,
    CancelTxn,
    DeleteTxn(Id<Txn>),
    ToggleCleared(Id<Txn>, bool),
    Approve(Vec<Id<Txn>>),
    Categorize(Vec<Id<Txn>>),
    ChooseCategory,
    Chose(Choice),
    Sync,
    AddAccount,
    AccountName(String),
    AccountKind(&'static str),
    AccountBalance(String),
    AccountOnBudget(bool),
    CreateAccount,
    Reconcile(Id<Account>),
    ReconcileAgrees(bool),
    ReconcileTyped(String),
    ReconcileDone,
    CloseModal,
    Escape,
    /// A scroll or click that only has to stop at a layer.
    Swallow,
    /// The socket said something, or the peer's deadline came: pump it.
    Wake,
    /// A sign-in came back.
    SignedIn(Result<ark_auth::Login, String>),
}

pub struct App {
    pub peer: Peer,
    pub page: Page,
    pub today: String,
    /// The category the inspector is about; none is the month's summary.
    pub selected: Option<Id<Category>>,
    /// An assigned cell being typed into, and what is typed.
    pub assigning: Option<(Id<Category>, String)>,
    /// The inspector's target, as typed.
    pub target: String,
    pub collapsed: Vec<Id<Group>>,
    pub show_hidden: bool,
    pub new_group: Option<String>,
    pub new_category: Option<(Id<Group>, String)>,
    pub form: Option<TxnForm>,
    pub modal: Option<Modal>,
    /// The demo's bank, standing in for Plaid.
    #[cfg(any(feature = "demo", test))]
    pub bank: seed::Bank,
    /// The server this window is on; none for the demo, its own authority.
    pub server: Option<live::Server>,
    /// The last thing worth saying: a refusal, a sync.
    pub note: String,
}

impl App {
    pub fn with_peer(client: client::Replica, today: &str) -> App {
        App {
            peer: Peer::open(client, &month_of(today)),
            page: Page::Budget,
            today: today.into(),
            selected: None,
            assigning: None,
            target: String::new(),
            collapsed: Vec::new(),
            show_hidden: false,
            new_group: None,
            new_category: None,
            form: None,
            modal: None,
            #[cfg(any(feature = "demo", test))]
            bank: seed::Bank::default(),
            server: None,
            note: "a demo \u{2014} nothing here leaves this window".into(),
        }
    }

    #[cfg(feature = "demo")]
    fn boot() -> (App, Task<Message>) {
        let today = day_of_ms(ark_peer::now_ms());
        (App::with_peer(seed::seeded(), &today), Task::none())
    }

    /// The window on its server, signed in as the login this device
    /// remembers there, or on its way to sign in.
    #[cfg(not(feature = "demo"))]
    fn boot() -> (App, Task<Message>) {
        let today = day_of_ms(ark_peer::now_ms());
        let (url, user) = live::config();
        let logins = live::logins();
        let login = logins.recall(&url);
        let (client, note) = match live::open(&url, login.as_ref()) {
            Ok(c) => (c, String::new()),
            // A replica that will not open is no reason to show nothing:
            // this window works in memory, and says so.
            Err(e) => {
                let mut c = client::open(Box::new(ark_peer::Memory::default()));
                c.connect(&ark_auth::socket_url(&url));
                if let Some(l) = &login {
                    c.login(ark_peer::who(l), l.token.clone());
                }
                (c, format!("the saved budget would not open ({e}) \u{2014} working in memory"))
            }
        };
        let mut app = App::on_server(client, &today, url, login);
        if let Some(s) = &mut app.server {
            (s.logins, s.user) = (Some(logins), user);
        }
        app.note = match (&app.server.as_ref().and_then(|s| s.login.as_ref()), note.is_empty()) {
            (_, false) => note,
            (Some(l), true) => format!("signed in as {}", live::who(l)),
            (None, true) => "signing in\u{2026}".into(),
        };
        let task = match app.server.as_ref().is_some_and(|s| s.login.is_none()) {
            true => app.sign_in(),
            false => Task::none(),
        };
        (app, task)
    }

    /// A window over `client`, a replica of the server at `url`, as `login`.
    pub fn on_server(client: client::Replica, today: &str, url: String, login: Option<ark_auth::Login>) -> App {
        let bell = arkui::wake::Bell::new();
        let mut client = client;
        client.set_waker(live::waker(&bell));
        let mut app = App::with_peer(client, today);
        app.server = Some(live::Server {
            url,
            login,
            logins: None,
            user: None,
            tried: false,
            bell,
        });
        app
    }

    /// Go to sign in, once a load: in a browser the page goes to the
    /// server's sign-in and comes back with a code (traded here when it
    /// has one); the desktop hands the browser the same page and listens on
    /// loopback for the code.
    pub fn sign_in(&mut self) -> Task<Message> {
        let Some(s) = &mut self.server else { return Task::none() };
        #[cfg(target_arch = "wasm32")]
        if let Some(code) = ark_auth::web::take_code() {
            s.tried = true;
            let url = s.url.clone();
            return Task::perform(async move { ark_auth::web::exchange(&url, &code).await }, Message::SignedIn);
        }
        if std::mem::replace(&mut s.tried, true) {
            return Task::none();
        }
        #[cfg(target_arch = "wasm32")]
        {
            ark_auth::web::go_sign_in(&s.url, s.user.as_deref());
            Task::none()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.note = "signing in \u{2014} look for a browser tab if one opened".into();
            let (url, user) = (s.url.clone(), s.user.clone());
            off_thread(move || ark_auth::client::login(&url, user.as_deref(), ark_auth::client::open_browser))
        }
    }

    /// Somebody signed in: the replica moves on as them — whatever was done
    /// signed out becomes theirs and is pushed — and the link dials.
    pub fn signed_in(&mut self, login: ark_auth::Login) {
        let Some(s) = &mut self.server else { return };
        if let Some(logins) = &s.logins {
            logins.remember(&s.url, &login);
        }
        self.peer.client.login(ark_peer::who(&login), login.token.clone());
        self.note = format!("signed in as {}", live::who(&login));
        s.login = Some(login);
        self.peer.refresh();
    }

    /// What the socket brought, taken in: the views moved, the server's
    /// refusals said, and a login the server turned away forgotten and
    /// signed in again (once).
    pub fn wake(&mut self) -> Task<Message> {
        let p = self.peer.client.pump();
        for (_, why) in self.peer.client.take_rejections() {
            self.note = format!("the server refused a change: {why}");
        }
        self.peer.refresh();
        let Some(why) = p.denied else { return Task::none() };
        if let Some(s) = &mut self.server {
            if let Some(logins) = &s.logins {
                logins.forget(&s.url);
            }
            s.login = None;
        }
        self.note = format!("the server turned the sign-in away ({why})");
        self.sign_in()
    }

    fn title(&self) -> String {
        "money".into()
    }

    /// Author a change by the function's name: whether it was taken. A
    /// refusal is the domain's sentence, said as it is.
    pub fn author(&mut self, what: &str, name: &str, input: &impl ark::format::Encode) -> bool {
        let taken = match self.peer.client.mutate(name, input) {
            Ok(_) => true,
            Err(e) => {
                self.note = format!("could not {what}: {e}");
                false
            }
        };
        for (_, why) in self.peer.client.take_rejections() {
            self.note = format!("could not {what}: {why}");
        }
        self.peer.refresh();
        taken
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Go(page) => {
                self.page = page;
                self.form = None;
                if let Page::Ledger(l) = page {
                    self.peer.set_ledger(l);
                }
            }
            Message::Month(n) => {
                let m = add_months(&self.peer.month, n);
                self.peer.set_month(&m);
                self.assigning = None;
            }
            Message::ThisMonth => self.peer.set_month(&month_of(&self.today)),
            Message::Select(c) => self.select(if self.selected == c { None } else { c }),
            Message::EditAssigned(c) => {
                let had = self.peer.line(c).map_or(0, |l| l.assigned);
                self.assigning = Some((c, amount::editable(had)));
                self.select(Some(c));
                return iced::widget::operation::focus(look::ASSIGNED);
            }
            Message::AssignedTyped(t) => {
                if let Some((_, typed)) = &mut self.assigning {
                    *typed = t;
                }
            }
            Message::AssignedDone => {
                if let Some((c, typed)) = self.assigning.take() {
                    match amount::parse(&typed) {
                        Some(cents) => {
                            let a = Assign {
                                month: self.peer.month.clone(),
                                category_id: c,
                                amount: cents,
                            };
                            self.author("assign", "assign", &a);
                        }
                        None => self.note = format!("{typed:?} is not an amount"),
                    }
                }
            }
            Message::AssignTo(c, cents) => {
                let a = Assign {
                    month: self.peer.month.clone(),
                    category_id: c,
                    amount: cents,
                };
                self.author("assign", "assign", &a);
            }
            Message::Auto(how, only) => {
                let a = AutoAssign {
                    month: self.peer.month.clone(),
                    how,
                    only,
                };
                self.author("assign the month", "auto_assign", &a);
            }
            Message::TargetTyped(t) => self.target = t,
            Message::TargetDone => {
                if let (Some(c), Some(cents)) = (self.selected, amount::parse(&self.target)) {
                    self.author("set the target", "set_target", &SetTarget { id: c, target: cents.max(0) });
                }
            }
            Message::Collapse(g) => match self.collapsed.iter().position(|x| *x == g) {
                Some(i) => drop(self.collapsed.remove(i)),
                None => self.collapsed.push(g),
            },
            Message::NewGroup => {
                self.new_group = Some(String::new());
                return iced::widget::operation::focus(look::NEW_GROUP);
            }
            Message::NewGroupTyped(t) => self.new_group = Some(t),
            Message::NewGroupDone => {
                if let Some(name) = self.new_group.take().filter(|n| !n.trim().is_empty()) {
                    self.author("add the group", "add_group", &AddGroup { name });
                }
            }
            Message::NewCategory(g) => {
                self.new_category = Some((g, String::new()));
                self.collapsed.retain(|x| *x != g);
                return iced::widget::operation::focus(look::NEW_CATEGORY);
            }
            Message::NewCategoryTyped(t) => {
                if let Some((_, typed)) = &mut self.new_category {
                    *typed = t;
                }
            }
            Message::NewCategoryDone => {
                if let Some((g, name)) = self.new_category.take().filter(|(_, n)| !n.trim().is_empty()) {
                    self.author("add the category", "add_category", &AddCategory { group_id: g, name });
                }
            }
            Message::HideCategory(c, hidden) => {
                self.author("hide the category", "hide_category", &HideCategory { id: c, hidden });
            }
            Message::DeleteCategory(c) => {
                if self.author("delete the category", "delete_category", &DeleteCategory { id: c, move_to: None }) {
                    self.selected = None;
                }
            }
            Message::ShowHidden(on) => self.show_hidden = on,
            Message::MoveMoney => {
                let from = self.selected;
                let available = from.and_then(|c| self.peer.line(c)).map_or(0, |l| l.available);
                self.modal = Some(Modal::Choose(Choosing::Move {
                    from,
                    amount: amount::editable(available.abs()),
                }));
            }
            Message::MoveTyped(t) => {
                if let Some(Modal::Choose(Choosing::Move { amount, .. })) = &mut self.modal {
                    *amount = t;
                }
            }
            Message::AddTxn => {
                let account = match self.peer.ledger {
                    Ledger::Account(a) => Some(a),
                    _ => self.peer.accounts.rows.iter().find(|a| a.on_budget && !a.closed).map(|a| a.id),
                };
                self.form = Some(TxnForm {
                    account,
                    date: self.today.clone(),
                    ..TxnForm::default()
                });
                return iced::widget::operation::focus(look::PAYEE);
            }
            Message::EditTxn(id) => {
                if self.form.as_ref().is_some_and(|f| f.editing == Some(id)) {
                    return Task::none();
                }
                if let Some(t) = self.peer.register.rows.iter().find(|t| t.id == id) {
                    let transfer_to = match t.transfer {
                        true => self.transfer_of(t.id),
                        false => None,
                    };
                    self.form = Some(TxnForm {
                        editing: Some(id),
                        account: Some(t.account_id),
                        date: t.date.clone(),
                        payee: if t.transfer { String::new() } else { t.payee.clone() },
                        category: t.category_id,
                        transfer_to,
                        memo: t.memo.clone(),
                        outflow: if t.amount < 0 { amount::editable(-t.amount) } else { String::new() },
                        inflow: if t.amount > 0 { amount::editable(t.amount) } else { String::new() },
                        cleared: t.cleared != Cleared::Uncleared,
                        by: t.user_id.clone(),
                    });
                }
            }
            Message::Form(field, value) => {
                if let Some(f) = &mut self.form {
                    match field {
                        Field::Date => f.date = value,
                        Field::Payee => {
                            // A payee somebody has paid before brings the
                            // category it was for last time.
                            if f.category.is_none() && f.transfer_to.is_none() {
                                f.category = self.peer.remembered(&value);
                            }
                            f.payee = value;
                        }
                        Field::Memo => f.memo = value,
                        Field::Outflow => (f.outflow, f.inflow) = (value, String::new()),
                        Field::Inflow => (f.inflow, f.outflow) = (value, String::new()),
                    }
                }
            }
            Message::FormCleared(c) => {
                if let Some(f) = &mut self.form {
                    f.cleared = c;
                }
            }
            Message::FormAccount(a) => {
                if let Some(f) = &mut self.form {
                    f.account = Some(a);
                    if f.transfer_to == Some(a) {
                        f.transfer_to = None;
                    }
                }
            }
            Message::SaveTxn => self.save_txn(),
            Message::CancelTxn => self.form = None,
            Message::DeleteTxn(id) => {
                if self.author("delete the transaction", "delete_transactions", &Txns { ids: vec![id] }) {
                    self.form = None;
                }
            }
            Message::ToggleCleared(id, cleared) => {
                self.author("clear it", "set_cleared", &SetCleared { id, cleared });
            }
            Message::Approve(ids) => {
                self.author("approve", "approve", &Txns { ids });
            }
            Message::Categorize(ids) => self.modal = Some(Modal::Choose(Choosing::Txns(ids))),
            Message::ChooseCategory => self.modal = Some(Modal::Choose(Choosing::Form)),
            Message::Chose(choice) => self.chose(choice),
            Message::Sync => self.sync(),
            Message::AddAccount => {
                self.modal = Some(Modal::AddAccount {
                    name: String::new(),
                    kind: "checking",
                    balance: String::new(),
                    on_budget: true,
                });
            }
            Message::AccountName(t) => {
                if let Some(Modal::AddAccount { name, .. }) = &mut self.modal {
                    *name = t;
                }
            }
            Message::AccountKind(k) => {
                if let Some(Modal::AddAccount { kind, on_budget, .. }) = &mut self.modal {
                    (*kind, *on_budget) = (k, accounts::on_budget_by_default(k));
                }
            }
            Message::AccountBalance(t) => {
                if let Some(Modal::AddAccount { balance, .. }) = &mut self.modal {
                    *balance = t;
                }
            }
            Message::AccountOnBudget(b) => {
                if let Some(Modal::AddAccount { on_budget, .. }) = &mut self.modal {
                    *on_budget = b;
                }
            }
            Message::CreateAccount => {
                if let Some(Modal::AddAccount {
                    name,
                    kind,
                    balance,
                    on_budget,
                }) = self.modal.clone()
                {
                    let Some(cents) = amount::parse(&balance) else {
                        self.note = format!("{balance:?} is not an amount");
                        return Task::none();
                    };
                    let owed = matches!(kind, "credit" | "loan");
                    let open = OpenAccount {
                        name,
                        kind: kind.into(),
                        on_budget,
                        balance: if owed { -cents.abs() } else { cents },
                        date: self.today.clone(),
                    };
                    if self.author("open the account", "open_account", &open) {
                        self.modal = None;
                    }
                }
            }
            Message::Reconcile(account) => {
                self.modal = Some(Modal::Reconcile {
                    account,
                    differs: false,
                    balance: String::new(),
                });
            }
            Message::ReconcileAgrees(agrees) => {
                let Some(Modal::Reconcile { account, .. }) = self.modal.clone() else {
                    return Task::none();
                };
                if agrees {
                    let balance = self.peer.account(account).map_or(0, |a| a.cleared);
                    self.reconcile(account, balance);
                } else if let Some(Modal::Reconcile { differs, .. }) = &mut self.modal {
                    *differs = true;
                }
            }
            Message::ReconcileTyped(t) => {
                if let Some(Modal::Reconcile { balance, .. }) = &mut self.modal {
                    *balance = t;
                }
            }
            Message::ReconcileDone => {
                if let Some(Modal::Reconcile { account, balance, .. }) = self.modal.clone() {
                    match amount::parse(&balance) {
                        Some(cents) => self.reconcile(account, cents),
                        None => self.note = format!("{balance:?} is not an amount"),
                    }
                }
            }
            Message::CloseModal => self.modal = None,
            Message::Escape => {
                if self.modal.is_some() {
                    self.modal = None;
                } else if self.assigning.is_some() {
                    self.assigning = None;
                } else if self.form.is_some() {
                    self.form = None;
                } else {
                    self.selected = None;
                }
            }
            Message::Swallow => {}
            Message::Wake => return self.wake(),
            Message::SignedIn(Ok(login)) => self.signed_in(login),
            // Said once, and not tried again until a reload: going straight
            // back would be a loop the person cannot stop.
            Message::SignedIn(Err(e)) => self.note = format!("could not sign in: {e} \u{2014} reload to try again"),
        }
        Task::none()
    }

    /// "Sync". On a server it is an ask (`banks.sync`) the server's
    /// importer answers by asking the banks; what they have arrives as any
    /// change does, and the bank's line in the sidebar says when. In the
    /// demo the demo's bank stands in for Plaid, and the import is authored
    /// here, this peer holding the importer's role.
    fn sync(&mut self) {
        if self.server.is_none() {
            return self.demo_sync();
        }
        if self.author("ask the bank to sync", "sync", &()) {
            self.note = "asked the server to ask the banks \u{2014} what they have arrives as it hears".into();
        }
    }

    #[cfg(any(feature = "demo", test))]
    fn demo_sync(&mut self) {
        let owed = (self.peer.accounts.rows.iter())
            .find(|a| a.bank == seed::BANK_NAME)
            .map_or(0, |a| a.cleared);
        let import = self.bank.next(&self.today, owed);
        let (n, u) = (import.upserts.len(), import.removed.len());
        if self.author("sync the bank", "import", &import) {
            self.note = match (n, self.bank.syncs) {
                (0, _) => "Chase: nothing new at the bank".into(),
                (_, 1) => format!("Chase: {n} new from the bank \u{2014} one still pending"),
                _ => format!("Chase: {n} from the bank, {u} pending one posted"),
            };
        }
    }

    /// No demo, no demo's bank: every window of this build is on a server.
    #[cfg(not(any(feature = "demo", test)))]
    fn demo_sync(&mut self) {}

    /// The inspector on a category, or on the month; its target field
    /// holding the category's target.
    fn select(&mut self, c: Option<Id<Category>>) {
        self.selected = c;
        self.target = c.and_then(|c| self.peer.line(c)).map(|l| amount::editable(l.target)).unwrap_or_default();
    }

    /// The other account of a transfer.
    fn transfer_of(&self, id: Id<Txn>) -> Option<Id<Account>> {
        let t = self.peer.register.rows.iter().find(|t| t.id == id)?;
        let name = t.payee.strip_prefix("Transfer : ")?;
        self.peer.accounts.rows.iter().find(|a| a.name == name).map(|a| a.id)
    }

    fn reconcile(&mut self, account: Id<Account>, balance: i64) {
        let r = Reconcile {
            account_id: account,
            balance,
            date: self.today.clone(),
        };
        if self.author("reconcile", "reconcile", &r) {
            self.modal = None;
            self.note = format!("reconciled at {}", amount::money(balance));
        }
    }

    fn chose(&mut self, choice: Choice) {
        let Some(Modal::Choose(choosing)) = self.modal.take() else { return };
        match choosing {
            Choosing::Form => {
                if let Some(f) = &mut self.form {
                    (f.category, f.transfer_to) = match choice {
                        Choice::Category(c) => (c, None),
                        Choice::Transfer(a) => (None, Some(a)),
                    };
                }
            }
            Choosing::Txns(ids) => {
                if let Choice::Category(c) = choice {
                    self.author("categorize", "categorize", &Categorize { ids, category_id: c });
                }
            }
            Choosing::Move { from, amount: typed } => {
                let Choice::Category(to) = choice else { return };
                match amount::parse(&typed) {
                    Some(cents) if cents > 0 => {
                        // An overspent category's money comes *from* the one
                        // chosen; any other's goes *to* it.
                        let short = from.and_then(|c| self.peer.line(c)).is_some_and(|l| l.available < 0);
                        let (from, to) = if short { (to, from) } else { (from, to) };
                        let m = MoveMoney {
                            month: self.peer.month.clone(),
                            from,
                            to,
                            amount: cents,
                        };
                        self.author("move the money", "move_money", &m);
                    }
                    _ => self.note = format!("{typed:?} is not an amount to move"),
                }
            }
        }
    }

    fn save_txn(&mut self) {
        let Some(f) = self.form.clone() else { return };
        let (Some(out), Some(inn)) = (amount::parse(&f.outflow), amount::parse(&f.inflow)) else {
            self.note = "an outflow or an inflow, as 12.50".into();
            return;
        };
        let amount = inn - out;
        let taken = match f.editing {
            Some(id) => {
                let e = EditTransaction {
                    id,
                    date: f.date,
                    payee: f.payee,
                    category_id: f.category,
                    memo: f.memo,
                    amount,
                };
                self.author("save the transaction", "edit_transaction", &e)
            }
            None => {
                let Some(account) = f.account else {
                    self.note = "which account?".into();
                    return;
                };
                let t = AddTransaction {
                    account_id: account,
                    date: f.date,
                    payee: f.payee,
                    category_id: f.category,
                    memo: f.memo,
                    amount,
                    cleared: f.cleared,
                    transfer_to: f.transfer_to,
                };
                self.author("add the transaction", "add_transaction", &t)
            }
        };
        if taken {
            self.form = None;
        }
    }

    /// The keys, and on a server what wakes the window: the socket ringing
    /// its bell, and the peer's next deadline (a redial) as one timer,
    /// keyed by it — a window at rest runs nothing.
    pub fn subscription(&self) -> Subscription<Message> {
        let keys = iced::keyboard::listen().filter_map(|e| match e {
            iced::keyboard::Event::KeyPressed {
                key: Key::Named(Named::Escape),
                ..
            } => Some(Message::Escape),
            _ => None,
        });
        let Some(s) = &self.server else { return keys };
        let mut subs = vec![keys, s.bell.subscription().map(|()| Message::Wake)];
        if let Some(at) = self.peer.client.deadline() {
            let delay = std::time::Duration::from_millis(at.saturating_sub(ark_peer::monotonic_ms()) + 2);
            subs.push(arkui::wake::at(at, delay).map(|()| Message::Wake));
        }
        Subscription::batch(subs)
    }
}

/// A desktop sign-in, which waits on a loopback listener for the browser,
/// on a thread of its own.
#[cfg(not(target_arch = "wasm32"))]
fn off_thread(work: impl FnOnce() -> Result<ark_auth::Login, String> + Send + 'static) -> Task<Message> {
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    Task::perform(
        async move { rx.await.unwrap_or_else(|_| Err("the sign-in thread went away".into())) },
        Message::SignedIn,
    )
}

#[cfg(target_arch = "wasm32")]
fn panic_to_console() {
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&info.to_string().into());
    }));
}

pub fn main() -> iced::Result {
    #[cfg(target_arch = "wasm32")]
    panic_to_console();
    arkui::theme::install(palette::MONEY);
    iced::application(App::boot, App::update, App::view)
        .subscription(App::subscription)
        .title(App::title)
        .window_size((1200.0, 760.0))
        .font(arkui::hero::FIRA_SANS_BOLD)
        .run()
}
