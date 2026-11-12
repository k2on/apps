//! money in the browser: the shell every platform runs
//! (`money_app::shell::MoneyShell`) and its screens — Budget, Accounts,
//! Transactions, a register, the sheets — run by ark-app here as on the
//! phones and the desktop, drawn by arkui-web (Leptos over the DOM); the
//! screens the shell hosts that are not in Rust's vocabulary yet are this
//! crate's (`screens`), as money/iced has its own.
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

use std::sync::Arc;

use ark_app::host::{Has, Hosting, Place, Session};
use ark_app::App;
use ark_peer::store::Store;
use ark_peer::Peer as Replica;
use arkui_web::{AnyView, WebApp};
use money_app::accounts::AccountsMsg;
use money_app::budget::BudgetMsg;
use money_app::host::{Money, Native};
use money_app::insights::InsightsMsg;
use money_app::register::RegisterMsg;
use money_app::sheets::SheetsMsg;
use money_app::shell::view::{commands, money_shell};
use money_app::shell::{MoneyShell, ShellMsg};
use money_app::transactions::TransactionsMsg;
use money_app::MoneyRequest;
use money_core::dates::{days_from_civil, parse_day};
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
    /// The Transactions tab's (`money_app::transactions`).
    Transactions(TransactionsMsg),
    /// Insights' (`money_app::insights`).
    Insights(InsightsMsg),
    /// The register's (`money_app::register`).
    Register(RegisterMsg),
    /// The sheets' (`money_app::sheets`).
    Sheets(SheetsMsg),
    /// The budget's inspector's Auto-Assign, for the month shown.
    Auto(Auto),
    /// Settings shown, or put away.
    Settings(bool),
    /// Where the address says the page is besides the shell's launch
    /// arguments (`MoneyWeb::route`): a key and its value.
    At(String, String),
    /// The socket has something to say, or the peer's deadline came
    /// (`live`): it is pumped.
    Wake,
    /// A device's operation answered after its message (a file chosen):
    /// its runtime takes it.
    Answered,
    /// What came of one of the sign-in machine's effects, or a press on
    /// the sign-in page (`live`).
    Onboard(ark_auth::onboarding::Event),
    /// A press on the first run's page.
    Welcome(ark_app::welcome::WelcomeMsg),
}

/// The page: its replica and the views it holds, and money's part of its
/// host (`money_app::host`, every platform's) running every screen over it.
pub struct MoneyWeb {
    pub peer: Replica,
    pub store: Store<MoneyView>,
    pub today: String,
    pub money: Hosting<Money>,
    /// Settings shown: the page's own window over the app.
    pub settings: bool,
    /// How many changes this page has sent the household.
    pub changes: u64,
    /// Changes counted and not sent (`WebApp::dry_run`).
    pub dry: bool,
    /// The page on its server, signed in there; none for the demo.
    pub live: Option<live::Live>,
}

/// The views the page holds as long as it is open: what its own parts read
/// (a field's suggestions).
const ALWAYS: [MoneyView; 3] = [MoneyView::Accounts, MoneyView::Groups, MoneyView::Payees];

/// Noon on `today`, in milliseconds: the time a screen is worked out at.
pub fn noon(today: &str) -> i64 {
    let (y, m, d) = parse_day(today).unwrap_or((1970, 1, 1));
    days_from_civil(y, m, d) * 86_400_000 + 43_200_000
}

/// The page as the place money's host runs over (`ark_app::host::Place`).
pub struct WebPlace<'a> {
    peer: &'a mut Replica,
    store: &'a mut Store<MoneyView>,
    live: Option<&'a live::Live>,
    today: &'a str,
    changes: &'a mut u64,
    dry: bool,
}

