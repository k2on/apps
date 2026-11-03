//! Filters: which transactions, said as conditions every one of which a
//! transaction meets — the accounts (none is every one; or only those on
//! the budget), text its payee or the bank's line holds, its categories
//! ("no category" among them), which way the money went, transfers or not,
//! only what needs a look, and dates.
//!
//! **One matcher.** [`Filter::matches`] is the only place a transaction is
//! tested against conditions: a register on screen ([`filtered`]), the
//! built-in filters the sidebar lists — every account ([`Filter::all`]),
//! what needs a look ([`Filter::needs_a_look`], the inbox), one account
//! ([`Filter::account`]) — the phones' `register` and `inbox`, a saved
//! filter's total, and a formula's terms ([`crate::formulas`]) all ask it.
//!
//! **Saved filters** are named, ordered, and kept ([`SavedFilter`]): the
//! sidebar lists them with what they add up to ([`saved`]), and a formula
//! sums one month by month. A filter a formula sums is not deleted: the
//! deletion is refused, naming the categories, since every month the
//! formula covers is worked out from it. Changing one's conditions moves
//! every month of every formula that sums it.

use ark::prelude::*;

use crate::budget::Plan;
use crate::dates::{first_day, is_day, last_day};
use crate::schema::*;
use crate::transactions::txn_line;
use crate::types::*;

impl Filter {
    /// Every transaction: "All Accounts".
    pub fn all() -> Filter {
        Filter::default()
    }

    /// What needs somebody to look at it: imported and not yet approved,
    /// or on budget and for no category — "Needs a Look", the inbox.
    pub fn needs_a_look() -> Filter {
        Filter {
            needs_look: true,
            ..Filter::default()
        }
    }

    /// One account's transactions: its register.
    pub fn account(id: Id<Account>) -> Filter {
        Filter {
            accounts: vec![id],
            ..Filter::default()
        }
    }

    /// The one account it is exactly the register of, when it is: one
    /// account named and no other condition.
    pub fn only_account(&self) -> Option<Id<Account>> {
        match self.accounts[..] {
            [a] if *self == Filter::account(a) => Some(a),
            _ => None,
        }
    }

    /// Kept to `month`'s days as well as its own.
    pub fn within(&self, month: &str) -> Filter {
        let (lo, hi) = (first_day(month), last_day(month));
        Filter {
            from: Some(self.from.clone().map_or(lo.clone(), |f| f.max(lo))),
            through: Some(self.through.clone().map_or(hi.clone(), |t| t.min(hi))),
            ..self.clone()
        }
    }

    /// Its conditions put in order and trimmed, so two that say the same
    /// are equal: accounts and categories sorted once each, the text
    /// trimmed, and a blank date none.
    pub fn tidy(&self) -> Filter {
        let mut f = self.clone();
        f.accounts.sort();
        f.accounts.dedup();
        f.categories.sort();
        f.categories.dedup();
        f.text = f.text.trim().to_string();
        let day = |d: &Option<String>| d.as_ref().map(|d| d.trim().to_string()).filter(|d| !d.is_empty());
        (f.from, f.through) = (day(&f.from), day(&f.through));
        f
    }

    /// Whether `t` meets every condition — read from the store: its
    /// account (on the budget?) and, for "needs a look", whether it needs a
    /// category. The only matcher there is ([`Filter::holds`] is it, given
    /// those two).
    pub fn matches(&self, db: &Db<Money>, t: &Txn) -> bool {
        let on_budget = db.account.get(t.account_id).is_some_and(|a| a.on_budget);
        // Whether it needs a category is read only when it is asked.
        let needs_category = self.needs_look && t.approved && needs_category(db, t, on_budget);
        self.holds(t, on_budget, needs_category)
    }

    /// [`Filter::matches`], given whether `t`'s account is on the budget
    /// and whether `t` needs a category (an on-budget transaction for no
    /// category that is not a transfer).
    pub fn holds(&self, t: &Txn, on_budget: bool, needs_category: bool) -> bool {
        if !self.accounts.is_empty() && !self.accounts.contains(&t.account_id) {
            return false;
        }
        if self.on_budget && !on_budget {
            return false;
        }
        if self.from.as_ref().is_some_and(|f| t.date < *f) || self.through.as_ref().is_some_and(|h| t.date > *h) {
            return false;
        }
        let ok = match self.direction {
            Direction::Either => true,
            Direction::In => t.amount > 0,
            Direction::Out => t.amount < 0,
        };
        if !ok {
            return false;
        }
        if self.no_transfers && is_transfer(t) {
            return false;
        }
        if !self.categories.is_empty() && !self.categories.contains(&t.category_id) {
            return false;
        }
        if self.needs_look && t.approved && !needs_category {
            return false;
        }
        let want = self.text.trim().to_lowercase();
        want.is_empty() || t.payee.to_lowercase().contains(&want) || t.imported_payee.to_lowercase().contains(&want)
    }

