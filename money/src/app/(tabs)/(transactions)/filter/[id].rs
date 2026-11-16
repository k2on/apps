/// A saved filter's register, by its id.
#[derive(ark_router::Route)]
pub struct Filter {
    #[hex]
    pub id: String,
    #[slug]
    pub name: String,
}
