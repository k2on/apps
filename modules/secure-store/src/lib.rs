//! The secure store: secrets kept by the system (the keychain, the Android
//! keystore), such as a login's token. An update asks with [`get`],
//! [`put`] or [`remove`]; a read answers `Text` (none when nothing is kept
//! under the key), the others `Done`.
//!
//! No native implementation yet (README.md).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use ark_module::{Call, ModuleAnswer, ModuleCall, Modules, Reply};

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
pub enum SecureStoreCall {
    Get(String),
    Put(String, String),
    Remove(String),
}

impl Call for SecureStoreCall {
    type Module = dyn SecureStore;
    const MODULE: &'static str = "secure-store";
    fn run(&self, m: &dyn SecureStore, reply: Arc<Reply>) {
        reply.answer(match self {
            SecureStoreCall::Get(k) => ModuleAnswer::Text { value: m.get(k.clone()) },
            SecureStoreCall::Put(k, v) => {
                m.put(k.clone(), v.clone());
                ModuleAnswer::Done
            }
            SecureStoreCall::Remove(k) => {
                m.remove(k.clone());
                ModuleAnswer::Done
            }
        })
    }
}

/// The secret kept under `key`.
pub fn get(key: impl Into<String>) -> ModuleCall {
    SecureStoreCall::Get(key.into()).into()
}

/// `value` kept under `key`.
pub fn put(key: impl Into<String>, value: impl Into<String>) -> ModuleCall {
    SecureStoreCall::Put(key.into(), value.into()).into()
}

/// What is kept under `key`, gone.
pub fn remove(key: impl Into<String>) -> ModuleCall {
    SecureStoreCall::Remove(key.into()).into()
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
            m.call(get("token"), move |a| *g.lock().unwrap() = Some(a));
            let a = got.lock().unwrap().clone();
            a
        };
        assert_eq!(read(&m), Some(ModuleAnswer::Text { value: None }));
        m.call(put("token", "t"), |_| {});
        assert_eq!(read(&m), Some(ModuleAnswer::Text { value: Some("t".into()) }));
        m.call(remove("token"), |_| {});
        assert_eq!(read(&m), Some(ModuleAnswer::Text { value: None }));
    }
}
