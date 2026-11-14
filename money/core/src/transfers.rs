//! Transfers made of transactions already there. A card payment comes from
//! the banks as two transactions that know nothing of each other —
//! checking's "Payment to Chase", the card's "Payment Thank You" — and is
//! one transfer: the import joins such a pair as it brings the second half
//! ([`join_imported`]), and a person makes any transaction a transfer from
//! its edit form ([`make`]), paired with the matching one in the other
//! account when there is one, or given its other half there.
//!
//! Undone ([`unlink`]), a transfer keeps what the bank told of: a half
//! that is the bank's goes back to being a transaction of its own, waiting
//! to be looked at; a half made only for the transfer goes.

use ark::prelude::*;

use crate::dates::{add_days, days_between};
use crate::plaid::MATCH_DAYS;
use crate::schema::*;
use crate::transactions::{category_for, open_account};
use crate::types::*;

/// Whether one side of a transfer between `a` and `b` is for a category:
/// only the budget's side of money leaving it, or coming into it, from an
/// account off it.
pub fn for_budget(a: &Account, b: &Account) -> bool {
    a.on_budget && !b.on_budget
}

/// A transfer's other half let go: kept as a transaction of its own when
/// the bank told of it, else gone. `t` itself is the caller's to write.
pub fn unlink(db: &mut Db<Money>, t: &Txn) -> Result<(), Refusal> {
    let Some(pair) = t.transfer_id.and_then(|p| db.txn.get(p)) else {
        return Ok(());
    };
    match pair.imported_id {
        Some(_) => db.txn.update(Txn {
            transfer_account: None,
            transfer_id: None,
            approved: false,
            ..pair
        }),
        None => db.txn.remove(&pair),
    }
}

/// The transactions in `account` dated within a few days of `date` for
/// `-amount`, not a transfer already, the nearest first.
fn opposite(db: &Db<Money>, account: Option<Id<Account>>, not: Id<Txn>, date: &str, amount: i64) -> Vec<Txn> {
    let (lo, hi) = (add_days(date, -MATCH_DAYS), add_days(date, MATCH_DAYS + 1));
    let near = match account {
        Some(a) => db.txn.by(Txn::ACCOUNT_ID_DATE).between((a, lo), (a, hi)).all(),
        None => db.txn.by(Txn::DATE).between(lo, hi).all(),
    };
    let mut out: Vec<Txn> = near
        .into_iter()
        .filter(|t| t.id != not && t.amount == -amount && t.transfer_id.is_none())
        .collect();
    out.sort_by_key(|t| (days_between(&t.date, date).abs(), t.created_ms, t.id));
    out
}

/// Two transactions made one transfer: each names the other (and is drawn
/// as a transfer to it, its own payee kept for when it is let go), and
/// only a side the budget pays from or into an account off it keeps its
/// category.
fn join(db: &mut Db<Money>, a: Txn, b: Txn) -> Result<(), Refusal> {
    let (aa, ba) = (
        crate::accounts::account_of(db, a.account_id)?,
        crate::accounts::account_of(db, b.account_id)?,
    );
    let (a_id, b_id) = (a.id, b.id);
    db.txn.update(Txn {
        category_id: a.category_id.filter(|_| for_budget(&aa, &ba)),
        transfer_account: Some(ba.id),
        transfer_id: Some(b_id),
        ..a
    })?;
    db.txn.update(Txn {
        category_id: b.category_id.filter(|_| for_budget(&ba, &aa)),
        transfer_account: Some(aa.id),
        transfer_id: Some(a_id),
        ..b
    })
}

/// A transaction the bank just told of, joined with its other half when
/// that is already here: money out of one account on the budget and the
/// same money into another, both from the banks, within a few days of each
/// other, neither looked at yet. Both stay waiting to be looked at, now as
/// one transfer. Whether it joined.
pub fn join_imported(db: &mut Db<Money>, t: &Txn) -> Result<bool, Refusal> {
    let Some(here) = db.account.get(t.account_id).filter(|a| a.on_budget) else {
        return Ok(false);
    };
    if t.amount == 0 || t.transfer_id.is_some() {
        return Ok(false);
    }
    let pair = opposite(db, None, t.id, &t.date, t.amount).into_iter().find(|o| {
        o.account_id != here.id
            && o.imported_id.is_some()
            && !o.approved
            && o.cleared != Cleared::Reconciled
            && db.account.get(o.account_id).is_some_and(|a| a.on_budget && !a.closed)
    });
    let Some(pair) = pair else { return Ok(false) };
    join(db, t.clone(), pair)?;
    Ok(true)
}

/// A transaction made a transfer to `to`, as its edit form said: its old
/// transfer (if any) let go, the edit applied, and then paired with the
/// matching transaction in `to` — the nearest in date for the opposite
/// amount — or given its other half there.
pub fn make(ctx: &Ctx, db: &mut Db<Money>, t: Txn, account: &Account, to: Id<Account>, input: &EditTransaction) -> Result<(), Refusal> {
    let other = open_account(db, to)?;
    if other.id == account.id {
        return Err(Refusal::new("a transfer is between two accounts"));
    }
    unlink(db, &t)?;
    let here = Txn {
        date: input.date.clone(),
        memo: input.memo.clone(),
        amount: input.amount,
        approved: true,
        transfer_account: None,
        transfer_id: None,
        ..t
    };
    let there = match opposite(db, Some(other.id), here.id, &here.date, here.amount).into_iter().next() {
        Some(p) => p,
        None => {
            let p = Txn {
                id: ctx.new_id(),
                account_id: other.id,
                date: here.date.clone(),
                memo: here.memo.clone(),
                amount: -here.amount,
                cleared: Cleared::Uncleared,
                approved: true,
                created_ms: ctx.now,
                user_id: ctx.user().into(),
                ..Txn::default()
            };
            db.txn.insert(p.clone())?;
            p
        }
    };
    // The category said is for whichever side the budget pays from (or
    // into) the account off it.
    let side = |a: &Account, b: &Account| match for_budget(a, b) {
        true => category_for(db, a, input.category_id),
        false => Ok(None),
    };
    let here = Txn {
        category_id: side(account, &other)?,
        ..here
    };
    let there = Txn {
        category_id: side(&other, account)?,
        ..there
    };
    join(db, here, there)
}
