//! money, in iced — on the desktop and in a browser: a household's budget.
//!
//! Two builds of one window. Without features it is the household's
//! client: it signs in to a money server and dials it (`live.rs`), and
//! "Sync" asks the server's importer to ask the banks now. With `demo` it
//! is the demo: no server and no sign-in, a seeded household (`money_seed`)
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
//! the domain's own sentence, said for a moment over the page.
//!
//! Most of what a window is lives in `arkui`: the palette and the rule that
//! nothing writes a colour down, the table's cells and rows, the glyphs,
//! the layers a panel floats on. What is here is money's: its pages, its
//! messages, its forms.

mod amount;
mod banks;
mod browser;
mod client;
mod filter;
mod formula;
mod live;
mod look;
mod overview;
mod palette;
mod peer;
mod receipt;
mod sections;
mod settings;
mod stores;
mod view;

#[cfg(test)]
mod tests;

use ark_auth::onboarding::{Effect, Event, Onboarding};
use arkui::outline::{self, Grab, Shift};
use arkui::{glyphs, menu, vim};
use iced::keyboard::{Key, Modifiers};
use iced::{Point, Size, Subscription, Task};
use money_domain::dates::{add_months, day_of_ms, month_of};
use money_domain::*;
#[cfg(any(feature = "demo", test))]
use money_seed as seed;

use peer::{Ledger, Peer};

/// Which pane has the keys: ArkUI's panes (`arkui::shell`), every app's.
pub use arkui::shell::Pane;

use money_app::shell::ShellMsg;
/// Where the window is: money's routes (`money_domain::routes`), which the
/// address bar spells by their paths (`arkui::router`).
pub use money_domain::routes::Route;

/// What fills the page beside the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Budget,
    /// Every account and what the household is worth: the page money_app
    /// lays out once for every platform.
    Accounts,
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
    /// Split: its amount in lines, each for a category.
    pub split: Option<Vec<SplitRow>>,
}

/// One line of a split as it is typed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SplitRow {
    pub category: Option<Id<Category>>,
    /// Spent on it, as typed: an outflow, unsigned.
    pub amount: String,
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
    /// The register's filter's categories: one chosen is ticked or not.
    Filter,
    /// Receipts' items: what they are filed under.
    Items(Vec<ItemRef>),
    /// A line of the form's split.
    SplitLine(usize),
    /// What paid a receipt whole, or one of its charges.
    Payer(PaidFor),
}

/// What a chooser offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Category(Option<Id<Category>>),
    Transfer(Id<Account>),
    /// For a filter: any category at all.
    AnyCategory,
    /// What paid a receipt or a charge; none for nothing found.
    Paid(Option<Id<Txn>>),
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
    Filter(Id<SavedFilter>),
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
    /// A receipt's photo.
    Photo(ark::file::FileRef),
    /// A bank to disconnect, asked twice.
    Disconnect(String),
    /// The register's filter, being named to keep.
    SaveFilter {
        name: String,
    },
}

#[derive(Debug, Clone)]
pub enum Message {
    Go(Page),
    /// The Accounts page's own messages (`money_app::accounts`).
    Accounts(money_app::accounts::AccountsMsg),
    /// A message of the shell's: a section, the inspector, the search, a command.
    Shell(ShellMsg),
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
    /// The inspector's formula: its terms, saved from the month on
    /// screen, or removed from it (`formula.rs`).
    Formula(formula::Msg),
    /// The register's filter: its conditions changed, kept, opened
    /// (`filter.rs`).
    Filter(filter::Msg),
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
    /// Who is signed in, at the sidebar's foot: its menu, whose entries
    /// open Settings and sign out.
    AccountMenu,
    /// The toolbar's Filter: the register's bar of conditions shown or put away.
    ShowFilter(bool),
    /// The toolbar's Auto-Assign: its menu, under the wand.
    AutoMenu,
    SignOut,
    /// "Add a Device": a one-time code for another device, asked of the
    /// server as this login; and what it answered.
    AddDevice,
    DeviceCode(Result<String, String>),
    /// The join link shown, copied.
    CopyLink(String),
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
    /// Reflect or Settings was scrolled to this far down.
    TopScrolled(f32),
    /// The small header pressed: Reflect or Settings back to its top.
    PageTop,
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
    /// A sign-in's code came back from the browser, or why not.
    Coded(Result<String, String>),
    /// A sign-in came back.
    SignedIn(Result<ark_auth::Login, String>),
    /// The replica [`App::boot`] read on a thread.
    Opened(arkui::later::Slot<Result<client::Replica, String>>),
    /// The account's picture, fetched.
    Picture(arkui::images::Loaded),
    /// The which-key panel's delay went by for the prefix it was armed for.
    WhichDue(u64),
    /// The note the toast was saying has had its time.
    ToastDue(u64),
    /// Items' category: the dropdown, under the pointer.
    FileItems(Vec<ItemRef>),
    /// The form's transaction split: into two lines, or (none) back into one.
    Split(bool),
    SplitAmount(usize, String),
    SplitCategory(usize),
    SplitAdd,
    SplitRemove(usize),
    /// A kind of a category, being named.
    NewKind(Id<Category>),
    NewKindTyped(String),
    NewKindDone,
    /// Settings' Amazon and Costco sections.
    Stores(stores::Msg),
    /// What paid a receipt or a charge: the dropdown of what could have.
    ChoosePayer(PaidFor),
    /// The open transaction's receipt.
    Receipt(receipt::Msg),
}

pub struct App {
    pub peer: Peer,
    pub page: Page,
    /// The Accounts page, running.
    pub accounts: ark_app::Runtime<money_app::accounts::Accounts>,
    pub today: String,
    /// The category the inspector is about; none is the month's summary.
    pub selected: Option<Id<Category>>,
    /// An assigned cell being typed into, and what is typed.
    pub assigning: Option<(Id<Category>, String)>,
    /// The inspector's target, as typed.
    pub target: String,
    /// The inspector's formula, as typed.
    pub formula: formula::Form,
    pub collapsed: Vec<Id<Group>>,
    pub show_hidden: bool,
    /// The register's bar of conditions, shown by the toolbar's Filter
    /// (and always while the register is one no built-in names).
    pub show_filter: bool,
    pub new_group: Option<String>,
    pub new_category: Option<(Id<Group>, String)>,
    pub form: Option<TxnForm>,
    pub modal: Option<Modal>,
    /// The demo's bank, standing in for Plaid.
    #[cfg(any(feature = "demo", test))]
    pub bank: seed::Bank,
    /// The server this window is on; none for the demo, its own authority.
    pub server: Option<live::Server>,
    /// The keyboard every Ark app gets (`arkui::shell::Keyboard`): the
    /// grammar, with money's sequences bound (`cc`, `cg`, `rc`, `rg`), the
    /// which-key panel, and which pane has the keys.
    pub keyboard: arkui::shell::Keyboard,
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
    /// The shell (money/app/src/shell): the sections, their toolbars,
    /// search and inspector, the commands, in Rust as on the phones.
    pub shell: ark_app::Runtime<money_app::shell::MoneyShell>,
    /// Where the window has been: the address bar, and back and forward by
    /// `^O`/`^I` and a swipe (`arkui::history`).
    pub history: arkui::history::History<Route>,
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
    pub awaiting: Option<Option<Id<BankAsk>>>,
    /// Every page this window opened, in order.
    pub opened: Vec<String>,
    /// The ask whose answer was put away.
    pub dismissed: Option<Id<BankAsk>>,
    /// A kind of the inspector's category, being named.
    pub new_kind: Option<(Id<Category>, String)>,
    /// Settings' Amazon and Costco: what the server said, what is typed.
    pub settings: stores::Settings,
    /// A receipt being typed under the open transaction.
    pub paper: Option<receipt::Paper>,
    /// The photo the photo modal shows, by its file, once decoded.
    pub photo: Option<(String, iced::widget::image::Handle)>,
    /// The signed-in person's picture, for the account button: fetched
    /// once, kept on disk as harken's covers are (`arkui::images`).
    pub pictures: arkui::images::Images,
    /// The note, said for a moment over the page's foot (`arkui::toast`).
    pub toast: arkui::toast::Toast,
    /// How far down Reflect or Settings is scrolled, as it last said.
    pub top_y: f32,
    /// Their small header, in the band once the page's title has scrolled
    /// away (`arkui::compact`), and the page it is for.
    pub compact: arkui::compact::Compact,
    pub compact_for: Option<Page>,
}

