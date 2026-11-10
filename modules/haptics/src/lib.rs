//! Haptics: the phone's answer in the hand (`ui/docs/motion.md`,
//! Haptics). Fired on the change itself, never on a tap that might change
//! nothing. An update asks with [`perform`], a [`HapticsOp`] whose output
//! is `()` (nothing says whether it was felt), or `Unavailable` where there
//! is none (the desktop, the browser).

use ark_module::{Op, Reply};

#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!();

pub mod fake;

/// What a haptic says: `ui/docs/motion.md`'s three, kept for what matters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "ffi", derive(uniffi::Enum))]
pub enum Haptic {
    /// Something was kept: a transaction saved, an account added.
    Success,
    /// A choice stepped: a month, a tick, a detent.
    Selection,
    /// Something went that cannot simply come back: an account closed.
    Warning,
}

/// The phone's haptics.
#[cfg_attr(feature = "ffi", uniffi::export(with_foreign))]
pub trait Haptics: Send + Sync {
    fn perform(&self, haptic: Haptic);
}

/// What an update asks of the haptics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HapticsOp {
    Perform(Haptic),
}

impl Op for HapticsOp {
    type Module = dyn Haptics;
    type Output = ();
    const MODULE: &'static str = "haptics";
    fn run(&self, m: &dyn Haptics, reply: Reply<()>) {
        match *self {
            HapticsOp::Perform(h) => m.perform(h),
        }
        reply.done(())
    }
}

/// `haptic` performed.
pub fn perform(haptic: Haptic) -> HapticsOp {
    HapticsOp::Perform(haptic)
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
