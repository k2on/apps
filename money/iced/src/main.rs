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
//! the domain's own sentence, said in the page's corner.
//!
//! Most of what a window is lives in `arkui`: the palette and the rule that
//! nothing writes a colour down, the table's cells and rows, the glyphs,
//! the layers a panel floats on. What is here is money's: its pages, its
//! messages, its forms.

mod amount;
mod banks;
mod browser;
mod client;
mod live;
mod look;
mod palette;
mod peer;
mod settings;
mod view;

#[cfg(test)]
mod tests;

use arkui::outline::{self, Grab, Shift};
use arkui::{glyphs, menu, vim};
use iced::keyboard::{key::Named, Key, Modifiers};
use iced::{Point, Size, Subscription, Task};
use money_domain::dates::{add_months, day_of_ms, month_of};
#[cfg(any(feature = "demo", test))]
use money_domain::seed;
use money_domain::*;

use peer::{Ledger, Peer};

/// Which pane has the keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Sidebar,
    Page,
}

/// A page as the address bar spells it: names, not ids (`arkui::route`).
#[derive(Debug, Clone, PartialEq)]
pub enum Place {
    Budget,
    Reflect,
    Accounts,
    Settings,
    Inbox,
    Account(String),
}

impl arkui::route::Route for Place {
    fn fragment(&self) -> String {
        match self {
            Place::Budget => arkui::route::join("budget", None),
            Place::Reflect => arkui::route::join("reflect", None),
            Place::Accounts => arkui::route::join("accounts", None),
            Place::Settings => arkui::route::join("settings", None),
            Place::Inbox => arkui::route::join("inbox", None),
            Place::Account(name) => arkui::route::join("account", Some(name)),
        }
    }

    fn parse(fragment: &str) -> Self {
        match arkui::route::split(fragment) {
            ("reflect", _) => Place::Reflect,
            ("accounts", _) => Place::Accounts,
            ("settings", _) => Place::Settings,
            ("inbox", _) => Place::Inbox,
            ("account", Some(name)) => Place::Account(name),
            _ => Place::Budget,
        }
    }
}

/// What fills the page beside the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Budget,
    Reflect,
    Settings,
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
    /// What the bank called the one corrected, where it says more than the
    /// payee does (`bank_said`).
    pub bank: String,
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

/// A category being chosen, as YNAB has it: a search box dropped under
/// the cell it is for, the categories under it by group (`arkui::command`).
pub struct Dropdown {
    pub choosing: Choosing,
    pub search: arkui::command::Command,
    /// Its corner: its top left under the cell, or, `above` it where
    /// there is no room under, its bottom left -- so a list that narrows as
    /// it is typed into shrinks toward the cell, not away from it.
    pub at: Point,
    pub above: bool,
}

/// A name being typed over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Naming {
    Category(Id<Category>),
    Group(Id<Group>),
    Account(Id<Account>),
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
    /// A bank to disconnect, asked twice.
    Disconnect(String),
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
    /// Settings' Banks section: connecting, and what each account feeds.
    Banks(banks::Msg),
    /// The account button at the top right: its menu, whose entries open
    /// Settings and sign out.
    AccountMenu,
    SignOut,
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
    /// A key no widget took, for the grammar (`arkui::vim`).
    Key(Key, Modifiers),
    /// Where the pointer is, for a press to start a drag from.
    Pointer(Point),
    Resized(Size),
    /// A row or a heading of the budget pressed, and the pointer moving
    /// and let go while it is held.
    Grab(Grab),
    Dragged(Point),
    Dropped,
    /// The clock while something slides.
    Frame,
    /// The wheel moved sideways: perhaps a swipe back or forward.
    Wheel(iced::mouse::ScrollDelta),
    /// A register row clicked: the cursor onto it, or, on the cursor, open.
    TxnClick(Id<Txn>),
    /// A row's category clicked: the dropdown under it.
    CategorizeRow(Id<Txn>),
    RegisterScrolled(f32),
    /// The budget scrolled; and a page opened scrolled to where it was left.
    PageScrolled(f32),
    ScrollPage(f32),
    /// The category dropdown: typed into, the pointer on a row, its list
    /// scrolled, its keys, closed.
    DropTyped(String),
    DropAt(usize),
    DropScrolled(f32),
    DropKey(arkui::command::Press),
    /// `<Enter>` in the box: the row with the keys chosen.
    DropChoose,
    CloseDrop,
    /// A row or a heading of the budget right-clicked: its menu.
    Menu(Grab),
    MenuAt(usize),
    MenuActivate,
    CloseMenu,
    Rename(Naming),
    RenameTyped(String),
    RenameDone,
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
    /// The grammar, with money's sequences bound (`cc`, `cg`, `rc`, `rg`).
    pub keys: vim::Keys,
    /// The other end of a visual selection from `selected`: where `v` was.
    pub anchor: Option<Id<Category>>,
    /// The last `/` search, for `n` and `N`.
    pub search: String,
    pub drag: Option<outline::Drag>,
    pub settle: Option<arkui::reorder::Settle>,
    pub pointer: Point,
    pub window: Size,
    /// A row's or a heading's menu.
    pub ctx: arkui::context::Context<Message, ()>,
    pub renaming: Option<(Naming, String)>,
    /// Which pane has the keys: the sidebar or the page beside it.
    pub pane: Pane,
    /// Where the window has been: the address bar, and back and forward by
    /// `^O`/`^I` and a swipe (`arkui::history`).
    pub history: arkui::history::History<Place>,
    /// The register's cursor, and the other end of its visual selection.
    pub txn_at: Option<Id<Txn>>,
    pub txn_anchor: Option<Id<Txn>>,
    /// How far down the register is scrolled, for where a row is.
    pub register_scroll: f32,
    /// Where the page just opened is to be scrolled, once it is drawn.
    pub scroll_due: Option<f32>,
    /// A category being chosen, in a searchable dropdown.
    pub dropdown: Option<Dropdown>,
    /// The last click on a row or a heading, and when: a second on the same
    /// one soon after is a double click, which renames it.
    pub last_click: Option<(Grab, iced::time::Instant)>,
    /// The last thing worth saying: a refusal, a sync.
    pub note: String,
    /// A Plaid page asked for here, and when: opened as soon as the
    /// server answers with its address.
    pub awaiting: Option<i64>,
    /// Every page this window opened, in order.
    pub opened: Vec<String>,
    /// The ask whose answer was put away.
    pub dismissed: Option<Id<BankAsk>>,
}

