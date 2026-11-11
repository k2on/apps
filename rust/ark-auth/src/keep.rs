//! Where a device keeps its login between launches: in the secure store
//! (`ark_module::secure_store`: the keychain, the Android keystore, the
//! desktop's Secret Service), never in the app's ordinary storage. What
//! is not a secret — the server, whether the demo shows, whether the
//! app's pages were seen — stays where each shell already keeps it.
//!
//! A shell makes one [`LoginKeep`] over the modules it registered, and
//! carries out [`crate::onboarding::Effect::Remember`]'s login with
//! [`LoginKeep::keep`] and reads it at a launch with
//! [`LoginKeep::recall`]. Every shell's rules are these, written once:
//!
//! - **The secret is the login's JSON** ([`crate::onboarding::login_json`]),
//!   kept under one key; one that no longer reads as a login is forgotten.
//! - **Moved once:** a login kept where it used to be ([`Before`]: the
//!   phones' preferences, the desktop's logins directory) is put in the
//!   secure store at the first recall that finds it there, and the old copy
//!   deleted once it is. A store that fails just then (a locked keyring)
//!   leaves it for the next launch.
//! - **No secure store** (a desktop with no Secret Service): the login is
//!   held for this run only, and the old copy is deleted all the same —
//!   the next launch signs in again, as a launch with nobody signed in does
//!   by itself (`crate::onboarding`). Never written to ordinary storage.
//!
//! Nothing here prints a secret.

use std::sync::{Arc, Mutex};

use ark_module::secure_store::{self, SecureStoreOp};
/// The registry a [`LoginKeep`] asks the secure store through.
pub use ark_module::Modules;
use ark_module::{Answer, ModuleError};

use crate::onboarding::{login_json, login_parse, Login};

/// Where a login was kept before the secure store: read once, and deleted
/// once it is moved.
pub trait Before: Send + Sync {
    /// The login kept there, as [`login_json`] wrote it.
    fn read(&self) -> Option<String>;
    /// It, gone.
    fn delete(&self);
}

/// What became of the last call, for a shell that says so: kept in the
/// secure store, held for this run alone (there is none), or failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kept {
    Secure,
    ThisRun,
    Failed { why: String },
}

/// A device's login, kept in the secure store under `key`.
pub struct LoginKeep {
    key: String,
    modules: Modules,
    before: Option<Arc<dyn Before>>,
    /// The login while there is no store to keep it in: this run's alone.
    held: Mutex<Option<Login>>,
    last: Mutex<Option<Kept>>,
}

impl LoginKeep {
    /// The login under `key` (`login`) in the secure store `modules` has, if
    /// any; moved there once from `before`, where it used to be kept.
    pub fn new(key: impl Into<String>, modules: Modules, before: Option<Arc<dyn Before>>) -> LoginKeep {
        LoginKeep {
            key: key.into(),
            modules,
            before,
            held: Mutex::new(None),
            last: Mutex::new(None),
        }
    }

    /// The login this device keeps, if any — moved first from where it was
    /// kept before.
    pub fn recall(&self) -> Option<Login> {
        let kept = match ask(&self.modules, secure_store::get(&self.key)) {
            Ok(Some(json)) => {
                self.said(Kept::Secure);
                // Moved by a launch that put it here and stopped short of
                // deleting the old copy.
                if let Some(b) = &self.before {
                    if b.read().is_some() {
                        b.delete();
                    }
                }
                return login_parse(json);
            }
            Ok(None) => Ok(()),
            Err(e) => Err(e),
        };
        let old = self.before.as_ref().and_then(|b| b.read().map(|j| (b, j)));
        match (kept, old) {
            (Ok(()), None) => {
                self.said(Kept::Secure);
                None
            }
            (Err(e), None) => {
                self.said(match e {
                    ModuleError::Unavailable => Kept::ThisRun,
                    ModuleError::Failed { why } => Kept::Failed { why },
                });
                self.held().clone()
            }
            (Ok(()), Some((b, json))) => match ask(&self.modules, secure_store::put(&self.key, json.clone())) {
                Ok(_) => {
                    b.delete();
                    self.said(Kept::Secure);
                    login_parse(json)
                }
                Err(e) => self.moving_failed(b, json, e),
            },
            (Err(e), Some((b, json))) => self.moving_failed(b, json, e),
        }
    }

    /// `login` kept for the next launch, or none: whoever was signed in
    /// forgotten. Never written anywhere but the secure store.
    pub fn keep(&self, login: Option<&Login>) {
        *self.held() = login.cloned();
        let op = match login {
            Some(l) => secure_store::put(&self.key, login_json(l.clone())),
            None => secure_store::remove(&self.key),
        };
        let kept = match ask(&self.modules, op) {
            Ok(_) => Kept::Secure,
            Err(ModuleError::Unavailable) => Kept::ThisRun,
            Err(ModuleError::Failed { why }) => Kept::Failed { why },
        };
        self.said(kept);
        // Whatever is kept now, no copy is left where it used to be.
        if let Some(b) = &self.before {
            if b.read().is_some() {
                b.delete();
            }
        }
    }

