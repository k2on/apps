//! The budget's shape: one tree of categories. A heading (no parent) is
//! what the budget draws as a group; under it, envelopes, which a month
//! gives money to; under an envelope, at any depth, kinds of it
//! ("Groceries › Eggs"), whose spending is the envelope's. Anybody in the
//! household changes it; the budget's own heading and its one category
//! ([`crate::budget::ready_to_assign`]) are not anybody's to change.
//!
//! The mutators keep the names they had when groups were a table of their
//! own: a group is a heading, and `group_id` is the heading a category is
//! under.

use ark::prelude::*;

use crate::budget::{depth, ensure_setup, inflow, is_income, ready_to_assign, subtree};
use crate::schema::*;
use crate::types::*;

/// A heading somebody may change.
fn group_of(db: &Db<Money>, id: Id<Group>) -> Result<Group, Refusal> {
    match db.category.get(id) {
        Some(g) if g.income => Err(Refusal::new("the budget's own group is not to change")),
        Some(g) if g.parent_id.is_none() => Ok(g),
        Some(g) => Err(Refusal::new(format!("{} is a category, not a group", g.name))),
        None => Err(Refusal::new("no such group")),
    }
}

/// A category (an envelope, or a kind of one) somebody may change.
fn category_of(db: &Db<Money>, id: Id<Category>) -> Result<Category, Refusal> {
    match db.category.get(id) {
        Some(_) if id == ready_to_assign() || is_income(db, id) => Err(Refusal::new("Ready to Assign is not to change")),
        Some(c) if c.parent_id.is_none() => Err(Refusal::new(format!("{} is a group", c.name))),
        Some(c) => Ok(c),
        None => Err(Refusal::new("no such category")),
    }
}

/// After every other.
fn last_pos(db: &Db<Money>) -> i64 {
    db.category.by(Category::POS).whole().last().map_or(0, |c| c.pos) + 1
}

/// A group, after every other.
#[mutator("categories.add_group")]
pub fn add_group(ctx: &Ctx, db: &mut Db<Money>, input: AddGroup) -> Result<(), Refusal> {
    ensure_setup(ctx, db)?;
    let pos = db
        .category
        .by(Category::PARENT_ID_POS)
        .from(None)
        .all()
        .iter()
        .map(|g| g.pos)
        .max()
        .unwrap_or(0)
        + 1;
    db.category.insert(Category {
        id: ctx.new_id(),
        name: input.name,
        pos,
        created_ms: ctx.now,
        user_id: ctx.user().into(),
        ..Category::default()
    })
}

#[mutator("categories.rename_group")]
pub fn rename_group(_: &Ctx, db: &mut Db<Money>, input: RenameGroup) -> Result<(), Refusal> {
    let g = group_of(db, input.id)?;
    db.category.update(Category { name: input.name, ..g })
}

#[mutator("categories.hide_group")]
pub fn hide_group(_: &Ctx, db: &mut Db<Money>, input: HideGroup) -> Result<(), Refusal> {
    let g = group_of(db, input.id)?;
    db.category.update(Category { hidden: input.hidden, ..g })
}

/// A group gone, with everything under it, each as [`delete_category`]
/// does it.
#[mutator("categories.delete_group")]
pub fn delete_group(ctx: &Ctx, db: &mut Db<Money>, input: DeleteGroup) -> Result<(), Refusal> {
    let g = group_of(db, input.id)?;
    delete(ctx, db, g, input.move_to)
}

/// A category, at the end of its group.
#[mutator("categories.add_category")]
pub fn add_category(ctx: &Ctx, db: &mut Db<Money>, input: AddCategory) -> Result<(), Refusal> {
    let g = group_of(db, input.group_id)?;
    let pos = last_pos(db);
    db.category.insert(Category {
        id: ctx.new_id(),
        parent_id: Some(g.id),
        name: input.name,
        pos,
        created_ms: ctx.now,
        user_id: ctx.user().into(),
        ..Category::default()
    })
}

