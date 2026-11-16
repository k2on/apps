use crate::pages::{pages, OVERVIEW};

/// Any other page, by its tag: a component's, a section's, the index of
/// every page.
#[derive(ark_router::Route)]
pub struct Page {
    pub tag: String,
}

impl Page {
    /// Every page but the overview, in reading order: each is written.
    pub fn params() -> Vec<Page> {
        pages("/")
            .into_iter()
            .filter(|p| p.tag != OVERVIEW)
            .map(|p| Page { tag: p.tag })
            .collect()
    }
}
