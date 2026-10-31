//! The banks this server is connected to, and what the importer keeps
//! about them: one file in the data directory, `banks.json`, the server's
//! alone. It holds each bank's Plaid access token — the login to it — so
//! it is never in the log, never fed to a device, never in a line the
//! server says; it is written whole (beside, then moved in, mode 0600) and
//! read once at start. Restoring the data directory from a backup, or
//! moving it to another server, keeps every connection: nothing to link
//! again.
//!
//! ```json
//! {
//!   "version": 1,
//!   "banks": {
//!     "wellsfargo": {
//!       "access_token": "access-production-…",
//!       "cursor": "…",
//!       "item_id": "…",
//!       "institution_id": "ins_127991",
//!       "institution": "Wells Fargo",
//!       "added_ms": 1793000000000
//!     }
//!   },
//!   "answered_ms": 1793000000000
//! }
//! ```
//!
//! - `banks`: each connection by the name the budget knows the bank by
//!   (its `bank` row; the name a connection made in the app is given is
//!   the institution's, as `wells-fargo`). Only `access_token` is needed:
//!   - `cursor`: Plaid's `/transactions/sync` cursor after the last import
//!     the hub confirmed. Without one, the next import asks for the bank's
//!     whole history — which changes nothing already imported (Plaid's id
//!     is each transaction's `imported_id`), but brings back any a person
//!     deleted. A connection with no cursor here takes the one
//!     `importer.json` (what the importer kept before this file) has under
//!     its name, if any.
//!   - `item_id`, `institution_id`, `institution`: Plaid's names for the
//!     connection, which the importer fills in at its next run when they
//!     are missing.
//!   - `added_ms`: when it was connected.
//! - `answered_ms`: the newest `banks.sync` the importer answered, so a
//!   restart does not answer it again.
//!
//! Anything else in the file is kept as it is.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The file's version, which a later format would move.
pub const VERSION: u32 = 1;

/// Where the connections are kept.
pub fn file(data: &Path) -> PathBuf {
    data.join("banks.json")
}

/// Where the importer kept its cursors before `banks.json`.
pub fn legacy_file(data: &Path) -> PathBuf {
    data.join("importer.json")
}

/// One bank this server holds a login for.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub item_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub institution_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub institution: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub added_ms: i64,
    #[serde(flatten)]
    pub rest: BTreeMap<String, Value>,
}

fn is_zero(n: &i64) -> bool {
    *n == 0
}

/// Never the token.
impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection")
            .field("cursor", &self.cursor)
            .field("item_id", &self.item_id)
            .field("institution", &self.institution)
            .finish_non_exhaustive()
    }
}

/// Every connection, and what the importer last answered.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub banks: BTreeMap<String, Connection>,
    #[serde(default)]
    pub answered_ms: i64,
    #[serde(flatten)]
    pub rest: BTreeMap<String, Value>,
}

/// `importer.json`, as the importer kept it before.
#[derive(Default, Deserialize)]
struct Legacy {
    #[serde(default)]
    cursors: BTreeMap<String, String>,
    #[serde(default)]
    answered_ms: i64,
}

