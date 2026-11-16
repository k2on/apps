/// A numbered page: typed, so `/page/x/` is no page.
#[derive(ark_router::Route)]
pub struct Page {
    pub n: u32,
}

impl Page {
    /// The pages a static export writes.
    pub fn params() -> Vec<Page> {
        vec![Page { n: 1 }, Page { n: 2 }]
    }
}
