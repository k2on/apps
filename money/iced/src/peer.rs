//! The replica, and the lists a window draws from it — every one a view
//! the peer keeps up to date (§1.10), patched by the rows a change touched.
//!
//! The sidebar's accounts, the month's lines and its summary, the register
//! on screen, what needs looking at, the payees a form offers and the banks
//! live as long as the window; the month's and the register's are opened
//! again when the window moves to another month or account.

use ark::format::Encode;
use ark_peer::{Update, Watched};
use money_domain::*;

use crate::client::Replica;

/// A view's row, decoded: a row that does not decode is a domain this
/// build does not match, which is not a state to draw.
fn read<T: Encode>(b: &[u8]) -> T {
    T::from_bytes(b).unwrap_or_else(|e| panic!("a view's row: {e}"))
}

/// Which register is on screen: one account's, every account's, or what
/// needs looking at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ledger {
    Account(Id<Account>),
    All,
    Inbox,
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
    pub register: Watched<TxnLine>,
    pub inbox: Watched<TxnLine>,
    pub payees: Watched<PayeeLine>,
    pub banks: Watched<BankLine>,
    seen: ark::hash::StateHash,
}

fn month_of(month: &str) -> Vec<u8> {
    Month { month: month.into() }.to_bytes()
}

fn register_of(ledger: Ledger) -> (&'static str, Vec<u8>) {
    match ledger {
        Ledger::Account(a) => ("register", Register { account_id: Some(a) }.to_bytes()),
        Ledger::All => ("register", Register { account_id: None }.to_bytes()),
        Ledger::Inbox => ("inbox", ().to_bytes()),
    }
}

impl Peer {
    /// Every list the window starts on, hydrated once.
    pub fn open(mut client: Replica, month: &str) -> Peer {
        let c = &mut client;
        let none = || ().to_bytes();
        let (name, input) = register_of(Ledger::All);
        let mut p = Peer {
            accounts: Watched::open(c, "accounts", none(), read),
            groups: Watched::open(c, "groups", none(), read),
            lines: Watched::open(c, "month", month_of(month), read),
            summary: Watched::open(c, "summary", month_of(month), read),
            spending: Watched::open(c, "spending", month_of(month), read),
            register: Watched::open(c, name, input, read),
            inbox: Watched::open(c, "inbox", none(), read),
            payees: Watched::open(c, "payees", none(), read),
            banks: Watched::open(c, "banks", none(), read),
            ledger: Ledger::All,
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
            self.inbox.view(),
            self.payees.view(),
            self.banks.view(),
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
        note(self.inbox.settle(c, moved));
        note(self.payees.settle(c, moved));
        note(self.banks.settle(c, moved));
        any
    }

    /// The window on another month: its lines, summary and spending.
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
        self.month = month.into();
    }

    /// The register on screen.
    pub fn set_ledger(&mut self, ledger: Ledger) {
        if ledger == self.ledger {
            return;
        }
        let (name, input) = register_of(ledger);
        let c = &mut self.client;
        let old = std::mem::replace(&mut self.register, Watched::open(c, name, input, read));
        old.close(c);
        self.ledger = ledger;
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