    /// What became of the last [`recall`](Self::recall) or
    /// [`keep`](Self::keep).
    pub fn last(&self) -> Option<Kept> {
        self.last.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// An old copy the store could not take: with no store at all it is
    /// deleted and held for this run; with a store that failed it stays
    /// where it was for the next launch to try again.
    fn moving_failed(&self, before: &Arc<dyn Before>, json: String, e: ModuleError) -> Option<Login> {
        let login = login_parse(json);
        match e {
            ModuleError::Unavailable => {
                before.delete();
                *self.held() = login.clone();
                self.said(Kept::ThisRun);
            }
            ModuleError::Failed { why } => self.said(Kept::Failed { why }),
        }
        login
    }

    fn held(&self) -> std::sync::MutexGuard<'_, Option<Login>> {
        self.held.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn said(&self, k: Kept) {
        *self.last.lock().unwrap_or_else(|e| e.into_inner()) = Some(k);
    }
}

/// Where an old copy of a server's login is, for [`ServerLogins`].
type BeforeOf = dyn Fn(&str) -> Option<Arc<dyn Before>> + Send + Sync;

/// A desktop's logins, one per server: each a [`LoginKeep`] under
/// `login <server>`, made at its first use and held for the run (so a
/// login with no secure store to keep it in lasts the run). A clone is the
/// same logins.
#[derive(Clone)]
pub struct ServerLogins {
    modules: Modules,
    before: Arc<BeforeOf>,
    keeps: Arc<Mutex<std::collections::BTreeMap<String, Arc<LoginKeep>>>>,
}

impl ServerLogins {
    /// Kept in the secure store `modules` has, each server's moved once
    /// from `before(server)`.
    pub fn new(modules: Modules, before: impl Fn(&str) -> Option<Arc<dyn Before>> + Send + Sync + 'static) -> ServerLogins {
        ServerLogins {
            modules,
            before: Arc::new(before),
            keeps: Arc::default(),
        }
    }

    fn of(&self, server: &str) -> Arc<LoginKeep> {
        let mut keeps = self.keeps.lock().unwrap_or_else(|e| e.into_inner());
        keeps
            .entry(server.to_string())
            .or_insert_with(|| Arc::new(LoginKeep::new(format!("login {server}"), self.modules.clone(), (self.before)(server))))
            .clone()
    }

    /// The login kept for `server`.
    pub fn recall(&self, server: &str) -> Option<crate::Login> {
        self.of(server).recall().map(Into::into)
    }

    /// `login` kept for `server`.
    pub fn remember(&self, server: &str, login: &crate::Login) {
        self.of(server).keep(Some(&login.clone().into()))
    }

    /// Nobody kept for `server`.
    pub fn forget(&self, server: &str) {
        self.of(server).keep(None)
    }

