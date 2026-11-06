//! The rows' methods and the domain's rules as the phones call them
//! (the `ffi` feature): each is the Rust one of [`crate::words`],
//! [`crate::accounts`], [`crate::transactions`], [`crate::banks`] and
//! [`crate::changes`] on the rows' UniFFI twins, so that a phone asks
//! `line.tone()`, `ask.said(now)`, `draft.problem()` and never works them
//! out itself.

use ark_ffi::{RowId, Twin};
use ark_format::NumberStyle;

use crate::accounts::AccountShelfFfi;
use crate::accounts::{self, AccountKind};
use crate::banks::FollowFfi;
use crate::changes::{MoneyChange, MoneyChangeFfi};
use crate::transactions::{Draft, DraftFfi};
use crate::types::*;
use crate::words::{self, BankStatusFfi, CaptionsFfi, MeterFfi, Tone, ToneFfi};
use crate::Id;

type Tag = crate::UniFfiTag;

fn of<T: Twin<Tag>>(f: &T::Ffi) -> T
where
    T::Ffi: Clone,
{
    T::from_ffi(f.clone())
}

fn all<T: Twin<Tag>>(fs: Vec<T::Ffi>) -> Vec<T> {
    fs.into_iter().map(T::from_ffi).collect()
}

fn out<T: Twin<Tag>>(ts: Vec<T>) -> Vec<T::Ffi> {
    ts.into_iter().map(T::ffi).collect()
}

fn id<R>(r: Option<RowId>) -> Option<Id<R>> {
    r.map(|r| Id::new(r.0))
}

// * Accounts

#[uniffi::export]
impl AccountKind {
    /// The key an account keeps: `checking`.
    #[uniffi::method(name = "key")]
    pub fn ffi_key(&self) -> String {
        self.key().into()
    }

    /// What a person calls it: "Credit Card".
    #[uniffi::method(name = "title")]
    pub fn ffi_title(&self) -> String {
        self.title().into()
    }

    #[uniffi::method(name = "on_budget_by_default")]
    pub fn ffi_on_budget_by_default(&self) -> bool {
        self.on_budget_by_default()
    }

    /// Whether its balance is what is owed on it.
    #[uniffi::method(name = "owes")]
    pub fn ffi_owes(&self) -> bool {
        AccountKind::owes(*self)
    }
}

/// Every kind of account, in the order a form offers them.
#[uniffi::export]
pub fn account_kinds() -> Vec<AccountKind> {
    AccountKind::ALL.to_vec()
}

/// What a person calls a kind by its key; the key, capitalized, for none.
#[uniffi::export]
pub fn kind_title(key: String) -> String {
    accounts::kind_title(&key)
}

/// The accounts in their sections: on the budget, tracking, closed.
#[uniffi::export]
pub fn account_shelves(accounts: Vec<AccountLineFfi>) -> Vec<AccountShelfFfi> {
    out(accounts::shelves(&all::<AccountLine>(accounts)))
}

/// The open accounts' balances, together.
#[uniffi::export]
pub fn net_worth(accounts: Vec<AccountLineFfi>) -> i64 {
    accounts::net_worth(&all::<AccountLine>(accounts))
}

/// The account a new transaction goes to when none is chosen.
#[uniffi::export]
pub fn usual_account(accounts: Vec<AccountLineFfi>) -> Option<RowId> {
    accounts::usual(&all::<AccountLine>(accounts)).map(|a| RowId(a.0))
}

/// The accounts a bank's account may feed, those of `kind` first.
#[uniffi::export]
pub fn linkable_accounts(accounts: Vec<AccountLineFfi>, kind: String) -> Vec<AccountLineFfi> {
    out(accounts::linkable(&all::<AccountLine>(accounts), &kind))
}

#[uniffi::export(name = "AccountLine")]
impl AccountLineFfi {
    /// Its bank and last digits, or its kind.
    pub fn subtitle(&self) -> String {
        of::<AccountLine>(self).subtitle()
    }

    pub fn account_kind(&self) -> Option<AccountKind> {
        of::<AccountLine>(self).account_kind()
    }
}

// * The budget

#[uniffi::export(name = "BudgetLine")]
impl BudgetLineFfi {
    pub fn is_payment(&self) -> bool {
        of::<BudgetLine>(self).is_payment()
    }

    pub fn is_ready_to_assign(&self) -> bool {
        of::<BudgetLine>(self).is_ready_to_assign()
    }

