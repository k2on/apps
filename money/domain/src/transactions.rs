//! Transactions: money moving, as somebody entered it or the bank told of
//! it. A transfer is two, one in each account, kept together: whatever is
//! done to one is done to the other.

use ark::prelude::*;

use crate::accounts::account_of;
use crate::budget::{ensure_setup, ready_to_assign, Plan};
use crate::schema::*;
use crate::types::*;

/// What a reconciliation that did not agree adds, to make it.
pub const ADJUSTMENT: &str = "Reconciliation Balance Adjustment";

/// A category a transaction in `account` may be for: on budget, any that
/// is (or none yet); off budget, none — it is only tracked.
fn category_for(db: &Db<Money>, account: &Account, category: Option<Id<Category>>) -> Result<Option<Id<Category>>, Refusal> {
    match category {
        None => Ok(None),
        Some(_) if !account.on_budget => Err(Refusal::new("an account off the budget is only tracked: its money is for no category")),
        Some(c) if db.category.has(c) => Ok(Some(c)),
        Some(_) => Err(Refusal::new("no such category")),
    }
}

/// An account money may be written into.
fn open_account(db: &Db<Money>, id: Id<Account>) -> Result<Account, Refusal> {
    let a = account_of(db, id)?;
    if a.closed {
        return Err(Refusal::new(format!("{} is closed", a.name)));
    }
    Ok(a)
}

fn cleared(c: bool) -> Cleared {
    match c {
        true => Cleared::Cleared,
        false => Cleared::Uncleared,
    }
}

/// A transaction, or a transfer's two. Between two accounts on budget a
/// transfer is for nothing; out of the budget, its budget side is for a
/// category like any spending.
#[mutator("transactions.add_transaction")]
pub fn add_transaction(ctx: &Ctx, db: &mut Db<Money>, input: AddTransaction) -> Result<(), Refusal> {
    ensure_setup(ctx, db)?;
    let account = open_account(db, input.account_id)?;
    let base = Txn {
        account_id: account.id,
        date: input.date.clone(),
        payee: input.payee.clone(),
        memo: input.memo.clone(),
        amount: input.amount,
        cleared: cleared(input.cleared),
        approved: true,
        created_ms: ctx.now,
        user_id: ctx.user().into(),
        ..Txn::default()
    };
    let Some(to) = input.transfer_to else {
        let category_id = category_for(db, &account, input.category_id)?;
        return db.txn.insert(Txn {
            id: ctx.new_id(),
            category_id,
            ..base
        });
    };
    let other = open_account(db, to)?;
    if other.id == account.id {
        return Err(Refusal::new("a transfer is between two accounts"));
    }
    let (here, there): (Id<Txn>, Id<Txn>) = (ctx.new_id(), ctx.new_id());
    let for_budget = |a: &Account, b: &Account| a.on_budget && !b.on_budget;
    let here_category = match for_budget(&account, &other) {
        true => category_for(db, &account, input.category_id)?,
        false => None,
    };
    let there_category = match for_budget(&other, &account) {
        true => category_for(db, &other, input.category_id)?,
        false => None,
    };
    db.txn.insert(Txn {
        id: here,
        category_id: here_category,
        transfer_account: Some(other.id),
        transfer_id: Some(there),
        payee: String::new(),
        ..base.clone()
    })?;
    db.txn.insert(Txn {
        id: there,
        account_id: other.id,
        category_id: there_category,
        amount: -input.amount,
        cleared: Cleared::Uncleared,
        transfer_account: Some(account.id),
        transfer_id: Some(here),
        payee: String::new(),
        ..base
    })
}

fn txn_of(db: &Db<Money>, id: Id<Txn>) -> Result<Txn, Refusal> {
    db.txn.get(id).ok_or_else(|| Refusal::new("no such transaction"))
}