/// A kind of a category, under it: "Eggs" under "Groceries", or under
/// another kind. Its spending is the envelope's; nothing is assigned to it.
#[mutator("categories.add_subcategory")]
pub fn add_subcategory(ctx: &Ctx, db: &mut Db<Money>, input: AddSubcategory) -> Result<(), Refusal> {
    let parent = category_of(db, input.parent_id)?;
    if depth(db, parent.id).unwrap_or(0) + 1 >= crate::budget::DEEPEST {
        return Err(Refusal::new("that is as deep as categories go"));
    }
    let pos = last_pos(db);
    db.category.insert(Category {
        id: ctx.new_id(),
        parent_id: Some(parent.id),
        name: input.name,
        pos,
        created_ms: ctx.now,
        user_id: ctx.user().into(),
        ..Category::default()
    })
}

#[mutator("categories.rename_category")]
pub fn rename_category(_: &Ctx, db: &mut Db<Money>, input: RenameCategory) -> Result<(), Refusal> {
    let c = category_of(db, input.id)?;
    db.category.update(Category { name: input.name, ..c })
}

#[mutator("categories.hide_category")]
pub fn hide_category(_: &Ctx, db: &mut Db<Money>, input: HideCategory) -> Result<(), Refusal> {
    let c = category_of(db, input.id)?;
    db.category.update(Category { hidden: input.hidden, ..c })
}

/// A category under another, in front of `before` (one of that parent's)
/// or at its end. An envelope moves into a group (`group_id`, a heading); a
/// kind moves under any category but itself and its own kinds, and stays a
/// kind — so what was assigned is always to an envelope. The parent's
/// children are numbered again in their new order, so where it lands is
/// what the order says and not arithmetic on neighbours' numbers.
#[mutator("categories.move_category")]
pub fn move_category(_: &Ctx, db: &mut Db<Money>, input: MoveCategory) -> Result<(), Refusal> {
    let c = category_of(db, input.id)?;
    let envelope = depth(db, c.id) == Some(1);
    let parent = match envelope {
        true => group_of(db, input.group_id)?,
        false => {
            let p = category_of(db, input.group_id)?;
            if subtree(db, c.id).iter().any(|s| s.id == p.id) {
                return Err(Refusal::new("a category does not go under itself"));
            }
            p
        }
    };
    let mut order: Vec<Category> = crate::budget::children(db, parent.id).into_iter().filter(|o| o.id != c.id).collect();
    let at = match input.before {
        Some(b) if b == c.id => return Err(Refusal::new("a category does not go in front of itself")),
        Some(b) => order
            .iter()
            .position(|o| o.id == b)
            .ok_or_else(|| Refusal::new("that category is not in the group"))?,
        None => order.len(),
    };
    order.insert(
        at,
        Category {
            parent_id: Some(parent.id),
            ..c.clone()
        },
    );
    for (i, o) in order.into_iter().enumerate() {
        let pos = i as i64 + 1;
        if o.pos != pos || o.id == c.id {
            db.category.update(Category { pos, ..o })?;
        }
    }
    Ok(())
}

/// A group in front of `before`, or after every other; the groups are
/// numbered again in their new order, as [`move_category`] numbers a
/// group's categories.
#[mutator("categories.move_group")]
pub fn move_group(_: &Ctx, db: &mut Db<Money>, input: MoveGroup) -> Result<(), Refusal> {
    let g = group_of(db, input.id)?;
    let mut order: Vec<Group> = db
        .category
        .by(Category::PARENT_ID_POS)
        .from(None)
        .all()
        .into_iter()
        .filter(|o| o.id != g.id && !o.income)
        .collect();
    order.sort_by_key(|o| (o.pos, o.id));
    let at = match input.before {
        Some(b) if b == g.id => return Err(Refusal::new("a group does not go in front of itself")),
        Some(b) => order.iter().position(|o| o.id == b).ok_or_else(|| Refusal::new("no such group"))?,
        None => order.len(),
    };
    order.insert(at, g);
    for (i, o) in order.into_iter().enumerate() {
        let pos = i as i64 + 1;
        if o.pos != pos {
            db.category.update(Category { pos, ..o })?;
        }
    }
    Ok(())
}

