//! The budget: every dollar given a job, a month at a time.
//!
//! The rules are a zero-based budget's, the ones YNAB made familiar:
//!
//! - **Money that comes in is ready to assign.** An inflow for the budget's
//!   own category, [`ready_to_assign`], adds to it; giving a category money
//!   in a month ([`assign`]) takes from it.
//! - **A category's money stays until it is spent.** What a category has
//!   (`available`) is what it carried in, plus what the month assigned it,
//!   plus the month's activity (spending is negative). What is left at the
//!   end of a month carries into the next.
//! - **Overspending is paid for by next month.** A category that ends a
//!   month below nothing starts the next at nothing, and what it was short
//!   comes out of that month's ready to assign.
//! - **A card is money owed, not money had** ([`crate::cards`]): what a
//!   category covers of spending on a card moves to the card's payment
//!   category, and what it cannot is debt — overspending the next month's
//!   ready to assign does not pay for.
//! - **A category may be assigned by formula** ([`crate::formulas`]): from
//!   a month on, a sum of shares of what saved filters match in the month,
//!   as a spreadsheet's cell is. Its amount is assigned as a typed one is,
//!   everywhere; nobody assigns it by hand.
//!
//! Nothing here is stored but what people said — the assignments and the
//! transactions. Every figure is worked out by the views from them, so a
//! transaction that arrives late (an import, a phone that was offline)
//! moves every month after it, on every device, the same way.

use std::collections::BTreeMap;

use ark::prelude::*;

use crate::cards::Cards;
use crate::dates::{add_months, month_end, month_of};
use crate::schema::*;
use crate::types::*;

/// A plan over money's tables.
pub type Plan<T> = ark::plan::Plan<Money, T>;

pub(crate) fn fixed<T>(what: &str) -> Id<T> {
    let h = ark::hash::sha256(&[b"money ", what.as_bytes()]);
    Id::new(h[..16].try_into().unwrap())
}

/// The budget's own group, which holds only [`ready_to_assign`]: the same id
/// on every peer, so that any two that set the budget up make one.
pub fn inflow() -> Id<Group> {
    fixed("inflow")
}

/// The category money coming in is for: categorised so, a transaction adds
/// to what is ready to assign.
pub fn ready_to_assign() -> Id<Category> {
    fixed("ready to assign")
}

/// The budget's own group and category, when it has not got them. Every
/// function that may need them calls this first, so that no device has to
/// guess whether another already did.
pub fn ensure_setup(ctx: &Ctx, db: &mut Db<Money>) -> Result<(), Refusal> {
    if db.category.has(ready_to_assign()) {
        return Ok(());
    }
    if !db.category.has(inflow()) {
        db.category.insert(Category {
            id: inflow(),
            name: "Inflow".into(),
            hidden: true,
            income: true,
            created_ms: ctx.now,
            user_id: ctx.user().into(),
            ..Category::default()
        })?;
    }
    db.category.insert(Category {
        id: ready_to_assign(),
        parent_id: Some(inflow()),
        name: "Ready to Assign".into(),
        pos: 0,
        created_ms: ctx.now,
        user_id: ctx.user().into(),
        ..Category::default()
    })
}

/// A new budget's own group and category; again, nothing.
#[mutator("budget.setup")]
pub fn setup(ctx: &Ctx, db: &mut Db<Money>, _: ()) -> Result<(), Refusal> {
    ensure_setup(ctx, db)
}

// * The tree

/// How deep the tree may go: a parent's chain longer than this is a loop
/// (which no mutator makes) and is read as ending there.
pub const DEEPEST: usize = 16;

/// A category and every one above it, itself first and its heading last.
pub fn ancestry(db: &Db<Money>, id: Id<Category>) -> Vec<Category> {
    let mut out: Vec<Category> = vec![];
    let mut at = db.category.get(id);
    while let Some(c) = at {
        if out.len() == DEEPEST {
            break;
        }
        at = c.parent_id.and_then(|p| db.category.get(p));
        out.push(c);
    }
    out
}

/// How far below its heading a category is: 0 is a heading (a group), 1 an
/// envelope, 2 and on a kind of one. None for no such category.
pub fn depth(db: &Db<Money>, id: Id<Category>) -> Option<usize> {
    ancestry(db, id).len().checked_sub(1)
}

/// The heading a category is under (itself, for a heading).
pub fn heading_of(db: &Db<Money>, id: Id<Category>) -> Option<Category> {
    ancestry(db, id).pop()
}

