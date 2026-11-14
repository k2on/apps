//! The types: every record a function takes or answers. A refinement
//! (`#[ark(..)]`) is checked once, where bytes come in, and its words are
//! what a form shows.

use ark::prelude::*;

use crate::dates::{is_day, is_month};
use crate::schema::{Account, AskState, BankAsk, Category, Filter, Group, Photo, SavedFilter, Term, Txn};

/// How far the bank and the household agree on a transaction: not yet seen
/// at the bank, seen there, or agreed to as part of a reconciled balance —
/// which nothing changes afterwards, an import included.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize, Describe)]
#[ark(ffi)]
pub enum Cleared {
    #[default]
    Uncleared,
    Cleared,
    Reconciled,
}

fn at_least_0(n: &i64) -> bool {
    *n >= 0
}

fn more_than_0(n: &i64) -> bool {
    *n > 0
}

// * Accounts

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct OpenAccount {
    #[ark(trim, non_empty, why = "an account needs a name")]
    #[ark(len = "0..=80")]
    pub name: String,
    pub kind: crate::accounts::AccountKind,
    pub on_budget: bool,
    /// What is in it today, in cents: what you owe on a card or a loan is
    /// negative.
    pub balance: i64,
    #[ark(check = "is_day", why = "a day, as 2026-10-26")]
    pub date: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct RenameAccount {
    pub id: Id<Account>,
    #[ark(trim, non_empty, why = "an account needs a name")]
    #[ark(len = "0..=80")]
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct CloseAccount {
    pub id: Id<Account>,
    pub closed: bool,
}

// * Groups and categories

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct AddGroup {
    #[ark(trim, non_empty, why = "a group needs a name")]
    #[ark(len = "0..=80")]
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct RenameGroup {
    pub id: Id<Group>,
    #[ark(trim, non_empty, why = "a group needs a name")]
    #[ark(len = "0..=80")]
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct HideGroup {
    pub id: Id<Group>,
    pub hidden: bool,
}

/// A group gone, and with it its categories: what was spent from them is
/// moved to `move_to`, or left for nothing.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct DeleteGroup {
    pub id: Id<Group>,
    pub move_to: Option<Id<Category>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct AddCategory {
    pub group_id: Id<Group>,
    #[ark(trim, non_empty, why = "a category needs a name")]
    #[ark(len = "0..=80")]
    pub name: String,
}

/// A kind of a category, under it: "Eggs" under "Groceries".
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct AddSubcategory {
    pub parent_id: Id<Category>,
    #[ark(trim, non_empty, why = "a category needs a name")]
    #[ark(len = "0..=80")]
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct RenameCategory {
    pub id: Id<Category>,
    #[ark(trim, non_empty, why = "a category needs a name")]
    #[ark(len = "0..=80")]
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct HideCategory {
    pub id: Id<Category>,
    pub hidden: bool,
}

/// A category moved: an envelope into a group (`group_id`, a heading),
/// a kind under another category (`group_id`, its new parent).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct MoveCategory {
    pub id: Id<Category>,
    pub group_id: Id<Group>,
    /// The category it goes in front of, in that group; none for its end.
    #[serde(default)]
    pub before: Option<Id<Category>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct MoveGroup {
    pub id: Id<Group>,
    /// The group it goes in front of; none for after every other.
    #[serde(default)]
    pub before: Option<Id<Group>>,
}

/// What a category wants each month, in cents; 0 for nothing.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct SetTarget {
    pub id: Id<Category>,
    #[ark(check = "at_least_0", why = "a target is never below nothing")]
    pub target: i64,
}

/// A category gone: what it was given and what was spent from it moved to
/// `move_to`, or (none) its spending left for nothing and its money back
/// to be assigned.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct DeleteCategory {
    pub id: Id<Category>,
    pub move_to: Option<Id<Category>>,
}

// * The budget

/// A category's assigned amount for a month, set.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct Assign {
    #[ark(check = "is_month", why = "a month, as 2026-10")]
    pub month: String,
    pub category_id: Id<Category>,
    pub amount: i64,
}

/// Money moved between two categories in a month, or to and from what is
/// ready to assign (`None`).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct MoveMoney {
    #[ark(check = "is_month", why = "a month, as 2026-10")]
    pub month: String,
    pub from: Option<Id<Category>>,
    pub to: Option<Id<Category>>,
    #[ark(check = "more_than_0", why = "move something")]
    pub amount: i64,
}

/// A whole month assigned at once, the way the budget's quick buttons do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[ark(ffi)]
pub enum Auto {
    /// Each category given what it was given the month before.
    #[default]
    LastMonth,
    /// Each category with a target given what it still lacks of it.
    Underfunded,
    /// Each category's assignment taken back.
    Reset,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct AutoAssign {
    #[ark(check = "is_month", why = "a month, as 2026-10")]
    pub month: String,
    pub how: Auto,
    /// Only these categories; empty is every one.
    pub only: Vec<Id<Category>>,
}

// * Transactions

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct AddTransaction {
    pub account_id: Id<Account>,
    #[ark(check = "is_day", why = "a day, as 2026-10-26")]
    pub date: String,
    #[ark(trim, len = "0..=120")]
    pub payee: String,
    pub category_id: Option<Id<Category>>,
    #[ark(trim, len = "0..=400")]
    pub memo: String,
    pub amount: i64,
    pub cleared: bool,
    /// The other account, for a transfer: the payee is then that account.
    pub transfer_to: Option<Id<Account>>,
}

/// A transaction added split: its amount in lines, each for a category.
/// Its own input, so that `AddTransaction` keeps the shape every client
/// sends.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct AddSplit {
    pub account_id: Id<Account>,
    #[ark(check = "is_day", why = "a day, as 2026-10-26")]
    pub date: String,
    #[ark(trim, len = "0..=120")]
    pub payee: String,
    #[ark(trim, len = "0..=400")]
    pub memo: String,
    pub amount: i64,
    pub cleared: bool,
    pub lines: Vec<SplitPart>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct EditTransaction {
    pub id: Id<Txn>,
    #[ark(check = "is_day", why = "a day, as 2026-10-26")]
    pub date: String,
    #[ark(trim, len = "0..=120")]
    pub payee: String,
    pub category_id: Option<Id<Category>>,
    #[ark(trim, len = "0..=400")]
    pub memo: String,
    pub amount: i64,
    /// Made a transfer to (or from) this account: paired with a matching
    /// transaction already there, or given its other half there. None
    /// leaves it as it is — a transfer stays one, to the same account.
    #[serde(default)]
    pub transfer_to: Option<Id<Account>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct Txns {
    pub ids: Vec<Id<Txn>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct Categorize {
    pub ids: Vec<Id<Txn>>,
    pub category_id: Option<Id<Category>>,
}

/// One line of a split: part of the transaction's amount, for a category
/// (none: not yet said).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[ark(ffi)]
pub struct SplitPart {
    pub category_id: Option<Id<Category>>,
    pub amount: i64,
    #[serde(default)]
    pub memo: String,
}

/// A transaction split into `lines`, which add up to its amount; no lines
/// is unsplit, for no category until somebody gives it one.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct SplitTxn {
    pub id: Id<Txn>,
    pub lines: Vec<SplitPart>,
}

/// One transaction, by id.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct OneTxn {
    pub id: Id<Txn>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct SetCleared {
    pub id: Id<Txn>,
    pub cleared: bool,
}

/// The bank says the account holds `balance` on `date`: every cleared
/// transaction becomes reconciled, and a difference is an adjustment.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct Reconcile {
    pub account_id: Id<Account>,
    pub balance: i64,
    #[ark(check = "is_day", why = "a day, as 2026-10-26")]
    pub date: String,
}

// * The Plaid import

/// One account of a linked bank, as Plaid reported it at the import.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[ark(ffi)]
pub struct BankAccount {
    pub plaid_account_id: String,
    pub name: String,
    pub mask: String,
    /// Plaid's type: `depository`, `credit`, `loan`, `investment`…
    pub kind: String,
    /// Plaid's subtype: `checking`, `savings`, `credit card`…
    pub subtype: String,
    /// The bank's current balance, in cents, our sign (owed is negative).
    pub balance: Option<i64>,
    /// What the account the first import opens for it is called, when the
    /// server was told (its `accounts`); blank is the bank's own name.
    pub called: String,
    /// Not followed from its first sight: the server was told which of
    /// this bank's accounts to follow, and this is not one. It is known
    /// (a `Linked` row, ignored) and opens nothing; `banks.link` follows it.
    pub ignored: bool,
}

/// A transaction Plaid added or changed, our sign: what left is negative.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[ark(ffi)]
pub struct BankTxn {
    pub transaction_id: String,
    pub plaid_account_id: String,
    /// The pending transaction this posted one replaces.
    pub pending_transaction_id: Option<String>,
    pub date: String,
    pub amount: i64,
    /// The merchant's name, else the bank's line.
    pub payee: String,
    /// The bank's own line for it.
    pub imported_payee: String,
    pub pending: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[ark(ffi)]
pub struct BankRemoved {
    pub transaction_id: String,
    pub plaid_account_id: String,
}

/// One run of a bank's `/transactions/sync`, whole: what the server's
/// importer authors (`crate::plaid::import`).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct Import {
    #[ark(trim, non_empty, why = "a bank needs a name")]
    pub bank: String,
    pub institution: String,
    pub cursor: String,
    pub accounts: Vec<BankAccount>,
    pub upserts: Vec<BankTxn>,
    pub removed: Vec<BankRemoved>,
    /// An account this import is the first to see waits for somebody to
    /// say what it feeds (`crate::banks`), rather than opening one here
    /// under the bank's name: what a server's importer says. The demo's
    /// bank opens its card as it is seen.
    #[serde(default)]
    pub choose: bool,
    /// Asked of Plaid from no cursor: the bank's whole history, which an
    /// account opened for one of its accounts takes its starting balance
    /// from.
    #[serde(default)]
    pub from_start: bool,
    /// Plaid's id for the bank's institution, when the importer knows it.
    #[serde(default)]
    pub institution_id: Option<String>,
}

/// A bank's account made to feed an account already here, in place of the
/// one its first import opened.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct LinkAccount {
    pub plaid_account_id: String,
    pub account_id: Id<Account>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct IgnoreLinked {
    pub plaid_account_id: String,
    pub ignored: bool,
}

// * Receipts

fn is_store(s: &str) -> bool {
    crate::stores::STORES.contains(&s) || s == crate::stores::PAPER
}

/// A thing bought, as a store's adapter read it.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[ark(ffi)]
pub struct StoreItem {
    pub name: String,
    pub sku: String,
    /// In thousandths: 1000 is one.
    pub quantity: i64,
    pub unit: String,
    /// Our sign: spent is negative, a discount positive.
    pub amount: i64,
}

/// A receipt or order, as a store's adapter read it.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[ark(ffi)]
pub struct StoreReceipt {
    /// The store's own id for it: an order number, a receipt's barcode.
    pub order_id: String,
    pub date: String,
    pub place: String,
    /// What was paid, tax in, our sign.
    pub total: i64,
    pub items: Vec<StoreItem>,
}

/// One store's receipts, from one export ([`crate::stores`]).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct ReceiptImport {
    #[ark(check = "is_store", why = "amazon, target or costco")]
    pub store: String,
    pub receipts: Vec<StoreReceipt>,
}

/// One item of one receipt.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[ark(ffi)]
pub struct ItemRef {
    pub receipt_id: String,
    pub line: i64,
}

/// Items filed under a category (none: unfiled); every unfiled item with
/// the same key as one of them is filed with it.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct ClassifyItems {
    pub items: Vec<ItemRef>,
    pub category_id: Option<Id<Category>>,
}

/// A charge a store made, as its adapter read it: for which order, on
/// which day, how much (our sign: a refund is positive), on which card.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[ark(ffi)]
pub struct StoreCharge {
    pub order_id: String,
    pub date: String,
    pub amount: i64,
    pub card: String,
}

/// One store's charges, from one history ([`crate::stores`]).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct ChargeImport {
    #[ark(check = "is_store", why = "amazon, target or costco")]
    pub store: String,
    pub charges: Vec<StoreCharge>,
}

