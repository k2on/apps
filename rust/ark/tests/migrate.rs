//! §1.2's other half: rows of a schema the domain's does not grow from are
//! read through its migrations ([`ark::migrate`]).

#[path = "demo.rs"]
mod demo;

use std::sync::Arc;

use ark::migrate::{migrate, Migration, Tables};
use ark::prelude::*;
use ark::schema::Schema;
use ark::store::Store;
use demo::{playlist_row, Item};

/// The demo's schema with `item.track_id` called `media_id`: what does not
/// grow from it.
fn renamed() -> Arc<Schema> {
    let mut s = demo::schema();
    s.tables[1].columns[1].name = "media_id".into();
    assert!(ark::schema::compat(&demo::schema(), &s).is_err());
    Arc::new(s)
}

fn track_to_media(t: &mut Tables) -> Result<(), Refusal> {
    for item in t.get_mut("item").into_iter().flatten() {
        let v = item.remove("track_id").ok_or(Refusal::new("an item without a track"))?;
        item.insert("media_id".into(), v);
    }
    Ok(())
}

fn rows() -> ark::log::Rows {
    let item = Item {
        playlist_id: Id::new([1; 16]),
        track_id: "t1".into(),
        pos: 3,
    };
    vec![
        ("playlist".into(), vec![playlist_row(&[1; 16], "Hymns", "max")]),
        ("item".into(), vec![item.to_bytes()]),
    ]
}

/// A column renamed: the step's rows read under the new schema with the
/// value under its new name, the other table's as they were. Falsified by
/// `read` handing the step the rows as the new schema reads them: the
/// item has no `track_id` to rename.
#[test]
fn a_step_reads_rows_the_new_schema_does_not() {
    let step = Migration::new("item.media_id", &serde_json::to_string(&demo::schema()).unwrap(), track_to_media);
    let to = renamed();
    let rows = migrate(&demo::schema(), &rows(), &to, &[step]).unwrap().expect("migrated");
    let (store, other) = Store::from_rows(to.clone(), ark::schema::Space::Durable, &rows).unwrap();
    assert!(!other, "every row is the new schema's as it is");
    let (_, t) = to.table("item").unwrap();
    let items = store.snapshot_rows().into_iter().find(|(n, _)| n == "item").unwrap().1;
    let item = ark::dynamic::row(t, &items[0]).unwrap();
    assert_eq!(item["media_id"], "t1");
    assert_eq!(item["pos"], 3);
    assert_eq!(rows[0], ("playlist".into(), vec![playlist_row(&[1; 16], "Hymns", "max")]));
}

/// Rows of a schema that grows into the new one need no step; rows no step
/// reads are refused, never read as something else. Falsified by `migrate`
/// running the first step whatever schema the rows are of.
#[test]
fn rows_no_step_reads_are_refused() {
    assert_eq!(migrate(&demo::schema(), &rows(), &demo::shared(), &[]), Ok(None));
    let mut other = demo::schema();
    other.tables[0].columns[1].name = "title".into();
    let step = Migration::new("item.media_id", &serde_json::to_string(&other).unwrap(), track_to_media);
    let why = migrate(&demo::schema(), &rows(), &renamed(), &[step]).unwrap_err();
    assert!(why.0.starts_with("no migration reads rows of schema"), "{}", why.0);
}

/// Steps run in order, each written under the next one's schema, the last
/// under the domain's: two renames, one after the other. Falsified by
/// writing every step's rows under the domain's schema.
#[test]
fn steps_run_in_order() {
    let mid = renamed();
    let mut last = (*mid).clone();
    last.tables[1].columns[1].name = "song_id".into();
    fn media_to_song(t: &mut Tables) -> Result<(), Refusal> {
        for item in t.get_mut("item").into_iter().flatten() {
            let v = item.remove("media_id").unwrap();
            item.insert("song_id".into(), v);
        }
        Ok(())
    }
    let steps = [
        Migration::new("item.media_id", &serde_json::to_string(&demo::schema()).unwrap(), track_to_media),
        Migration::new("item.song_id", &serde_json::to_string(&*mid).unwrap(), media_to_song),
    ];
    let to = Arc::new(last);
    let rows = migrate(&demo::schema(), &rows(), &to, &steps).unwrap().unwrap();
    let (_, t) = to.table("item").unwrap();
    let item = ark::dynamic::row(t, &rows[1].1[0]).unwrap();
    assert_eq!(item["song_id"], "t1");
}
