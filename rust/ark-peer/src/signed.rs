//! Who is signed in, as the domain's rows say (`docs/plan-v5.md` S1).
//!
//! A server writes each sign-in as a row of the domain's [`SESSIONS`]
//! table and its token names that row (`ark-auth`). The peer reads it by
//! the domain's [`CURRENT`] query: a device to say who it is
//! ([`Peer::current`]), a server to judge a token ([`Peer::judge`]) — the
//! session live, its roles those it was opened with and those the
//! [`ROLES`] table grants. Signing out is the domain's [`REVOKE`]
//! ([`Peer::sign_out`]); a server closes every connection of a session
//! revoked or expired, and of a person whose roles changed, so that each
//! is judged again.

use ark::log::EntryId;
use ark::peer::{Ctx, Who};
use ark::store::{Change, Store};
use ark::Domain;

use crate::Peer;

/// The table a sign-in is a row of.
pub const SESSIONS: &str = "sign_in";
/// The table a granted role is a row of.
pub const ROLES: &str = "role";
/// The query answering the caller's live session, roles merged.
pub const CURRENT: &str = "session.current";
/// The mutation ending one of the caller's sessions, by `id`.
pub const REVOKE: &str = "session.revoke";

/// A session, as [`CURRENT`] answers it.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(default)]
pub struct Signed {
    pub id: String,
    pub user: String,
    pub name: String,
    pub email: String,
    pub device: String,
    pub issued_ms: i64,
    pub expires_ms: i64,
    pub roles: Vec<String>,
}

/// Who `ctx`'s session is over `store`, as `domain` answers it.
pub fn signed(domain: &Domain, store: &Store, ctx: &Ctx) -> Option<Signed> {
    let b = domain.ask(store, ctx, CURRENT, &[]).ok()?;
    serde_json::from_str(&domain.answer_json(CURRENT, &b).ok()?).ok().flatten()
}

impl Peer {
    /// Who this device is signed in as, as its replica holds its session:
    /// nobody signed out, or before the session has synced.
    pub fn current(&self) -> Option<Signed> {
        signed(self.config.log.domain.as_deref()?, &self.log.store, &self.ctx())
    }

    /// Signed out: this session revoked by the domain's mutation, pushed
    /// while linked (and otherwise pending, for the next sign-in), and
    /// nothing dialled after it.
    pub fn sign_out(&mut self) -> Result<EntryId, String> {
        let id = serde_json::json!({ "id": self.me().session }).to_string();
        let r = self.input_json(REVOKE, &id).and_then(|input| self.mutate_bytes(REVOKE, input));
        self.set_token(None);
        r
    }

    /// The login session `session` proves now, judged by its row: live,
    /// its person and roles; its expiry kept, to close it then.
    pub fn judge(&mut self, session: &str, now: i64) -> Option<Who> {
        let ctx = Ctx {
            draws: Default::default(),
            id: EntryId::of(session, 0),
            actor: String::new(),
            session: session.into(),
            roles: vec![],
            now,
        };
        let s = signed(self.config.log.domain.as_deref()?, &self.log.store, &ctx)?;
        self.hub.expires.insert(session.into(), s.expires_ms);
        Some(Who {
            actor: s.user,
            session: session.into(),
            roles: s.roles,
        })
    }

    /// A device's roles joined by its session's when its sign-ins changed;
    /// a server's sessions ended by what changed: one revoked or gone, or
    /// a role of its person granted or taken back.
    pub(crate) fn signs(&mut self, cs: &[Change]) {
        if !cs.iter().any(|c| [SESSIONS, ROLES].contains(&c.table())) {
            return;
        }
        if !self.hub.serving {
            // Joined to what the login said (the configuration's roles,
            // which no row holds): a preview refuses nothing the server
            // would take.
            let mut roles = self.me().roles.clone();
            roles.extend(self.current().map(|s| s.roles).unwrap_or_default());
            roles.sort();
            roles.dedup();
            if roles != self.me().roles {
                self.set_roles(roles);
            }
            return;
        }
        let Some(d) = self.config.log.domain.clone() else { return };
        for c in cs {
            let (row, gone) = match c {
                Change::Add { row, .. } => (row, false),
                Change::Remove { row, .. } => (row, true),
                Change::Edit { new, .. } => (new, false),
            };
            let Some(v) = d
                .row_json(c.table(), row)
                .ok()
                .and_then(|j| serde_json::from_str::<serde_json::Value>(&j).ok())
            else {
                continue;
            };
            let text = |k: &str| v[k].as_str().unwrap_or_default().to_string();
            match c.table() {
                SESSIONS if gone || v["revoked"] == true => drop(self.hub.ended.insert(text("id"))),
                ROLES => {
                    let user = text("user");
                    let of: Vec<String> = self
                        .connections()
                        .filter(|(_, w)| w.actor == user)
                        .map(|(_, w)| w.session.clone())
                        .collect();
                    self.hub.ended.extend(of);
                }
                _ => {}
            }
        }
    }

    /// The sessions with a connection to close now — revoked, its person's
    /// roles changed, or expired by `now` — taken.
    pub fn ended(&mut self, now: i64) -> Vec<String> {
        let expired: Vec<String> = self.hub.expires.iter().filter(|(_, e)| **e <= now).map(|(s, _)| s.clone()).collect();
        for s in expired {
            self.hub.expires.remove(&s);
            self.hub.ended.insert(s);
        }
        let ended = std::mem::take(&mut self.hub.ended);
        ended.into_iter().filter(|s| !self.of_session(s).is_empty()).collect()
    }
}
