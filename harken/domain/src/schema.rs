//! The tables. This file is the schema: the table set is a struct of
//! tables, a table is a row struct with its columns, key, indexes and
//! references said once in `Row::columns`.
//!
//! harken's `schema.sql`, table for table and column for column, one log
//! and one set of tables. The unique index on `(playlist.user_id,
//! playlist.name)` is the one addition: it is what `create_playlist`'s
//! insert matches on, so a person's second "Favorites" is no row at all.
use ark::authoring::*;

/// Every table harken has, in one set: the library (what the scanner
/// authors and every peer reads) and the playlists (what people make of it).
pub struct Harken {
    pub media: Table<Media>,
    pub album: Table<Album>,
    pub person: Table<Person>,
    pub work: Table<Work>,
    pub movement: Table<Movement>,
    pub recording: Table<Recording>,
    pub credit: Table<Credit>,
    pub song: Table<Song>,
    pub playlist: Table<Playlist>,
    pub playlist_item: Table<PlaylistItem>,
}
impl Tables for Harken {
    fn open() -> Self {
        Harken {
            media: table(),
            album: table(),
            person: table(),
            work: table(),
            movement: table(),
            recording: table(),
            credit: table(),
            song: table(),
            playlist: table(),
            playlist_item: table(),
        }
    }
}

pub struct Media {
    pub id: Id<Media>,
    pub kind: Text,
    pub title: Text,
    pub creator: Text,
    pub duration_ms: Int,
    pub file: Text,
    pub pos: Int,
    pub added_ms: Int,
    pub user_id: Text,
}
impl Row for Media {
    const NAME: &str = "media";
    type Key = (Id<Media>,);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::id)
            .text(Self::kind)
            .text(Self::title)
            .text(Self::creator)
            .int(Self::duration_ms)
            .text(Self::file)
            .int(Self::pos)
            .int(Self::added_ms)
            .text(Self::user_id)
            .key((Self::id,))
    }
}
impl Media {
    pub const id: Col<Self, Id<Self>> = col("id");
    pub const kind: Col<Self, Text> = col("kind");
    pub const title: Col<Self, Text> = col("title");
    pub const creator: Col<Self, Text> = col("creator");
    pub const duration_ms: Col<Self, Int> = col("duration_ms");
    pub const file: Col<Self, Text> = col("file");
    pub const pos: Col<Self, Int> = col("pos");
    pub const added_ms: Col<Self, Int> = col("added_ms");
    pub const user_id: Col<Self, Text> = col("user_id");
    pub const song: Rel<Self, Song> = rel("song");
    pub const playlist_item: Rel<Self, PlaylistItem> = rel("playlist_item");
}

pub struct Album {
    pub name: Text,
    pub label: Text,
    pub released: Int,
    pub art: Text,
    pub added_ms: Int,
    pub user_id: Text,
}
impl Row for Album {
    const NAME: &str = "album";
    type Key = (Text,);
    fn columns() -> Columns<Self> {
        columns()
            .text(Self::name)
            .text(Self::label)
            .int(Self::released)
            .text(Self::art)
            .int(Self::added_ms)
            .text(Self::user_id)
            .key((Self::name,))
    }
}
impl Album {
    pub const name: Col<Self, Text> = col("name");
    pub const label: Col<Self, Text> = col("label");
    pub const released: Col<Self, Int> = col("released");
    pub const art: Col<Self, Text> = col("art");
    pub const added_ms: Col<Self, Int> = col("added_ms");
    pub const user_id: Col<Self, Text> = col("user_id");
    pub const song: Rel<Self, Song> = rel("song");
}

pub struct Person {
    pub name: Text,
    pub sort_name: Text,
    pub born: Int,
    pub died: Int,
    pub art: Text,
    pub added_ms: Int,
    pub user_id: Text,
}
impl Row for Person {
    const NAME: &str = "person";
    type Key = (Text,);
    fn columns() -> Columns<Self> {
        columns()
            .text(Self::name)
            .text(Self::sort_name)
            .int(Self::born)
            .int(Self::died)
            .text(Self::art)
            .int(Self::added_ms)
            .text(Self::user_id)
            .key((Self::name,))
    }
}
impl Person {
    pub const name: Col<Self, Text> = col("name");
    pub const sort_name: Col<Self, Text> = col("sort_name");
    pub const born: Col<Self, Int> = col("born");
    pub const died: Col<Self, Int> = col("died");
    pub const art: Col<Self, Text> = col("art");
    pub const added_ms: Col<Self, Int> = col("added_ms");
    pub const user_id: Col<Self, Text> = col("user_id");
    pub const work: Rel<Self, Work> = rel("work");
    pub const credit: Rel<Self, Credit> = rel("credit");
}

