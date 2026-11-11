//! Haptics: the phone's answer in the hand (`ui/docs/motion.md`,
//! Haptics). Its port — [`Haptic`], the [`Haptics`] trait, [`perform`], a
//! [`HapticsOp`] whose output is `()` — is the runtime's
//! (`ark_module::haptics`), which an update asks through; here is what
//! implements it: the fake, and (`ffi`) the phones' foreign trait.

pub use ark_module::haptics::*;

#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!();

pub mod fake;

/// The phones' haptics, through UniFFI (`ffi`): the foreign trait Swift's
/// and Kotlin's `PhoneHaptics` implement, and it registered as the port.
#[cfg(feature = "ffi")]
pub mod phone {
    use std::sync::Arc;

    use super::Haptic;

    #[uniffi::remote(Enum)]
    pub enum Haptic {
        Success,
        Selection,
        Warning,
    }

    /// The phone's haptics, as Swift and Kotlin implement them.
    #[uniffi::export(with_foreign)]
    pub trait Haptics: Send + Sync {
        fn perform(&self, haptic: Haptic);
    }

    struct Phone(Arc<dyn Haptics>);

    impl super::Haptics for Phone {
        fn perform(&self, haptic: Haptic) {
            self.0.perform(haptic)
        }
    }

    /// The phone's haptics as the port an update asks through.
    pub fn port(h: Arc<dyn Haptics>) -> Arc<dyn super::Haptics> {
        Arc::new(Phone(h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_module::Modules;

    #[test]
    fn a_haptic_reaches_the_registered_module() {
        let fake = fake::Fake::new();
        let mut m = Modules::new();
        fake.register(&mut m);
        let got = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
        let g = got.clone();
        m.run(&perform(Haptic::Warning), move |a| g.lock().unwrap().push(a));
        assert_eq!(fake.performed(), vec![Haptic::Warning]);
        assert_eq!(*got.lock().unwrap(), vec![Ok(())]);
    }
}
