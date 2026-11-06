//! money's views as the phones hold them (`Session`): what the shells did
//! by hand before — a view held while a screen shows it, a month's page as
//! its views, the rows spliced as changes land, the app told which moved,
//! and a change's refusal kept — over the demo's household.

use std::sync::{Arc, Mutex};

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
    let input = format!(
        r#"{{"month":"2026-10","category_id":"{}","amount":{}}}"#,
        ark::hash::hex(&line.category_id.0),
        line.assigned + 2_500
    );
    assert!(matches!(s.send("assign".into(), input), Sent::Intent { .. }));
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
    let open = |name: &str| format!(r#"{{"name":"{name}","kind":"checking","on_budget":true,"balance":0,"date":"{TODAY}"}}"#);
    assert!(matches!(s.send("open_account".into(), open(" ")), Sent::Refused { why } if why.contains(ready)));
    assert!(s.refusal().is_some_and(|w| w.contains(ready)));
    assert!(matches!(s.send("open_account".into(), open("Travel")), Sent::Intent { .. }));
    assert_eq!(s.refusal(), None);
}

/// The receipts' rows, which have no record, come as their JSON.
#[test]
fn the_receipts_come_as_their_json() {
    let s = session();
    let _r = s.hold(MoneyView::Receipts);
    let rows = s.receipts();
    assert!(rows.iter().all(|r| r.starts_with('{') && r.contains("\"store\"")), "{rows:?}");
}