pub struct Work {
    pub id: Text,
    pub composer: Text,
    pub title: Text,
    pub catalogue: Text,
    pub opus: Text,
    pub key_sig: Text,
    pub form: Text,
    pub period: Text,
    pub composed: Int,
    pub art: Text,
    pub added_ms: Int,
    pub user_id: Text,
}
impl Row for Work {
    const NAME: &str = "work";
    type Key = (Text,);
    fn columns() -> Columns<Self> {
        columns()
            .text(Self::id)
            .text(Self::composer)
            .refs::<Person>()
            .text(Self::title)
            .text(Self::catalogue)
            .text(Self::opus)
            .text(Self::key_sig)
            .text(Self::form)
            .text(Self::period)
            .int(Self::composed)
            .text(Self::art)
            .int(Self::added_ms)
            .text(Self::user_id)
            .key((Self::id,))
    }
}
impl Work {
    pub const id: Col<Self, Text> = col("id");
    pub const composer: Col<Self, Text> = col("composer");
    pub const title: Col<Self, Text> = col("title");
    pub const catalogue: Col<Self, Text> = col("catalogue");
    pub const opus: Col<Self, Text> = col("opus");
    pub const key_sig: Col<Self, Text> = col("key_sig");
    pub const form: Col<Self, Text> = col("form");
    pub const period: Col<Self, Text> = col("period");
    pub const composed: Col<Self, Int> = col("composed");
    pub const art: Col<Self, Text> = col("art");
    pub const added_ms: Col<Self, Int> = col("added_ms");
    pub const user_id: Col<Self, Text> = col("user_id");
    pub const movement: Rel<Self, Movement> = rel("movement");
    pub const recording: Rel<Self, Recording> = rel("recording");
}

pub struct Movement {
    pub id: Text,
    pub work_id: Text,
    pub no: Int,
    pub title: Text,
    pub part: Text,
    pub added_ms: Int,
    pub user_id: Text,
}
impl Row for Movement {
    const NAME: &str = "movement";
    type Key = (Text,);
    fn columns() -> Columns<Self> {
        columns()
            .text(Self::id)
            .text(Self::work_id)
            .refs::<Work>()
            .int(Self::no)
            .text(Self::title)
            .text(Self::part)
            .int(Self::added_ms)
            .text(Self::user_id)
            .key((Self::id,))
    }
}
impl Movement {
    pub const id: Col<Self, Text> = col("id");
    pub const work_id: Col<Self, Text> = col("work_id");
    pub const no: Col<Self, Int> = col("no");
    pub const title: Col<Self, Text> = col("title");
    pub const part: Col<Self, Text> = col("part");
    pub const added_ms: Col<Self, Int> = col("added_ms");
    pub const user_id: Col<Self, Text> = col("user_id");
    pub const song: Rel<Self, Song> = rel("song");
}

pub struct Recording {
    pub id: Text,
    pub work_id: Opt<Text>,
    pub recorded: Int,
    pub venue: Text,
    pub label: Text,
    pub licence: Text,
    pub art: Text,
    pub added_ms: Int,
    pub user_id: Text,
}
impl Row for Recording {
    const NAME: &str = "recording";
    type Key = (Text,);
    fn columns() -> Columns<Self> {
        columns()
            .text(Self::id)
            .text(Self::work_id)
            .nullable()
            .refs::<Work>()
            .int(Self::recorded)
            .text(Self::venue)
            .text(Self::label)
            .text(Self::licence)
            .text(Self::art)
            .int(Self::added_ms)
            .text(Self::user_id)
            .key((Self::id,))
    }
}
impl Recording {
    pub const id: Col<Self, Text> = col("id");
    pub const work_id: Col<Self, Opt<Text>> = col("work_id");
    pub const recorded: Col<Self, Int> = col("recorded");
    pub const venue: Col<Self, Text> = col("venue");
    pub const label: Col<Self, Text> = col("label");
    pub const licence: Col<Self, Text> = col("licence");
    pub const art: Col<Self, Text> = col("art");
    pub const added_ms: Col<Self, Int> = col("added_ms");
    pub const user_id: Col<Self, Text> = col("user_id");
    pub const credit: Rel<Self, Credit> = rel("credit");
    pub const song: Rel<Self, Song> = rel("song");
}

