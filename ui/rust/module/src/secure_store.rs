//! The secure store's port: secrets kept by the system, such as a login's
//! token. An update, or a shell's shared code (`ark_auth::keep`), asks
//! with [`get`], [`put`] or [`remove`], a [`SecureStoreOp`] whose output is
//! what is kept under the key once it is done: a read's secret (none when
//! nothing is), a put's value, a removal's none. Its implementations (the
//! keychain, the Android keystore, the desktop's Secret Service) and its
//! fake are the module's (modules/ark-secure-store, `ark-secure-store`).
//!
//! Each operation is in a [`Scope`]: the app's own ([`Scope::Private`],
//! what [`get`], [`put`] and [`remove`] ask), or every Ark app's on the
//! device ([`Scope::Shared`], for a device-wide sign-in), which no platform
//! has yet: it is answered Unavailable here, before any store is asked.
//! When one does, the shared store is registered beside the private one.
//!
//! A platform with no secure store registers none, and every operation is
//! answered [`ModuleError::Unavailable`](crate::ModuleError::Unavailable);
//! one whose store is not there at the moment (no Secret Service running)
//! answers the same from its calls.

use crate::{Answer, ModuleError, Op, Reply};

/// Secrets kept by the system, each under a key of the app's own.
pub trait SecureStore: Send + Sync {
    /// The secret kept under `key`; none when nothing is.
    fn get(&self, key: String) -> Answer<Option<String>>;
    /// `value` kept under `key`, in place of any before.
    fn put(&self, key: String, value: String) -> Answer<()>;
    /// What is kept under `key`, gone; nothing kept there is no failure.
    fn remove(&self, key: String) -> Answer<()>;
}

/// Whose a secret is: this app's alone, or every Ark app's on the device.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Scope {
    /// This app's alone: the keychain under its bundle id, its own
    /// keystore key and preferences, its own keyring items.
    #[default]
    Private,
    /// Readable by every Ark app on this device (a device-wide sign-in):
    /// not built yet, so answered Unavailable on every platform. What it
    /// will need on each is in modules/ark-secure-store/README.md.
    Shared,
}

/// What is asked of the secure store, in a scope.
#[derive(Clone, PartialEq, Eq)]
pub enum SecureStoreOp {
    Get(Scope, String),
    Put(Scope, String, String),
    Remove(Scope, String),
}

impl SecureStoreOp {
    pub fn scope(&self) -> Scope {
        match self {
            SecureStoreOp::Get(s, _) | SecureStoreOp::Put(s, _, _) | SecureStoreOp::Remove(s, _) => *s,
        }
    }
}

/// The key and never the secret: an operation is printed where effects
/// are, and a secret is never printed.
impl std::fmt::Debug for SecureStoreOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecureStoreOp::Get(s, k) => f.debug_tuple("Get").field(s).field(k).finish(),
            SecureStoreOp::Put(s, k, _) => f.debug_tuple("Put").field(s).field(k).field(&"…").finish(),
            SecureStoreOp::Remove(s, k) => f.debug_tuple("Remove").field(s).field(k).finish(),
        }
    }
}

impl Op for SecureStoreOp {
    type Module = dyn SecureStore;
    type Output = Option<String>;
    const MODULE: &'static str = "ark-secure-store";
    fn run(&self, m: &dyn SecureStore, reply: Reply<Option<String>>) {
        reply.answer(match self {
            // No platform shares a store between apps yet.
            op if op.scope() == Scope::Shared => Err(ModuleError::Unavailable),
            SecureStoreOp::Get(_, k) => m.get(k.clone()),
            SecureStoreOp::Put(_, k, v) => m.put(k.clone(), v.clone()).map(|()| Some(v.clone())),
            SecureStoreOp::Remove(_, k) => m.remove(k.clone()).map(|()| None),
        })
    }
}

/// The app's own secret kept under `key`.
pub fn get(key: impl Into<String>) -> SecureStoreOp {
    SecureStoreOp::Get(Scope::Private, key.into())
}

/// `value` kept under `key`, the app's own.
pub fn put(key: impl Into<String>, value: impl Into<String>) -> SecureStoreOp {
    SecureStoreOp::Put(Scope::Private, key.into(), value.into())
}

/// The app's own secret under `key`, gone.
pub fn remove(key: impl Into<String>) -> SecureStoreOp {
    SecureStoreOp::Remove(Scope::Private, key.into())
}

/// The secret kept under `key` in `scope`.
pub fn get_in(scope: Scope, key: impl Into<String>) -> SecureStoreOp {
    SecureStoreOp::Get(scope, key.into())
}

/// `value` kept under `key` in `scope`.
pub fn put_in(scope: Scope, key: impl Into<String>, value: impl Into<String>) -> SecureStoreOp {
    SecureStoreOp::Put(scope, key.into(), value.into())
}

/// The secret under `key` in `scope`, gone.
pub fn remove_in(scope: Scope, key: impl Into<String>) -> SecureStoreOp {
    SecureStoreOp::Remove(scope, key.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_op_prints_its_key_and_never_its_secret() {
        let printed = format!("{:?}", put("login", "s3cret"));
        assert!(printed.contains("login") && !printed.contains("s3cret"), "{printed}");
    }

    /// A store that keeps anything, to show the shared scope never reaches it.
    struct Keeps;
    impl SecureStore for Keeps {
        fn get(&self, _: String) -> Answer<Option<String>> {
            Ok(Some("kept".into()))
        }
        fn put(&self, _: String, _: String) -> Answer<()> {
            Ok(())
        }
        fn remove(&self, _: String) -> Answer<()> {
            Ok(())
        }
    }

    #[test]
    fn the_shared_scope_is_unavailable_everywhere_for_now() {
        let mut m = crate::Modules::new();
        m.register::<dyn SecureStore>(std::sync::Arc::new(Keeps));
        for op in [get_in(Scope::Shared, "k"), put_in(Scope::Shared, "k", "v"), remove_in(Scope::Shared, "k")] {
            m.run(&op, |a| assert_eq!(a, Err(ModuleError::Unavailable)));
        }
        m.run(&get_in(Scope::Private, "k"), |a| assert_eq!(a, Ok(Some("kept".into()))));
        assert_eq!(get("k"), get_in(Scope::Private, "k"));
    }
}