impl App {
    pub fn with_peer(client: client::Replica, today: &str) -> App {
        let mut app = App {
            peer: Peer::open(client, &month_of(today)),
            page: Page::Budget,
            accounts: ark_app::Runtime::new(),
            today: today.into(),
            selected: None,
            assigning: None,
            target: String::new(),
            formula: formula::Form::default(),
            collapsed: Vec::new(),
            show_hidden: false,
            show_filter: false,
            new_group: None,
            new_category: None,
            form: None,
            modal: None,
            #[cfg(any(feature = "demo", test))]
            bank: seed::Bank::default(),
            server: None,
            // Nothing to say until something is done: what a window is (a
            // demo, who it is signed in as) is not news, and the toast is
            // for news (`arkui::toast`).
            note: String::new(),
            keyboard: arkui::shell::Keyboard {
                keys: money_keys(),
                which: arkui::which::Which::new(),
                focus: Pane::Content,
            },
            anchor: None,
            search: String::new(),
            drag: None,
            settle: None,
            pointer: Point::ORIGIN,
            window: Size::new(1200.0, 760.0),
            ctx: arkui::context::Context::new(),
            renaming: None,
            shell: ark_app::Runtime::new(),
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
            new_kind: None,
            settings: stores::Settings::default(),
            paper: None,
            photo: None,
            pictures: arkui::images::Images::new("money/pictures"),
            toast: arkui::toast::Toast::new(),
            top_y: 0.0,
            compact: arkui::compact::Compact::new(iced::time::Instant::now()),
            compact_for: None,
        };
        // A link the window was opened with, read before anything is drawn;
        // then where it is, as the history's first place.
        if let Some(linked) = app.history.poll(true) {
            if let Some(page) = app.page_of(&linked) {
                let _ = app.step(Message::Go(page));
            }
            app.landed(&linked);
        }
        app.history.sync(&app.route());
        app
    }

    /// The bar said `linked` and the app landed somewhere it spells
    /// otherwise — an older link's name, now the account's id; a stale one's
    /// fallback — so where it is is written over the link, not after it: the
    /// back button would go to the link, land here again, and never get past.
    fn landed(&mut self, linked: &Route) {
        if self.route() != *linked {
            self.history.replace_next();
        }
    }

    #[cfg(feature = "demo")]
    fn boot() -> (App, Task<Message>) {
        let today = day_of_ms(ark_peer::now_ms());
        (App::with_peer(seed::seeded(), &today), Task::none())
    }

    /// The window, open on nothing and saying so while its server's replica
    /// on this device is read on a thread (`arkui::later`): iced draws no
    /// window until boot returns, and a budget's log is reading and
    /// replaying. [`Message::Opened`] brings it back, to [`App::opened`].
    ///
    /// In a browser the replica is in memory, opened at once, and a window
    /// on nothing first would put its own page in the address bar before
    /// the link it was opened with had been read: it is opened here.
    #[cfg(not(feature = "demo"))]
    fn boot() -> (App, Task<Message>) {
        let (url, _) = live::config();
        let login = live::logins().recall(&url);
        {
            let today = day_of_ms(ark_peer::now_ms());
            let mut app = App::with_peer(client::open(Box::new(ark_peer::Memory::default())), &today);
            app.note = "opening the budget\u{2026}".into();
            let task = arkui::later::off_thread(move || live::open(&url, login.as_ref()), Message::Opened);
            (app, task)
        }
    }