/// The envelope a category's money is: itself, or the one it is a kind of.
/// None for a heading.
pub fn envelope_of(db: &Db<Money>, id: Id<Category>) -> Option<Category> {
    let mut a = ancestry(db, id);
    a.pop();
    a.pop()
}

/// A category's children, in order.
pub fn children(db: &Db<Money>, id: Id<Category>) -> Vec<Category> {
    db.category.by(Category::PARENT_ID_POS).from(Some(id)).all()
}

/// A category and every one below it, itself first.
pub fn subtree(db: &Db<Money>, id: Id<Category>) -> Vec<Category> {
    let Some(top) = db.category.get(id) else { return vec![] };
    let mut out = vec![top];
    let mut i = 0;
    while i < out.len() && out.len() < 10_000 {
        let next = children(db, out[i].id);
        out.extend(next);
        i += 1;
    }
    out
}

/// The categories from a heading down to `id`, by name: "Everyday ›
/// Groceries › Eggs".
pub fn path_of(db: &Db<Money>, id: Id<Category>) -> String {
    let mut names: Vec<String> = ancestry(db, id).into_iter().map(|c| c.name).collect();
    names.reverse();
    names.join(" › ")
}

/// Whether a category is the budget's own (money in, not a job for it).
pub fn is_income(db: &Db<Money>, id: Id<Category>) -> bool {
    heading_of(db, id).is_some_and(|g| g.income)
}

/// A category money can be assigned to: an envelope, and not the budget's
/// own. A kind of one is given money through it.
pub fn budgetable(db: &Db<Money>, id: Id<Category>) -> Result<Category, Refusal> {
    let c = db.category.get(id).ok_or_else(|| Refusal::new("no such category"))?;
    if is_income(db, id) {
        return Err(Refusal::new("money is assigned from Ready to Assign, not to it"));
    }
    match depth(db, id) {
        Some(1) => Ok(c),
        Some(0) => Err(Refusal::new("money is assigned to a category, not a group")),
        _ => Err(Refusal::new(format!("money for {} is assigned to the category it is a kind of", c.name))),
    }
}

/// What was spent from (or came into) one category itself, between two
/// days: its transactions' amounts and its split lines'. Not its kinds'.
pub fn own_activity(db: &Db<Money>, id: Id<Category>, from: &str, to: &str) -> i64 {
    let txns: i64 = db
        .txn
        .by(Txn::CATEGORY_ID_DATE)
        .between((Some(id), from.to_string()), (Some(id), to.to_string()))
        .all()
        .iter()
        .map(|t| t.amount)
        .sum();
    let splits: i64 = db
        .split
        .by(Split::CATEGORY_ID_DATE)
        .between((Some(id), from.to_string()), (Some(id), to.to_string()))
        .all()
        .iter()
        .map(|s| s.amount)
        .sum();
    txns + splits
}

/// What a month gave a category.
pub fn assigned_in(db: &Db<Money>, month: &str, id: Id<Category>) -> i64 {
    db.assigned.get((month.to_string(), id)).map_or(0, |a| a.amount)
}

/// A category's assignment for a month, set: no row for nothing.
fn set_assigned(ctx: &Ctx, db: &mut Db<Money>, month: &str, id: Id<Category>, amount: i64) -> Result<(), Refusal> {
    let key = (month.to_string(), id);
    if amount == 0 {
        return db.assigned.delete(key);
    }
    db.assigned.put(Assigned {
        month: month.into(),
        category_id: id,
        amount,
        at_ms: ctx.now,
        user_id: ctx.user().into(),
    })
}

/// Give a category an amount for a month, in place of what it had.
#[mutator("budget.assign")]
pub fn assign(ctx: &Ctx, db: &mut Db<Money>, input: Assign) -> Result<(), Refusal> {
    budgetable(db, input.category_id)?;
    crate::formulas::refuse(db, input.category_id, &input.month)?;
    set_assigned(ctx, db, &input.month, input.category_id, input.amount)
}

