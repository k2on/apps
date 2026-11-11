//! A transaction's receipt in its form, over the demo household with the
//! scanner's fake: a page scanned or a photo chosen is kept for the
//! transaction and waits to be read, its photo shown whole; backed out is
//! nothing and a failure is said; a receipt typed pays for it and is drawn
//! as its paper, put right without being doubled, taken off and matched
//! again from the receipts nobody has, the nearest first.

use super::*;
use crate::sheets::{Sheets, SheetsMsg};
use crate::testing::Demo;
use ark_app::Runtime;

/// The form up over a spending transaction with no receipt.
fn open() -> (Demo<Sheets>, Runtime<Sheets>, TxnLine) {
    let mut d = Demo::<Sheets>::new();
    let mut rt = Runtime::<Sheets>::new();
    d.run(
        &mut rt,
        [SheetsMsg::Month {
            month: "2026-10".into(),
            today: "2026-10-26".into(),
        }],
    );
    let t = d
        .rows::<TxnLine>(&MoneyView::Register { account: None })
        .iter()
        .find(|t| t.amount < -2_000 && !t.transfer && t.receipt.is_empty() && t.photos == 0)
        .cloned()
        .expect("a spending transaction with no receipt");
    d.run(&mut rt, [SheetsMsg::Open { key: t.id.hex() }]);
    (d, rt, t)
}

fn section(d: &Demo<Sheets>, rt: &Runtime<Sheets>) -> ReceiptSection {
    rt.screen(d).transaction.expect("the form is up").receipt.expect("its receipt's section")
}

fn typed(d: &mut Demo<Sheets>, rt: &mut Runtime<Sheets>, items: &[(&str, &str)]) {
    d.run(rt, [SheetsMsg::TypeReceipt]);
    for (n, (name, amount)) in items.iter().enumerate() {
        if n > 0 {
            d.run(rt, [SheetsMsg::AddItem]);
        }
        d.run(
            rt,
            [
                SheetsMsg::ItemName { name: name.to_string() },
                SheetsMsg::ItemAmount { typed: amount.to_string() },
            ],
        );
    }
}

/// With none, the ways to give it one; scanned, the page is kept for the
/// transaction and waits on the server's read, shown as being read; its
/// photo opened is fetched and shown by its URL. Falsified by keeping the
/// photo for no transaction: nothing reads as this one's.
#[test]
fn a_scanned_page_is_kept_and_shown_while_it_is_read() {
    let (mut d, mut rt, t) = open();
    let s = section(&d, &rt);
    assert!(s.none && s.can_add && !s.has_paper);
    d.run(&mut rt, [SheetsMsg::ScanReceipt]);
    assert_eq!(d.scanner.waiting(), 1, "the scanner was asked");
    d.run(&mut rt, [SheetsMsg::ChoosePhoto]);
    assert_eq!(d.scanner.waiting(), 1, "not asked again while it is up");
    d.scanner.scanned(Some(vec![0xff, 0xd8, 0xff, 0xd9]));
    d.drain(&mut rt);
    assert!(matches!(&d.requests[..], [MoneyRequest::KeepPhoto { txn: Some(x), .. }] if *x == t.id.hex()));
    assert_eq!(d.haptics.performed(), vec![Haptic::Success]);
    let s = section(&d, &rt);
    assert!(!s.none, "it is being read");
    let reading = s.reading.first().expect("the photo waits to be read").clone();
    assert_eq!(reading.note, "Your server reads it once the photo reaches it.");
    d.run(&mut rt, [SheetsMsg::ShowPhoto { hash: reading.hash.clone() }]);
    assert_eq!(d.wanted.len(), 1, "asked for, in case the phone lacks it");
    let shown = section(&d, &rt).viewing.expect("the photo whole");
    assert!(shown.url.starts_with("data:image/jpeg;base64,"), "{}", shown.url);
    d.run(&mut rt, [SheetsMsg::ClosePhoto]);
    assert!(section(&d, &rt).viewing.is_none());
}

