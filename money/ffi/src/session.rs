//! money's views on the phones (`ark_peer::store`, `ark_ffi::Session`):
//! one [`Session`] over a peer holds the views the screens show — each
//! named by a [`MoneyView`], what a page is made of said by the domain
//! (`money_domain::MoneyView`) — keeps them spliced on the bridge's thread
//! and tells the app which moved ([`Changed`]). A screen holds its view
//! while it shows ([`Session::hold`], a [`Holding`] let go when dropped)
//! and reads it by name: `budget(month)`, `register(account)`, … — the
//! domain's records. A change is sent as one too ([`Session::send`], a
//! `MoneyChange`): nothing crosses as JSON.

use std::sync::Arc;

use ark::format::Encode;
use ark_ffi::{Peer, Refusal, RowId, Twin};
use money_domain::{
    self as domain, AccountLine, AskLine, BankLine, BudgetLine, Filter, Filtered, GroupLine, ItemLine, LinkedLine, MoneyChange, PayeeLine, PhotoLine,
    ReceiptLine, SpendingLine, Summary, TxnLine,
};

use crate::Sent;

type Tag = domain::UniFfiTag;
type Ffi<T> = <T as Twin<Tag>>::Ffi;

/// A view the phones hold: what a screen shows.
#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum MoneyView {
    /// The accounts, with their balances.
    Accounts,
    /// The budget's groups.
    Groups,
    /// The budget for a month (`2026-10`): its lines, summary and spending
    /// (and its formulas, which the window shows).
    Budget {
        month: String,
    },
    /// What needs a look.
    Inbox,
    /// One account's register, or (none) every account's.
    Register {
        account: Option<RowId>,
    },
    /// The payees a form offers.
    Payees,
    Banks,
    /// Each bank's accounts and the one it feeds.
    Linked,
    /// This person's asks of the server's importer.
    Asks,
    /// Every receipt photo, and where reading it stands.
    Photos,
    /// Every receipt.
    Receipts,
    /// The receipts that paid for a transaction.
    Paid {
        txn: RowId,
    },
    /// A receipt's items.
    Items {
        receipt: String,
    },
    /// The transactions a search finds — in one account's register, or
    /// (none) every account's: those whose payee, bank's words, memo,
    /// category, account or amount as written hold `text`, the core's one
    /// matcher (`Filter`).
    Search {
        account: Option<RowId>,
        text: String,
    },
}

impl MoneyView {
    /// The domain's filter a search is.
    fn search(account: Option<RowId>, text: String) -> Filtered {
        Filtered {
            filter: Filter {
                accounts: account.map(|a| domain::Id::new(a.0)).into_iter().collect(),
                text,
                ..Filter::default()
            },
            search: true,
            ..Filtered::default()
        }
    }

    /// The search a domain's filter is, when it is one.
    fn searched(filtered: &[u8]) -> Option<MoneyView> {
        let f = Filtered::from_bytes(filtered).ok()?;
        let search = MoneyView::search(f.filter.accounts.first().map(|a| RowId(a.0)), f.filter.text.clone());
        (f == search).then_some(MoneyView::Search {
            account: f.filter.accounts.first().map(|a| RowId(a.0)),
            text: f.filter.text,
        })
    }
}

impl From<MoneyView> for domain::MoneyView {
    fn from(v: MoneyView) -> domain::MoneyView {
        use domain::MoneyView as D;
        match v {
            MoneyView::Accounts => D::Accounts,
            MoneyView::Groups => D::Groups,
            MoneyView::Budget { month } => D::Budget { month },
            MoneyView::Inbox => D::Inbox,
            MoneyView::Register { account } => D::Register {
                account: account.map(|a| domain::Id::new(a.0)),
            },
            MoneyView::Payees => D::Payees,
            MoneyView::Banks => D::Banks,
            MoneyView::Linked => D::Linked,
            MoneyView::Asks => D::Asks,
            MoneyView::Photos => D::Photos,
            MoneyView::Receipts => D::Receipts,
            MoneyView::Paid { txn } => D::Paid { txn: domain::Id::new(txn.0) },
            MoneyView::Items { receipt } => D::Items { receipt },
            MoneyView::Search { account, text } => D::filtered(&MoneyView::search(account, text)),
        }
    }
}