/// Who is signed in where, as the page knows it: its server and login, or
/// the demo in the page alone (which has no way out).
fn session_of(peer: &Replica, live: Option<&live::Live>, today: &str) -> Session {
    let version = env!("CARGO_PKG_VERSION").to_string();
    let Some(live) = live else {
        return Session {
            demo: true,
            today: today.into(),
            version,
            ..Default::default()
        };
    };
    let status = peer.status();
    let user = live.login.as_ref().map(|l| l.user.clone()).unwrap_or_default();
    Session {
        demo: false,
        today: today.into(),
        name: user.name.clone(),
        user: user.id.clone(),
        email: user.email.clone(),
        picture: ark_auth::picture_url(&live.server, &user.id, &user.picture).unwrap_or_default(),
        server: ark_auth::onboarding::display(&live.server),
        linked: Some(status.linked),
        pending: status.pending as i64,
        version,
        leaves: live.login.is_some(),
        // The browser makes no code for another device.
        adds_devices: false,
    }
}

fn read_files(peer: &Replica, store: &Store<MoneyView>, f: &mut dyn FnMut(&dyn ark::views::Views<MoneyView>)) {
    // A file a row names is read from the replica's memory (`data:`).
    f(&ark::views::WithFiles {
        views: store,
        url: |file: &ark::file::FileRef| peer.file_url(file),
    })
}

impl Place<MoneyView, MoneyChange> for WebPlace<'_> {
    fn read(&self, f: &mut dyn FnMut(&dyn ark::views::Views<MoneyView>)) {
        read_files(self.peer, self.store, f)
    }
    fn now_ms(&self) -> i64 {
        noon(self.today)
    }
    fn send(&mut self, change: MoneyChange) -> Result<(), String> {
        *self.changes += 1;
        if self.dry {
            return Ok(());
        }
        let r = self.peer.mutate_bytes(change.name(), change.input()).map(|_| ());
        self.store.settle(self.peer);
        r
    }
    fn hold(&mut self, key: MoneyView) {
        self.store.hold(self.peer, key);
        self.store.settle(self.peer);
    }
    fn release(&mut self, key: &MoneyView) {
        self.store.release(self.peer, key)
    }
    /// The page's own file input is its scanner (a scan and a photo
    /// chosen); any other module answers Unavailable.
    fn modules(&self) -> ark_app::Modules {
        #[allow(unused_mut)]
        let mut modules = ark_app::Modules::new();
        #[cfg(target_arch = "wasm32")]
        modules.register::<dyn ark_app::modules::scanner::Scanner>(Arc::new(ark_scanner::Browser));
        modules
    }
    /// A file chosen later wakes the page, which drains its runtimes.
    fn waker(&self) -> ark_app::host::Waker {
        Arc::new(|| arkui_web::send(Msg::Answered))
    }
    fn want(&mut self, file: ark::file::FileRef) {
        let _ = self.peer.open_file(&file);
    }
    fn keep(&mut self, bytes: &[u8], media: &str) -> Option<ark::file::FileRef> {
        self.peer.put_file(bytes, media).ok()
    }
    fn session(&self) -> Session {
        session_of(self.peer, self.live, self.today)
    }
}

/// The page as a place only read (a page drawn, the idle signal): it sends
/// and holds nothing.
pub struct Seen<'a> {
    peer: &'a Replica,
    store: &'a Store<MoneyView>,
    live: Option<&'a live::Live>,
    today: &'a str,
}

