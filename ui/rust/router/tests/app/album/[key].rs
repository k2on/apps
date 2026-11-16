/// An album by its key, its title only to show.
#[derive(ark_router::Route)]
pub struct Album {
    pub key: String,
    pub title: String,
}