    pub fn assignable(&self) -> bool {
        of::<BudgetLine>(self).assignable()
    }

    pub fn on_card(&self) -> bool {
        of::<BudgetLine>(self).on_card()
    }

    pub fn tone(&self) -> ToneFfi {
        of::<BudgetLine>(self).tone().ffi()
    }

    pub fn meter(&self) -> Option<MeterFfi> {
        of::<BudgetLine>(self).meter().map(Twin::<Tag>::ffi)
    }

    pub fn story(&self, style: NumberStyle) -> String {
        of::<BudgetLine>(self).story(&style)
    }

    pub fn captions(&self, style: NumberStyle) -> CaptionsFfi {
        Twin::<Tag>::ffi(of::<BudgetLine>(self).captions(&style))
    }

    pub fn progress(&self, style: NumberStyle) -> String {
        of::<BudgetLine>(self).progress(&style)
    }

    pub fn rule_words(&self) -> Option<String> {
        of::<BudgetLine>(self).rule_words()
    }

    pub fn to_cover(&self) -> Option<i64> {
        of::<BudgetLine>(self).to_cover()
    }

    pub fn to_target(&self) -> i64 {
        of::<BudgetLine>(self).to_target()
    }

    pub fn spoken(&self, style: NumberStyle) -> String {
        of::<BudgetLine>(self).spoken(&style)
    }
}

#[uniffi::export(name = "Tone")]
impl ToneFfi {
    pub fn words(&self) -> String {
        Tone::from_ffi(self.clone()).words().into()
    }
}

#[uniffi::export(name = "Meter")]
impl MeterFfi {
    /// The share, 0 to 1.
    pub fn fraction(&self) -> f64 {
        self.thousandths as f64 / 1000.0
    }
}

#[uniffi::export(name = "Summary")]
impl SummaryFfi {
    pub fn ready_words(&self) -> String {
        of::<Summary>(self).ready_words()
    }

    pub fn tone(&self) -> ToneFfi {
        of::<Summary>(self).tone().ffi()
    }

    pub fn attention(&self) -> i64 {
        of::<Summary>(self).attention()
    }
}

/// What the cards owe beyond what their payment categories hold.
#[uniffi::export]
pub fn uncovered(lines: Vec<BudgetLineFfi>) -> i64 {
    words::uncovered(&all::<BudgetLine>(lines))
}

// * Transactions

#[uniffi::export(name = "TxnLine")]
impl TxnLineFfi {
    pub fn needs_category(&self) -> bool {
        of::<TxnLine>(self).needs_category()
    }

    pub fn bank_said(&self) -> Option<String> {
        of::<TxnLine>(self).bank_said()
    }

    /// The form over it, to correct it.
    pub fn draft(&self) -> DraftFfi {
        Draft::of(&of::<TxnLine>(self)).ffi()
    }
}

/// A new transaction's form on `date`, in `account` or the usual one.
#[uniffi::export]
pub fn new_draft(accounts: Vec<AccountLineFfi>, account: Option<RowId>, date: String) -> DraftFfi {
    Draft::new(&all::<AccountLine>(accounts), id(account), &date).ffi()
}

#[uniffi::export(name = "Draft")]
impl DraftFfi {
    pub fn problem(&self) -> Option<String> {
        of::<Draft>(self).problem()
    }

    pub fn to_change(&self) -> Option<MoneyChangeFfi> {
        of::<Draft>(self).to_change().map(Twin::<Tag>::ffi)
    }

    pub fn with_payee(&self, payee: PayeeLineFfi) -> DraftFfi {
        of::<Draft>(self).with_payee(&PayeeLine::from_ffi(payee)).ffi()
    }

    pub fn with_category(&self, category: Option<RowId>) -> DraftFfi {
        of::<Draft>(self).with_category(id(category)).ffi()
    }

    pub fn with_transfer(&self, account: RowId) -> DraftFfi {
        of::<Draft>(self).with_transfer(Id::new(account.0)).ffi()
    }
}

/// Who a payee field offers while `typed` is typed.
#[uniffi::export]
pub fn suggest_payees(typed: String, payees: Vec<PayeeLineFfi>, limit: u32) -> Vec<PayeeLineFfi> {
    out(crate::transactions::suggest(&typed, &all::<PayeeLine>(payees), limit as usize))
}

#[uniffi::export(name = "MoneyChange")]
impl MoneyChangeFfi {
    /// The function it is (`accounts.open_account`).
    pub fn name(&self) -> String {
        of::<MoneyChange>(self).name().into()
    }

