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
//! those granted its person. [`feed`] adds to an app's scope that a
//! connection holds only its person's sign-ins and roles.
//!
//! The functions are written once over any schema holding the two tables
//! ([`AuthTables`], which [`install!`] implements) and registered by
//! [`register`].

use ark::db::Tab;
use ark::prelude::*;

/// The role that grants roles.
pub const ADMIN: &str = "admin";
/// The functions by name.
pub const REVOKE: &str = "session.revoke";
pub const CURRENT: &str = "session.current";
pub const MINE: &str = "session.mine";
pub const GRANT: &str = "role.grant";
pub const UNGRANT: &str = "role.revoke";

/// One sign-in on one device (`sign_in`: a `session` table's type would be
/// named as an app's own sessions are). Kept after it ends, revoked or
/// expired: an entry authored under it is still its person's.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
#[ark(index(user))]
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

/// A schema holding the two tables: their handles.
pub trait AuthTables: Tables + Sized {
    fn sign_in<'a, 't>(db: &'a Db<'t, Self>) -> &'a Tab<'t, SignIn>;
    fn sign_in_mut<'a, 't>(db: &'a mut Db<'t, Self>) -> &'a mut Tab<'t, SignIn>;
    fn role<'a, 't>(db: &'a Db<'t, Self>) -> &'a Tab<'t, Role>;
    fn role_mut<'a, 't>(db: &'a mut Db<'t, Self>) -> &'a mut Tab<'t, Role>;
}

/// `install!(App)`: [`AuthTables`] for a schema whose tables `sign_in` and
/// `role` are [`SignIn`] and [`Role`]. The app registers the functions
/// with `.with(ark_auth::register::<App>)`.
#[macro_export]
macro_rules! install {
    ($s:ty) => {
        impl $crate::AuthTables for $s {
            fn sign_in<'a, 't>(db: &'a ::ark::db::Db<'t, Self>) -> &'a ::ark::db::Tab<'t, $crate::SignIn> {
                &db.sign_in
            }
            fn sign_in_mut<'a, 't>(db: &'a mut ::ark::db::Db<'t, Self>) -> &'a mut ::ark::db::Tab<'t, $crate::SignIn> {
                &mut db.sign_in
            }
            fn role<'a, 't>(db: &'a ::ark::db::Db<'t, Self>) -> &'a ::ark::db::Tab<'t, $crate::Role> {
                &db.role
            }
            fn role_mut<'a, 't>(db: &'a mut ::ark::db::Db<'t, Self>) -> &'a mut ::ark::db::Tab<'t, $crate::Role> {
                &mut db.role
            }
        }
    };
}

/// The functions, registered by name.
pub fn register<S: AuthTables>(d: Domain) -> Domain {
    d.mutator(REVOKE, revoke::<S>)
        .mutator(GRANT, grant::<S>)
        .mutator(UNGRANT, revoke_role::<S>)
        .query(CURRENT, current::<S>)
        .view(MINE, mine::<S>)
}

/// Whether a session still proves its person at `now`.
pub fn live(s: &SignIn, now: i64) -> bool {
    !s.revoked && s.expires_ms > now
}

/// End one of the caller's own sign-ins; anybody else's is refused. One
/// this store does not hold is nothing here — a device signing out before
/// its session synced — and the server's run, which holds every one,
/// judges it.
pub fn revoke<S: AuthTables>(ctx: &Ctx, db: &mut Db<S>, input: Revoke) -> Result<(), Refusal> {
    match S::sign_in(db).get(input.id) {
        Some(s) if s.user == ctx.user() => S::sign_in_mut(db).update(SignIn { revoked: true, ..s }),
        Some(_) => Err(Refusal::new("not your session")),
        None => Ok(()),
    }
}

fn admin(ctx: &Ctx) -> Result<(), Refusal> {
    match ctx.has_role(ADMIN) {
        true => Ok(()),
        false => Err(Refusal::new("only an admin grants roles")),
    }
}

/// A role granted; held already, nothing.
pub fn grant<S: AuthTables>(ctx: &Ctx, db: &mut Db<S>, input: Grant) -> Result<(), Refusal> {
    admin(ctx)?;
    S::role_mut(db).put(Role {
        user: input.user,
        role: input.role,
    })
}

/// A grant taken back; not held, nothing.
pub fn revoke_role<S: AuthTables>(ctx: &Ctx, db: &mut Db<S>, input: Grant) -> Result<(), Refusal> {
    admin(ctx)?;
    S::role_mut(db).delete((input.user, input.role))
}

/// Who the caller is: its session while it is live, holding the roles it
/// was opened with and those granted its person, sorted, once each.
pub fn current<S: AuthTables>(ctx: &Ctx, db: &Db<S>, _: ()) -> Result<Option<SignIn>, Refusal> {
    let Some(mut s) = S::sign_in(db).get(ctx.session.clone()).filter(|s| live(s, ctx.now)) else {
        return Ok(None);
    };
    let granted = S::role(db).by(Role::KEY).from(s.user.clone()).all();
    s.roles.extend(granted.into_iter().map(|r| r.role));
    s.roles.sort();
    s.roles.dedup();
    Ok(Some(s))
}

/// The caller's live sign-ins, newest first.
pub fn mine<S: AuthTables>(ctx: &Ctx, _: &Db<S>, _: ()) -> Result<Plan<S, SignIn>, Refusal> {
    let (user, now) = (ctx.user().to_string(), ctx.now);
    Ok(scan::<S, SignIn>()
        .filter(move |s| s.user == user && live(s, now))
        .sort(|s| (Desc(s.issued_ms), s.id.clone())))
}

/// `scope` holding, of the sign-ins and the roles, only the connection's
/// person's (§1.8).
pub fn feed<S: AuthTables>(scope: Scope<S>) -> Scope<S> {
    scope
        .on::<SignIn>(|ctx, s| s.user == ctx.user())
        .on::<Role>(|ctx, r| r.user == ctx.user())
}
