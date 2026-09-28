//! The sessions a server has issued, in a file of its own.
//!
//! A session is one login on one device. The client keeps its token; this
//! keeps a hash of it, who it belongs to, and when it stops working — and
//! never forgets one, because an entry authored under a session long
//! expired is still that person's and the engine may ask.
//!
//! The file is JSON, written whole to a temporary name and renamed after
//! every issue and revoke. Sessions are a few hundred bytes each and are
//! issued once per sign-in, so a rewrite is nothing; a store with no path
//! keeps them in memory, for tests and a dev server that should forget.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{Account, Login};

/// Thirty days. A phone that has not spoken to the server in a month signs
/// in again, and loses nothing by it.
pub const DEFAULT_TTL_MS: i64 = 30 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Row {
    id: String,
    token_hash: String,
    user: String,
    name: String,
    email: String,
    issued_ms: i64,
    expires_ms: i64,
    revoked: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    sessions: Vec<Row>,
}

/// The store. One per server, behind whatever lock the server keeps it.
pub struct SessionStore {
    path: Option<PathBuf>,
    rows: Vec<Row>,
    ttl_ms: i64,
    now: Box<dyn FnMut() -> i64 + Send>,
}

impl std::fmt::Debug for SessionStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionStore")
            .field("path", &self.path)
            .field("sessions", &self.rows.len())
            .field("ttl_ms", &self.ttl_ms)
            .finish_non_exhaustive()
    }
}

impl SessionStore {
    /// Kept in `path`, which is read if it is there.
    pub fn open(path: impl AsRef<Path>) -> Result<SessionStore, String> {
        let path = path.as_ref().to_path_buf();
        let file: File = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => File::default(),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        Ok(SessionStore {
            path: Some(path),
            rows: file.sessions,
            ttl_ms: DEFAULT_TTL_MS,
            now: Box::new(now_ms),
        })
    }

    /// Kept nowhere.
    pub fn memory() -> SessionStore {
        SessionStore {
            path: None,
            rows: vec![],
            ttl_ms: DEFAULT_TTL_MS,
            now: Box::new(now_ms),
        }
    }

    /// How long a session lasts from issue.
    pub fn with_ttl(mut self, ttl_ms: i64) -> Self {
        self.ttl_ms = ttl_ms;
        self
    }

    /// A clock of the test's choosing.
    pub fn with_clock(mut self, now: impl FnMut() -> i64 + Send + 'static) -> Self {
        self.now = Box::new(now);
        self
    }

    fn write(&self) -> Result<(), String> {
        let Some(path) = &self.path else { return Ok(()) };
        let bytes = serde_json::to_vec_pretty(&File {
            sessions: self.rows.clone(),
        })
        .map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Sign `account` in: a new session, and the token that proves it.
    pub fn issue(&mut self, account: &Account) -> Result<Login, String> {
        let token = random_token();
        let issued_ms = (self.now)();
        let row = Row {
            id: random_id(),
            token_hash: hash(&token),
            user: account.id.clone(),
            name: account.name.clone(),
            email: account.email.clone(),
            issued_ms,
            expires_ms: issued_ms + self.ttl_ms,
            revoked: false,
        };
        let login = Login {
            token,
            session: row.id.clone(),
            user: account.clone(),
            expires_ms: row.expires_ms,
        };
        self.rows.push(row);
        if let Err(e) = self.write() {
            self.rows.pop();
            return Err(e);
        }
        Ok(login)
    }

    /// The login a token proves, if it is live.
    pub fn lookup(&mut self, token: &str) -> Option<Login> {
        let now = (self.now)();
        let h = hash(token);
        let r = self.rows.iter().find(|r| r.token_hash == h)?;
        (!r.revoked && r.expires_ms > now).then(|| Login {
            token: token.to_string(),
            session: r.id.clone(),
            user: Account {
                id: r.user.clone(),
                name: r.name.clone(),
                email: r.email.clone(),
            },
            expires_ms: r.expires_ms,
        })
    }

    /// End a session. Its token proves nothing from here; entries authored
    /// under it are still its owner's. Whether there was a live one.
    pub fn revoke(&mut self, token: &str) -> Result<bool, String> {
        let h = hash(token);
        let Some(r) = self.rows.iter_mut().find(|r| r.token_hash == h && !r.revoked) else {
            return Ok(false);
        };
        r.revoked = true;
        self.write()?;
        Ok(true)
    }

    /// Whether `session` is or was `user`'s — live, expired or revoked.
    pub fn owned_by(&self, user: &str, session: &str) -> bool {
        self.rows.iter().any(|r| r.id == session && r.user == user)
    }
}

/// 128 bits from the OS, as hex: a session id, which entries carry.
pub fn random_id() -> String {
    let mut b = [0u8; 16];
    getrandom::getrandom(&mut b).expect("the OS gives no randomness");
    hex(&b)
}

/// 256 bits from the OS, as hex.
pub fn random_token() -> String {
    let mut b = [0u8; 32];
    getrandom::getrandom(&mut b).expect("the OS gives no randomness");
    hex(&b)
}

pub(crate) fn hash(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::sync::Arc;

    fn alice() -> Account {
        Account {
            id: "sub-alice".into(),
            name: "Alice".into(),
            email: "alice@example".into(),
        }
    }

    #[test]
    fn a_token_proves_its_session_until_it_does_not() {
        let clock = Arc::new(AtomicI64::new(1_000));
        let tick = clock.clone();
        let mut store = SessionStore::memory().with_ttl(100).with_clock(move || tick.load(Ordering::SeqCst));
        let login = store.issue(&alice()).unwrap();
        assert_eq!(login.expires_ms, 1_100);
        assert_eq!(store.lookup(&login.token).unwrap().session, login.session);
        assert!(store.lookup("not a token").is_none());
        clock.store(1_100, Ordering::SeqCst);
        assert!(store.lookup(&login.token).is_none(), "expired");
        assert!(store.owned_by("sub-alice", &login.session), "expired is not forgotten");
        assert!(!store.owned_by("sub-bob", &login.session));
    }

    #[test]
    fn revoking_ends_the_token_and_keeps_the_ownership() {
        let mut store = SessionStore::memory();
        let login = store.issue(&alice()).unwrap();
        assert!(store.revoke(&login.token).unwrap());
        assert!(!store.revoke(&login.token).unwrap(), "already gone");
        assert!(store.lookup(&login.token).is_none());
        assert!(store.owned_by("sub-alice", &login.session));
    }

    #[test]
    fn the_file_survives_a_restart_and_holds_no_token() {
        let dir = std::env::temp_dir().join(format!("ark-auth-sessions-{}", std::process::id()));
        let path = dir.join("sessions.json");
        let _ = std::fs::remove_dir_all(&dir);
        let (kept, gone) = {
            let mut store = SessionStore::open(&path).unwrap();
            let kept = store.issue(&alice()).unwrap();
            let gone = store.issue(&alice()).unwrap();
            store.revoke(&gone.token).unwrap();
            (kept, gone)
        };
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains(&kept.token), "the token itself is never written");
        assert!(text.contains(&hash(&kept.token)));
        let mut again = SessionStore::open(&path).unwrap();
        assert_eq!(again.lookup(&kept.token).map(|l| l.session), Some(kept.session));
        assert!(again.lookup(&gone.token).is_none(), "a revocation survives the restart");
        assert!(again.owned_by("sub-alice", &gone.session));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
