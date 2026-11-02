//! What the server keeps of the stores that is nobody's to sync: Amazon's
//! login, given on the stores page, and the keys a store's bookmarklet
//! uploads with. One file in the data directory (`stores.json`), written
//! readable by the server alone; never a row, so never in the log, on a
//! device, or in a backup of the budget.

use std::collections::BTreeMap;
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
    /// Each store's upload key, by store.
    #[serde(default)]
    pub keys: BTreeMap<String, String>,
}

pub fn vault_file(data: &Path) -> PathBuf {
    data.join("stores.json")
}

impl Vault {
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

    /// The store whose upload key this is.
    pub fn store_of_key(&self, key: &str) -> Option<&str> {
        self.keys
            .iter()
            .find(|(_, k)| !key.is_empty() && constant_eq(k, key))
            .map(|(s, _)| s.as_str())
    }
}

fn constant_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A new key: 32 random bytes, hex.
pub fn new_key() -> Result<String> {
    use std::io::Read;
    let mut b = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .context("reading /dev/urandom")?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vault_is_the_servers_alone() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Vault::load(dir.path()).unwrap(), Vault::default());
        let mut v = Vault {
            amazon: Some(AmazonLogin {
                username: "me@example.com".into(),
                password: "hunter2".into(),
                otp_secret: None,
            }),
            ..Default::default()
        };
        let key = new_key().unwrap();
        assert_eq!(key.len(), 64);
        v.keys.insert("costco".into(), key.clone());
        v.save(dir.path()).unwrap();
        assert_eq!(Vault::load(dir.path()).unwrap(), v);
        assert_eq!(v.store_of_key(&key), Some("costco"));
        assert_eq!(v.store_of_key("nope"), None);
        assert_eq!(v.store_of_key(""), None);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(vault_file(dir.path())).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert!(!format!("{:?}", v).contains("hunter2"));
        assert_eq!(masked("me@example.com"), "m••••@example.com");
    }
}