/// What an envelope wants each month. A kind's money is its envelope's,
/// and so is its target.
#[mutator("categories.set_target")]
pub fn set_target(_: &Ctx, db: &mut Db<Money>, input: SetTarget) -> Result<(), Refusal> {
    let c = category_of(db, input.id)?;
    if depth(db, c.id) != Some(1) {
        return Err(Refusal::new(format!("{}'s target is the category it is a kind of", c.name)));
    }
    db.category.update(Category { target: input.target, ..c })
}

/// A category gone, and every kind of it: their transactions, split lines
/// and receipts' items moved to `move_to` (or to no category), and what
/// each month gave it given to `move_to`'s envelope instead (or back to be
/// assigned).
#[mutator("categories.delete_category")]
pub fn delete_category(ctx: &Ctx, db: &mut Db<Money>, input: DeleteCategory) -> Result<(), Refusal> {
    let c = category_of(db, input.id)?;
    delete(ctx, db, c, input.move_to)
}

fn delete(ctx: &Ctx, db: &mut Db<Money>, c: Category, move_to: Option<Id<Category>>) -> Result<(), Refusal> {
    let gone = subtree(db, c.id);
    if let Some(to) = move_to {
        if gone.iter().any(|g| g.id == to) {
            return Err(Refusal::new(match c.parent_id {
                None => "move its money somewhere outside the group",
                Some(_) => "move its money to another category",
            }));
        }
        match db.category.get(to) {
            None => return Err(Refusal::new("no such category to move to")),
            Some(t) if t.parent_id.is_none() => return Err(Refusal::new("move its money to a category, not a group")),
            Some(_) => {}
        }
    }
    let into = move_to
        .filter(|to| *to != ready_to_assign() && db.category.get(*to).is_some_and(|c| c.parent_id != Some(inflow())))
        .and_then(|to| crate::budget::envelope_of(db, to))
        .map(|e| e.id);
    // The deepest first, so that no row is left naming a parent gone.
    for c in gone.into_iter().rev() {
        for t in db.txn.by(Txn::CATEGORY_ID_DATE).from(Some(c.id)).all() {
            db.txn.update(Txn { category_id: move_to, ..t })?;
        }
        for s in db.split.by(Split::CATEGORY_ID_DATE).from(Some(c.id)).all() {
            db.split.update(Split { category_id: move_to, ..s })?;
        }
        for i in db.item.by(Item::CATEGORY_ID).from(Some(c.id)).all() {
            db.item.update(Item { category_id: move_to, ..i })?;
        }
        for a in db.assigned.by(Assigned::CATEGORY_ID_MONTH).from(c.id).all() {
            if let Some(to) = into {
                let had = crate::budget::assigned_in(db, &a.month, to);
                db.assigned.put(Assigned {
                    month: a.month.clone(),
                    category_id: to,
                    amount: had + a.amount,
                    at_ms: ctx.now,
                    user_id: ctx.user().into(),
                })?;
            }
            db.assigned.remove(&a)?;
        }
        db.category.remove(&c)?;
    }
    Ok(())
}

// * The views

/// Every category, the whole tree in order — each heading, then what is
/// under it, depth first: what a picker that reaches past the envelopes
/// draws, and where a receipt's item can be filed.
#[view("categories.tree")]
pub fn tree(_: &Ctx, _: &Db<Money>, _: ()) -> Result<crate::budget::Plan<CategoryNode>, Refusal> {
    Ok(scan::<Money, Category>().map_with(node_of).sort(|n| (n.order.clone(), n.id)))
}

/// A category as the tree draws it.
pub fn node_of(db: &Db<Money>, c: &Category) -> CategoryNode {
    let mut up = crate::budget::ancestry(db, c.id);
    up.reverse();
    CategoryNode {
        id: c.id,
        parent_id: c.parent_id,
        name: c.name.clone(),
        path: up.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(" › "),
        depth: up.len() as i64 - 1,
        hidden: up.iter().any(|a| a.hidden),
        income: up.first().is_some_and(|h| h.income),
        order: up.iter().map(|a| format!("{:012}.{}", a.pos.max(0), a.id.hex())).collect::<Vec<_>>().join("/"),
    }
}
