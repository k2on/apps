//! Share: text (and a link) handed to the system's share sheet. Its port —
//! the [`Share`] trait, [`share`], a [`ShareOp`] whose output is `()`,
//! given once the sheet is shown — is the runtime's (`ark_module::share`);
//! here is what implements it: the fake, and (`ffi`) the phones' foreign
//! trait.

use std::sync::{Arc, Mutex};

use ark_module::Modules;

pub use ark_module::share::*;

#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!();

/// The phones' share sheet, through UniFFI (`ffi`): the foreign trait
/// Swift's `PhoneShare` implements, and it registered as the port.
#[cfg(feature = "ffi")]
pub mod phone {
    use std::sync::Arc;

    /// The system's share sheet, as Swift and Kotlin show it.
    #[uniffi::export(with_foreign)]
    pub trait Share: Send + Sync {
        fn share(&self, text: String, url: Option<String>);
    }

    struct Phone(Arc<dyn Share>);

    impl super::Share for Phone {
        fn share(&self, text: String, url: Option<String>) {
            self.0.share(text, url)
        }
    }

    /// The phone's share sheet as the port an update asks through.
    pub fn port(s: Arc<dyn Share>) -> Arc<dyn super::Share> {
        Arc::new(Phone(s))
    }
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
        m.run(&share("Checking", Some("money://account/1".into())), |a| assert_eq!(a, Ok(())));
        assert_eq!(fake.shared(), vec![("Checking".to_string(), Some("money://account/1".to_string()))]);
    }
}
