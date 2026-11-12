//! money's part of its host, over the demo's household on a test's place:
//! what every platform's host now runs, so what each used to do by hand is
//! held here once.

use ark_app::host::{Hosting, Link, Place, Session};
use ark_peer::store::Store;
use ark_peer::Peer;
use money_core::{MoneyChange, MoneyView, TxnLine};

use super::{Money, ShellAsk};
use crate::shell::{MoneyShell, ShellMsg};
use crate::testing::NOW;
use crate::MoneyRequest;

/// A place over the demo's household, as a phone (`stacks`) or a window.
pub(crate) struct Test {
    pub(crate) peer: Peer,
    pub(crate) store: Store<MoneyView>,
    stacks: bool,
    said: Vec<String>,
    dialled: bool,
}

impl Test {
    fn new(stacks: bool) -> Test {
        Test {
            peer: money_seed::open(money_seed::seeded_disk("2026-10-26", NOW)),
            store: Store::new(),
            stacks,
            said: vec![],
            dialled: false,
        }
    }
}

fn demo() -> Session {
    Session {
        demo: true,
        today: "2026-10-26".into(),
        version: "0.1 (1)".into(),
        leaves: true,
        adds_devices: false,
        ..Default::default()
    }
}

impl Place<MoneyView, MoneyChange> for Test {
    fn read(&self, f: &mut dyn FnMut(&dyn ark::views::Views<MoneyView>)) {
        f(&self.store)
    }
    fn now_ms(&self) -> i64 {
        NOW
    }
    fn send(&mut self, change: MoneyChange) -> Result<(), String> {
        let r = self.store.sent(self.peer.mutate_bytes(change.name(), change.input()));
        self.store.settle(&mut self.peer);
        r.map(|_| ())
    }
    fn hold(&mut self, key: MoneyView) {
        self.store.hold(&mut self.peer, key);
        self.store.settle(&mut self.peer);
    }
    fn release(&mut self, key: &MoneyView) {
        self.store.release(&mut self.peer, key)
    }
    fn stacks(&self) -> bool {
        self.stacks
    }
    fn session(&self) -> Session {
        demo()
    }
    fn notice(&mut self, said: String) {
        self.said.push(said)
    }
    fn dial(&mut self) {
        self.dialled = true
    }
}

pub(crate) fn open(stacks: bool) -> (Hosting<Money>, Test) {
    let mut place = Test::new(stacks);
    let h = Hosting::<Money>::open(&demo(), &mut place);
    (h, place)
}

fn ask(h: &mut Hosting<Money>, place: &mut Test, r: MoneyRequest) {
    h.with(place, |_, cx| cx.request(r));
}

/// Opened, every screen is on the session's month and the shell's Settings
/// says who is using it. Falsified by a runtime left on no month, or the
/// facts never told.
#[test]
fn a_session_opens_on_its_month_with_its_facts() {
    let (h, place) = open(true);
    let m = h.app();
    assert_eq!(m.month(), "2026-10");
    assert_eq!(m.budget.state().month, "2026-10");
    assert_eq!(m.transactions.state().month, "2026-10");
    assert_eq!(m.insights.state().month, "2026-10");
    let facts = &m.shell.state().settings.facts;
    assert_eq!((facts.who.as_str(), facts.demo, facts.version.as_str()), ("Alex", true, "0.1 (1)"));
    assert!(h.idle(&place), "the views every screen names are held");
    assert!(m.register.is_none(), "a phone pushes its own registers");
}

/// A month stepped moves every runtime that shows one. Falsified by the
/// budget alone moving, as a shell that tells one runtime did.
#[test]
fn a_month_stepped_moves_every_screen() {
    let (mut h, mut place) = open(true);
    let v = h.version();
    ask(&mut h, &mut place, MoneyRequest::StepMonth { by: -1 });
    let m = h.app();
    assert_eq!(m.month(), "2026-09");
    assert_eq!(m.budget.state().month, "2026-09");
    assert_eq!(m.transactions.state().month, "2026-09");
    assert_eq!(m.insights.state().month, "2026-09");
    assert_eq!(m.sheets.state().month, "2026-09");
    assert!(h.version() > v);
    ask(&mut h, &mut place, MoneyRequest::ThisMonth);
    assert_eq!(h.app().month(), "2026-10");
}

