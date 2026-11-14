//! The replica, and the lists a window draws from it — every one a view
//! the peer keeps up to date (§1.10), patched by the rows a change touched,
//! held in the store every app of money holds its views in
//! (`ark_peer::store`, keyed by `money_core::MoneyView`).
//!
//! The sidebar's accounts and saved filters, what needs looking at, the
//! payees a form offers, the banks, their accounts and this person's asks
//! about them live as long as the window; the month's budget (its lines,
//! summary, spending and formulas, as the domain says a budget is) and the
//! register on screen (its rows and what they add up to) are held again
//! when the window moves to another month or filter.

use ark_peer::store::Store;
use money_core::*;

use crate::client::Replica;

/// Which register is on screen: the register for a filter. Every one of
/// these is a [`Filter`] — the built-in ones every account's, what needs a
/// look, one account's; a saved one; or conditions not saved (`Adhoc`,
/// held in [`Peer::adhoc`]) — and every one is read by the domain's one
/// matcher (`transactions.filtered`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ledger {
    All,
    Inbox,
    Account(Id<Account>),
    Saved(Id<SavedFilter>),
    Adhoc,
}

impl Ledger {
    /// The built-in register these conditions are exactly, if any: a
    /// filter changed back to every account's, or to one account's, is
    /// that register again.
    pub fn built_in(f: &Filter) -> Option<Ledger> {
        match () {
            _ if *f == Filter::all() => Some(Ledger::All),
            _ if *f == Filter::needs_a_look() => Some(Ledger::Inbox),
            _ => f.only_account().map(Ledger::Account),
        }
    }
}

pub struct Peer {
    pub client: Replica,
    pub month: String,
    pub ledger: Ledger,
    /// The conditions of a register not saved, while one is on screen.
    pub adhoc: Filter,
    /// What a register not saved was changed from: the saved filter or
    /// the account it started as.
    pub base: Ledger,
    /// Every view the window holds.
    pub store: Store<MoneyView>,
    /// The register on screen, as its key.
    register: MoneyView,
    /// The transaction open in the form, whose receipts are held.
    txn: Option<Id<Txn>>,
    /// The receipt open on the receipts page, whose items and charges are
    /// held.
    receipt: Option<String>,
    /// What could have paid a receipt or a charge, while somebody says.
    candidates: Option<PaidFor>,
    /// The split transaction open in the form, whose lines are held.
    splits: Option<Id<Txn>>,
    /// What the formula being typed would assign, while it is one.
    preview: Option<MoneyView>,
}

/// The views the window holds as long as it is open.
const ALWAYS: [MoneyView; 12] = [
    MoneyView::Accounts,
    MoneyView::Groups,
    MoneyView::Payees,
    MoneyView::Banks,
    MoneyView::Tree,
    MoneyView::Receipts,
    MoneyView::Photos,
    MoneyView::Unfiled,
    MoneyView::Linked,
    MoneyView::Asks,
    MoneyView::Saved,
    // Every transaction: the net worth's trend, and Insights' months.
    MoneyView::Register { account: None },
];

/// What the register for `ledger` asks the domain.
pub fn filtered(ledger: Ledger, adhoc: &Filter) -> Filtered {
    let (saved, filter) = match ledger {
        Ledger::All => (None, Filter::all()),
        Ledger::Inbox => (None, Filter::needs_a_look()),
        Ledger::Account(a) => (None, Filter::account(a)),
        Ledger::Saved(id) => (Some(id), Filter::all()),
        Ledger::Adhoc => (None, adhoc.clone()),
    };
    // The window's own conditions are its search box's: read wider, as a search is.
    let search = matches!(ledger, Ledger::Adhoc);
    Filtered {
        saved,
        filter,
        month: None,
        search,
    }
}

/// What needs a look, as the window's inbox reads it.
fn inbox() -> MoneyView {
    MoneyView::filtered(&filtered(Ledger::Inbox, &Filter::all()))
}

impl Peer {
    /// Every list the window starts on, hydrated once.
    pub fn open(mut client: Replica, month: &str) -> Peer {
        let mut store = Store::new();
        for v in ALWAYS {
            store.hold(&mut client, v);
        }
        store.hold(&mut client, inbox());
        let register = MoneyView::ledger(&filtered(Ledger::All, &Filter::all()));
        store.hold(&mut client, register.clone());
        store.hold(&mut client, MoneyView::Budget { month: month.into() });
        // What the hydrate read is not news.
        store.settle(&mut client);
        Peer {
            client,
            month: month.into(),
            ledger: Ledger::All,
            adhoc: Filter::all(),
            base: Ledger::All,
            store,
            register,
            txn: None,
            receipt: None,
            candidates: None,
            splits: None,
            preview: None,
        }
    }

    /// `new` held in place of `old`: held first, so what the two share is
    /// not opened again.
    fn swap(&mut self, old: Option<MoneyView>, new: Option<MoneyView>) {
        if old == new {
            return;
        }
        if let Some(n) = new {
            self.store.hold(&mut self.client, n);
        }
        if let Some(o) = old {
            self.store.release(&mut self.client, &o);
        }
    }

