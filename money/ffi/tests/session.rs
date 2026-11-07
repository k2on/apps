//! money's views as the phones hold them (`Session`): what the shells did
//! by hand before — a view held while a screen shows it, a month's page as
//! its views, the rows spliced as changes land, the app told which moved,
//! and a change's refusal kept — over the demo's household.

use std::sync::{Arc, Mutex};

use ark_ffi::Twin;
use money_domain::accounts::AccountKind;
use money_domain::changes::MoneyChangeFfi;
use money_domain::{Assign, MoneyChange, OpenAccount, UniFfiTag};
use money_ffi::{money_demo, Changed, MoneyView, Sent, Session};

const TODAY: &str = "2026-10-26";

fn session() -> Arc<Session> {
    Session::new(money_demo(Some(TODAY.into())).unwrap())
}

#[derive(Default)]
struct Told(Mutex<Vec<MoneyView>>);

impl Changed for Told {
    fn moved(&self, views: Vec<MoneyView>) {
        self.0.lock().unwrap().extend(views);
    }
}

fn budget(m: &str) -> MoneyView {
    MoneyView::Budget { month: m.into() }
}

/// A change as a phone builds it: UniFFI's enum.
fn change(c: MoneyChange) -> MoneyChangeFfi {
    Twin::<UniFfiTag>::ffi(c)
}

fn open(name: &str) -> MoneyChangeFfi {
    change(MoneyChange::OpenAccount(OpenAccount {
        name: name.into(),
        kind: AccountKind::Checking,
        on_budget: true,
        balance: 0,
        date: TODAY.into(),
    }))
}

/// A month's budget held is its lines, its summary and its spending, read
/// at once; another month is opened beside it, and let go it reads
/// nothing. Falsified by `Budget` made of its lines alone: the summary
/// was none.
#[test]
fn a_months_budget_is_its_views() {
    let s = session();
    assert!(s.budget("2026-10".into()).lines.is_empty(), "nothing holds it");
    let october = s.hold(budget("2026-10"));
    let b = s.budget("2026-10".into());
    assert_eq!(b.summary.as_ref().map(|s| s.month.as_str()), Some("2026-10"));
    assert!(b.lines.iter().any(|l| l.name == "Dining Out"));
    assert!(!b.spending.is_empty(), "the seed spends in October");
    assert!(s.version(budget("2026-10")) > 0);
    let september = s.hold(budget("2026-09"));
    assert_eq!(s.budget("2026-09".into()).summary.map(|s| s.month), Some("2026-09".into()));
    drop(october);
    assert!(s.budget("2026-10".into()).lines.is_empty(), "let go");
    assert_eq!(s.version(budget("2026-10")), 0);
    assert!(!s.budget("2026-09".into()).lines.is_empty(), "still held");
    drop(september);
}

/// A change lands in what is held, the listener is told which views moved
/// — the page with its views — and only those, and a view's version grows
/// with it. Falsified by telling every view held: the register, which the
/// assignment did not touch, was told too.
#[test]
fn the_app_is_told_what_moved() {
    let s = session();
    let told = Arc::new(Told::default());
    s.set_listener(Some(told.clone()));
    let _b = s.hold(budget("2026-10"));
    let _r = s.hold(MoneyView::Register { account: None });
    let line = s.budget("2026-10".into()).lines.into_iter().find(|l| l.name == "Groceries").unwrap();
    let v = s.version(budget("2026-10"));
    let assign = change(MoneyChange::Assign(Assign {
        month: "2026-10".into(),
        category_id: money_domain::Id::new(line.category_id.0),
        amount: line.assigned + 2_500,
    }));
    assert!(matches!(s.send(assign), Sent::Intent { .. }));
    let moved = told.0.lock().unwrap().clone();
    assert!(moved.contains(&budget("2026-10")), "{moved:?}");
    assert!(!moved.contains(&MoneyView::Register { account: None }), "{moved:?}");
    assert!(s.version(budget("2026-10")) > v);
    let after = s.budget("2026-10".into()).lines.into_iter().find(|l| l.name == "Groceries").unwrap();
    assert_eq!(after.assigned, line.assigned + 2_500, "spliced");
}

