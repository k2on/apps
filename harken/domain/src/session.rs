//! Who is signed in (`docs/plan-v5.md` §1.16, S1): a sign-in is a row of
//! the log, and a role a grant is a row beside it.
//!
//! The server writes a [`SignIn`] when somebody signs in — an entry it
//! authors itself, a raw write no client's intent can name — and judges
//! every token by it: the token is the row's id with the server's HMAC of
//! it, so the row holds no secret and syncs like any other. A device reads
//! who it is with [`current`], lists its person's sign-ins with [`mine`]
//! and ends one with [`revoke`]; the server closes every connection of a
//! session revoked or expired. [`Role`]s are granted by an admin
//! ([`grant`]); every connection holds the roles its session was opened
//! with and those granted its person. A connection is fed [`feed`]: the
//! whole library, and of the sign-ins and the roles only its person's.

use ark::prelude::*;

use crate::browse::Plan;
use crate::schema::*;

/// The role that grants roles.
pub const ADMIN: &str = "admin";

/// One sign-in on one device (`sign_in`: a `session` table's type would be
/// named as the listening session's is, which the phones' types refuse).
/// Kept after it ends, revoked or expired: an entry authored under it is
/// still its person's.
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
    /// The roles it was opened with: a dev name's (`alice:library`), the
    /// scanner's own.
    pub roles: Vec<String>,
}

/// A role granted to a person.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
pub struct Role {
    #[ark(key)]
    pub user: String,
    #[ark(key)]
    pub role: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Revoke {
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Grant {
    #[ark(trim, non_empty, why = "a grant needs somebody to grant to")]
    pub user: String,
    #[ark(trim, non_empty, why = "a grant needs a role")]
    pub role: String,
}

/// Whether a session still proves its person, now.
pub fn live(ctx: &Ctx, s: &SignIn) -> bool {
    !s.revoked && s.expires_ms > ctx.now
}

/// End one of the caller's own sign-ins; anybody else's is refused. One
/// this store does not hold is nothing here — a device signing out before
/// its session synced — and the server's run, which holds every one,
/// judges it.
#[mutator("session.revoke")]
pub fn revoke(ctx: &Ctx, db: &mut Db<Harken>, input: Revoke) -> Result<(), Refusal> {
    match db.sign_in.get(input.id) {
        Some(s) if s.user == ctx.user() => db.sign_in.update(SignIn { revoked: true, ..s }),
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
#[mutator("role.grant")]
pub fn grant(ctx: &Ctx, db: &mut Db<Harken>, input: Grant) -> Result<(), Refusal> {
    admin(ctx)?;
    db.role.put(Role {
        user: input.user,
        role: input.role,
    })
}

/// A grant taken back; not held, nothing.
#[mutator("role.revoke")]
pub fn revoke_role(ctx: &Ctx, db: &mut Db<Harken>, input: Grant) -> Result<(), Refusal> {
    admin(ctx)?;
    db.role.delete((input.user, input.role))
}

/// Who the caller is: its session while it is live, holding the roles it
/// was opened with and those granted its person, sorted, once each.
#[query("session.current")]
pub fn current(ctx: &Ctx, db: &Db<Harken>, _: ()) -> Result<Option<SignIn>, Refusal> {
    let Some(mut s) = db.sign_in.get(ctx.session.clone()).filter(|s| live(ctx, s)) else {
        return Ok(None);
    };
    s.roles
        .extend(db.role.by(Role::KEY).from(s.user.clone()).all().into_iter().map(|r| r.role));
    s.roles.sort();
    s.roles.dedup();
    Ok(Some(s))
}

/// The caller's live sign-ins, newest first.
#[view("session.mine")]
pub fn mine(ctx: &Ctx, _: &Db<Harken>, _: ()) -> Result<Plan<SignIn>, Refusal> {
    let (user, now) = (ctx.user().to_string(), ctx.now);
    Ok(scan::<Harken, SignIn>()
        .filter(move |s| s.user == user && !s.revoked && s.expires_ms > now)
        .sort(|s| (Desc(s.issued_ms), s.id.clone())))
}

/// What a connection holds (§1.8): every durable table whole, and of the
/// sign-ins and the roles its person's.
#[scope("session.feed")]
pub fn feed() -> Scope<Harken> {
    fn all<R>(_: &Ctx, _: &R) -> bool {
        true
    }
    Scope::new()
        .on::<Media>(all)
        .on::<Album>(all)
        .on::<Person>(all)
        .on::<Work>(all)
        .on::<Movement>(all)
        .on::<Recording>(all)
        .on::<Credit>(all)
        .on::<Song>(all)
        .on::<Playlist>(all)
        .on::<PlaylistItem>(all)
        .on::<Play>(all)
        .on::<SignIn>(|ctx, s| s.user == ctx.user())
        .on::<Role>(|ctx, r| r.user == ctx.user())
}
