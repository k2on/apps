//! Banks, through Plaid: what ArkOS's `money-sync` did for Actual Budget,
//! with the budget now the domain's own.
//!
//! Two halves, as there were:
//!
//! - **What Plaid says, as an import** ([`Pulled`], [`import_of`]): Plaid's
//!   `/accounts/get` and `/transactions/sync` answers, read as they come
//!   (dollars as floats, money leaving positive) and made one [`Import`] in
//!   cents and our sign — `sync.rs`'s `entity` and `bank_balance`. A server's
//!   importer holds the access tokens (secrets, never in the log), pulls from
//!   each bank's cursor and authors one import per bank; it holds the
//!   [`crate::BANK`] role, which nobody else does.
//! - **The import itself** ([`import`]): `actual-import`'s `runImport`, as a
//!   mutator — so it runs, on every peer, against the budget as it is when
//!   the entry lands. Plaid's transaction id travels as `imported_id`, which
//!   is what makes a run idempotent. A pending transaction that posts arrives
//!   as a new one naming the pending one (and a removal of it); it is moved
//!   to the posted id in place, so a category somebody gave it while it was
//!   pending survives. One somebody typed in by hand first is matched rather
//!   than doubled. A reconciled one is never touched. The first import that
//!   sees an account opens it here, under the bank's name for it or the
//!   one the server was told, with the balance before the imported history
//!   as its starting balance; one the server was told not to follow is
//!   known and opens nothing.
//!
//! And **an ask** ([`sync`]): anybody in the household may ask the importer
//! to ask the banks now, as a row it sees (`money/server`'s `importer.rs`).

use std::collections::BTreeSet;

use ark::prelude::*;
use serde::Deserialize as De;

use crate::accounts::STARTING_BALANCE;
use crate::budget::{ensure_setup, ready_to_assign};
use crate::dates::{add_days, day_of_ms, days_between};
use crate::schema::*;
use crate::transactions::remembered;
use crate::types::*;

// * What Plaid says

/// One of an item's accounts, as `/accounts/get` has it.
#[derive(De, Clone, Debug, PartialEq)]
pub struct PlaidAccount {
    pub account_id: String,
    pub name: String,
    pub mask: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    pub subtype: Option<String>,
    #[serde(default)]
    pub balances: Balances,
}

#[derive(De, Clone, Debug, Default, PartialEq)]
pub struct Balances {
    pub current: Option<f64>,
}

/// A transaction as `/transactions/sync` has it: `amount` in dollars,
/// positive for money leaving the account.
#[derive(De, Clone, Debug, PartialEq)]
pub struct PlaidTransaction {
    pub transaction_id: String,
    pub account_id: String,
    pub amount: f64,
    pub date: String,
    pub name: String,
    #[serde(default)]
    pub merchant_name: Option<String>,
    #[serde(default)]
    pub original_description: Option<String>,
    #[serde(default)]
    pub pending: bool,
    #[serde(default)]
    pub pending_transaction_id: Option<String>,
}

#[derive(De, Clone, Debug, PartialEq)]
pub struct PlaidRemoved {
    pub transaction_id: String,
    pub account_id: String,
}

/// Every page of one `/transactions/sync` run from a cursor, and the cursor
/// after it.
#[derive(De, Clone, Debug, Default, PartialEq)]
pub struct Pulled {
    pub added: Vec<PlaidTransaction>,
    pub modified: Vec<PlaidTransaction>,
    pub removed: Vec<PlaidRemoved>,
    #[serde(alias = "next_cursor")]
    pub cursor: String,
}

/// Dollars as cents.
pub fn cents(amount: f64) -> i64 {
    (amount * 100.0).round() as i64
}

/// The bank's balance as the budget counts it: what you owe on a card or a
/// loan is negative.
pub fn bank_balance(account: &PlaidAccount, current: f64) -> i64 {
    let owed = account.kind == "credit" || account.kind == "loan";
    cents(if owed { -current } else { current })
}