impl MoneyView {
    /// The phones' name for a key of the domain's, when they hold it: the
    /// views a page is made of are told as the page.
    fn of(v: &domain::MoneyView) -> Option<MoneyView> {
        use domain::MoneyView as D;
        Some(match v {
            D::Accounts => MoneyView::Accounts,
            D::Groups => MoneyView::Groups,
            D::Budget { month } => MoneyView::Budget { month: month.clone() },
            D::Inbox => MoneyView::Inbox,
            D::Register { account } => MoneyView::Register {
                account: account.map(|a| RowId(a.0)),
            },
            D::Payees => MoneyView::Payees,
            D::Banks => MoneyView::Banks,
            D::Linked => MoneyView::Linked,
            D::Asks => MoneyView::Asks,
            D::Photos => MoneyView::Photos,
            D::Receipts => MoneyView::Receipts,
            D::Paid { txn } => MoneyView::Paid { txn: RowId(txn.0) },
            D::Items { receipt } => MoneyView::Items { receipt: receipt.clone() },
            D::Filtered { filtered } => return MoneyView::searched(filtered),
            _ => return None,
        })
    }
}

/// The budget for a month, as its page draws it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Budget {
    pub month: String,
    /// Each category's line, in the budget's order.
    pub lines: Vec<Ffi<BudgetLine>>,
    /// What the month adds up to; none until it is read.
    pub summary: Option<Ffi<Summary>>,
    /// What the month spent, by category.
    pub spending: Vec<Ffi<SpendingLine>>,
}

/// What the app is told when views it holds moved: it reads them again.
#[uniffi::export(with_foreign)]
pub trait Changed: Send + Sync {
    fn moved(&self, views: Vec<MoneyView>);
}

/// money's views over a peer.
#[derive(uniffi::Object)]
pub struct Session {
    inner: Arc<ark_ffi::Session<domain::MoneyView>>,
}

/// A view held while a screen shows it: let go when dropped (or
/// [`Holding::release`]d).
#[derive(uniffi::Object)]
pub struct Holding {
    hold: ark_ffi::Hold<domain::MoneyView>,
}

#[uniffi::export]
impl Holding {
    /// Held no more, now rather than when dropped.
    pub fn release(&self) {
        self.hold.close();
    }
}

impl Session {
    fn rows<T: Twin<Tag> + Clone + 'static>(&self, key: domain::MoneyView) -> Vec<T::Ffi> {
        self.inner.read(|st, _| st.rows::<T>(&key).iter().cloned().map(T::ffi).collect())
    }
}

#[uniffi::export]
impl Session {
    /// The views of `peer`, none held yet.
    #[uniffi::constructor]
    pub fn new(peer: Arc<Peer>) -> Arc<Session> {
        Arc::new(Session {
            inner: Arc::new(ark_ffi::Session::new(peer)),
        })
    }

    /// The peer it is over.
    pub fn peer(&self) -> Arc<Peer> {
        self.inner.peer().clone()
    }

    /// `view` held once more — opened and read the first time — until the
    /// holding is dropped.
    pub fn hold(&self, view: MoneyView) -> Arc<Holding> {
        Arc::new(Holding {
            hold: self.inner.hold(view.into()),
        })
    }

    /// A number that grows whenever `view`'s rows moved; zero while nothing
    /// holds it.
    pub fn version(&self, view: MoneyView) -> u64 {
        self.inner.version(&view.into())
    }

    /// Told which views moved, on the bridge's thread, whenever any held
    /// did; none tells nobody.
    pub fn set_listener(&self, listener: Option<Arc<dyn Changed>>) {
        self.inner.set_listener(listener.map(|l| -> ark_ffi::session::Moved<domain::MoneyView> {
            Arc::new(move |moved: Vec<domain::MoneyView>| {
                let views: Vec<MoneyView> = moved.iter().filter_map(MoneyView::of).collect();
                if !views.is_empty() {
                    l.moved(views);
                }
            })
        }));
    }

