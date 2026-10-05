//! `docs/plan-auth.md` A table's rules, from the vocabulary to the wire.
//!
//! The vocabulary says a rule as a domain reads it — `visible(Everyone)`,
//! `writable(Role("library"))`, `Self::user_id.is(Me)`, the one lookup
//! through a reference — and this holds it to the IR the schema carries,
//! which is what is encoded, hashed and enforced.

#![allow(non_upper_case_globals, dead_code)]

use ark::authoring::*;
use ark::ir::{CmpOp, Expr, Pred as IrPred};
use ark::schema::{check_schema, Schema};

pub struct Owner {
    pub id: Id<Owner>,
    pub user_id: Text,
    pub name: Text,
}
impl Row for Owner {
    const NAME: &str = "owner";
    type Key = (Id<Owner>,);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::id)
            .text(Self::user_id)
            .text(Self::name)
            .key((Self::id,))
            .visible(Self::user_id.is(Me).or(exists(Member::owner_id, Member::user_id.is(Me))))
            .writable(Self::user_id.is(Me))
    }
}
impl Owner {
    pub const id: Col<Self, Id<Self>> = col("id");
    pub const user_id: Col<Self, Text> = col("user_id");
    pub const name: Col<Self, Text> = col("name");
}

pub struct Member {
    pub id: Id<Member>,
    pub owner_id: Id<Owner>,
    pub user_id: Text,
}
impl Row for Member {
    const NAME: &str = "member";
    type Key = (Id<Member>,);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::id)
            .id(Self::owner_id)
            .refs::<Owner>()
            .text(Self::user_id)
            .key((Self::id,))
            .visible(Everyone)
            .writable(Role("library"))
    }
}
impl Member {
    pub const id: Col<Self, Id<Self>> = col("id");
    pub const owner_id: Col<Self, Id<Owner>> = col("owner_id");
    pub const user_id: Col<Self, Text> = col("user_id");
}

/// The vocabulary writes the IR the design names: `Me` is the context's
/// user, a role is a leaf, the lookup names the referencing table and its
/// column with that table's own predicate, and `Everyone` is nothing at
/// all. Falsified by writing `Everyone` as an always-true predicate (an
/// empty `All`): the `member` table's `visible` was `Some`.
#[test]
fn the_vocabulary_writes_the_rule_the_schema_carries() {
    let me = |c: &str| IrPred::Cmp(c.into(), CmpOp::Eq, Expr::CtxUser);
    let owner = table_of::<Owner>();
    assert_eq!(
        owner.visible,
        Some(IrPred::Any(vec![
            me("user_id"),
            IrPred::Exists("member".into(), "owner_id".into(), Box::new(me("user_id")))
        ]))
    );
    assert_eq!(owner.writable, Some(me("user_id")));
    let member = table_of::<Member>();
    assert_eq!(member.visible, None, "Everyone is not written down");
    assert_eq!(member.writable, Some(IrPred::Role("library".into())));
    let sch = Schema { tables: vec![owner, member] };
    assert_eq!(check_schema(&sch), vec![]);
}

pub struct Db {
    pub owner: Table<Owner>,
    pub member: Table<Member>,
}
impl Tables for Db {
    fn open() -> Self {
        Db {
            owner: table(),
            member: table(),
        }
    }
}

/// A rule's leaves are a table's and are refused in a plan: a role is asked
/// of an identity, and a read has none. The same module with the filter on
/// a column instead verifies. Falsified by letting `pred_ok` accept a
/// role: the module built.
#[test]
fn a_role_in_a_plan_is_refused() {
    let routes = |role: bool| {
        let r = router::<Db>("r");
        r.routes((r.query("owners", move |_ctx, db, _input: ()| {
            if role {
                db.owner.filter(Pred::from(Role("admin")))
            } else {
                db.owner.filter(Owner::name.eq("x"))
            }
        }),))
    };
    assert!(Module::new((routes(false),)).try_build().is_ok());
    let errs = Module::new((routes(true),)).try_build().expect_err("a role in a filter is refused");
    assert!(errs.iter().any(|e| e.contains("RuleLeafInPlan")), "{errs:?}");
}
