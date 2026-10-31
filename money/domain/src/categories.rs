//! The budget's shape: groups, and the categories in them. Anybody in the
//! household changes it; the budget's own group and its one category
//! ([`crate::budget::ready_to_assign`]) are not anybody's to change, and
//! the card payments' group and its categories ([`crate::cards`]) are the
//! cards': hidden or put in order, but named, made and deleted as the
//! cards are.

use ark::prelude::*;

use crate::budget::{ensure_setup, inflow, ready_to_assign};
use crate::cards::card_group;
use crate::schema::*;
use crate::types::*;

fn group_of(db: &Db<Money>, id: Id<Group>) -> Result<Group, Refusal> {
    match db.group.get(id) {
        Some(g) if g.income => Err(Refusal::new("the budget's own group is not to change")),
        Some(g) => Ok(g),
        None => Err(Refusal::new("no such group")),
    }
}

/// A group whose name and categories are people's: not the budget's own,
/// nor the card payments' — those are the cards' (hidden or moved, yes).
fn named_group_of(db: &Db<Money>, id: Id<Group>) -> Result<Group, Refusal> {
    match group_of(db, id)? {
        g if g.id == card_group() => Err(Refusal::new("the card payments' group is the cards': it changes as they do")),
        g => Ok(g),
    }
}

fn category_of(db: &Db<Money>, id: Id<Category>) -> Result<Category, Refusal> {
    match db.category.get(id) {
        Some(_) if id == ready_to_assign() => Err(Refusal::new("Ready to Assign is not to change")),
        Some(c) => Ok(c),
        None => Err(Refusal::new("no such category")),
    }
}

/// A category people name and keep: not a card's payment category, which
/// is named as its card is and goes when the card does.
fn named_category_of(db: &Db<Money>, id: Id<Category>) -> Result<Category, Refusal> {
    match category_of(db, id)? {
        c if c.group_id == card_group() => Err(Refusal::new("a card's payment category is the card's: rename or close the card")),
        c => Ok(c),
    }
}

/// A group, after every other.
#[mutator("categories.add_group")]
pub fn add_group(ctx: &Ctx, db: &mut Db<Money>, input: AddGroup) -> Result<(), Refusal> {
    ensure_setup(ctx, db)?;
    let pos = db.group.by(Group::POS).whole().last().map_or(0, |g| g.pos) + 1;
    db.group.insert(Group {
        id: ctx.new_id(),
        name: input.name,
        pos,
        hidden: false,
        income: false,
        created_ms: ctx.now,
        user_id: ctx.user().into(),
    })
}

#[mutator("categories.rename_group")]
pub fn rename_group(_: &Ctx, db: &mut Db<Money>, input: RenameGroup) -> Result<(), Refusal> {
    let g = named_group_of(db, input.id)?;
    db.group.update(Group { name: input.name, ..g })
}

#[mutator("categories.hide_group")]
pub fn hide_group(_: &Ctx, db: &mut Db<Money>, input: HideGroup) -> Result<(), Refusal> {
    let g = group_of(db, input.id)?;
    db.group.update(Group { hidden: input.hidden, ..g })
}

/// A group gone, each of its categories as [`delete_category`] does it.
#[mutator("categories.delete_group")]
pub fn delete_group(ctx: &Ctx, db: &mut Db<Money>, input: DeleteGroup) -> Result<(), Refusal> {
    let g = named_group_of(db, input.id)?;
    if let Some(to) = input.move_to {
        if db.category.get(to).is_some_and(|c| c.group_id == g.id) {
            return Err(Refusal::new("move its money somewhere outside the group"));
        }
    }
    for c in db.category.by(Category::GROUP_ID_POS).from(g.id).all() {
        delete(ctx, db, c, input.move_to)?;
    }
    db.group.remove(&g)
}

/// A category, at the end of its group.
#[mutator("categories.add_category")]
pub fn add_category(ctx: &Ctx, db: &mut Db<Money>, input: AddCategory) -> Result<(), Refusal> {
    let g = named_group_of(db, input.group_id)?;
    let last = db.category.by(Category::POS).whole().last().map_or(0, |c| c.pos);
    db.category.insert(Category {
        id: ctx.new_id(),
        group_id: g.id,
        name: input.name,
        pos: last + 1,
        created_ms: ctx.now,
        user_id: ctx.user().into(),
        ..Category::default()
    })
}

