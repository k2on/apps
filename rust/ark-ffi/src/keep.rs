//! A phone's login, kept in the secure store (`ark_auth::keep`): the
//! keychain on the iPhone, the iPad and the Mac, the Android keystore on
//! Android (modules/ark-secure-store). The shell makes one [`LoginKeep`]
//! over its `PhoneSecureStore` and its preferences, carries out
//! `Effect::Remember`'s login with [`LoginKeep::keep`], and reads it at a
//! launch with [`LoginKeep::recall`]. Its server, its demo and what else
//! is no secret stay in its preferences, as before.

use std::sync::Arc;

use ark_module::Modules;

use crate::Login;

/// The phone's preferences (`UserDefaults`, `SharedPreferences`), where a
/// login was kept before: read once, and the key deleted once the login
/// is moved.
#[uniffi::export(with_foreign)]
pub trait OldShelf: Send + Sync {
    fn get(&self, key: String) -> Option<String>;
    fn remove(&self, key: String);
}

struct Shelved {
    shelf: Arc<dyn OldShelf>,
    key: String,
}

impl ark_auth::keep::Before for Shelved {
    fn read(&self) -> Option<String> {
        self.shelf.get(self.key.clone())
    }
    fn delete(&self) {
        self.shelf.remove(self.key.clone())
    }
}

/// The phone's login, kept in the secure store under `login`.
#[derive(uniffi::Object)]
pub struct LoginKeep(ark_auth::keep::LoginKeep);

#[uniffi::export]
impl LoginKeep {
    /// Kept in `store` (none: held for this run alone); moved there once
    /// from `shelf`'s `old_key`, where it used to be kept.
    #[uniffi::constructor]
    pub fn new(store: Option<Arc<dyn ark_secure_store::phone::SecureStore>>, shelf: Option<Arc<dyn OldShelf>>, old_key: String) -> Arc<LoginKeep> {
        let mut m = Modules::new();
        m.set(store.map(ark_secure_store::phone::port));
        let before = shelf.map(|shelf| Arc::new(Shelved { shelf, key: old_key }) as Arc<dyn ark_auth::keep::Before>);
        Arc::new(LoginKeep(ark_auth::keep::LoginKeep::new("login", m, before)))
    }

    /// The login this phone keeps, if any; moved first from where it was
    /// kept before.
    pub fn recall(&self) -> Option<Login> {
        self.0.recall()
    }

    /// `login` kept for the next launch; none forgets whoever was.
    pub fn keep(&self, login: Option<Login>) {
        self.0.keep(login.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use ark_secure_store::phone::{SecureStore, SecureStoreError};

    #[derive(Default)]
    struct Phone(Mutex<Option<String>>);

    impl SecureStore for Phone {
        fn get(&self, _: String) -> Result<Option<String>, SecureStoreError> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn put(&self, _: String, value: String) -> Result<(), SecureStoreError> {
            *self.0.lock().unwrap() = Some(value);
            Ok(())
        }
        fn remove(&self, _: String) -> Result<(), SecureStoreError> {
            *self.0.lock().unwrap() = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct Prefs(Mutex<std::collections::BTreeMap<String, String>>);

    impl OldShelf for Prefs {
        fn get(&self, key: String) -> Option<String> {
            self.0.lock().unwrap().get(&key).cloned()
        }
        fn remove(&self, key: String) {
            self.0.lock().unwrap().remove(&key);
        }
    }

    #[test]
    fn a_phone_moves_its_old_login_into_the_keychain() {
        let l = Login {
            token: "placeholder".into(),
            user: "ana".into(),
            ..Login::default()
        };
        let phone = Arc::new(Phone::default());
        let prefs = Arc::new(Prefs::default());
        prefs
            .0
            .lock()
            .unwrap()
            .insert("money.login".into(), ark_auth::onboarding::login_json(l.clone()));
        prefs.0.lock().unwrap().insert("money.server".into(), "https://h".into());
        let keep = LoginKeep::new(Some(phone.clone()), Some(prefs.clone()), "money.login".into());
        assert_eq!(keep.recall(), Some(l));
        assert_eq!(
            prefs.0.lock().unwrap().keys().collect::<Vec<_>>(),
            vec!["money.server"],
            "only the secret moved"
        );
        assert!(phone.0.lock().unwrap().is_some());
        keep.keep(None);
        assert!(phone.0.lock().unwrap().is_none());
    }

    #[test]
    fn a_phone_store_that_is_unavailable_is_the_port_s_unavailable() {
        struct None_;
        impl SecureStore for None_ {
            fn get(&self, _: String) -> Result<Option<String>, SecureStoreError> {
                Err(SecureStoreError::Unavailable)
            }
            fn put(&self, _: String, _: String) -> Result<(), SecureStoreError> {
                Err(SecureStoreError::Unavailable)
            }
            fn remove(&self, _: String) -> Result<(), SecureStoreError> {
                Err(SecureStoreError::Unavailable)
            }
        }
        let keep = LoginKeep::new(Some(Arc::new(None_)), None, "money.login".into());
        keep.keep(Some(Login::default()));
        assert_eq!(keep.0.last(), Some(ark_auth::keep::Kept::ThisRun));
    }
}