/// A register held by two screens stays while either does. Falsified by
/// letting go at the first drop, as a hold without a count would.
#[test]
fn a_view_is_held_while_any_screen_holds_it() {
    let s = session();
    let all = MoneyView::Register { account: None };
    let (a, b) = (s.hold(all.clone()), s.hold(all.clone()));
    let n = s.register(None).len();
    assert!(n > 0, "the seed's transactions");
    drop(a);
    assert_eq!(s.register(None).len(), n);
    b.release();
    assert!(s.register(None).is_empty());
    drop(b);
}

/// A refusal is the domain's sentence, kept until the next change is
/// taken. Falsified by `send` throwing it away: `refusal` was none.
#[test]
fn a_refusal_is_kept_until_the_next_change() {
    let s = session();
    let ready = "account needs a name";
    assert_eq!(s.check(open(" ")).as_deref(), Some("an account needs a name"), "asked before it is sent");
    assert!(matches!(s.send(open(" ")), Sent::Refused { why } if why.contains(ready)));
    assert!(s.refusal().is_some_and(|w| w.contains(ready)));
    assert!(matches!(s.send(open("Travel")), Sent::Intent { .. }));
    assert_eq!(s.refusal(), None);
}

/// The receipts' rows are records too, as every view's are. Falsified by
/// `receipts` reading the rows' JSON: it did not compile.
#[test]
fn the_receipts_are_records() {
    let s = session();
    let _r = s.hold(MoneyView::Receipts);
    let rows = s.receipts();
    assert!(!rows.is_empty() && rows.iter().all(|r| !r.store.is_empty()), "{rows:?}");
    let _p = s.hold(MoneyView::Photos);
    let _ = s.photos();
}

/// A search is the core's filter over a register: payee or the bank's own
/// words, any case; told when it moves, as any view is.
#[test]
fn a_search_is_the_cores_filter() {
    let s = session();
    let told = Arc::new(Told::default());
    s.set_listener(Some(told.clone()));
    let q = MoneyView::Search {
        account: None,
        text: "WHOLE".into(),
    };
    let _h = s.hold(q.clone());
    let found = s.search(None, "WHOLE".into());
    assert!(
        !found.is_empty()
            && found
                .iter()
                .all(|t| t.payee.to_lowercase().contains("whole") || t.imported_payee.to_lowercase().contains("whole")),
        "{found:?}"
    );
    let first = found[0].account_id;
    let _one = s.hold(MoneyView::Search {
        account: Some(first),
        text: "whole".into(),
    });
    assert!(s.search(Some(first), "whole".into()).iter().all(|t| t.account_id == first));
    assert!(s.version(q) > 0);
}

/// A search finds what a phone's register shows: the category, the
/// account, the memo and the amount as written, as well as the payee.
/// Falsified by the filter's text reading the payee and the bank's words
/// alone: a search for a category's name found nothing.
#[test]
fn a_search_finds_every_field_a_register_shows() {
    let s = session();
    let _all = s.hold(MoneyView::Register { account: None });
    let rows = s.register(None);
    let find = |text: &str| {
        let _h = s.hold(MoneyView::Search {
            account: None,
            text: text.into(),
        });
        s.search(None, text.into())
    };
    let grocery = rows.iter().find(|t| t.category == "Groceries").expect("the seed buys groceries");
    let by_category = find("groceries");
    assert!(by_category.iter().any(|t| t.id == grocery.id), "{by_category:?}");
    let by_account = find(&grocery.account.to_uppercase());
    assert!(by_account.iter().any(|t| t.id == grocery.id));
    let written = ark_format::amount(grocery.amount, &ark_format::NumberStyle::us());
    let by_amount = find(&written);
    assert!(by_amount.iter().any(|t| t.id == grocery.id), "{written}");
    if let Some(m) = rows.iter().find(|t| !t.memo.trim().is_empty()) {
        assert!(find(m.memo.trim()).iter().any(|t| t.id == m.id), "the memo {:?}", m.memo);
    }
    assert!(find("no such thing anywhere").is_empty());
}
