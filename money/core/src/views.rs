//! Every view an app of money opens, named once (`ark::views`): the
//! window and the phones hold these keys, and what a page is made of is
//! said here, so that every one of them opens the same views for it — the
//! budget for a month is its lines, its summary, its spending and its
//! formulas wherever it is drawn.

use ark::format::Encode;
use ark::views::{Opening, ViewKey};

use crate::schema::{Account, Category, Txn};
use crate::types::*;
use crate::Id;

/// A view of money's, or a page made of several.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MoneyView {
    /// The accounts, each with its balances (`AccountLine`).
    Accounts,
    /// The budget's groups (`GroupLine`).
    Groups,
    /// The budget for a month, `2026-10`: its lines, summary, spending and
    /// formulas.
    Budget { month: String },
    /// A month's line for each category (`BudgetLine`).
    Lines { month: String },
    /// What a month adds up to: one row (`Summary`).
    Summary { month: String },
    /// What a month spent, by category (`SpendingLine`).
    Spending { month: String },
    /// The month's formulas, one per category that follows one
    /// (`FormulaLine`).
    Formulas { month: String },
    /// What needs a look (`TxnLine`).
    Inbox,
    /// One account's transactions, or every account's (`TxnLine`).
    Register { account: Option<Id<Account>> },
    /// The payees a form offers (`PayeeLine`).
    Payees,
    /// The banks connected (`BankLine`).
    Banks,
    /// Each bank's accounts and the one it feeds (`LinkedLine`).
    Linked,
    /// This person's asks of the server's importer (`AskLine`).
    Asks,
    /// Every receipt photo, and where reading it stands (`PhotoLine`).
    Photos,
    /// Every receipt (`ReceiptLine`).
    Receipts,
    /// The receipts that paid for a transaction (`ReceiptLine`).
    Paid { txn: Id<Txn> },
    /// A receipt's items (`ItemLine`).
    Items { receipt: String },
    /// A receipt's charges, when the store said how it charged it
    /// (`ChargeLine`).
    Charges { receipt: String },
    /// Every item not filed under a category yet (`ItemLine`).
    Unfiled,
    /// Every category, the whole tree (`CategoryNode`).
    Tree,
    /// The saved filters, in order, with their totals (`FilterLine`).
    Saved,
    /// The register for a filter (`Filtered`'s bytes): its transactions
    /// and what they add up to.
    Ledger { filtered: Vec<u8> },
    /// A filter's transactions (`TxnLine`).
    Filtered { filtered: Vec<u8> },
    /// What a filter's transactions add up to: one row (`FilterTotal`).
    FilteredTotal { filtered: Vec<u8> },
    /// What could have paid a receipt or a charge (`PaidFor`'s bytes;
    /// `CandidateLine`).
    Candidates { paid_for: Vec<u8> },
    /// One transaction's split lines (`SplitLine`).
    Splits { txn: Id<Txn> },
    /// What a formula not yet saved would assign (`PreviewFormula`'s
    /// bytes; `FormulaPreview`).
    Preview { formula: Vec<u8> },
}

impl MoneyView {
    /// The register for `filtered`, its transactions and its total.
    pub fn ledger(filtered: &Filtered) -> MoneyView {
        MoneyView::Ledger {
            filtered: filtered.to_bytes(),
        }
    }

    /// A ledger's transactions.
    pub fn filtered(filtered: &Filtered) -> MoneyView {
        MoneyView::Filtered {
            filtered: filtered.to_bytes(),
        }
    }

    /// A ledger's total.
    pub fn filtered_total(filtered: &Filtered) -> MoneyView {
        MoneyView::FilteredTotal {
            filtered: filtered.to_bytes(),
        }
    }

    pub fn candidates(of: &PaidFor) -> MoneyView {
        MoneyView::Candidates { paid_for: of.to_bytes() }
    }