    /// Every view brought up to what the peer holds: what each one did,
    /// spliced into its rows. Whether anything moved.
    pub fn refresh(&mut self) -> bool {
        let any = !self.store.settle(&mut self.client).is_empty();
        // A receipt read or attached: its items on screen.
        if any {
            self.follow_txn();
        }
        any
    }

    /// The window on another month: its budget.
    pub fn set_month(&mut self, month: &str) {
        if month == self.month {
            return;
        }
        let old = MoneyView::Budget { month: self.month.clone() };
        self.swap(Some(old), Some(MoneyView::Budget { month: month.into() }));
        self.month = month.into();
    }

    /// The register on screen: the one for `ledger`, `adhoc` its
    /// conditions when it is not saved.
    pub fn set_ledger(&mut self, ledger: Ledger, adhoc: Filter) {
        let key = MoneyView::ledger(&filtered(ledger, &adhoc));
        let old = std::mem::replace(&mut self.register, key.clone());
        self.swap(Some(old), Some(key));
        (self.ledger, self.adhoc) = (ledger, adhoc);
    }

    /// What a formula not yet saved would assign, watched: none closes it.
    pub fn set_preview(&mut self, formula: Option<PreviewFormula>) {
        let key = formula.map(|f| MoneyView::preview(&f));
        let old = std::mem::replace(&mut self.preview, key.clone());
        self.swap(old, key);
    }

    /// What the formula being typed would assign, while there is one.
    pub fn preview(&self) -> Option<&FormulaPreview> {
        self.preview.as_ref().and_then(|k| self.store.first(k))
    }

    /// Whether a formula is being previewed.
    pub fn previewing(&self) -> bool {
        self.preview.is_some()
    }

    fn of_month(&self, f: fn(String) -> MoneyView) -> MoneyView {
        f(self.month.clone())
    }

    pub fn accounts(&self) -> &[AccountLine] {
        self.store.rows(&MoneyView::Accounts)
    }

    pub fn groups(&self) -> &[GroupLine] {
        self.store.rows(&MoneyView::Groups)
    }

    /// The month's line for each category.
    pub fn lines(&self) -> &[BudgetLine] {
        self.store.rows(&self.of_month(|month| MoneyView::Lines { month }))
    }

    /// What the month spent, by category.
    pub fn spending(&self) -> &[SpendingLine] {
        self.store.rows(&self.of_month(|month| MoneyView::Spending { month }))
    }

    /// The month's formulas, one per category that follows one.
    pub fn formulas(&self) -> &[FormulaLine] {
        self.store.rows(&self.of_month(|month| MoneyView::Formulas { month }))
    }

    /// The register on screen.
    pub fn register(&self) -> &[TxnLine] {
        let MoneyView::Ledger { filtered } = &self.register else { return &[] };
        self.store.rows(&MoneyView::Filtered { filtered: filtered.clone() })
    }

    /// What needs looking at.
    pub fn inbox(&self) -> &[TxnLine] {
        self.store.rows(&inbox())
    }

    /// The payees a form offers.
    pub fn payees(&self) -> &[PayeeLine] {
        self.store.rows(&MoneyView::Payees)
    }

    pub fn banks(&self) -> &[BankLine] {
        self.store.rows(&MoneyView::Banks)
    }

    /// Every category, the whole tree: the envelopes' kinds too.
    pub fn tree(&self) -> &[CategoryNode] {
        self.store.rows(&MoneyView::Tree)
    }

    pub fn receipts(&self) -> &[ReceiptLine] {
        self.store.rows(&MoneyView::Receipts)
    }

    /// Every receipt photo, and where reading each stands.
    pub fn photos(&self) -> &[PhotoLine] {
        self.store.rows(&MoneyView::Photos)
    }

    /// Every item not filed under a category yet.
    pub fn unfiled(&self) -> &[ItemLine] {
        self.store.rows(&MoneyView::Unfiled)
    }

    pub fn linked(&self) -> &[LinkedLine] {
        self.store.rows(&MoneyView::Linked)
    }

    pub fn asks(&self) -> &[AskLine] {
        self.store.rows(&MoneyView::Asks)
    }

    /// The saved filters, in order, with their totals.
    pub fn filters(&self) -> &[FilterLine] {
        self.store.rows(&MoneyView::Saved)
    }

    /// The formula a category's month follows, if it follows one.
    pub fn formula(&self, id: Id<Category>) -> Option<&FormulaLine> {
        self.formulas().iter().find(|r| r.category_id == id)
    }

    /// A saved filter, by its id.
    pub fn saved(&self, id: Id<SavedFilter>) -> Option<&FilterLine> {
        self.filters().iter().find(|f| f.id == id)
    }