impl App {
    pub fn with_peer(client: client::Replica, today: &str) -> App {
        let mut app = App {
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
            keys: money_keys(),
            anchor: None,
            search: String::new(),
            drag: None,
            settle: None,
            pointer: Point::ORIGIN,
            window: Size::new(1200.0, 760.0),
            ctx: arkui::context::Context::new(),
            renaming: None,
            pane: Pane::Page,
            history: arkui::history::History::new(),
            txn_at: None,
            txn_anchor: None,
            register_scroll: 0.0,
            scroll_due: None,
            dropdown: None,
            last_click: None,
            awaiting: None,
            opened: Vec::new(),
            dismissed: None,
        };
        // A link the window was opened with, read before anything is drawn;
        // then where it is, as the history's first place.
        if let Some(page) = app.history.poll(true).and_then(|p| app.page_of(&p)) {
            let _ = app.step(Message::Go(page));
        }
        app.history.sync(&app.place());
        app
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
        self.follow_ask();
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

    /// One message, then the address bar: where it went (the back button,
    /// `^O`/`^I`, a swipe, a link) is gone to, and where the window now is
    /// is written to it.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.step(message);
        // An account is named in the bar, so a link to one waits for the
        // accounts to have arrived.
        let ready = !self.peer.accounts.rows.is_empty();
        if let Some(page) = self.history.poll(ready).and_then(|p| self.page_of(&p)) {
            if page != self.page {
                let _ = self.step(Message::Go(page));
            }
        }
        self.history.sync(&self.place());
        // A page just opened is drawn before this comes back, so the scroll
        // lands on its list rather than the one it replaced.
        match self.scroll_due.take() {
            Some(y) => {
                if let Page::Ledger(_) = self.page {
                    self.register_scroll = y;
                }
                Task::batch([task, Task::done(Message::ScrollPage(y))])
            }
            None => task,
        }
    }

    /// Where the window is, as the bar spells it.
    pub fn place(&self) -> Place {
        match self.page {
            Page::Budget => Place::Budget,
            Page::Reflect => Place::Reflect,
            Page::Ledger(Ledger::All) => Place::Accounts,
            Page::Settings => Place::Settings,
            Page::Ledger(Ledger::Inbox) => Place::Inbox,
            Page::Ledger(Ledger::Account(id)) => self.peer.account(id).map_or(Place::Accounts, |a| Place::Account(a.name.clone())),
        }
    }

    /// The page a place is, if it is one here: an account by its name.
    fn page_of(&self, place: &Place) -> Option<Page> {
        Some(match place {
            Place::Budget => Page::Budget,
            Place::Reflect => Page::Reflect,
            Place::Accounts => Page::Ledger(Ledger::All),
            Place::Settings => Page::Settings,
            Place::Inbox => Page::Ledger(Ledger::Inbox),
            Place::Account(name) => Page::Ledger(Ledger::Account(self.peer.accounts.rows.iter().find(|a| &a.name == name)?.id)),
        })
    }