    /// The first of its refinements it fails, in the domain's words.
    pub fn problem(&self) -> Option<String> {
        of::<MoneyChange>(self).problem()
    }
}

// * Banks

#[uniffi::export(name = "AskLine")]
impl AskLineFfi {
    pub fn is_open(&self) -> bool {
        of::<AskLine>(self).is_open()
    }

    pub fn bank_name(&self) -> String {
        of::<AskLine>(self).bank_name()
    }

    /// What has come of it, at `now` (ms).
    pub fn said(&self, now: i64) -> String {
        of::<AskLine>(self).said(now)
    }

    /// When Plaid's page closes, said, while it waits on it.
    pub fn closes(&self, now: i64) -> Option<String> {
        of::<AskLine>(self).closes(now)
    }
}

#[uniffi::export(name = "BankLine")]
impl BankLineFfi {
    pub fn title(&self) -> String {
        of::<BankLine>(self).title()
    }

    /// How it is doing at `now` (ms).
    pub fn status(&self, now: i64) -> BankStatusFfi {
        of::<BankLine>(self).status(now).ffi()
    }
}

#[uniffi::export(name = "LinkedLine")]
impl LinkedLineFfi {
    pub fn title(&self) -> String {
        of::<LinkedLine>(self).title()
    }

    pub fn kind_words(&self) -> String {
        of::<LinkedLine>(self).kind_words()
    }

    pub fn opens_words(&self) -> String {
        of::<LinkedLine>(self).opens_words()
    }
}

/// What to do about an ask made here once the asks moved; `since` is the
/// newest ask there was before it.
#[uniffi::export]
pub fn follow(asks: Vec<AskLineFfi>, since: Option<RowId>) -> FollowFfi {
    crate::banks::follow(&all::<AskLine>(asks), id(since)).ffi()
}

/// The ask to tell of, unless `dismissed`.
#[uniffi::export]
pub fn ask_shown(asks: Vec<AskLineFfi>, dismissed: Option<RowId>, now: i64) -> Option<AskLineFfi> {
    words::ask_shown(&all::<AskLine>(asks), id(dismissed), now).map(Twin::<Tag>::ffi)
}

/// What wants somebody in Settings.
#[uniffi::export]
pub fn banks_wanting(banks: Vec<BankLineFfi>) -> i64 {
    words::banks_wanting(&all::<BankLine>(banks))
}

/// A bank's accounts, those waiting for a choice first.
#[uniffi::export]
pub fn bank_accounts(bank: String, linked: Vec<LinkedLineFfi>) -> Vec<LinkedLineFfi> {
    out(words::bank_accounts(&bank, &all::<LinkedLine>(linked)))
}

// * Receipts

#[uniffi::export(name = "ReceiptLine")]
impl ReceiptLineFfi {
    pub fn store_name(&self) -> String {
        of::<ReceiptLine>(self).store_name()
    }

    pub fn unmatched(&self) -> bool {
        of::<ReceiptLine>(self).unmatched()
    }
}

#[uniffi::export(name = "ItemLine")]
impl ItemLineFfi {
    pub fn quantity_words(&self) -> String {
        of::<ItemLine>(self).quantity_words()
    }
}

/// A store as a receipt names it.
#[uniffi::export]
pub fn store_name(store: String) -> String {
    words::store_name(&store)
}

/// The receipts nothing pays for yet that a transaction of `amount` on
/// `day` is offered.
#[uniffi::export]
pub fn unmatched_receipts(receipts: Vec<ReceiptLineFfi>, amount: i64, day: String) -> Vec<ReceiptLineFfi> {
    out(words::unmatched_for(&all::<ReceiptLine>(receipts), amount, &day))
}

// * The budget's own ids

/// Ready to Assign's category: the same id on every peer.
#[uniffi::export]
pub fn ready_to_assign_id() -> RowId {
    RowId(crate::budget::ready_to_assign().0)
}

/// The cards' own group, Credit Card Payments: the same id on every peer.
#[uniffi::export]
pub fn card_group_id() -> RowId {
    RowId(crate::cards::card_group().0)
}

#[uniffi::export(name = "GroupLine")]
impl GroupLineFfi {
    /// Whether it is the cards' own group, filled and emptied as the cards are.
    pub fn is_cards(&self) -> bool {
        of::<GroupLine>(self).id == crate::cards::card_group()
    }
}
