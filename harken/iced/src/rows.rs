//! What the domain's queries answer, as plain Rust a screen can hold.
//!
//! A query answers a list of the domain's own records (the bindings'
//! `…Entry` types, `crate::domain`), each row its bytes; a view function
//! wants plain fields it can borrow and copy, and an id it can compare. Each
//! type here is one query's row, read leniently: bytes that do not read as
//! the record — a module this build was not made with — read as empty rather
//! than as a panic in the middle of a frame.

use crate::domain::{AlbumsEntry, ArtistsEntry, ComposersEntry, CreditsEntry, LibraryEntry, RecordingsEntry, TrackDetailsEntry, WorkSummary};
use ark::format::Encode;

/// A row's id: its sixteen bytes, whatever table it names.
pub type Id = [u8; 16];

/// One entry of the library as a list holds it (`library`, `search`,
/// `album`, `artist`, `recording`, `playlist`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Item {
    pub id: Id,
    /// `"song"` today; the name of the table carrying the rest.
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
    pub name: String,
    /// Whoever made its first track.
    pub creator: String,
    pub tracks: i64,
    /// `media.file`'s spelling; empty is the derived square.
    pub art: String,
    pub label: String,
    /// The year it came out; 0 is "nobody said".
    pub released: i64,
}

impl From<AlbumsEntry> for Album {
    fn from(e: AlbumsEntry) -> Album {
        Album {
            name: e.name,
            creator: e.creator,
            tracks: e.tracks,
            art: e.art,
            label: e.label,
            released: e.released,
        }
    }
}

impl Album {
    pub fn from_bytes(b: &[u8]) -> Album {
        AlbumsEntry::from_bytes(b).map_or_else(|_| Album::default(), Album::from)
    }
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
}

impl From<TrackDetailsEntry> for TrackDetail {
    fn from(e: TrackDetailsEntry) -> TrackDetail {
        TrackDetail {
            media_id: e.media_id.0,
            album: e.album,
            track: e.track,
            part: e.part,
            catalogue: e.catalogue,
            performer: e.performer,
            licence: e.licence,
            bpm: e.bpm,
            work: e.work,
            composer: e.composer,
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
            label: String::new(),
            name: "Water Music".into(),
            released: 0,
            tracks: 3,
        };
        assert_eq!(Album::from_bytes(&good.to_bytes()).name, "Water Music");
    }
}
