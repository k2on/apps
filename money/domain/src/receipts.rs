//! Receipts: what a store says was bought, line by line, beside what the
//! bank says was paid.
//!
//! A store's adapter ([`crate::stores`]) reads an export — Amazon's order
//! history, Target's, Costco's warehouse and gas receipts — into one
//! [`ReceiptImport`], which [`add_receipts`] applies: each receipt keyed by its
//! store's own id, so importing the same export twice changes nothing, and
//! each item filed under the category it was filed under last time (the
//! same store's same item), or one whose name is in the item's ("Eggs" in
//! `GG LARGE EGGS 12CT`), or none yet. Filing one item ([`file_items`]) files
//! every unfiled item with the same key, so a Costco item number is said
//! once.
//!
//! **Paid by.** A receipt is paid by the bank's transaction of the same
//! amount, in an account on budget, from a payee that is the store's, dated
//! from a couple of days before it to a few after (two weeks for Amazon,
//! which charges as it ships), and paying for no other receipt; whichever
//! arrives second, the receipt or the charge, makes the match. Somebody can
//! say otherwise ([`pay`]).
//!
//! **The split.** A paid receipt splits its transaction by amounts: its
//! items' amounts summed by category, then each sum given its share of
//! what the bank took — so tax, fees and rounding are spread over the
//! lines in proportion, and the lines add up to the transaction exactly.
//! Items not filed yet go with what the transaction was for before: its
//! category, or (split already) the payee's last; for none, a line for no
//! category, which leaves the transaction in the inbox. All one category
//! is no split, only that category. The split is written again whenever
//! the receipt is matched or one of its items is filed, so it follows the
//! items; a split typed by hand is a split like any other, and stands
//! until then.

use std::collections::BTreeMap;

use ark::prelude::*;

use crate::budget::{ancestry, path_of, Plan};
use crate::dates::{add_days, is_day};
use crate::schema::*;
use crate::transactions::{remembered, write_split};
use crate::types::*;

// * Keys and guesses

/// Lower-case words of letters and digits, a plural's `s` taken off: what
/// a category's name and an item's are compared by.
pub fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| {
            let w = w.to_lowercase();
            match w.strip_suffix('s') {
                Some(stem) if stem.len() >= 3 && !stem.ends_with('s') => stem.to_string(),
                _ => w,
            }
        })
        .collect()
}

/// What remembering an item goes by: the store's number for it, else its
/// name folded — the same store's same thing.
pub fn key_of(store: &str, sku: &str, name: &str) -> String {
    match sku.trim() {
        "" => format!("{store}:~{}", words(name).join(" ")),
        sku => format!("{store}:{sku}"),
    }
}

/// The category an item with this key was filed under last.
fn remembered_item(db: &Db<Money>, key: &str) -> Option<Id<Category>> {
    db.item
        .by(Item::SAME)
        .from(key.to_string())
        .rev()
        .all()
        .into_iter()
        .find_map(|i| i.category_id.filter(|c| db.category.has(*c)))
}

/// The category whose name is in the item's, the deepest and most words
/// first: "Eggs" for `KS ORGANIC EGGS 24CT`, "Paint" for `DUTCH BOY INT
/// PAINT`. Not a heading, and not the budget's own.
fn named_in(db: &Db<Money>, name: &str) -> Option<Id<Category>> {
    let have = words(name);
    db.category
        .scan()
        .into_iter()
        .filter(|c| c.parent_id.is_some() && !crate::budget::is_income(db, c.id))
        .filter_map(|c| {
            let want = words(&c.name);
            let all = !want.is_empty() && want.iter().all(|w| have.contains(w));
            all.then(|| ((ancestry(db, c.id).len(), want.len()), c.id))
        })
        .max_by_key(|(rank, id)| (*rank, std::cmp::Reverse(*id)))
        .map(|(_, id)| id)
}

