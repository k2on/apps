#[derive(ark_router::Route)]
pub struct Search {
    #[query = "q"]
    pub text: Option<String>,
    #[query]
    pub max_rows: Option<u32>,
}
