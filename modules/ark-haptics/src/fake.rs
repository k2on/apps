//! The haptics, faked for tests: what was performed is kept to look at.

use std::sync::{Arc, Mutex};

use ark_module::Modules;

use crate::{Haptic, Haptics};

#[derive(Clone, Default)]
pub struct Fake {
    seen: Arc<Mutex<Vec<Haptic>>>,
}

impl Fake {
    pub fn new() -> Fake {
        Fake::default()
    }

    /// This fake registered as the haptics.
    pub fn register(&self, modules: &mut Modules) {
        modules.register::<dyn Haptics>(Arc::new(self.clone()));
    }

    /// The haptics performed, in order.
    pub fn performed(&self) -> Vec<Haptic> {
        self.seen.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl Haptics for Fake {
    fn perform(&self, haptic: Haptic) {
        self.seen.lock().unwrap_or_else(|e| e.into_inner()).push(haptic)
    }
}
