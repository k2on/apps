//! The replica, and the lists a window draws from it — every one a view
//! the peer keeps up to date (§1.10), patched by the rows a change touched.
//!
//! The sidebar's accounts and saved filters, the month's lines, its
//! summary and its formulas, the register on screen and what it adds up
//! to, what needs looking at, the payees a form offers, the banks, their
//! accounts and this person's asks about them live as long as the window;
//! the month's and the register's are opened again when the window moves
//! to another month or filter.

use ark_peer::Watched;
use money_domain::*;

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
    pub accounts: Watched<AccountLine>,
    pub groups: Watched<GroupLine>,
    pub lines: Watched<BudgetLine>,
    pub summary: Watched<Summary>,
    pub spending: Watched<SpendingLine>,
    pub ledger: Ledger,
    /// The conditions of a register not saved, while one is on screen.
    pub adhoc: Filter,
    /// What a register not saved was changed from: the saved filter or
    /// the account it started as.
    pub base: Ledger,
    pub register: Watched<TxnLine>,
    /// What the register on screen adds up to: one row, none for nothing.
    pub total: Watched<FilterTotal>,
    pub inbox: Watched<TxnLine>,
    pub payees: Watched<PayeeLine>,
    pub banks: Watched<BankLine>,
    /// Every category, the whole tree: the envelopes' kinds too.
    pub tree: Watched<CategoryNode>,
    pub receipts: Watched<ReceiptLine>,
    /// Every receipt photo, and where reading each stands.
    pub photos: Watched<PhotoLine>,
    /// The transaction open in the form, and the receipts that paid for it.
    pub txn: Option<Watched<ReceiptLine>>,
    /// Every item not filed under a category yet.
    pub unfiled: Watched<ItemLine>,
    /// The receipt open on the receipts page, and its items.
    pub receipt: Option<Watched<ItemLine>>,
    /// Its charges, when the store said how it charged it.
    pub charges: Option<Watched<ChargeLine>>,
    /// What could have paid a receipt or a charge, while somebody says.
    pub candidates: Option<Watched<CandidateLine>>,
    /// The split transaction open in the form, and its lines.
    pub splits: Option<Watched<SplitLine>>,
    pub linked: Watched<LinkedLine>,
    pub asks: Watched<AskLine>,
    /// The saved filters, in order, with their totals.
    pub filters: Watched<FilterLine>,
    /// The month's formulas, one per category that follows one.
    pub formulas: Watched<FormulaLine>,
    /// What the formula being typed would assign, while it is one.
    pub preview: Option<Watched<FormulaPreview>>,
}

fn month_of(month: &str) -> Month {
    Month { month: month.into() }
}

/// What the register for `ledger` asks the domain.
pub fn filtered(ledger: Ledger, adhoc: &Filter) -> Filtered {
    let (saved, filter) = match ledger {
        Ledger::All => (None, Filter::all()),
        Ledger::Inbox => (None, Filter::needs_a_look()),
        Ledger::Account(a) => (None, Filter::account(a)),
        Ledger::Saved(id) => (Some(id), Filter::all()),
        Ledger::Adhoc => (None, adhoc.clone()),
    };
    Filtered { saved, filter, month: None }
}

impl Peer {
    /// Every list the window starts on, hydrated once.
    pub fn open(mut client: Replica, month: &str) -> Peer {
        let c = &mut client;
        let all = filtered(Ledger::All, &Filter::all());
        let m = &month_of(month);
        Peer {
            accounts: Watched::open(c, accounts::ACCOUNTS.on(&())),
            groups: Watched::open(c, budget::GROUPS.on(&())),
            lines: Watched::open(c, budget::MONTH.on(m)),
            summary: Watched::open(c, budget::SUMMARY.on(m)),
            spending: Watched::open(c, budget::SPENDING.on(m)),
            register: Watched::open(c, filters::FILTERED.on(&all)),
            total: Watched::open(c, filters::FILTERED_TOTAL.on(&all)),
            inbox: Watched::open(c, filters::FILTERED.on(&filtered(Ledger::Inbox, &Filter::all()))),
            payees: Watched::open(c, transactions::PAYEES.on(&())),
            banks: Watched::open(c, plaid::BANKS.on(&())),
            tree: Watched::open(c, categories::TREE.on(&())),
            receipts: Watched::open(c, receipts::RECEIPTS.on(&())),
            photos: Watched::open(c, receipts::PHOTOS.on(&())),
            txn: None,
            unfiled: Watched::open(c, receipts::ITEMS.on(&ItemsIn { category_id: None })),
            receipt: None,
            charges: None,
            candidates: None,
            splits: None,
            linked: Watched::open(c, plaid::LINKED.on(&())),
            asks: Watched::open(c, banks::ASKS.on(&())),
            filters: Watched::open(c, filters::SAVED.on(&())),
            formulas: Watched::open(c, formulas::FORMULAS.on(m)),
            preview: None,
            ledger: Ledger::All,
            adhoc: Filter::all(),
            base: Ledger::All,
            month: month.into(),
            client,
        }
    }