/// What an item is filed under when it arrives.
pub fn guess(db: &Db<Money>, key: &str, name: &str) -> Option<Id<Category>> {
    remembered_item(db, key).or_else(|| named_in(db, name))
}

// * Paid by

/// The words a store's charges carry at the bank.
fn aliases(store: &str) -> &'static [&'static str] {
    match store {
        "amazon" => &["amazon", "amzn"],
        "target" => &["target"],
        "costco" => &["costco"],
        _ => &[],
    }
}

/// How many days after a receipt its charge may land.
fn days_after(store: &str) -> i64 {
    match store {
        "amazon" => 14,
        _ => 4,
    }
}

fn the_stores(store: &str, t: &Txn) -> bool {
    let (a, b) = (t.payee.to_lowercase(), t.imported_payee.to_lowercase());
    aliases(store).iter().any(|w| a.contains(w) || b.contains(w))
}

/// Whether `t` can be what paid for `r`.
fn could_pay(db: &Db<Money>, r: &Receipt, t: &Txn) -> bool {
    let on_budget = db.account.get(t.account_id).is_some_and(|a| a.on_budget);
    let lo = add_days(&r.date, -2);
    let hi = add_days(&r.date, days_after(&r.store));
    on_budget
        && t.transfer_id.is_none()
        && t.amount == r.total
        && t.date >= lo
        && t.date <= hi
        && the_stores(&r.store, t)
        && db.receipt.by(Receipt::TXN_ID).from(Some(t.id)).all().iter().all(|o| o.id == r.id)
}

/// The transaction that paid for `r`, of those that could: the nearest in
/// days.
fn payer(db: &Db<Money>, r: &Receipt) -> Option<Txn> {
    let lo = add_days(&r.date, -2);
    let hi = add_days(&r.date, days_after(&r.store) + 1);
    db.txn
        .by(Txn::DATE)
        .between(lo, hi)
        .all()
        .into_iter()
        .filter(|t| could_pay(db, r, t))
        .min_by_key(|t| (crate::dates::days_between(&r.date, &t.date).abs(), t.id))
}

/// A transaction that just arrived, as what pays for a receipt waiting for
/// one; its split written when it does.
pub fn pay_waiting(db: &mut Db<Money>, id: Id<Txn>) -> Result<(), Refusal> {
    let Some(t) = db.txn.get(id) else { return Ok(()) };
    let waiting = db.receipt.by(Receipt::TXN_ID).from(None).all();
    let mut fits: Vec<Receipt> = waiting.into_iter().filter(|r| could_pay(db, r, &t)).collect();
    fits.sort_by_key(|r| (crate::dates::days_between(&r.date, &t.date).abs(), r.id.clone()));
    if let Some(r) = fits.into_iter().next() {
        db.receipt.update(Receipt {
            txn_id: Some(t.id),
            ..r.clone()
        })?;
        split_by(db, &r.id)?;
    }
    Ok(())
}

// * The split

/// The items of a receipt, in order.
pub fn items_of(db: &Db<Money>, receipt_id: &str) -> Vec<Item> {
    let mut items = db.item.by(Item::RECEIPT_ID).from(receipt_id.to_string()).all();
    items.sort_by_key(|i| i.line);
    items
}

/// `total` shared over `parts` in proportion to them, every share whole
/// cents and the shares adding up to `total` exactly: the largest remainders
/// get the cents rounding left over.
pub fn shares(total: i64, parts: &[i64]) -> Vec<i64> {
    let sum: i128 = parts.iter().map(|p| *p as i128).sum();
    if sum == 0 || parts.is_empty() {
        let mut out = vec![0; parts.len()];
        if let Some(first) = out.first_mut() {
            *first = total;
        }
        return out;
    }
    // Over a positive denominator, so that each share's floor is below its
    // exact value and what is left to give out is never negative.
    let sign = sum.signum();
    let den = sum.abs();
    let exact: Vec<i128> = parts.iter().map(|p| *p as i128 * total as i128 * sign).collect();
    let mut out: Vec<i64> = exact.iter().map(|e| e.div_euclid(den) as i64).collect();
    let left = total - out.iter().sum::<i64>();
    let mut by_remainder: Vec<usize> = (0..parts.len()).collect();
    by_remainder.sort_by_key(|i| (std::cmp::Reverse(exact[*i].rem_euclid(den)), *i));
    for i in by_remainder.into_iter().cycle().take(left.max(0) as usize) {
        out[i] += 1;
    }
    out
}