impl Place<MoneyView, MoneyChange> for Seen<'_> {
    fn read(&self, f: &mut dyn FnMut(&dyn ark::views::Views<MoneyView>)) {
        read_files(self.peer, self.store, f)
    }
    fn now_ms(&self) -> i64 {
        noon(self.today)
    }
    fn send(&mut self, _: MoneyChange) -> Result<(), String> {
        Err("a page drawn sends nothing".into())
    }
    fn hold(&mut self, _: MoneyView) {}
    fn release(&mut self, _: &MoneyView) {}
    fn session(&self) -> Session {
        session_of(self.peer, self.live, self.today)
    }
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
        MoneyWeb::over(money_seed::open(disk), today, None)
    }

    /// The page over `peer` (on its server when `live`): its views held,
    /// money's host opened on this month.
    pub fn over(mut peer: Replica, today: String, live: Option<live::Live>) -> MoneyWeb {
        let mut store = Store::new();
        for v in ALWAYS {
            store.hold(&mut peer, v);
        }
        store.settle(&mut peer);
        let mut changes = 0;
        let money = {
            let mut place = WebPlace {
                peer: &mut peer,
                store: &mut store,
                live: live.as_ref(),
                today: &today,
                changes: &mut changes,
                dry: false,
            };
            let session = place.session();
            Hosting::open(&session, &mut place)
        };
        let mut app = MoneyWeb {
            peer,
            store,
            today,
            money,
            settings: false,
            changes,
            dry: false,
            live,
        };
        // The page's own `?theme=`, which it wore from its first frame, is
        // the shell's appearance from here.
        if let Some(key) = arkui_web::query("theme").filter(|t| t == "light" || t == "dark") {
            app.send::<MoneyShell>(ShellMsg::AppearanceKey { key });
        }
        app
    }

    pub fn now_ms(&self) -> i64 {
        noon(&self.today)
    }

    /// The page, as a place read.
    pub fn seen(&self) -> Seen<'_> {
        Seen {
            peer: &self.peer,
            store: &self.store,
            live: self.live.as_ref(),
            today: &self.today,
        }
    }

    /// money's runtimes, as they are.
    pub fn app(&self) -> &Money {
        self.money.app()
    }

    /// The month the budget shows.
    pub fn month(&self) -> String {
        self.app().month().to_string()
    }

    /// What the screen `A` shows now.
    pub fn page<A>(&self) -> A::Screen
    where
        Money: Has<A>,
        A: App<Key = MoneyView, Change = MoneyChange, Route = money_app::Route, Request = MoneyRequest>,
    {
        self.money.page::<A>(&self.seen())
    }

    pub fn shell_page(&self) -> money_app::shell::ShellPage {
        self.page::<MoneyShell>()
    }

    /// `f` over money's host and the page as its place; what it asked of
    /// the page carried out after.
    fn host<R>(&mut self, f: impl FnOnce(&mut Hosting<Money>, &mut WebPlace<'_>) -> R) -> R {
        let r = {
            let MoneyWeb {
                peer,
                store,
                today,
                money,
                changes,
                dry,
                live,
                ..
            } = self;
            let mut place = WebPlace {
                peer,
                store,
                live: live.as_ref(),
                today,
                changes,
                dry: *dry,
            };
            f(money, &mut place)
        };
        for n in self.money.take_natives() {
            self.native(n);
        }
        self.appearance();
        r
    }

    /// `msg` taken by the screen `A`.
    fn send<A>(&mut self, msg: A::Msg)
    where
        Money: Has<A>,
        A: App<Key = MoneyView, Change = MoneyChange, Route = money_app::Route, Request = MoneyRequest>,
    {
        self.host(|h, place| h.send::<A>(msg, place))
    }

    /// A request of money's host's.
    fn request(&mut self, r: MoneyRequest) {
        self.host(|h, place| h.with(place, |_, cx| cx.request(r)))
    }

    /// What money's host asked of the page itself.
    fn native(&mut self, n: Native) {
        match n {
            Native::Settings { .. } => self.settings = true,
            Native::Reset => self.settings = false,
            // Signed out of the page's server: the session revoked, the
            // sign-in machine back to its start. The demo's page has no way
            // out (`SessionFacts::leaves`).
            Native::SignOut => {
                if self.live.is_some() {
                    let _ = self.peer.sign_out();
                    self.onboard(ark_auth::onboarding::Event::SignOut);
                    self.settings = false;
                }
            }
            // The page's sections are its places, never a stack; it makes no
            // code for another device (`SessionFacts::adds_devices`).
            Native::Open { .. } | Native::AddDevice => {}
        }
    }

    /// What the page follows of the shell that is no screen's: Plaid's page,
    /// once the server has answered this page's ask with it, in a tab of its
    /// own, which the page cannot follow, so it is taken as closed once
    /// opened.
    fn follow(&mut self) {
        if let Some(url) = self.shell_page().settings.plaid {
            #[cfg(target_arch = "wasm32")]
            if let Some(w) = web_sys::window() {
                let _ = w.open_with_url_and_target(&url, "_blank");
            }
            self.send::<MoneyShell>(ShellMsg::PageClosed { url });
        }
    }

    /// The category the shell inspects, by its id, when it names one.
    pub fn category(&self) -> Option<Id<Category>> {
        let key = self.app().shell.state().category.clone()?;
        money_app::shell::inspect::category_named(screens::lines(self), &key).map(|l| l.category_id)
    }

    /// Whether nothing is left to come on the page: no runtime waits on
    /// an operation and each holds every view it needs; with
    /// [`MoneyWeb::version`] unchanged, what a screenshot driver waits for.
    pub fn is_idle(&self) -> bool {
        self.money.idle(&self.seen())
    }

    /// The runtimes' versions summed: grows with every message any took.
    pub fn version(&self) -> u64 {
        self.money.version()
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

    /// The appearance the shell asks for (a launch's, Settings'), over
    /// the system's: the page's `data-theme`, or none.
    fn appearance(&self) {
        #[cfg(target_arch = "wasm32")]
        if let Some(root) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element()) {
            use money_app::shell::Appearance;
            let _ = match self.app().shell.state().appearance {
                Some(Appearance::Dark) => root.set_attribute("data-theme", "dark"),
                Some(Appearance::Light) => root.set_attribute("data-theme", "light"),
                None => root.remove_attribute("data-theme"),
            };
        }
    }
}

