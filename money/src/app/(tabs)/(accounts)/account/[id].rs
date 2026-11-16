//! An account is its own place in the sidebar, a child of Accounts
//! (`accounts/<id>`, `crate::shell::account_tag`): Accounts' tab with its
//! register pushed where the window is compact.

/// One account's register, by the account's id; its name is the title.
#[derive(ark_router::Route)]
#[tab = "accounts/{id}"]
pub struct Account {
    #[hex]
    pub id: String,
    #[slug]
    pub name: String,
}
