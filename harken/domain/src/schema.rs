//! The tables (`harken/domain/src/schema.rs`, table for table and column
//! for column): a table is a row struct, its key, indexes and references
//! said once in `#[ark(..)]`, its columns read by name. The indexes are
//! v4's, there for the mutations' reads; v4's two text indexes on
//! `media.title` and `media.creator` (`search`'s trigrams) have no v5
//! counterpart and are left out, which changes no answer.
//!
//! Beside the log's tables are the listening session's (§1.9), volatile:
//! one row per account (`listening`), one per device standing in it
//! (`listening_device`) — both ordered — and one per device dragging a
//! scrubber (`listening_scrub`), ephemeral: the two kinds of state v4's
//! live record kept as `field` and `ephemeral`, as two tables.

use ark::author::{Id, Input, Int, Node, Row, Schema, Sum, Table, Text};

use crate::listening::{Present, Session};

/// The library's role: the scanner's account holds it, and nobody else
/// unless the server's configuration grants it. Every library mutation is
/// guarded by it (`library::is_library`).
pub const LIBRARY: &str = "library";

/// Every table harken has, in one set, in schema order.
#[derive(Schema)]
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
    pub play: Table<Play>,
    pub listening: Table<Listening>,
    pub listening_device: Table<ListeningDevice>,
    pub listening_scrub: Table<ListeningScrub>,
}

#[derive(Row, Node, Clone, Debug, PartialEq)]
#[ark(index(file))]
#[ark(index(pos))]
pub struct Media {
    #[ark(key)]
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

#[derive(Row, Node, Clone, Debug, PartialEq)]
pub struct Album {
    #[ark(key)]
    pub name: Text,
    pub label: Text,
    pub released: Int,
    pub art: Text,
    pub added_ms: Int,
    pub user_id: Text,
}

#[derive(Row, Node, Clone, Debug, PartialEq)]
pub struct Person {
    #[ark(key)]
    pub name: Text,
    pub sort_name: Text,
    pub born: Int,
    pub died: Int,
    pub art: Text,
    pub added_ms: Int,
    pub user_id: Text,
}

#[derive(Row, Node, Clone, Debug, PartialEq)]
pub struct Work {
    #[ark(key)]
    pub id: Text,
    #[ark(refs = Person)]
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

#[derive(Row, Node, Clone, Debug, PartialEq)]
pub struct Movement {
    #[ark(key)]
    pub id: Text,
    #[ark(refs = Work)]
    pub work_id: Text,
    pub no: Int,
    pub title: Text,
    pub part: Text,
    pub added_ms: Int,
    pub user_id: Text,
}

#[derive(Row, Node, Clone, Debug, PartialEq)]
pub struct Recording {
    #[ark(key)]
    pub id: Text,
    #[ark(refs = Work)]
    pub work_id: Option<Text>,
    pub recorded: Int,
    pub venue: Text,
    pub label: Text,
    pub licence: Text,
    pub art: Text,
    pub added_ms: Int,
    pub user_id: Text,
}

#[derive(Row, Node, Clone, Debug, PartialEq)]
pub struct Credit {
    #[ark(key, refs = Recording)]
    pub recording_id: Text,
    #[ark(key, refs = Person)]
    pub person_name: Text,
    #[ark(key)]
    pub role: Text,
    pub instrument: Text,
    pub pos: Int,
    pub added_ms: Int,
    pub user_id: Text,
}

#[derive(Row, Node, Clone, Debug, PartialEq)]
pub struct Song {
    #[ark(key, refs = Media)]
    pub media_id: Id<Media>,
    #[ark(refs = Album)]
    pub album_name: Option<Text>,
    pub disc: Int,
    pub track: Int,
    #[ark(refs = Recording)]
    pub recording_id: Text,
    #[ark(refs = Movement)]
    pub movement_id: Option<Text>,
    pub bpm: Int,
}

#[derive(Row, Node, Clone, Debug, PartialEq)]
#[ark(unique(user_id, name))]
#[ark(index(pos))]
#[allow(clippy::duplicated_attributes)] // two indexes may share a column
#[ark(index(user_id, pos))]
pub struct Playlist {
    #[ark(key)]
    pub id: Id<Playlist>,
    pub name: Text,
    pub pos: Int,
    pub created_ms: Int,
    pub user_id: Text,
}

#[derive(Row, Node, Clone, Debug, PartialEq)]
#[ark(index(playlist_id, pos))]
pub struct PlaylistItem {
    #[ark(key, refs = Playlist)]
    pub playlist_id: Id<Playlist>,
    #[ark(key, refs = Media)]
    pub media_id: Id<Media>,
    pub pos: Int,
    pub added_ms: Int,
    pub user_id: Text,
}

/// A track that was played: one row per play, authored by the device that
/// made the sound once it had played long enough to count
/// (`listening::record_play`). It carries the track as it was heard and
/// the media id as text, which references nothing, on purpose: a play is a
/// fact about what was heard, not a hold on the library.
#[derive(Row, Node, Clone, Debug, PartialEq)]
#[ark(index(user_id, played_ms))]
pub struct Play {
    #[ark(key)]
    pub id: Id<Play>,
    pub user_id: Text,
    pub track: Text,
    pub title: Text,
    pub creator: Text,
    pub album: Text,
    pub duration_ms: Int,
    pub file: Text,
    pub played_ms: Int,
    pub from: Text,
}

/// One account's listening session, while there is one: v4's
/// `scope(|ctx| ctx.user)` is the key. No row is `Idle`.
#[derive(Row, Node, Clone, Debug, PartialEq)]
#[ark(volatile)]
pub struct Listening {
    #[ark(key)]
    pub user: Text,
    pub session: Session,
}

/// A device standing in an account's session (v4's `devices`, ordered).
#[derive(Row, Node, Clone, Debug, PartialEq)]
#[ark(volatile)]
pub struct ListeningDevice {
    #[ark(key)]
    pub user: Text,
    #[ark(key)]
    pub device: Text,
    pub present: Present,
}

/// Where a device's scrubber is being dragged to, in milliseconds into the
/// current track (v4's `scrub`, ephemeral): only its own device writes it,
/// and nothing ordered reads it.
#[derive(Row, Node, Clone, Debug, PartialEq)]
#[ark(volatile)]
pub struct ListeningScrub {
    #[ark(key)]
    pub user: Text,
    #[ark(key)]
    pub device: Text,
    pub at_ms: Int,
}

#[allow(unused_imports)]
use {Input as _, Sum as _};