/// A Plaid transaction in our sign, with the merchant as its payee where
/// Plaid knows one and the bank's own line kept beside it.
pub fn bank_txn(t: &PlaidTransaction) -> BankTxn {
    let non_empty = |s: &Option<String>| s.clone().filter(|s| !s.trim().is_empty());
    BankTxn {
        transaction_id: t.transaction_id.clone(),
        plaid_account_id: t.account_id.clone(),
        pending_transaction_id: t.pending_transaction_id.clone(),
        date: t.date.clone(),
        amount: cents(-t.amount),
        payee: non_empty(&t.merchant_name).unwrap_or_else(|| t.name.clone()),
        imported_payee: non_empty(&t.original_description).unwrap_or_else(|| t.name.clone()),
        pending: t.pending,
    }
}

/// One bank's run as the import its importer authors.
pub fn import_of(bank: &str, institution: &str, accounts: &[PlaidAccount], pulled: &Pulled) -> Import {
    Import {
        bank: bank.into(),
        institution: institution.into(),
        cursor: pulled.cursor.clone(),
        accounts: accounts
            .iter()
            .map(|a| BankAccount {
                plaid_account_id: a.account_id.clone(),
                name: a.name.clone(),
                mask: a.mask.clone().unwrap_or_default(),
                kind: a.kind.clone(),
                subtype: a.subtype.clone().unwrap_or_default(),
                balance: a.balances.current.map(|c| bank_balance(a, c)),
                called: String::new(),
                ignored: false,
            })
            .collect(),
        upserts: pulled.added.iter().chain(&pulled.modified).map(bank_txn).collect(),
        removed: pulled
            .removed
            .iter()
            .map(|r| BankRemoved {
                transaction_id: r.transaction_id.clone(),
                plaid_account_id: r.account_id.clone(),
            })
            .collect(),
    }
}

/// What sort of account a Plaid account is here.
pub fn kind_of(kind: &str, subtype: &str) -> &'static str {
    match (kind, subtype) {
        ("depository", "savings" | "money market" | "cd" | "hsa") => "savings",
        ("depository", _) => "checking",
        ("credit", _) => "credit",
        ("loan", _) => "loan",
        _ => "investment",
    }
}

// * The import

/// How far apart a typed-in transaction and the bank's may be dated and
/// still be one.
pub const MATCH_DAYS: i64 = 3;

/// What one import did, account by account.
#[derive(Default)]
struct Counts {
    added: i64,
    updated: i64,
    deleted: i64,
    locked: i64,
}

fn by_imported(db: &Db<Money>, id: &str) -> Option<Txn> {
    db.txn.by(Txn::IMPORTED_ID).from(Some(id.to_string())).first()
}

/// One bank's run of `/transactions/sync`, whole: its accounts opened or
/// brought up to date, its transactions applied, its cursor kept.
#[mutator("banks.import")]
pub fn import(ctx: &Ctx, db: &mut Db<Money>, input: Import) -> Result<(), Refusal> {
    if !ctx.has_role(crate::BANK) {
        return Err(Refusal::new("only the bank importer imports"));
    }
    ensure_setup(ctx, db)?;
    let previous = db.bank.get(&input.bank);
    db.bank.put(Bank {
        name: input.bank.clone(),
        institution: input.institution.clone(),
        cursor: input.cursor.clone(),
        synced_ms: ctx.now,
        user_id: ctx.user().into(),
        ..previous.unwrap_or_default()
    })?;
    for a in &input.accounts {
        follow(ctx, db, &input, a)?;
    }

    let superseded: BTreeSet<&str> = input.upserts.iter().filter_map(|u| u.pending_transaction_id.as_deref()).collect();
    let mut counts = Counts::default();
    for u in &input.upserts {
        let Some(account) = followed(db, &u.plaid_account_id) else { continue };
        upsert(ctx, db, &account, u, &superseded, &mut counts)?;
    }
    for r in &input.removed {
        if superseded.contains(r.transaction_id.as_str()) || followed(db, &r.plaid_account_id).is_none() {
            continue;
        }
        let Some(prior) = by_imported(db, &r.transaction_id) else { continue };
        if prior.cleared == Cleared::Reconciled {
            counts.locked += 1;
            continue;
        }
        if let Some(pair) = prior.transfer_id {
            db.txn.delete(pair)?;
        }
        db.txn.remove(&prior)?;
        counts.deleted += 1;
    }

    let bank = db.bank.get(&input.bank).expect("the bank just written");
    db.bank.update(Bank {
        added: counts.added,
        updated: counts.updated,
        deleted: counts.deleted,
        locked: counts.locked,
        ..bank
    })
}

