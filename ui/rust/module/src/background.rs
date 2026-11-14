//! The background's port: whether, and how often, the OS wakes the process
//! to sync with no screen (ui/docs/app.md, Background And Notifications).
//! Its implementations — a `BGAppRefreshTask` on the iPhone and the iPad,
//! `NSBackgroundActivityScheduler` on the Mac, a `JobScheduler` job on
//! Android — and its fake are the module's (modules/ark-background). The
//! desktop and the browser have none: their operations answer
//! `Unavailable`.
//!
//! **Off by default.** Nothing is scheduled until the app asks
//! ([`set`]), and an app asks only once the person has turned it on in
//! the app (`ark_app::background`'s setting); [`set`] with none stops it.
//! What was asked is kept across launches by the platform ([`wanted`]).
//! An app whose app.toml declares no `[background]` has none of this in
//! its projects at all.

use crate::{Op, Reply};

/// The periodic background sync.
pub trait Background: Send + Sync {
    /// Woken about every `every_s` seconds (the OS decides when: at least
    /// fifteen minutes apart on both phones), or never again with none.
    /// Kept across launches and restarts.
    fn set(&self, every_s: Option<u32>);
    /// What was last set.
    fn wanted(&self, reply: Reply<Option<u32>>);
}

/// The sync scheduled, or stopped; answered `()` at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetSync(pub Option<u32>);

impl Op for SetSync {
    type Module = dyn Background;
    type Output = ();
    const MODULE: &'static str = "ark-background";
    fn run(&self, m: &dyn Background, reply: Reply<()>) {
        m.set(self.0);
        reply.done(())
    }
}

/// What was last set: how often, or none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wanted;

impl Op for Wanted {
    type Module = dyn Background;
    type Output = Option<u32>;
    const MODULE: &'static str = "ark-background";
    fn run(&self, m: &dyn Background, reply: Reply<Option<u32>>) {
        m.wanted(reply)
    }
}

/// Woken about every `every_s` seconds, or never with none.
pub fn set(every_s: Option<u32>) -> SetSync {
    SetSync(every_s)
}

/// What was last set.
pub fn wanted() -> Wanted {
    Wanted
}