    pub fn preview(formula: &PreviewFormula) -> MoneyView {
        MoneyView::Preview { formula: formula.to_bytes() }
    }

    /// Whether it is a page of a month (`Budget`, or one of its views),
    /// and which.
    pub fn month(&self) -> Option<&str> {
        match self {
            MoneyView::Budget { month }
            | MoneyView::Lines { month }
            | MoneyView::Summary { month }
            | MoneyView::Spending { month }
            | MoneyView::Formulas { month } => Some(month),
            _ => None,
        }
    }
}

fn month(m: &str) -> Vec<u8> {
    Month { month: m.into() }.to_bytes()
}

fn none() -> Vec<u8> {
    ().to_bytes()
}

impl ViewKey for MoneyView {
    fn parts(&self) -> Vec<MoneyView> {
        match self {
            MoneyView::Budget { month } => vec![
                MoneyView::Lines { month: month.clone() },
                MoneyView::Summary { month: month.clone() },
                MoneyView::Spending { month: month.clone() },
                MoneyView::Formulas { month: month.clone() },
            ],
            MoneyView::Ledger { filtered } => vec![
                MoneyView::Filtered { filtered: filtered.clone() },
                MoneyView::FilteredTotal { filtered: filtered.clone() },
            ],
            v => vec![v.clone()],
        }
    }

    fn open(&self) -> Option<Opening> {
        use MoneyView as V;
        Some(match self {
            V::Budget { .. } | V::Ledger { .. } => return None,
            V::Accounts => Opening::of::<AccountLine>("accounts", none()),
            V::Groups => Opening::of::<GroupLine>("groups", none()),
            V::Lines { month: m } => Opening::of::<BudgetLine>("month", month(m)),
            V::Summary { month: m } => Opening::of::<Summary>("summary", month(m)),
            V::Spending { month: m } => Opening::of::<SpendingLine>("spending", month(m)),
            V::Formulas { month: m } => Opening::of::<FormulaLine>("formulas", month(m)),
            V::Inbox => Opening::of::<TxnLine>("inbox", none()),
            V::Register { account } => Opening::of::<TxnLine>("register", Register { account_id: *account }.to_bytes()),
            V::Payees => Opening::of::<PayeeLine>("payees", none()),
            V::Banks => Opening::of::<BankLine>("banks", none()),
            V::Linked => Opening::of::<LinkedLine>("linked", none()),
            V::Asks => Opening::of::<AskLine>("asks", none()),
            V::Photos => Opening::of::<PhotoLine>("photos", none()),
            V::Receipts => Opening::of::<ReceiptLine>("receipts", none()),
            V::Paid { txn } => Opening::of::<ReceiptLine>("for_txn", OneTxn { id: *txn }.to_bytes()),
            V::Items { receipt } => Opening::of::<ItemLine>("receipt_items", OneReceipt { receipt_id: receipt.clone() }.to_bytes()),
            V::Charges { receipt } => Opening::of::<ChargeLine>("charges", OneReceipt { receipt_id: receipt.clone() }.to_bytes()),
            V::Unfiled => Opening::of::<ItemLine>(
                "items",
                ItemsIn {
                    category_id: None::<Id<Category>>,
                }
                .to_bytes(),
            ),
            V::Tree => Opening::of::<CategoryNode>("tree", none()),
            V::Saved => Opening::of::<FilterLine>("saved", none()),
            V::Filtered { filtered } => Opening::of::<TxnLine>("filtered", filtered.clone()),
            V::FilteredTotal { filtered } => Opening::of::<FilterTotal>("filtered_total", filtered.clone()),
            V::Candidates { paid_for } => Opening::of::<CandidateLine>("candidates", paid_for.clone()),
            V::Splits { txn } => Opening::of::<SplitLine>("splits", OneTxn { id: *txn }.to_bytes()),
            V::Preview { formula } => Opening::of::<FormulaPreview>("preview_formula", formula.clone()),
        })
    }
}