    fn step(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Go(page) => {
                self.page = page;
                self.form = None;
                if let Page::Ledger(l) = page {
                    self.peer.set_ledger(l);
                }
                // A new visit opens at the top; back and forward where it was.
                self.scroll_due = Some(self.history.arrive(&self.place()));
            }
            Message::PageScrolled(y) => self.history.scrolled(&self.place(), y),
            Message::ScrollPage(y) => {
                let id = match self.page {
                    Page::Ledger(_) => {
                        self.register_scroll = y;
                        look::REGISTER
                    }
                    _ => look::BUDGET,
                };
                return iced::widget::operation::scroll_to(id, iced::widget::scrollable::AbsoluteOffset { x: 0.0, y });
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
                        bank: bank_said(t),
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
            Message::Categorize(ids) => return self.open_dropdown(Choosing::Txns(ids), (self.pointer, false)),
            Message::ChooseCategory => return self.open_dropdown(Choosing::Form, self.form_category_at()),
            // `<Enter>` in the box: the row with the keys.
            Message::DropChoose => {
                let groups = self.drop_groups();
                if let Some(m) = self.dropdown.as_ref().and_then(|d| d.search.chosen(&groups)) {
                    return self.update(m);
                }
            }
            Message::CategorizeRow(id) => {
                // The selection, when the row is in it; the row alone when not.
                let ids = match self.txn_selected().contains(&id) {
                    true => self.txn_selected(),
                    false => vec![id],
                };
                self.txn_at = Some(id);
                let at = self.category_at(id);
                return self.open_dropdown(Choosing::Txns(ids), at);
            }
            Message::TxnClick(id) => {
                self.pane = Pane::Page;
                self.txn_anchor = None;
                if self.txn_at == Some(id) {
                    return self.update(Message::EditTxn(id));
                }
                self.txn_at = Some(id);
            }
            Message::RegisterScrolled(y) => {
                self.register_scroll = y;
                self.history.scrolled(&self.place(), y);
            }
            Message::DropTyped(q) => {
                if let Some(d) = &mut self.dropdown {
                    return d.search.typed(q, look::DROP_LIST);
                }
            }
            Message::DropAt(at) => {
                if let Some(d) = &mut self.dropdown {
                    d.search.land(at);
                }
            }
            Message::DropScrolled(y) => {
                if let Some(d) = &mut self.dropdown {
                    d.search.scrolled(y);
                }
            }
            Message::DropKey(press) => {
                let groups = self.drop_groups();
                match (press, &mut self.dropdown) {
                    (arkui::command::Press::Close, _) => self.dropdown = None,
                    (arkui::command::Press::Up, Some(d)) => return d.search.step(false, &groups, look::DROP_LIST),
                    (arkui::command::Press::Down, Some(d)) => return d.search.step(true, &groups, look::DROP_LIST),
                    _ => {}
                }
            }
            Message::CloseDrop => self.dropdown = None,
            Message::Chose(choice) => self.chose(choice),
            Message::Sync => self.sync(),
            Message::Banks(m) => return self.banks(m),
            Message::AccountMenu => {
                if let Some(account) = self.account() {
                    let right = self.window.width - view::PAD;
                    self.ctx.open_menu(account.menu(right, view::PAD + arkui::account::HEIGHT, self.window));
                }
            }
            Message::SignOut => self.sign_out(),
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
                if self.ctx.is_open() {
                    self.ctx.close_menu();
                } else if self.dropdown.is_some() {
                    self.dropdown = None;
                } else if self.modal.is_some() {
                    self.modal = None;
                } else if self.renaming.is_some() {
                    self.renaming = None;
                } else if self.assigning.is_some() {
                    self.assigning = None;
                } else if self.form.is_some() {
                    self.form = None;
                } else if self.new_group.is_some() || self.new_category.is_some() {
                    (self.new_group, self.new_category) = (None, None);
                } else if self.txn_anchor.is_some() {
                    self.txn_anchor = None;
                } else if self.anchor.is_some() {
                    self.anchor = None;
                } else {
                    self.selected = None;
                }
            }
            Message::Key(key, mods) => return self.key(key, mods),
            Message::Pointer(at) => self.pointer = at,
            Message::Resized(size) => self.window = size,
            Message::Grab(grab) => {
                let now = iced::time::Instant::now();
                self.settle = None;
                self.drag = Some(outline::Drag::press(grab, &self.sizes(), view::LINE, self.pointer.y, now));
            }
            Message::Dragged(at) => {
                self.pointer = at;
                if let Some(d) = self.drag.as_mut() {
                    d.drag(at.y, iced::time::Instant::now());
                }
            }
            Message::Dropped => return self.dropped(),
            Message::Wheel(delta) => self.history.wheel(delta, iced::time::Instant::now()),
            Message::Frame => {
                let now = iced::time::Instant::now();
                self.history.tick(now);
                if self.settle.is_some_and(|s| !s.moving(now)) {
                    self.settle = None;
                }
            }
            Message::Menu(grab) => self.menu(grab),
            Message::MenuAt(at) => self.ctx.land(at),
            Message::MenuActivate => {
                let chosen = self.ctx.chosen();
                self.ctx.close_menu();
                if let Some(m) = chosen {
                    return self.update(m);
                }
            }
            Message::CloseMenu => self.ctx.close_menu(),
            Message::Rename(named) => {
                let name = match named {
                    Naming::Category(c) => self.peer.line(c).map(|l| l.name.clone()),
                    Naming::Group(g) => self.peer.groups.rows.iter().find(|r| r.id == g).map(|r| r.name.clone()),
                    Naming::Account(a) => self.peer.account(a).map(|a| a.name.clone()),
                };
                if let Some(name) = name {
                    self.renaming = Some((named, name));
                    return iced::widget::operation::focus(look::RENAME);
                }
            }
            Message::RenameTyped(t) => {
                if let Some((_, typed)) = &mut self.renaming {
                    *typed = t;
                }
            }
            Message::RenameDone => match self.renaming.take() {
                Some((Naming::Category(id), name)) => {
                    self.author("rename the category", "rename_category", &RenameCategory { id, name });
                }
                Some((Naming::Group(id), name)) => {
                    self.author("rename the group", "rename_group", &RenameGroup { id, name });
                }
                Some((Naming::Account(id), name)) => {
                    self.author("rename the account", "rename_account", &RenameAccount { id, name });
                }
                None => {}
            },
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
    /// change does, and the note in the page's corner says what came of
    /// it. In the demo the demo's bank stands in for Plaid, and the import
    /// is authored here, this peer holding the importer's role.
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

    // * The budget's keys, drags and menus

    /// The groups as the budget draws them, each with every line it sums
    /// and the rows the cursor can be on: none when folded.
    pub fn shown(&self) -> Vec<Shown<'_>> {
        self.peer
            .groups
            .rows
            .iter()
            .filter(|g| self.show_hidden || !g.hidden)
            .map(|g| {
                let lines: Vec<&BudgetLine> = self
                    .peer
                    .lines
                    .rows
                    .iter()
                    .filter(|l| l.group_id == g.id && (self.show_hidden || !l.hidden))
                    .collect();
                let rows = match self.collapsed.contains(&g.id) {
                    true => Vec::new(),
                    false => lines.clone(),
                };
                Shown { group: g, lines, rows }
            })
            .collect()
    }

