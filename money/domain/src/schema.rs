//! The tables. One budget per server: everybody the server lets in (a
//! household, by its OpenID provider) reads and writes all of it, so no
//! row is anybody's — `user_id` says who wrote it, and nothing more.
//!
//! Money is a whole number of cents, signed the way an account sees it:
//! what comes in is positive, what goes out negative. A day is
//! `YYYY-MM-DD` and a month `YYYY-MM` ([`crate::dates`]): text that sorts
//! as time does, which is what makes a month a range of an index.

use ark::prelude::*;

use crate::types::Cleared;

/// Somewhere money is: a checking account, a card, a jar of cash, a
/// mortgage. On budget, its money is the budget's to assign; off budget
/// (a loan, an investment) it is only tracked.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
#[ark(index(pos))]
pub struct Account {
    #[ark(key)]
    pub id: Id<Account>,
    pub name: String,
    /// What sort of account: `checking`, `savings`, `cash`, `credit`,
    /// `loan`, `investment` ([`crate::accounts::KINDS`]).
    pub kind: String,
    pub on_budget: bool,
    pub closed: bool,
    pub pos: i64,
    pub note: String,
    pub created_ms: i64,
    pub user_id: String,
}

// One column in two indexes reads to clippy as an attribute said twice, on
// `Category` and `Txn`.

/// A heading: a [`Category`] with no parent, which the budget draws as a
/// group. Its own table once; one tree now (`src/migrations.rs`).
pub type Group = Category;

/// Something money is for, in one tree. A category with no parent is a
/// heading ("Bills", "Everyday": what the budget draws as a group); one
/// under a heading is an envelope ("Groceries", "Rent": what a month gives
/// money to); one under an envelope, at any depth, is a kind of it
/// ("Groceries › Eggs"), whose spending is its envelope's. The one heading
/// marked `income` is the budget's own, holding
/// [`crate::budget::ready_to_assign`].
///
/// `target` is what an envelope wants set aside each month — assigned in
/// the month, whatever it carried in — 0 for nothing in particular.
#[allow(clippy::duplicated_attributes)]
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
#[ark(index(parent_id, pos), index(pos))]
pub struct Category {
    #[ark(key)]
    pub id: Id<Category>,
    #[ark(refs(Category))]
    pub parent_id: Option<Id<Category>>,
    pub name: String,
    pub pos: i64,
    pub hidden: bool,
    pub income: bool,
    pub target: i64,
    pub note: String,
    pub created_ms: i64,
    pub user_id: String,
}

/// What a month gave a category: one row per month and category that was
/// given anything, so a month nobody touched is no rows.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
#[ark(index(category_id, month))]
pub struct Assigned {
    #[ark(key)]
    pub month: String,
    #[ark(key, refs(Category))]
    pub category_id: Id<Category>,
    pub amount: i64,
    pub at_ms: i64,
    pub user_id: String,
}

/// Money moving: into or out of one account, on a day, to or from a
/// payee, for a category. A transfer is two of these, one in each account,
/// each naming the other (`transfer_id`); between two accounts on budget it
/// is for nothing, the money having only moved.
///
/// One the bank told of carries the bank's id for it (`imported_id`) and
/// what the bank called the payee; until somebody looks at it, it is not
/// `approved`.
#[allow(clippy::duplicated_attributes)]
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
#[ark(index(account_id, date), index(category_id, date), index(date), index(payee, date), index(imported_id))]
pub struct Txn {
    #[ark(key)]
    pub id: Id<Txn>,
    #[ark(refs(Account))]
    pub account_id: Id<Account>,
    pub date: String,
    pub payee: String,
    #[ark(refs(Category))]
    pub category_id: Option<Id<Category>>,
    pub memo: String,
    pub amount: i64,
    pub cleared: Cleared,
    pub approved: bool,
    #[ark(refs(Account))]
    pub transfer_account: Option<Id<Account>>,
    pub transfer_id: Option<Id<Txn>>,
    pub imported_id: Option<String>,
    pub imported_payee: String,
    pub created_ms: i64,
    pub user_id: String,
    /// Split: what it was for is its [`Split`] lines, and `category_id`
    /// is none. Defaulted, for the rows written before splits.
    #[serde(default)]
    pub split: bool,
}

