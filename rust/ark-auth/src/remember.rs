//! The logins a device has, one per server, kept between runs.
//!
//! Natively a JSON file, `$XDG_CONFIG_HOME/<app>/logins.json` (or
//! `~/.config/…`); in a browser `localStorage`, under `<app>.login.<server>`.
//! The token is a secret, and the file is the user's own.

use crate::Login;

/// Where one app's logins are remembered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Logins {
    app: String,
}

impl Logins {
    /// `app` names the directory, or the key prefix: `"harken"`.
    pub fn new(app: &str) -> Logins {
        Logins { app: app.into() }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Logins {
    fn file(&self) -> Option<std::path::PathBuf> {
        use std::path::PathBuf;
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join(&self.app).join("logins.json"))
    }

    fn all(&self) -> std::collections::BTreeMap<String, Login> {
        self.file()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn write(&self, all: &std::collections::BTreeMap<String, Login>) {
        let Some(file) = self.file() else { return };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(all) {
            let tmp = file.with_extension("tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, &file);
            }
        }
    }

    /// The login remembered for `server`.
    pub fn recall(&self, server: &str) -> Option<Login> {
        self.all().remove(server)
    }

    pub fn remember(&self, server: &str, login: &Login) {
        let mut all = self.all();
        all.insert(server.to_string(), login.clone());
        self.write(&all);
    }

    pub fn forget(&self, server: &str) {
        let mut all = self.all();
        if all.remove(server).is_some() {
            self.write(&all);
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl Logins {
    fn key(&self, server: &str) -> String {
        format!("{}.login.{server}", self.app)
    }

    /// The login remembered for `server`.
    pub fn recall(&self, server: &str) -> Option<Login> {
        let json = crate::web::storage()?.get_item(&self.key(server)).ok()??;
        serde_json::from_str(&json).ok()
    }

    pub fn remember(&self, server: &str, login: &Login) {
        if let (Some(storage), Ok(json)) = (crate::web::storage(), serde_json::to_string(login)) {
            let _ = storage.set_item(&self.key(server), &json);
        }
    }

    pub fn forget(&self, server: &str) {
        if let Some(storage) = crate::web::storage() {
            let _ = storage.remove_item(&self.key(server));
        }
    }
}