/// A charge said to be paid by a transaction, or by none found yet.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct PayCharge {
    pub charge_id: String,
    pub txn_id: Option<Id<Txn>>,
}

/// A receipt said to be paid by a transaction (none: by none found yet).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct MatchReceipt {
    pub receipt_id: String,
    pub txn_id: Option<Id<Txn>>,
}

/// A bank's account given an account of its own here, opened for it: a
/// credit card's a credit account, a savings account's savings. Blank is
/// the bank's name for it.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct OpenLinked {
    pub plaid_account_id: String,
    #[ark(trim, len = "0..=80")]
    pub name: String,
}

/// A connected bank, by the name the server keeps it under.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct BankName {
    #[ark(trim, non_empty, why = "which bank?")]
    pub bank: String,
}

/// The importer's answer to an ask (`crate::banks::answer`).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct Answer {
    pub id: Id<BankAsk>,
    pub state: AskState,
    pub bank: String,
    pub institution: String,
    pub url: String,
    pub message: String,
    pub expires_ms: i64,
}

/// The importer could not import a bank: what Plaid said.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct BankFailed {
    #[ark(trim, non_empty, why = "a bank needs a name")]
    pub bank: String,
    pub institution: String,
    pub error_code: String,
    pub error: String,
}

