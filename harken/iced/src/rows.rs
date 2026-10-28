//! What the domain's queries answer, as plain Rust a screen can hold.
//!
//! A query answers a list of the domain's own records (the bindings'
//! `…Entry` types, `crate::domain`), each row its bytes; a view function
//! wants plain fields it can borrow and copy, and an id it can compare. Each
//! type here is one query's row, read leniently: bytes that do not read as
//! the record — a module this build was not made with — read as empty rather
//! than as a panic in the middle of a frame.

use crate::domain::{
    AlbumsEntry, ArtistsEntry, ChaptersEntry, ComposersEntry, CreditsEntry, DownloadsEntry, EpisodeDetailsEntry, LibraryEntry, PeopleEntry,
    ProgressEntry, RecordingsEntry, ShowsEntry, SubscriptionsEntry, TrackDetailsEntry, TranscriptEntry, WorkSummary,
};
use ark::format::Encode;

/// A row's id: its sixteen bytes, whatever table it names.
pub type Id = [u8; 16];

/// One entry of the library as a list holds it (`library`, `search`,
/// `album`, `artist`, `recording`, `playlist`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Item {
    pub id: Id,
    /// `"song"`, or a show's kind (`"podcast"`, `"sermon"`) for an episode
    /// of one: which table carries the rest.
    pub kind: String,
    pub title: String,
    /// Whoever made it: a song's artist, a sermon's speaker.
    pub creator: String,
    pub duration_ms: i64,
    /// A path under the media root, or a whole URL.
    pub file: String,
    pub pos: i64,
    pub added_ms: i64,
    pub user_id: String,
    /// Where it sits on the playlist it was read against, if it is on it.
    pub playlist_pos: Option<i64>,
}

impl From<LibraryEntry> for Item {
    fn from(e: LibraryEntry) -> Item {
        Item {
            id: e.id.0,
            kind: e.kind,
            title: e.title,
            creator: e.creator,
            duration_ms: e.duration_ms,
            file: e.file,
            pos: e.pos,
            added_ms: e.added_ms,
            user_id: e.user_id,
            playlist_pos: e.playlist_pos,
        }
    }
}

impl Item {
    pub fn from_bytes(b: &[u8]) -> Item {
        LibraryEntry::from_bytes(b).map_or_else(|_| Item::default(), Item::from)
    }

    /// On the playlist it was read against.
    pub fn on_playlist(&self) -> bool {
        self.playlist_pos.is_some()
    }
}

/// A playlist of the caller's (`playlists`, `playlists_of`): the table's
/// own row.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Playlist {
    pub id: Id,
    pub name: String,
}

impl From<crate::domain::Playlist> for Playlist {
    fn from(p: crate::domain::Playlist) -> Playlist {
        Playlist { id: p.id.0, name: p.name }
    }
}

impl Playlist {
    pub fn from_bytes(b: &[u8]) -> Playlist {
        crate::domain::Playlist::from_bytes(b).map_or_else(|_| Playlist::default(), Playlist::from)
    }
}

/// A record, as the albums page lists it (`albums`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Album {
    /// Its key (`keys::album_id`): its folder and its name.
    pub id: String,
    pub name: String,
    /// Whoever made its first track.
    pub creator: String,
    pub tracks: i64,
    /// `media.file`'s spelling; empty is the derived square.
    pub art: String,
    pub label: String,
    /// Who played the most of it; empty where nobody is credited.
    pub performer: String,
    /// The year it came out; 0 is "nobody said".
    pub released: i64,
}

impl From<AlbumsEntry> for Album {
    fn from(e: AlbumsEntry) -> Album {
        Album {
            id: e.id,
            name: e.name,
            creator: e.creator,
            tracks: e.tracks,
            art: e.art,
            label: e.label,
            performer: e.performer,
            released: e.released,
        }
    }
}

impl Album {
    pub fn from_bytes(b: &[u8]) -> Album {
        AlbumsEntry::from_bytes(b).map_or_else(|_| Album::default(), Album::from)
    }
}

/// A podcast or a series of sermons, as the shows pages list them
/// (`shows`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Show {
    /// Its key: `podcast/full-time-nix`.
    pub id: String,
    /// `podcast` or `sermon`.
    pub kind: String,
    pub title: String,
    pub author: String,
    pub description: String,
    pub art: String,
    pub episodes: i64,
    /// When its newest episode went out; 0 is "nobody said".
    pub newest_ms: i64,
}