    /// The days it reads, at most, as a range of the date index: from its
    /// first, up to just past its last.
    fn days(&self) -> (String, String) {
        (
            self.from.clone().unwrap_or_default(),
            self.through.as_ref().map_or_else(|| "~".into(), |t| format!("{t}~")),
        )
    }

    /// Every transaction it matches, read through the narrowest index:
    /// its accounts' by date when it names some, else every one by date.
    pub fn txns(&self, db: &Db<Money>) -> Vec<Txn> {
        let (lo, hi) = self.days();
        let all = match self.accounts.is_empty() {
            true => db.txn.by(Txn::DATE).between(lo, hi).all(),
            false => {
                let mut out = Vec::new();
                let mut accounts = self.accounts.clone();
                accounts.sort();
                accounts.dedup();
                for a in &accounts {
                    out.extend(db.txn.by(Txn::ACCOUNT_ID_DATE).between((*a, lo.clone()), (*a, hi.clone())).all());
                }
                out
            }
        };
        all.into_iter().filter(|t| self.matches(db, t)).collect()
    }

    /// What they add up to: how many, and their sum.
    pub fn total(&self, db: &Db<Money>) -> (i64, i64) {
        self.txns(db).iter().fold((0, 0), |(n, s), t| (n + 1, s + t.amount))
    }

    /// Its conditions, said shortly for a sidebar or a formula:
    /// "Checking, Savings · \"acme\" · money in · no transfers". Every
    /// transaction is "every transaction".
    pub fn said(&self, db: &Db<Money>) -> String {
        let mut parts = Vec::new();
        if !self.accounts.is_empty() {
            let mut named: Vec<Account> = self.accounts.iter().filter_map(|a| db.account.get(*a)).collect();
            named.sort_by_key(|a| (a.pos, a.id));
            parts.push(named.into_iter().map(|a| a.name).collect::<Vec<_>>().join(", "));
        }
        if self.on_budget {
            parts.push("on budget".into());
        }
        if !self.text.trim().is_empty() {
            parts.push(format!("\u{201c}{}\u{201d}", self.text.trim()));
        }
        if !self.categories.is_empty() {
            let names: Vec<String> = self
                .categories
                .iter()
                .map(|c| match c.and_then(|c| db.category.get(c)) {
                    Some(c) => c.name,
                    None if c.is_none() => "no category".into(),
                    None => "a category gone".into(),
                })
                .collect();
            parts.push(names.join(", "));
        }
        match self.direction {
            Direction::Either => {}
            Direction::In => parts.push("money in".into()),
            Direction::Out => parts.push("money out".into()),
        }
        if self.no_transfers {
            parts.push("no transfers".into());
        }
        if self.needs_look {
            parts.push("needs a look".into());
        }
        match (&self.from, &self.through) {
            (None, None) => {}
            (Some(f), None) => parts.push(format!("from {f}")),
            (None, Some(t)) => parts.push(format!("through {t}")),
            (Some(f), Some(t)) => parts.push(format!("{f} to {t}")),
        }
        match parts.is_empty() {
            true => "every transaction".into(),
            false => parts.join(" \u{b7} "),
        }
    }

    /// What is wrong with it as conditions to keep, if anything: an
    /// account or a category that is not here, or a day that is not one.
    pub fn check(&self, db: &Db<Money>) -> Result<(), Refusal> {
        if self.accounts.iter().any(|a| !db.account.has(*a)) {
            return Err(Refusal::new("no such account"));
        }
        if self.categories.iter().flatten().any(|c| !db.category.has(*c)) {
            return Err(Refusal::new("no such category"));
        }
        if [&self.from, &self.through].into_iter().flatten().any(|d| !is_day(d)) {
            return Err(Refusal::new("a day, as 2026-10-26"));
        }
        if self.text.chars().count() > 120 {
            return Err(Refusal::new("the text is at most 120 characters"));
        }
        Ok(())
    }
}

/// A transfer: one of the two halves of money moved between accounts.
pub fn is_transfer(t: &Txn) -> bool {
    t.transfer_account.is_some() || t.transfer_id.is_some()
}

