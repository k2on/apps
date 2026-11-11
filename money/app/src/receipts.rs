//! A transaction's receipt, being given: a paper receipt scanned (the
//! scanner module, `ark_app::modules::scanner`: the page found, held still
//! and made flat), then kept — the photo put on the device and sent to the
//! server on this connection or the next (`MoneyRequest::KeepPhoto`), which
//! reads it (money/server's `read_receipt`) and files what it read under
//! the transaction for review. What the server read comes back as the
//! domain's rows (`MoneyView::Receipts`, `Photos`), which Transaction
//! Details shows.
//!
//! [`State`] keeps which transaction a scan is for, while one is under way,
//! and what to say of the last one.

use ark_app::modules::haptics::{perform, Haptic};
use ark_app::modules::scanner::scan_page;
use ark_app::{Answer, App, Command, Data};
use money_core::{MoneyChange, MoneyView};

use crate::routes::Route;
use crate::MoneyRequest;

#[cfg(test)]
mod tests;

/// The screen, as `ark_app` runs it.
pub struct Receipts;

/// What a scan is in the middle of.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// A scan is under way: the camera's page is up.
    pub scanning: bool,
    /// The transaction it is for (`Id::hex`); none for a receipt of no
    /// transaction yet, which the server matches.
    pub txn: Option<String>,
    /// What went wrong with the last, said once.
    pub problem: Option<String>,
}

/// Everything a receipt's giving can be told.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub enum ReceiptsMsg {
    /// Scan a paper receipt for the transaction of this hex id (empty:
    /// for none).
    Scan { txn: String },
    /// What the scanner gave: the flat page's JPEG, none when backed out,
    /// or why it failed.
    Scanned { page: Option<Vec<u8>>, failed: Option<String> },
    /// What was said put away.
    Dismiss,
}

/// What the receipt's giving shows.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct ReceiptsPage {
    pub scanning: bool,
    pub problem: Option<String>,
}

fn scanned(a: Answer<Option<Vec<u8>>>) -> ReceiptsMsg {
    match a {
        Ok(page) => ReceiptsMsg::Scanned { page, failed: None },
        Err(e) => ReceiptsMsg::Scanned {
            page: None,
            failed: Some(e.to_string()),
        },
    }
}

impl App for Receipts {
    type Key = MoneyView;
    type State = State;
    type Msg = ReceiptsMsg;
    type Change = MoneyChange;
    type Route = Route;
    type Request = MoneyRequest;
    type Screen = ReceiptsPage;

    fn holds(_: &State) -> Vec<MoneyView> {
        vec![]
    }

    fn update(s: &mut State, msg: ReceiptsMsg, _: &Data<'_, MoneyView>) -> Vec<Command<Self>> {
        match msg {
            ReceiptsMsg::Scan { txn } if !s.scanning => {
                s.scanning = true;
                s.problem = None;
                s.txn = (!txn.is_empty()).then_some(txn);
                vec![Command::ask_then(scan_page(), scanned)]
            }
            ReceiptsMsg::Scan { .. } => vec![],
            ReceiptsMsg::Scanned { page, failed } => {
                s.scanning = false;
                let txn = s.txn.take();
                match (page, failed) {
                    (Some(jpeg), _) => vec![
                        Command::Request(MoneyRequest::KeepPhoto { jpeg, txn }),
                        Command::ask(perform(Haptic::Success)),
                    ],
                    (None, Some(why)) => {
                        s.problem = Some(crate::words::scan_failed(&why));
                        vec![]
                    }
                    (None, None) => vec![],
                }
            }
            ReceiptsMsg::Dismiss => {
                s.problem = None;
                vec![]
            }
        }
    }

    fn screen(s: &State, _: &Data<'_, MoneyView>) -> ReceiptsPage {
        ReceiptsPage {
            scanning: s.scanning,
            problem: s.problem.clone(),
        }
    }
}
