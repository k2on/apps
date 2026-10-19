//! What a screen holds of a library list: [`Item`], one row of what
//! `library`, `search`, `album`, `artist`, `recording` and `playlist`
//! answer — the record itself, decoded by its derived `Node`.

use ark5::author::Node;
use ark5::Refusal;

pub use crate::library::LibraryEntry as Item;

impl Item {
    /// The rows of a query's answer, a list of entries.
    pub fn list(bytes: &[u8]) -> Result<Vec<Item>, Refusal> {
        Vec::<Item>::from_bytes(bytes)
    }

    /// On the playlist it was read against.
    pub fn on_playlist(&self) -> bool {
        self.playlist_pos.is_some()
    }
}
