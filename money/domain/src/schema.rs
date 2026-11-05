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
    /// The bank whose import last told of it (its `bank` row), blank for
    /// one typed in or imported before this was kept: what tells one
    /// connection's copy of a transaction from another's, where two feed
    /// one account ([`crate::plaid`]).
    #[serde(default)]
    pub imported_by: String,
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
    /// What Plaid said the last time the importer asked and it failed —
    /// `ITEM_LOGIN_REQUIRED` and its sentence — and when; blank since the
    /// last import that worked.
    #[serde(default)]
    pub error_code: String,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub failed_ms: i64,
    /// Somebody chose what one of its accounts feeds: the next import asks
    /// for the bank's whole history again, so what the choice follows
    /// arrives from the start (`crate::banks`).
    #[serde(default)]
    pub repull: bool,
    /// Plaid's id for its institution (`ins_127991`), when an import has
    /// said it: two logins to one bank share it.
    #[serde(default)]
    pub institution_id: String,
    /// Connected from the app and not imported yet: its first import folds
    /// it into another login to the same bank, if there is one
    /// ([`crate::plaid`]).
    #[serde(default)]
    pub fresh: bool,
    /// Accounts were just added to it at Plaid (`banks.add_accounts`): its
    /// next import opens every account it has not seen before.
    #[serde(default)]
    pub adding: bool,
    /// Another login to the same bank, connected since, carries every
    /// account this one fed or ignored: the importer removes this one at
    /// Plaid and drops it (`banks.retired`). Its imports change nothing.
    #[serde(default)]
    pub replaced_by: String,
}

/// One of a bank's accounts, as Plaid names it, and the account here it
/// feeds — made by the first import that saw it. `ignored` is an account
/// somebody said not to follow; its transactions are dropped. Neither
/// feeding nor ignored, it waits for somebody to say which
/// ([`crate::banks`]), and its transactions are dropped until then.
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
    /// Plaid's subtype: `checking`, `savings`, `credit card`.
    #[serde(default)]
    pub subtype: String,
    /// The day an import first saw it.
    #[serde(default)]
    pub seen: String,
    /// Transactions dated before this day are not imported into the
    /// account it feeds: an account kept by hand before it was linked
    /// already has them. Blank is every one.
    #[serde(default)]
    pub since: String,
    /// The account it feeds was opened for it and is owed its starting
    /// balance, which the next import from the bank's start brings.
    #[serde(default)]
    pub opening: bool,
    /// It feeds an account another connection fed before it, or feeds
    /// still: what it imports is matched to what that one imported
    /// rather than doubled ([`crate::plaid`]).
    #[serde(default)]
    pub seam: bool,
}

/// Where an ask about a bank connection stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize, Describe)]
#[cfg_attr(feature = "ffi", ark(ffi))]
pub enum AskState {
    /// Nobody has answered it yet: the server's importer will.
    #[default]
    Asked,
    /// A Plaid page is open for it at `url`, until `expires_ms`.
    Waiting,
    /// Done: the bank connected, logged in again, or disconnected.
    Done,
    /// It could not be done; `message` says why.
    Failed,
    /// Nobody finished Plaid's page in time.
    Expired,
}

