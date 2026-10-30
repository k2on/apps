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
//!
//! Nothing here is stored but what people said — the assignments and the
//! transactions. Every figure is worked out by the views from them, so a
//! transaction that arrives late (an import, a phone that was offline)
//! moves every month after it, on every device, the same way.

use std::collections::BTreeMap;

use ark::prelude::*;

use crate::dates::{add_months, month_end, month_of};
use crate::schema::*;
use crate::types::*;

/// A plan over money's tables.
pub type Plan<T> = ark::plan::Plan<Money, T>;

fn fixed<T>(what: &str) -> Id<T> {
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
    if !db.group.has(inflow()) {
        db.group.insert(Group {
            id: inflow(),
            name: "Inflow".into(),
            pos: 0,
            hidden: true,
            income: true,
            created_ms: ctx.now,
            user_id: ctx.user().into(),
        })?;
    }
    db.category.insert(Category {
        id: ready_to_assign(),
        group_id: inflow(),
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

/// Whether a category is the budget's own (money in, not a job for it).
pub fn is_income(db: &Db<Money>, id: Id<Category>) -> bool {
    db.category.get(id).and_then(|c| db.group.get(c.group_id)).is_some_and(|g| g.income)
}

/// A category money can be assigned to: one that is, and not the budget's own.
pub fn budgetable(db: &Db<Money>, id: Id<Category>) -> Result<Category, Refusal> {
    let c = db.category.get(id).ok_or_else(|| Refusal::new("no such category"))?;
    if is_income(db, id) {
        return Err(Refusal::new("money is assigned from Ready to Assign, not to it"));
    }
    Ok(c)
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
        let had = assigned_in(db, &input.month, from);
        set_assigned(ctx, db, &input.month, from, had - input.amount)?;
    }
    if let Some(to) = input.to {
        budgetable(db, to)?;
        let had = assigned_in(db, &input.month, to);
        set_assigned(ctx, db, &input.month, to, had + input.amount)?;
    }
    Ok(())
}

/// A month assigned at once: last month's amounts, or what each target
/// still lacks, or nothing.
#[mutator("budget.auto_assign")]
pub fn auto_assign(ctx: &Ctx, db: &mut Db<Money>, input: AutoAssign) -> Result<(), Refusal> {
    let last = add_months(&input.month, -1);
    for c in db.category.by(Category::POS).whole().all() {
        if is_income(db, c.id) || (!input.only.is_empty() && !input.only.contains(&c.id)) {
            continue;
        }
        let amount = match input.how {
            Auto::LastMonth => assigned_in(db, &last, c.id),
            Auto::Underfunded => assigned_in(db, &input.month, c.id).max(c.target),
            Auto::Reset => 0,
        };
        set_assigned(ctx, db, &input.month, c.id, amount)?;
    }
    Ok(())
}

// * The arithmetic

/// One category's history, month by month: what each assigned it and what
/// each spent from it (activity, spending negative).
#[derive(Default)]
pub struct History {
    pub months: BTreeMap<String, (i64, i64)>,
}

impl History {
    /// Read from its assignments and its transactions.
    pub fn of(db: &Db<Money>, id: Id<Category>) -> History {
        let mut months: BTreeMap<String, (i64, i64)> = BTreeMap::new();
        for a in db.assigned.by(Assigned::CATEGORY_ID_MONTH).from(id).all() {
            months.entry(a.month).or_default().0 += a.amount;
        }
        for t in db.txn.by(Txn::CATEGORY_ID_DATE).from(Some(id)).all() {
            months.entry(month_of(&t.date)).or_default().1 += t.amount;
        }
        History { months }
    }

    /// The month as this category stood in it.
    pub fn at(&self, month: &str) -> Standing {
        let mut s = Standing::default();
        let Some(first) = self.months.keys().next().cloned() else {
            return s;
        };
        let mut carry = 0;
        let mut k = first;
        while k.as_str() < month {
            let (a, act) = self.months.get(&k).copied().unwrap_or_default();
            let available = carry + a + act;
            if available < 0 {
                s.overspent_before += available;
            }
            if k == add_months(month, -1) {
                s.overspent_last_month = available.min(0);
                (s.assigned_last_month, s.spent_last_month) = (a, act);
            }
            if k >= add_months(month, -3) {
                s.spent_last_three += act;
            }
            carry = available.max(0);
            k = add_months(&k, 1);
        }
        let (a, act) = self.months.get(month).copied().unwrap_or_default();
        (s.carried, s.assigned, s.activity) = (carry, a, act);
        s.available = carry + a + act;
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
    /// What it was overspent by, in every month before (negative).
    pub overspent_before: i64,
    pub overspent_last_month: i64,
    pub assigned_last_month: i64,
    pub spent_last_month: i64,
    pub spent_last_three: i64,
}

/// A category's line in a month.
pub fn line(db: &Db<Money>, c: &Category, month: &str) -> BudgetLine {
    let s = History::of(db, c.id).at(month);
    let g = db.group.get(c.group_id).unwrap_or_default();
    BudgetLine {
        category_id: c.id,
        group_id: c.group_id,
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
    }
}

/// The month as a whole, from every category.
pub fn summary_of(db: &Db<Money>, categories: impl Iterator<Item = Category>, month: &str) -> Summary {
    let mut out = Summary {
        month: month.into(),
        ..Summary::default()
    };
    let mut assigned_through = 0;
    for a in db.assigned.scan() {
        if a.month.as_str() <= month {
            assigned_through += a.amount;
        }
    }
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
    let mut overspent_before = 0;
    for c in categories {
        if c.id == rta || is_income(db, c.id) {
            continue;
        }
        let s = History::of(db, c.id).at(month);
        overspent_before += s.overspent_before;
        out.overspent_last_month += s.overspent_last_month;
        out.assigned += s.assigned;
        out.activity += s.activity;
        out.available += s.available;
        out.overspent += (s.available < 0) as i64;
        out.underfunded += (s.assigned < c.target) as i64;
    }
    out.ready_to_assign = income_through - assigned_through + overspent_before;
    out
}

// * The views

/// The groups the budget draws, in order; not the budget's own.
#[view("budget.groups")]
pub fn groups(_: &Ctx, _: &Db<Money>, _: ()) -> Result<Plan<GroupLine>, Refusal> {
    Ok(scan::<Money, Group>().filter(|g| !g.income).sort(|g| (g.pos, g.id)).map(|g| GroupLine {
        id: g.id,
        name: g.name.clone(),
        pos: g.pos,
        hidden: g.hidden,
    }))
}

/// Every category in one month, in the budget's order. A category's line
/// is kept by the rows it read — its own assignments and transactions —
/// so a transaction moves the one line it is for.
#[view("budget.month")]
pub fn month(_: &Ctx, _: &Db<Money>, input: Month) -> Result<Plan<BudgetLine>, Refusal> {
    let month = input.month;
    Ok(scan::<Money, Category>()
        .filter_with(|db, c| !is_income(db, c.id))
        .map_with(move |db, c| line(db, c, &month))
        .sort(|l| (l.group_pos, l.group_id, l.pos, l.category_id)))
}

/// The month as a whole, one row: what is ready to assign, and the totals.
#[view("budget.summary")]
pub fn summary(_: &Ctx, _: &Db<Money>, input: Month) -> Result<Plan<Summary>, Refusal> {
    let month = input.month;
    Ok(scan::<Money, Category>().group(|_| 0i64, move |db, _, cats| summary_of(db, cats.cloned(), &month)))
}

/// What a month's spending went on, group by group, the most first.
#[view("budget.spending")]
pub fn spending(_: &Ctx, _: &Db<Money>, input: Month) -> Result<Plan<SpendingLine>, Refusal> {
    let month = input.month;
    Ok(scan::<Money, Category>()
        .filter_with(|db, c| !is_income(db, c.id))
        .group(
            |c| c.group_id,
            move |db, group, cats| {
                let g = db.group.get(*group).unwrap_or_default();
                let lo = (crate::dates::month_start(&month), month_end(&month));
                let spent: i64 = cats
                    .map(|c| {
                        db.txn
                            .by(Txn::CATEGORY_ID_DATE)
                            .between((Some(c.id), lo.0.clone()), (Some(c.id), lo.1.clone()))
                            .all()
                            .iter()
                            .map(|t| t.amount)
                            .sum::<i64>()
                    })
                    .sum();
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
            months: months.iter().map(|(m, a, act)| (m.to_string(), (*a, *act))).collect(),
        }
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