/// The account a Plaid account feeds, when it is followed.
fn followed(db: &Db<Money>, plaid_account_id: &str) -> Option<Account> {
    let l = db.linked.get(plaid_account_id).filter(|l| !l.ignored)?;
    db.account.get(l.account_id?)
}

/// A Plaid account seen: brought up to date, or — the first time — opened
/// here under what the server calls it (the bank's name unless told), its
/// starting balance what the bank holds less the history this import
/// brings (posted only: the bank's balance leaves pending out). One the
/// server was told not to follow is known from then on and opens nothing.
/// What the server says applies at first sight only: after that, which
/// account a bank's feeds is the household's (`banks.link`, `banks.ignore`).
fn follow(ctx: &Ctx, db: &mut Db<Money>, input: &Import, a: &BankAccount) -> Result<(), Refusal> {
    if let Some(l) = db.linked.get(&a.plaid_account_id) {
        return db.linked.update(Linked {
            name: a.name.clone(),
            mask: a.mask.clone(),
            kind: a.kind.clone(),
            balance: a.balance,
            ..l
        });
    }
    if a.ignored {
        return db.linked.insert(Linked {
            plaid_account_id: a.plaid_account_id.clone(),
            bank: input.bank.clone(),
            account_id: None,
            name: a.name.clone(),
            mask: a.mask.clone(),
            kind: a.kind.clone(),
            balance: a.balance,
            ignored: true,
        });
    }
    let name = match a.called.trim() {
        "" => a.name.clone(),
        called => called.to_string(),
    };
    let kind = kind_of(&a.kind, &a.subtype);
    let on_budget = crate::accounts::on_budget_by_default(kind);
    let id: Id<Account> = ctx.new_id();
    let pos = db.account.by(Account::POS).whole().last().map_or(0, |a| a.pos) + 1;
    db.account.insert(Account {
        id,
        name,
        kind: kind.into(),
        on_budget,
        pos,
        created_ms: ctx.now,
        user_id: ctx.user().into(),
        ..Account::default()
    })?;
    db.linked.insert(Linked {
        plaid_account_id: a.plaid_account_id.clone(),
        bank: input.bank.clone(),
        account_id: Some(id),
        name: a.name.clone(),
        mask: a.mask.clone(),
        kind: a.kind.clone(),
        balance: a.balance,
        ignored: false,
    })?;
    let Some(balance) = a.balance else { return Ok(()) };
    let mine = input.upserts.iter().filter(|u| u.plaid_account_id == a.plaid_account_id);
    let posted: i64 = mine.clone().filter(|u| !u.pending).map(|u| u.amount).sum();
    let opening = balance - posted;
    let date = match mine.map(|u| u.date.as_str()).min() {
        Some(oldest) => add_days(oldest, -1),
        None => day_of_ms(ctx.now),
    };
    if opening == 0 {
        return Ok(());
    }
    db.txn.insert(Txn {
        id: ctx.new_id(),
        account_id: id,
        date,
        payee: STARTING_BALANCE.into(),
        category_id: on_budget.then(ready_to_assign),
        memo: "Balance before the first import".into(),
        amount: opening,
        cleared: Cleared::Cleared,
        approved: true,
        created_ms: ctx.now,
        user_id: ctx.user().into(),
        ..Txn::default()
    })
}

