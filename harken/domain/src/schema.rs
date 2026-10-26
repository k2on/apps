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
    /// A wide picture to open their page with, where `art` is the square.
    /// Defaulted: a row written before it was added has none, and without
    /// a default it cannot be read at all.
    #[serde(default)]
    pub banner: String,
    /// A few sentences about them, for their page. Defaulted, as `banner`.
    #[serde(default)]
    pub description: String,
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
    /// A few sentences about it, for its page. Defaulted, as
    /// `Person::banner`: a row written before it was added has none.
    #[serde(default)]
    pub description: String,
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

// The shows: a podcast or a series of sermons, and what only an episode of
// one has. An episode is a `media` row like a song — its kind the show's —
// so a playlist, the queue, a play and `/media` take it as they take a song.

/// A podcast or a series of sermons: a feed the server follows, or a
/// folder of episodes. Keyed by its kind and its title (`keys::show_key`).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
pub struct Show {
    #[ark(key)]
    pub id: String,
    /// `podcast` or `sermon`.
    pub kind: String,
    pub title: String,
    /// Who makes it: a host, a preacher, a church.
    pub author: String,
    pub description: String,
    pub art: String,
    /// The RSS feed it is read from; empty for a folder of files.
    pub feed: String,
    /// Its own page on the web.
    pub link: String,
    pub added_ms: i64,
    pub user_id: String,
}

/// What only an episode has.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
#[ark(index(show_id))]
pub struct Episode {
    #[ark(key, refs(Media))]
    pub media_id: Id<Media>,
    #[ark(refs(Show))]
    pub show_id: String,
    /// The feed's id for it, where it came from one.
    pub guid: String,
    /// When it went out: 0 for "nobody said".
    pub published_ms: i64,
    pub description: String,
    /// Where it was downloaded from; empty for a file that was always here.
    pub source: String,
    /// Its own picture, where it has one other than its show's.
    pub art: String,
    pub season: i64,
    pub number: i64,
}

/// A feed the server follows, as somebody added it from a client: a
/// podcast's or a series of sermons'. The server reads it at every full
/// look, every hour and whenever this row moves, downloads its newest
/// `keep` episodes, and says here what it read (`library.feed_read`). Its
/// show is the `show` whose `feed` is this URL, once a read has made it.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
pub struct Subscription {
    #[ark(key)]
    pub url: String,
    /// `podcast` or `sermon`.
    pub kind: String,
    /// How many of the feed's newest episodes are downloaded; 0 for every
    /// one.
    pub keep: i64,
    /// The show's title as the last read had it; empty until a read.
    pub title: String,
    /// How many episodes the feed has, as the last read had it.
    pub items: i64,
    /// Why the last read failed; empty when it did not.
    pub error: String,
    pub added_ms: i64,
    pub user_id: String,
}

/// A file the library has a row for and the server has not got yet: its
/// media row's `file`, to be downloaded from `source` — an episode of a
/// followed feed, a recording of the catalogue. Said as it goes (queued,
/// downloading and how far, failed and why), and gone once the file is
/// here: a media row with no download is on the server.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Row)]
pub struct Download {
    #[ark(key, refs(Media))]
    pub media_id: Id<Media>,
    /// Where it is downloaded from.
    pub source: String,
    /// `queued`, `downloading` or `failed`.
    pub state: String,
    /// How much of it is here, and how much there is: 0 for "not said".
    pub bytes: i64,
    pub total: i64,
    /// Why it failed; empty when it did not.
    pub error: String,
    pub asked_ms: i64,
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
    pub show: Show,
    pub episode: Episode,
    pub subscription: Subscription,
    pub download: Download,
}

ark_auth::install!(Harken);