    /// What the register on screen adds up to.
    pub fn total(&self) -> FilterTotal {
        let MoneyView::Ledger { filtered } = &self.register else {
            return FilterTotal::default();
        };
        let key = MoneyView::FilteredTotal { filtered: filtered.clone() };
        self.store.first::<FilterTotal>(&key).cloned().unwrap_or_default()
    }

    /// The transaction open in the form, while there is one.
    pub fn txn(&self) -> Option<Id<Txn>> {
        self.txn
    }

    /// The receipts of the transaction open in the form, and its photos
    /// wanted from the server: the first receipt's items on screen.
    pub fn set_txn(&mut self, id: Option<Id<Txn>>) {
        if self.txn == id {
            return;
        }
        let old = std::mem::replace(&mut self.txn, id);
        self.swap(old.map(|txn| MoneyView::Paid { txn }), id.map(|txn| MoneyView::Paid { txn }));
        self.follow_txn();
    }

    /// The open transaction's first receipt's items, as its receipts move.
    fn follow_txn(&mut self) {
        let first = self.txn_receipts().first().map(|r| r.id.clone());
        self.set_receipt(first.as_deref());
    }

    /// The receipts of the transaction open in the form.
    pub fn txn_receipts(&self) -> &[ReceiptLine] {
        self.txn.map_or(&[], |txn| self.store.rows(&MoneyView::Paid { txn }))
    }

    /// The receipt on screen, while there is one.
    pub fn receipt(&self) -> Option<&str> {
        self.receipt.as_deref()
    }

    /// The items of the receipt on screen.
    pub fn receipt_items(&self) -> &[ItemLine] {
        self.receipt
            .as_ref()
            .map_or(&[], |r| self.store.rows(&MoneyView::Items { receipt: r.clone() }))
    }

    /// The charges of the receipt on screen, while there is one.
    pub fn charges(&self) -> Option<&[ChargeLine]> {
        self.receipt.as_ref().map(|r| self.store.rows(&MoneyView::Charges { receipt: r.clone() }))
    }

    /// One receipt's items on screen, or none.
    pub fn set_receipt(&mut self, id: Option<&str>) {
        if self.receipt.as_deref() == id {
            return;
        }
        let old = std::mem::replace(&mut self.receipt, id.map(String::from));
        let items = |r: &Option<String>| r.clone().map(|receipt| MoneyView::Items { receipt });
        let charges = |r: &Option<String>| r.clone().map(|receipt| MoneyView::Charges { receipt });
        let new = self.receipt.clone();
        self.swap(items(&old), items(&new));
        self.swap(charges(&old), charges(&new));
    }

    /// What could have paid for what is being matched, while somebody says.
    pub fn candidates(&self) -> Option<&[CandidateLine]> {
        self.candidates.as_ref().map(|p| self.store.rows(&MoneyView::candidates(p)))
    }

    /// What could have paid for `of`, or nothing open.
    pub fn set_candidates(&mut self, of: Option<PaidFor>) {
        if self.candidates == of {
            return;
        }
        let old = std::mem::replace(&mut self.candidates, of.clone());
        self.swap(old.as_ref().map(MoneyView::candidates), of.as_ref().map(MoneyView::candidates));
    }

    /// The split transaction open in the form's lines, while there is one.
    pub fn splits(&self) -> Option<&[SplitLine]> {
        self.splits.map(|txn| self.store.rows(&MoneyView::Splits { txn }))
    }

    /// One transaction's split lines, or none.
    pub fn set_splits(&mut self, id: Option<Id<Txn>>) {
        if self.splits == id {
            return;
        }
        let old = std::mem::replace(&mut self.splits, id);
        self.swap(old.map(|txn| MoneyView::Splits { txn }), id.map(|txn| MoneyView::Splits { txn }));
    }

    /// A category of the tree.
    pub fn node(&self, id: Id<Category>) -> Option<&CategoryNode> {
        self.tree().iter().find(|n| n.id == id)
    }

    /// A category's name as a form shows it: an envelope's own, a kind's
    /// with the envelope it is a kind of.
    pub fn category_name(&self, id: Id<Category>) -> Option<String> {
        let n = self.node(id)?;
        Some(match n.depth {
            0 | 1 => n.name.clone(),
            _ => n.path.split(" \u{203a} ").skip(1).collect::<Vec<_>>().join(" \u{203a} "),
        })
    }

    pub fn summary(&self) -> Summary {
        let key = self.of_month(|month| MoneyView::Summary { month });
        self.store.first::<Summary>(&key).cloned().unwrap_or_else(|| Summary {
            month: self.month.clone(),
            ..Summary::default()
        })
    }

    pub fn account(&self, id: Id<Account>) -> Option<&AccountLine> {
        self.accounts().iter().find(|a| a.id == id)
    }

    pub fn line(&self, id: Id<Category>) -> Option<&BudgetLine> {
        self.lines().iter().find(|l| l.category_id == id)
    }

    /// The category a payee's last transaction was for.
    pub fn remembered(&self, payee: &str) -> Option<Id<Category>> {
        self.payees()
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(payee))
            .and_then(|p| p.category_id)
    }
}