    /// The window on its server, over `opened`, signed in as the login this
    /// device remembers there, or on its way to sign in.
    #[cfg(not(feature = "demo"))]
    fn opened(opened: Result<client::Replica, String>) -> (App, Task<Message>) {
        let today = day_of_ms(ark_peer::now_ms());
        let (url, user) = live::config();
        let logins = live::logins();
        let login = logins.recall(&url);
        let (client, note) = match opened {
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
        // The sign-in machine, told where this launch starts: signed in
        // already, or on its way to sign in (once a load), or — a page that
        // came back from signing in — trading the code it came with. Who is
        // signed in is the account button's to show, all the time; a note
        // says only what is news.
        let (code, returned) = (None, false);
        let remembered = ark_auth::onboarding::Remembered {
            server: app.server.as_ref().map(|s| s.url.clone()),
            login: app.server.as_ref().and_then(|s| s.login.clone()).map(Into::into),
            demo: false,
        };
        let task = app.onboard(Event::Launched { remembered, code, returned });
        if !note.is_empty() {
            app.note = note;
        }
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
            onboarding: Onboarding::new(live::app()),
            bell,
            device: None,
        });
        app
    }

    /// An event for the sign-in machine (`ark_auth::onboarding`), and its
    /// effects carried out: what it says said in the note.
    pub fn onboard(&mut self, event: Event) -> Task<Message> {
        let Some(s) = &self.server else { return Task::none() };
        let effects = s.onboarding.next(event);
        let said = s.onboarding.said();
        if let Some(t) = said.error.or(said.status) {
            self.note = t;
        }
        Task::batch(effects.into_iter().map(|e| self.effect(e)).collect::<Vec<_>>())
    }

    /// One of the machine's effects: the browser opened (the page goes to
    /// the server's sign-in and comes back with a code; the desktop hands
    /// the browser the same page and listens on loopback for it), the code
    /// traded, the login remembered or forgotten, the replica signed in.
    fn effect(&mut self, effect: Effect) -> Task<Message> {
        let Some(s) = &mut self.server else { return Task::none() };
        match effect {
            Effect::Remember { remembered } => {
                if let Some(logins) = &s.logins {
                    match remembered.login {
                        Some(l) => logins.remember(&s.url, &l.into()),
                        None => logins.forget(&s.url),
                    }
                }
                Task::none()
            }
            Effect::Browse { server, .. } => {
                let user = s.user.clone();
                off_thread(
                    move || ark_auth::client::code(&server, user.as_deref(), ark_auth::client::open_browser),
                    Message::Coded,
                )
            }
            Effect::Exchange { server, code } => off_thread(move || ark_auth::client::exchange(&server, &code), Message::SignedIn),
            Effect::Redeem { server, code } => off_thread(move || ark_auth::client::redeem(&server, &code), Message::SignedIn),
            Effect::OpenSession { login, .. } => {
                let login: ark_auth::Login = login.into();
                if s.login.as_ref() != Some(&login) {
                    self.signed_in(login);
                }
                self.note.clear();
                // The replica is the window's from the start: nothing to wait for.
                self.onboard(Event::Synced)
            }
            Effect::CloseSession => {
                s.login = None;
                Task::none()
            }
            Effect::FindServer { .. } | Effect::OpenDemo => Task::none(),
        }
    }

    /// Somebody signed in: the replica moves on as them — whatever was done
    /// signed out becomes theirs and is pushed — and the link dials.
    pub fn signed_in(&mut self, login: ark_auth::Login) {
        let Some(s) = &mut self.server else { return };
        self.peer.client.login(ark_peer::who(&login), login.token.clone());
        // The account button now shows who; the "signing in" said while
        // it was on its way is over.
        self.note.clear();
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
        self.stores_answered();
        let Some(reason) = p.denied else { return Task::none() };
        self.onboard(Event::Denied { reason })
    }

    fn title(&self) -> String {
        "money".into()
    }

    /// Author a change by the function's name: whether it was taken. A
    /// refusal is the domain's sentence, said as it is.
    pub fn author(&mut self, what: &str, name: &str, input: &impl ark::format::Encode) -> bool {
        self.taken(what, |c| c.mutate(name, input))
    }

    /// Author one of money's changes ([`MoneyChange`]): whether it was
    /// taken, as [`App::author`].
    pub fn change(&mut self, what: &str, change: &money_domain::MoneyChange) -> bool {
        self.taken(what, |c| c.mutate_bytes(change.name(), change.input()))
    }

    fn taken(&mut self, what: &str, f: impl FnOnce(&mut ark_peer::Peer) -> Result<ark::log::EntryId, String>) -> bool {
        let taken = match f(&mut self.peer.client) {
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
        // The shell's section is the page's.
        self.sync_shell();
        // The account's picture is asked for once somebody is signed in,
        // whichever message signed them in.
        let task = Task::batch([task, self.want_picture()]);
        // The receipt beside the register is the selected transaction's (the
        // one in the form, else the cursor's); a receipt typed goes with the
        // transaction it was for.
        let open = self.receipt_txn();
        if open != self.peer.txn() {
            self.paper = None;
        }
        self.peer.set_txn(open);
        self.load_photo();
        // An account is named in the bar, so a link to one waits for the
        // accounts to have arrived.
        let ready = !self.peer.accounts().is_empty();
        if let Some(linked) = self.history.poll(ready) {
            if let Some(page) = self.page_of(&linked) {
                if page != self.page {
                    let _ = self.step(Message::Go(page));
                }
            }
            self.landed(&linked);
        }
        self.history.sync(&self.route());
        // Whatever the message came to is said for a moment over the page
        // (`arkui::toast`), heard here once rather than wherever a note is
        // written.
        self.toast.heard(&self.note);
        // The small header: for a new page, hidden at once; otherwise in or
        // out as the page is scrolled past its title or back.
        let now = iced::time::Instant::now();
        if self.compact_for != Some(self.page) {
            self.compact_for = Some(self.page);
            self.compact.reset(now);
            self.top_y = 0.0;
        }
        let after = self.compact_title().map(|_| view::TITLED);
        self.compact.heard(self.top_y, after, false, now);
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

    /// Where the window is, as the bar spells it. An account or a saved
    /// filter that is gone is its route's fallback, All Accounts.
    pub fn route(&self) -> Route {
        let named = |route: Option<Route>, of: Route| route.unwrap_or_else(|| of.fallback());
        match self.page {
            Page::Budget => Route::Budget,
            Page::Reflect => Route::Reflect,
            // The overview has no path of its own yet: the bar says All Accounts.
            Page::Accounts => Route::Accounts,
            Page::Ledger(Ledger::All) => Route::Accounts,
            Page::Settings => Route::Settings,
            Page::Ledger(Ledger::Inbox) => Route::Inbox,
            Page::Ledger(Ledger::Account(id)) => named(
                self.peer.account(id).map(|a| Route::Account {
                    id: id.hex(),
                    name: a.name.clone(),
                }),
                Route::Account {
                    id: id.hex(),
                    name: String::new(),
                },
            ),
            Page::Ledger(Ledger::Saved(id)) => named(
                self.peer.saved(id).map(|f| Route::Filter {
                    id: id.hex(),
                    name: f.name.clone(),
                }),
                Route::Filter {
                    id: id.hex(),
                    name: String::new(),
                },
            ),
            Page::Ledger(Ledger::Adhoc) => Route::Filtered,
        }
    }

    /// The page a route is, if it is one here: an account or a saved filter
    /// by its id (an older link's by its name, `Route::names`); conditions
    /// not saved, the last ones this window had. The banks are Settings'
    /// here, and a category is the budget's.
    pub(crate) fn page_of(&self, route: &Route) -> Option<Page> {
        Some(match route {
            Route::Budget | Route::Category { .. } => Page::Budget,
            Route::Reflect => Page::Reflect,
            Route::Accounts => Page::Ledger(Ledger::All),
            Route::Settings | Route::Banks | Route::Bank { .. } => Page::Settings,
            Route::Inbox => Page::Ledger(Ledger::Inbox),
            Route::Account { .. } => Page::Ledger(Ledger::Account(
                self.peer.accounts().iter().find(|a| route.names(&a.id.hex(), &a.name))?.id,
            )),
            Route::Filter { .. } => Page::Ledger(Ledger::Saved(self.peer.filters().iter().find(|f| route.names(&f.id.hex(), &f.name))?.id)),
            Route::Filtered => Page::Ledger(Ledger::Adhoc),
            // A sign-in link is the onboarding's, not a page.
            Route::Join { .. } => return None,
        })
    }

    fn step(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Accounts(m) => return self.accounts_step(m),
            Message::Shell(m) => return self.shell_step(m),
            Message::Go(page) => {
                self.page = page;
                self.form = None;
                if page == Page::Settings {
                    self.ask_stores();
                }
                if let Page::Ledger(l) = page {
                    // A register opened as itself is what its conditions are
                    // changed from, later.
                    if l != Ledger::Adhoc {
                        self.peer.base = l;
                    }
                    let adhoc = self.peer.adhoc.clone();
                    self.peer.set_ledger(l, adhoc);
                    self.txn_anchor = None;
                }
                // A new visit opens at the top; back and forward where it was.
                self.scroll_due = Some(self.history.arrive(&self.route()));
            }
            Message::PageScrolled(y) => self.history.scrolled(&self.route(), y),
            Message::TopScrolled(y) => self.top_y = y,
            Message::PageTop => {
                self.top_y = 0.0;
                return iced::widget::operation::scroll_to(look::PAGE_TOP, iced::widget::scrollable::AbsoluteOffset { x: 0.0, y: 0.0 });
            }
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
                self.load_formula();
            }
            Message::ThisMonth => {
                self.peer.set_month(&month_of(&self.today));
                self.load_formula();
            }
            Message::Select(c) => self.select(if self.selected == c { None } else { c }),
            Message::EditAssigned(c) => {
                // A month that follows its formula is not typed into: why not
                // is said, and the inspector, where the formula is, follows.
                if self.follows(c) {
                    self.select(Some(c));
                    return Task::none();
                }
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
                if self.follows(c) {
                    return Task::none();
                }
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
            Message::Formula(m) => return self.formula_msg(m),
            Message::Filter(m) => return self.filter_msg(m),
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
                if from.is_some_and(|c| self.follows(c)) {
                    return Task::none();
                }
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
                // The register's account, when it is one account's.
                let account = self
                    .register_account()
                    .or_else(|| self.peer.accounts().iter().find(|a| a.on_budget && !a.closed).map(|a| a.id));
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
                if let Some(t) = self.peer.register().iter().find(|t| t.id == id) {
                    self.form = Some(TxnForm {
                        editing: Some(id),
                        account: Some(t.account_id),
                        date: t.date.clone(),
                        payee: if t.transfer { String::new() } else { t.payee.clone() },
                        category: t.category_id,
                        transfer_to: t.transfer_account_id,
                        memo: t.memo.clone(),
                        outflow: if t.amount < 0 { amount::editable(-t.amount) } else { String::new() },
                        inflow: if t.amount > 0 { amount::editable(t.amount) } else { String::new() },
                        cleared: t.cleared != Cleared::Uncleared,
                        by: t.user_id.clone(),
                        bank: t.bank_said().unwrap_or_default(),
                        split: None,
                    });
                    // A split one opens on its lines.
                    self.peer.set_splits(Some(id));
                    let lines = self.peer.splits().map(<[_]>::to_vec).unwrap_or_default();
                    if let (Some(f), false) = (&mut self.form, lines.is_empty()) {
                        f.split = Some(
                            lines
                                .iter()
                                .map(|l| SplitRow {
                                    category: l.category_id,
                                    amount: amount::editable(-l.amount),
                                })
                                .collect(),
                        );
                    }
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
                self.keyboard.focus = Pane::Content;
                self.txn_anchor = None;
                if self.txn_at == Some(id) {
                    return self.update(Message::EditTxn(id));
                }
                self.txn_at = Some(id);
            }
            Message::RegisterScrolled(y) => {
                self.register_scroll = y;
                self.history.scrolled(&self.route(), y);
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
            Message::CloseDrop => {
                self.dropdown = None;
                self.peer.set_candidates(None);
            }
            Message::Chose(choice) => return self.chose(choice),
            Message::Sync => self.sync(),
            Message::Banks(m) => return self.banks(m),
            Message::AccountMenu => {
                if let Some(account) = self.account() {
                    // Over the sidebar's foot, where it was pressed.
                    let rows = account.entries.len() as f32 + 1.0;
                    let up = self.window.height - 44.0 - rows * 30.0;
                    let right = arkui::layout_tokens::SIDEBAR_WIDTH.min(self.window.width) - 10.0;
                    self.ctx.open_menu(account.menu(right, up.max(0.0), self.window));
                }
            }
            Message::ShowFilter(on) => self.show_filter = on,
            Message::AutoMenu => self.auto_menu(),
            Message::SignOut => self.sign_out(),
            Message::AddDevice => return self.add_device(),
            Message::DeviceCode(made) => self.device_coded(made),
            Message::CopyLink(link) => return iced::clipboard::write(link),
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
                    let kind = money_domain::accounts::AccountKind::of(kind).unwrap_or_default();
                    let owed = kind.owes();
                    let open = OpenAccount {
                        name,
                        kind,
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
                } else if self.new_group.is_some() || self.new_category.is_some() || self.new_kind.is_some() {
                    (self.new_group, self.new_category, self.new_kind) = (None, None, None);
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
                    Naming::Group(g) => self.peer.groups().iter().find(|r| r.id == g).map(|r| r.name.clone()),
                    Naming::Account(a) => self.peer.account(a).map(|a| a.name.clone()),
                    Naming::Filter(f) => self.peer.saved(f).map(|f| f.name.clone()),
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
                Some((Naming::Filter(id), name)) => {
                    self.author("rename the filter", "rename_filter", &RenameFilter { id, name });
                }
                None => {}
            },
            Message::FileItems(items) => return self.open_dropdown(Choosing::Items(items), (self.pointer, false)),
            Message::Split(on) => {
                if let Some(f) = &mut self.form {
                    f.split = match on {
                        true => {
                            let whole = amount::parse(&f.outflow).unwrap_or(0) - amount::parse(&f.inflow).unwrap_or(0);
                            Some(vec![
                                SplitRow {
                                    category: f.category,
                                    amount: amount::editable(whole.abs()),
                                },
                                SplitRow::default(),
                            ])
                        }
                        false => None,
                    };
                    if on {
                        f.transfer_to = None;
                    }
                }
            }
            Message::SplitAmount(i, typed) => {
                if let Some(row) = self.form.as_mut().and_then(|f| f.split.as_mut()).and_then(|s| s.get_mut(i)) {
                    row.amount = typed;
                }
            }
            Message::SplitCategory(i) => return self.open_dropdown(Choosing::SplitLine(i), (self.pointer, false)),
            Message::SplitAdd => {
                if let Some(lines) = self.form.as_mut().and_then(|f| f.split.as_mut()) {
                    lines.push(SplitRow::default());
                }
            }
            Message::SplitRemove(i) => {
                if let Some(lines) = self.form.as_mut().and_then(|f| f.split.as_mut()) {
                    if lines.len() > 2 && i < lines.len() {
                        lines.remove(i);
                    }
                }
            }
            Message::NewKind(parent) => {
                self.new_kind = Some((parent, String::new()));
                return iced::widget::operation::focus(look::NEW_KIND);
            }
            Message::NewKindTyped(t) => {
                if let Some((_, typed)) = &mut self.new_kind {
                    *typed = t;
                }
            }
            Message::NewKindDone => {
                if let Some((parent_id, name)) = self.new_kind.take().filter(|(_, n)| !n.trim().is_empty()) {
                    self.author("add the kind", "add_subcategory", &AddSubcategory { parent_id, name });
                }
            }
            Message::Stores(m) => self.stores(m),
            Message::Receipt(m) => return self.receipt(m),
            Message::ChoosePayer(of) => {
                self.peer.set_candidates(Some(of.clone()));
                return self.open_dropdown(Choosing::Payer(of), (self.pointer, false));
            }
            Message::Swallow => {}
            Message::Wake => return self.wake(),
            Message::Coded(Ok(code)) => return self.onboard(Event::Code { code }),
            Message::Coded(Err(why)) => return self.onboard(Event::CameBack { why: Some(why) }),
            Message::SignedIn(Ok(login)) => return self.onboard(Event::SignedIn { login: login.into() }),
            // Said once, and not tried again until a reload: going straight
            // back would be a loop the person cannot stop.
            Message::SignedIn(Err(why)) => return self.onboard(Event::SignInFailed { why }),
            Message::Opened(opened) => {
                #[cfg(not(feature = "demo"))]
                if let Some(opened) = opened.take() {
                    let (mut app, task) = App::opened(opened);
                    app.window = self.window;
                    *self = app;
                    return task;
                }
                #[cfg(feature = "demo")]
                let _ = opened;
            }
            Message::Picture(done) => self.pictures.loaded(done),
            Message::WhichDue(key) => self.keyboard.which.due(key),
            Message::ToastDue(key) => self.toast.due(key),
        }
        Task::none()
    }

    /// "Sync". On a server it is an ask (`banks.sync`) the server's
    /// importer answers by asking the banks; what they have arrives as any
    /// change does, and the note said over the page says what came of
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
        let owed = (self.peer.accounts().iter()).find(|a| a.bank == seed::BANK_NAME).map_or(0, |a| a.cleared);
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
            .groups()
            .iter()
            .filter(|g| self.show_hidden || !g.hidden)
            .map(|g| {
                let lines: Vec<&BudgetLine> = self
                    .peer
                    .lines()
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
        self.page == Page::Budget && self.keyboard.focus == Pane::Content && self.keys_free()
    }

    fn key(&mut self, key: Key, mods: Modifiers) -> Task<Message> {
        // A command's shortcut, the menu bar's on a Mac: ⌘ there, Ctrl here.
        if let Some(run) = self.shortcut(&key, mods) {
            return self.update(Message::Shell(run));
        }
        let free = self.keys_free();
        let before = self.keyboard.focus;
        let panes = self.panes();
        let section = self.sidebar_at();
        // The budget binds its sequences; the register none, so its `c`
        // categorizes at once rather than waiting for a second key.
        let bindings: &[&'static str] = match self.page {
            Page::Budget => &MONEY_KEYS,
            _ => &[],
        };
        let places = self.places().len();
        let routed = self.keyboard.press(&key, mods, &panes, section, places, bindings);
        // Nothing moves between panes while something is over the window.
        if !free {
            self.keyboard.focus = before;
        }
        use arkui::shell::Routed;
        match routed {
            Routed::Done => Task::none(),
            Routed::Escape => self.update(Message::Escape),
            // Back and forward, the way the browser's buttons go: the bar
            // moves, and the next update arrives where it went.
            Routed::Jump(by) => {
                self.history.go(by);
                Task::none()
            }
            Routed::Section(i) if free => {
                // A cursor walking is a scrub: one history entry, not one a place.
                self.history.replace_next();
                let tag = self.place_tag(i);
                self.update(Message::Shell(ShellMsg::Select { tag }))
            }
            Routed::Scroll(place) if free => self.scroll_cursor(place),
            Routed::Pane(_, action) if self.ctx.is_open() => match action {
                // A menu that is up has the keys.
                vim::Action::Move(m) => match self.ctx.travel(m) {
                    Some(msg) => self.update(msg),
                    None => Task::none(),
                },
                vim::Action::Activate => self.update(Message::MenuActivate),
                _ => Task::none(),
            },
            Routed::Pane(pane, action) if free => match (pane, self.page) {
                (Pane::Sidebar, _) => Task::none(),
                (_, Page::Budget) => self.act(action),
                (_, Page::Ledger(_)) => self.register_act(action),
                _ => Task::none(),
            },
            _ => Task::none(),
        }
    }

    /// One command of the grammar, on the budget.
    pub fn act(&mut self, action: vim::Action) -> Task<Message> {
        let ids = self.row_ids();
        let sizes = self.sizes();
        let n = ids.len();
        match action {
            // The budget is one column: `h` is the sidebar, beside it.
            vim::Action::Move(vim::Motion::Left(_)) => self.keyboard.focus = Pane::Sidebar,
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
                let double = self
                    .last_click
                    .is_some_and(|(g, at)| g == grab && now.duration_since(at) < arkui::button::DOUBLE_CLICK);
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
        self.peer.register().iter().position(|t| t.id == at)
    }

    /// The register's rows under the cursor, first to last: one, or from the
    /// visual selection's anchor to the cursor.
    pub fn txn_lit(&self) -> Option<(usize, usize)> {
        let at = self.txn_cursor()?;
        let rows = self.peer.register();
        let from = self.txn_anchor.and_then(|a| rows.iter().position(|t| t.id == a)).unwrap_or(at);
        Some((from.min(at), from.max(at)))
    }

    /// The transactions under the cursor.
    pub fn txn_selected(&self) -> Vec<Id<Txn>> {
        match self.txn_lit() {
            Some((a, b)) => self.peer.register()[a..=b].iter().map(|t| t.id).collect(),
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
            .register()
            .iter()
            .map(|t| (t.id, t.approved, t.cleared, t.payee.clone()))
            .collect();
        let n = rows.len();
        match action {
            vim::Action::Move(vim::Motion::Left(_)) => self.keyboard.focus = Pane::Sidebar,
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
        let payees: Vec<String> = self.peer.register().iter().map(|t| t.payee.clone()).collect();
        match vim::find(&payees, &self.search, self.txn_cursor().unwrap_or(0), delta) {
            Some(i) => {
                self.txn_at = Some(self.peer.register()[i].id);
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
        (self.window.height - self.page_origin().y - self.ledger_top() - view::LEDGER_HEAD - 1.0).max(view::TXN_ROW)
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
        // The last row is followed by the page's padding, inside what
        // scrolls (`view::page_pad`): scrolled to it, the list goes to its
        // end, so the row stands clear of the window's foot.
        let last = i + 1 >= self.peer.register().len();
        let foot = if last { view::PAD } else { 0.0 };
        let seen = self.register_view();
        let y = if top < self.register_scroll {
            top
        } else if top + view::TXN_ROW + foot > self.register_scroll + seen {
            top + view::TXN_ROW + foot - seen
        } else {
            return Task::none();
        };
        self.register_scroll = y;
        self.history.scrolled(&self.route(), y);
        iced::widget::operation::scroll_to(look::REGISTER, iced::widget::scrollable::AbsoluteOffset { x: 0.0, y })
    }

    /// Where the category of transaction `id`'s row is on screen: the
    /// dropdown's corner, under it. The register's columns are the ones
    /// `view_ledger` lays out, worked out for this window's width.
    fn category_at(&self, id: Id<Txn>) -> (Point, bool) {
        let i = self.peer.register().iter().position(|t| t.id == id).unwrap_or(0);
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
            Some(id) => self.peer.register().iter().position(|t| t.id == id).unwrap_or(0),
            None => 0,
        };
        self.under(i as f32 * view::TXN_ROW + FORM_LINE)
    }

    /// The register's category column at `y` down its list (scrolled), as
    /// a dropdown's corner: above, if there is no room under it.
    fn under(&self, y: f32) -> (Point, bool) {
        let all = self.register_account().is_none();
        let origin = self.page_origin();
        let inset = view::PAD - 6.0 + arkui::table::ROW_PADDING[1];
        let left = origin.x + 1.0 + inset;
        let inspector = match self.shell_page().inspector {
            true => arkui::layout_tokens::INSPECTOR_WIDTH + 1.0,
            false => 0.0,
        };
        let width = self.window.width - origin.x - 1.0 - inspector - inset * 2.0;
        let fixed = view::DATE_COL + view::AMOUNT_COL + view::CLEARED_COL;
        let portions = if all { 10.0 } else { 8.0 };
        let payee = (width - fixed).max(0.0) * 3.0 / portions;
        let x = left + view::DATE_COL + payee;
        let top = origin.y + self.ledger_top() + view::LEDGER_HEAD + 1.0 + 4.0;
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
        if let Choosing::Payer(_) = &d.choosing {
            let rows = self.peer.candidates().map(<[_]>::to_vec).unwrap_or_default();
            let txns = rows
                .iter()
                .map(|c| (format!("{} \u{2014} {}", c.payee, c.date), c))
                .filter(|(label, c)| has(label) || has(&c.account))
                .map(|(label, c)| arkui::command::Item {
                    glyph: glyphs::CARD,
                    label,
                    detail: Some(format!("{} \u{00b7} {}", amount::money(c.amount), c.account)),
                    choose: Message::Chose(Choice::Paid(Some(c.txn_id))),
                })
                .collect();
            let none = arkui::command::Item {
                glyph: glyphs::CLOSE,
                label: "Nothing found yet".into(),
                detail: None,
                choose: Message::Chose(Choice::Paid(None)),
            };
            let mut groups = vec![arkui::command::Group {
                heading: "Paid by".into(),
                items: txns,
            }];
            if has("nothing found yet") {
                groups.push(arkui::command::Group {
                    heading: "None".into(),
                    items: vec![none],
                });
            }
            groups.retain(|g| !g.items.is_empty());
            return groups;
        }
        // For a filter, each category is ticked or not.
        let ticking = d.choosing == Choosing::Filter;
        let ticked = match ticking {
            true => self.register_filter().categories,
            false => Vec::new(),
        };
        let item = |label: String, detail: Option<String>, choice: Choice| arkui::command::Item {
            glyph: match choice {
                Choice::Category(c) if ticking => match ticked.contains(&c) {
                    true => glyphs::TICKED,
                    false => glyphs::UNTICK,
                },
                Choice::AnyCategory => glyphs::CLOSE,
                Choice::Transfer(_) => glyphs::SEND,
                Choice::Paid(_) => glyphs::CARD,
                Choice::Category(Some(c)) if c == money_domain::budget::ready_to_assign() => glyphs::GROWTH,
                Choice::Category(Some(_)) => view::category_glyph(&label),
                Choice::Category(None) => glyphs::CLOSE,
            },
            label,
            detail,
            choose: Message::Chose(choice),
        };
        let mut groups = Vec::new();
        if ticking && has("Any category") {
            groups.push(arkui::command::Group {
                heading: "Any".into(),
                items: vec![item("Any category".into(), None, Choice::AnyCategory)],
            });
        }
        if let (Choosing::Form, Some(f)) = (&d.choosing, &self.form) {
            let transfers = self
                .peer
                .accounts()
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
        for g in self.peer.groups().iter().filter(|g| !g.hidden && (moving || g.id != cards)) {
            let items = self
                .peer
                .lines()
                .iter()
                .filter(|l| l.group_id == g.id && !l.hidden)
                .flat_map(|l| {
                    // The envelope, then its kinds, each named from it.
                    let kinds = self
                        .peer
                        .tree()
                        .iter()
                        .filter(move |n| n.depth >= 2 && !n.hidden && n.path.starts_with(&format!("{} \u{203a} {} \u{203a} ", g.name, l.name)))
                        .filter_map(|n| self.peer.category_name(n.id).map(|name| (name, n.id)))
                        .filter(|(name, _)| has(name))
                        .map(|(name, id)| item(name, None, Choice::Category(Some(id))));
                    has(&l.name)
                        .then(|| item(l.name.clone(), Some(amount::money(l.available)), Choice::Category(Some(l.category_id))))
                        .into_iter()
                        .chain(kinds)
                        .collect::<Vec<_>>()
                })
                .collect();
            groups.push(arkui::command::Group {
                heading: g.name.clone(),
                items,
            });
        }
        // A card's own payment category, for its own transactions only: a
        // payment into it from outside the budget is debt paid and nothing
        // else, as its starting balance is.
        if let Some(l) = self
            .own_card(&d.choosing)
            .and_then(|c| self.peer.line(money_domain::cards::payment_category(c)))
        {
            if !moving && has(&l.name) {
                groups.push(arkui::command::Group {
                    heading: money_domain::cards::CARD_GROUP.into(),
                    items: vec![item(
                        l.name.clone(),
                        Some(amount::money(l.available)),
                        Choice::Category(Some(l.category_id)),
                    )],
                });
            }
        }
        if matches!(
            d.choosing,
            Choosing::Txns(_) | Choosing::Filter | Choosing::Items(_) | Choosing::SplitLine(_)
        ) && has("No category")
        {
            groups.push(arkui::command::Group {
                heading: "None".into(),
                items: vec![item("No category".into(), None, Choice::Category(None))],
            });
        }
        groups.retain(|g| !g.items.is_empty());
        groups
    }

    /// The card every transaction being categorised is on, when they are
    /// all on one card on the budget.
    fn own_card(&self, choosing: &Choosing) -> Option<Id<Account>> {
        let account = match choosing {
            Choosing::Form => self.form.as_ref()?.account?,
            Choosing::Txns(ids) => {
                let mut on = ids
                    .iter()
                    .map(|id| self.peer.register().iter().find(|t| t.id == *id).map(|t| t.account_id));
                let first = on.next()??;
                if !on.all(|a| a == Some(first)) {
                    return None;
                }
                first
            }
            _ => return None,
        };
        self.peer.account(account).filter(|a| a.kind == "credit" && a.on_budget).map(|a| a.id)
    }

    /// Keep row `n` in view, the budget moving only once it would leave.
    fn reveal(&self, n: usize) -> Task<Message> {
        let (line, lines) = outline::line_of(&self.sizes(), n);
        arkui::scroll::follow(look::BUDGET, line, lines, arkui::scroll::Place::Near)
    }

    /// `zz`, `zt` and `zb`: the page's list scrolled so its cursor's row is
    /// in the middle, at the top or at the foot, the cursor where it was.
    fn scroll_cursor(&mut self, place: arkui::scroll::Place) -> Task<Message> {
        match (self.keyboard.focus, self.page) {
            (Pane::Content, Page::Budget) => match self.at() {
                Some(n) => {
                    let (line, lines) = outline::line_of(&self.sizes(), n);
                    arkui::scroll::follow(look::BUDGET, line, lines, place)
                }
                None => Task::none(),
            },
            (Pane::Content, Page::Ledger(_)) => {
                let Some(i) = self.txn_at.and_then(|id| self.peer.register().iter().position(|t| t.id == id)) else {
                    return Task::none();
                };
                // The register's rows are one height, and a new
                // transaction's form one more above them.
                let above = usize::from(self.form.as_ref().is_some_and(|f| f.editing.is_none()));
                let rows = self.peer.register().len() + above;
                let content = rows as f32 * view::TXN_ROW;
                match arkui::scroll::offset_for(i + above, rows, self.register_view(), content, self.register_scroll, place) {
                    Some(y) => {
                        self.register_scroll = y;
                        self.history.scrolled(&self.route(), y);
                        iced::widget::operation::scroll_to(look::REGISTER, iced::widget::scrollable::AbsoluteOffset { x: 0.0, y })
                    }
                    None => Task::none(),
                }
            }
            _ => Task::none(),
        }
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
        self.load_formula();
    }

    /// Whether a category's month follows its formula, which is then said:
    /// nothing typed or moved by hand lands on it.
    fn follows(&mut self, c: Id<Category>) -> bool {
        let ruled = self.peer.line(c).is_some_and(|l| l.ruled);
        if ruled {
            self.note = format!("could not assign: {}", money_domain::formulas::FOLLOWS);
        }
        ruled
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

    fn chose(&mut self, choice: Choice) -> Task<Message> {
        // The dropdown, if one is up; the panel moving money, if not.
        let choosing = match (self.dropdown.take(), self.modal.take()) {
            (Some(d), modal) => {
                self.modal = modal;
                d.choosing
            }
            (None, Some(Modal::Choose(c))) => c,
            (None, other) => {
                self.modal = other;
                return Task::none();
            }
        };
        match choosing {
            Choosing::Form => {
                if let Some(f) = &mut self.form {
                    (f.category, f.transfer_to) = match choice {
                        Choice::Category(c) => (c, None),
                        Choice::Transfer(a) => (None, Some(a)),
                        Choice::Paid(_) => (f.category, f.transfer_to),
                        Choice::AnyCategory => (None, None),
                    };
                    if f.transfer_to.is_some() {
                        f.split = None;
                    }
                }
            }
            Choosing::Payer(of) => {
                self.peer.set_candidates(None);
                if let Choice::Paid(txn_id) = choice {
                    match of.charge_id {
                        Some(charge_id) => self.author("say what paid it", "pay_charge", &PayCharge { charge_id, txn_id }),
                        None => self.author(
                            "say what paid it",
                            "pay",
                            &MatchReceipt {
                                receipt_id: of.receipt_id,
                                txn_id,
                            },
                        ),
                    };
                }
            }
            Choosing::Items(items) => {
                if let Choice::Category(c) = choice {
                    self.author("file it", "file_items", &ClassifyItems { items, category_id: c });
                }
            }
            Choosing::SplitLine(i) => {
                if let (Choice::Category(c), Some(row)) = (choice, self.form.as_mut().and_then(|f| f.split.as_mut()).and_then(|s| s.get_mut(i))) {
                    row.category = c;
                }
            }
            Choosing::Filter => {
                let edit = match choice {
                    Choice::Category(c) => filter::Edit::Category(c),
                    _ => filter::Edit::AnyCategory,
                };
                return self.edit_filter(edit);
            }
            Choosing::Txns(ids) => {
                if let Choice::Category(c) = choice {
                    self.author("categorize", "categorize", &Categorize { ids, category_id: c });
                    self.txn_anchor = None;
                }
            }
            Choosing::Move { from, amount: typed } => {
                let Choice::Category(to) = choice else { return Task::none() };
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
        Task::none()
    }

    fn save_txn(&mut self) {
        let Some(f) = self.form.clone() else { return };
        let (Some(out), Some(inn)) = (amount::parse(&f.outflow), amount::parse(&f.inflow)) else {
            self.note = "an outflow or an inflow, as 12.50".into();
            return;
        };
        let amount = inn - out;
        let splits = match &f.split {
            Some(rows) => match split_lines(rows, amount) {
                Ok(lines) => lines,
                Err(why) => {
                    self.note = why;
                    return;
                }
            },
            None => vec![],
        };
        if let (Some(_), Some(id)) = (&f.split, f.editing) {
            return self.save_split(id, &f, splits, amount);
        }
        let taken = match f.editing {
            None if f.split.is_some() && splits.len() > 1 => {
                let Some(account) = f.account else {
                    self.note = "Choose an account.".into();
                    return;
                };
                let t = AddSplit {
                    account_id: account,
                    date: f.date,
                    payee: f.payee,
                    memo: f.memo,
                    amount,
                    cleared: f.cleared,
                    lines: splits,
                };
                self.author("add the transaction", "add_split", &t)
            }
            _ => {
                // The one form every client has (`transactions::Draft`):
                // its rules, and the change it is.
                let d = money_domain::transactions::Draft {
                    editing: f.editing,
                    account: f.account,
                    date: f.date,
                    payee: f.payee,
                    category: splits.first().map_or(f.category, |l| l.category_id),
                    transfer_to: f.transfer_to,
                    memo: f.memo,
                    amount,
                    cleared: f.cleared,
                };
                match d.to_change() {
                    Some(c) => self.change("save the transaction", &c),
                    None => {
                        self.note = d.problem().unwrap_or_default();
                        return;
                    }
                }
            }
        };
        if taken {
            self.form = None;
        }
    }

    /// A split saved: its lines adding up to the amount, then what else the
    /// form says. A split whose amount moves is unsplit first, as the
    /// domain keeps a split's amount to its lines.
    fn save_split(&mut self, id: Id<Txn>, f: &TxnForm, lines: Vec<SplitPart>, whole: i64) {
        let was = self.peer.register().iter().find(|t| t.id == id).map(|t| t.amount);
        if was != Some(whole) && self.peer.splits().is_some_and(|w| !w.is_empty()) {
            self.author("unsplit it", "split", &SplitTxn { id, lines: vec![] });
        }
        let e = EditTransaction {
            id,
            date: f.date.clone(),
            payee: f.payee.clone(),
            category_id: None,
            memo: f.memo.clone(),
            amount: whole,
            transfer_to: None,
        };
        if self.author("save the transaction", "edit_transaction", &e) && self.author("split it", "split", &SplitTxn { id, lines }) {
            self.form = None;
            self.peer.set_splits(None);
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
            // Something under the window's root wants a frame of its own.
            arkui::window::wakes().map(|()| Message::Swallow),
            // A photo dropped on the window: the open transaction's receipt.
            // A photo dropped on the window (or the page): the open
            // transaction's receipt.
            arkui::files::dropped().map(|p| Message::Receipt(receipt::Msg::Given(p))),
        ];
        if self.dropdown.is_some() {
            subs.push(arkui::command::keys(true).map(Message::DropKey));
        }
        if self.history.active()
            || self.drag.as_ref().is_some_and(|d| d.moving(now))
            || self.settle.is_some_and(|s| s.moving(now))
            || self.compact.moving(now)
        {
            subs.push(arkui::motion::Clock::Frames.ticks().map(|()| Message::Frame));
        }
        // A prefix half typed: the which-key panel after its delay. Keyed
        // from the top of the range, clear of the deadlines' milliseconds.
        if let Some(key) = self.keyboard.which.waiting() {
            subs.push(
                arkui::wake::at(u64::MAX - key, arkui::which::DELAY)
                    .with(key)
                    .map(|(key, ())| Message::WhichDue(key)),
            );
        }
        if let Some(timer) = self.toast.timer() {
            subs.push(timer.map(Message::ToastDue));
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

/// A split's lines as typed, in the transaction's sign (an outflow's lines
/// are outflows), adding up to `whole`; or what is wrong with them.
pub fn split_lines(rows: &[SplitRow], whole: i64) -> Result<Vec<SplitPart>, String> {
    let sign = if whole < 0 { -1 } else { 1 };
    let mut lines = Vec::new();
    for r in rows.iter().filter(|r| !r.amount.trim().is_empty() || r.category.is_some()) {
        let cents = amount::parse(&r.amount).ok_or_else(|| format!("{:?} is not an amount", r.amount))?;
        lines.push(SplitPart {
            category_id: r.category,
            amount: sign * cents.abs(),
            memo: String::new(),
        });
    }
    let sum: i64 = lines.iter().map(|l| l.amount).sum();
    if lines.len() > 1 && sum != whole {
        return Err(format!(
            "the lines come to {}, of {}",
            amount::money(sum.abs()),
            amount::money(whole.abs())
        ));
    }
    Ok(lines)
}

/// A desktop sign-in's step, which waits on a loopback listener for the
/// browser or on the server, on a thread of its own.
fn off_thread<T: Send + 'static>(work: impl FnOnce() -> Result<T, String> + Send + 'static, done: fn(Result<T, String>) -> Message) -> Task<Message> {
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    Task::perform(
        async move { rx.await.unwrap_or_else(|_| Err("the sign-in thread went away".into())) },
        done,
    )
}

pub fn main() -> iced::Result {
    // Which renderer draws the window, decided before iced starts and said
    // on stderr (`arkui::renderer`): iced would fall back to the CPU silently.
    arkui::renderer::choose();
    arkui::theme::install(palette::MONEY);
    // Whether to move, asked of the system now rather than in a frame.
    arkui::motion::reduced();
    // The app's boot, and what every arkui window starts with (a trackpad's
    // resting fingers heard, on Wayland).
    let boot = || {
        let (mut app, task) = App::boot();
        // The link the desktop was opened with (`money://account/<id>/checking`),
        // put in the address bar for the next update to arrive at, as a
        // browser's own link is. A sign-in link (`money://join?server=…`) is
        // not a page: it goes to the sign-in machine as `Event::Joined`, which
        // asks first.
        let mut joined = Task::none();
        if let Some(link) = arkui::navigator::link_arg("money") {
            use arkui::navigator::Navigator;
            let mut join = None;
            app.history.handle(&link, "money", &mut |r| match r {
                Route::Join { server, code } => {
                    join = Some(Event::Joined {
                        server: server.clone(),
                        code: code.clone(),
                    });
                    true
                }
                _ => false,
            });
            if let Some(event) = join {
                joined = app.onboard(event);
            }
        }
        (app, Task::batch([task, joined, arkui::window::boot()]))
    };
    let app = iced::application(boot, App::update, App::view)
        .subscription(App::subscription)
        .title(App::title)
        .window_size((1200.0, 760.0))
        .font(arkui::hero::FIRA_SANS_BOLD)
        // A receipt's paper is monospace, and the system's may not draw.
        .font(arkui::fonts::FIRA_MONO);
    // The screenshots' run (.github/screens/apps.md): this application, its
    // shots taken headless, and no window. Only when `ARK_SHOTS` asks.
    if let Some(shots) = arkui_shots::asked() {
        shots.run(app);
    }
    app.run()
}

/// A group as the budget draws it: every line it sums, and the rows the
/// cursor can be on — none while it is folded.
pub struct Shown<'a> {
    pub group: &'a GroupLine,
    pub lines: Vec<&'a BudgetLine>,
    pub rows: Vec<&'a BudgetLine>,
}

/// The budget's sequences: `cc` a category and `cg` a group made, `rc` and
/// `rg` one renamed. The first four of [`KEYS`], which say what they do.
const MONEY_KEYS: [&str; 4] = [KEYS[0].keys, KEYS[1].keys, KEYS[2].keys, KEYS[3].keys];

/// The keys money binds for itself, beside the grammar's
/// (`arkui::vim::GRAMMAR`): what the which-key panel lists, and what the
/// budget's and the register's `act` dispatch. The sequences come first,
/// and are [`MONEY_KEYS`].
pub const KEYS: &[vim::Binding] = &[
    vim::Binding {
        keys: "cc",
        what: "a new category",
    },
    vim::Binding {
        keys: "cg",
        what: "a new group",
    },
    vim::Binding {
        keys: "rc",
        what: "rename the category",
    },
    vim::Binding {
        keys: "rg",
        what: "rename the group",
    },
    vim::Binding {
        keys: "J",
        what: "move it down",
    },
    vim::Binding {
        keys: "K",
        what: "move it up",
    },
    vim::Binding {
        keys: "{",
        what: "the group up",
    },
    vim::Binding {
        keys: "}",
        what: "the group down",
    },
    vim::Binding {
        keys: "c",
        what: "categorize (register)",
    },
    vim::Binding {
        keys: "a",
        what: "approve (register)",
    },
    vim::Binding {
        keys: "i",
        what: "a new transaction",
    },
    vim::Binding {
        keys: "?",
        what: "these keys",
    },
];

/// money's prefixes, for the which-key panel.
pub const GROUPS: &[vim::Group] = &[vim::Group { key: "c", what: "create" }, vim::Group { key: "r", what: "rename" }];

/// The grammar, with the budget's sequences bound.
fn money_keys() -> vim::Keys {
    let mut keys = vim::Keys::new();
    keys.set_bindings(&MONEY_KEYS);
    keys
}

/// How wide the category dropdown is.
pub const DROP_WIDTH: f32 = 340.0;

/// What the bank called an imported transaction, where it says something the
/// payee does not: `CHIPOTLE 1234 SAN FRANCISCO CA` beside `Chipotle`. Blank
/// for one typed in, or one the bank and the payee name alike.
/// How far down the register's form its category box ends.
const FORM_LINE: f32 = 40.0;