// * Filters and formulas

fn is_month_or_now(m: &Option<String>) -> bool {
    m.as_deref().is_none_or(is_month)
}

/// A filter kept under a name ([`crate::filters`]).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct SaveFilter {
    #[ark(trim, non_empty, why = "a filter needs a name")]
    #[ark(len = "0..=80")]
    pub name: String,
    pub filter: Filter,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct RenameFilter {
    pub id: Id<SavedFilter>,
    #[ark(trim, non_empty, why = "a filter needs a name")]
    #[ark(len = "0..=80")]
    pub name: String,
}

/// A saved filter's conditions, changed: every formula that sums it moves
/// with it, in every month it covers.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct ChangeFilter {
    pub id: Id<SavedFilter>,
    pub filter: Filter,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct FilterId {
    pub id: Id<SavedFilter>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct MoveFilter {
    pub id: Id<SavedFilter>,
    /// The filter it goes in front of; none for after every other.
    #[serde(default)]
    pub before: Option<Id<SavedFilter>>,
}

/// A category assigned by formula from a month on: each month, the sum of
/// `times / per` of what each term's saved filter adds up to in it,
/// rounded once to the cent ([`crate::formulas`]).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct SetFormula {
    pub category_id: Id<Category>,
    /// The first month it assigns; none is this month.
    #[ark(check = "is_month_or_now", why = "a month, as 2026-10")]
    #[serde(default)]
    pub from: Option<String>,
    pub terms: Vec<Term>,
}