/// A receipt's transaction split by its items, as this module's head says.
pub fn split_by(db: &mut Db<Money>, receipt_id: &str) -> Result<(), Refusal> {
    let Some(r) = db.receipt.get(receipt_id.to_string()) else {
        return Ok(());
    };
    let Some(t) = r.txn_id.and_then(|id| db.txn.get(id)) else {
        return Ok(());
    };
    let on_budget = db.account.get(t.account_id).is_some_and(|a| a.on_budget);
    if !on_budget || t.transfer_id.is_some() {
        return Ok(());
    }
    let before = match t.split {
        false => t.category_id,
        true => remembered(db, &t.payee),
    };
    let mut order: Vec<Option<Id<Category>>> = vec![];
    let mut sums: BTreeMap<Option<Id<Category>>, (i64, Vec<String>)> = BTreeMap::new();
    for i in items_of(db, &r.id) {
        let c = i.category_id.filter(|c| db.category.has(*c)).or(before);
        if !sums.contains_key(&c) {
            order.push(c);
        }
        let e = sums.entry(c).or_default();
        e.0 += i.amount;
        e.1.push(i.name);
    }
    if order.is_empty() {
        return Ok(());
    }
    let parts: Vec<i64> = order.iter().map(|c| sums[c].0).collect();
    let amounts = shares(t.amount, &parts);
    let lines: Vec<SplitPart> = order
        .iter()
        .zip(amounts)
        .map(|(c, amount)| {
            let names = &sums[c].1;
            let mut memo = names.join(", ");
            if memo.chars().count() > 120 {
                memo = memo.chars().take(117).collect::<String>() + "…";
            }
            SplitPart {
                category_id: *c,
                amount,
                memo: match names.len() {
                    1 => memo,
                    n => format!("{n} items: {memo}"),
                },
            }
        })
        .collect();
    write_split(db, t, &lines, false)
}

/// A transaction split again by the receipt it paid for, when it paid for
/// one.
pub fn resplit_txn(db: &mut Db<Money>, id: Id<Txn>) -> Result<(), Refusal> {
    if let Some(r) = db.receipt.by(Receipt::TXN_ID).from(Some(id)).first() {
        split_by(db, &r.id)?;
    }
    Ok(())
}

// * The mutators