    /// A change: its intent, or the domain's sentence for why not — kept
    /// as [`Session::refusal`] until the next change is taken.
    pub fn send(&self, change: Ffi<MoneyChange>) -> Sent {
        let c = MoneyChange::from_ffi(change);
        match self.inner.send(|p| p.mutate_encoded(c.name(), c.input())) {
            Ok(id) => Sent::Intent { id },
            Err(Refusal::Refused { why }) => Sent::Refused { why },
        }
    }

    /// The domain's sentence for the first thing wrong with a change, or
    /// none: what a form says under its field before anything is sent.
    pub fn check(&self, change: Ffi<MoneyChange>) -> Option<String> {
        MoneyChange::from_ffi(change).problem()
    }

    /// Why the last change sent was refused, while it is the last.
    pub fn refusal(&self) -> Option<String> {
        self.inner.refusal()
    }

    pub fn accounts(&self) -> Vec<Ffi<AccountLine>> {
        self.rows::<AccountLine>(domain::MoneyView::Accounts)
    }

    pub fn groups(&self) -> Vec<Ffi<GroupLine>> {
        self.rows::<GroupLine>(domain::MoneyView::Groups)
    }

    /// The budget for `month`, as far as it is held.
    pub fn budget(&self, month: String) -> Budget {
        let m = || month.clone();
        let summary = self
            .inner
            .read(|st, _| st.first::<Summary>(&domain::MoneyView::Summary { month: m() }).cloned());
        Budget {
            lines: self.rows::<BudgetLine>(domain::MoneyView::Lines { month: m() }),
            summary: summary.map(Twin::<Tag>::ffi),
            spending: self.rows::<SpendingLine>(domain::MoneyView::Spending { month: m() }),
            month,
        }
    }

    pub fn inbox(&self) -> Vec<Ffi<TxnLine>> {
        self.rows::<TxnLine>(domain::MoneyView::Inbox)
    }

    /// `account`'s register, or (none) every account's.
    pub fn register(&self, account: Option<RowId>) -> Vec<Ffi<TxnLine>> {
        self.rows::<TxnLine>(MoneyView::Register { account }.into())
    }

    pub fn payees(&self) -> Vec<Ffi<PayeeLine>> {
        self.rows::<PayeeLine>(domain::MoneyView::Payees)
    }

    pub fn banks(&self) -> Vec<Ffi<BankLine>> {
        self.rows::<BankLine>(domain::MoneyView::Banks)
    }

    pub fn linked(&self) -> Vec<Ffi<LinkedLine>> {
        self.rows::<LinkedLine>(domain::MoneyView::Linked)
    }

    /// This person's asks, the newest first.
    pub fn asks(&self) -> Vec<Ffi<AskLine>> {
        self.rows::<AskLine>(domain::MoneyView::Asks)
    }

    /// Every receipt photo, and where reading it stands.
    pub fn photos(&self) -> Vec<Ffi<PhotoLine>> {
        self.rows::<PhotoLine>(domain::MoneyView::Photos)
    }

    /// Every receipt.
    pub fn receipts(&self) -> Vec<Ffi<ReceiptLine>> {
        self.rows::<ReceiptLine>(domain::MoneyView::Receipts)
    }

    /// The receipts that paid for `txn`.
    pub fn paid(&self, txn: RowId) -> Vec<Ffi<ReceiptLine>> {
        self.rows::<ReceiptLine>(MoneyView::Paid { txn }.into())
    }

    /// A receipt's items.
    pub fn items(&self, receipt: String) -> Vec<Ffi<ItemLine>> {
        self.rows::<ItemLine>(domain::MoneyView::Items { receipt })
    }

    /// What a search finds, the newest first, as far as it is held.
    pub fn search(&self, account: Option<RowId>, text: String) -> Vec<Ffi<TxnLine>> {
        self.rows::<TxnLine>(MoneyView::Search { account, text }.into())
    }
}