fn cleared(pending: bool) -> Cleared {
    match pending {
        true => Cleared::Uncleared,
        false => Cleared::Cleared,
    }
}

/// One transaction the bank added or changed.
fn upsert(ctx: &Ctx, db: &mut Db<Money>, account: &Account, u: &BankTxn, superseded: &BTreeSet<&str>, counts: &mut Counts) -> Result<(), Refusal> {
    let prior = by_imported(db, &u.transaction_id).or_else(|| u.pending_transaction_id.as_deref().and_then(|p| by_imported(db, p)));
    if let Some(prior) = prior {
        if prior.cleared == Cleared::Reconciled {
            counts.locked += 1;
            return Ok(());
        }
        // Known: the bank's facts brought up to date, and what somebody
        // chose — payee, category, memo — left alone.
        if let Some(pair) = prior.transfer_id.and_then(|p| db.txn.get(p)) {
            db.txn.update(Txn {
                amount: -u.amount,
                date: u.date.clone(),
                ..pair
            })?;
        }
        db.txn.update(Txn {
            imported_id: Some(u.transaction_id.clone()),
            imported_payee: u.imported_payee.clone(),
            amount: u.amount,
            date: u.date.clone(),
            cleared: cleared(u.pending),
            ..prior
        })?;
        counts.updated += 1;
        return Ok(());
    }
    // Never a pending transaction whose posted form is in the same run.
    if superseded.contains(u.transaction_id.as_str()) {
        return Ok(());
    }
    // Typed in by hand before the bank told of it: the same amount, in the
    // same account, within a few days, not yet the bank's.
    let lo = (account.id, add_days(&u.date, -MATCH_DAYS));
    let hi = (account.id, add_days(&u.date, MATCH_DAYS + 1));
    let typed = db
        .txn
        .by(Txn::ACCOUNT_ID_DATE)
        .between(lo, hi)
        .all()
        .into_iter()
        .filter(|t| t.imported_id.is_none() && t.amount == u.amount && t.cleared != Cleared::Reconciled)
        .min_by_key(|t| days_between(&t.date, &u.date).abs());
    if let Some(t) = typed {
        db.txn.update(Txn {
            imported_id: Some(u.transaction_id.clone()),
            imported_payee: u.imported_payee.clone(),
            cleared: cleared(u.pending),
            ..t
        })?;
        counts.updated += 1;
        return Ok(());
    }
    let category_id = match account.on_budget {
        true => remembered(db, &u.payee),
        false => None,
    };
    db.txn.insert(Txn {
        id: ctx.new_id(),
        account_id: account.id,
        date: u.date.clone(),
        payee: u.payee.clone(),
        category_id,
        amount: u.amount,
        cleared: cleared(u.pending),
        approved: false,
        imported_id: Some(u.transaction_id.clone()),
        imported_payee: u.imported_payee.clone(),
        created_ms: ctx.now,
        user_id: ctx.user().into(),
        ..Txn::default()
    })?;
    counts.added += 1;
    Ok(())
}

/// A bank's account made to feed `account_id`: every import after this
/// writes there. What the account it fed before already holds stays.
#[mutator("banks.link")]
pub fn link(_: &Ctx, db: &mut Db<Money>, input: LinkAccount) -> Result<(), Refusal> {
    let l = db
        .linked
        .get(&input.plaid_account_id)
        .ok_or_else(|| Refusal::new("no such bank account"))?;
    crate::accounts::account_of(db, input.account_id)?;
    if db.linked.by(Linked::ACCOUNT_ID).from(Some(input.account_id)).any() {
        return Err(Refusal::new("that account is fed by a bank already"));
    }
    db.linked.update(Linked {
        account_id: Some(input.account_id),
        ignored: false,
        ..l
    })
}

