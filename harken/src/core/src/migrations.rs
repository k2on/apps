//! The steps that read the rows of a schema before this one
//! ([`ark::migrate`]), oldest first. Each pins the schema it reads, as the
//! server last stood on it (`migrations/<name>.json`, a copy of
//! `tests/schema.json` then), and is a function over every row as JSON.

use std::collections::{BTreeMap, BTreeSet};

use ark::migrate::{Migration, Tables};
use ark::Refusal;
use serde_json::{Map, Value as Json};

use crate::keys;

/// Every step, oldest first.
pub fn all() -> Vec<Migration> {
    vec![Migration::new(
        "albums by folder",
        include_str!("../migrations/albums-by-folder.json"),
        albums_by_folder,
    )]
}

/// An album was its name, so two recordings of one title were one album;
/// it is now its folder and its name ([`keys::album_id`]), and a song names
/// it by that key (`album_id`, was `album_name`). An album is one per
/// folder its songs are in, each the row its name had, but for its picture
/// when it was split: that is the album's of the folder it was taken from
/// (`.cover/…/<folder>/cover.jpg`, `.catalogue/…`), and the others have
/// none until their files offer one; a picture of no folder's (one taken
/// out of a file, `.art/…`) is each one's. An album no song was on goes,
/// as the albums page never listed it.
fn albums_by_folder(t: &mut Tables) -> Result<(), Refusal> {
    let file_of: BTreeMap<String, String> = t
        .get("media")
        .into_iter()
        .flatten()
        .map(|m| (m["id"].to_string(), m["file"].as_str().unwrap_or_default().to_string()))
        .collect();
    let by_name: BTreeMap<String, Map<String, Json>> = t
        .remove("album")
        .unwrap_or_default()
        .into_iter()
        .map(|a| (a["name"].as_str().unwrap_or_default().to_string(), a))
        .collect();
    let mut folders: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for song in t.get_mut("song").into_iter().flatten() {
        let id = match song.remove("album_name") {
            Some(Json::String(name)) => {
                let file = file_of
                    .get(&song["media_id"].to_string())
                    .ok_or(Refusal::new("a song without its media"))?;
                let folder = keys::folder_of(file);
                folders.entry(name.clone()).or_default().insert(folder.to_string());
                Json::String(keys::album_id(folder, &name))
            }
            _ => Json::Null,
        };
        song.insert("album_id".into(), id);
    }
    let from = |art: &str, folder: &str| art.starts_with(&format!("{folder}/")) || art.contains(&format!("/{folder}/"));
    let mut albums = vec![];
    for (name, folders) in folders {
        let row = by_name.get(&name).ok_or(Refusal(format!("a song on {name}, an album with no row")))?;
        let art = row["art"].as_str().unwrap_or_default();
        let taken_from = folders.iter().any(|f| from(art, f));
        for folder in &folders {
            let mut album = row.clone();
            album.insert("id".into(), Json::String(keys::album_id(folder, &name)));
            if folders.len() > 1 && taken_from && !from(art, folder) {
                album.insert("art".into(), Json::String(String::new()));
            }
            albums.push(album);
        }
    }
    t.insert("album".into(), albums);
    Ok(())
}
