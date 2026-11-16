//! money in a window its host draws itself, over the demo's household on
//! the host's test place (not a phone's: its sections are the window's
//! places): the address says where it is and brings a fresh window there,
//! Settings is the window's own.

use ark_app::host::{Hosting, Link, Place};
use ark_app::window::Windowed;
use money_core::{AccountLine, MoneyView};

use super::MoneyMsg;
use crate::host::tests::{open, Test};
use crate::host::Money;
use crate::shell::{account_tag, ShellMsg};
use crate::MoneyRequest;
use ark_app::host::ShellAsk;

fn send(h: &mut Hosting<Money>, place: &mut Test, msg: MoneyMsg) {
    h.with(place, |app, cx| app.take(msg, cx));
    for n in h.take_natives() {
        h.with(place, |app, cx| app.native(n, cx));
    }
}

fn address(h: &Hosting<Money>, place: &Test) -> String {
    h.app().address(place, h.now_ms(place))
}

/// A fresh window brought to `address`, as a page loaded at it is.
fn arrive(address: &str) -> (Hosting<Money>, Test) {
    let (mut h, mut place) = open();
    h.link(Link::Address(address.into()), &mut place);
    for n in h.take_natives() {
        h.with(&mut place, |app, cx| app.native(n, cx));
    }
    (h, place)
}

fn checking(place: &Test) -> AccountLine {
    let mut out = vec![];
    place.read(&mut |v| out = v.rows::<AccountLine>(&MoneyView::Accounts).to_vec());
    out.into_iter()
        .find(|a| a.name.contains("Checking"))
        .expect("the demo's checking account")
}

/// The address is the shell's own: a section is its route's path, the UI
/// over it the query. Falsified by an address that does not change with
/// the place, or a query written for nothing.
#[test]
fn the_address_says_the_place() {
    let (mut h, mut place) = open();
    assert_eq!(address(&h, &place), "/budget/");
    send(&mut h, &mut place, MoneyMsg::Shell(ShellMsg::Select { tag: "insights".into() }));
    assert_eq!(address(&h, &place), "/insights/");
    send(&mut h, &mut place, MoneyMsg::Shell(ShellMsg::Select { tag: "transactions".into() }));
    send(&mut h, &mut place, MoneyMsg::Shell(ShellMsg::Search { text: "Target".into() }));
    assert_eq!(address(&h, &place), "/transactions/?search=Target");
    let a = checking(&place);
    send(
        &mut h,
        &mut place,
        MoneyMsg::Shell(ShellMsg::Select {
            tag: account_tag(&a.id.hex()),
        }),
    );
    let at = address(&h, &place);
    assert!(at.starts_with(&format!("/account/{}/", a.id.hex())), "{at}");
}

/// What the address says, a fresh window arrives at: the account, the
/// inspector, the search and a month that is not this one. Falsified by a
/// month lost on a reload, or an account's register opened on another.
#[test]
fn an_address_brings_a_fresh_window_back() {
    let (mut h, mut place) = open();
    let a = checking(&place);
    h.with(&mut place, |_, cx| cx.request(MoneyRequest::StepMonth { by: -1 }));
    send(
        &mut h,
        &mut place,
        MoneyMsg::Shell(ShellMsg::Select {
            tag: account_tag(&a.id.hex()),
        }),
    );
    send(&mut h, &mut place, MoneyMsg::Shell(ShellMsg::Inspector { shown: true }));
    send(&mut h, &mut place, MoneyMsg::Shell(ShellMsg::Search { text: "Rent".into() }));
    let at = address(&h, &place);
    assert!(
        at.contains("inspector=1") && at.contains("search=Rent") && at.contains("month=2026-09"),
        "{at}"
    );
    let (back, place) = arrive(&at);
    let s = back.app().shell.state();
    assert_eq!(s.section, account_tag(&a.id.hex()));
    assert!(s.inspector);
    assert_eq!(s.search, "Rent");
    assert_eq!(back.app().month(), "2026-09");
    assert_eq!(address(&back, &place), at, "the same address, read back");
}

/// Category Details is its category's route, and a fresh window opens on
/// it. Falsified by the category written while its inspector is put away.
#[test]
fn category_details_is_its_route() {
    let (mut h, mut place) = open();
    h.with(&mut place, |_, cx| cx.request(MoneyRequest::OpenCategory { key: "groceries".into() }));
    let at = address(&h, &place);
    assert!(at.starts_with("/category/") && at.ends_with("?inspector=1"), "{at}");
    let (back, place) = arrive(&at);
    assert_eq!(address(&back, &place), at);
    assert!(back.app().shell.page(&place, back.now_ms(&place)).category.is_some());
}

/// Settings is the window's own: put up by what the shell asks, its pane in
/// the address, put away by Done. Falsified by Settings left to a native
/// the window never takes.
#[test]
fn settings_is_the_windows_own() {
    let (mut h, mut place) = open();
    h.with(&mut place, |_, cx| cx.request(MoneyRequest::Settings { pane: "Household".into() }));
    let natives = h.take_natives();
    assert_eq!(natives, vec![ShellAsk::Settings { pane: "Household".into() }]);
    for n in natives {
        h.with(&mut place, |app, cx| app.native(n, cx));
    }
    let now = h.now_ms(&place);
    assert!(h.app().settings(&place, now).is_some());
    assert_eq!(address(&h, &place), "/settings/?pane=household");
    let (back, place2) = arrive("/settings/?pane=banks");
    assert!(back.app().settings(&place2, now).is_some());
    assert_eq!(back.app().shell.state().pane, "Connected Banks");
    send(&mut h, &mut place, MoneyMsg::CloseSettings);
    assert!(h.app().settings(&place, now).is_none());
    assert_eq!(address(&h, &place), "/budget/");
}

/// Auto-Assign from the month's inspector reaches the budget's runtime.
/// Falsified by a press the window drops.
#[test]
fn the_month_inspector_presses_reach_the_budget() {
    let (mut h, mut place) = open();
    let before = h.version();
    send(&mut h, &mut place, MoneyMsg::Budget(crate::budget::BudgetMsg::AutoUnderfunded));
    assert!(h.version() > before);
}