/// Move money from one category to another within a month; `None` on
/// either side is ready to assign. The amount is said once, so it is the
/// same on both sides however the categories stood when it lands.
#[mutator("budget.move_money")]
pub fn move_money(ctx: &Ctx, db: &mut Db<Money>, input: MoveMoney) -> Result<(), Refusal> {
    if input.from == input.to {
        return Err(Refusal::new("money moves between two places"));
    }
    if let Some(from) = input.from {
        budgetable(db, from)?;
        crate::formulas::refuse(db, from, &input.month)?;
        let had = assigned_in(db, &input.month, from);
        set_assigned(ctx, db, &input.month, from, had - input.amount)?;
    }
    if let Some(to) = input.to {
        budgetable(db, to)?;
        crate::formulas::refuse(db, to, &input.month)?;
        let had = assigned_in(db, &input.month, to);
        set_assigned(ctx, db, &input.month, to, had + input.amount)?;
    }
    Ok(())
}

/// A month assigned at once: last month's amounts, or what each target
/// still lacks, or nothing. "Underfunded" gives a card's payment category
/// what it lacks of the card's debt — after every other category, since
/// what they cover of their card spending is money the payment category
/// then already has.
#[mutator("budget.auto_assign")]
pub fn auto_assign(ctx: &Ctx, db: &mut Db<Money>, input: AutoAssign) -> Result<(), Refusal> {
    let last = add_months(&input.month, -1);
    let all = db.category.by(Category::POS).whole().all();
    let (payments, others): (Vec<Category>, Vec<Category>) = all
        .into_iter()
        .filter(|c| budgetable(db, c.id).is_ok() && (input.only.is_empty() || input.only.contains(&c.id)))
        // A month that follows a formula is not anybody's to assign.
        .filter(|c| crate::formulas::formula_in(db, c.id, &input.month).is_none())
        .partition(|c| crate::cards::is_payment(db, c.id));
    for c in others.into_iter().chain(payments) {
        let had = assigned_in(db, &input.month, c.id);
        let amount = match input.how {
            Auto::LastMonth => crate::formulas::assigned_of(db, &last, c.id),
            Auto::Underfunded if crate::cards::is_payment(db, c.id) => had + line(db, &c, &input.month).underfunded,
            Auto::Underfunded => had.max(c.target),
            Auto::Reset => 0,
        };
        set_assigned(ctx, db, &input.month, c.id, amount)?;
    }
    Ok(())
}

// * The arithmetic

/// What one month did to one category: what it assigned, what was spent
/// from it (activity, spending negative), and of that, what was on each of
/// the budget's cards ([`crate::cards`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Moves {
    pub assigned: i64,
    pub activity: i64,
    pub cards: BTreeMap<Id<Account>, i64>,
}

/// One category's history, month by month.
#[derive(Default)]
pub struct History {
    pub months: BTreeMap<String, Moves>,
}

/// One month of a category, walked: what it had, and how what it was short
/// (if it was) splits between cash and a card.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Step {
    pub carried: i64,
    pub moves: Moves,
    pub available: i64,
    /// Cash spent beyond what it had (positive): the next month's ready to
    /// assign pays for it.
    pub cash_overspent: i64,
    /// Card spending beyond what it had (positive): new debt, which the
    /// card's payment category shows as not covered.
    pub credit_overspent: i64,
    /// Per card, what the month's spending on it moved to its payment
    /// category (a refund moving it back is negative).
    pub covers: BTreeMap<Id<Account>, i64>,
}

impl Step {
    /// A month, from what it carried in and what it did.
    pub fn of(carried: i64, moves: &Moves) -> Step {
        let on_cards: i64 = moves.cards.values().sum();
        let available = carried + moves.assigned + moves.activity;
        // What it had before its card spending: cash overspending is what
        // that left it short, and any more is the cards'.
        let before_cards = available - on_cards;
        let (cash, credit) = match available < 0 {
            true => {
                let cash = (-available).min((-before_cards).max(0));
                (cash, -available - cash)
            }
            false => (0, 0),
        };
        // A refunding card moves what it brought back; the spending cards
        // move what was covered, the uncovered part laid on them in turn.
        let mut uncovered = credit;
        let mut covers = BTreeMap::new();
        for (card, net) in &moves.cards {
            let short = match *net < 0 {
                true => uncovered.min(-net),
                false => 0,
            };
            uncovered -= short;
            covers.insert(*card, -net - short);
        }
        Step {
            carried,
            moves: moves.clone(),
            available,
            cash_overspent: cash,
            credit_overspent: credit,
            covers,
        }
    }
}

impl History {
    /// Read from its assignments, and the transactions and split lines for
    /// it or for any kind of it: a payment category's from its card's too
    /// ([`crate::cards::Cards::payment_history`]).
    pub fn of(db: &Db<Money>, id: Id<Category>) -> History {
        History::with(db, id, &Cards::of(db))
    }

