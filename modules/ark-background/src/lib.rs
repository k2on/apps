//! The periodic background sync. Its port — the [`Background`] trait and
//! its calls ([`set`], answered `()`; [`wanted`], answered what was last
//! set) — is the runtime's (`ark_module::background`); here is what
//! implements it:
//!
//! - iPhone and iPad: `PhoneBackground` (swift/: a `BGAppRefreshTask`,
//!   `<bundle id>.sync`), Mac: `NSBackgroundActivityScheduler`; through
//!   `phone` (feature `ffi`).
//! - Android: `PhoneBackground` (kotlin/: a periodic `JobScheduler` job,
//!   `ArkSyncJob`, kept across restarts), through `phone`.
//! - The desktop and the browser: none (`Unavailable`).
//! - Tests: [`Fake`].
//!
//! Each run is a wake of the process's one runtime (`ArkProcess::wake`,
//! `Woken::Sync`), never a runtime of the platform's own.

use std::sync::{Arc, Mutex};

pub use ark_module::background::*;
use ark_module::{Modules, Reply};

#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!();

/// The background, faked for tests: what was last set.
#[derive(Clone, Default)]
pub struct Fake {
    every: Arc<Mutex<Option<u32>>>,
}

impl Fake {
    pub fn new() -> Fake {
        Fake::default()
    }

    /// This fake registered as the background.
    pub fn register(&self, modules: &mut Modules) {
        modules.register::<dyn Background>(Arc::new(self.clone()));
    }

    /// How often it would be woken; none, never.
    pub fn every(&self) -> Option<u32> {
        *self.every.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Background for Fake {
    fn set(&self, every_s: Option<u32>) {
        *self.every.lock().unwrap_or_else(|e| e.into_inner()) = every_s
    }
    fn wanted(&self, reply: Reply<Option<u32>>) {
        reply.done(self.every())
    }
}

/// The phones' background, through UniFFI (`ffi`).
#[cfg(feature = "ffi")]
pub mod phone {
    use std::sync::Arc;

    /// How often it was last set, answered once.
    #[derive(uniffi::Object)]
    pub struct WantedReply(ark_module::Reply<Option<u32>>);

    #[uniffi::export]
    impl WantedReply {
        pub fn wanted(&self, every_s: Option<u32>) {
            self.0.done(every_s)
        }
    }

    /// The phone's background, as Swift and Kotlin implement it.
    #[uniffi::export(with_foreign)]
    pub trait NativeBackground: Send + Sync {
        fn set(&self, every_s: Option<u32>);
        fn wanted(&self, reply: Arc<WantedReply>);
    }

    struct Phone(Arc<dyn NativeBackground>);

    impl super::Background for Phone {
        fn set(&self, every_s: Option<u32>) {
            self.0.set(every_s)
        }
        fn wanted(&self, reply: ark_module::Reply<Option<u32>>) {
            self.0.wanted(Arc::new(WantedReply(reply)))
        }
    }

    /// The phone's background as the port an update asks through.
    pub fn port(b: Arc<dyn NativeBackground>) -> Arc<dyn super::Background> {
        Arc::new(Phone(b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_until_set_and_off_again() {
        let fake = Fake::new();
        let mut m = Modules::new();
        fake.register(&mut m);
        m.run(&wanted(), |a| assert_eq!(a, Ok(None)));
        m.run(&set(Some(900)), |_| {});
        m.run(&wanted(), |a| assert_eq!(a, Ok(Some(900))));
        m.run(&set(None), |_| {});
        assert_eq!(fake.every(), None);
        Modules::new().run(&set(Some(900)), |a| assert_eq!(a, Err(ark_module::ModuleError::Unavailable)));
    }
}