pub struct Credit {
    pub recording_id: Text,
    pub person_name: Text,
    pub role: Text,
    pub instrument: Text,
    pub pos: Int,
    pub added_ms: Int,
    pub user_id: Text,
}
impl Row for Credit {
    const NAME: &str = "credit";
    type Key = (Text, Text, Text);
    fn columns() -> Columns<Self> {
        columns()
            .text(Self::recording_id)
            .refs::<Recording>()
            .text(Self::person_name)
            .refs::<Person>()
            .text(Self::role)
            .text(Self::instrument)
            .int(Self::pos)
            .int(Self::added_ms)
            .text(Self::user_id)
            .key((Self::recording_id, Self::person_name, Self::role))
    }
}
impl Credit {
    pub const recording_id: Col<Self, Text> = col("recording_id");
    pub const person_name: Col<Self, Text> = col("person_name");
    pub const role: Col<Self, Text> = col("role");
    pub const instrument: Col<Self, Text> = col("instrument");
    pub const pos: Col<Self, Int> = col("pos");
    pub const added_ms: Col<Self, Int> = col("added_ms");
    pub const user_id: Col<Self, Text> = col("user_id");
}

pub struct Song {
    pub media_id: Id<Media>,
    pub album_name: Opt<Text>,
    pub disc: Int,
    pub track: Int,
    pub recording_id: Text,
    pub movement_id: Opt<Text>,
    pub bpm: Int,
}
impl Row for Song {
    const NAME: &str = "song";
    type Key = (Id<Media>,);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::media_id)
            .refs::<Media>()
            .text(Self::album_name)
            .nullable()
            .refs::<Album>()
            .int(Self::disc)
            .int(Self::track)
            .text(Self::recording_id)
            .refs::<Recording>()
            .text(Self::movement_id)
            .nullable()
            .refs::<Movement>()
            .int(Self::bpm)
            .key((Self::media_id,))
    }
}
impl Song {
    pub const media_id: Col<Self, Id<Media>> = col("media_id");
    pub const album_name: Col<Self, Opt<Text>> = col("album_name");
    pub const disc: Col<Self, Int> = col("disc");
    pub const track: Col<Self, Int> = col("track");
    pub const recording_id: Col<Self, Text> = col("recording_id");
    pub const movement_id: Col<Self, Opt<Text>> = col("movement_id");
    pub const bpm: Col<Self, Int> = col("bpm");
}

pub struct Playlist {
    pub id: Id<Playlist>,
    pub name: Text,
    pub pos: Int,
    pub created_ms: Int,
    pub user_id: Text,
}
impl Row for Playlist {
    const NAME: &str = "playlist";
    type Key = (Id<Playlist>,);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::id)
            .text(Self::name)
            .int(Self::pos)
            .int(Self::created_ms)
            .text(Self::user_id)
            .key((Self::id,))
            .unique((Self::user_id, Self::name))
    }
}
impl Playlist {
    pub const id: Col<Self, Id<Self>> = col("id");
    pub const name: Col<Self, Text> = col("name");
    pub const pos: Col<Self, Int> = col("pos");
    pub const created_ms: Col<Self, Int> = col("created_ms");
    pub const user_id: Col<Self, Text> = col("user_id");
    pub const playlist_item: Rel<Self, PlaylistItem> = rel("playlist_item");
}

pub struct PlaylistItem {
    pub playlist_id: Id<Playlist>,
    pub media_id: Id<Media>,
    pub pos: Int,
    pub added_ms: Int,
    pub user_id: Text,
}
impl Row for PlaylistItem {
    const NAME: &str = "playlist_item";
    type Key = (Id<Playlist>, Id<Media>);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::playlist_id)
            .refs::<Playlist>()
            .id(Self::media_id)
            .refs::<Media>()
            .int(Self::pos)
            .int(Self::added_ms)
            .text(Self::user_id)
            .key((Self::playlist_id, Self::media_id))
    }
}
impl PlaylistItem {
    pub const playlist_id: Col<Self, Id<Playlist>> = col("playlist_id");
    pub const media_id: Col<Self, Id<Media>> = col("media_id");
    pub const pos: Col<Self, Int> = col("pos");
    pub const added_ms: Col<Self, Int> = col("added_ms");
    pub const user_id: Col<Self, Text> = col("user_id");
}
