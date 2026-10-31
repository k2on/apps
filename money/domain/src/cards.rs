//! Credit cards, budgeted the way YNAB budgets them.
//!
//! A card is not money the budget has: it is money the budget owes. So
//! every card on the budget has a **payment category**, in the budget's
//! own "Credit Card Payments" group ([`card_group`]), which holds the money
//! set aside to pay it. The rules:
//!
//! - **Spending on a card from a category that has the money moves it to
//!   the card's payment category.** The category goes down as any
//!   spending takes it down; the payment category's activity rises by what
//!   the category covered — the money that was for groceries is now for
//!   the card that bought them.
//! - **Spending more than a category has is credit overspending: new
//!   debt.** The part the category could not cover moves nothing to the
//!   payment category. The category starts the next month at nothing, as
//!   any overspent category does — but unlike cash overspending, what it
//!   was short is *not* taken from the next month's ready to assign: no
//!   cash was spent. It is debt, and the payment category shows it as not
//!   covered (`underfunded`) until money is assigned to it.
//! - **A refund on a card into a category reverses it**: it first undoes
//!   the month's credit overspending in that category, and the rest moves
//!   back out of the payment category.
//! - **Paying the card is spending from its payment category.** A transfer
//!   from an account on the budget to the card takes the payment
//!   category's available down by what was paid (and a transfer the other
//!   way, a cash advance, puts it up: the cash it brought is for the debt).
//! - **Debt a card comes with is not the budget's money.** Its starting
//!   balance is filed under its payment category, where it moves nothing:
//!   it does not come off ready to assign, and shows as the payment
//!   category being underfunded by that much. Money assigned to the
//!   payment category pays debt down, as money assigned to any category is
//!   for it.
//!
//! As everywhere in the budget, nothing here is stored but what people
//! said: the payment category's activity is worked out by the views from
//! every category's transactions and assignments ([`Cards::covers`]), and
//! what the card owes from its own.

use std::cell::OnceCell;
use std::collections::BTreeMap;

use ark::prelude::*;

use crate::budget::{ensure_setup, fixed, ready_to_assign, History};
use crate::dates::month_end;
use crate::schema::*;

/// The budget's own group of payment categories: the same id on every
/// peer, as [`crate::budget::inflow`] is.
pub fn card_group() -> Id<Group> {
    fixed("credit card payments")
}

/// What the group is called.
pub const CARD_GROUP: &str = "Credit Card Payments";

/// A card's payment category: its id is the card's, so every peer that
/// opens the card makes the same one.
pub fn payment_category(card: Id<Account>) -> Id<Category> {
    fixed(&format!("card payment {}", card.hex()))
}

/// Whether an account is a card the budget pays: a credit account on it.
pub fn is_card(a: &Account) -> bool {
    a.kind == "credit" && a.on_budget
}

/// The card a payment category is for.
pub fn card_of(db: &Db<Money>, id: Id<Category>) -> Option<Account> {
    let c = db.category.get(id)?;
    if c.group_id != card_group() {
        return None;
    }
    db.account.scan().into_iter().find(|a| a.kind == "credit" && payment_category(a.id) == id)
}

/// Whether a category is a card's payment category.
pub fn is_payment(db: &Db<Money>, id: Id<Category>) -> bool {
    db.category.get(id).is_some_and(|c| c.group_id == card_group())
}

/// A card's payment category kept as the card stands: there, named as the
/// card is, while the card is on the budget; hidden once it is closed or
/// off it. Every function that opens, renames or closes an account calls
/// this, so no device has to guess whether another made it.
pub fn keep(ctx: &Ctx, db: &mut Db<Money>, account: &Account) -> Result<(), Refusal> {
    if account.kind != "credit" {
        return Ok(());
    }
    let id = payment_category(account.id);
    let hidden = account.closed || !account.on_budget;
    if let Some(c) = db.category.get(id) {
        if c.name != account.name || c.hidden != hidden {
            db.category.update(Category {
                name: account.name.clone(),
                hidden,
                ..c
            })?;
        }
        return Ok(());
    }
    if !account.on_budget {
        return Ok(());
    }
    ensure_setup(ctx, db)?;
    if !db.group.has(card_group()) {
        // Above every other group, as YNAB draws it.
        db.group.insert(Group {
            id: card_group(),
            name: CARD_GROUP.into(),
            pos: 0,
            hidden: false,
            income: false,
            created_ms: ctx.now,
            user_id: ctx.user().into(),
        })?;
    }
    let pos = db.category.by(Category::POS).whole().last().map_or(0, |c| c.pos) + 1;
    db.category.insert(Category {
        id,
        group_id: card_group(),
        name: account.name.clone(),
        pos,
        hidden,
        created_ms: ctx.now,
        user_id: ctx.user().into(),
        ..Category::default()
    })
}

