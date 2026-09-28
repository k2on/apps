//! harken's domain as the screens use it: its tables `track`, `playlist`
//! and `playlist_item`; its queries `library`, `playlists` and
//! `playlist_items`, run natively over a scope's optimistic store; its
//! mutators `create_playlist`, `add_to_playlist` and `remove_from_playlist`
//! as [`Call`]s the peer authors through harken's own procedures
//! (`harken_domain::module()`). `add_track` is the scanner's and is
//! deliberately not offered as a [`Call`]; a peer alone authors it by name
//! (`Peer::author_by_intent`).

use std::collections::BTreeMap;

use ark::authoring::Procedure;
use ark::eval::{Args, Ctx};
use ark::hash::FnHash;
use ark::store::MemoryStore;
use ark::value::{Id, Value};

pub use harken_domain::{LIBRARY, PLAYLISTS};

/// One call of a mutator: which, into which scope, with which input. The
/// procedure that applies it is harken's own, run natively.
pub struct Call {
    pub name: &'static str,
    pub scope: &'static str,
    pub args: Args,
}

/// A scope's optimistic store as the queries read it: the store, who is
/// asking, and the procedures.
pub struct Db<'a> {
    pub store: &'a MemoryStore,
    pub ctx: &'a Ctx,
    pub procs: &'a BTreeMap<String, (FnHash, Procedure)>,
}

fn query(db: &Db, name: &str, args: Args) -> Result<Value, String> {
    let (_, p) = db.procs.get(name).ok_or_else(|| format!("no procedure {name}"))?;
    p.query(db.ctx, &args, db.store).map_err(|e| format!("{name}: {e:?}"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    pub id: Id,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Playlist {
    pub id: Id,
    pub name: String,
    pub user_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub playlist_id: Id,
    pub track_id: Id,
    pub pos: i64,
}

fn text_opt(v: Value) -> Option<String> {
    match v {
        Value::Text(t) => Some(t),
        _ => None,
    }
}

/// A `track` row as the screens hold it.
pub fn track_of(row: &Value) -> Track {
    Track {
        id: row.field("id").as_id(),
        title: row.field("title").as_text().to_string(),
        artist: row.field("artist").as_text().to_string(),
        album: text_opt(row.field("album")),
        duration_ms: row.field("duration_ms").as_int(),
    }
}

fn playlist_of(row: &Value) -> Playlist {
    Playlist {
        id: row.field("id").as_id(),
        name: row.field("name").as_text().to_string(),
        user_id: row.field("user_id").as_text().to_string(),
    }
}

fn item_of(row: &Value) -> Item {
    Item {
        playlist_id: row.field("playlist_id").as_id(),
        track_id: row.field("track_id").as_id(),
        pos: row.field("pos").as_int(),
    }
}

// -- reads: harken's queries, over the `library` and `playlists` stores

/// `library()`: every track, by artist, album, title, id.
pub fn library(db: &Db) -> Result<Vec<Track>, String> {
    Ok(query(db, "library", Args::new())?.as_list().iter().map(track_of).collect())
}

/// `playlists()`: the asker's playlists, by name.
pub fn playlists(db: &Db) -> Result<Vec<Playlist>, String> {
    Ok(query(db, "playlists", Args::new())?.as_list().iter().map(playlist_of).collect())
}

/// `playlist_items(playlist_id)`: the items of one playlist, by position.
pub fn playlist_items(db: &Db, playlist_id: Id) -> Result<Vec<Item>, String> {
    let args = Args::from([("playlist_id".to_string(), Value::id(playlist_id))]);
    Ok(query(db, "playlist_items", args)?.as_list().iter().map(item_of).collect())
}

// -- writes: harken's mutators, each with its input

pub fn create_playlist(name: String) -> Call {
    Call {
        name: "create_playlist",
        scope: PLAYLISTS,
        args: Args::from([("name".to_string(), Value::text(name))]),
    }
}

fn on_playlist(playlist_id: Id, track_id: Id) -> Args {
    Args::from([
        ("playlist_id".to_string(), Value::id(playlist_id)),
        ("track_id".to_string(), Value::id(track_id)),
    ])
}

pub fn add_to_playlist(playlist_id: Id, track_id: Id) -> Call {
    Call {
        name: "add_to_playlist",
        scope: PLAYLISTS,
        args: on_playlist(playlist_id, track_id),
    }
}

/// `Some` always for harken; the screens ask, because a module may lack it.
pub fn remove_from_playlist(playlist_id: Id, track_id: Id) -> Option<Call> {
    Some(Call {
        name: "remove_from_playlist",
        scope: PLAYLISTS,
        args: on_playlist(playlist_id, track_id),
    })
}

/// The arguments of `add_track`, for authoring it by intent.
pub fn add_track_args(title: &str, artist: &str, album: Option<&str>, duration_ms: i64, file: &str) -> Args {
    Args::from([
        ("title".to_string(), Value::text(title)),
        ("artist".to_string(), Value::text(artist)),
        ("album".to_string(), Value::opt(album.map(Value::text))),
        ("duration_ms".to_string(), Value::int(duration_ms)),
        ("file".to_string(), Value::text(file)),
    ])
}
