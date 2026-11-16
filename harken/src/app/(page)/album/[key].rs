/// An album by its key (`keys::album_id`), and its name: two records of
/// one title are two albums.
#[derive(ark_router::Route)]
pub struct Album {
    pub key: String,
    pub title: String,
}