/// A category assigned by hand again from a month on (none is this one).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct ClearFormula {
    pub category_id: Id<Category>,
    #[ark(check = "is_month_or_now", why = "a month, as 2026-10")]
    #[serde(default)]
    pub from: Option<String>,
}

/// What a formula would assign in a month, before it is saved.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct PreviewFormula {
    #[ark(check = "is_month", why = "a month, as 2026-10")]
    pub month: String,
    pub terms: Vec<Term>,
}

/// What the phones set before formulas, kept for them: a category given,
/// from a month on, `times / per` of the month's inflows (not transfers)
/// in `accounts` (none is every account on the budget) whose payee or
/// bank line holds `matching`. It is a formula of one term, over a saved
/// filter of exactly those conditions ([`crate::formulas::set_rule`]).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct SetRule {
    pub category_id: Id<Category>,
    /// The first month it assigns; none is this month.
    #[ark(check = "is_month_or_now", why = "a month, as 2026-10")]
    #[serde(default)]
    pub from: Option<String>,
    #[ark(trim, non_empty, why = "a rule matches some text")]
    #[ark(len = "0..=120")]
    pub matching: String,
    pub accounts: Vec<Id<Account>>,
    #[ark(check = "at_least_0", why = "a share is never below nothing")]
    pub times: i64,
    #[ark(check = "more_than_0", why = "a share is of something")]
    pub per: i64,
}

// * The views' inputs

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Month {
    #[ark(check = "is_month", why = "a month, as 2026-10")]
    pub month: String,
}

/// One account's transactions, or (none) every account's.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Register {
    pub account_id: Option<Id<Account>>,
}

/// The items filed under a category or any kind of it — the eggs from
/// every store under "Eggs" — or (none) every item not filed yet.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct ItemsIn {
    pub category_id: Option<Id<Category>>,
}

/// One receipt, by id.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct OneReceipt {
    pub receipt_id: String,
}

/// What is to be paid for: a receipt whole, or (`charge_id`) one of its
/// charges.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct PaidFor {
    pub receipt_id: String,
    pub charge_id: Option<String>,
}

