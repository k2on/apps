//! The demo through the bridge: the seeded household read typed, as the
//! apps' previews and screenshots draw it (its bank is money's host's:
//! money/app/src/host).

use ark_ffi::Peer;
use money_bridge::{money_add_months, money_demo};
use money_bridge::{AccountLineView, BudgetLineView, SummaryView, TxnLineView};
use money_seed as seed;
use std::sync::Arc;

const TODAY: &str = "2026-10-26";

fn demo() -> Arc<Peer> {
    money_demo(Some(TODAY.into())).unwrap()
}

/// Falsified by `money_demo` seeding for the clock rather than the day it
/// is given: the month on screen had no lines for 2026-10; and by the
/// demo opened without the importer's role: its bank's import was refused.
#[test]
fn the_demo_is_the_seeded_household() {
    let p = demo();
    let st = p.status();
    assert!(st.alone, "{st:?}");
    assert_eq!(st.user, seed::ME);

    let accounts = AccountLineView::new(p.clone(), "accounts".into(), "null".into()).unwrap().rows();
    let names: Vec<&str> = accounts.iter().map(|a| a.name.as_str()).collect();
    for n in ["Checking", "Savings", "Cash", "Mortgage", "Retirement", "Chase Sapphire"] {
        assert!(names.contains(&n), "{names:?}");
    }
    let month = r#"{"month":"2026-10"}"#;
    let lines = BudgetLineView::new(p.clone(), "month".into(), month.into()).unwrap().rows();
    let dining = lines.iter().find(|l| l.name == "Dining Out").unwrap();
    assert!(dining.available < 0, "the seed overspends it: {}", dining.available);
    let summary = SummaryView::new(p.clone(), "summary".into(), month.into()).unwrap().rows();
    assert_eq!(summary.len(), 1);
    assert!(summary[0].ready_to_assign > 0, "{:?}", summary[0]);

    let inbox = TxnLineView::new(p.clone(), "inbox".into(), "null".into()).unwrap();
    let waiting = inbox.rows().len();
    assert!(waiting > 0, "the card's last days wait for a look");
}

/// Falsified by the day's noon taken in the local zone's sense of a day
/// (`now_ms` itself): two demos for one day differed.
#[test]
fn a_day_is_the_same_household_every_time() {
    let (a, b) = (demo(), demo());
    let rows = |p: Arc<Peer>| TxnLineView::new(p, "register".into(), r#"{"account_id":null}"#.into()).unwrap().rows();
    assert_eq!(rows(a), rows(b));
    assert!(money_demo(Some("26 October".into())).is_err());
    assert_eq!(money_add_months("2026-11".into(), 2), "2027-01");
}
