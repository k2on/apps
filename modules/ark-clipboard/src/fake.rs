//! The clipboard, faked for tests: what was copied is kept to look at.

use std::sync::{Arc, Mutex};

use ark_module::Modules;

use crate::Clipboard;

#[derive(Clone, Default)]
pub struct Fake {
    seen: Arc<Mutex<Vec<String>>>,
}

impl Fake {
    pub fn new() -> Fake {
        Fake::default()
    }

    /// This fake registered as the clipboard.
    pub fn register(&self, modules: &mut Modules) {
        modules.register::<dyn Clipboard>(Arc::new(self.clone()));
    }

    /// What was put on the clipboard, in order.
    pub fn copied(&self) -> Vec<String> {
        self.seen.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl Clipboard for Fake {
    fn copy_text(&self, text: String) {
        self.seen.lock().unwrap_or_else(|e| e.into_inner()).push(text)
    }
}
