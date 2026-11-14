//! The secure store, faked for tests: a map, which can be told to fail.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use ark_module::{Answer, ModuleError, Modules};

use crate::SecureStore;

#[derive(Clone, Default)]
pub struct Fake {
    secrets: Arc<Mutex<BTreeMap<String, String>>>,
    failing: Arc<Mutex<Option<String>>>,
}

impl Fake {
    pub fn new() -> Fake {
        Fake::default()
    }

    /// This fake registered as the secure store.
    pub fn register(&self, modules: &mut Modules) {
        modules.register::<dyn SecureStore>(Arc::new(self.clone()));
    }

    /// What is kept under `key`, to look at.
    pub fn kept(&self, key: &str) -> Option<String> {
        self.secrets.lock().unwrap_or_else(|e| e.into_inner()).get(key).cloned()
    }

    /// Every call from now on fails, saying `why`.
    pub fn fail(&self, why: &str) {
        *self.failing.lock().unwrap_or_else(|e| e.into_inner()) = Some(why.into());
    }

    fn failed(&self) -> Answer<()> {
        match self.failing.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            Some(why) => Err(ModuleError::Failed { why }),
            None => Ok(()),
        }
    }
}

impl SecureStore for Fake {
    fn get(&self, key: String) -> Answer<Option<String>> {
        self.failed()?;
        Ok(self.kept(&key))
    }

    fn put(&self, key: String, value: String) -> Answer<()> {
        self.failed()?;
        self.secrets.lock().unwrap_or_else(|e| e.into_inner()).insert(key, value);
        Ok(())
    }

    fn remove(&self, key: String) -> Answer<()> {
        self.failed()?;
        self.secrets.lock().unwrap_or_else(|e| e.into_inner()).remove(&key);
        Ok(())
    }
}
