//! The tables. The indexes are there for the mutations' reads.

use ark::prelude::*;

use ark::effect::Job;

use crate::types::{Looked, Present, Session};

// The library: every playable thing, what only a song has, and the rows a
// track belongs to, each keyed by what it is (`keys.rs`).

/// Every playable thing.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
#[ark(index(file), index(pos))]
pub struct Media {
    #[ark(key)]
    pub id: Id<Media>,
    pub kind: String,
    pub title: String,
    pub creator: String,
    pub duration_ms: i64,
    pub file: String,
    pub pos: i64,
    pub added_ms: i64,
    pub user_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
pub struct Album {
    #[ark(key)]
    pub name: String,
    pub label: String,
    pub released: i64,
    pub art: String,
    pub added_ms: i64,
    pub user_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
pub struct Person {
    #[ark(key)]
    pub name: String,
    pub sort_name: String,
    pub born: i64,
    pub died: i64,
    pub art: String,
    pub added_ms: i64,
    pub user_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
pub struct Work {
    #[ark(key)]
    pub id: String,
    #[ark(refs(Person))]
    pub composer: String,
    pub title: String,
    pub catalogue: String,
    pub opus: String,
    pub key_sig: String,
    pub form: String,
    pub period: String,
    pub composed: i64,
    pub art: String,
    pub added_ms: i64,
    pub user_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
pub struct Movement {
    #[ark(key)]
    pub id: String,
    #[ark(refs(Work))]
    pub work_id: String,
    pub no: i64,
    pub title: String,
    pub part: String,
    pub added_ms: i64,
    pub user_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
pub struct Recording {
    #[ark(key)]
    pub id: String,
    #[ark(refs(Work))]
    pub work_id: Option<String>,
    pub recorded: i64,
    pub venue: String,
    pub label: String,
    pub licence: String,
    pub art: String,
    pub added_ms: i64,
    pub user_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
pub struct Credit {
    #[ark(key, refs(Recording))]
    pub recording_id: String,
    #[ark(key, refs(Person))]
    pub person_name: String,
    #[ark(key)]
    pub role: String,
    pub instrument: String,
    pub pos: i64,
    pub added_ms: i64,
    pub user_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
pub struct Song {
    #[ark(key, refs(Media))]
    pub media_id: Id<Media>,
    #[ark(refs(Album))]
    pub album_name: Option<String>,
    pub disc: i64,
    pub track: i64,
    #[ark(refs(Recording))]
    pub recording_id: String,
    #[ark(refs(Movement))]
    pub movement_id: Option<String>,
    pub bpm: i64,
}

// The playlists: each one person's, its name unique among theirs.

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
// One column in two indexes reads to clippy as an attribute said twice.
#[allow(clippy::duplicated_attributes)]
#[ark(unique(user_id, name), index(pos), index(user_id, pos))]
#[cfg_attr(feature = "ffi", ark(ffi_view))]
pub struct Playlist {
    #[ark(key)]
    pub id: Id<Playlist>,
    pub name: String,
    pub pos: i64,
    pub created_ms: i64,
    pub user_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
#[ark(index(playlist_id, pos))]
pub struct PlaylistItem {
    #[ark(key, refs(Playlist))]
    pub playlist_id: Id<Playlist>,
    #[ark(key, refs(Media))]
    pub media_id: Id<Media>,
    pub pos: i64,
    pub added_ms: i64,
    pub user_id: String,
}

/// A track that was played: one row per play, carrying the track as it was
/// heard and its media id as text, which references nothing, on purpose: a
/// play is a fact about what was heard, not a hold on the library.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
#[ark(index(user_id, played_ms))]
pub struct Play {
    #[ark(key)]
    pub id: Id<Play>,
    pub user_id: String,
    pub track: String,
    pub title: String,
    pub creator: String,
    pub album: String,
    pub duration_ms: i64,
    pub file: String,
    pub played_ms: i64,
    pub from: String,
}

// The listening session, volatile: one row per account while it has a
// session (no row is `Idle`), one per device standing in it — both ordered —
// and one per device dragging a scrubber, which only its own device writes
// and nothing ordered reads.

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Row)]
#[ark(volatile)]
pub struct Listening {
    #[ark(key)]
    pub user: String,
    pub session: Session,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
#[ark(volatile)]
pub struct ListeningDevice {
    #[ark(key)]
    pub user: String,
    #[ark(key)]
    pub device: String,
    pub present: Present,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
#[ark(volatile)]
pub struct ListeningScrub {
    #[ark(key)]
    pub user: String,
    #[ark(key)]
    pub device: String,
    pub at_ms: i64,
}

/// A look the library is asked to take at its media directory (`paths`
/// under it, or the whole of it): a job (`ark::effect`) the scanner's
/// handler claims and does, done with what it read and authored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Row, Effect)]
#[effect(role = crate::LIBRARY, lease_ms = 600_000)]
pub struct Look {
    #[ark(key)]
    pub id: Id<Look>,
    pub paths: Vec<String>,
    pub state: Job<Looked>,
}

/// harken's tables, in the schema's order.
#[derive(Schema)]
pub struct Harken {
    pub media: Media,
    pub album: Album,
    pub person: Person,
    pub work: Work,
    pub movement: Movement,
    pub recording: Recording,
    pub credit: Credit,
    pub song: Song,
    pub playlist: Playlist,
    pub playlist_item: PlaylistItem,
    pub play: Play,
    pub listening: Listening,
    pub listening_device: ListeningDevice,
    pub listening_scrub: ListeningScrub,
    pub sign_in: ark_auth::SignIn,
    pub role: ark_auth::Role,
    pub look: Look,
}

ark_auth::install!(Harken);
