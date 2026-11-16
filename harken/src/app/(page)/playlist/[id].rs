/// A playlist, by its id (`Id::hex`), its name after it only to read.
#[derive(ark_router::Route)]
pub struct Playlist {
    #[hex]
    pub id: String,
    #[slug]
    pub name: String,
}