#[mutator("categories.rename_category")]
pub fn rename_category(_: &Ctx, db: &mut Db<Money>, input: RenameCategory) -> Result<(), Refusal> {
    let c = named_category_of(db, input.id)?;
    db.category.update(Category { name: input.name, ..c })
}

#[mutator("categories.hide_category")]
pub fn hide_category(_: &Ctx, db: &mut Db<Money>, input: HideCategory) -> Result<(), Refusal> {
    let c = category_of(db, input.id)?;
    db.category.update(Category { hidden: input.hidden, ..c })
}

/// A category into a group, in front of `before` (one of that group's) or
/// at its end. The group's categories are numbered again in their new
/// order, so where it lands is what the order says and not arithmetic on
/// neighbours' numbers.
#[mutator("categories.move_category")]
pub fn move_category(_: &Ctx, db: &mut Db<Money>, input: MoveCategory) -> Result<(), Refusal> {
    let c = category_of(db, input.id)?;
    let g = group_of(db, input.group_id)?;
    // The cards' payment categories are put in order among themselves,
    // and nothing else is put among them.
    if (c.group_id == card_group()) != (g.id == card_group()) {
        return Err(Refusal::new("a card's payment category stays with the cards"));
    }
    let mut order: Vec<Category> = db
        .category
        .by(Category::GROUP_ID_POS)
        .from(g.id)
        .all()
        .into_iter()
        .filter(|o| o.id != c.id)
        .collect();
    let at = match input.before {
        Some(b) if b == c.id => return Err(Refusal::new("a category does not go in front of itself")),
        Some(b) => order
            .iter()
            .position(|o| o.id == b)
            .ok_or_else(|| Refusal::new("that category is not in the group"))?,
        None => order.len(),
    };
    order.insert(at, Category { group_id: g.id, ..c });
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
        .group
        .by(Group::POS)
        .whole()
        .all()
        .into_iter()
        .filter(|o| o.id != g.id && !o.income)
        .collect();
    let at = match input.before {
        Some(b) if b == g.id => return Err(Refusal::new("a group does not go in front of itself")),
        Some(b) => order.iter().position(|o| o.id == b).ok_or_else(|| Refusal::new("no such group"))?,
        None => order.len(),
    };
    order.insert(at, g);
    for (i, o) in order.into_iter().enumerate() {
        let pos = i as i64 + 1;
        if o.pos != pos {
            db.group.update(Group { pos, ..o })?;
        }
    }
    Ok(())
}

#[mutator("categories.set_target")]
pub fn set_target(_: &Ctx, db: &mut Db<Money>, input: SetTarget) -> Result<(), Refusal> {
    let c = category_of(db, input.id)?;
    db.category.update(Category { target: input.target, ..c })
}

/// A category gone: its transactions moved to `move_to` (or to no
/// category), and what each month gave it given to `move_to` instead (or
/// back to be assigned).
#[mutator("categories.delete_category")]
pub fn delete_category(ctx: &Ctx, db: &mut Db<Money>, input: DeleteCategory) -> Result<(), Refusal> {
    let c = named_category_of(db, input.id)?;
    delete(ctx, db, c, input.move_to)
}

fn delete(ctx: &Ctx, db: &mut Db<Money>, c: Category, move_to: Option<Id<Category>>) -> Result<(), Refusal> {
    if move_to == Some(c.id) {
        return Err(Refusal::new("move its money to another category"));
    }
    if let Some(to) = move_to {
        if !db.category.has(to) {
            return Err(Refusal::new("no such category to move to"));
        }
    }
    for t in db.txn.by(Txn::CATEGORY_ID_DATE).from(Some(c.id)).all() {
        db.txn.update(Txn { category_id: move_to, ..t })?;
    }
    let into = move_to.filter(|to| db.category.get(*to).is_some_and(|c| c.group_id != inflow()));
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
    db.category.remove(&c)
}
