//! The steps that read the rows of a schema before this one
//! (`src/migrations.rs`), run over rows as a server kept them.

use std::sync::Arc;

use ark::schema::{Schema, Space};
use ark::store::Store;
use harken_domain::{domain, Album, Song};
use serde_json::{json, Value as Json};

/// Rows of `schema`'s tables, from their JSON objects.
fn rows(schema: &Schema, tables: &[(&str, Vec<Json>)]) -> ark::log::Rows {
    tables
        .iter()
        .map(|(name, objects)| {
            let (_, t) = schema.table(name).unwrap();
            (name.to_string(), objects.iter().map(|o| ark::dynamic::row_bytes(t, o).unwrap()).collect())
        })
        .collect()
}

fn media(id: u8, file: &str) -> Json {
    json!({"id": format!("{id:032x}"), "kind": "song", "title": "I. Allegro", "creator": "Antonio Vivaldi",
        "duration_ms": 1, "file": file, "pos": id, "added_ms": 1, "user_id": "library"})
}

fn song(id: u8, album: Option<&str>, by: &str) -> Json {
    json!({"media_id": format!("{id:032x}"), "album_name": album, "disc": 1, "track": 1,
        "recording_id": format!("antonio-vivaldi/rv-269@{by}"), "movement_id": null, "bpm": 0})
}

fn recording(by: &str) -> Json {
    json!({"id": format!("antonio-vivaldi/rv-269@{by}"), "work_id": null, "recorded": 0, "venue": "",
        "label": "", "licence": "", "art": "", "added_ms": 1, "user_id": "library"})
}

/// The Four Seasons in a folder of the library's and in the catalogue's,
/// once one album by its name, are two by their folders, each its year as
/// the one row had it and the picture only the catalogue's, whose folder it
/// was taken from; a folder's other album is its own, its picture with it;
/// an album no song was on goes; a song on none stays on none. Falsified by
/// `album_id` keyed by the name alone: one Four Seasons; and by each split
/// album keeping the picture: the library's had the catalogue's.
#[test]
fn albums_of_one_name_in_two_folders_are_two() {
    let d = domain();
    let was: Schema = serde_json::from_str(include_str!("../migrations/albums-by-folder.json")).unwrap();
    let album = |name: &str, art: &str| json!({"name": name, "label": "", "released": 1977, "art": art, "added_ms": 1, "user_id": "library"});
    let old = rows(
        &was,
        &[
            (
                "media",
                vec![
                    media(1, "music/Vivaldi/The Four Seasons/01 Spring.mp3"),
                    media(2, ".catalogue/a1b2.flac"),
                    media(3, "music/Vivaldi/The Four Seasons/13 Encore.mp3"),
                    media(4, "music/loose.mp3"),
                ],
            ),
            (
                "album",
                vec![
                    album("The Four Seasons", ".catalogue/c061.png"),
                    album("Encores", ".art/7b2a.png"),
                    album("Nobody's", ""),
                ],
            ),
            ("recording", vec![recording("bernstein"), recording("modena")]),
            (
                "song",
                vec![
                    song(1, Some("The Four Seasons"), "bernstein"),
                    song(2, Some("The Four Seasons"), "modena"),
                    song(3, Some("Encores"), "bernstein"),
                    song(4, None, "bernstein"),
                ],
            ),
        ],
    );
    let new = ark::migrate::migrate(&was, &old, &d.schema, &d.migrations).unwrap().expect("migrated");
    let (store, _) = Store::from_rows(Arc::clone(&d.schema), Space::Durable, &new).unwrap();
    let (albums, songs) = ark::db::read::<harken_domain::Harken, _>(&store, |db| (db.album.scan(), db.song.scan()));
    let ids: Vec<(&str, &str, &str, i64)> = albums
        .iter()
        .map(|a: &Album| (a.id.as_str(), a.name.as_str(), a.art.as_str(), a.released))
        .collect();
    assert_eq!(
        ids,
        [
            (".catalogue/the-four-seasons", "The Four Seasons", ".catalogue/c061.png", 1977),
            ("music/Vivaldi/The Four Seasons/encores", "Encores", ".art/7b2a.png", 1977),
            ("music/Vivaldi/The Four Seasons/the-four-seasons", "The Four Seasons", "", 1977),
        ]
    );
    let on: Vec<Option<&str>> = songs.iter().map(|s: &Song| s.album_id.as_deref()).collect();
    assert_eq!(
        on,
        [
            Some("music/Vivaldi/The Four Seasons/the-four-seasons"),
            Some(".catalogue/the-four-seasons"),
            Some("music/Vivaldi/The Four Seasons/encores"),
            None
        ]
    );
}
