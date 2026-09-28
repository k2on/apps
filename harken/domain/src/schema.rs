//! The tables, as the scopes that hold them. This file is the schema:
//! a scope is a struct of tables, a table is a row struct with its
//! columns, key, indexes and references said once in `Row::columns`.
use ark::authoring::*;

/// Every track the scanner has found. One log for the whole library.
pub struct Library {
    pub track: Table<Track>,
}
impl Scope for Library {
    const NAME: &str = "library";
}

/// What people make of it: playlists, and what is on them.
pub struct Playlists {
    pub playlist: Table<Playlist>,
    pub playlist_item: Table<PlaylistItem>,
}
impl Scope for Playlists {
    const NAME: &str = "playlists";
}

pub struct Track {
    pub id: Id<Track>,
    pub title: Text,
    pub artist: Text,
    pub album: Opt<Text>,
    pub duration_ms: Int,
    pub file: Text,
    pub added_ms: Int,
    pub user_id: Text,
}
impl Row for Track {
    const NAME: &str = "track";
    type Key = (Id<Track>,);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::id)
            .text(Self::title)
            .text(Self::artist)
            .text(Self::album)
            .nullable()
            .int(Self::duration_ms)
            .text(Self::file)
            .int(Self::added_ms)
            .text(Self::user_id)
            .key((Self::id,))
            .unique((Self::file,))
    }
}
impl Track {
    pub const id: Col<Self, Id<Self>> = col("id");
    pub const title: Col<Self, Text> = col("title");
    pub const artist: Col<Self, Text> = col("artist");
    pub const album: Col<Self, Opt<Text>> = col("album");
    pub const duration_ms: Col<Self, Int> = col("duration_ms");
    pub const file: Col<Self, Text> = col("file");
    pub const added_ms: Col<Self, Int> = col("added_ms");
    pub const user_id: Col<Self, Text> = col("user_id");
}

pub struct Playlist {
    pub id: Id<Playlist>,
    pub name: Text,
    pub user_id: Text,
    pub created_ms: Int,
}
impl Row for Playlist {
    const NAME: &str = "playlist";
    type Key = (Id<Playlist>,);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::id)
            .text(Self::name)
            .text(Self::user_id)
            .int(Self::created_ms)
            .key((Self::id,))
            .unique((Self::user_id, Self::name))
    }
}
impl Playlist {
    pub const id: Col<Self, Id<Self>> = col("id");
    pub const name: Col<Self, Text> = col("name");
    pub const user_id: Col<Self, Text> = col("user_id");
    pub const created_ms: Col<Self, Int> = col("created_ms");
    pub const playlist_item: Rel<Self, PlaylistItem> = rel("playlist_item");
}

pub struct PlaylistItem {
    pub playlist_id: Id<Playlist>,
    pub track_id: Id<Track>,
    pub pos: Int,
    pub added_ms: Int,
    pub user_id: Text,
}
impl Row for PlaylistItem {
    const NAME: &str = "playlist_item";
    type Key = (Id<Playlist>, Id<Track>);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::playlist_id)
            .refs::<Playlist>()
            .id(Self::track_id)
            .int(Self::pos)
            .int(Self::added_ms)
            .text(Self::user_id)
            .key((Self::playlist_id, Self::track_id))
    }
}
impl PlaylistItem {
    pub const playlist_id: Col<Self, Id<Playlist>> = col("playlist_id");
    pub const track_id: Col<Self, Id<Track>> = col("track_id");
    pub const pos: Col<Self, Int> = col("pos");
    pub const added_ms: Col<Self, Int> = col("added_ms");
    pub const user_id: Col<Self, Text> = col("user_id");
}