/// A photo chosen from the library goes the same way; backed out is
/// nothing; a failure is said until the next try. Where there is no
/// scanner at all, that is said, not waited on.
#[test]
fn a_photo_is_chosen_and_a_failure_said() {
    let (mut d, mut rt, _) = open();
    d.run(&mut rt, [SheetsMsg::ChoosePhoto]);
    d.scanner.scanned(None);
    d.drain(&mut rt);
    assert!(d.requests.is_empty() && !section(&d, &rt).has_problem);
    d.run(&mut rt, [SheetsMsg::ChoosePhoto]);
    d.scanner.failed("The photo library is busy");
    d.drain(&mut rt);
    assert_eq!(section(&d, &rt).problem, "Couldn't scan the receipt: The photo library is busy.");
    d.run(&mut rt, [SheetsMsg::ChoosePhoto]);
    assert!(!section(&d, &rt).has_problem, "put away by the next try");
    d.scanner.scanned(Some(vec![0xff, 0xd8]));
    d.drain(&mut rt);
    assert!(matches!(&d.requests[..], [MoneyRequest::KeepPhoto { .. }]));
    let (mut d, mut rt, _) = open();
    d.modules = ark_app::Modules::new();
    d.run(&mut rt, [SheetsMsg::ScanReceipt]);
    assert_eq!(section(&d, &rt).problem, "Couldn't scan the receipt: not available here.");
    assert!(rt.is_idle(&d.store));
}

/// Typed, a receipt pays for the transaction and is drawn as its paper,
/// a purchase without a sign and a discount below zero; edited, it is
/// put right, not doubled; taken off, it is offered to match again, and
/// matched it pays again. Falsified by writing the amounts unsigned: the
/// receipt would come to the opposite of what was paid.
#[test]
fn a_receipt_typed_pays_for_it_and_is_put_right_taken_off_and_matched() {
    let (mut d, mut rt, t) = open();
    let us = NumberStyle::us();
    let rest = amount_editable(-t.amount - 100 + 50, &us);
    typed(&mut d, &mut rt, &[("Eggs", &rest), ("Tax", "1.00"), ("Coupon", "-0.50")]);
    let f = section(&d, &rt).form.expect("the receipt form");
    assert!(f.adding && f.can_save, "{}", f.note);
    assert_eq!(f.total, amount_editable(-t.amount, &us));
    d.run(&mut rt, [SheetsMsg::SaveReceipt]);
    let s = section(&d, &rt);
    assert!(s.form.is_none(), "put away once taken");
    assert!(s.has_paper && !s.can_add, "drawn as its paper");
    assert_eq!(s.names, ["Eggs", "Tax", "Coupon"]);
    assert_eq!(s.figures[2], "-0.50", "a discount below zero");
    assert_eq!(s.total, amount_editable(-t.amount, &us));
    let receipts = d.rows::<ReceiptLine>(&MoneyView::Receipts).len();
    d.run(&mut rt, [SheetsMsg::EditReceipt]);
    let f = section(&d, &rt).form.expect("the form over it");
    assert!(!f.adding && f.items.len() == 3);
    d.run(
        &mut rt,
        [
            SheetsMsg::PickItem { key: "0".into() },
            SheetsMsg::ItemName { name: "Brown Eggs".into() },
            SheetsMsg::SaveReceipt,
        ],
    );
    assert_eq!(section(&d, &rt).names[0], "Brown Eggs");
    assert_eq!(d.rows::<ReceiptLine>(&MoneyView::Receipts).len(), receipts, "put right, not doubled");
    d.run(&mut rt, [SheetsMsg::TakeReceiptOff]);
    let s = section(&d, &rt);
    assert!(s.can_add && s.can_match, "taken off, it waits to be matched");
    d.run(&mut rt, [SheetsMsg::MatchReceipt]);
    let m = section(&d, &rt).matching.expect("the receipts nobody has");
    assert_eq!(m.receipts[0].cents, t.amount, "the nearest first");
    d.run(
        &mut rt,
        [SheetsMsg::Matched {
            id: m.receipts[0].id.clone(),
        }],
    );
    let s = section(&d, &rt);
    assert!(s.matching.is_none() && s.has_paper, "matched, it pays again");
}

/// The form keeps what cannot be saved from being saved, and says why.
#[test]
fn a_receipt_form_says_what_is_missing() {
    let style = NumberStyle::us();
    let mut f = Draft::new(None, "Corner Shop", "2026-10-26");
    assert_eq!(f.problem(&style).as_deref(), Some("Add an item."));
    f.items[0].amount = "4.29".into();
    assert_eq!(f.problem(&style).as_deref(), Some("Every item needs a name."));
    f.items[0].name = "EGGS".into();
    f.items.push(Item {
        name: "TAX".into(),
        amount: "lots".into(),
        ..Item::blank()
    });
    assert_eq!(f.problem(&style).as_deref(), Some("An amount isn't one."));
    f.items[1].amount = "$0.31".into();
    assert_eq!(f.problem(&style), None);
    assert_eq!(f.total(&style), 460);
    let w = f.write(&style);
    assert_eq!(w.items.iter().map(|i| i.amount).collect::<Vec<_>>(), [-429, -31], "spent is below zero");
    assert_eq!(w.total, None, "the items' sum");
}
