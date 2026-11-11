//! A register over the demo household: an account's balances and its days
//! as a table's rows, the search, a row's menu (approve, clear, delete
//! asked first), Approve All, Reconcile… and Edit as the sheets' requests.

use super::*;
use crate::testing::Demo;
use ark_app::Runtime;

fn checking(d: &Demo<Register>) -> AccountLine {
    let all: &[AccountLine] = d.rows(&MoneyView::Accounts);
    all.iter()
        .find(|a| a.name.contains("Checking"))
        .expect("the demo has a checking account")
        .clone()
}

fn open(account: Option<String>) -> (Demo<Register>, Runtime<Register>) {
    let mut d = Demo::<Register>::new();
    let mut rt = Runtime::<Register>::new();
    d.run(&mut rt, [RegisterMsg::Show { account }]);
    (d, rt)
}

/// One account's: its balances over it, its days newest first, a cell a
/// column (no account column). Falsified by leaving the account's column
/// in: a row of one account's had six cells.
#[test]
fn an_accounts_register_is_its_days() {
    let d0 = Demo::<Register>::new();
    let a = checking(&d0);
    let (d, rt) = open(Some(a.id.hex()));
    let p = rt.screen(&d);
    assert!(p.one && !p.all);
    assert_eq!((p.balance, p.cleared, p.uncleared), (a.balance, a.cleared, a.uncleared));
    assert_eq!(p.trend.last(), Some(&a.balance), "the trend ends at the balance");
    assert!(!p.days.is_empty() && !p.empty);
    let row = &p.days[0].rows[0];
    assert_eq!(row.cells.len(), 5);
    assert_eq!((row.tints.len(), row.glyphs.len()), (5, 5));
    assert!(p.days.iter().all(|d| !d.rows.is_empty()));
    let (_, every) = open(None);
    let d2 = Demo::<Register>::new();
    let p = every.screen(&d2);
    assert!(p.all);
    assert_eq!(p.days[0].rows[0].cells.len(), 6, "every account's has the account");
}

/// The search keeps what matches, and says so when nothing does.
#[test]
fn the_search_keeps_what_matches() {
    let (mut d, mut rt) = open(None);
    let first = rt.screen(&d).days[0].rows[0].cells[0].clone();
    d.run(&mut rt, [RegisterMsg::Search { text: first.clone() }]);
    let p = rt.screen(&d);
    assert!(p
        .days
        .iter()
        .flat_map(|d| &d.rows)
        .all(|r| r.said.to_lowercase().contains(&first.to_lowercase()) || r.cells.iter().any(|c| c.to_lowercase().contains(&first.to_lowercase()))));
    d.run(&mut rt, [RegisterMsg::Search { text: "zzzz nothing".into() }]);
    let p = rt.screen(&d);
    assert!(p.empty && p.days.is_empty());
    assert_eq!(p.empty_title, "No Results");
}

/// Approve All approves every one waiting, with one Success; a row is
/// cleared; a delete is asked first. Falsified by deleting on the menu's
/// press.
#[test]
fn a_rows_menu_does_what_it_says() {
    let (mut d, mut rt) = open(None);
    let p = rt.screen(&d);
    assert!(p.can_approve_all, "the demo has imports waiting");
    d.run(&mut rt, [RegisterMsg::ApproveAll]);
    let p = rt.screen(&d);
    assert!(!p.can_approve_all);
    assert!(p.days.iter().flat_map(|d| &d.rows).all(|r| !r.waiting));
    assert_eq!(d.haptics.performed(), vec![Haptic::Success]);

    let row = p
        .days
        .iter()
        .flat_map(|d| &d.rows)
        .find(|r| r.can_mark_cleared)
        .expect("one uncleared")
        .clone();
    d.run(
        &mut rt,
        [RegisterMsg::Clear {
            id: row.id.clone(),
            cleared: true,
        }],
    );
    let again = rt.screen(&d).days.iter().flat_map(|d| d.rows.clone()).find(|r| r.id == row.id).unwrap();
    assert!(again.can_mark_uncleared && !again.can_mark_cleared);

    let before: usize = rt.screen(&d).days.iter().map(|d| d.rows.len()).sum();
    d.run(&mut rt, [RegisterMsg::Delete { id: row.id.clone() }]);
    let p = rt.screen(&d);
    assert!(p.deleting && p.delete_message.contains("can't be undone"));
    assert_eq!(p.days.iter().map(|d| d.rows.len()).sum::<usize>(), before, "asked first");
    d.run(&mut rt, [RegisterMsg::ConfirmDelete]);
    let p = rt.screen(&d);
    assert!(!p.deleting);
    assert_eq!(p.days.iter().map(|d| d.rows.len()).sum::<usize>(), before - 1);
}

/// Edit, Categorize… and Reconcile… are the sheets'; a tap is the host's
/// details.
#[test]
fn the_sheets_are_asked_for() {
    let d0 = Demo::<Register>::new();
    let a = checking(&d0);
    let (mut d, mut rt) = open(Some(a.id.hex()));
    let id = rt.screen(&d).days[0].rows[0].id.clone();
    d.run(
        &mut rt,
        [
            RegisterMsg::Open { id: id.clone() },
            RegisterMsg::Edit { id: id.clone() },
            RegisterMsg::Categorize { id: id.clone() },
            RegisterMsg::Reconcile,
        ],
    );
    assert_eq!(
        d.requests,
        vec![
            MoneyRequest::OpenTransaction { key: id.clone() },
            MoneyRequest::Sheet {
                msg: SheetsMsg::Open { key: id.clone() }
            },
            MoneyRequest::Sheet {
                msg: SheetsMsg::Categorize {
                    ids: vec![id],
                    approve: false
                }
            },
            MoneyRequest::Sheet {
                msg: SheetsMsg::Reconcile { account: a.id.hex() }
            },
        ]
    );
}
