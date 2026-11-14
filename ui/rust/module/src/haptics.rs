//! Haptics' port: the phone's answer in the hand (`ui/docs/motion.md`,
//! Haptics). Fired on the change itself, never on a tap that might change
//! nothing. An update asks with [`perform`], a [`HapticsOp`] whose output
//! is `()` (nothing says whether it was felt), or `Unavailable` where there
//! is none (the desktop, the browser). Its implementations, its fake and
//! the phones' are the module's (modules/ark-haptics, `ark-haptics`).

use crate::{Op, Reply};

/// What a haptic says: `ui/docs/motion.md`'s three, kept for what matters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Haptic {
    /// Something was kept: a transaction saved, an account added.
    Success,
    /// A choice stepped: a month, a tick, a detent.
    Selection,
    /// Something went that cannot simply come back: an account closed.
    Warning,
}

/// The phone's haptics.
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
    const MODULE: &'static str = "ark-haptics";
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