    /// Every view brought up to what the peer holds: what each one did,
    /// spliced into its rows. Whether anything moved.
    pub fn refresh(&mut self) -> bool {
        let c = &mut self.client;
        let mut any = false;
        let mut note = |u: Vec<ark_peer::Patch>| any |= !u.is_empty();
        note(self.accounts.settle(c));
        note(self.groups.settle(c));
        note(self.lines.settle(c));
        note(self.summary.settle(c));
        note(self.spending.settle(c));
        note(self.register.settle(c));
        note(self.total.settle(c));
        note(self.inbox.settle(c));
        note(self.payees.settle(c));
        note(self.banks.settle(c));
        note(self.tree.settle(c));
        note(self.receipts.settle(c));
        note(self.photos.settle(c));
        note(self.unfiled.settle(c));
        note(self.linked.settle(c));
        note(self.asks.settle(c));
        note(self.filters.settle(c));
        note(self.formulas.settle(c));
        if let Some(w) = &mut self.txn {
            note(w.settle(c));
        }
        if let Some(w) = &mut self.receipt {
            note(w.settle(c));
        }
        if let Some(w) = &mut self.charges {
            note(w.settle(c));
        }
        if let Some(w) = &mut self.candidates {
            note(w.settle(c));
        }
        if let Some(w) = &mut self.splits {
            note(w.settle(c));
        }
        if let Some(w) = &mut self.preview {
            note(w.settle(c));
        }
        // A receipt read or attached: its items on screen.
        if any {
            self.follow_txn();
        }
        any
    }

    /// The window on another month: its lines, summary, spending and
    /// formulas.
    pub fn set_month(&mut self, month: &str) {
        let (c, m) = (&mut self.client, &month_of(month));
        self.lines.set(c, budget::MONTH.on(m));
        self.summary.set(c, budget::SUMMARY.on(m));
        self.spending.set(c, budget::SPENDING.on(m));
        self.formulas.set(c, formulas::FORMULAS.on(m));
        self.month = month.into();
    }

    /// The register on screen: the one for `ledger`, `adhoc` its
    /// conditions when it is not saved.
    pub fn set_ledger(&mut self, ledger: Ledger, adhoc: Filter) {
        let input = filtered(ledger, &adhoc);
        self.register.set(&mut self.client, filters::FILTERED.on(&input));
        self.total.set(&mut self.client, filters::FILTERED_TOTAL.on(&input));
        (self.ledger, self.adhoc) = (ledger, adhoc);
    }

    /// What a formula not yet saved would assign, watched: none closes it.
    pub fn set_preview(&mut self, formula: Option<PreviewFormula>) {
        Watched::want(&mut self.preview, &mut self.client, formula.map(|f| formulas::PREVIEW_FORMULA.on(&f)));
    }

    /// The formula a category's month follows, if it follows one.
    pub fn formula(&self, id: Id<Category>) -> Option<&FormulaLine> {
        self.formulas.rows.iter().find(|r| r.category_id == id)
    }

    /// A saved filter, by its id.
    pub fn saved(&self, id: Id<SavedFilter>) -> Option<&FilterLine> {
        self.filters.rows.iter().find(|f| f.id == id)
    }

    /// What the register on screen adds up to.
    pub fn total(&self) -> FilterTotal {
        self.total.rows.first().cloned().unwrap_or_default()
    }

    /// The receipts of the transaction open in the form, and its photos
    /// wanted from the server: the first receipt's items on screen. Whether
    /// it is another.
    pub fn set_txn(&mut self, id: Option<Id<Txn>>) -> bool {
        let on = id.map(|id| receipts::FOR_TXN.on(&OneTxn { id }));
        let changed = Watched::want(&mut self.txn, &mut self.client, on);
        self.follow_txn();
        changed
    }

    /// The open transaction's first receipt's items, as its receipts move.
    fn follow_txn(&mut self) {
        let first = self.txn_receipts().first().map(|r| r.id.clone());
        self.set_receipt(first.as_deref());
    }

    /// The receipts of the transaction open in the form.
    pub fn txn_receipts(&self) -> &[ReceiptLine] {
        self.txn.as_ref().map_or(&[], |w| &w.rows[..])
    }

    /// The items of the receipt on screen.
    pub fn receipt_items(&self) -> &[ItemLine] {
        self.receipt.as_ref().map_or(&[], |w| &w.rows[..])
    }

    /// One receipt's items on screen, and its charges, or none.
    pub fn set_receipt(&mut self, id: Option<&str>) {
        let input = id.map(|id| OneReceipt { receipt_id: id.into() });
        Watched::want(&mut self.receipt, &mut self.client, input.as_ref().map(|i| receipts::RECEIPT_ITEMS.on(i)));
        Watched::want(&mut self.charges, &mut self.client, input.as_ref().map(|i| receipts::CHARGES.on(i)));
    }

    /// What could have paid for `of`, or nothing open.
    pub fn set_candidates(&mut self, of: Option<PaidFor>) {
        Watched::want(&mut self.candidates, &mut self.client, of.map(|p| receipts::CANDIDATES.on(&p)));
    }

    /// One transaction's split lines, or none.
    pub fn set_splits(&mut self, id: Option<Id<Txn>>) {
        let on = id.map(|id| transactions::SPLITS.on(&OneTxn { id }));
        Watched::want(&mut self.splits, &mut self.client, on);
    }

    /// A category of the tree.
    pub fn node(&self, id: Id<Category>) -> Option<&CategoryNode> {
        self.tree.rows.iter().find(|n| n.id == id)
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
        self.summary.rows.first().cloned().unwrap_or_else(|| Summary {
            month: self.month.clone(),
            ..Summary::default()
        })
    }

    pub fn account(&self, id: Id<Account>) -> Option<&AccountLine> {
        self.accounts.rows.iter().find(|a| a.id == id)
    }

    pub fn line(&self, id: Id<Category>) -> Option<&BudgetLine> {
        self.lines.rows.iter().find(|l| l.category_id == id)
    }

    /// The category a payee's last transaction was for.
    pub fn remembered(&self, payee: &str) -> Option<Id<Category>> {
        self.payees
            .rows
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(payee))
            .and_then(|p| p.category_id)
    }
}
