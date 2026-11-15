//! Every view an app of harken opens, named once (`ark::views`): the
//! window and the phones hold these keys, and what a page is made of is
//! said here, so that every one of them opens the same views for it — a
//! show's page is its episodes, how far everybody has got in each and what
//! each is about, wherever it is drawn.
//!
//! A key's rows are held as their bytes ([`ViewKey::open`]); a host that
//! reads them as types of its own opens them so ([`HarkenView::query`]).

use ark::format::Encode;
use ark::views::{Opening, ViewKey};

use crate::types::*;
use crate::Id;

/// A row id as a key holds it: its sixteen bytes.
pub type Key = [u8; 16];

/// A view of harken's, or a page made of several. A playlist given is the
/// one a list of tracks is read against (each row's place on it); none, the
/// zero id, is none.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HarkenView {
    /// Every song, read against a playlist (`library`).
    Library {
        playlist: Key,
    },
    /// The songs holding `needle` (`search`).
    Search {
        playlist: Key,
        needle: String,
    },
    /// The caller's playlists.
    Playlists,
    Albums,
    Artists,
    Composers,
    /// Every podcast and series of sermons.
    Shows,
    /// The feeds the library follows.
    Subscriptions,
    /// The files the server has not got yet.
    Downloads,
    /// How far the signed-in person has got in each episode.
    MyProgress,
    /// What only a song has, every song's.
    TrackDetails,
    /// The newest play in the log, this person's.
    LastPlayed,
    /// A playlist's tracks.
    Playlist {
        id: Key,
    },
    /// Which of the caller's playlists a track is on.
    PlaylistsOf {
        media: Key,
    },
    /// An album's tracks.
    Album {
        playlist: Key,
        id: String,
    },
    /// Who is on an album.
    AlbumCredits {
        id: String,
    },
    /// Somebody's tracks.
    Artist {
        playlist: Key,
        name: String,
    },
    /// The works somebody wrote.
    Works {
        composer: String,
    },
    /// One work's own row.
    Work {
        id: String,
    },
    /// A work's performances, the most complete first.
    Recordings {
        work: String,
    },
    /// One performance's tracks, in the order the work goes.
    Recording {
        playlist: Key,
        id: String,
    },
    /// Who is on a performance.
    Credits {
        recording: String,
    },
    /// A show's episodes, newest first.
    Show {
        playlist: Key,
        id: String,
    },
    /// How far everybody has got in a show's episodes.
    ShowProgress {
        playlist: Key,
        id: String,
    },
    /// What a show's episodes are about, and when each went out.
    EpisodeDetails {
        playlist: Key,
        id: String,
    },
    /// Who is on each of a show's episodes.
    ShowPeople {
        playlist: Key,
        id: String,
    },
    /// Every episode somebody is on, newest first.
    Appearances {
        playlist: Key,
        name: String,
    },
    /// Who is on an episode.
    People {
        media: Key,
    },
    /// An episode's transcript.
    Transcript {
        media: Key,
    },
    // The pages, each its views.
    /// A show's page: its episodes, how far everybody has got in them and
    /// what each is about.
    ShowPage {
        playlist: Key,
        id: String,
    },
    /// A work's page: the work, for its header, and its performances.
    WorkPage {
        id: String,
    },
    /// Somebody's page: their tracks, the works they wrote and the episodes
    /// they are on.
    ArtistPage {
        playlist: Key,
        name: String,
    },
    /// An album's page: its tracks and who is on it.
    AlbumPage {
        playlist: Key,
        id: String,
    },
    /// A performance's page: its tracks and who is on it, and — its key
    /// being `work@…` — its work and that work's performances.
    RecordingPage {
        playlist: Key,
        id: String,
    },
}

fn id<R>(k: &Key) -> Id<R> {
    Id::new(*k)
}

impl HarkenView {
    /// The views of a page by what each is to it: its tracks, its works,
    /// its performances, who is on it, how far everybody has got, what its
    /// episodes are about, the episodes its person is on. A view that is not
    /// a page is its own tracks when it is a list of them.
    pub fn tracks(&self) -> Option<HarkenView> {
        use HarkenView as V;
        match self {
            V::ShowPage { playlist, id } => Some(V::Show {
                playlist: *playlist,
                id: id.clone(),
            }),
            V::ArtistPage { playlist, name } => Some(V::Artist {
                playlist: *playlist,
                name: name.clone(),
            }),
            V::AlbumPage { playlist, id } => Some(V::Album {
                playlist: *playlist,
                id: id.clone(),
            }),
            V::RecordingPage { playlist, id } => Some(V::Recording {
                playlist: *playlist,
                id: id.clone(),
            }),
            V::WorkPage { .. } => None,
            v @ (V::Library { .. }
            | V::Search { .. }
            | V::Playlist { .. }
            | V::Album { .. }
            | V::Artist { .. }
            | V::Recording { .. }
            | V::Show { .. }
            | V::Appearances { .. }) => Some(v.clone()),
            _ => None,
        }
    }

    /// A page's works: a work's own row, or the works its person wrote.
    pub fn works(&self) -> Option<HarkenView> {
        use HarkenView as V;
        match self {
            V::WorkPage { id } => Some(V::Work { id: id.clone() }),
            V::ArtistPage { name, .. } => Some(V::Works { composer: name.clone() }),
            V::RecordingPage { id, .. } => id.split_once('@').map(|(w, _)| V::Work { id: w.into() }),
            _ => None,
        }
    }