impl From<ShowsEntry> for Show {
    fn from(e: ShowsEntry) -> Show {
        Show {
            id: e.id,
            kind: e.kind,
            title: e.title,
            author: e.author,
            description: e.description,
            art: e.art,
            episodes: e.episodes,
            newest_ms: e.newest_ms,
        }
    }
}

impl Show {
    pub fn from_bytes(b: &[u8]) -> Show {
        ShowsEntry::from_bytes(b).map_or_else(|_| Show::default(), Show::from)
    }
}

/// A feed the library follows, as Settings and a show's page list them
/// (`subscriptions`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Subscription {
    pub url: String,
    /// `podcast` or `sermon`.
    pub kind: String,
    /// Its show's title; empty until the server has read it.
    pub title: String,
    /// Its show's key; empty until a read has made the show.
    pub show_id: String,
    /// How many of its episodes are in the library, and how many the feed
    /// has.
    pub episodes: i64,
    pub items: i64,
    /// The newest this many are downloaded; 0 for every one.
    pub keep: i64,
    /// Why the last read failed; empty when it did not.
    pub error: String,
}

impl From<SubscriptionsEntry> for Subscription {
    fn from(e: SubscriptionsEntry) -> Subscription {
        Subscription {
            url: e.url,
            kind: e.kind,
            title: e.title,
            show_id: e.show_id,
            episodes: e.episodes,
            items: e.items,
            keep: e.keep,
            error: e.error,
        }
    }
}

impl Subscription {
    pub fn from_bytes(b: &[u8]) -> Subscription {
        SubscriptionsEntry::from_bytes(b).map_or_else(|_| Subscription::default(), Subscription::from)
    }

    /// Whether the feed has older episodes than the library keeps of it.
    pub fn has_older(&self) -> bool {
        self.keep > 0 && self.items > self.keep
    }
}

/// A file the server has not got yet (`downloads`): what its row says
/// instead of its length.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Download {
    pub media_id: Id,
    /// `queued`, `downloading` or `failed`.
    pub state: String,
    pub bytes: i64,
    pub total: i64,
    pub error: String,
}

impl From<DownloadsEntry> for Download {
    fn from(e: DownloadsEntry) -> Download {
        Download {
            media_id: e.media_id.0,
            state: e.state,
            bytes: e.bytes,
            total: e.total,
            error: e.error,
        }
    }
}

impl Download {
    pub fn from_bytes(b: &[u8]) -> Download {
        DownloadsEntry::from_bytes(b).map_or_else(|_| Download::default(), Download::from)
    }

    /// What a row says in place of its length, short enough for the
    /// column: queued, how far it has got, or that it failed.
    pub fn short(&self) -> String {
        match self.state.as_str() {
            "downloading" if self.total > 0 => format!("{}%", (self.bytes * 100 / self.total).clamp(0, 99)),
            "downloading" => "\u{2193}".into(),
            "failed" => "failed".into(),
            _ => "queued".into(),
        }
    }

    /// What it is, said whole.
    pub fn said(&self) -> String {
        match self.state.as_str() {
            "downloading" if self.total > 0 => format!("Downloading to the server: {}", self.short()),
            "downloading" => "Downloading to the server".into(),
            "failed" if !self.error.is_empty() => format!("Could not download: {}", self.error),
            "failed" => "Could not download".into(),
            _ => "Not on the server yet".into(),
        }
    }
}

/// What a show's page says of an episode beside its row
/// (`episode_details`): what it is about, and when it went out.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EpisodeDetail {
    pub media_id: Id,
    pub description: String,
    /// 0 for "nobody said".
    pub published_ms: i64,
}

impl From<EpisodeDetailsEntry> for EpisodeDetail {
    fn from(e: EpisodeDetailsEntry) -> EpisodeDetail {
        EpisodeDetail {
            media_id: e.media_id.0,
            description: e.description,
            published_ms: e.published_ms,
        }
    }
}

impl EpisodeDetail {
    pub fn from_bytes(b: &[u8]) -> EpisodeDetail {
        EpisodeDetailsEntry::from_bytes(b).map_or_else(|_| EpisodeDetail::default(), EpisodeDetail::from)
    }
}

/// How far somebody has listened to an episode (`my_progress`,
/// `show_progress`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Listened {
    pub media_id: Id,
    pub user_id: String,
    pub name: String,
    pub position_ms: i64,
    pub duration_ms: i64,
    pub finished: bool,
}

