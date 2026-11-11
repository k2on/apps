//! The scanner, faked for tests: a scan or a photo picked waits until the
//! test answers it, oldest first, the two in one queue.

use std::sync::{Arc, Mutex};

use ark_module::{Modules, Reply};

/// Scans waiting, oldest first.
#[derive(Clone, Default)]
pub struct Fake {
    waiting: Arc<Mutex<Vec<crate::PageReply>>>,
}

impl Fake {
    pub fn new() -> Fake {
        Fake::default()
    }

    /// This fake registered as the scanner.
    pub fn register(&self, modules: &mut Modules) {
        modules.register::<dyn super::Scanner>(Arc::new(self.clone()));
    }

    /// How many scans wait.
    pub fn waiting(&self) -> usize {
        self.waiting.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// The oldest scan answered with `jpeg` (none: backed out).
    pub fn scanned(&self, jpeg: Option<Vec<u8>>) {
        let mut w = self.waiting.lock().unwrap_or_else(|e| e.into_inner());
        if !w.is_empty() {
            w.remove(0).done(jpeg)
        }
    }

    /// The oldest scan failed, in the platform's words.
    pub fn failed(&self, why: &str) {
        let mut w = self.waiting.lock().unwrap_or_else(|e| e.into_inner());
        if !w.is_empty() {
            w.remove(0).failed(why)
        }
    }
}

impl super::Scanner for Fake {
    fn scan_page(&self, reply: Reply<Option<Vec<u8>>>) {
        self.waiting.lock().unwrap_or_else(|e| e.into_inner()).push(reply)
    }

    fn pick_photo(&self, reply: Reply<Option<Vec<u8>>>) {
        self.waiting.lock().unwrap_or_else(|e| e.into_inner()).push(reply)
    }
}
