//! What a sign-in hands a client, and what a client keeps.

use serde::{Deserialize, Serialize};

/// A signed-in person, as the server knows them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Account {
    /// The stable id — the provider's `sub`, or the name given to a server
    /// in dev mode. What every entry this person authors carries as its
    /// actor, and the room their live frames are in.
    pub id: String,
    /// A display name, as the provider had it. May be empty.
    #[serde(default)]
    pub name: String,
    /// May be empty.
    #[serde(default)]
    pub email: String,
    /// `docs/plan-auth.md` The roles this person holds, as the server says:
    /// what it was configured with for the account
    /// (`server::Auth::with_roles`) and, signed in through dev auth, what
    /// the name asked for (`alice:library`). What `has_role` asks, of the
    /// roles the server stamps on every entry this login pushes
    /// (`docs/plan-guards.md` D1).
    /// Absent when there are none, so a login written before roles reads
    /// as one holding none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<String>,
}

/// What the server hands back for a login code: everything a client needs
/// to open its replica as the right person and prove itself on the socket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Login {
    /// Proves the session. Sent in every `Hello`; never in a URL.
    pub token: String,
    /// The session's id, which entries authored under it carry.
    pub session: String,
    pub user: Account,
    /// When the token stops working, as milliseconds since the epoch.
    pub expires_ms: i64,
}