fn read_json<T: serde::de::DeserializeOwned + Default>(path: &Path) -> Result<Option<T>> {
    match std::fs::read_to_string(path) {
        // What a parse error says is where, never what: the file holds tokens.
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| anyhow::anyhow!("{} does not read: line {} column {}", path.display(), e.line(), e.column())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

impl Store {
    /// The connections kept in `data`: `banks.json`, with what
    /// `importer.json` kept filling in a cursor or the last answer it
    /// lacks. Neither file is no connections.
    pub fn load(data: &Path) -> Result<Store> {
        let path = file(data);
        let mut store: Store = read_json(&path)?.unwrap_or_default();
        let legacy: Legacy = read_json(&legacy_file(data))?.unwrap_or_default();
        for (name, c) in &mut store.banks {
            if c.cursor.as_deref().is_none_or(str::is_empty) {
                c.cursor = legacy.cursors.get(name).filter(|c| !c.is_empty()).cloned();
            }
        }
        store.banks.retain(|_, c| !c.access_token.trim().is_empty());
        store.answered_ms = store.answered_ms.max(legacy.answered_ms);
        tighten(&path);
        Ok(store)
    }

    /// Written whole beside and moved in, each step on the disk before the
    /// next, readable by the server's user alone: a crash leaves the old
    /// file or the new, never half of one.
    pub fn save(&self, data: &Path) -> Result<()> {
        use std::io::Write;
        let path = file(data);
        let tmp = path.with_extension("json.tmp");
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut f = options.open(&tmp).with_context(|| format!("writing {}", tmp.display()))?;
        let mut whole = self.clone();
        whole.version = VERSION;
        f.write_all(serde_json::to_string_pretty(&whole)?.as_bytes())?;
        f.write_all(b"\n")?;
        f.sync_all()?;
        drop(f);
        tighten(&tmp);
        std::fs::rename(&tmp, &path).with_context(|| format!("writing {}", path.display()))?;
        if let Some(dir) = path.parent() {
            std::fs::File::open(dir)?.sync_all()?;
        }
        Ok(())
    }

    /// A name for a new connection to `institution`: its name in lower case,
    /// words joined by `-`, and a number after when it is taken.
    pub fn name_for(&self, institution: &str, taken: impl Fn(&str) -> bool) -> String {
        let mut base = String::new();
        for c in institution.chars() {
            match c {
                c if c.is_ascii_alphanumeric() => base.push(c.to_ascii_lowercase()),
                _ if !base.is_empty() && !base.ends_with('-') => base.push('-'),
                _ => {}
            }
        }
        let base = match base.trim_end_matches('-') {
            "" => "bank".to_string(),
            b => b.to_string(),
        };
        let free = |n: &str| !self.banks.contains_key(n) && !taken(n);
        if free(&base) {
            return base;
        }
        (2..).map(|i| format!("{base}-{i}")).find(|n| free(n)).expect("a free name")
    }
}

/// The file readable by its owner alone, whoever wrote it.
fn tighten(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(m) = std::fs::metadata(path) {
            if m.permissions().mode() & 0o077 != 0 {
                let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
            }
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file written by hand with a token alone reads, takes the cursor
    /// `importer.json` kept under its name, and is written back 0600 with
    /// whatever else it said kept. Falsified by reading `importer.json`
    /// only when `banks.json` is missing: the cursor was lost.
    #[test]
    fn a_file_written_by_hand_reads() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            file(dir.path()),
            r#"{"banks": {"wellsfargo": {"access_token": "access-x", "note": "moved by hand"}}}"#,
        )
        .unwrap();
        std::fs::write(
            legacy_file(dir.path()),
            r#"{"cursors": {"wellsfargo": "c-old", "gone": "c-gone"}, "answered_ms": 7}"#,
        )
        .unwrap();
        let s = Store::load(dir.path()).unwrap();
        let wf = &s.banks["wellsfargo"];
        assert_eq!((wf.access_token.as_str(), wf.cursor.as_deref(), s.answered_ms), ("access-x", Some("c-old"), 7));
        assert!(!format!("{s:?}").contains("access-x"), "a store's Debug never says a token");
        s.save(dir.path()).unwrap();
        let text = std::fs::read_to_string(file(dir.path())).unwrap();
        assert!(text.contains("\"note\": \"moved by hand\"") && text.contains("\"version\": 1"), "{text}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(file(dir.path())).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let back = Store::load(dir.path()).unwrap();
        assert_eq!((back.banks, back.answered_ms, back.version), (s.banks, 7, VERSION));
    }

    #[test]
    fn a_new_connection_is_named_for_its_bank() {
        let mut s = Store::default();
        assert_eq!(s.name_for("Wells Fargo", |_| false), "wells-fargo");
        s.banks.insert("wells-fargo".into(), Connection::default());
        assert_eq!(s.name_for("Wells Fargo", |n| n == "wells-fargo-2"), "wells-fargo-3");
        assert_eq!(s.name_for("  ", |_| false), "bank");
        assert_eq!(s.name_for("Capital One (360)", |_| false), "capital-one-360");
    }

    #[test]
    fn a_broken_file_says_where_not_what() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(file(dir.path()), r#"{"banks": {"x": {"access_token": "access-secret""#).unwrap();
        let e = Store::load(dir.path()).unwrap_err().to_string();
        assert!(e.contains("does not read") && !e.contains("access-secret"), "{e}");
    }
}
