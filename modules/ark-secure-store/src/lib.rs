//! The secure store: secrets kept by the system, such as a login's token.
//! Its port — the [`SecureStore`] trait, [`get`], [`put`] and [`remove`],
//! a [`SecureStoreOp`] whose output is what is kept under the key after —
//! is the runtime's (`ark_module::secure_store`); here is what implements
//! it:
//!
//! - iPhone, iPad and Mac: `PhoneSecureStore` (swift/: the keychain, a
//!   generic password under the app's bundle id, this device only).
//! - Android: `PhoneSecureStore` (kotlin/: an AES-GCM key in the Android
//!   keystore, the ciphertext in app-private preferences kept out of
//!   backups).
//! - The Linux desktop (feature `desktop`): [`SecretService`], over D-Bus;
//!   Unavailable when no Secret Service runs.
//! - The browser: none, so Unavailable. A page's session is an `HttpOnly`
//!   cookie, which no script reads.
//!
//! README.md says what each does and what an app does when there is none.

pub use ark_module::secure_store::*;

#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!();

pub mod fake;
pub use fake::Fake;

/// The phones' secure store, through UniFFI (`ffi`): the foreign trait
/// Swift's and Kotlin's `PhoneSecureStore` implement, and it registered as
/// the port.
#[cfg(feature = "ffi")]
pub mod phone {
    use std::sync::Arc;

    use ark_module::{Answer, ModuleError};

    /// Why the phone's store gave nothing.
    #[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
    pub enum SecureStoreError {
        /// There is no store to keep it in.
        Unavailable,
        /// It failed, in the platform's words (never the secret's).
        Failed { why: String },
    }

    impl std::fmt::Display for SecureStoreError {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            match self {
                SecureStoreError::Unavailable => f.write_str("not available here"),
                SecureStoreError::Failed { why } => f.write_str(why),
            }
        }
    }

    impl std::error::Error for SecureStoreError {}

    impl From<uniffi::UnexpectedUniFFICallbackError> for SecureStoreError {
        fn from(e: uniffi::UnexpectedUniFFICallbackError) -> SecureStoreError {
            SecureStoreError::Failed { why: e.reason }
        }
    }

    impl From<SecureStoreError> for ModuleError {
        fn from(e: SecureStoreError) -> ModuleError {
            match e {
                SecureStoreError::Unavailable => ModuleError::Unavailable,
                SecureStoreError::Failed { why } => ModuleError::Failed { why },
            }
        }
    }

    /// Secrets kept by the phone, as Swift and Kotlin keep them.
    #[uniffi::export(with_foreign)]
    pub trait SecureStore: Send + Sync {
        fn get(&self, key: String) -> Result<Option<String>, SecureStoreError>;
        fn put(&self, key: String, value: String) -> Result<(), SecureStoreError>;
        fn remove(&self, key: String) -> Result<(), SecureStoreError>;
    }

    struct Phone(Arc<dyn SecureStore>);

    impl super::SecureStore for Phone {
        fn get(&self, key: String) -> Answer<Option<String>> {
            Ok(self.0.get(key)?)
        }
        fn put(&self, key: String, value: String) -> Answer<()> {
            Ok(self.0.put(key, value)?)
        }
        fn remove(&self, key: String) -> Answer<()> {
            Ok(self.0.remove(key)?)
        }
    }

    /// The phone's store as the port.
    pub fn port(s: Arc<dyn SecureStore>) -> Arc<dyn super::SecureStore> {
        Arc::new(Phone(s))
    }
}

#[cfg(all(feature = "desktop", target_os = "linux"))]
mod desktop;
#[cfg(all(feature = "desktop", target_os = "linux"))]
pub use desktop::SecretService;

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use ark_module::{Answer, ModuleError, Modules};

    fn asked(m: &Modules, op: SecureStoreOp) -> Option<Answer<Option<String>>> {
        let got = Arc::new(Mutex::new(None));
        let g = got.clone();
        m.run(&op, move |a| *g.lock().unwrap() = Some(a));
        let a = got.lock().unwrap().take();
        a
    }

    #[test]
    fn a_secret_is_kept_read_and_removed() {
        let mut m = Modules::new();
        Fake::new().register(&mut m);
        assert_eq!(asked(&m, get("token")), Some(Ok(None)));
        assert_eq!(asked(&m, put("token", "t")), Some(Ok(Some("t".into()))));
        assert_eq!(asked(&m, get("token")), Some(Ok(Some("t".into()))));
        assert_eq!(asked(&m, remove("token")), Some(Ok(None)));
        assert_eq!(asked(&m, get("token")), Some(Ok(None)));
    }

    #[test]
    fn with_no_store_every_op_is_unavailable() {
        let m = Modules::new();
        for op in [get("token"), put("token", "t"), remove("token")] {
            assert_eq!(asked(&m, op), Some(Err(ModuleError::Unavailable)));
        }
    }

    #[test]
    fn a_failing_store_says_so() {
        let mut m = Modules::new();
        let f = Fake::new();
        f.fail("the keychain is locked");
        f.register(&mut m);
        assert_eq!(
            asked(&m, put("token", "t")),
            Some(Err(ModuleError::Failed {
                why: "the keychain is locked".into()
            }))
        );
        assert_eq!(f.kept("token"), None);
    }

    #[cfg(all(feature = "desktop", target_os = "linux"))]
    #[test]
    fn with_no_session_bus_the_desktop_is_unavailable() {
        // A bus at an address nothing listens on: what a machine without a
        // Secret Service answers too.
        let s = SecretService::at("money", "unix:path=/nonexistent/ark-secure-store-test-bus");
        assert_eq!(SecureStore::get(&s, "login".into()), Err(ModuleError::Unavailable));
        assert_eq!(SecureStore::put(&s, "login".into(), "x".into()), Err(ModuleError::Unavailable));
        assert_eq!(SecureStore::remove(&s, "login".into()), Err(ModuleError::Unavailable));
    }

    /// Against a real Secret Service on the session bus: run under one with
    /// a login keyring, as `dbus-run-session -- sh -c 'printf pw |
    /// gnome-keyring-daemon --daemonize --login; gnome-keyring-daemon --start
    /// --components=secrets; cargo test -p ark-secure-store --features
    /// desktop -- --ignored'` (a throwaway `HOME` and `XDG_DATA_HOME`).
    #[cfg(all(feature = "desktop", target_os = "linux"))]
    #[test]
    #[ignore = "needs a Secret Service on the session bus"]
    fn the_desktop_keeps_reads_and_removes_in_the_secret_service() {
        let s = SecretService::new("ark-secure-store-test");
        assert_eq!(SecureStore::remove(&s, "k".into()), Ok(()));
        assert_eq!(SecureStore::get(&s, "k".into()), Ok(None));
        assert_eq!(SecureStore::put(&s, "k".into(), "one".into()), Ok(()));
        assert_eq!(SecureStore::put(&s, "k".into(), "two".into()), Ok(()));
        assert_eq!(SecureStore::get(&s, "k".into()), Ok(Some("two".into())));
        // Another app's is another item.
        assert_eq!(SecureStore::get(&SecretService::new("ark-secure-store-other"), "k".into()), Ok(None));
        assert_eq!(SecureStore::remove(&s, "k".into()), Ok(()));
        assert_eq!(SecureStore::get(&s, "k".into()), Ok(None));
    }
}
