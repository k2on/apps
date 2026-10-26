//! Accounts: where the money is. One opened by hand starts with its balance
//! as a transaction of its own; one a bank feeds is opened by its first
//! import ([`crate::plaid`]).

use ark::prelude::*;

use crate::budget::{ensure_setup, ready_to_assign, Plan};
use crate::schema::*;
use crate::types::*;

/// The sorts of account there are.
pub const KINDS: [&str; 6] = ["checking", "savings", "cash", "credit", "loan", "investment"];

/// Whether an account of this sort belongs in the budget unless somebody
/// says otherwise: what is spent from it is spent from the budget.
pub fn on_budget_by_default(kind: &str) -> bool {
    matches!(kind, "checking" | "savings" | "cash" | "credit")
}

/// What the first transaction of an account is called.
pub const STARTING_BALANCE: &str = "Starting Balance";

/// An account, after every other, and its first transaction: `balance` on
/// `date`. On budget, that money is ready to assign.
#[allow(clippy::too_many_arguments)]
pub fn open(
    ctx: &Ctx,
    db: &mut Db<Money>,
    id: Id<Account>,
    name: &str,
    kind: &str,
    on_budget: bool,
    balance: i64,
    date: &str,
) -> Result<(), Refusal> {
    ensure_setup(ctx, db)?;
    let pos = db.account.by(Account::POS).whole().last().map_or(0, |a| a.pos) + 1;
    db.account.insert(Account {
        id,
        name: name.into(),
        kind: kind.into(),
        on_budget,
        closed: false,
        pos,
        note: String::new(),
        created_ms: ctx.now,
        user_id: ctx.user().into(),
    })?;
    db.txn.insert(Txn {
        id: ctx.new_id(),
        account_id: id,
        date: date.into(),
        payee: STARTING_BALANCE.into(),
        category_id: on_budget.then(ready_to_assign),
        amount: balance,
        cleared: Cleared::Cleared,
        approved: true,
        created_ms: ctx.now,
        user_id: ctx.user().into(),
        ..Txn::default()
    })
}

#[mutator("accounts.open_account")]
pub fn open_account(ctx: &Ctx, db: &mut Db<Money>, input: OpenAccount) -> Result<(), Refusal> {
    open(ctx, db, ctx.new_id(), &input.name, &input.kind, input.on_budget, input.balance, &input.date)
}

pub fn account_of(db: &Db<Money>, id: Id<Account>) -> Result<Account, Refusal> {
    db.account.get(id).ok_or_else(|| Refusal::new("no such account"))
}

#[mutator("accounts.rename_account")]
pub fn rename_account(_: &Ctx, db: &mut Db<Money>, input: RenameAccount) -> Result<(), Refusal> {
    let a = account_of(db, input.id)?;
    db.account.update(Account { name: input.name, ..a })
}

/// An account closed, or open again. One with money in it stays open: the
/// money has to go somewhere first.
#[mutator("accounts.close_account")]
pub fn close_account(_: &Ctx, db: &mut Db<Money>, input: CloseAccount) -> Result<(), Refusal> {
    let a = account_of(db, input.id)?;
    if input.closed && balances(db, a.id).0 != 0 {
        return Err(Refusal::new("an account is closed with nothing in it"));
    }
    db.account.update(Account { closed: input.closed, ..a })
}

/// An account's balance, what of it has cleared, and how many of its
/// transactions are waiting to be looked at.
pub fn balances(db: &Db<Money>, id: Id<Account>) -> (i64, i64, i64) {
    let (mut all, mut cleared, mut unapproved) = (0, 0, 0);
    for t in db.txn.by(Txn::ACCOUNT_ID_DATE).from(id).all() {
        all += t.amount;
        if t.cleared != Cleared::Uncleared {
            cleared += t.amount;
        }
        unapproved += (!t.approved) as i64;
    }
    (all, cleared, unapproved)
}

/// Every account, in order, with its balances and the bank that feeds it.
#[view("accounts.accounts")]
pub fn accounts(_: &Ctx, _: &Db<Money>, _: ()) -> Result<Plan<AccountLine>, Refusal> {
    Ok(scan::<Money, Account>()
        .map_with(|db, a| {
            let (balance, cleared, unapproved) = balances(db, a.id);
            let linked = db.linked.by(Linked::ACCOUNT_ID).from(Some(a.id)).first();
            AccountLine {
                id: a.id,
                name: a.name.clone(),
                kind: a.kind.clone(),
                on_budget: a.on_budget,
                closed: a.closed,
                pos: a.pos,
                balance,
                cleared,
                uncleared: balance - cleared,
                bank: linked.as_ref().map(|l| l.bank.clone()).unwrap_or_default(),
                mask: linked.as_ref().map(|l| l.mask.clone()).unwrap_or_default(),
                bank_balance: linked.and_then(|l| l.balance),
                unapproved,
            }
        })
        .sort(|a| (a.pos, a.id)))
}
