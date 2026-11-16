//! All Accounts is a register, so it is the Transactions tab's: the
//! Accounts tab's own place (its shelves) has no route, and the shell
//! spells it `/accounts/` itself.

/// Every account's register: All Accounts.
#[derive(ark_router::Route)]
pub struct Accounts;
