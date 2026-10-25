//! The logins a device has, one per server, kept between runs: a row each
//! of the `login` table of a local schema of this crate's own
//! ([`Remembered`]), which every app's peer opens as a local instance
//! (`ark_peer::Logins`: natively under `$XDG_CONFIG_HOME/<app>/logins/`, or
//! `~/.config/…`; in a browser `localStorage`, under `<app>.logins`). The
//! token is the user's own secret, on the user's own disk; a server's
//! secrets stay in its configuration.

use ark::db::Db;
use ark::prelude::{Deserialize, Row, Schema, Serialize};
use ark::Refusal;

use crate::{Account, Login};

/// One server's login.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Row)]
#[ark(table = "login")]
pub struct Remember {
    #[ark(key)]
    pub server: String,
    pub user: String,
    pub name: String,
    pub email: String,
    pub roles: Vec<String>,
    pub session: String,
    pub token: String,
    pub expires_ms: i64,
    /// Defaulted, as `SignIn::picture` is: a login remembered before it
    /// still opens.
    #[serde(default)]
    pub picture: String,
}

/// The logins a device remembers.
#[derive(Schema)]
pub struct Remembered {
    pub login: Remember,
}

/// The login remembered for `server`.
pub fn recall(db: &Db<Remembered>, server: &str) -> Option<Login> {
    let r = db.login.get(server)?;
    Some(Login {
        token: r.token,
        session: r.session,
        user: Account {
            id: r.user,
            name: r.name,
            email: r.email,
            picture: r.picture,
            roles: r.roles,
        },
        expires_ms: r.expires_ms,
    })
}

pub fn remember(db: &mut Db<Remembered>, server: &str, l: &Login) -> Result<(), Refusal> {
    db.login.put(Remember {
        server: server.into(),
        user: l.user.id.clone(),
        name: l.user.name.clone(),
        email: l.user.email.clone(),
        roles: l.user.roles.clone(),
        session: l.session.clone(),
        token: l.token.clone(),
        expires_ms: l.expires_ms,
        picture: l.user.picture.clone(),
    })
}

pub fn forget(db: &mut Db<Remembered>, server: &str) -> Result<(), Refusal> {
    db.login.delete(server)
}

/// Where `app`'s logins are kept natively: `$XDG_CONFIG_HOME/<app>/logins`
/// (or `~/.config/…`).
#[cfg(not(target_arch = "wasm32"))]
pub fn dir(app: &str) -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join(app).join("logins"))
}