    /// Each group's row count, for `arkui::outline`.
    pub fn sizes(&self) -> Vec<usize> {
        self.shown().iter().map(|s| s.rows.len()).collect()
    }

    /// The rows' categories, in order.
    fn row_ids(&self) -> Vec<Id<Category>> {
        self.shown().iter().flat_map(|s| s.rows.iter().map(|l| l.category_id)).collect()
    }

    fn group_ids(&self) -> Vec<Id<Group>> {
        self.shown().iter().map(|s| s.group.id).collect()
    }

    /// The cursor's row.
    fn at(&self) -> Option<usize> {
        let c = self.selected?;
        self.row_ids().iter().position(|&r| r == c)
    }

    /// The rows under the cursor, first to last: one, or from the visual
    /// selection's anchor to the cursor.
    pub fn lit(&self) -> Option<(usize, usize)> {
        let at = self.at()?;
        let from = self.anchor.and_then(|a| self.row_ids().iter().position(|&r| r == a)).unwrap_or(at);
        Some((from.min(at), from.max(at)))
    }

    /// Whether the keys are the grammar's: nothing over the window, nothing
    /// typed into.
    pub fn keys_free(&self) -> bool {
        self.modal.is_none()
            && self.dropdown.is_none()
            && self.form.is_none()
            && self.renaming.is_none()
            && self.assigning.is_none()
            && !self.ctx.is_open()
    }

    /// Whether the budget has the keyboard.
    pub fn budget_keys(&self) -> bool {
        self.page == Page::Budget && self.pane == Pane::Page && self.keys_free()
    }

    fn key(&mut self, key: Key, mods: Modifiers) -> Task<Message> {
        if key == Key::Named(Named::Escape) {
            self.keys.press(&key, mods);
            return self.update(Message::Escape);
        }
        // The budget binds its sequences; the register none, so its `c`
        // categorizes at once rather than waiting for a second key.
        self.keys.set_bindings(match (self.pane, self.page) {
            (Pane::Page, Page::Budget) => &MONEY_KEYS,
            _ => &[],
        });
        let Some(action) = self.keys.press(&key, mods) else {
            return Task::none();
        };
        // A menu that is up has the keys.
        if self.ctx.is_open() {
            return match action {
                vim::Action::Move(m) => match self.ctx.travel(m) {
                    Some(msg) => self.update(msg),
                    None => Task::none(),
                },
                vim::Action::Activate => self.update(Message::MenuActivate),
                _ => Task::none(),
            };
        }
        // Back and forward, the way the browser's buttons go: the bar moves,
        // and the next update arrives where it went.
        if let vim::Action::Jump(by) = action {
            self.history.go(by);
            return Task::none();
        }
        if !self.keys_free() {
            return Task::none();
        }
        if action == vim::Action::Cycle {
            self.pane = match self.pane {
                Pane::Sidebar => Pane::Page,
                Pane::Page => Pane::Sidebar,
            };
            return Task::none();
        }
        match (self.pane, self.page) {
            (Pane::Sidebar, _) => self.sidebar_act(action),
            (Pane::Page, Page::Budget) => self.act(action),
            (Pane::Page, Page::Ledger(_)) => self.register_act(action),
            (Pane::Page, Page::Reflect | Page::Settings) => {
                if let vim::Action::Move(vim::Motion::Left(_)) = action {
                    self.pane = Pane::Sidebar;
                }
                Task::none()
            }
        }
    }

    /// A command with the sidebar's keys: `j` and `k` walk the places, the
    /// page following, and `l` or `<Enter>` give the keys to the page.
    fn sidebar_act(&mut self, action: vim::Action) -> Task<Message> {
        let places = self.places();
        match action {
            vim::Action::Move(vim::Motion::Right(_)) | vim::Action::Activate => self.pane = Pane::Page,
            vim::Action::Move(m) => {
                if let Some(i) = vim::Grid::column(places.len()).step(self.sidebar_at(), m) {
                    // A cursor walking is a scrub: one history entry, not one
                    // a place.
                    self.history.replace_next();
                    return self.update(Message::Go(places[i].3));
                }
            }
            _ => {}
        }
        Task::none()
    }