    /// [`History::of`], with the cards already read.
    pub fn with(db: &Db<Money>, id: Id<Category>, cards: &Cards) -> History {
        match cards.card_for(id) {
            Some(card) => cards.payment_history(db, id, card),
            None => History::spending(db, id, cards),
        }
    }

    /// A category's own history: its assignments, and its transactions
    /// and split lines and its kinds', with what was spent on a card kept
    /// apart.
    pub fn spending(db: &Db<Money>, id: Id<Category>, cards: &Cards) -> History {
        let mut h = History::assigned(db, id);
        for c in subtree(db, id) {
            for t in db.txn.by(Txn::CATEGORY_ID_DATE).from(Some(c.id)).all() {
                let m = h.month(&month_of(&t.date));
                m.activity += t.amount;
                if cards.is_card(t.account_id) {
                    *m.cards.entry(t.account_id).or_default() += t.amount;
                }
            }
            for s in db.split.by(Split::CATEGORY_ID_DATE).from(Some(c.id)).all() {
                let account = db.txn.get(s.txn_id).map(|t| t.account_id);
                let m = h.month(&month_of(&s.date));
                m.activity += s.amount;
                if let Some(a) = account.filter(|a| cards.is_card(*a)) {
                    *m.cards.entry(a).or_default() += s.amount;
                }
            }
        }
        h
    }

    /// Its assignments alone: typed, or its formula's ([`crate::formulas::assignments`]).
    pub fn assigned(db: &Db<Money>, id: Id<Category>) -> History {
        let mut h = History::default();
        for (month, amount) in crate::formulas::assignments(db, id) {
            h.month(&month).assigned += amount;
        }
        h
    }

    /// What it was assigned in every month through `month`.
    pub fn assigned_through(&self, month: &str) -> i64 {
        self.months.range(..=month.to_string()).map(|(_, m)| m.assigned).sum()
    }

    /// A month's moves, to add to.
    pub fn month(&mut self, month: &str) -> &mut Moves {
        self.months.entry(month.to_string()).or_default()
    }

    /// Every month from its first through `upto` (or its last), in order.
    pub fn walk(&self, upto: Option<&str>, mut f: impl FnMut(&str, &Step)) {
        let Some(first) = self.months.keys().next().cloned() else { return };
        let last = match upto {
            Some(m) => m.to_string(),
            None => self.months.keys().next_back().cloned().unwrap_or_default(),
        };
        let none = Moves::default();
        let mut carry = 0;
        let mut k = first;
        while k <= last {
            let step = Step::of(carry, self.months.get(&k).unwrap_or(&none));
            f(&k, &step);
            carry = step.available.max(0);
            k = add_months(&k, 1);
        }
    }

    /// The month as this category stood in it.
    pub fn at(&self, month: &str) -> Standing {
        let mut s = Standing::default();
        let (last, three) = (add_months(month, -1), add_months(month, -3));
        self.walk(Some(month), |k, step| {
            if k == month {
                (s.carried, s.assigned, s.activity) = (step.carried, step.moves.assigned, step.moves.activity);
                (s.available, s.credit_overspent) = (step.available, step.credit_overspent);
                return;
            }
            s.overspent_before -= step.cash_overspent;
            if k == last {
                s.overspent_last_month = -step.cash_overspent;
                (s.assigned_last_month, s.spent_last_month) = (step.moves.assigned, step.moves.activity);
            }
            if k >= three.as_str() {
                s.spent_last_three += step.moves.activity;
            }
        });
        s
    }
}

/// One category in one month.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Standing {
    pub carried: i64,
    pub assigned: i64,
    pub activity: i64,
    pub available: i64,
    /// What its cash overspending took from ready to assign, in every
    /// month before (negative). Credit overspending took nothing.
    pub overspent_before: i64,
    /// Of that, the month before's.
    pub overspent_last_month: i64,
    /// This month's card spending it could not cover (positive): new debt.
    pub credit_overspent: i64,
    pub assigned_last_month: i64,
    pub spent_last_month: i64,
    pub spent_last_three: i64,
}

/// A category's line in a month.
pub fn line(db: &Db<Money>, c: &Category, month: &str) -> BudgetLine {
    line_with(db, c, month, &Cards::of(db))
}

