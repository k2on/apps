//! Who is signed in, as rows of the app's own log (`docs/plan-v5.md` §1.16,
//! S1): a sign-in is a [`SignIn`] row and a granted role a [`Role`] row,
//! two tables every app that signs people in puts in its schema.
//!
//! The server writes a sign-in as an entry it authors itself — a raw write
//! no client's intent can name (`server::Auth::issue`) — and judges every
//! token by it: the token is the row's id and the server's HMAC of it, so
//! the row holds no secret and syncs like any other. A device reads who it
//! is with [`current`], lists its person's sign-ins with [`mine`] and ends
//! one with [`revoke`]; the hub closes every connection of a session
//! revoked or expired. Roles are granted by an admin ([`grant`],
//! [`revoke_role`]); a session holds the roles it was opened with and
//! those granted its person. Each row says its owner, so a connection fed
//! by scope ([`FEED`]) holds only its person's sign-ins and roles.
//!
//! The functions are written once over any schema holding the two tables
//! (`db.tab::<SignIn>()`) and registered by [`register`]:
//! `.with(ark_auth::register::<App>)`.

use ark::prelude::*;

/// The role that grants roles.
pub const ADMIN: &str = "admin";
/// The functions by name.
pub const REVOKE: &str = "session.revoke";
pub const MINE: &str = "session.mine";
pub const GRANT: &str = "role.grant";
pub const UNGRANT: &str = "role.revoke";
/// The scope a server feeds every connection by when it feeds one by
/// scope (§1.8): each table whole, unless its row says its owner.
pub const FEED: &str = "session.feed";

/// One sign-in on one device (`sign_in`: a `session` table's type would be
/// named as an app's own sessions are). Kept after it ends, revoked or
/// expired: an entry authored under it is still its person's.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
#[ark(index(user), owner = user)]
pub struct SignIn {
    #[ark(key)]
    pub id: String,
    pub user: String,
    pub name: String,
    pub email: String,
    /// What signed in, as its browser or program said it.
    pub device: String,
    pub issued_ms: i64,
    pub expires_ms: i64,
    pub revoked: bool,
    /// The roles it was opened with: a dev name's (`alice:library`), a
    /// server's own peer's.
    pub roles: Vec<String>,
    /// The person's picture as the provider said it at this sign-in
    /// ([`crate::Account::picture`]). Defaulted: a row written before it
    /// was added has none, and without a default it cannot be read at all.
    #[serde(default)]
    pub picture: String,
}

/// A role granted to a person.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
#[ark(owner = user)]
pub struct Role {
    #[ark(key)]
    pub user: String,
    #[ark(key)]
    pub role: String,
}

/// Which sign-in to end.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Revoke {
    pub id: String,
}

/// Who signed in for the first time, as [`server::Auth::welcome`] hands it
/// to the function an app names: authored by the server's authority, once a
/// person's first sign-in row is written.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Welcome {
    pub user: String,
    pub name: String,
}

/// A role and who it is granted to, or taken from.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Grant {
    #[ark(trim, non_empty, why = "a grant needs somebody to grant to")]
    pub user: String,
    #[ark(trim, non_empty, why = "a grant needs a role")]
    pub role: String,
}

/// The functions, registered by name.
pub fn register<S: Tables>(d: Domain) -> Domain {
    d.mutator(REVOKE, revoke::<S>)
        .mutator(GRANT, grant::<S>)
        .mutator(UNGRANT, revoke_role::<S>)
        .view(MINE, mine::<S>)
        .scope(FEED, Scope::<S>::new())
}

/// Whether a session still proves its person at `now`.
pub fn live(s: &SignIn, now: i64) -> bool {
    !s.revoked && s.expires_ms > now
}

/// End one of the caller's own sign-ins; anybody else's is refused. One
/// this store does not hold is nothing here — a device signing out before
/// its session synced — and the server's run, which holds every one,
/// judges it.
pub fn revoke<S: Tables>(ctx: &Ctx, db: &mut Db<S>, input: Revoke) -> Result<(), String> {
    match db.tab::<SignIn>().get(input.id) {
        Some(s) if s.user == ctx.user() => db.tab::<SignIn>().update(SignIn { revoked: true, ..s }),
        Some(_) => Err("not your session".into()),
        None => Ok(()),
    }
}

fn admin(ctx: &Ctx) -> Result<(), String> {
    match ctx.has_role(ADMIN) {
        true => Ok(()),
        false => Err("only an admin grants roles".into()),
    }
}

/// A role granted; held already, nothing.
pub fn grant<S: Tables>(ctx: &Ctx, db: &mut Db<S>, input: Grant) -> Result<(), String> {
    admin(ctx)?;
    db.tab::<Role>().put(Role {
        user: input.user,
        role: input.role,
    })
}

/// A grant taken back; not held, nothing.
pub fn revoke_role<S: Tables>(ctx: &Ctx, db: &mut Db<S>, input: Grant) -> Result<(), String> {
    admin(ctx)?;
    db.tab::<Role>().delete((input.user, input.role))
}

/// The two tables alone: what [`current`] reads any store holding them as.
#[derive(Schema)]
pub struct Signed {
    pub sign_in: SignIn,
    pub role: Role,
}

/// Who the caller is, over a store holding the two tables: its session
/// while it is live, holding the roles it was opened with and those
/// granted its person, sorted, once each.
pub fn current(ctx: &Ctx, store: &ark::store::Store) -> Option<SignIn> {
    ark::db::read::<Signed, _>(store, |db| {
        let mut s = db.sign_in.get(ctx.session.clone()).filter(|s| live(s, ctx.now))?;
        let granted = db.role.by(Role::KEY).from(s.user.clone()).all();
        s.roles.extend(granted.into_iter().map(|r| r.role));
        s.roles.sort();
        s.roles.dedup();
        Some(s)
    })
}

/// The caller's live sign-ins, newest first.
pub fn mine<S: Tables>(ctx: &Ctx, _: &Db<S>, _: ()) -> Result<Plan<S, SignIn>, String> {
    let (user, now) = (ctx.user().to_string(), ctx.now);
    Ok(scan::<S, SignIn>()
        .filter(move |s| s.user == user && live(s, now))
        .sort(|s| (Desc(s.issued_ms), s.id.clone())))
}
