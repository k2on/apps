//! What the server keeps of the stores that is nobody's to sync: Amazon's
//! login and Costco's, given in the app's Settings. One file in the data directory (`stores.json`), written
//! readable by the server alone; never a row, so never in the log, on a
//! device, or in a backup of the budget.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// The Amazon account `amazon-orders` signs in as.
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AmazonLogin {
    pub username: String,
    pub password: String,
    /// The account's one-time-code secret (two-step verification by an
    /// authenticator app): `amazon-orders` makes each code from it.
    #[serde(default)]
    pub otp_secret: Option<String>,
}

impl std::fmt::Debug for AmazonLogin {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "AmazonLogin({}, otp: {})", masked(&self.username), self.otp_secret.is_some())
    }
}

/// Costco's sign-in, as its app keeps one: who, and the refresh token,
/// which Costco replaces at every use ([`crate::costco`]).
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CostcoLogin {
    /// The account's email, for the page; blank when the token named none.
    #[serde(default)]
    pub who: String,
    pub refresh_token: String,
}

impl std::fmt::Debug for CostcoLogin {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "CostcoLogin({})", masked(&self.who))
    }
}

/// An email as the page shows it: `m••••@example.com`.
pub fn masked(email: &str) -> String {
    match email.split_once('@') {
        Some((who, host)) => format!("{}••••@{host}", who.chars().next().unwrap_or('•')),
        None => format!("{}••••", email.chars().next().unwrap_or('•')),
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vault {
    #[serde(default)]
    pub amazon: Option<AmazonLogin>,
    #[serde(default)]
    pub costco: Option<CostcoLogin>,
}

pub fn vault_file(data: &Path) -> PathBuf {
    data.join("stores.json")
}

/// One writer of the vault at a time: Settings' calls, and the importer
/// keeping Costco's new refresh token.
static WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());

impl Vault {
    /// Loaded, changed by `f`, and saved, with no other change between.
    pub fn change(data: &Path, f: impl FnOnce(&mut Vault) -> Result<()>) -> Result<()> {
        let _held = WRITING.lock().unwrap_or_else(|e| e.into_inner());
        let mut v = Vault::load(data)?;
        f(&mut v)?;
        v.save(data)
    }

    pub fn load(data: &Path) -> Result<Vault> {
        let path = vault_file(data);
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).with_context(|| format!("reading {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vault::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    /// Written whole beside and moved in, readable by this user alone.
    pub fn save(&self, data: &Path) -> Result<()> {
        use std::io::Write;
        let path = vault_file(data);
        let tmp = path.with_extension("json.tmp");
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        let mut f = o.open(&tmp).with_context(|| format!("writing {}", tmp.display()))?;
        f.write_all(serde_json::to_string_pretty(self)?.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, &path).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vault_is_the_servers_alone() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Vault::load(dir.path()).unwrap(), Vault::default());
        let v = Vault {
            amazon: Some(AmazonLogin {
                username: "me@example.com".into(),
                password: "hunter2".into(),
                otp_secret: None,
            }),
            costco: Some(CostcoLogin {
                who: "me@example.com".into(),
                refresh_token: "the-refresh-token".into(),
            }),
        };
        v.save(dir.path()).unwrap();
        assert_eq!(Vault::load(dir.path()).unwrap(), v);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(vault_file(dir.path())).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert!(!format!("{:?}", v).contains("hunter2"));
        assert!(!format!("{:?}", v).contains("the-refresh-token"));
        assert_eq!(masked("me@example.com"), "m••••@example.com");
    }
}