/// Whether a transaction is waiting for a category: on the budget, for no
/// category here — or, split, a line for none — and not a transfer to an
/// account still here; what a register says "Needs a category" of
/// ([`txn_line`]).
pub fn needs_category(db: &Db<Money>, t: &Txn, on_budget: bool) -> bool {
    let none = |c: Option<Id<Category>>| c.and_then(|c| db.category.get(c)).is_none();
    let wanting = match t.split {
        true => crate::transactions::splits_of(db, t.id).iter().any(|l| none(l.category_id)),
        false => none(t.category_id),
    };
    on_budget && wanting && t.transfer_account.and_then(|a| db.account.get(a)).is_none()
}

/// A saved filter, or no such filter.
pub fn saved_of(db: &Db<Money>, id: Id<SavedFilter>) -> Result<SavedFilter, Refusal> {
    db.saved_filter.get(id).ok_or_else(|| Refusal::new("no such filter"))
}

/// A name no other saved filter has, any case.
fn unique(db: &Db<Money>, name: &str, besides: Option<Id<SavedFilter>>) -> Result<String, Refusal> {
    let name = name.trim().to_string();
    let taken = db
        .saved_filter
        .scan()
        .into_iter()
        .any(|f| Some(f.id) != besides && f.name.eq_ignore_ascii_case(&name));
    match taken {
        true => Err(Refusal::new(format!("a filter is already called {name}"))),
        false => Ok(name),
    }
}

