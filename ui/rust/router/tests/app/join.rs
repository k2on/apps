#[derive(ark_router::Route)]
pub struct Join {
    #[query]
    pub server: String,
    #[query]
    pub code: Option<String>,
}