/// The demo's bank imports behind Sync here, as the server's importer
/// would, and says what it brought; on a server the importer is asked and
/// the link dialled. Falsified by the demo's sync importing nothing.
#[test]
fn the_demos_bank_imports_and_says_so() {
    let (mut h, mut place) = open(true);
    let all = MoneyView::Register { account: None };
    place.hold(all.clone());
    let before = place.store.rows::<TxnLine>(&all).len();
    ask(&mut h, &mut place, MoneyRequest::Refresh);
    assert_eq!(place.said, vec!["Chase: 2 new from the bank \u{2014} one still pending".to_string()]);
    assert_eq!(place.store.rows::<TxnLine>(&all).len(), before + 2);
    assert!(!place.dialled, "the demo has no server to dial");
    ask(&mut h, &mut place, MoneyRequest::Refresh);
    ask(&mut h, &mut place, MoneyRequest::Refresh);
    assert_eq!(
        place.said[1..],
        ["Chase: 2 from the bank, 1 pending one posted", "Chase: nothing new at the bank"]
    );
}

/// A page opened goes on a phone's stack, and is one of a window's places
/// there; a category's month is Category Details either way. Falsified by
/// a window asked to push a page it shows in its sidebar.
#[test]
fn a_route_is_pushed_on_a_phone_and_selected_in_a_window() {
    let (mut h, mut place) = open(true);
    let a = place.store.rows::<money_core::AccountLine>(&MoneyView::Accounts)[0].clone();
    let route = crate::Route::Account {
        id: a.id.hex(),
        name: a.name.clone(),
    };
    h.with(&mut place, |m, cx| m.shell.send(ShellMsg::Select { tag: "accounts".into() }, cx));
    h.take_natives();
    let r = route.clone();
    h.with(&mut place, |m, cx| ark_app::host::HostedApp::route(m, r, cx));
    assert_eq!(h.take_natives(), vec![ShellAsk::Open { route: route.clone() }]);

    let (mut h, mut place) = open(false);
    h.with(&mut place, |m, cx| ark_app::host::HostedApp::route(m, route, cx));
    assert!(h.take_natives().is_empty());
    assert_eq!(h.app().shell.state().section, format!("accounts/{}", a.id.hex()));
    assert_eq!(
        h.app().register.as_ref().map(|r| r.state().account.clone()),
        Some(Some(a.id.hex())),
        "the register follows the shell"
    );
}

/// A category a launch names is Category Details in the shell's inspector,
/// on the budget. Falsified by the name matched in no case.
#[test]
fn a_category_named_is_inspected() {
    let (mut h, mut place) = open(true);
    ask(&mut h, &mut place, MoneyRequest::OpenCategory { key: "Groceries".into() });
    let s = h.app().shell.state();
    assert!(s.inspector);
    assert_eq!(s.section, "budget");
    assert!(s.category.is_some());
}

/// The shell's search is Transactions'. Falsified by the search typed
/// reaching the shell alone.
#[test]
fn the_shells_search_is_followed() {
    let (mut h, mut place) = open(true);
    h.send::<MoneyShell>(ShellMsg::Search { text: "coffee".into() }, &mut place);
    assert_eq!(h.app().transactions.state().search, "coffee");
}

/// A link taken puts away what shows first (the toolkit's pages too), and
/// Settings, Add a Device and Sign Out are the toolkit's. Falsified by a
/// link that leaves a sheet over the page it opens.
#[test]
fn a_link_puts_away_what_shows_and_natives_are_taken_in_order() {
    let (mut h, mut place) = open(true);
    h.take_natives();
    h.link(Link::Launch(vec!["-screen".into(), "insights".into()]), &mut place);
    assert_eq!(h.take_natives(), vec![ShellAsk::Reset]);
    ask(&mut h, &mut place, MoneyRequest::Settings { pane: "Household".into() });
    ask(&mut h, &mut place, MoneyRequest::AddDevice);
    ask(&mut h, &mut place, MoneyRequest::SignOut);
    assert_eq!(
        h.take_natives(),
        vec![ShellAsk::Settings { pane: "Household".into() }, ShellAsk::AddDevice, ShellAsk::SignOut]
    );
    assert_eq!(h.app().shell.state().pane, "Household");
}

/// The session's day is the time pages are worked out on: noon of the
/// demo's day when the clock is on another.
#[test]
fn the_time_is_on_the_sessions_day() {
    let noon_26 = super::session_now(0, "2026-10-26");
    assert_eq!(super::session_now(noon_26 + 3_600_000, "2026-10-26"), noon_26 + 3_600_000);
    assert_eq!(super::session_now(noon_26 + 30 * 86_400_000, "2026-10-26"), noon_26);
    assert_eq!(money_core::dates::day_of_ms(super::session_now(0, "2026-10-26")), "2026-10-26");
}