    /// One command of the grammar, on the budget.
    pub fn act(&mut self, action: vim::Action) -> Task<Message> {
        let ids = self.row_ids();
        let sizes = self.sizes();
        let n = ids.len();
        match action {
            // The budget is one column: `h` is the sidebar, beside it.
            vim::Action::Move(vim::Motion::Left(_)) => self.pane = Pane::Sidebar,
            vim::Action::Move(m) => {
                let grid = vim::Grid::column(n);
                let to = match self.at() {
                    Some(i) => grid.step(i, m),
                    // No cursor yet: `j` starts at the top, `k` at the bottom.
                    None => match m {
                        vim::Motion::Up(_) | vim::Motion::Last => n.checked_sub(1),
                        vim::Motion::To(_) => grid.step(0, m),
                        _ => (n > 0).then_some(0),
                    },
                };
                if let Some(i) = to {
                    self.select(Some(ids[i]));
                    return self.reveal(i);
                }
            }
            vim::Action::Activate => {
                if let Some(c) = self.selected {
                    return self.update(Message::EditAssigned(c));
                }
            }
            vim::Action::Search(q) => {
                self.search = q;
                return self.find(1);
            }
            vim::Action::Match(delta) => return self.find(delta),
            vim::Action::Visual(on) => {
                if on && self.selected.is_none() {
                    self.selected = ids.first().copied();
                }
                self.anchor = if on { self.selected } else { None };
            }
            // `vig`: every row of the cursor's group.
            vim::Action::Inner('g') => {
                if let Some(g) = self.at().and_then(|i| outline::group_of(&sizes, i)) {
                    let rows = outline::rows_of(&sizes, g);
                    self.anchor = Some(ids[rows.start]);
                    self.select(Some(ids[rows.end - 1]));
                }
            }
            vim::Action::Bound("cc") => {
                let group = self.cursor_group().or_else(|| self.group_ids().first().copied());
                if let Some(g) = group {
                    return self.update(Message::NewCategory(g));
                }
            }
            vim::Action::Bound("cg") => return self.update(Message::NewGroup),
            vim::Action::Bound("rc") => {
                if let Some(c) = self.selected {
                    return self.update(Message::Rename(Naming::Category(c)));
                }
            }
            vim::Action::Bound("rg") => {
                if let Some(g) = self.cursor_group() {
                    return self.update(Message::Rename(Naming::Group(g)));
                }
            }
            vim::Action::Key('J') => return self.shift(true),
            vim::Action::Key('K') => return self.shift(false),
            vim::Action::Key(c @ ('{' | '}')) => {
                if let Some(g) = self.at().and_then(|i| outline::group_of(&sizes, i)) {
                    let shift = outline::groups(sizes.len(), g, g, c == '}');
                    self.apply(shift, g, g);
                    return self.reveal_cursor();
                }
            }
            _ => {}
        }
        Task::none()
    }

    /// The group the cursor's row is in.
    fn cursor_group(&self) -> Option<Id<Group>> {
        let g = outline::group_of(&self.sizes(), self.at()?)?;
        self.group_ids().get(g).copied()
    }

    /// `/` and `n`/`N`: the next category whose name has the search in it.
    fn find(&mut self, delta: isize) -> Task<Message> {
        let ids = self.row_ids();
        let labels: Vec<String> = self.shown().iter().flat_map(|s| s.rows.iter().map(|l| l.name.clone())).collect();
        match vim::find(&labels, &self.search, self.at().unwrap_or(0), delta) {
            Some(i) => {
                self.select(Some(ids[i]));
                self.reveal(i)
            }
            None => {
                self.note = format!("no category has {:?} in it", self.search);
                Task::none()
            }
        }
    }

    /// `J` and `K`: the selection one place down or up — every row of whole
    /// groups moves the groups.
    fn shift(&mut self, down: bool) -> Task<Message> {
        let Some((first, last)) = self.lit() else {
            return Task::none();
        };
        let shift = outline::shift(&self.sizes(), first, last, down);
        self.apply(shift, first, last);
        self.reveal_cursor()
    }

    /// A shift's moves, authored: rows `first..=last` (or the groups they
    /// are) each in front of the same neighbour, so they keep their order.
    fn apply(&mut self, shift: Shift, first: usize, last: usize) {
        let ids = self.row_ids();
        let groups = self.group_ids();
        match shift {
            Shift::Refused => {}
            Shift::Rows { group, before } => {
                for &id in &ids[first..=last] {
                    let m = MoveCategory {
                        id,
                        group_id: groups[group],
                        before: before.map(|b| ids[b]),
                    };
                    self.author("move the category", "move_category", &m);
                }
            }
            Shift::Groups { first, last, before } => {
                for &id in &groups[first..=last] {
                    let m = MoveGroup {
                        id,
                        before: before.map(|b| groups[b]),
                    };
                    self.author("move the group", "move_group", &m);
                }
            }
        }
    }

    /// A row or a heading let go.
    fn dropped(&mut self) -> Task<Message> {
        let Some(drag) = self.drag.take() else {
            return Task::none();
        };
        let ids = self.row_ids();
        let groups = self.group_ids();
        let now = iced::time::Instant::now();
        match drag.drop(&self.sizes(), now) {
            outline::Dropped::Click(grab) => {
                let double = self.last_click.is_some_and(|(g, at)| g == grab && now.duration_since(at) < DOUBLE_CLICK);
                self.last_click = (!double).then_some((grab, now));
                if let Grab::Row(n) = grab {
                    self.anchor = None;
                    self.select(ids.get(n).copied());
                }
                if double {
                    let named = match grab {
                        Grab::Row(n) => ids.get(n).map(|&c| Naming::Category(c)),
                        Grab::Group(g) => groups.get(g).map(|&g| Naming::Group(g)),
                    };
                    if let Some(named) = named {
                        return self.update(Message::Rename(named));
                    }
                }
            }
            outline::Dropped::Stayed => {}
            outline::Dropped::Row { row, group, before, settle } => {
                let m = MoveCategory {
                    id: ids[row],
                    group_id: groups[group],
                    before: before.map(|b| ids[b]),
                };
                self.anchor = None;
                self.select(Some(ids[row]));
                if self.author("move the category", "move_category", &m) {
                    self.settle = Some(settle);
                }
            }
            outline::Dropped::Group { group, before } => {
                let m = MoveGroup {
                    id: groups[group],
                    before: before.map(|b| groups[b]),
                };
                self.author("move the group", "move_group", &m);
            }
        }
        Task::none()
    }

