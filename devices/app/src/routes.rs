//! Where devices can be: its status, and Settings. One path each
//! (`ark-router`), as every shell spells them.
use ark_router::Routes;

/// Somewhere devices can be.
#[derive(Debug, Clone, Routes, ark::twin::Twin)]
#[ark(ffi_eq)]
pub enum Route {
    /// What the phone is held to and what it last found.
    #[home]
    #[route("/status")]
    Status,
    #[route("/settings")]
    Settings,
}