/// A bank's account followed again, or not: an ignored one's transactions
/// are dropped from every import after.
#[mutator("banks.ignore")]
pub fn ignore(_: &Ctx, db: &mut Db<Money>, input: IgnoreLinked) -> Result<(), Refusal> {
    let l = db
        .linked
        .get(&input.plaid_account_id)
        .ok_or_else(|| Refusal::new("no such bank account"))?;
    db.linked.update(Linked { ignored: input.ignored, ..l })
}

/// Ask the server's importer to ask the banks now: anybody in the
/// household may. The importer is a peer like any other and sees the ask
/// as a row; what it imports arrives as any import does, and a bank's
/// `synced_ms` moving is the answer.
#[mutator("banks.sync")]
pub fn sync(ctx: &Ctx, db: &mut Db<Money>, _: ()) -> Result<(), Refusal> {
    db.asked.put(Asked {
        user_id: ctx.user().into(),
        asked_ms: ctx.now,
    })
}

/// The latest ask of anybody's: when the importer was last asked to sync.
pub fn last_asked(db: &Db<Money>) -> Option<Asked> {
    db.asked.scan().into_iter().max_by_key(|a| a.asked_ms)
}

/// Every linked bank, and what its last import did.
#[view("banks.banks")]
pub fn banks(_: &Ctx, _: &Db<Money>, _: ()) -> Result<crate::budget::Plan<BankLine>, Refusal> {
    Ok(scan::<Money, Bank>().map_with(|db, b| BankLine {
        name: b.name.clone(),
        institution: b.institution.clone(),
        synced_ms: b.synced_ms,
        added: b.added,
        updated: b.updated,
        deleted: b.deleted,
        locked: b.locked,
        accounts: db.linked.scan().iter().filter(|l| l.bank == b.name).count() as i64,
    }))
}

/// Every account a bank has, whether it feeds one here and which, or is
/// ignored: what a household links (`banks.link`) and stops following
/// (`banks.ignore`) from.
#[view("banks.linked")]
pub fn linked(_: &Ctx, _: &Db<Money>, _: ()) -> Result<crate::budget::Plan<LinkedLine>, Refusal> {
    Ok(scan::<Money, Linked>().map_with(|db, l| LinkedLine {
        plaid_account_id: l.plaid_account_id.clone(),
        bank: l.bank.clone(),
        account_id: l.account_id,
        account: l.account_id.and_then(|a| db.account.get(a)).map(|a| a.name).unwrap_or_default(),
        name: l.name.clone(),
        mask: l.mask.clone(),
        kind: l.kind.clone(),
        balance: l.balance,
        ignored: l.ignored,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plaid's sign is the other way, and an owed balance is negative here.
    #[test]
    fn plaid_reads_in_our_sign() {
        let t: PlaidTransaction = serde_json::from_str(
            r#"{"transaction_id":"t1","account_id":"a1","amount":12.34,"date":"2026-10-01","name":"SQ *BLUE BOTTLE",
                "merchant_name":"Blue Bottle","original_description":"SQ *BLUE BOTTLE 1234","pending":true}"#,
        )
        .unwrap();
        let b = bank_txn(&t);
        assert_eq!(
            (b.amount, b.payee.as_str(), b.imported_payee.as_str(), b.pending),
            (-1234, "Blue Bottle", "SQ *BLUE BOTTLE 1234", true)
        );
        let card: PlaidAccount =
            serde_json::from_str(r#"{"account_id":"a2","name":"Card","mask":"9999","type":"credit","balances":{"current":410.5}}"#).unwrap();
        assert_eq!(bank_balance(&card, 410.5), -41050);
        assert_eq!(kind_of("depository", "savings"), "savings");
        assert_eq!(kind_of("credit", "credit card"), "credit");
    }
}
