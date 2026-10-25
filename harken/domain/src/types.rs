//! The types: every record a function takes or answers, and the session's
//! state. A refinement (`#[ark(..)]`) is checked once, where bytes come in,
//! and its words are what a form shows.

use ark::prelude::*;

use crate::schema::{Media, Playlist};

// * The library's inputs

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct AddSong {
    #[ark(trim, non_empty, why = "a song needs a title")]
    pub title: String,
    #[ark(trim)]
    pub artist: String,
    #[ark(trim)]
    pub album: String,
    pub duration_ms: i64,
    #[ark(trim)]
    pub file: String,
    pub track: i64,
    #[ark(trim)]
    pub part: String,
    #[ark(trim)]
    pub catalogue: String,
    #[ark(trim)]
    pub performer: String,
    pub bpm: i64,
    #[ark(trim)]
    pub album_art: String,
    #[ark(trim)]
    pub artist_art: String,
    pub disc: i64,
    #[ark(trim)]
    pub work_title: String,
    pub movement_no: i64,
    /// The wide picture beside the artist's (`backdrop.*`), for their page.
    #[ark(trim)]
    pub artist_banner: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct DescribeWork {
    pub id: String,
    #[ark(trim)]
    pub opus: String,
    #[ark(trim)]
    pub key_sig: String,
    #[ark(trim)]
    pub form: String,
    #[ark(trim)]
    pub period: String,
    pub composed: i64,
    #[ark(trim)]
    pub art: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct DescribeRecording {
    pub id: String,
    pub recorded: i64,
    #[ark(trim)]
    pub venue: String,
    #[ark(trim)]
    pub label: String,
    #[ark(trim)]
    pub licence: String,
    #[ark(trim)]
    pub art: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct DescribePerson {
    #[ark(trim, non_empty, why = "a person needs a name")]
    pub name: String,
    #[ark(trim)]
    pub sort_name: String,
    pub born: i64,
    pub died: i64,
    #[ark(trim)]
    pub art: String,
    #[ark(trim)]
    pub banner: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct CreditRecording {
    pub recording_id: String,
    #[ark(trim, non_empty, why = "a credit needs somebody to credit")]
    pub person_name: String,
    #[ark(trim)]
    pub role: String,
    #[ark(trim)]
    pub instrument: String,
    pub pos: i64,
}

/// A look at the media directory asked for: these paths under it, or the
/// whole of it when there are none.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct LookAt {
    pub paths: Vec<String>,
}

/// What a look came to: the files whose tags it read, the entries it
/// authored.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize, Describe)]
pub struct Looked {
    pub read: i64,
    pub authored: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct RemoveMedia {
    pub id: Id<Media>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Library {
    pub playlist_id: Id<Playlist>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Search {
    pub playlist_id: Id<Playlist>,
    pub needle: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct AlbumInput {
    pub playlist_id: Id<Playlist>,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Artist {
    pub playlist_id: Id<Playlist>,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Works {
    pub composer: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct WorkInput {
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Recordings {
    pub work_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Credits {
    pub recording_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct RecordingInput {
    pub playlist_id: Id<Playlist>,
    pub id: String,
}

// * What the library's queries answer

/// Something playable as a list renders it, and where it sits on the
/// playlist the list was read against, if it is on it.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
#[cfg_attr(feature = "ffi", ark(ffi_view))]
pub struct LibraryEntry {
    pub added_ms: i64,
    pub creator: String,
    pub duration_ms: i64,
    pub file: String,
    pub id: Id<Media>,
    pub kind: String,
    pub playlist_pos: Option<i64>,
    pub pos: i64,
    pub title: String,
    pub user_id: String,
}

/// A record, as the albums page lists it.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
pub struct AlbumsEntry {
    pub art: String,
    pub creator: String,
    /// Who put it out, empty where nobody said.
    pub label: String,
    pub name: String,
    /// Who played the most of its tracks, as one line ("Trevor Pinnock, The
    /// English Concert"); empty where nobody is credited.
    pub performer: String,
    /// The year it came out; 0 is "nobody said".
    pub released: i64,
    pub tracks: i64,
}

/// Whoever made something, as the artists page lists them.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
pub struct ArtistsEntry {
    pub art: String,
    pub banner: String,
    /// The years they lived, where `person` has them: 0 for unknown.
    pub born: i64,
    pub died: i64,
    pub name: String,
    pub tracks: i64,
}

/// What is true of a track as a song.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
pub struct TrackDetailsEntry {
    pub album: String,
    pub bpm: i64,
    pub catalogue: String,
    /// Whoever the work it is a movement of is by; empty for none.
    pub composer: String,
    pub licence: String,
    pub media_id: Id<Media>,
    pub part: String,
    pub performer: String,
    pub track: i64,
    /// The title of the work it is a movement of; empty for none.
    pub work: String,
}

/// Somebody some work is by, as the composers index lists them.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
pub struct ComposersEntry {
    pub art: String,
    pub banner: String,
    pub born: i64,
    pub died: i64,
    pub name: String,
    pub sort_name: String,
    pub tracks: i64,
    pub works: i64,
}

/// A composition, as a composer's page lists them and a work's page heads.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
pub struct WorkSummary {
    pub art: String,
    pub catalogue: String,
    pub composer: String,
    pub form: String,
    pub id: String,
    pub period: String,
    pub recordings: i64,
    pub title: String,
    pub tracks: i64,
}

/// One performance, as a work's page lists them.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
pub struct RecordingsEntry {
    pub art: String,
    /// How long its tracks run, together.
    pub duration_ms: i64,
    pub id: String,
    pub label: String,
    pub licence: String,
    pub performers: String,
    pub recorded: i64,
    pub tracks: i64,
}

/// One person on a recording, and what they did on it.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
pub struct CreditsEntry {
    pub instrument: String,
    pub name: String,
    pub pos: i64,
    pub role: String,
}

// * The playlists' inputs

/// A playlist's name: trimmed, and something.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct CreatePlaylist {
    #[ark(trim, non_empty, why = "a playlist needs a name")]
    #[ark(len = "0..=120")]
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct AddToPlaylist {
    pub playlist_id: Id<Playlist>,
    pub media_id: Id<Media>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct AddAllToPlaylist {
    pub playlist_id: Id<Playlist>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct RemoveFromPlaylist {
    pub playlist_id: Id<Playlist>,
    pub media_id: Id<Media>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct PlaylistsOf {
    pub media_id: Id<Media>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct PlaylistInput {
    pub playlist_id: Id<Playlist>,
}

// * The listening session

/// Something queued, or nothing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Describe)]
pub enum Session {
    Active { active: Active },
    Idle,
}

/// A queue, where in it, whether it is playing, and on which device. `at`
/// is always inside the queue, and the queue is never empty: every
/// transition that could empty it goes `Idle`. `from` is where the queue was
/// taken from, as the client that pressed it names it (`album/Water Music`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Describe)]
pub struct Active {
    pub at: i64,
    pub clock: Clock,
    pub from: String,
    pub output: Output,
    pub queue: Vec<Track>,
}

/// How far into the track: paused at a point, or playing from a point since
/// a moment of the server's clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Describe)]
pub enum Clock {
    Paused { at_ms: i64 },
    Playing { at_ms: i64, since: i64 },
}

/// Which device the sound belongs to: one that has gone and keeps it (its
/// name kept beside its id, for the picker), one that is here (and, until it
/// says it has started, `moving`), or none.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Describe)]
pub enum Output {
    Away { id: String, name: String },
    Device { id: String, moving: bool },
    Nowhere,
}

/// A track, carried rather than looked up: everything needed to play it.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
pub struct Track {
    pub album: String,
    pub creator: String,
    pub duration_ms: i64,
    pub file: String,
    pub id: Id<Media>,
    pub title: String,
}

/// A device standing in the session: what it said it is when it came, what
/// it can do, and where its volume (0 to 100) and its group are.
/// `can_volume` is what it said of itself (`equip`); `family`, which
/// speakers it can be joined with, none for anything that is not a speaker
/// that groups; `follows`, the leader whose playback it follows while it is
/// joined to one.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
pub struct Present {
    pub can_play: bool,
    pub can_volume: bool,
    pub family: Option<String>,
    pub follows: Option<String>,
    pub id: String,
    pub kind: String,
    pub muted: bool,
    pub name: String,
    pub user: String,
    pub volume: i64,
}

/// A device as it introduces itself: what `on_join` and `on_leave` are told.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Device {
    pub can_play: bool,
    pub id: String,
    pub kind: String,
    pub name: String,
    pub user: String,
}

/// A group's volumes, counted and added.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Describe)]
pub struct Tally {
    pub n: i64,
    pub sum: i64,
}

// * The listening session's inputs

/// A queue to play: never empty.
pub type Queue = Vec<Track>;

/// Milliseconds from 0.
pub type Ms = i64;

/// A speaker family's name: at least one character.
pub type Family = String;

fn at_least_0(n: &i64) -> bool {
    *n >= 0
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Start {
    #[ark(check = "at_least_0")]
    pub at: i64,
    pub from: String,
    #[ark(check = "at_least_0")]
    pub position: i64,
    #[ark(non_empty)]
    pub queue: Queue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Seek {
    #[ark(check = "at_least_0")]
    pub ms: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Skip {
    #[ark(range = "-1..=1", why = "a skip is one track either way")]
    #[ark(check = "|n: &i64| *n != 0", why = "a skip is one track either way")]
    pub by: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Jump {
    #[ark(check = "at_least_0")]
    pub index: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Transfer {
    pub to: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct At {
    #[ark(check = "at_least_0")]
    pub at_ms: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Report {
    #[ark(check = "at_least_0")]
    pub at_ms: i64,
    pub since: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Advance {
    pub from_track: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Enqueue {
    pub next: bool,
    #[ark(non_empty)]
    pub tracks: Queue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Remove {
    #[ark(check = "at_least_0")]
    pub index: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Move {
    #[ark(check = "at_least_0")]
    pub from: i64,
    #[ark(check = "at_least_0")]
    pub to: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Scrub {
    #[ark(check = "|at: &Option<Ms>| at.is_none_or(|n| n >= 0)")]
    pub at_ms: Option<Ms>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Equip {
    pub can_volume: bool,
    #[ark(check = "|f: &Option<Family>| f.as_ref().is_none_or(|f| !f.is_empty())", why = "at least 1 character")]
    pub family: Option<Family>,
    pub muted: bool,
    #[ark(range = "0..=100", why = "between 0 and 100")]
    pub volume: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct SetVolume {
    pub device: String,
    #[ark(range = "0..=100", why = "between 0 and 100")]
    pub level: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct NudgeVolume {
    #[ark(range = "-100..=100", why = "between -100 and 100")]
    #[ark(check = "|n: &i64| *n != 0", why = "a nudge moves the volume")]
    pub delta: i64,
    pub device: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct SetMute {
    pub device: String,
    pub muted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct ReportVolume {
    #[ark(range = "0..=100", why = "between 0 and 100")]
    pub level: i64,
    pub muted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct Member {
    pub device: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct ReportGroup {
    pub members: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct GroupVolume {
    #[ark(range = "0..=100", why = "between 0 and 100")]
    pub level: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct GroupNudge {
    #[ark(range = "-100..=100", why = "between -100 and 100")]
    #[ark(check = "|n: &i64| *n != 0", why = "a nudge moves the volume")]
    pub delta: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct GroupMute {
    pub muted: bool,
}

/// A track that was played, as the device that made the sound says it.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Input)]
pub struct RecordPlay {
    pub track: Id<Media>,
    pub title: String,
    pub creator: String,
    pub album: String,
    #[ark(check = "at_least_0")]
    pub duration_ms: i64,
    pub file: String,
    pub from: String,
}
