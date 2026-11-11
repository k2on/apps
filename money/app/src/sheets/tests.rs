//! The sheets over the demo household: a transaction added on the keypad
//! with a payee offered and its category, one corrected and deleted (asked
//! first), a category given and approved with it, an account reconciled.

use super::*;
use crate::testing::Demo;
use ark_app::Runtime;

fn open() -> (Demo<Sheets>, Runtime<Sheets>) {
    let mut d = Demo::<Sheets>::new();
    let mut rt = Runtime::<Sheets>::new();
    d.run(
        &mut rt,
        [SheetsMsg::Month {
            month: "2026-10".into(),
            today: "2026-10-26".into(),
        }],
    );
    (d, rt)
}

fn register(d: &Demo<Sheets>) -> Vec<TxnLine> {
    d.rows::<TxnLine>(&MoneyView::Register { account: None }).to_vec()
}

/// A new one: nothing to save until it has an amount and a payee; a payee
/// offered brings its category; saved, it is in the register, out going
/// out, and the sheet is gone with a Success. Falsified by saving the
/// amount unsigned: it came in.
#[test]
fn a_transaction_is_added() {
    let (mut d, mut rt) = open();
    d.run(&mut rt, [SheetsMsg::New { account: None }]);
    let t = rt.screen(&d).transaction.expect("the form is up");
    assert!(t.adding && !t.can_save);
    assert_eq!(t.note, "Enter an amount.");
    assert_eq!(t.day, "2026-10-26");
    assert!(!t.account.is_empty(), "the usual account");
    let payee = d.rows::<PayeeLine>(&MoneyView::Payees)[0].clone();
    d.run(
        &mut rt,
        [
            SheetsMsg::Cents { cents: 1234 },
            SheetsMsg::Payee {
                payee: payee.name[..2].to_string(),
            },
        ],
    );
    let t = rt.screen(&d).transaction.unwrap();
    assert!(t.has_payees, "payees are offered as it is typed");
    d.run(&mut rt, [SheetsMsg::ChosePayee { name: payee.name.clone() }]);
    let t = rt.screen(&d).transaction.unwrap();
    assert_eq!(t.payee, payee.name);
    if let Some(c) = payee.category_id {
        assert_eq!(t.category, format!("c{}", c.hex()), "its last category came with it");
    }
    assert!(t.can_save, "{}", t.note);
    let before = register(&d).len();
    d.run(&mut rt, [SheetsMsg::Save]);
    assert!(rt.screen(&d).transaction.is_none(), "put away once taken");
    let all = register(&d);
    assert_eq!(all.len(), before + 1);
    assert!(all.iter().any(|x| x.payee == payee.name && x.amount == -1234));
    assert_eq!(d.haptics.performed(), vec![Haptic::Success]);
}

/// One already there opens by its id with its amount and way; corrected,
/// it is saved; Delete asks first.
#[test]
fn a_transaction_is_corrected_and_deleted() {
    let (mut d, mut rt) = open();
    let t0 = register(&d).into_iter().find(|t| !t.transfer && t.amount < 0).unwrap();
    d.run(&mut rt, [SheetsMsg::Open { key: t0.id.hex() }]);
    let t = rt.screen(&d).transaction.unwrap();
    assert!(t.editing && t.way == "outflow" && t.cents == -t0.amount);
    d.run(&mut rt, [SheetsMsg::Memo { memo: "Corrected".into() }, SheetsMsg::Save]);
    assert!(register(&d).iter().any(|t| t.id == t0.id && t.memo == "Corrected"));
    d.run(&mut rt, [SheetsMsg::Open { key: t0.id.hex() }, SheetsMsg::Delete]);
    let t = rt.screen(&d).transaction.unwrap();
    assert!(t.deleting && t.delete_message.contains("can't be undone"));
    assert!(register(&d).iter().any(|t| t.id == t0.id), "asked first");
    d.run(&mut rt, [SheetsMsg::ConfirmDelete]);
    assert!(rt.screen(&d).transaction.is_none());
    assert!(!register(&d).iter().any(|t| t.id == t0.id));
}

/// A category chosen for one waiting is saved and approves it.
#[test]
fn a_category_is_given_and_approved_with() {
    let (mut d, mut rt) = open();
    let t0 = register(&d).into_iter().find(|t| !t.approved && !t.transfer).expect("one waiting");
    d.run(
        &mut rt,
        [SheetsMsg::Categorize {
            ids: vec![t0.id.hex()],
            approve: true,
        }],
    );
    let c = rt.screen(&d).categorizing.unwrap();
    let pick = c.choices.iter().find(|o| o.key.starts_with('c')).unwrap().clone();
    d.run(&mut rt, [SheetsMsg::ChoseCategory { key: pick.key.clone() }, SheetsMsg::SaveCategory]);
    assert!(rt.screen(&d).categorizing.is_none());
    let t = register(&d).into_iter().find(|t| t.id == t0.id).unwrap();
    assert_eq!(format!("c{}", t.category_id.unwrap().hex()), pick.key);
    assert!(t.approved);
}

/// Yes locks what was cleared; No asks what the bank says, and the
/// difference is entered. Falsified by reconciling at the bank's figure on Yes.
#[test]
fn an_account_is_reconciled() {
    let (mut d, mut rt) = open();
    let a = d
        .rows::<AccountLine>(&MoneyView::Accounts)
        .iter()
        .find(|a| !a.closed && a.on_budget)
        .unwrap()
        .clone();
    d.run(&mut rt, [SheetsMsg::Reconcile { account: a.id.hex() }]);
    let r = rt.screen(&d).reconciling.unwrap();
    assert!(r.asking && r.question.contains(&a.name));
    d.run(
        &mut rt,
        [
            SheetsMsg::Differs,
            SheetsMsg::ReconcileCents {
                cents: a.cleared.abs() + 500,
            },
        ],
    );
    let r = rt.screen(&d).reconciling.unwrap();
    assert!(r.differs && r.problem.is_none());
    d.run(&mut rt, [SheetsMsg::SaveReconcile]);
    assert!(rt.screen(&d).reconciling.is_none());
    let after = d.rows::<AccountLine>(&MoneyView::Accounts).iter().find(|x| x.id == a.id).unwrap().clone();
    let owes = a.account_kind().is_some_and(|k| k.owes());
    let said = a.cleared.abs() + 500;
    assert_eq!(after.cleared, if owes { -said } else { said }, "cleared is what the bank says");
}