/// What the address's path says after the section, each a key and its
/// value (`MoneyWeb::route`): what the page shows.
const PATH_KEYS: [&str; 3] = ["category", "txn", "settings"];

/// What its query says: the UI's state over it, in the order read back.
const QUERY_KEYS: [&str; 2] = ["search", "month"];

impl MoneyWeb {
    /// Where the page is, as its address (ui/docs/app.md, "The Browser's
    /// Modes"). The path is what it shows: the section's tag
    /// (`budget`, `accounts/<id>`), the month when it is not this one, the
    /// category or the transaction chosen, Settings and its pane —
    /// `/budget/2026-10/category/<id>/`. The query is the UI over it: the
    /// inspector and the search. `arrive` reads it back.
    pub fn route(&self) -> arkui_web::Address {
        let s = self.app().shell.state();
        let mut path: Vec<String> = s.section.split('/').filter(|p| !p.is_empty()).map(String::from).collect();
        if self.month() != self.today[..7] {
            path.push(self.month());
        }
        if let Some(c) = self.category() {
            path.extend(["category".into(), c.hex()]);
        }
        if let Some(t) = &s.transaction {
            path.extend(["txn".into(), t.clone()]);
        }
        if self.settings {
            path.extend(["settings".into(), s.pane.clone()]);
        }
        let mut r = arkui_web::Address::at(path);
        if s.inspector {
            r = r.with("inspector", "1");
        }
        if !s.search.is_empty() {
            r = r.with("search", s.search.clone());
        }
        r
    }

    /// One of the address's keys taken (`route`'s, read back).
    fn at(&mut self, key: &str, v: String) {
        let category = |app: &MoneyWeb| screens::lines(app).iter().find(|l| l.category_id.hex() == v).map(|l| l.category_id);
        match key {
            "month" if dates_ok(&v) => {
                let by = months_between(&self.month(), &v);
                if by != 0 {
                    self.request(MoneyRequest::StepMonth { by });
                }
            }
            "category" => {
                let c = category(self);
                self.send::<MoneyShell>(ShellMsg::InspectCategory { id: c.map(|c| c.hex()) });
            }
            "txn" => self.send::<MoneyShell>(ShellMsg::InspectTransaction { id: Some(v) }),
            "search" => self.send::<MoneyShell>(ShellMsg::Search { text: v }),
            "settings" => {
                self.settings = true;
                if !v.is_empty() {
                    self.send::<MoneyShell>(ShellMsg::Pane { title: v });
                }
            }
            _ => {}
        }
    }
}