/// One store's receipts, as its adapter read them: new ones added with
/// their items filed as they were before, known ones brought up to date
/// with what somebody filed kept, and each paid for by the transaction
/// that did, once there is one.
#[mutator("receipts.add_receipts")]
pub fn add_receipts(ctx: &Ctx, db: &mut Db<Money>, input: ReceiptImport) -> Result<(), Refusal> {
    crate::budget::ensure_setup(ctx, db)?;
    for got in &input.receipts {
        if !is_day(&got.date) {
            return Err(Refusal::new(format!("{}: {} is not a day", got.order_id, got.date)));
        }
        if got.order_id.trim().is_empty() {
            return Err(Refusal::new("a receipt needs the store's id for it"));
        }
        let id = format!("{}:{}", input.store, got.order_id.trim());
        let had = db.receipt.get(id.clone());
        let r = Receipt {
            id: id.clone(),
            store: input.store.clone(),
            date: got.date.clone(),
            place: got.place.clone(),
            total: got.total,
            txn_id: had.as_ref().and_then(|h| h.txn_id),
            imported_ms: had.as_ref().map_or(ctx.now, |h| h.imported_ms),
            user_id: had.as_ref().map_or_else(|| ctx.user().to_string(), |h| h.user_id.clone()),
        };
        let changed = had.as_ref() != Some(&r);
        db.receipt.put(r.clone())?;
        let old = items_of(db, &id);
        let mut items_changed = false;
        for (n, it) in got.items.iter().enumerate() {
            let line = n as i64 + 1;
            let key = key_of(&input.store, &it.sku, &it.name);
            let prior = old.iter().find(|o| o.line == line);
            let category_id = match prior {
                Some(p) if p.same == key && p.category_id.is_some() => p.category_id,
                _ => guess(db, &key, &it.name),
            };
            let item = Item {
                receipt_id: id.clone(),
                line,
                name: it.name.clone(),
                sku: it.sku.clone(),
                same: key,
                quantity: it.quantity,
                unit: it.unit.clone(),
                amount: it.amount,
                category_id,
            };
            if prior != Some(&item) {
                db.item.put(item)?;
                items_changed = true;
            }
        }
        for o in old.iter().filter(|o| o.line > got.items.len() as i64) {
            db.item.remove(o)?;
            items_changed = true;
        }
        let paid = match r.txn_id.and_then(|t| db.txn.get(t)) {
            Some(t) if t.amount == r.total => Some(t),
            _ => payer(db, &r),
        };
        if paid.as_ref().map(|t| t.id) != r.txn_id {
            db.receipt.update(Receipt {
                txn_id: paid.as_ref().map(|t| t.id),
                ..r
            })?;
            items_changed = true;
        }
        if paid.is_some() && (changed || items_changed) {
            split_by(db, &id)?;
        }
    }
    Ok(())
}

/// Items filed under a category, or unfiled; every other unfiled item with
/// the same key as one of them filed with it; and the transactions their
/// receipts paid for split again.
#[mutator("receipts.file_items")]
pub fn file_items(_: &Ctx, db: &mut Db<Money>, input: ClassifyItems) -> Result<(), Refusal> {
    if let Some(c) = input.category_id {
        match db.category.get(c) {
            None => return Err(Refusal::new("no such category")),
            Some(g) if g.parent_id.is_none() => return Err(Refusal::new(format!("{} is a group: file it under a category in it", g.name))),
            Some(_) => {}
        }
    }
    let mut receipts: Vec<String> = vec![];
    for r in &input.items {
        let item = db.item.get((r.receipt_id.clone(), r.line)).ok_or_else(|| Refusal::new("no such item"))?;
        let mut same: Vec<Item> = match input.category_id {
            Some(_) => db
                .item
                .by(Item::SAME)
                .from(item.same.clone())
                .all()
                .into_iter()
                .filter(|o| o.category_id.is_none())
                .collect(),
            None => vec![],
        };
        same.push(item);
        for i in same {
            if i.category_id == input.category_id {
                continue;
            }
            if !receipts.contains(&i.receipt_id) {
                receipts.push(i.receipt_id.clone());
            }
            db.item.update(Item {
                category_id: input.category_id,
                ..i
            })?;
        }
    }
    for r in receipts {
        split_by(db, &r)?;
    }
    Ok(())
}

/// A receipt said to be paid by a transaction, or by none; split by it
/// when one.
#[mutator("receipts.pay")]
pub fn pay(_: &Ctx, db: &mut Db<Money>, input: MatchReceipt) -> Result<(), Refusal> {
    let r = db.receipt.get(input.receipt_id.clone()).ok_or_else(|| Refusal::new("no such receipt"))?;
    if let Some(t) = input.txn_id {
        let t = db.txn.get(t).ok_or_else(|| Refusal::new("no such transaction"))?;
        if t.transfer_id.is_some() {
            return Err(Refusal::new("a transfer pays for no receipt"));
        }
        if let Some(other) = db.receipt.by(Receipt::TXN_ID).from(Some(t.id)).all().into_iter().find(|o| o.id != r.id) {
            return Err(Refusal::new(format!("that transaction paid for {} already", other.id)));
        }
    }
    db.receipt.update(Receipt {
        txn_id: input.txn_id,
        ..r.clone()
    })?;
    split_by(db, &r.id)
}