/// What an account's starting balance is filed under: on the budget, money
/// is ready to assign — except a card's debt, which is its payment
/// category's (and so nobody's money); off it, nothing.
pub fn starting_category(account: &Account, balance: i64) -> Option<Id<Category>> {
    match (account.on_budget, is_card(account) && balance < 0) {
        (false, _) => None,
        (true, true) => Some(payment_category(account.id)),
        (true, false) => Some(ready_to_assign()),
    }
}

/// The budget's cards, read once for a view: which accounts they are, and
/// (worked out the first time it is asked for) what every category's
/// spending on each moved to its payment category, month by month.
pub struct Cards {
    /// The cards on the budget, by id.
    pub on_budget: BTreeMap<Id<Account>, Account>,
    /// Every card's payment category, on the budget or not.
    pub payments: BTreeMap<Id<Category>, Id<Account>>,
    covers: OnceCell<BTreeMap<(Id<Account>, String), i64>>,
}

impl Cards {
    pub fn of(db: &Db<Money>) -> Cards {
        let mut on_budget = BTreeMap::new();
        let mut payments = BTreeMap::new();
        for a in db.account.scan() {
            if a.kind != "credit" {
                continue;
            }
            payments.insert(payment_category(a.id), a.id);
            if a.on_budget {
                on_budget.insert(a.id, a);
            }
        }
        Cards {
            on_budget,
            payments,
            covers: OnceCell::new(),
        }
    }

    /// Whether an account is one of the budget's cards.
    pub fn is_card(&self, id: Id<Account>) -> bool {
        self.on_budget.contains_key(&id)
    }

    /// The card a category pays, when it is a payment category.
    pub fn card_for(&self, id: Id<Category>) -> Option<Id<Account>> {
        self.payments.get(&id).copied()
    }

    /// Per card and month, what the categories' spending on it moved to
    /// its payment category (refunds moving it back, negative): every
    /// category but the budget's own and the payment categories, walked
    /// month by month.
    pub fn covers(&self, db: &Db<Money>) -> &BTreeMap<(Id<Account>, String), i64> {
        self.covers.get_or_init(|| {
            let mut out: BTreeMap<(Id<Account>, String), i64> = BTreeMap::new();
            if self.on_budget.is_empty() {
                return out;
            }
            for c in db.category.scan() {
                if self.payments.contains_key(&c.id) || crate::budget::is_income(db, c.id) {
                    continue;
                }
                let h = History::spending(db, c.id, self);
                h.walk(None, |month, step| {
                    for (card, covered) in &step.covers {
                        *out.entry((*card, month.to_string())).or_default() += covered;
                    }
                });
            }
            out.retain(|_, v| *v != 0);
            out
        })
    }

    /// A payment category's history: what was assigned to it, what was
    /// spent from it by hand, what spending on its card moved to it, and
    /// what paid the card from the budget's other accounts.
    pub fn payment_history(&self, db: &Db<Money>, id: Id<Category>, card: Id<Account>) -> History {
        let mut h = History::default();
        for a in db.assigned.by(Assigned::CATEGORY_ID_MONTH).from(id).all() {
            h.month(&a.month).assigned += a.amount;
        }
        // Filed under it by hand (a payment from checking that is not a
        // transfer) — but on the card itself, it is debt and nothing else:
        // the card's starting balance, an interest charge filed so.
        for t in db.txn.by(Txn::CATEGORY_ID_DATE).from(Some(id)).all() {
            if t.account_id != card {
                h.month(&crate::dates::month_of(&t.date)).activity += t.amount;
            }
        }
        if !self.is_card(card) {
            return h;
        }
        for ((c, month), covered) in self.covers(db) {
            if *c == card {
                h.month(month).activity += covered;
            }
        }
        for t in db.txn.by(Txn::ACCOUNT_ID_DATE).from(card).all() {
            let Some(from) = t.transfer_account.and_then(|o| db.account.get(o)) else {
                continue;
            };
            if from.on_budget && from.id != card && t.category_id.is_none() {
                h.month(&crate::dates::month_of(&t.date)).activity -= t.amount;
            }
        }
        h
    }
}

/// What a card owes at the end of a month: nothing, when it is owed.
pub fn debt(db: &Db<Money>, card: Id<Account>, month: &str) -> i64 {
    let balance: i64 = db
        .txn
        .by(Txn::ACCOUNT_ID_DATE)
        .between((card, String::new()), (card, month_end(month)))
        .all()
        .iter()
        .map(|t| t.amount)
        .sum();
    (-balance).max(0)
}
