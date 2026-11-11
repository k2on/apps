//! A receipt scanned and kept, over the demo household with the scanner's
//! fake: the scan asked, the page kept for its transaction and the photo
//! waiting on the server to read it; backed out is nothing; a failure said
//! once; a second scan not asked while one is up.

use super::*;
use crate::testing::Demo;
use ark_app::Runtime;
use money_core::{PhotoLine, TxnLine};

/// A transaction of the demo's.
fn a_txn(d: &mut Demo<Receipts>) -> String {
    d.hold(vec![MoneyView::Register { account: None }]);
    let t: &[TxnLine] = d.rows(&MoneyView::Register { account: None });
    t[0].id.hex()
}

/// Falsified by keeping the photo before the scanner answers, or for no
/// transaction: the photo is not the transaction's.
#[test]
fn a_scanned_page_is_kept_for_its_transaction_and_waits_to_be_read() {
    let mut d = Demo::<Receipts>::new();
    let mut rt = Runtime::<Receipts>::new();
    let txn = a_txn(&mut d);
    d.run(&mut rt, [ReceiptsMsg::Scan { txn: txn.clone() }]);
    assert_eq!(d.scanner.waiting(), 1, "the scanner was asked");
    assert!(rt.screen(&d).scanning);
    assert!(!rt.is_idle(&d.store), "a scan's answer is awaited");
    d.run(&mut rt, [ReceiptsMsg::Scan { txn: txn.clone() }]);
    assert_eq!(d.scanner.waiting(), 1, "not asked again while it is up");
    d.scanner.scanned(Some(vec![0xff, 0xd8, 0xff, 0xd9]));
    d.drain(&mut rt);
    assert!(matches!(&d.requests[..], [MoneyRequest::KeepPhoto { txn: Some(t), .. }] if *t == txn));
    assert_eq!(d.haptics.performed(), vec![Haptic::Success]);
    assert!(!rt.screen(&d).scanning);
    d.hold(vec![MoneyView::Photos]);
    let photos: &[PhotoLine] = d.rows(&MoneyView::Photos);
    let kept = photos
        .iter()
        .find(|p| p.txn_id.map(|i| i.hex()) == Some(txn.clone()))
        .expect("the photo is the transaction's");
    assert_eq!(kept.state, "reading", "it waits on the server's read_receipt");
}

#[test]
fn backed_out_is_nothing_and_a_failure_is_said() {
    let mut d = Demo::<Receipts>::new();
    let mut rt = Runtime::<Receipts>::new();
    d.run(&mut rt, [ReceiptsMsg::Scan { txn: String::new() }]);
    d.scanner.scanned(None);
    d.drain(&mut rt);
    assert!(d.requests.is_empty() && rt.screen(&d).problem.is_none());
    d.run(&mut rt, [ReceiptsMsg::Scan { txn: String::new() }]);
    d.scanner.failed("The camera is in use");
    d.drain(&mut rt);
    assert_eq!(rt.screen(&d).problem.as_deref(), Some("Couldn't scan the receipt: The camera is in use."));
    d.run(&mut rt, [ReceiptsMsg::Dismiss]);
    assert_eq!(rt.screen(&d).problem, None);
    assert!(rt.is_idle(&d.store));
}

/// Where there is no scanner (a platform without one), the scan is said
/// to be unavailable rather than waited on for ever.
#[test]
fn no_scanner_is_said() {
    let mut d = Demo::<Receipts>::new();
    d.modules = ark_app::Modules::new();
    let mut rt = Runtime::<Receipts>::new();
    d.run(&mut rt, [ReceiptsMsg::Scan { txn: String::new() }]);
    assert_eq!(rt.screen(&d).problem.as_deref(), Some("Couldn't scan the receipt: not available here."));
    assert!(rt.is_idle(&d.store));
}