impl From<ProgressEntry> for Listened {
    fn from(e: ProgressEntry) -> Listened {
        Listened {
            media_id: e.media_id.0,
            user_id: e.user_id,
            name: e.name,
            position_ms: e.position_ms,
            duration_ms: e.duration_ms,
            finished: e.finished,
        }
    }
}

impl Listened {
    pub fn from_bytes(b: &[u8]) -> Listened {
        ProgressEntry::from_bytes(b).map_or_else(|_| Listened::default(), Listened::from)
    }

    /// How far, short: played, or a share of it.
    pub fn short(&self) -> String {
        match (self.finished, self.duration_ms) {
            (true, _) => "played".into(),
            (false, d) if d > 0 => format!("{}%", (self.position_ms * 100 / d).clamp(0, 99)),
            _ => "started".into(),
        }
    }

    /// Where a play of it starts: a few seconds before where they stopped,
    /// so the sentence it broke off in is heard again; from the start once
    /// it is played.
    pub fn resume_ms(&self) -> i64 {
        match self.finished {
            true => 0,
            false => (self.position_ms - 3_000).max(0),
        }
    }
}

/// One chapter of an episode (`chapters`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Chapter {
    pub start_ms: i64,
    pub title: String,
}

impl From<ChaptersEntry> for Chapter {
    fn from(e: ChaptersEntry) -> Chapter {
        Chapter {
            start_ms: e.start_ms,
            title: e.title,
        }
    }
}

/// The chapter `ms` is in: the last that starts at or before it.
pub fn chapter_at(chapters: &[Chapter], ms: i64) -> Option<&Chapter> {
    chapters.iter().rev().find(|c| c.start_ms <= ms)
}

/// Somebody on an episode (`people`), in the order its feed names them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Person {
    pub name: String,
    /// As the feed says it: `Host`, `Guest`.
    pub role: String,
    /// Their picture, as a cover is spelt.
    pub art: String,
    /// Their web page; empty for none.
    pub link: String,
}

impl From<PeopleEntry> for Person {
    fn from(e: PeopleEntry) -> Person {
        Person {
            name: e.name,
            role: e.role,
            art: e.art,
            link: e.link,
        }
    }
}

impl Person {
    /// Whether they host it, rather than being on it as a guest is.
    pub fn hosts(&self) -> bool {
        self.role.eq_ignore_ascii_case("host")
    }
}

/// One line of an episode's transcript (`transcript`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Cue {
    pub start_ms: i64,
    pub end_ms: i64,
    /// Who says it; empty where the transcript does not say.
    pub speaker: String,
    pub text: String,
    /// The voice told apart that says it, which can be named; empty for none.
    pub voice: String,
}

impl From<TranscriptEntry> for Cue {
    fn from(e: TranscriptEntry) -> Cue {
        Cue {
            start_ms: e.start_ms,
            end_ms: e.end_ms,
            speaker: e.speaker,
            text: e.text,
            voice: e.voice,
        }
    }
}

/// The line being said at `ms`: the last that starts at or before it.
pub fn cue_at(cues: &[Cue], ms: i64) -> Option<usize> {
    cues.iter().rposition(|c| c.start_ms <= ms)
}

/// Whoever made something, as the artists page lists them (`artists`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Artist {
    pub name: String,
    pub tracks: i64,
    pub art: String,
    pub banner: String,
    pub born: i64,
    pub died: i64,
    pub description: String,
}

impl From<ArtistsEntry> for Artist {
    fn from(e: ArtistsEntry) -> Artist {
        Artist {
            name: e.name,
            tracks: e.tracks,
            art: e.art,
            banner: e.banner,
            born: e.born,
            died: e.died,
            description: e.description,
        }
    }
}

impl Artist {
    pub fn from_bytes(b: &[u8]) -> Artist {
        ArtistsEntry::from_bytes(b).map_or_else(|_| Artist::default(), Artist::from)
    }
}

/// Somebody some work is by (`composers`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Composer {
    pub name: String,
    pub sort_name: String,
    pub born: i64,
    pub died: i64,
    pub works: i64,
    pub tracks: i64,
    pub art: String,
    pub banner: String,
    pub description: String,
}

impl From<ComposersEntry> for Composer {
    fn from(e: ComposersEntry) -> Composer {
        Composer {
            name: e.name,
            sort_name: e.sort_name,
            born: e.born,
            died: e.died,
            works: e.works,
            tracks: e.tracks,
            art: e.art,
            banner: e.banner,
            description: e.description,
        }
    }
}

