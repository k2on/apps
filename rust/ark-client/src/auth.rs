//! Sign-in, through `ark-auth`'s client half: a login is a token the link
//! sends first on every connection, and the [`Who`] a client authors as.

use ark::peer::Who;
pub use ark_auth::remember::Logins;
pub use ark_auth::{Account, Login};

/// Who a login is, as the hub verifies it: the account, the session, the
/// roles it holds.
pub fn who(l: &Login) -> Who {
    Who {
        actor: l.user.id.clone(),
        session: l.session.clone(),
        roles: l.user.roles.clone(),
    }
}

/// Signs in at `server` (in the system browser; `user` names the account a
/// dev server signs in) and says who that is.
#[cfg(not(target_arch = "wasm32"))]
pub fn sign_in(server: &str, user: Option<&str>) -> Result<(Login, Who), String> {
    let l = ark_auth::client::login(server, user, ark_auth::client::open_browser)?;
    let w = who(&l);
    Ok((l, w))
}
