//! The transforms that moved data from a schema before this one
//! (`src/migrations.rs`), run over rows as a server kept them: exported,
//! transformed, imported; and a device of a build before one, told by a
//! server of this build to update.

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
    let mut tables = ark::dynamic::tables(&Store::from_rows(Arc::new(was), Space::Durable, &old).unwrap()).unwrap();
    harken_domain::migrations::albums_by_folder(&mut tables).unwrap();
    let store = ark::dynamic::store(Arc::clone(&d.schema), Space::Durable, &tables).unwrap();
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

/// A device of the build before albums-by-folder — its schema the one the
/// migration moved from — is told by a server of this build to update, and
/// not served: neither schema grows from the other (`album` gained `id` as
/// its first column), so its rows would be read otherwise. Falsified by a
/// server judging no schema: its `Hello` was answered as any build's.
#[test]
fn a_device_from_before_albums_by_folder_is_told_to_update() {
    use ark::frames::Frame;
    use ark::peer::{Ctx, Input, Output, Peer, Role, To};
    let was: Schema = serde_json::from_str(include_str!("../migrations/albums-by-folder.json")).unwrap();
    assert!(ark::schema::compat(&was, &domain().schema).is_err() && ark::schema::compat(&domain().schema, &was).is_err());
    let mut server = Peer::new(Arc::new(domain()), Role::authority(), Ctx::who("server", "server", vec![]));
    let phone = Some(Ctx::who("alice", "phone", vec![]));
    let hello = Frame::Hello {
        space: Space::Durable,
        cursor: 0,
        hash: Default::default(),
        fill: None,
        most: ark::peer::PART as u64,
        schema: was.hash(),
    };
    let schema = Frame::Schema {
        space: Space::Durable,
        schema: was,
    };
    server.step(Input::Frame {
        from: phone.clone(),
        frame: schema,
    });
    let out = server.step(Input::Frame { from: phone, frame: hello });
    assert!(matches!(&out[..], [Output::Send(To::Back, Frame::Update { .. })]), "{out:?}");
}