// * The views

/// Every receipt, the newest first, with how many of its items are filed
/// and what paid for it.
#[view("receipts.receipts")]
pub fn receipts(_: &Ctx, _: &Db<Money>, _: ()) -> Result<Plan<ReceiptLine>, Refusal> {
    Ok(scan::<Money, Receipt>()
        .map_with(|db, r| {
            let items = items_of(db, &r.id);
            let t = r.txn_id.and_then(|t| db.txn.get(t));
            ReceiptLine {
                id: r.id.clone(),
                store: r.store.clone(),
                date: r.date.clone(),
                place: r.place.clone(),
                total: r.total,
                items: items.len() as i64,
                filed: items.iter().filter(|i| i.category_id.is_some()).count() as i64,
                txn_id: r.txn_id,
                payee: t.as_ref().map(|t| t.payee.clone()).unwrap_or_default(),
                account: t.and_then(|t| db.account.get(t.account_id)).map(|a| a.name).unwrap_or_default(),
            }
        })
        .sort(|l| (Desc(l.date.clone()), l.id.clone())))
}

/// The items filed under a category or any kind of it, from every store —
/// "Groceries › Eggs" is Costco's, Target's and Amazon's eggs — or (none)
/// every item not filed yet; the newest first.
#[view("receipts.items")]
pub fn items(_: &Ctx, _: &Db<Money>, input: ItemsIn) -> Result<Plan<ItemLine>, Refusal> {
    let under = input.category_id;
    Ok(scan::<Money, Item>()
        .filter_with(move |db, i| match (under, i.category_id) {
            (None, None) => true,
            (Some(u), Some(c)) => ancestry(db, c).iter().any(|a| a.id == u),
            _ => false,
        })
        .map_with(|db, i| {
            let r = db.receipt.get(i.receipt_id.clone()).unwrap_or_default();
            ItemLine {
                receipt_id: i.receipt_id.clone(),
                line: i.line,
                store: r.store,
                date: r.date,
                place: r.place,
                name: i.name.clone(),
                sku: i.sku.clone(),
                quantity: i.quantity,
                unit: i.unit.clone(),
                amount: i.amount,
                unit_price: match i.quantity {
                    0 => i.amount,
                    q => (i.amount as i128 * 1000 / q as i128) as i64,
                },
                category_id: i.category_id,
                category: i.category_id.map(|c| path_of(db, c)).unwrap_or_default(),
            }
        })
        .sort(|l| (Desc(l.date.clone()), l.receipt_id.clone(), l.line)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tax spread in proportion, and every cent accounted for.
    #[test]
    fn shares_add_up_to_the_whole() {
        assert_eq!(shares(-8740, &[-429, -3599, -4181]), vec![-457, -3832, -4451]);
        assert_eq!(shares(-100, &[-1, -1, -1]), vec![-33, -33, -34]);
        assert_eq!(shares(-500, &[-1000, 200]), vec![-625, 125]);
        assert_eq!(shares(-500, &[0, 0]), vec![-500, 0]);
        for (t, p) in [(-12345, vec![-333, -667, -1]), (999, vec![1, 2, 3, 4])] {
            assert_eq!(shares(t, &p).iter().sum::<i64>(), t);
        }
    }

    #[test]
    fn words_fold_plurals_and_case() {
        assert_eq!(words("KS ORGANIC EGGS 24CT"), ["ks", "organic", "egg", "24ct"].map(String::from).to_vec());
        assert_eq!(words("Glass"), vec!["glass".to_string()]);
        assert_eq!(key_of("costco", "", "KS Eggs, 24ct"), "costco:~ks egg 24ct");
        assert_eq!(key_of("amazon", "B07X", "x"), "amazon:B07X");
    }
}