/// How many months on from `from` `to` is (`2026-10` to `2026-08`: -2).
fn months_between(from: &str, to: &str) -> i32 {
    let n = |m: &str| -> i32 { m.get(..4).and_then(|y| y.parse().ok()).unwrap_or(0) * 12 + m.get(5..7).and_then(|m| m.parse().ok()).unwrap_or(0) };
    n(to) - n(from)
}

fn dates_ok(month: &str) -> bool {
    month.len() == 7 && parse_day(&format!("{month}-01")).is_some()
}

impl WebApp for MoneyWeb {
    type Msg = Msg;

    fn view(&self) -> AnyView {
        self.say_idle();
        // The first run's pages while the app is not running yet, and its
        // pages of what money is over the app while they show.
        if let Some(live) = self.live.as_ref().filter(|l| !l.running() || l.first_run.page().intro.is_some()) {
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
            Msg::Shell(m) => self.send::<MoneyShell>(m),
            Msg::Accounts(m) => self.send::<money_app::accounts::Accounts>(m),
            Msg::Budget(m) => self.send::<money_app::budget::Budget>(m),
            Msg::Transactions(m) => self.send::<money_app::transactions::Transactions>(m),
            Msg::Insights(m) => self.send::<money_app::insights::Insights>(m),
            Msg::Register(m) => self.host(|h, place| {
                h.with(place, |money, cx| {
                    if let Some(r) = &mut money.register {
                        r.send(m, cx)
                    }
                })
            }),
            Msg::Sheets(m) => self.send::<money_app::sheets::Sheets>(m),
            Msg::Auto(how) => {
                let month = self.month();
                self.host(|h, place| {
                    h.with(place, |_, cx| {
                        let _ = cx.place.send(MoneyChange::AutoAssign(AutoAssign { month, how, only: vec![] }));
                    })
                })
            }
            Msg::Settings(on) => self.settings = on,
            Msg::At(k, v) => self.at(&k, v),
            Msg::Wake => {
                self.wake();
                self.host(|h, place| h.moved(place));
            }
            Msg::Answered => self.host(|h, place| h.moved(place)),
            Msg::Onboard(e) => self.onboard(e),
            Msg::Welcome(m) => self.first_run(m),
        }
        self.follow();
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
        // The launch arguments every shell reads, as a query of the older
        // form says them (`?screen=budget&inspector=1`); otherwise the
        // address is the launch (`ShellMsg::Address`).
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
        // money's pages, by their path (`Route::Welcome`), over the app.
        if route.path.first().map(String::as_str) == Some("welcome") {
            self.show_intro();
        }
        if args.is_empty() {
            // The section, an account, the inspector, the appearance: what
            // the address names, selected as a launch selects it.
            out.push(Msg::Shell(ShellMsg::Address { address: route.url() }));
            if segs.next() == Some("accounts") && segs.peek().is_some_and(|s| !PATH_KEYS.contains(s)) {
                segs.next();
            }
        } else {
            let section = match segs.next() {
                Some("accounts") if segs.peek().is_some_and(|s| !PATH_KEYS.contains(s)) => segs.next().map(money_app::shell::account_tag),
                Some(s) => Some(s.to_string()),
                None => None,
            };
            if let Some(tag) = section {
                out.push(Msg::Shell(ShellMsg::Select { tag }));
            }
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
        // money's host opened again over the same household: every screen
        // as a load finds it.
        let fresh = {
            let MoneyWeb {
                peer,
                store,
                today,
                changes,
                dry,
                live,
                ..
            } = self;
            let mut place = WebPlace {
                peer,
                store,
                live: live.as_ref(),
                today,
                changes,
                dry: *dry,
            };
            let session = place.session();
            Hosting::open(&session, &mut place)
        };
        let old = std::mem::replace(&mut self.money, fresh);
        self.host(|_, place| old.close(place));
        self.settings = false;
        self.follow();
        true
    }
}

/// money's own styles, after ArkUI's.
pub const CSS: &str = include_str!("../money.css");
