//! The generated domain, included from exactly one place, and the names the
//! screens use for it: harken's tables `track`, `playlist` and
//! `playlist_item`; its queries `library`, `playlists` and `playlist_items`;
//! its mutators `create_playlist`, `add_to_playlist` and
//! `remove_from_playlist`, each reached through the typed `*_args` builder
//! so that no field name is spelled here. `add_track` is the scanner's and
//! is deliberately not offered as a [`Call`]; a peer alone authors it by
//! intent through the module's own closure (`Peer::author_by_intent`).

#[rustfmt::skip]
#[path = "../../domain/gen/rust/harken_gen.rs"]
pub mod gen;

use ark::gen::{Args, Ctx, Db, Fault, Id, Value};

/// The scope the tracks live in.
pub const LIBRARY: &str = "library";
/// The scope the playlists live in.
pub const PLAYLISTS: &str = "playlists";

/// A generated mutator: `fn(db, ctx, autos, args)`.
pub type Body = fn(&mut Db, &Ctx, &Args, &Args) -> Result<(), Fault>;

/// One call of a generated mutator: which function, into which scope, with
/// which arguments, and the generated body that applies it.
pub struct Call {
    pub name: &'static str,
    pub scope: &'static str,
    pub args: Args,
    pub body: Body,
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

// -- reads: the generated queries, over the `library` and `playlists` stores

/// `library()`: every track, by artist, album, title, id.
pub fn library(db: &Db) -> Result<Vec<Track>, Fault> {
    Ok(gen::library(db, &Args::new())?.as_list().iter().map(track_of).collect())
}

/// `playlists()`: every playlist, by name.
pub fn playlists(db: &Db) -> Result<Vec<Playlist>, Fault> {
    Ok(gen::playlists(db, &Args::new())?.as_list().iter().map(playlist_of).collect())
}

/// `playlist_items(playlist_id)`: the items of one playlist, by position.
pub fn playlist_items(db: &Db, playlist_id: Id) -> Result<Vec<Item>, Fault> {
    let args = Args::from([("playlist_id".to_string(), Value::id(playlist_id))]);
    Ok(gen::playlist_items(db, &args)?.as_list().iter().map(item_of).collect())
}

// -- writes: the generated mutators, each with its typed argument builder

pub fn create_playlist(name: String) -> Call {
    Call {
        name: "create_playlist",
        scope: PLAYLISTS,
        args: gen::create_playlist_args(name),
        body: gen::create_playlist,
    }
}

pub fn add_to_playlist(playlist_id: Id, track_id: Id) -> Call {
    Call {
        name: "add_to_playlist",
        scope: PLAYLISTS,
        args: gen::add_to_playlist_args(playlist_id, track_id),
        body: gen::add_to_playlist,
    }
}

/// `Some` always for harken; the screens ask, because a module may lack it.
pub fn remove_from_playlist(playlist_id: Id, track_id: Id) -> Option<Call> {
    Some(Call {
        name: "remove_from_playlist",
        scope: PLAYLISTS,
        args: gen::remove_from_playlist_args(playlist_id, track_id),
        body: gen::remove_from_playlist,
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