    /// A page's performances: its work's.
    pub fn recordings(&self) -> Option<HarkenView> {
        use HarkenView as V;
        match self {
            V::WorkPage { id } => Some(V::Recordings { work: id.clone() }),
            V::RecordingPage { id, .. } => id.split_once('@').map(|(w, _)| V::Recordings { work: w.into() }),
            _ => None,
        }
    }

    /// Who is on the page's performance or album.
    pub fn credits(&self) -> Option<HarkenView> {
        use HarkenView as V;
        match self {
            V::RecordingPage { id, .. } => Some(V::Credits { recording: id.clone() }),
            V::AlbumPage { id, .. } => Some(V::AlbumCredits { id: id.clone() }),
            _ => None,
        }
    }

    /// How far everybody has got in the page's show's episodes.
    pub fn listened(&self) -> Option<HarkenView> {
        match self {
            HarkenView::ShowPage { playlist, id } => Some(HarkenView::ShowProgress {
                playlist: *playlist,
                id: id.clone(),
            }),
            _ => None,
        }
    }

    /// What the page's show's episodes are about.
    pub fn episodes(&self) -> Option<HarkenView> {
        match self {
            HarkenView::ShowPage { playlist, id } => Some(HarkenView::EpisodeDetails {
                playlist: *playlist,
                id: id.clone(),
            }),
            _ => None,
        }
    }

    /// The episodes the page's person is on.
    pub fn appearances(&self) -> Option<HarkenView> {
        match self {
            HarkenView::ArtistPage { playlist, name } => Some(HarkenView::Appearances {
                playlist: *playlist,
                name: name.clone(),
            }),
            _ => None,
        }
    }

    /// The function a view is of and its input's bytes; none for a page.
    pub fn query(&self) -> Option<(&'static str, Vec<u8>)> {
        use HarkenView as V;
        let none = || ().to_bytes();
        Some(match self {
            V::Library { playlist } => ("library", Library { playlist_id: id(playlist) }.to_bytes()),
            V::Search { playlist, needle } => (
                "search",
                Search {
                    playlist_id: id(playlist),
                    needle: needle.clone(),
                }
                .to_bytes(),
            ),
            V::Playlists => ("playlists", none()),
            V::Albums => ("albums", none()),
            V::Artists => ("artists", none()),
            V::Composers => ("composers", none()),
            V::Shows => ("shows", none()),
            V::Subscriptions => ("subscriptions", none()),
            V::Downloads => ("downloads", none()),
            V::MyProgress => ("my_progress", none()),
            V::TrackDetails => ("track_details", none()),
            V::LastPlayed => ("last_played", none()),
            V::Playlist { id: p } => ("playlist", PlaylistInput { playlist_id: id(p) }.to_bytes()),
            V::PlaylistsOf { media } => ("playlists_of", PlaylistsOf { media_id: id(media) }.to_bytes()),
            V::Album { playlist, id: a } => (
                "album",
                AlbumInput {
                    playlist_id: id(playlist),
                    id: a.clone(),
                }
                .to_bytes(),
            ),
            V::AlbumCredits { id: a } => ("album_credits", AlbumCredits { id: a.clone() }.to_bytes()),
            V::Artist { playlist, name } | V::Appearances { playlist, name } => (
                if matches!(self, V::Artist { .. }) { "artist" } else { "appearances" },
                Artist {
                    playlist_id: id(playlist),
                    name: name.clone(),
                }
                .to_bytes(),
            ),
            V::Works { composer } => ("works", Works { composer: composer.clone() }.to_bytes()),
            V::Work { id: w } => ("work", WorkInput { id: w.clone() }.to_bytes()),
            V::Recordings { work } => ("recordings", Recordings { work_id: work.clone() }.to_bytes()),
            V::Recording { playlist, id: r } => (
                "recording",
                RecordingInput {
                    playlist_id: id(playlist),
                    id: r.clone(),
                }
                .to_bytes(),
            ),
            V::Credits { recording } => (
                "credits",
                Credits {
                    recording_id: recording.clone(),
                }
                .to_bytes(),
            ),
            V::Show { playlist, id: s }
            | V::ShowProgress { playlist, id: s }
            | V::EpisodeDetails { playlist, id: s }
            | V::ShowPeople { playlist, id: s } => (
                match self {
                    V::Show { .. } => "show",
                    V::ShowProgress { .. } => "show_progress",
                    V::EpisodeDetails { .. } => "episode_details",
                    _ => "show_people",
                },
                ShowInput {
                    playlist_id: id(playlist),
                    id: s.clone(),
                }
                .to_bytes(),
            ),
            V::People { media } | V::Transcript { media } => (
                if matches!(self, V::People { .. }) { "people" } else { "transcript" },
                EpisodeInput { media_id: id(media) }.to_bytes(),
            ),
            V::ShowPage { .. } | V::WorkPage { .. } | V::ArtistPage { .. } | V::AlbumPage { .. } | V::RecordingPage { .. } => return None,
        })
    }

    /// Every view of a page: what [`ViewKey::parts`] holds.
    pub fn views(&self) -> Vec<HarkenView> {
        if self.query().is_some() {
            return vec![self.clone()];
        }
        [
            self.tracks(),
            self.works(),
            self.recordings(),
            self.credits(),
            self.listened(),
            self.episodes(),
            self.appearances(),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

impl ViewKey for HarkenView {
    fn parts(&self) -> Vec<HarkenView> {
        self.views()
    }

    /// Its rows as their bytes: the phones read them as JSON, and the
    /// window as types of its own (it opens its views itself, by
    /// [`HarkenView::query`]).
    fn open(&self) -> Option<Opening> {
        let (name, input) = self.query()?;
        Some(Opening::read(name, input, <[u8]>::to_vec))
    }
}