/// Somebody asking the server to connect a bank, log in to one again
/// (Plaid's update mode) or disconnect one: what the server's importer
/// answers, here, since the bank's login is the server's alone. `what` is
/// `connect`, `reconnect`, `add_accounts` or `disconnect`.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
pub struct BankAsk {
    #[ark(key)]
    pub id: Id<BankAsk>,
    pub what: String,
    /// The bank asked about; for a connection, the one it made.
    pub bank: String,
    pub institution: String,
    pub state: AskState,
    /// Plaid's page, while it waits.
    pub url: String,
    pub message: String,
    pub expires_ms: i64,
    pub asked_ms: i64,
    pub answered_ms: i64,
    pub user_id: String,
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
#[allow(clippy::duplicated_attributes)]
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
#[ark(index(txn_id), index(category_id, date))]
pub struct Split {
    #[ark(key, refs(Txn))]
    pub txn_id: Id<Txn>,
    #[ark(key)]
    pub line: i64,
    /// None: a part nobody has said the purpose of yet (a receipt's item
    /// not yet filed), which leaves the transaction needing a category.
    #[ark(refs(Category))]
    pub category_id: Option<Id<Category>>,
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
#[ark(index(receipt_id), index(category_id), index(same))]
pub struct Item {
    #[ark(key, refs(Receipt))]
    pub receipt_id: String,
    #[ark(key)]
    pub line: i64,
    pub name: String,
    /// The store's number for it: an ASIN, a TCIN, a Costco item number;
    /// blank when the store gave none.
    pub sku: String,
    /// What remembering goes by — the same store's same thing:
    /// `<store>:<sku>`, else the store and the name, folded
    /// ([`crate::receipts::key_of`]).
    pub same: String,
    /// How many, in thousandths: two cartons are 2000, 11.482 gallons 11482.
    pub quantity: i64,
    /// `ea`, `gal`, `lb`.
    pub unit: String,
    /// Paid for it, our sign: spent is negative, a discount positive.
    pub amount: i64,
    #[ark(refs(Category))]
    pub category_id: Option<Id<Category>>,
}

/// One charge a store made for a receipt, as the store says it: Amazon
/// charges an order shipment by shipment, and refunds what comes back, so
/// one order is paid by several transactions. Each finds its own (`txn_id`)
/// the way a receipt does, and each is split by the receipt's items. A
/// receipt with charges is paid by them, not whole. Keyed by the store's
/// order, the day and the amount (and which of a day's same amounts), so
/// the same history said twice is the same charges; `receipt_id` names a
/// receipt that may not have arrived yet.
#[allow(clippy::duplicated_attributes)]
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
#[ark(index(receipt_id), index(txn_id), index(date))]
pub struct Charge {
    #[ark(key)]
    pub id: String,
    pub receipt_id: String,
    pub date: String,
    /// Our sign: a charge is negative, a refund positive.
    pub amount: i64,
    /// The card, as the store names it: `Visa ending in 4821`.
    pub card: String,
    #[ark(refs(Txn))]
    pub txn_id: Option<Id<Txn>>,
    pub user_id: String,
}

/// A photo of a paper receipt, kept for good (docs/plan-files.md): its
/// file, the transaction it was taken for when it was, and reading it — a
/// job (`ark::effect`) the server's importer claims, runs `receipt-ocr`
/// on and finishes with the receipt it wrote.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Row)]
#[ark(index(txn_id))]
#[effect(role = crate::BANK, lease_ms = 600_000, retry_ms = 60_000)]
pub struct Photo {
    #[ark(key)]
    pub id: Id<Photo>,
    pub file: ark::file::FileRef,
    #[ark(refs(Txn))]
    pub txn_id: Option<Id<Txn>>,
    pub taken_ms: i64,
    pub user_id: String,
    pub state: Job<PhotoRead>,
}

/// What reading a photo came to: the receipt written from it, and how
/// sure the reading is — `sure`, it adds up as read; `settled`, after the
/// readings' other answers; `check`, it does not add up.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
pub struct PhotoRead {
    pub receipt_id: String,
    pub state: String,
    pub changed: Vec<String>,
}

/// **No longer read** — kept because the schema only grows: what
/// assignment by rule stored before formulas ([`Formula`]) replaced it. A
/// row here assigns nothing now, and nothing writes one. It said how a
/// category's assignment was worked out from a month on: a share of what came
/// in, in that month, matching `matching` in the chosen accounts — or,
/// `manual`, what people assign it by hand again. One row per category and
/// the month it starts; a month reads the latest that starts at or before
/// it, so every month before the first is assigned by hand as it was.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
#[ark(index(category_id, from))]
pub struct Rule {
    #[ark(key)]
    pub from: String,
    #[ark(key, refs(Category))]
    pub category_id: Id<Category>,
    /// From this month the category is assigned by hand again: the rule
    /// before it stops.
    pub manual: bool,
    /// What a transaction's payee or the bank's own line for it holds,
    /// any case.
    pub matching: String,
    /// The on-budget accounts whose inflows count; none is every one.
    pub accounts: Vec<Id<Account>>,
    /// The share, exactly: `times / per` of what matched (10% is 10/100,
    /// 1.5 is 15/10).
    pub times: i64,
    pub per: i64,
    pub at_ms: i64,
    pub user_id: String,
}

/// Which way money moved: either, in, or out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize, Describe)]
pub enum Direction {
    #[default]
    /// Either way (not `Any`, which a Kotlin binding could not name).
    Either,
    /// Money in: a positive amount.
    In,
    /// Money out: a negative amount.
    Out,
}

/// Which transactions: conditions every one of which a transaction meets
/// ([`crate::filters`]); the default is every transaction. One type for
/// the register on screen, a saved filter, the built-in ones (every
/// account, what needs a look, one account) and what a formula sums — and
/// one matcher for all of them, [`Filter::matches`].
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
pub struct Filter {
    /// The accounts it reads; none is every account.
    pub accounts: Vec<Id<Account>>,
    /// Only accounts on the budget.
    pub on_budget: bool,
    /// What the payee or the bank's own line for it holds, any case; blank
    /// is anything.
    pub text: String,
    /// The categories; none is any. `None` among them is "no category".
    pub categories: Vec<Option<Id<Category>>>,
    pub direction: Direction,
    /// Transfers left out.
    pub no_transfers: bool,
    /// Only what needs a look: not yet approved, or on the budget and for
    /// no category (the inbox's condition).
    pub needs_look: bool,
    /// Dated on or after this day, and on or before this one, when said.
    pub from: Option<String>,
    pub through: Option<String>,
}

/// A filter somebody named and kept: in the sidebar under Filters, in
/// `pos` order, and what a formula's term sums ([`crate::formulas`]).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
#[ark(index(pos))]
pub struct SavedFilter {
    #[ark(key)]
    pub id: Id<SavedFilter>,
    pub name: String,
    pub pos: i64,
    pub filter: Filter,
    pub created_ms: i64,
    pub user_id: String,
}

/// One term of a formula: `times / per` (kept exactly: 10% is 1/10) of
/// what a saved filter's transactions add up to in the month.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
pub struct Term {
    pub filter_id: Id<SavedFilter>,
    pub times: i64,
    pub per: i64,
}

/// How a category's assignment is worked out from a month on, until a
/// later version says otherwise ([`crate::formulas`]): the sum of its
/// terms over the month, rounded once to the cent — or, `manual`, what
/// people assign it by hand again. One row per category and the month it
/// starts; a month reads the latest that starts at or before it, so every
/// month before the first is assigned by hand as it was.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
#[ark(index(category_id, from))]
pub struct Formula {
    #[ark(key)]
    pub from: String,
    #[ark(key, refs(Category))]
    pub category_id: Id<Category>,
    /// From this month the category is assigned by hand again.
    pub manual: bool,
    pub terms: Vec<Term>,
    /// What it is said as, when it was set through the phones' old
    /// `categories.set_rule` ("10% of income matching ACME in Checking");
    /// blank for one set as a formula, which is said by its terms.
    pub said: String,
    pub at_ms: i64,
    pub user_id: String,
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
    pub bank_ask: BankAsk,
    /// No longer read: assignment by rule became formulas.
    pub rule: Rule,
    pub saved_filter: SavedFilter,
    pub formula: Formula,
    pub split: Split,
    pub receipt: Receipt,
    pub item: Item,
    pub charge: Charge,
    pub photo: Photo,
}

ark_auth::install!(Money);
