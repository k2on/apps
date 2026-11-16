//! Category Details: the shell's inspector over the budget, which the
//! host opens itself (`host::Money::inspect`).

/// A category's month, by the category's id.
#[derive(ark_router::Route)]
pub struct Category {
    #[hex]
    pub id: String,
    #[slug]
    pub name: String,
}
