//! The tables. Two scopes: what the scanner authors, and what people
//! make. A playlist item names a track across the boundary, unchecked
//! (docs/arkdb.md §3.2).
use ark_builder::*;

pub fn schema(m: &mut ModuleBuilder) {
    m.scope("library", |s| {
        s.table("track", |t| {
            t.id("id");
            t.text("title");
            t.text("artist");
            t.text_opt("album");
            t.int("duration_ms");
            t.text("file");
            t.int("added_ms");
            t.text("user_id");
            t.key(&["id"]);
        });
    });
    m.scope("playlists", |s| {
        s.table("playlist", |t| {
            t.id("id");
            t.text("name");
            t.text("user_id");
            t.int("created_ms");
            t.key(&["id"]);
        });
        s.table("playlist_item", |t| {
            t.id_ref("playlist_id", "playlist");
            t.id_of("track_id", "track");
            t.int("pos");
            t.int("added_ms");
            t.text("user_id");
            t.key(&["playlist_id", "track_id"]);
        });
    });
}
