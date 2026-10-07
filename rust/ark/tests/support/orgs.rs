//! A domain with scopes (`docs/plan-guards.md` D2), shared by the tests of
//! what a person holds: organisations, who is a member of which, and
//! accounts whose password nobody but an admin holds.
//!
//! - `orgs`, scoped on the router by `memberships`: the orgs a person is a
//!   member of (the `exists` form), every org for an admin, and a
//!   person's own membership rows (every membership for an admin).
//! - `accounts`, with two chains: `own` holds a person's own account
//!   without its password, and `admin` — a guard on `admin`, then a scope
//!   holding every account whole for an admin.

// The row structs are the vocabulary's: a body reads their fields only
// natively.
#![allow(dead_code)]

use ark::authoring::*;

pub struct Orgs {
    pub org: Table<Org>,
    pub member: Table<Member>,
    pub account: Table<Account>,
}
impl Tables for Orgs {
    fn open() -> Self {
        Orgs {
            org: table(),
            member: table(),
            account: table(),
        }
    }
}

pub struct Org {
    pub id: Id<Org>,
    pub name: Text,
}
impl Row for Org {
    const NAME: &str = "org";
    type Key = (Id<Org>,);
    fn columns() -> Columns<Self> {
        columns().id(Self::id).text(Self::name).key((Self::id,))
    }
}
#[allow(non_upper_case_globals)]
impl Org {
    pub const id: Col<Self, Id<Self>> = col("id");
    pub const name: Col<Self, Text> = col("name");
}

pub struct Member {
    pub org_id: Id<Org>,
    pub user: Text,
}
impl Row for Member {
    const NAME: &str = "member";
    type Key = (Id<Org>, Text);
    fn columns() -> Columns<Self> {
        columns().id(Self::org_id).refs::<Org>().text(Self::user).key((Self::org_id, Self::user))
    }
}
#[allow(non_upper_case_globals)]
impl Member {
    pub const org_id: Col<Self, Id<Org>> = col("org_id");
    pub const user: Col<Self, Text> = col("user");
}

pub struct Account {
    pub user: Text,
    pub name: Text,
    pub password: Text,
}
impl Row for Account {
    const NAME: &str = "account";
    type Key = (Text,);
    fn columns() -> Columns<Self> {
        columns().text(Self::user).text(Self::name).text(Self::password).key((Self::user,))
    }
}
#[allow(non_upper_case_globals)]
impl Account {
    pub const user: Col<Self, Text> = col("user");
    pub const name: Col<Self, Text> = col("name");
    pub const password: Col<Self, Text> = col("password");
}

pub struct NewOrg {
    pub name: Text,
}
impl Input for NewOrg {
    fn schema() -> Object<Self> {
        object().field("name", text().min(1))
    }
}

pub struct Join {
    pub org_id: Id<Org>,
    pub user: Text,
}
impl Input for Join {
    fn schema() -> Object<Self> {
        object().field("org_id", id::<Org>().exists()).field("user", text().min(1))
    }
}

pub struct SignUp {
    pub user: Text,
    pub name: Text,
    pub password: Text,
}
impl Input for SignUp {
    fn schema() -> Object<Self> {
        object().field("user", text().min(1)).field("name", text()).field("password", text())
    }
}

/// The orgs router: its scope on the router, so every procedure inherits it.
pub fn orgs() -> Router<Orgs> {
    let orgs = router::<Orgs>("orgs");
    let scoped = orgs.server("memberships", |ctx, db| {
        (
            db.org
                .filter(exists(Member::org_id, Member::user.eq(ctx.user)).or(Pred::when(ctx.has_role("admin")))),
            db.member.filter(Member::user.eq(ctx.user).or(Pred::when(ctx.has_role("admin")))),
        )
    });
    scoped.routes((
        scoped.input::<NewOrg>().mutation("create_org", |ctx, db, input| {
            let id = ctx.new_id("id");
            db.org.insert(Org { id, name: input.name });
            db.member.insert(Member { org_id: id, user: ctx.user })
        }),
        scoped.input::<Join>().mutation("join", |_ctx, db, input| {
            db.member.insert(Member {
                org_id: input.org_id,
                user: input.user,
            })
        }),
        scoped.client("my_orgs", |_ctx, db, ()| db.org.rows().order_by(Org::name.asc())),
    ))
}

/// The accounts router: a person's own account without its password on one
/// chain, every account whole for an admin on another.
pub fn accounts() -> Router<Orgs> {
    let accounts = router::<Orgs>("accounts");
    let own = accounts.server("own_account", |ctx, db| {
        db.account.filter(Account::user.eq(ctx.user)).exclude(Account::password)
    });
    let is_admin = accounts.guard("is_admin", |ctx, _db| unless(ctx.has_role("admin"), || refuse("admins only")));
    let admin = is_admin.server("all_accounts", |ctx, db| db.account.filter(Pred::when(ctx.has_role("admin"))));
    accounts.routes((
        own.client("me", |_ctx, db, ()| db.account.rows()),
        admin.input::<SignUp>().mutation("sign_up", |_ctx, db, input| {
            db.account.upsert(Account {
                user: input.user,
                name: input.name,
                password: input.password,
            })
        }),
    ))
}

/// Both routers.
pub fn module() -> Module {
    Module::new((orgs(), accounts()))
}