    /// What became of the last call for `server`.
    pub fn last(&self, server: &str) -> Option<Kept> {
        self.of(server).last()
    }
}

/// `op` carried out by `modules`, and what came of it: the secure store
/// answers at once; one that answers later is taken as a failure.
fn ask(modules: &Modules, op: SecureStoreOp) -> Answer<Option<String>> {
    let got = Arc::new(Mutex::new(None));
    let g = got.clone();
    modules.run(&op, move |a| *g.lock().unwrap_or_else(|e| e.into_inner()) = Some(a));
    let answer = got.lock().unwrap_or_else(|e| e.into_inner()).take();
    answer.unwrap_or_else(|| {
        Err(ModuleError::Failed {
            why: "the secure store didn't answer".into(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use ark_module::secure_store::SecureStore;

    /// A map as the secure store, which can fail: ark-secure-store's Fake
    /// is an adapter this crate doesn't depend on.
    #[derive(Default)]
    struct Store {
        kept: Mutex<std::collections::BTreeMap<String, String>>,
        failing: Mutex<bool>,
    }

    impl Store {
        fn failed(&self) -> Answer<()> {
            match *self.failing.lock().unwrap() {
                true => Err(ModuleError::Failed { why: "locked".into() }),
                false => Ok(()),
            }
        }
    }

    impl SecureStore for Store {
        fn get(&self, key: String) -> Answer<Option<String>> {
            self.failed()?;
            Ok(self.kept.lock().unwrap().get(&key).cloned())
        }
        fn put(&self, key: String, value: String) -> Answer<()> {
            self.failed()?;
            self.kept.lock().unwrap().insert(key, value);
            Ok(())
        }
        fn remove(&self, key: String) -> Answer<()> {
            self.failed()?;
            self.kept.lock().unwrap().remove(&key);
            Ok(())
        }
    }

    /// The preferences a login used to be kept in.
    #[derive(Default)]
    struct Prefs(Mutex<Option<String>>);

    impl Before for Prefs {
        fn read(&self) -> Option<String> {
            self.0.lock().unwrap().clone()
        }
        fn delete(&self) {
            *self.0.lock().unwrap() = None;
        }
    }

    fn login() -> Login {
        Login {
            token: "not-a-real-token".into(),
            session: "s1".into(),
            user: "ana".into(),
            name: "Ana".into(),
            expires_ms: 1,
            ..Login::default()
        }
    }

    fn with(store: &Arc<Store>) -> Modules {
        let mut m = Modules::new();
        m.register::<dyn SecureStore>(store.clone());
        m
    }

    #[test]
    fn a_sign_in_is_kept_in_the_secure_store_and_a_sign_out_removes_it() {
        let store = Arc::new(Store::default());
        let before = Arc::new(Prefs::default());
        let k = LoginKeep::new("login", with(&store), Some(before.clone()));
        k.keep(Some(&login()));
        assert_eq!(k.last(), Some(Kept::Secure));
        assert_eq!(store.kept.lock().unwrap().get("login").cloned(), Some(login_json(login())));
        assert_eq!(before.read(), None, "nothing written where it used to be");
        // The next launch.
        let again = LoginKeep::new("login", with(&store), Some(before.clone()));
        assert_eq!(again.recall(), Some(login()));
        again.keep(None);
        assert!(store.kept.lock().unwrap().is_empty());
        assert_eq!(LoginKeep::new("login", with(&store), Some(before)).recall(), None);
    }

    #[test]
    fn an_old_login_is_moved_once_and_its_old_copy_deleted() {
        let store = Arc::new(Store::default());
        let before = Arc::new(Prefs(Mutex::new(Some(login_json(login())))));
        let k = LoginKeep::new("login", with(&store), Some(before.clone()));
        assert_eq!(k.recall(), Some(login()));
        assert_eq!(before.read(), None);
        assert_eq!(store.kept.lock().unwrap().get("login").cloned(), Some(login_json(login())));
        // And from the store from then on.
        assert_eq!(LoginKeep::new("login", with(&store), Some(before)).recall(), Some(login()));
    }

    #[test]
    fn a_copy_left_behind_by_a_move_cut_short_is_deleted() {
        let store = Arc::new(Store::default());
        store.kept.lock().unwrap().insert("login".into(), login_json(login()));
        let before = Arc::new(Prefs(Mutex::new(Some(login_json(login())))));
        assert_eq!(LoginKeep::new("login", with(&store), Some(before.clone())).recall(), Some(login()));
        assert_eq!(before.read(), None);
    }

    #[test]
    fn a_store_that_fails_leaves_the_old_copy_for_the_next_launch() {
        let store = Arc::new(Store::default());
        *store.failing.lock().unwrap() = true;
        let before = Arc::new(Prefs(Mutex::new(Some(login_json(login())))));
        let k = LoginKeep::new("login", with(&store), Some(before.clone()));
        assert_eq!(k.recall(), Some(login()), "still signed in this launch");
        assert_eq!(k.last(), Some(Kept::Failed { why: "locked".into() }));
        assert!(before.read().is_some());
        *store.failing.lock().unwrap() = false;
        assert_eq!(LoginKeep::new("login", with(&store), Some(before.clone())).recall(), Some(login()));
        assert_eq!(before.read(), None);
    }

    #[test]
    fn with_no_secure_store_a_login_lasts_the_run_and_is_never_written_down() {
        let before = Arc::new(Prefs(Mutex::new(Some(login_json(login())))));
        let k = LoginKeep::new("login", Modules::new(), Some(before.clone()));
        // The old copy is read for this run, and deleted.
        assert_eq!(k.recall(), Some(login()));
        assert_eq!(k.last(), Some(Kept::ThisRun));
        assert_eq!(before.read(), None);
        // A sign-in is held for the run…
        let other = Login {
            session: "s2".into(),
            ..login()
        };
        k.keep(Some(&other));
        assert_eq!(k.last(), Some(Kept::ThisRun));
        assert_eq!(k.recall(), Some(other));
        assert_eq!(before.read(), None);
        // …and gone at the next launch, which signs in again.
        assert_eq!(LoginKeep::new("login", Modules::new(), Some(before)).recall(), None);
        k.keep(None);
        assert_eq!(k.recall(), None);
    }

    #[test]
    fn a_desktop_keeps_a_login_per_server_and_holds_each_for_the_run() {
        let store = Arc::new(Store::default());
        let old = Arc::new(Prefs(Mutex::new(Some(login_json(login())))));
        let o = old.clone();
        let logins = ServerLogins::new(with(&store), move |s| (s == "https://a").then(|| o.clone() as Arc<dyn Before>));
        assert_eq!(logins.recall("https://a"), Some(login().into()));
        assert_eq!(old.read(), None);
        assert_eq!(logins.recall("https://b"), None);
        assert!(store.kept.lock().unwrap().contains_key("login https://a"));
        // No store: a clone holds what the first was told.
        let alone = ServerLogins::new(Modules::new(), |_| None);
        alone.clone().remember("https://b", &login().into());
        assert_eq!(alone.recall("https://b"), Some(login().into()));
        alone.forget("https://b");
        assert_eq!(alone.recall("https://b"), None);
    }

    #[test]
    fn a_kept_login_that_no_longer_reads_is_none() {
        let store = Arc::new(Store::default());
        store.kept.lock().unwrap().insert("login".into(), "{not a login".into());
        assert_eq!(LoginKeep::new("login", with(&store), None).recall(), None);
    }
}