    /// A row's or a heading's menu, at the pointer.
    fn menu(&mut self, grab: Grab) {
        let shown = self.shown();
        let (title, entries) = match grab {
            Grab::Row(n) => {
                let Some(l) = shown.iter().flat_map(|s| s.rows.iter()).nth(n).copied() else {
                    return;
                };
                let (c, g) = (l.category_id, l.group_id);
                let hide = match l.hidden {
                    true => menu::Entry::run(glyphs::UNHIDE, "Show", Message::HideCategory(c, false)),
                    false => menu::Entry::run(glyphs::HIDE, "Hide", Message::HideCategory(c, true)),
                };
                let entries = vec![
                    menu::Entry::run(glyphs::RENAME, "Rename", Message::Rename(Naming::Category(c))),
                    hide,
                    menu::Entry::run(glyphs::PLUS, "New Category", Message::NewCategory(g)),
                ];
                (l.name.clone(), entries)
            }
            Grab::Group(g) => {
                let Some(s) = shown.get(g) else {
                    return;
                };
                let id = s.group.id;
                let entries = vec![
                    menu::Entry::run(glyphs::RENAME, "Rename", Message::Rename(Naming::Group(id))),
                    menu::Entry::run(glyphs::PLUS, "New Category", Message::NewCategory(id)),
                    menu::Entry::run(glyphs::PLUS, "New Group", Message::NewGroup),
                ];
                (s.group.name.clone(), entries)
            }
        };
        if let Grab::Row(n) = grab {
            let ids = self.row_ids();
            self.anchor = None;
            self.select(ids.get(n).copied());
        }
        self.ctx
            .open_menu(menu::Menu::open(title, entries, self.pointer, self.window, menu::Anchor::Pointer));
    }

    // * The register's keys, and the category dropdown

    /// The register's cursor's row.
    fn txn_cursor(&self) -> Option<usize> {
        let at = self.txn_at?;
        self.peer.register.rows.iter().position(|t| t.id == at)
    }

    /// The register's rows under the cursor, first to last: one, or from the
    /// visual selection's anchor to the cursor.
    pub fn txn_lit(&self) -> Option<(usize, usize)> {
        let at = self.txn_cursor()?;
        let rows = &self.peer.register.rows;
        let from = self.txn_anchor.and_then(|a| rows.iter().position(|t| t.id == a)).unwrap_or(at);
        Some((from.min(at), from.max(at)))
    }

    /// The transactions under the cursor.
    pub fn txn_selected(&self) -> Vec<Id<Txn>> {
        match self.txn_lit() {
            Some((a, b)) => self.peer.register.rows[a..=b].iter().map(|t| t.id).collect(),
            None => Vec::new(),
        }
    }

    /// A command with the register's keys: `j`/`k` and the rest walk it, `v`
    /// selects (`V` too), `c` chooses the selection's category from the dropdown, `a`
    /// approves it, `<Space>` clears it (or un-clears), `<Enter>` edits the
    /// cursor's, `i` adds one, `/` finds a payee, and `h` is the sidebar.
    pub fn register_act(&mut self, action: vim::Action) -> Task<Message> {
        let rows: Vec<(Id<Txn>, bool, Cleared, String)> = self
            .peer
            .register
            .rows
            .iter()
            .map(|t| (t.id, t.approved, t.cleared, t.payee.clone()))
            .collect();
        let n = rows.len();
        match action {
            vim::Action::Move(vim::Motion::Left(_)) => self.pane = Pane::Sidebar,
            vim::Action::Move(m) => {
                let grid = vim::Grid::column(n);
                let to = match self.txn_cursor() {
                    Some(i) => grid.step(i, m),
                    None => match m {
                        vim::Motion::Up(_) | vim::Motion::Last => n.checked_sub(1),
                        vim::Motion::To(_) => grid.step(0, m),
                        _ => (n > 0).then_some(0),
                    },
                };
                if let Some(i) = to {
                    self.txn_at = Some(rows[i].0);
                    return self.reveal_txn(i);
                }
            }
            vim::Action::Visual(on) => {
                if on && self.txn_at.is_none() {
                    self.txn_at = rows.first().map(|r| r.0);
                }
                self.txn_anchor = if on { self.txn_at } else { None };
            }
            vim::Action::Activate => {
                if let Some(id) = self.txn_at {
                    return self.update(Message::EditTxn(id));
                }
            }
            vim::Action::Key('c') => {
                if let Some(id) = self.txn_at {
                    return self.update(Message::CategorizeRow(id));
                }
            }
            vim::Action::Key('a') => {
                let ids: Vec<Id<Txn>> = self
                    .txn_selected()
                    .into_iter()
                    .filter(|id| rows.iter().any(|r| r.0 == *id && !r.1))
                    .collect();
                self.txn_anchor = None;
                if !ids.is_empty() {
                    return self.update(Message::Approve(ids));
                }
            }
            vim::Action::Toggle => {
                for id in self.txn_selected() {
                    if let Some(r) = rows.iter().find(|r| r.0 == id) {
                        if r.2 != Cleared::Reconciled {
                            let _ = self.update(Message::ToggleCleared(id, r.2 == Cleared::Uncleared));
                        }
                    }
                }
            }
            vim::Action::Key('i') => return self.update(Message::AddTxn),
            vim::Action::Search(q) => {
                self.search = q;
                return self.find_txn(1);
            }
            vim::Action::Match(delta) => return self.find_txn(delta),
            _ => {}
        }
        Task::none()
    }