/// A transaction as somebody corrected it; a transfer's other half follows
/// its date, amount and memo. A reconciled one keeps its amount: the bank
/// agreed to it.
#[mutator("transactions.edit_transaction")]
pub fn edit_transaction(_: &Ctx, db: &mut Db<Money>, input: EditTransaction) -> Result<(), Refusal> {
    let t = txn_of(db, input.id)?;
    if t.cleared == Cleared::Reconciled && t.amount != input.amount {
        return Err(Refusal::new("reconciled: its amount is agreed with the bank"));
    }
    let account = account_of(db, t.account_id)?;
    let category_id = match t.transfer_account.and_then(|a| db.account.get(a)) {
        Some(other) if !(account.on_budget && !other.on_budget) => None,
        _ => category_for(db, &account, input.category_id)?,
    };
    let payee = match t.transfer_id {
        Some(_) => String::new(),
        None => input.payee,
    };
    if let Some(pair) = t.transfer_id.and_then(|p| db.txn.get(p)) {
        db.txn.update(Txn {
            date: input.date.clone(),
            amount: -input.amount,
            memo: input.memo.clone(),
            ..pair
        })?;
    }
    db.txn.update(Txn {
        date: input.date,
        payee,
        category_id,
        memo: input.memo,
        amount: input.amount,
        approved: true,
        ..t
    })
}

/// Transactions gone, each with its transfer's other half.
#[mutator("transactions.delete_transactions")]
pub fn delete_transactions(_: &Ctx, db: &mut Db<Money>, input: Txns) -> Result<(), Refusal> {
    for id in input.ids {
        let Some(t) = db.txn.get(id) else { continue };
        if let Some(pair) = t.transfer_id {
            db.txn.delete(pair)?;
        }
        db.txn.remove(&t)?;
    }
    Ok(())
}

/// Transactions the bank told of, looked at and accepted as they are.
#[mutator("transactions.approve")]
pub fn approve(_: &Ctx, db: &mut Db<Money>, input: Txns) -> Result<(), Refusal> {
    for id in input.ids {
        if let Some(t) = db.txn.get(id) {
            db.txn.update(Txn { approved: true, ..t })?;
        }
    }
    Ok(())
}

/// Transactions given a category (and so looked at). A transfer between
/// two accounts on budget is for nothing, and stays so.
#[mutator("transactions.categorize")]
pub fn categorize(_: &Ctx, db: &mut Db<Money>, input: Categorize) -> Result<(), Refusal> {
    for id in input.ids {
        let t = txn_of(db, id)?;
        let account = account_of(db, t.account_id)?;
        if let Some(other) = t.transfer_account.and_then(|a| db.account.get(a)) {
            if !(account.on_budget && !other.on_budget) {
                continue;
            }
        }
        let category_id = category_for(db, &account, input.category_id)?;
        db.txn.update(Txn {
            category_id,
            approved: true,
            ..t
        })?;
    }
    Ok(())
}

/// A transaction marked as seen at the bank, or not. A reconciled one stays
/// as it is.
#[mutator("transactions.set_cleared")]
pub fn set_cleared(_: &Ctx, db: &mut Db<Money>, input: SetCleared) -> Result<(), Refusal> {
    let t = txn_of(db, input.id)?;
    if t.cleared == Cleared::Reconciled {
        return Err(Refusal::new("reconciled: it stays cleared"));
    }
    db.txn.update(Txn {
        cleared: cleared(input.cleared),
        ..t
    })
}

/// The bank's balance agreed to: an adjustment for any difference from
/// what has cleared here, and everything cleared then reconciled.
#[mutator("transactions.reconcile")]
pub fn reconcile(ctx: &Ctx, db: &mut Db<Money>, input: Reconcile) -> Result<(), Refusal> {
    ensure_setup(ctx, db)?;
    let account = open_account(db, input.account_id)?;
    let (_, cleared_balance, _) = crate::accounts::balances(db, account.id);
    let difference = input.balance - cleared_balance;
    if difference != 0 {
        db.txn.insert(Txn {
            id: ctx.new_id(),
            account_id: account.id,
            date: input.date,
            payee: ADJUSTMENT.into(),
            category_id: account.on_budget.then(ready_to_assign),
            amount: difference,
            cleared: Cleared::Cleared,
            approved: true,
            created_ms: ctx.now,
            user_id: ctx.user().into(),
            ..Txn::default()
        })?;
    }
    for t in db.txn.by(Txn::ACCOUNT_ID_DATE).from(account.id).all() {
        if t.cleared == Cleared::Cleared {
            db.txn.update(Txn {
                cleared: Cleared::Reconciled,
                ..t
            })?;
        }
    }
    Ok(())
}