/// [`line`], with the cards already read.
pub fn line_with(db: &Db<Money>, c: &Category, month: &str, cards: &Cards) -> BudgetLine {
    let s = History::with(db, c.id, cards).at(month);
    let g = heading_of(db, c.id).unwrap_or_default();
    let card = cards.card_for(c.id);
    let debt = card.map_or(0, |k| crate::cards::debt(db, k, month));
    // A payment category lacks what the card owes beyond what it has; any
    // other, what its target wants beyond what the month gave it.
    let underfunded = match card {
        Some(_) => (debt - s.available).max(0),
        None if c.target > 0 => (c.target - s.assigned).max(0),
        None => 0,
    };
    let formula = crate::formulas::formula_in(db, c.id, month);
    BudgetLine {
        category_id: c.id,
        group_id: g.id,
        group: g.name,
        group_pos: g.pos,
        name: c.name.clone(),
        pos: c.pos,
        hidden: c.hidden || g.hidden,
        target: c.target,
        assigned: s.assigned,
        activity: s.activity,
        available: s.available,
        carried: s.carried,
        assigned_last_month: s.assigned_last_month,
        spent_last_month: s.spent_last_month,
        average_spent: s.spent_last_three / 3,
        card,
        debt,
        underfunded,
        credit_overspent: s.credit_overspent,
        ruled: formula.is_some(),
        rule: formula.map_or_else(String::new, |f| crate::formulas::said_of(db, &f)),
    }
}

/// The month as a whole, from every category.
pub fn summary_of(db: &Db<Money>, categories: impl Iterator<Item = Category>, month: &str) -> Summary {
    let mut out = Summary {
        month: month.into(),
        ..Summary::default()
    };
    // What every category was assigned through the month, typed or by
    // its formula, is read from its history below.
    let mut assigned_through = 0;
    let rta = ready_to_assign();
    let mut income_through = 0;
    for t in db
        .txn
        .by(Txn::CATEGORY_ID_DATE)
        .between((Some(rta), String::new()), (Some(rta), month_end(month)))
        .all()
    {
        income_through += t.amount;
        if month_of(&t.date) == month {
            out.income += t.amount;
        }
    }
    for s in db
        .split
        .by(Split::CATEGORY_ID_DATE)
        .between((Some(rta), String::new()), (Some(rta), month_end(month)))
        .all()
    {
        income_through += s.amount;
        if month_of(&s.date) == month {
            out.income += s.amount;
        }
    }
    let cards = Cards::of(db);
    let mut overspent_before = 0;
    for c in categories {
        if c.id == rta || budgetable(db, c.id).is_err() {
            continue;
        }
        let h = History::with(db, c.id, &cards);
        assigned_through += h.assigned_through(month);
        let s = h.at(month);
        overspent_before += s.overspent_before;
        out.overspent_last_month += s.overspent_last_month;
        out.assigned += s.assigned;
        out.activity += s.activity;
        out.available += s.available;
        out.overspent += (s.available < 0) as i64;
        // What Auto-Assign would fund: a month its formula assigns is
        // nobody's to fund by hand, so it is not counted short.
        out.underfunded += match cards.card_for(c.id) {
            Some(card) => crate::cards::debt(db, card, month) > s.available,
            None => s.assigned < c.target && crate::formulas::formula_in(db, c.id, month).is_none(),
        } as i64;
    }
    out.ready_to_assign = income_through - assigned_through + overspent_before;
    out
}

// * The views

/// The groups the budget draws, in order; not the budget's own.
#[view("budget.groups")]
pub fn groups(_: &Ctx, _: &Db<Money>, _: ()) -> Result<Plan<GroupLine>, Refusal> {
    Ok(scan::<Money, Category>()
        .filter(|g| g.parent_id.is_none() && !g.income)
        .sort(|g| (g.pos, g.id))
        .map(|g| GroupLine {
            id: g.id,
            name: g.name.clone(),
            pos: g.pos,
            hidden: g.hidden,
        }))
}

/// Every envelope in one month, in the budget's order. A line is kept by
/// the rows it read — its own assignments, and its and its kinds'
/// transactions and split lines — so a transaction moves the one line it is
/// for.
#[view("budget.month")]
pub fn month(_: &Ctx, _: &Db<Money>, input: Month) -> Result<Plan<BudgetLine>, Refusal> {
    let month = input.month;
    Ok(scan::<Money, Category>()
        .filter_with(|db, c| budgetable(db, c.id).is_ok())
        .map_with(move |db, c| line(db, c, &month))
        .sort(|l| (l.group_pos, l.group_id, l.pos, l.category_id)))
}