/// A bank connected through Plaid (an "item"), by the short name it was
/// linked under. Its access token is a secret of the server's and never
/// here; what is here is where the last import got to and what it did.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
pub struct Bank {
    #[ark(key)]
    pub name: String,
    pub institution: String,
    /// Plaid's cursor after the last import: the next one asks from here.
    pub cursor: String,
    pub synced_ms: i64,
    pub added: i64,
    pub updated: i64,
    pub deleted: i64,
    /// Reconciled transactions the last import left alone.
    pub locked: i64,
    pub user_id: String,
}

/// One of a bank's accounts, as Plaid names it, and the account here it
/// feeds — made by the first import that saw it. `ignored` is an account
/// somebody said not to follow; its transactions are dropped.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
#[ark(index(account_id))]
pub struct Linked {
    #[ark(key)]
    pub plaid_account_id: String,
    #[ark(refs(Bank))]
    pub bank: String,
    #[ark(refs(Account))]
    pub account_id: Option<Id<Account>>,
    pub name: String,
    pub mask: String,
    pub kind: String,
    /// The bank's own balance at the last import, in cents, our sign.
    pub balance: Option<i64>,
    pub ignored: bool,
}

/// Somebody asking the server's importer to ask the banks now, rather
/// than at its next hour: a person's latest ask, one row each. The
/// importer answers an ask newer than the last one it answered, by
/// importing every bank it holds a token for.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
pub struct Asked {
    #[ark(key)]
    pub user_id: String,
    pub asked_ms: i64,
}

/// One line of a split transaction: part of its amount, for a category of
/// its own. A split is by amounts, whatever told them — somebody typing, or
/// a receipt's items ([`crate::receipts`]) — and its lines add up to the
/// transaction's amount. Each carries its transaction's date, so a month's
/// spending in a category is a range of an index, as the transactions' is.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
#[ark(index(category_id, date))]
pub struct Split {
    #[ark(key, refs(Txn))]
    pub txn_id: Id<Txn>,
    #[ark(key)]
    pub line: i64,
    #[ark(refs(Category))]
    pub category_id: Id<Category>,
    pub date: String,
    pub amount: i64,
    pub memo: String,
}

/// A store's receipt or order, as one of its adapters read it
/// ([`crate::stores`]): Amazon's order history, Target's, Costco's
/// warehouse and gas receipts. Keyed `<store>:<the store's id for it>`, so
/// the same export imported twice is the same receipt. `txn_id` is the
/// bank's transaction that paid for it, once one is found.
#[allow(clippy::duplicated_attributes)]
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
#[ark(index(store, date), index(txn_id))]
pub struct Receipt {
    #[ark(key)]
    pub id: String,
    /// `amazon`, `target`, `costco`.
    pub store: String,
    pub date: String,
    /// Which warehouse, store or site.
    pub place: String,
    /// What was paid, our sign: spent is negative.
    pub total: i64,
    #[ark(refs(Txn))]
    pub txn_id: Option<Id<Txn>>,
    pub imported_ms: i64,
    pub user_id: String,
}

/// One line of a receipt: a thing bought, as the store printed it, and the
/// category it is filed under — the same "Eggs" whichever store sold them.
#[allow(clippy::duplicated_attributes)]
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
#[ark(index(category_id), index(key))]
pub struct Item {
    #[ark(key, refs(Receipt))]
    pub receipt_id: String,
    #[ark(key)]
    pub line: i64,
    pub name: String,
    /// The store's number for it: an ASIN, a TCIN, a Costco item number;
    /// blank when the store gave none.
    pub sku: String,
    /// What remembering goes by: `<store>:<sku>`, else the store and the
    /// name, folded ([`crate::receipts::key_of`]).
    pub key: String,
    /// How many, in thousandths: two cartons are 2000, 11.482 gallons 11482.
    pub quantity: i64,
    /// `ea`, `gal`, `lb`.
    pub unit: String,
    /// Paid for it, our sign: spent is negative, a discount positive.
    pub amount: i64,
    #[ark(refs(Category))]
    pub category_id: Option<Id<Category>>,
}

/// money's tables, in the schema's order.
#[derive(Schema)]
pub struct Money {
    pub account: Account,
    pub category: Category,
    pub assigned: Assigned,
    pub txn: Txn,
    pub bank: Bank,
    pub linked: Linked,
    pub asked: Asked,
    pub sign_in: ark_auth::SignIn,
    pub role: ark_auth::Role,
    pub split: Split,
    pub receipt: Receipt,
    pub item: Item,
}

ark_auth::install!(Money);
