//! The types: every record a function takes or answers. A refinement
//! (`#[ark(..)]`) is checked once, where bytes come in, and its words are
//! what a form shows.

use ark::prelude::*;

use crate::dates::{is_day, is_month};
use crate::schema::{Account, Category, Group, Txn};

/// How far the bank and the household agree on a transaction: not yet seen
/// at the bank, seen there, or agreed to as part of a reconciled balance —
/// which nothing changes afterwards, an import included.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize, Describe)]
#[cfg_attr(feature = "ffi", ark(ffi))]
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

fn is_kind(k: &str) -> bool {
    crate::accounts::KINDS.contains(&k)
}

// * Accounts

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct OpenAccount {
    #[ark(trim, non_empty, why = "an account needs a name")]
    #[ark(len = "0..=80")]
    pub name: String,
    #[ark(check = "is_kind", why = "checking, savings, cash, credit, loan or investment")]
    pub kind: String,
    pub on_budget: bool,
    /// What is in it today, in cents: what you owe on a card or a loan is
    /// negative.
    pub balance: i64,
    #[ark(check = "is_day", why = "a day, as 2026-10-26")]
    pub date: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct RenameAccount {
    pub id: Id<Account>,
    #[ark(trim, non_empty, why = "an account needs a name")]
    #[ark(len = "0..=80")]
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct CloseAccount {
    pub id: Id<Account>,
    pub closed: bool,
}

// * Groups and categories

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct AddGroup {
    #[ark(trim, non_empty, why = "a group needs a name")]
    #[ark(len = "0..=80")]
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct RenameGroup {
    pub id: Id<Group>,
    #[ark(trim, non_empty, why = "a group needs a name")]
    #[ark(len = "0..=80")]
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct HideGroup {
    pub id: Id<Group>,
    pub hidden: bool,
}

/// A group gone, and with it its categories: what was spent from them is
/// moved to `move_to`, or left for nothing.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct DeleteGroup {
    pub id: Id<Group>,
    pub move_to: Option<Id<Category>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct AddCategory {
    pub group_id: Id<Group>,
    #[ark(trim, non_empty, why = "a category needs a name")]
    #[ark(len = "0..=80")]
    pub name: String,
}

/// A kind of a category, under it: "Eggs" under "Groceries".
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct AddSubcategory {
    pub parent_id: Id<Category>,
    #[ark(trim, non_empty, why = "a category needs a name")]
    #[ark(len = "0..=80")]
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct RenameCategory {
    pub id: Id<Category>,
    #[ark(trim, non_empty, why = "a category needs a name")]
    #[ark(len = "0..=80")]
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct HideCategory {
    pub id: Id<Category>,
    pub hidden: bool,
}

/// A category moved: an envelope into a group (`group_id`, a heading),
/// a kind under another category (`group_id`, its new parent).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct MoveCategory {
    pub id: Id<Category>,
    pub group_id: Id<Group>,
    /// The category it goes in front of, in that group; none for its end.
    #[serde(default)]
    pub before: Option<Id<Category>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct MoveGroup {
    pub id: Id<Group>,
    /// The group it goes in front of; none for after every other.
    #[serde(default)]
    pub before: Option<Id<Group>>,
}

/// What a category wants each month, in cents; 0 for nothing.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct SetTarget {
    pub id: Id<Category>,
    #[ark(check = "at_least_0", why = "a target is never below nothing")]
    pub target: i64,
}

/// A category gone: what it was given and what was spent from it moved to
/// `move_to`, or (none) its spending left for nothing and its money back
/// to be assigned.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct DeleteCategory {
    pub id: Id<Category>,
    pub move_to: Option<Id<Category>>,
}

// * The budget

/// A category's assigned amount for a month, set.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Assign {
    #[ark(check = "is_month", why = "a month, as 2026-10")]
    pub month: String,
    pub category_id: Id<Category>,
    pub amount: i64,
}

/// Money moved between two categories in a month, or to and from what is
/// ready to assign (`None`).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
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
pub struct AutoAssign {
    #[ark(check = "is_month", why = "a month, as 2026-10")]
    pub month: String,
    pub how: Auto,
    /// Only these categories; empty is every one.
    pub only: Vec<Id<Category>>,
}

// * Transactions

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
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

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
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
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Txns {
    pub ids: Vec<Id<Txn>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Categorize {
    pub ids: Vec<Id<Txn>>,
    pub category_id: Option<Id<Category>>,
}

/// One line of a split: part of the transaction's amount, for a category
/// (none: not yet said).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
pub struct SplitPart {
    pub category_id: Option<Id<Category>>,
    pub amount: i64,
    #[serde(default)]
    pub memo: String,
}

/// A transaction split into `lines`, which add up to its amount; no lines
/// is unsplit, for no category until somebody gives it one.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
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
pub struct SetCleared {
    pub id: Id<Txn>,
    pub cleared: bool,
}

/// The bank says the account holds `balance` on `date`: every cleared
/// transaction becomes reconciled, and a difference is an adjustment.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Reconcile {
    pub account_id: Id<Account>,
    pub balance: i64,
    #[ark(check = "is_day", why = "a day, as 2026-10-26")]
    pub date: String,
}

// * The Plaid import

/// One account of a linked bank, as Plaid reported it at the import.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
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
pub struct BankRemoved {
    pub transaction_id: String,
    pub plaid_account_id: String,
}

/// One run of a bank's `/transactions/sync`, whole: what the server's
/// importer authors (`crate::plaid::import`).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Import {
    #[ark(trim, non_empty, why = "a bank needs a name")]
    pub bank: String,
    pub institution: String,
    pub cursor: String,
    pub accounts: Vec<BankAccount>,
    pub upserts: Vec<BankTxn>,
    pub removed: Vec<BankRemoved>,
}

/// A bank's account made to feed an account already here, in place of the
/// one its first import opened.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct LinkAccount {
    pub plaid_account_id: String,
    pub account_id: Id<Account>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct IgnoreLinked {
    pub plaid_account_id: String,
    pub ignored: bool,
}

// * Receipts

fn is_store(s: &str) -> bool {
    crate::stores::STORES.contains(&s)
}

/// A thing bought, as a store's adapter read it.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
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
pub struct ReceiptImport {
    #[ark(check = "is_store", why = "amazon, target or costco")]
    pub store: String,
    pub receipts: Vec<StoreReceipt>,
}

/// One item of one receipt.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
pub struct ItemRef {
    pub receipt_id: String,
    pub line: i64,
}

/// Items filed under a category (none: unfiled); every unfiled item with
/// the same key as one of them is filed with it.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct ClassifyItems {
    pub items: Vec<ItemRef>,
    pub category_id: Option<Id<Category>>,
}

/// A receipt said to be paid by a transaction (none: by none found yet).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct MatchReceipt {
    pub receipt_id: String,
    pub txn_id: Option<Id<Txn>>,
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

// * The views' rows

/// An account as the sidebar and its own page draw it.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[cfg_attr(feature = "ffi", ark(ffi_view))]
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
#[cfg_attr(feature = "ffi", ark(ffi_view))]
pub struct GroupLine {
    pub id: Id<Group>,
    pub name: String,
    pub pos: i64,
    pub hidden: bool,
}

/// One category in one month: what the month gave it, what was spent from
/// it, and what it has.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[cfg_attr(feature = "ffi", ark(ffi_view))]
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
}

/// The month as a whole: what is left to assign, and its totals.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[cfg_attr(feature = "ffi", ark(ffi_view))]
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
    /// than their target in it.
    pub overspent: i64,
    pub underfunded: i64,
}

/// A transaction as a register draws it.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[cfg_attr(feature = "ffi", ark(ffi_view))]
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
}

/// Somebody paid, and the category their last transaction was for.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[cfg_attr(feature = "ffi", ark(ffi_view))]
pub struct PayeeLine {
    pub name: String,
    pub count: i64,
    pub category_id: Option<Id<Category>>,
}

/// A linked bank, and where its last import got to.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[cfg_attr(feature = "ffi", ark(ffi_view))]
pub struct BankLine {
    pub name: String,
    pub institution: String,
    pub synced_ms: i64,
    pub added: i64,
    pub updated: i64,
    pub deleted: i64,
    pub locked: i64,
    pub accounts: i64,
}

/// What a month's spending went on, by group.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[cfg_attr(feature = "ffi", ark(ffi_view))]
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
#[cfg_attr(feature = "ffi", ark(ffi_view))]
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
}

/// A thing bought, wherever it was bought.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
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
