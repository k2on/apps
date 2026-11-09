//! Share: text (and a link) handed to the system's share sheet. An update
//! asks with [`share`]; the module answers `Done` once the sheet is shown.

use std::sync::{Arc, Mutex};

use ark_module::{Call, ModuleAnswer, ModuleCall, ModuleReply, Modules};

#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!();

/// The system's share sheet.
#[cfg_attr(feature = "ffi", uniffi::export(with_foreign))]
pub trait Share: Send + Sync {
    fn share(&self, text: String, url: Option<String>);
}

/// What an update asks of the share sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShareCall {
    Share { text: String, url: Option<String> },
}

impl Call for ShareCall {
    type Module = dyn Share;
    const MODULE: &'static str = "share";
    fn run(&self, m: &dyn Share, reply: Arc<ModuleReply>) {
        match self {
            ShareCall::Share { text, url } => m.share(text.clone(), url.clone()),
        }
        reply.answer(ModuleAnswer::Done)
    }
}

/// `text`, and `url` when there is one, handed to the share sheet.
pub fn share(text: impl Into<String>, url: Option<String>) -> ModuleCall {
    ShareCall::Share { text: text.into(), url }.into()
}

/// What was shared: the text, and the link when there was one.
pub type Shared = (String, Option<String>);

/// The share sheet, faked for tests: what was shared is kept to look at.
#[derive(Clone, Default)]
pub struct Fake {
    seen: Arc<Mutex<Vec<Shared>>>,
}

impl Fake {
    pub fn new() -> Fake {
        Fake::default()
    }

    /// This fake registered as the share sheet.
    pub fn register(&self, modules: &mut Modules) {
        modules.register::<dyn Share>(Arc::new(self.clone()));
    }

    /// What was shared, in order.
    pub fn shared(&self) -> Vec<Shared> {
        self.seen.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl Share for Fake {
    fn share(&self, text: String, url: Option<String>) {
        self.seen.lock().unwrap_or_else(|e| e.into_inner()).push((text, url))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_share_reaches_the_registered_module() {
        let fake = Fake::new();
        let mut m = Modules::new();
        fake.register(&mut m);
        m.call(share("Checking", Some("money://account/1".into())), |a| assert_eq!(a, ModuleAnswer::Done));
        assert_eq!(fake.shared(), vec![("Checking".to_string(), Some("money://account/1".to_string()))]);
    }
}