/// The month as a whole, one row: what is ready to assign, and the totals.
#[view("budget.summary")]
pub fn summary(_: &Ctx, _: &Db<Money>, input: Month) -> Result<Plan<Summary>, Refusal> {
    let month = input.month;
    Ok(scan::<Money, Category>().group(|_| 0i64, move |db, _, cats| summary_of(db, cats.cloned(), &month)))
}

/// What a month's spending went on, group by group, the most first. Paying
/// a card is not spending (what it bought was, from its own category), so
/// the card payments' group is not here.
#[view("budget.spending")]
pub fn spending(_: &Ctx, _: &Db<Money>, input: Month) -> Result<Plan<SpendingLine>, Refusal> {
    let month = input.month;
    Ok(scan::<Money, Category>()
        .filter_with(|db, c| c.parent_id.is_some() && !is_income(db, c.id))
        .map_with(|db, c| (heading_of(db, c.id).map_or(c.id, |g| g.id), c.id))
        .filter(|(heading, _)| *heading != crate::cards::card_group())
        .group(
            |(heading, _)| *heading,
            move |db, group, cats| {
                let g = db.category.get(*group).unwrap_or_default();
                let (lo, hi) = (crate::dates::month_start(&month), month_end(&month));
                let spent: i64 = cats.map(|(_, c)| own_activity(db, *c, &lo, &hi)).sum();
                SpendingLine {
                    group_id: *group,
                    group: g.name,
                    pos: g.pos,
                    spent: -spent,
                }
            },
        )
        .filter(|s| s.spent > 0)
        .sort(|s| (Desc(s.spent), s.pos)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history(months: &[(&str, i64, i64)]) -> History {
        History {
            months: months
                .iter()
                .map(|(m, a, act)| {
                    let moves = Moves {
                        assigned: *a,
                        activity: *act,
                        ..Moves::default()
                    };
                    (m.to_string(), moves)
                })
                .collect(),
        }
    }

    /// Card spending a category cannot cover is the card's debt, not next
    /// month's: its uncovered part moves nothing to the payment category,
    /// and the next month's ready to assign is not taken for it.
    #[test]
    fn card_spending_moves_what_was_covered() {
        let card: Id<Account> = Id::new([7; 16]);
        let other: Id<Account> = Id::new([9; 16]);
        let moves = |assigned, activity, cards: &[(Id<Account>, i64)]| Moves {
            assigned,
            activity,
            cards: cards.iter().copied().collect(),
        };
        // $100 given, $30 cash and $120 on the card: $70 covered, $50 debt.
        let s = Step::of(0, &moves(100, -150, &[(card, -120)]));
        assert_eq!((s.available, s.cash_overspent, s.credit_overspent), (-50, 0, 50));
        assert_eq!(s.covers[&card], 70);
        // Cash first: $100 given, $130 cash and $20 on the card.
        let s = Step::of(0, &moves(100, -150, &[(card, -20)]));
        assert_eq!((s.cash_overspent, s.credit_overspent, s.covers[&card]), (30, 20, 0));
        // A refund on one card moves money back from it; the other covers.
        let s = Step::of(50, &moves(0, -10, &[(card, 15), (other, -25)]));
        assert_eq!((s.available, s.covers[&card], s.covers[&other]), (40, -15, 25));
        let h = History {
            months: [("2026-09".to_string(), moves(0, -40, &[(card, -40)]))].into(),
        };
        let oct = h.at("2026-10");
        assert_eq!((oct.available, oct.overspent_before, oct.overspent_last_month), (0, 0, 0));
        assert_eq!(h.at("2026-09").credit_overspent, 40);
    }

    /// What is left carries; what is overspent starts the next month at
    /// nothing and is counted against it.
    #[test]
    fn a_category_carries_what_is_left_and_not_what_is_short() {
        let h = history(&[("2026-08", 100, -30), ("2026-09", 0, -120), ("2026-10", 50, 0)]);
        let aug = h.at("2026-08");
        assert_eq!((aug.carried, aug.available), (0, 70));
        let sep = h.at("2026-09");
        assert_eq!((sep.carried, sep.available), (70, -50));
        let oct = h.at("2026-10");
        assert_eq!(
            (oct.carried, oct.available, oct.overspent_before, oct.overspent_last_month),
            (0, 50, -50, -50)
        );
        let dec = h.at("2026-12");
        assert_eq!((dec.carried, dec.available, dec.overspent_last_month), (50, 50, 0));
        assert_eq!(h.at("2026-01"), Standing::default());
    }
}