/// The categories whose formulas sum a filter, in any month, by name.
pub fn used_by(db: &Db<Money>, id: Id<SavedFilter>) -> Vec<String> {
    let mut names: Vec<String> = db
        .formula
        .scan()
        .into_iter()
        .filter(|f| f.terms.iter().any(|t| t.filter_id == id))
        .filter_map(|f| db.category.get(f.category_id).map(|c| c.name))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// A filter kept under a name, after every other.
#[mutator("filters.save_filter")]
pub fn save_filter(ctx: &Ctx, db: &mut Db<Money>, input: SaveFilter) -> Result<(), Refusal> {
    let filter = input.filter.tidy();
    filter.check(db)?;
    let name = unique(db, &input.name, None)?;
    let pos = db.saved_filter.by(SavedFilter::POS).whole().all().last().map_or(0, |f| f.pos + 1);
    db.saved_filter.insert(SavedFilter {
        id: ctx.new_id(),
        name,
        pos,
        filter,
        created_ms: ctx.now,
        user_id: ctx.user().into(),
    })
}

#[mutator("filters.rename_filter")]
pub fn rename_filter(_: &Ctx, db: &mut Db<Money>, input: RenameFilter) -> Result<(), Refusal> {
    let f = saved_of(db, input.id)?;
    let name = unique(db, &input.name, Some(f.id))?;
    db.saved_filter.update(SavedFilter { name, ..f })
}

/// A saved filter's conditions, changed.
#[mutator("filters.change_filter")]
pub fn change_filter(_: &Ctx, db: &mut Db<Money>, input: ChangeFilter) -> Result<(), Refusal> {
    let f = saved_of(db, input.id)?;
    let filter = input.filter.tidy();
    filter.check(db)?;
    db.saved_filter.update(SavedFilter { filter, ..f })
}

/// A saved filter gone — unless a formula sums it.
#[mutator("filters.delete_filter")]
pub fn delete_filter(_: &Ctx, db: &mut Db<Money>, input: FilterId) -> Result<(), Refusal> {
    let f = saved_of(db, input.id)?;
    let using = used_by(db, f.id);
    if !using.is_empty() {
        return Err(Refusal::new(format!("{} is summed by the formula of {}", f.name, using.join(", "))));
    }
    db.saved_filter.remove(&f)
}

/// A saved filter put in front of another, or after every other.
#[mutator("filters.move_filter")]
pub fn move_filter(_: &Ctx, db: &mut Db<Money>, input: MoveFilter) -> Result<(), Refusal> {
    let moved = saved_of(db, input.id)?;
    if input.before == Some(moved.id) {
        return Ok(());
    }
    let mut order: Vec<SavedFilter> = db
        .saved_filter
        .by(SavedFilter::POS)
        .whole()
        .all()
        .into_iter()
        .filter(|f| f.id != moved.id)
        .collect();
    let at = match input.before {
        Some(b) => order
            .iter()
            .position(|f| f.id == b)
            .ok_or_else(|| Refusal::new("no such filter to go before"))?,
        None => order.len(),
    };
    order.insert(at, moved);
    for (i, f) in order.into_iter().enumerate() {
        if f.pos != i as i64 {
            db.saved_filter.update(SavedFilter { pos: i as i64, ..f })?;
        }
    }
    Ok(())
}

// * The views

/// A saved filter as the sidebar lists it.
pub fn filter_line(db: &Db<Money>, f: &SavedFilter) -> FilterLine {
    let (count, total) = f.filter.total(db);
    FilterLine {
        id: f.id,
        name: f.name.clone(),
        pos: f.pos,
        filter: f.filter.clone(),
        said: f.filter.said(db),
        count,
        total,
        used_by: used_by(db, f.id),
    }
}

/// Every saved filter, in order, with what each adds up to.
#[view("filters.saved")]
pub fn saved(_: &Ctx, _: &Db<Money>, _: ()) -> Result<Plan<FilterLine>, Refusal> {
    Ok(scan::<Money, SavedFilter>().map_with(filter_line).sort(|l| (l.pos, l.id)))
}

/// The conditions a register for `input` reads: a saved filter's (none
/// when it is gone), or those said; within the month, when one is said.
pub fn conditions(db: &Db<Money>, input: &Filtered) -> Option<Filter> {
    let f = match input.saved {
        Some(id) => db.saved_filter.get(id)?.filter,
        None => input.filter.tidy(),
    };
    Some(match &input.month {
        Some(m) => f.within(m),
        None => f,
    })
}

/// The register for a filter: every transaction it matches, the newest
/// first. A saved filter's is read from it, so the register follows a
/// change to it.
#[view("transactions.filtered")]
pub fn filtered(_: &Ctx, db: &Db<Money>, input: Filtered) -> Result<Plan<TxnLine>, Refusal> {
    let f = conditions(db, &input);
    Ok(scan::<Money, Txn>()
        .filter_with(move |db, t| f.as_ref().is_some_and(|f| f.matches(db, t)))
        .map_with(txn_line)
        .sort(|l| (Desc(l.date.clone()), Desc(l.created_ms), l.id)))
}

/// What the register for a filter adds up to: one row, none when it
/// matches nothing.
#[view("transactions.filtered_total")]
pub fn filtered_total(_: &Ctx, db: &Db<Money>, input: Filtered) -> Result<Plan<FilterTotal>, Refusal> {
    let f = conditions(db, &input);
    Ok(scan::<Money, Txn>()
        .filter_with(move |db, t| f.as_ref().is_some_and(|f| f.matches(db, t)))
        .group(
            |_| 0i64,
            |_, _, txns| {
                txns.fold(FilterTotal::default(), |s, t| FilterTotal {
                    count: s.count + 1,
                    total: s.total + t.amount,
                    inflow: s.inflow + t.amount.max(0),
                    outflow: s.outflow + t.amount.min(0),
                })
            },
        ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id<T>(n: u8) -> Id<T> {
        Id::new([n; 16])
    }

    fn txn() -> Txn {
        Txn {
            account_id: id(1),
            date: "2026-10-05".into(),
            payee: "ACME Payroll".into(),
            imported_payee: "ACME CORP DIR DEP".into(),
            amount: 1_000,
            approved: true,
            category_id: Some(id(7)),
            ..Txn::default()
        }
    }

    /// Each condition alone, then together: falsified by any one of them
    /// read the wrong way, or left out.
    #[test]
    fn every_condition_is_kept() {
        let t = txn();
        let all = Filter::all();
        assert!(all.holds(&t, true, false) && all.holds(&t, false, false), "every transaction");
        // Accounts.
        assert!(Filter::account(id(1)).holds(&t, true, false));
        assert!(!Filter::account(id(2)).holds(&t, true, false));
        let both = Filter {
            accounts: vec![id(2), id(1)],
            ..all.clone()
        };
        assert!(both.holds(&t, true, false));
        let on = Filter {
            on_budget: true,
            ..all.clone()
        };
        assert!(on.holds(&t, true, false) && !on.holds(&t, false, false));
        // Text: the payee or the bank's line, any case.
        for (text, hit) in [
            ("acme", true),
            ("PAYROLL", true),
            ("dir dep", true),
            (" corp ", true),
            ("side gig", false),
        ] {
            let f = Filter {
                text: text.into(),
                ..all.clone()
            };
            assert_eq!(f.holds(&t, true, false), hit, "{text:?}");
        }
        // Categories, "no category" among them.
        let cats = |c: Vec<Option<Id<Category>>>| Filter {
            categories: c,
            ..all.clone()
        };
        assert!(cats(vec![Some(id(7))]).holds(&t, true, false));
        assert!(!cats(vec![Some(id(8))]).holds(&t, true, false));
        assert!(!cats(vec![None]).holds(&t, true, false));
        let none = Txn {
            category_id: None,
            ..t.clone()
        };
        assert!(cats(vec![None, Some(id(8))]).holds(&none, true, true));
        // Direction.
        let dir = |d| Filter { direction: d, ..all.clone() };
        let out = Txn { amount: -5, ..t.clone() };
        let zero = Txn { amount: 0, ..t.clone() };
        assert!(dir(Direction::In).holds(&t, true, false) && !dir(Direction::In).holds(&out, true, false));
        assert!(dir(Direction::Out).holds(&out, true, false) && !dir(Direction::Out).holds(&t, true, false));
        assert!(!dir(Direction::In).holds(&zero, true, false) && !dir(Direction::Out).holds(&zero, true, false));
        assert!(dir(Direction::Either).holds(&zero, true, false));
        // Transfers, either half.
        let no = Filter {
            no_transfers: true,
            ..all.clone()
        };
        let half = Txn {
            transfer_id: Some(id(9)),
            ..t.clone()
        };
        let other = Txn {
            transfer_account: Some(id(3)),
            ..t.clone()
        };
        assert!(no.holds(&t, true, false) && !no.holds(&half, true, false) && !no.holds(&other, true, false));
        assert!(all.holds(&half, true, false));
        // Needs a look: not approved, or needing a category.
        let look = Filter::needs_a_look();
        assert!(!look.holds(&t, true, false));
        assert!(look.holds(
            &Txn {
                approved: false,
                ..t.clone()
            },
            true,
            false
        ));
        assert!(look.holds(&none, true, true));
        // Dates, inclusive.
        let dates = |f: Option<&str>, h: Option<&str>| Filter {
            from: f.map(str::to_string),
            through: h.map(str::to_string),
            ..all.clone()
        };
        assert!(dates(Some("2026-10-05"), Some("2026-10-05")).holds(&t, true, false));
        assert!(!dates(Some("2026-10-06"), None).holds(&t, true, false));
        assert!(!dates(None, Some("2026-10-04")).holds(&t, true, false));
        assert!(all.within("2026-10").holds(&t, true, false) && !all.within("2026-11").holds(&t, true, false));
        // Together: every one must hold.
        let paychecks = Filter {
            accounts: vec![id(1)],
            on_budget: true,
            text: "acme".into(),
            categories: vec![Some(id(7))],
            direction: Direction::In,
            no_transfers: true,
            from: Some("2026-10-01".into()),
            through: Some("2026-10-31".into()),
            ..all.clone()
        };
        assert!(paychecks.holds(&t, true, false));
        assert!(!paychecks.holds(&t, false, false), "off the budget");
        assert!(!paychecks.holds(&out, true, false), "money out");
        assert!(!paychecks.holds(&half, true, false), "a transfer");
        assert!(!paychecks.holds(
            &Txn {
                payee: "Side".into(),
                imported_payee: String::new(),
                ..t.clone()
            },
            true,
            false
        ));
        assert!(
            !Filter {
                needs_look: true,
                ..paychecks
            }
            .holds(&t, true, false),
            "looked at already"
        );
    }

    #[test]
    fn a_month_narrows_and_one_account_is_its_register() {
        let f = Filter {
            from: Some("2026-10-10".into()),
            ..Filter::all()
        };
        let w = f.within("2026-10");
        assert_eq!((w.from.as_deref(), w.through.as_deref()), (Some("2026-10-10"), Some("2026-10-31")));
        assert_eq!(Filter::account(id(1)).only_account(), Some(id(1)));
        assert_eq!(Filter::all().only_account(), None);
        let narrowed = Filter {
            text: "x".into(),
            ..Filter::account(id(1))
        };
        assert_eq!(narrowed.only_account(), None);
        let messy = Filter {
            accounts: vec![id(2), id(1), id(2)],
            text: "  acme ".into(),
            from: Some(" ".into()),
            ..Filter::all()
        };
        let tidy = messy.tidy();
        assert_eq!((tidy.accounts, tidy.text.as_str(), tidy.from), (vec![id(1), id(2)], "acme", None));
    }
}
