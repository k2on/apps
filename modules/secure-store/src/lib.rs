//! The secure store: secrets kept by the system (the keychain, the Android
//! keystore), such as a login's token. An update asks with [`get`],
//! [`put`] or [`remove`], a [`SecureStoreOp`] whose output is what is kept
//! under the key once it is done: a read's secret (none when nothing is),
//! a put's value, a removal's none.
//!
//! No native implementation yet (README.md).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use ark_module::{Modules, Op, Reply};

#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!();

/// Secrets kept by the system.
#[cfg_attr(feature = "ffi", uniffi::export(with_foreign))]
pub trait SecureStore: Send + Sync {
    fn get(&self, key: String) -> Option<String>;
    fn put(&self, key: String, value: String);
    fn remove(&self, key: String);
}

/// What an update asks of the secure store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SecureStoreOp {
    Get(String),
    Put(String, String),
    Remove(String),
}

impl Op for SecureStoreOp {
    type Module = dyn SecureStore;
    type Output = Option<String>;
    const MODULE: &'static str = "secure-store";
    fn run(&self, m: &dyn SecureStore, reply: Reply<Option<String>>) {
        reply.done(match self {
            SecureStoreOp::Get(k) => m.get(k.clone()),
            SecureStoreOp::Put(k, v) => {
                m.put(k.clone(), v.clone());
                Some(v.clone())
            }
            SecureStoreOp::Remove(k) => {
                m.remove(k.clone());
                None
            }
        })
    }
}

/// The secret kept under `key`.
pub fn get(key: impl Into<String>) -> SecureStoreOp {
    SecureStoreOp::Get(key.into())
}

/// `value` kept under `key`.
pub fn put(key: impl Into<String>, value: impl Into<String>) -> SecureStoreOp {
    SecureStoreOp::Put(key.into(), value.into())
}

/// What is kept under `key`, gone.
pub fn remove(key: impl Into<String>) -> SecureStoreOp {
    SecureStoreOp::Remove(key.into())
}

/// The secure store, faked for tests: a map.
#[derive(Clone, Default)]
pub struct Fake {
    secrets: Arc<Mutex<BTreeMap<String, String>>>,
}

impl Fake {
    pub fn new() -> Fake {
        Fake::default()
    }

    /// This fake registered as the secure store.
    pub fn register(&self, modules: &mut Modules) {
        modules.register::<dyn SecureStore>(Arc::new(self.clone()));
    }
}

impl SecureStore for Fake {
    fn get(&self, key: String) -> Option<String> {
        self.secrets.lock().unwrap_or_else(|e| e.into_inner()).get(&key).cloned()
    }

    fn put(&self, key: String, value: String) {
        self.secrets.lock().unwrap_or_else(|e| e.into_inner()).insert(key, value);
    }

    fn remove(&self, key: String) {
        self.secrets.lock().unwrap_or_else(|e| e.into_inner()).remove(&key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_is_kept_read_and_removed() {
        let mut m = Modules::new();
        Fake::new().register(&mut m);
        let read = |m: &Modules| {
            let got = Arc::new(Mutex::new(None));
            let g = got.clone();
            m.run(&get("token"), move |a| *g.lock().unwrap() = Some(a));
            let a = got.lock().unwrap().clone();
            a
        };
        assert_eq!(read(&m), Some(Ok(None)));
        m.run(&put("token", "t"), |a| assert_eq!(a, Ok(Some("t".into()))));
        assert_eq!(read(&m), Some(Ok(Some("t".into()))));
        m.run(&remove("token"), |a| assert_eq!(a, Ok(None)));
        assert_eq!(read(&m), Some(Ok(None)));
    }
}