    /// `/` and `n`/`N` in the register: the next payee with the search in it.
    fn find_txn(&mut self, delta: isize) -> Task<Message> {
        let payees: Vec<String> = self.peer.register.rows.iter().map(|t| t.payee.clone()).collect();
        match vim::find(&payees, &self.search, self.txn_cursor().unwrap_or(0), delta) {
            Some(i) => {
                self.txn_at = Some(self.peer.register.rows[i].id);
                self.reveal_txn(i)
            }
            None => {
                self.note = format!("no payee has {:?} in it", self.search);
                Task::none()
            }
        }
    }

    /// How tall the register's list is on screen.
    fn register_view(&self) -> f32 {
        (self.window.height - view::PAD * 2.0 - view::LEDGER_TOP - view::LEDGER_GAP - view::LEDGER_HEAD - 1.0).max(view::TXN_ROW)
    }

    /// Scroll the register just far enough that row `i` is in sight.
    fn reveal_txn(&mut self, i: usize) -> Task<Message> {
        // A new transaction's form stands above the rows.
        let above = if self.form.as_ref().is_some_and(|f| f.editing.is_none()) {
            view::TXN_ROW
        } else {
            0.0
        };
        let top = above + i as f32 * view::TXN_ROW;
        let seen = self.register_view();
        let y = if top < self.register_scroll {
            top
        } else if top + view::TXN_ROW > self.register_scroll + seen {
            top + view::TXN_ROW - seen
        } else {
            return Task::none();
        };
        self.register_scroll = y;
        self.history.scrolled(&self.place(), y);
        iced::widget::operation::scroll_to(look::REGISTER, iced::widget::scrollable::AbsoluteOffset { x: 0.0, y })
    }

    /// Where the category of transaction `id`'s row is on screen: the
    /// dropdown's corner, under it. The register's columns are the ones
    /// `view_ledger` lays out, worked out for this window's width.
    fn category_at(&self, id: Id<Txn>) -> (Point, bool) {
        let i = self.peer.register.rows.iter().position(|t| t.id == id).unwrap_or(0);
        let above = if self.form.as_ref().is_some_and(|f| f.editing.is_none()) {
            view::TXN_ROW
        } else {
            0.0
        };
        self.under(above + (i + 1) as f32 * view::TXN_ROW)
    }

    /// Where the form's category box is on screen: its list hangs under it.
    fn form_category_at(&self) -> (Point, bool) {
        let i = match self.form.as_ref().and_then(|f| f.editing) {
            Some(id) => self.peer.register.rows.iter().position(|t| t.id == id).unwrap_or(0),
            None => 0,
        };
        self.under(i as f32 * view::TXN_ROW + FORM_LINE)
    }

    /// The register's category column at `y` down its list (scrolled), as
    /// a dropdown's corner: above, if there is no room under it.
    fn under(&self, y: f32) -> (Point, bool) {
        let all = !matches!(self.page, Page::Ledger(Ledger::Account(_)));
        let left = view::SIDEBAR + 1.0 + view::PAD + arkui::table::ROW_PADDING[1];
        let fixed = if all { 130.0 } else { 0.0 } + 96.0 + 100.0 + 100.0 + 90.0;
        let width = self.window.width - view::SIDEBAR - 1.0 - view::PAD * 2.0 - arkui::table::ROW_PADDING[1] * 2.0;
        let payee = (width - fixed).max(0.0) * 3.0 / 8.0;
        let x = left + if all { 130.0 } else { 0.0 } + 96.0 + payee;
        let top = view::PAD + view::LEDGER_TOP + view::LEDGER_GAP + view::LEDGER_HEAD + 1.0;
        let y = top + y - self.register_scroll;
        let above = y + arkui::command::LIST_HEIGHT > self.window.height;
        // Above: its bottom on the cell's top.
        let y = if above { y - view::TXN_ROW } else { y };
        (Point::new(x.min(self.window.width - DROP_WIDTH).max(0.0), y), above)
    }

    /// The dropdown, for `choosing`, its corner at `at`, its box with the keys.
    fn open_dropdown(&mut self, choosing: Choosing, (at, above): (Point, bool)) -> Task<Message> {
        self.dropdown = Some(Dropdown {
            choosing,
            search: arkui::command::Command::new(iced::time::Instant::now()),
            at,
            above,
        });
        iced::widget::operation::focus(look::DROP_INPUT)
    }

