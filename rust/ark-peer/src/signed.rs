//! Who is signed in, on a device (`docs/plan-v5.md` S1): `ark-auth`'s
//! rows, read and written through the domain's functions by their names.
//!
//! A device says who it is by [`ark_auth::rows::current`]
//! ([`Peer::current`]) and signs out by [`ark_auth::rows::revoke`]
//! ([`Peer::sign_out`]); when its sign-ins or roles change, the roles it
//! previews its intents with are joined by its session's. The server's
//! side — the token judged by its row, a session revoked or expired closed
//! — is `ark_auth::server::Auth`, which the hub runs (`crate::serve`).

use ark::format::Encode;
use ark::log::EntryId;
use ark::row::Row;
use ark::store::Change;
use ark_auth::rows::{CURRENT, REVOKE};
use ark_auth::{Revoke, Role, SignIn};

use crate::Peer;

impl Peer {
    /// Who this device is signed in as, as its replica holds its session:
    /// nobody signed out, or before the session has synced.
    pub fn current(&self) -> Option<SignIn> {
        let d = self.config.log.domain.as_deref()?;
        let b = d.ask(&self.log.store, &self.ctx(), CURRENT, &[]).ok()?;
        Option::<SignIn>::from_bytes(&b).ok().flatten()
    }

    /// Signed out: this session revoked through the domain, pushed while
    /// linked (and otherwise pending, for the next sign-in), and nothing
    /// dialled after it.
    pub fn sign_out(&mut self) -> Result<EntryId, String> {
        let input = Revoke { id: self.me().session.clone() }.to_bytes();
        let r = self.mutate_bytes(REVOKE, input);
        self.set_token(None);
        r
    }

    /// A device's roles joined by its session's when its sign-ins or roles
    /// changed — with what the login said (the configuration's roles,
    /// which no row holds) — so a preview refuses nothing the server would
    /// take.
    pub(crate) fn signs(&mut self, cs: &[Change]) {
        if self.hub.serving || !cs.iter().any(|c| [SignIn::TABLE, Role::TABLE].contains(&c.table())) {
            return;
        }
        let mut roles = self.me().roles.clone();
        roles.extend(self.current().map(|s| s.roles).unwrap_or_default());
        roles.sort();
        roles.dedup();
        if roles != self.me().roles {
            self.set_roles(roles);
        }
    }
}