// * The views

/// A transaction as a register draws it: names in place of ids, and what
/// its category column says when it has none.
pub fn txn_line(db: &Db<Money>, t: &Txn) -> TxnLine {
    let account = db.account.get(t.account_id).unwrap_or_default();
    let other = t.transfer_account.and_then(|a| db.account.get(a));
    let category = t.category_id.and_then(|c| db.category.get(c));
    let group = category.as_ref().and_then(|c| db.group.get(c.group_id));
    let payee = match &other {
        Some(o) => format!("Transfer : {}", o.name),
        None => t.payee.clone(),
    };
    let label = match (&category, &other) {
        (Some(c), _) => c.name.clone(),
        (None, Some(_)) => String::new(),
        (None, None) if account.on_budget => "Needs a category".into(),
        (None, None) => String::new(),
    };
    TxnLine {
        id: t.id,
        account_id: t.account_id,
        account: account.name,
        date: t.date.clone(),
        payee,
        category_id: t.category_id,
        category: label,
        group: group.filter(|g| !g.income).map(|g| g.name).unwrap_or_default(),
        memo: t.memo.clone(),
        amount: t.amount,
        cleared: t.cleared,
        approved: t.approved,
        transfer: other.is_some(),
        imported: t.imported_id.is_some(),
        imported_payee: t.imported_payee.clone(),
        created_ms: t.created_ms,
        user_id: t.user_id.clone(),
    }
}

/// One account's transactions, or every account's, the newest first.
#[view("transactions.register")]
pub fn register(_: &Ctx, _: &Db<Money>, input: Register) -> Result<Plan<TxnLine>, Refusal> {
    let only = input.account_id;
    Ok(scan::<Money, Txn>()
        .filter(move |t| only.is_none_or(|a| t.account_id == a))
        .map_with(txn_line)
        .sort(|l| (Desc(l.date.clone()), Desc(l.created_ms), l.id)))
}

/// What needs somebody to look at it: imported and not yet approved, or
/// on budget and for no category.
#[view("transactions.inbox")]
pub fn inbox(_: &Ctx, _: &Db<Money>, _: ()) -> Result<Plan<TxnLine>, Refusal> {
    Ok(scan::<Money, Txn>()
        .map_with(txn_line)
        .filter(|l| !l.approved || l.category == "Needs a category")
        .sort(|l| (Desc(l.date.clone()), Desc(l.created_ms), l.id)))
}

/// Everybody paid, by name, with the category their newest transaction was
/// for: what a payee field offers, and what it fills the category with.
#[view("transactions.payees")]
pub fn payees(_: &Ctx, _: &Db<Money>, _: ()) -> Result<Plan<PayeeLine>, Refusal> {
    Ok(scan::<Money, Txn>().filter(|t| !t.payee.is_empty() && t.transfer_id.is_none()).group(
        |t| t.payee.clone(),
        |_, name, txns| {
            let txns: Vec<&Txn> = txns.collect();
            let newest = txns.iter().filter(|t| t.category_id.is_some()).max_by_key(|t| (&t.date, t.created_ms));
            PayeeLine {
                name: name.clone(),
                count: txns.len() as i64,
                category_id: newest.and_then(|t| t.category_id),
            }
        },
    ))
}

/// The category `payee`'s newest categorised transaction was for: what a new
/// one from them is filed under until somebody says otherwise.
pub fn remembered(db: &Db<Money>, payee: &str) -> Option<Id<Category>> {
    if payee.is_empty() {
        return None;
    }
    db.txn
        .by(Txn::PAYEE_DATE)
        .from(payee.to_string())
        .rev()
        .all()
        .into_iter()
        .find_map(|t| t.category_id.filter(|c| *c != ready_to_assign()))
}