    /// What the dropdown offers for what is typed: transfers (for the
    /// form's transaction, new or not — an existing one made a transfer is
    /// paired with its match in the other account), Ready to Assign, each
    /// group's categories with what they have, and no category at all —
    /// every one whose name has the typed in it. A card's payment category
    /// is money moved to, not a transaction's category: paying a card is a
    /// transfer to it, so the cards' group is offered only to move money.
    pub fn drop_groups(&self) -> Vec<arkui::command::Group<Message>> {
        let Some(d) = &self.dropdown else {
            return Vec::new();
        };
        let query = d.search.query.to_lowercase();
        let has = |name: &str| query.is_empty() || name.to_lowercase().contains(&query);
        let item = |label: String, detail: Option<String>, choice: Choice| arkui::command::Item {
            glyph: match choice {
                Choice::Transfer(_) => glyphs::SEND,
                Choice::Category(Some(c)) if c == money_domain::budget::ready_to_assign() => glyphs::GROWTH,
                Choice::Category(Some(_)) => view::category_glyph(&label),
                Choice::Category(None) => glyphs::CLOSE,
            },
            label,
            detail,
            choose: Message::Chose(choice),
        };
        let mut groups = Vec::new();
        if let (Choosing::Form, Some(f)) = (&d.choosing, &self.form) {
            let transfers = self
                .peer
                .accounts
                .rows
                .iter()
                .filter(|a| !a.closed && Some(a.id) != f.account && has(&a.name))
                .map(|a| item(format!("Transfer : {}", a.name), None, Choice::Transfer(a.id)))
                .collect();
            groups.push(arkui::command::Group {
                heading: "Transfer to".into(),
                items: transfers,
            });
        }
        if has("Ready to Assign") {
            let rta = self.peer.summary().ready_to_assign;
            groups.push(arkui::command::Group {
                heading: "Inflow".into(),
                items: vec![item(
                    "Ready to Assign".into(),
                    Some(amount::money(rta)),
                    Choice::Category(Some(money_domain::budget::ready_to_assign())),
                )],
            });
        }
        let moving = matches!(d.choosing, Choosing::Move { .. });
        let cards = money_domain::cards::card_group();
        for g in self.peer.groups.rows.iter().filter(|g| !g.hidden && (moving || g.id != cards)) {
            let items = self
                .peer
                .lines
                .rows
                .iter()
                .filter(|l| l.group_id == g.id && !l.hidden && has(&l.name))
                .map(|l| item(l.name.clone(), Some(amount::money(l.available)), Choice::Category(Some(l.category_id))))
                .collect();
            groups.push(arkui::command::Group {
                heading: g.name.clone(),
                items,
            });
        }
        if matches!(d.choosing, Choosing::Txns(_)) && has("No category") {
            groups.push(arkui::command::Group {
                heading: "None".into(),
                items: vec![item("No category".into(), None, Choice::Category(None))],
            });
        }
        groups.retain(|g| !g.items.is_empty());
        groups
    }

    /// Keep row `n` in view.
    fn reveal(&self, n: usize) -> Task<Message> {
        arkui::scroll::reveal_to(look::BUDGET, outline::progress(&self.sizes(), n))
    }

    fn reveal_cursor(&self) -> Task<Message> {
        match self.at() {
            Some(n) => self.reveal(n),
            None => Task::none(),
        }
    }

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
        // The dropdown, if one is up; the panel moving money, if not.
        let choosing = match (self.dropdown.take(), self.modal.take()) {
            (Some(d), modal) => {
                self.modal = modal;
                d.choosing
            }
            (None, Some(Modal::Choose(c))) => c,
            (None, other) => {
                self.modal = other;
                return;
            }
        };
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
                    self.txn_anchor = None;
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
                    transfer_to: f.transfer_to,
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

    /// The keys no widget took, the window's size, and a clock only while
    /// a row slides; on a server, what wakes the window: the socket ringing
    /// its bell, and the peer's next deadline (a redial) as one timer,
    /// keyed by it — a window at rest runs nothing.
    pub fn subscription(&self) -> Subscription<Message> {
        let now = iced::time::Instant::now();
        let mut subs = vec![
            vim::presses().map(|(key, mods)| Message::Key(key, mods)),
            iced::window::resize_events().map(|(_, size)| Message::Resized(size)),
            // The wheel, sideways: perhaps a swipe back or forward.
            arkui::history::wheel().map(Message::Wheel),
            // A paste the browser handed over: a frame, and the box takes it.
            arkui::clipboard::pastes().map(|()| Message::Swallow),
        ];
        if self.dropdown.is_some() {
            subs.push(arkui::command::keys(true).map(Message::DropKey));
        }
        if self.history.active() || self.drag.as_ref().is_some_and(|d| d.moving(now)) || self.settle.is_some_and(|s| s.moving(now)) {
            subs.push(iced::time::every(std::time::Duration::from_millis(16)).map(|_| Message::Frame));
        }
        if let Some(s) = &self.server {
            subs.push(s.bell.subscription().map(|()| Message::Wake));
            if let Some(at) = self.peer.client.deadline() {
                let delay = std::time::Duration::from_millis(at.saturating_sub(ark_peer::monotonic_ms()) + 2);
                subs.push(arkui::wake::at(at, delay).map(|()| Message::Wake));
            }
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

/// A group as the budget draws it: every line it sums, and the rows the
/// cursor can be on — none while it is folded.
pub struct Shown<'a> {
    pub group: &'a GroupLine,
    pub lines: Vec<&'a BudgetLine>,
    pub rows: Vec<&'a BudgetLine>,
}

/// The budget's sequences: `cc` a category and `cg` a group made, `rc` and
/// `rg` one renamed.
const MONEY_KEYS: [&str; 4] = ["cc", "cg", "rc", "rg"];

/// The grammar, with the budget's sequences bound.
fn money_keys() -> vim::Keys {
    let mut keys = vim::Keys::new();
    keys.set_bindings(&MONEY_KEYS);
    keys
}

/// How soon a second click is a double click: AppKit's default.
const DOUBLE_CLICK: std::time::Duration = std::time::Duration::from_millis(500);

/// How wide the category dropdown is.
pub const DROP_WIDTH: f32 = 340.0;

/// What the bank called an imported transaction, where it says something the
/// payee does not: `CHIPOTLE 1234 SAN FRANCISCO CA` beside `Chipotle`. Blank
/// for one typed in, or one the bank and the payee name alike.
pub fn bank_said(t: &TxnLine) -> String {
    let said = t.imported_payee.trim();
    match t.imported && !said.is_empty() && !said.eq_ignore_ascii_case(t.payee.trim()) {
        true => said.to_string(),
        false => String::new(),
    }
}

/// How far down the register's form its category box ends.
const FORM_LINE: f32 = 40.0;
