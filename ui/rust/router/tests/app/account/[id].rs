#[derive(ark_router::Route)]
pub struct Account {
    #[hex]
    pub id: String,
    #[slug]
    pub name: String,
}
