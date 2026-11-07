//! The demo through the bridge: the seeded household read typed, as the
//! apps' previews and screenshots draw it, and its bank's syncs.

use ark::format::Encode;
use ark_ffi::Peer;
use money_domain::{AccountLine, BudgetLine, Summary};
use money_ffi::{money_add_months, money_demo, money_month_name, DemoBank};
use money_seed as seed;
use std::sync::Arc;

const TODAY: &str = "2026-10-26";

/// A view's rows, each read as `T` from its bytes.
fn read<T: Encode>(rows: Vec<Vec<u8>>) -> Vec<T> {
    rows.iter().map(|r| T::from_bytes(r).unwrap()).collect()
}

fn demo() -> Arc<Peer> {
    money_demo(Some(TODAY.into())).unwrap()
}

/// Falsified by `money_demo` seeding for the clock rather than the day it
/// is given: the month on screen had no lines for 2026-10; and by the
/// demo opened without the importer's role: Sync was refused.
#[test]
fn the_demo_is_the_seeded_household() {
    let p = demo();
    let st = p.status();
    assert_eq!(st.link, "alone", "{st:?}");
    assert_eq!(st.user, seed::ME);

    let accounts: Vec<AccountLine> = read(p.view("accounts.accounts".into(), "null".into()).unwrap().rows());
    let names: Vec<&str> = accounts.iter().map(|a| a.name.as_str()).collect();
    for n in ["Checking", "Savings", "Cash", "Mortgage", "Retirement", "Chase Sapphire"] {
        assert!(names.contains(&n), "{names:?}");
    }
    let month = r#"{"month":"2026-10"}"#;
    let lines: Vec<BudgetLine> = read(p.view("budget.month".into(), month.into()).unwrap().rows());
    let dining = lines.iter().find(|l| l.name == "Dining Out").unwrap();
    assert!(dining.available < 0, "the seed overspends it: {}", dining.available);
    let summary: Vec<Summary> = read(p.view("budget.summary".into(), month.into()).unwrap().rows());
    assert_eq!(summary.len(), 1);
    assert!(summary[0].ready_to_assign > 0, "{:?}", summary[0]);

    let inbox = p.view("transactions.inbox".into(), "null".into()).unwrap();
    let waiting = inbox.rows().len();
    assert!(waiting > 0, "the card's last days wait for a look");

    let bank = DemoBank::new();
    let first = bank.sync(p.clone(), Some(TODAY.into())).unwrap();
    assert_eq!(first.note, "Chase: 2 new from the bank \u{2014} one still pending");
    assert!(
        p.outcome(first.intent).is_some_and(|o| o.pending && o.refused.is_none()),
        "no server: kept here"
    );
    let t = inbox.take();
    assert!(!t.ops.is_empty(), "{t:?}");
    assert_eq!(inbox.rows().len(), waiting + 2);
    let second = bank.sync(p.clone(), Some(TODAY.into())).unwrap();
    assert_eq!(second.note, "Chase: 2 from the bank, 1 pending one posted");
    assert_eq!(bank.sync(p, Some(TODAY.into())).unwrap().note, "Chase: nothing new at the bank");
}

/// Falsified by the day's noon taken in the local zone's sense of a day
/// (`now_ms` itself): two demos for one day differed.
#[test]
fn a_day_is_the_same_household_every_time() {
    let (a, b) = (demo(), demo());
    let rows = |p: Arc<Peer>| p.view("transactions.register".into(), r#"{"account_id":null}"#.into()).unwrap().rows();
    assert_eq!(rows(a), rows(b));
    assert!(money_demo(Some("26 October".into())).is_err());
    assert_eq!(money_add_months("2026-11".into(), 2), "2027-01");
    assert_eq!(money_month_name("2026-10".into()), "October 2026");
}