/// The register for a filter: a saved one's (`saved`, whose conditions
/// are read from it, so it follows a change to it), or these conditions —
/// a built-in filter's, or ones not saved. `month`, when said, keeps the
/// month's alone.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Filtered {
    #[serde(default)]
    pub saved: Option<Id<SavedFilter>>,
    #[serde(default)]
    pub filter: Filter,
    #[ark(check = "is_month_or_now", why = "a month, as 2026-10")]
    #[serde(default)]
    pub month: Option<String>,
    /// A search box's register, not saved: its text looked for in every
    /// field a register shows ([`crate::filters::Filter::finds`]). A saved
    /// filter's register, total and formulas never are.
    #[serde(default)]
    pub search: bool,
}

// * The views' rows

/// An account as the sidebar and its own page draw it.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[ark(ffi_view)]
pub struct AccountLine {
    pub id: Id<Account>,
    pub name: String,
    pub kind: String,
    pub on_budget: bool,
    pub closed: bool,
    pub pos: i64,
    /// Everything in it.
    pub balance: i64,
    /// What the bank has seen.
    pub cleared: i64,
    /// What it has not, yet.
    pub uncleared: i64,
    /// The linked bank's name and the account's last digits, when it is fed
    /// by one.
    pub bank: String,
    pub mask: String,
    /// The bank's own balance at its last import.
    pub bank_balance: Option<i64>,
    /// Transactions nobody has looked at yet.
    pub unapproved: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[ark(ffi_view)]
pub struct GroupLine {
    pub id: Id<Group>,
    pub name: String,
    pub pos: i64,
    pub hidden: bool,
}

/// One category in one month: what the month gave it, what was spent from
/// it, and what it has.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[ark(ffi_view)]
pub struct BudgetLine {
    pub category_id: Id<Category>,
    pub group_id: Id<Group>,
    pub group: String,
    pub group_pos: i64,
    pub name: String,
    pub pos: i64,
    pub hidden: bool,
    pub target: i64,
    pub assigned: i64,
    pub activity: i64,
    pub available: i64,
    /// What it had coming into the month (never below nothing).
    pub carried: i64,
    pub assigned_last_month: i64,
    pub spent_last_month: i64,
    /// The average a month spent from it over the three before this one.
    pub average_spent: i64,
    /// The card it is the payment category of, when it is one.
    pub card: Option<Id<Account>>,
    /// What that card owes at the end of the month; nothing for any other.
    pub debt: i64,
    /// What it lacks: of a card's debt, what its payment category does not
    /// cover; of any other's target, what the month has not given it.
    pub underfunded: i64,
    /// Card spending this month beyond what it had (positive): new debt,
    /// which the next month's ready to assign does not pay for.
    pub credit_overspent: i64,
    /// The formula the month's assignment follows, said ("10% ×
    /// SUM(ACME paychecks)"); blank when it is assigned by hand
    /// ([`crate::formulas`]). Named `rule` for the phones, which read it.
    #[serde(default)]
    pub rule: String,
    /// Its assignment follows that formula this month: `assigned` is worked
    /// out, and not anybody's to set or move money to or from.
    #[serde(default)]
    pub ruled: bool,
}

/// The month as a whole: what is left to assign, and its totals.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[ark(ffi_view)]
pub struct Summary {
    pub month: String,
    /// Every dollar that came in up to the end of the month, less what has
    /// been assigned through it and what was overspent before it.
    pub ready_to_assign: i64,
    /// What came in, to be assigned, in the month.
    pub income: i64,
    pub assigned: i64,
    pub activity: i64,
    pub available: i64,
    /// What categories were overspent by in the month before: taken from
    /// this month's money.
    pub overspent_last_month: i64,
    /// Categories overspent this month, and how many were assigned less
    /// than their target in it — those Auto-Assign would fund: a month a
    /// formula assigns is not counted — or are a card's payment short of
    /// its debt.
    pub overspent: i64,
    pub underfunded: i64,
}

/// A transaction as a register draws it.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[ark(ffi_view)]
pub struct TxnLine {
    pub id: Id<Txn>,
    pub account_id: Id<Account>,
    pub account: String,
    pub date: String,
    pub payee: String,
    pub category_id: Option<Id<Category>>,
    /// The category's name, or what the line is instead: "Ready to Assign",
    /// "Transfer", "Needs a category".
    pub category: String,
    pub group: String,
    pub memo: String,
    pub amount: i64,
    pub cleared: Cleared,
    pub approved: bool,
    pub transfer: bool,
    pub imported: bool,
    pub imported_payee: String,
    pub created_ms: i64,
    pub user_id: String,
    /// The receipt that paid for it (or one of whose charges did), blank
    /// when none; and how many photos of a receipt were taken for it.
    pub receipt: String,
    pub photos: i64,
    /// The other account of a transfer: the one its payee names.
    #[serde(default)]
    pub transfer_account_id: Option<Id<Account>>,
}

/// Somebody paid, and the category their last transaction was for.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[ark(ffi_view)]
pub struct PayeeLine {
    pub name: String,
    pub count: i64,
    pub category_id: Option<Id<Category>>,
}

/// A linked bank, and where its last import got to.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[ark(ffi_view)]
pub struct BankLine {
    pub name: String,
    pub institution: String,
    pub synced_ms: i64,
    pub added: i64,
    pub updated: i64,
    pub deleted: i64,
    pub locked: i64,
    pub accounts: i64,
    /// What Plaid said when the last import failed, and when; blank since
    /// one worked.
    pub error_code: String,
    pub error: String,
    pub failed_ms: i64,
    /// The bank wants its login again: a reconnect (`banks.reconnect`),
    /// not a retry, mends it.
    pub needs_login: bool,
    /// Its accounts waiting for somebody to say what each feeds.
    pub waiting: i64,
}

/// An ask of the person reading, about a bank connection, and where it
/// stands: newest first.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[ark(ffi_view)]
pub struct AskLine {
    pub id: Id<BankAsk>,
    /// `connect`, `reconnect`, `add_accounts` or `disconnect`.
    pub what: String,
    pub bank: String,
    pub institution: String,
    pub state: AskState,
    /// Plaid's page to open, while it waits.
    pub url: String,
    pub message: String,
    pub expires_ms: i64,
    pub asked_ms: i64,
    pub answered_ms: i64,
}

/// What a month's spending went on, by group.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[ark(ffi_view)]
pub struct SpendingLine {
    pub group_id: Id<Group>,
    pub group: String,
    pub pos: i64,
    /// Spent, as a positive number.
    pub spent: i64,
}

/// One of a bank's accounts, and the account here it feeds — none when it
/// feeds none yet, or is ignored.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[ark(ffi_view)]
pub struct LinkedLine {
    pub plaid_account_id: String,
    pub bank: String,
    pub account_id: Option<Id<Account>>,
    /// The account's name here, or nothing.
    pub account: String,
    pub name: String,
    pub mask: String,
    pub kind: String,
    /// The bank's own balance at its last import.
    pub balance: Option<i64>,
    pub ignored: bool,
    pub subtype: String,
    /// Neither feeding an account nor ignored: somebody is to say which.
    pub waiting: bool,
    /// What an account opened for it would be: `credit` for a card.
    pub opens_as: String,
}

/// A saved filter, with what its transactions add up to: every one it
/// matches, ever (within its own dates, if it has them).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
pub struct FilterLine {
    pub id: Id<SavedFilter>,
    pub name: String,
    pub pos: i64,
    pub filter: Filter,
    /// Its conditions, said shortly: "Checking, Savings · \"acme\" · money in".
    pub said: String,
    pub count: i64,
    pub total: i64,
    /// The categories whose formulas sum it, by name.
    pub used_by: Vec<String>,
}

/// What a register's transactions add up to: one row, or none for none.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
pub struct FilterTotal {
    pub count: i64,
    pub total: i64,
    /// Of that, what came in and what went out (negative).
    pub inflow: i64,
    pub outflow: i64,
}

/// One term of a formula in a month: its filter, its factor, and what the
/// filter's transactions added up to.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
pub struct TermLine {
    pub filter_id: Id<SavedFilter>,
    /// The filter's name; blank when it is gone.
    pub name: String,
    pub times: i64,
    pub per: i64,
    pub count: i64,
    pub sum: i64,
}

/// A category's formula as a month follows it ([`crate::formulas`]).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
pub struct FormulaLine {
    pub category_id: Id<Category>,
    /// The month this version of it started.
    pub from: String,
    pub terms: Vec<TermLine>,
    /// It, said: "10% × SUM(ACME paychecks)".
    pub text: String,
    /// What it gives the month: the terms' sum, rounded once.
    pub amount: i64,
}

/// What a formula not yet saved would assign in a month.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
pub struct FormulaPreview {
    pub month: String,
    pub terms: Vec<TermLine>,
    pub text: String,
    pub amount: i64,
}

/// A category in the whole tree.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
pub struct CategoryNode {
    pub id: Id<Category>,
    pub parent_id: Option<Id<Category>>,
    pub name: String,
    /// From its heading down: "Everyday › Groceries › Eggs".
    pub path: String,
    /// 0 a heading (a group), 1 an envelope, 2 and on a kind of one.
    pub depth: i64,
    /// It, or a category above it, is hidden.
    pub hidden: bool,
    /// Under the budget's own heading.
    pub income: bool,
    /// What the tree sorts by: each position from the heading down.
    pub order: String,
}

/// One line of a split transaction, as a register draws it.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
pub struct SplitLine {
    pub txn_id: Id<Txn>,
    pub line: i64,
    pub category_id: Option<Id<Category>>,
    /// The category's path, or "Needs a category".
    pub category: String,
    pub amount: i64,
    pub memo: String,
}

/// A receipt, and how far along its items are.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[ark(ffi_view)]
pub struct ReceiptLine {
    pub id: String,
    pub store: String,
    pub date: String,
    pub place: String,
    pub total: i64,
    pub items: i64,
    /// Items filed under a category.
    pub filed: i64,
    pub txn_id: Option<Id<Txn>>,
    /// The paying transaction's payee and account, when found.
    pub payee: String,
    pub account: String,
    /// The store's charges for it, and how many of them found what paid.
    pub charges: i64,
    pub charged: i64,
}

/// One charge of a receipt, and what paid it.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
pub struct ChargeLine {
    pub id: String,
    pub receipt_id: String,
    pub date: String,
    pub amount: i64,
    pub card: String,
    pub txn_id: Option<Id<Txn>>,
    pub payee: String,
    pub account: String,
}

/// A transaction that could be what paid for a receipt or a charge.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
pub struct CandidateLine {
    pub txn_id: Id<Txn>,
    pub date: String,
    pub payee: String,
    pub account: String,
    pub amount: i64,
    /// How far its amount is from what is to be paid, in cents.
    pub off_by: i64,
    /// Days from the receipt's or the charge's.
    pub days: i64,
}

/// A thing bought, wherever it was bought.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[ark(ffi_view)]
pub struct ItemLine {
    pub receipt_id: String,
    pub line: i64,
    pub store: String,
    pub date: String,
    pub place: String,
    pub name: String,
    pub sku: String,
    pub quantity: i64,
    pub unit: String,
    pub amount: i64,
    /// What one cost, in cents, our sign: per each, per gallon.
    pub unit_price: i64,
    pub category_id: Option<Id<Category>>,
    pub category: String,
}

/// A photo of a paper receipt taken (docs/plan-files.md): its file, made on
/// the device (`ark_peer::Peer::put_file`), and the transaction it was
/// taken for, when it was.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct AddPhoto {
    pub file: ark::file::FileRef,
    pub txn_id: Option<Id<Txn>>,
}

/// A receipt's photo, and where reading it stands: `reading` until the
/// server has, then the reading's `sure`, `settled` or `check`; `failed`
/// with why, while it is tried again.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[ark(ffi_view)]
pub struct PhotoLine {
    pub id: Id<Photo>,
    pub file: ark::file::FileRef,
    pub txn_id: Option<Id<Txn>>,
    pub receipt_id: String,
    pub taken_ms: i64,
    pub state: String,
    pub why: String,
}

/// A receipt written by hand, or one already read put right: its place,
/// day and items (each amount our sign, spent negative), and the
/// transaction it is for. `receipt_id` blank is a new one; the total is
/// the items'.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
#[ark(ffi)]
pub struct WriteReceipt {
    pub receipt_id: String,
    pub txn_id: Option<Id<Txn>>,
    #[ark(trim, len = "0..=80")]
    pub place: String,
    #[ark(check = "crate::dates::is_day", why = "a day, YYYY-MM-DD")]
    pub date: String,
    /// What was paid, our sign; none, the items' sum.
    pub total: Option<i64>,
    pub items: Vec<StoreItem>,
}
