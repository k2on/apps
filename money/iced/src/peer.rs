//! The replica, and the lists a window draws from it — every one a view
//! the peer keeps up to date (§1.10), patched by the rows a change touched.
//!
//! The sidebar's accounts and saved filters, the month's lines, its
//! summary and its formulas, the register on screen and what it adds up
//! to, what needs looking at, the payees a form offers, the banks, their
//! accounts and this person's asks about them live as long as the window;
//! the month's and the register's are opened again when the window moves
//! to another month or filter.

use ark::format::Encode;
use ark_peer::{Update, Watched};
use money_domain::*;

use crate::client::Replica;

/// A view's row, decoded: a row that does not decode is a domain this
/// build does not match, which is not a state to draw.
fn read<T: Encode>(b: &[u8]) -> T {
    T::from_bytes(b).unwrap_or_else(|e| panic!("a view's row: {e}"))
}

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
    pub linked: Watched<LinkedLine>,
    pub asks: Watched<AskLine>,
    /// The saved filters, in order, with their totals.
    pub filters: Watched<FilterLine>,
    /// The month's formulas, one per category that follows one.
    pub formulas: Watched<FormulaLine>,
    /// What the formula being typed would assign, while it is one.
    pub preview: Option<Watched<FormulaPreview>>,
    seen: ark::hash::StateHash,
}

fn month_of(month: &str) -> Vec<u8> {
    Month { month: month.into() }.to_bytes()
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
        let none = || ().to_bytes();
        let all = filtered(Ledger::All, &Filter::all()).to_bytes();
        let look = filtered(Ledger::Inbox, &Filter::all()).to_bytes();
        let mut p = Peer {
            accounts: Watched::open(c, "accounts", none(), read),
            groups: Watched::open(c, "groups", none(), read),
            lines: Watched::open(c, "month", month_of(month), read),
            summary: Watched::open(c, "summary", month_of(month), read),
            spending: Watched::open(c, "spending", month_of(month), read),
            register: Watched::open(c, "filtered", all.clone(), read),
            total: Watched::open(c, "filtered_total", all, read),
            inbox: Watched::open(c, "filtered", look, read),
            payees: Watched::open(c, "payees", none(), read),
            banks: Watched::open(c, "banks", none(), read),
            linked: Watched::open(c, "linked", none(), read),
            asks: Watched::open(c, "asks", none(), read),
            filters: Watched::open(c, "saved", none(), read),
            formulas: Watched::open(c, "formulas", month_of(month), read),
            preview: None,
            ledger: Ledger::All,
            adhoc: Filter::all(),
            base: Ledger::All,
            month: month.into(),
            seen: c.state(),
            client,
        };
        // What the hydrate read is not news.
        for v in p.views() {
            p.client.update(v);
        }
        p
    }

    fn views(&self) -> Vec<usize> {
        [
            self.accounts.view(),
            self.groups.view(),
            self.lines.view(),
            self.summary.view(),
            self.spending.view(),
            self.register.view(),
            self.total.view(),
            self.inbox.view(),
            self.payees.view(),
            self.banks.view(),
            self.linked.view(),
            self.asks.view(),
            self.filters.view(),
            self.formulas.view(),
            self.preview.as_ref().and_then(|p| p.view()),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    /// Every view brought up to what the peer holds: what each one did,
    /// spliced into its rows. Whether anything moved.
    pub fn refresh(&mut self) -> bool {
        let state = self.client.state();
        let moved = state != self.seen;
        self.seen = state;
        let c = &mut self.client;
        let mut any = false;
        let mut note = |u: Update| any |= u != Update::Unchanged;
        note(self.accounts.settle(c, moved));
        note(self.groups.settle(c, moved));
        note(self.lines.settle(c, moved));
        note(self.summary.settle(c, moved));
        note(self.spending.settle(c, moved));
        note(self.register.settle(c, moved));
        note(self.total.settle(c, moved));
        note(self.inbox.settle(c, moved));
        note(self.payees.settle(c, moved));
        note(self.banks.settle(c, moved));
        note(self.linked.settle(c, moved));
        note(self.asks.settle(c, moved));
        note(self.filters.settle(c, moved));
        note(self.formulas.settle(c, moved));
        if let Some(p) = &mut self.preview {
            note(p.settle(c, moved));
        }
        any
    }

    /// The window on another month: its lines, summary, spending and
    /// formulas.
    pub fn set_month(&mut self, month: &str) {
        if month == self.month {
            return;
        }
        let c = &mut self.client;
        let old = std::mem::replace(&mut self.lines, Watched::open(c, "month", month_of(month), read));
        old.close(c);
        let old = std::mem::replace(&mut self.summary, Watched::open(c, "summary", month_of(month), read));
        old.close(c);
        let old = std::mem::replace(&mut self.spending, Watched::open(c, "spending", month_of(month), read));
        old.close(c);
        let old = std::mem::replace(&mut self.formulas, Watched::open(c, "formulas", month_of(month), read));
        old.close(c);
        self.month = month.into();
    }

    /// The register on screen: the one for `ledger`, `adhoc` its
    /// conditions when it is not saved.
    pub fn set_ledger(&mut self, ledger: Ledger, adhoc: Filter) {
        let input = filtered(ledger, &adhoc).to_bytes();
        if self.register.is("filtered", &input) {
            (self.ledger, self.adhoc) = (ledger, adhoc);
            return;
        }
        let c = &mut self.client;
        let old = std::mem::replace(&mut self.register, Watched::open(c, "filtered", input.clone(), read));
        old.close(c);
        let old = std::mem::replace(&mut self.total, Watched::open(c, "filtered_total", input, read));
        old.close(c);
        (self.ledger, self.adhoc) = (ledger, adhoc);
    }

    /// What a formula not yet saved would assign, watched: none closes it.
    pub fn set_preview(&mut self, formula: Option<PreviewFormula>) {
        let input = formula.map(|f| f.to_bytes());
        if let (Some(p), Some(i)) = (&self.preview, &input) {
            if p.is("preview_formula", i) {
                return;
            }
        }
        let c = &mut self.client;
        if let Some(old) = self.preview.take() {
            old.close(c);
        }
        self.preview = input.map(|i| Watched::open(c, "preview_formula", i, read));
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