impl Composer {
    pub fn from_bytes(b: &[u8]) -> Composer {
        ComposersEntry::from_bytes(b).map_or_else(|_| Composer::default(), Composer::from)
    }
}

/// A composition (`works`, `work`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Work {
    /// The derived key, `johann-sebastian-bach/bwv-988`.
    pub id: String,
    pub title: String,
    pub catalogue: String,
    pub composer: String,
    pub form: String,
    pub period: String,
    pub recordings: i64,
    pub tracks: i64,
    pub art: String,
    pub description: String,
}

impl From<WorkSummary> for Work {
    fn from(e: WorkSummary) -> Work {
        Work {
            id: e.id,
            title: e.title,
            catalogue: e.catalogue,
            composer: e.composer,
            form: e.form,
            period: e.period,
            recordings: e.recordings,
            tracks: e.tracks,
            art: e.art,
            description: e.description,
        }
    }
}

impl Work {
    pub fn from_bytes(b: &[u8]) -> Work {
        WorkSummary::from_bytes(b).map_or_else(|_| Work::default(), Work::from)
    }
}

/// One performance of a work (`recordings`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Recording {
    /// The derived key: the work's, `@`, and who played it.
    pub id: String,
    pub performers: String,
    pub recorded: i64,
    pub label: String,
    pub licence: String,
    pub tracks: i64,
    /// How long its tracks run, together.
    pub duration_ms: i64,
    pub art: String,
}

impl From<RecordingsEntry> for Recording {
    fn from(e: RecordingsEntry) -> Recording {
        Recording {
            id: e.id,
            performers: e.performers,
            recorded: e.recorded,
            label: e.label,
            licence: e.licence,
            tracks: e.tracks,
            duration_ms: e.duration_ms,
            art: e.art,
        }
    }
}

impl Recording {
    pub fn from_bytes(b: &[u8]) -> Recording {
        RecordingsEntry::from_bytes(b).map_or_else(|_| Recording::default(), Recording::from)
    }
}

/// What is true of a track as a *song* (`track_details`): joined to the
/// kind-neutral library row in memory, by `media_id`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrackDetail {
    pub media_id: Id,
    pub album: String,
    /// That album's key, the page it opens; empty for none.
    pub album_id: String,
    /// Where it sits on the release; 0 is "nobody said".
    pub track: i64,
    /// The division of the work it belongs to.
    pub part: String,
    pub catalogue: String,
    pub performer: String,
    pub licence: String,
    pub bpm: i64,
    /// The work it is a movement of, and who wrote it; empty for none.
    pub work: String,
    pub composer: String,
    pub work_id: String,
}

impl From<TrackDetailsEntry> for TrackDetail {
    fn from(e: TrackDetailsEntry) -> TrackDetail {
        TrackDetail {
            media_id: e.media_id.0,
            album: e.album,
            album_id: e.album_id,
            track: e.track,
            part: e.part,
            catalogue: e.catalogue,
            performer: e.performer,
            licence: e.licence,
            bpm: e.bpm,
            work: e.work,
            composer: e.composer,
            work_id: e.work_id,
        }
    }
}

impl TrackDetail {
    pub fn from_bytes(b: &[u8]) -> TrackDetail {
        TrackDetailsEntry::from_bytes(b).map_or_else(|_| TrackDetail::default(), TrackDetail::from)
    }
}

/// Somebody on one recording, in what role (`credits`), billing order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Credit {
    pub name: String,
    pub role: String,
    pub instrument: String,
}

impl From<CreditsEntry> for Credit {
    fn from(e: CreditsEntry) -> Credit {
        Credit {
            name: e.name,
            role: e.role,
            instrument: e.instrument,
        }
    }
}

impl Credit {
    pub fn from_bytes(b: &[u8]) -> Credit {
        CreditsEntry::from_bytes(b).map_or_else(|_| Credit::default(), Credit::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row that does not read as its record reads as empty rather than
    /// panicking in a frame. Falsified by reading with `unwrap`: it panics.
    #[test]
    fn a_row_that_does_not_read_is_empty() {
        let a = Album::from_bytes(&[1, 2, 3]);
        assert_eq!((a.name.as_str(), a.creator.as_str(), a.tracks), ("", "", 0));
        let good = AlbumsEntry {
            art: String::new(),
            creator: "Handel".into(),
            id: "music/water-music".into(),
            label: String::new(),
            name: "Water Music".into(),
            performer: String::new(),
            released: 0,
            tracks: 3,
        };
        assert_eq!(Album::from_bytes(&good.to_bytes()).name, "Water Music");
    }
}
