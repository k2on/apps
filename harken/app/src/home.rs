//! Home: where you were, and what is new. What was played last and a way
//! back to it (the account's, the newest play in the log, the same on every
//! device and there offline), then what arrived most recently, the
//! person's playlists and the albums, each a shelf of covers opening its
//! page.

use ark_app::{App, Command, Data};
use harken_domain::*;

use crate::library::{album_tile, ArtTile, PlaylistItem};
use crate::routes::Route;
use crate::words::{self, rows};
use crate::HarkenRequest;

pub mod view;

#[cfg(test)]
mod tests;

/// How many of the newest songs Home's shelf shows.
pub const NEWEST: usize = 12;

/// Home, as `ark_app` runs it.
pub struct Home;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// Where covers are fetched from.
    pub server: String,
}

#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub enum HomeMsg {
    Server {
        server: String,
    },
    /// What was played last, played again from where it was played.
    Resume,
    /// One of the newest songs played, the newest first.
    PlayNew {
        id: String,
    },
    OpenPlaylist {
        id: String,
        name: String,
    },
    OpenAlbum {
        key: String,
        title: String,
    },
    /// Library, on one of its ways (`See All`).
    SeeAll {
        kind: String,
    },
}

/// What was played last, as its card says it.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct LastPlayed {
    pub title: String,
    pub under: String,
    pub art: String,
    pub seed: String,
}

/// Everything Home shows.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct HomePage {
    pub last: Option<LastPlayed>,
    pub newest: Vec<ArtTile>,
    pub has_playlists: bool,
    pub playlists: Vec<PlaylistItem>,
    pub has_albums: bool,
    pub albums: Vec<ArtTile>,
    /// Nothing in the library yet.
    pub empty: bool,
}

fn songs() -> HarkenView {
    HarkenView::Library { playlist: [0; 16] }
}

/// The newest songs, the newest first.
fn newest(data: &Data<'_, HarkenView>) -> Vec<LibraryEntry> {
    let mut all: Vec<LibraryEntry> = rows::<LibraryEntry>(data, &songs()).into_iter().filter(|e| e.kind != "episode").collect();
    all.sort_by_key(|e| std::cmp::Reverse((e.added_ms, e.pos)));
    all.truncate(NEWEST);
    all
}

impl App for Home {
    type Key = HarkenView;
    type State = State;
    type Msg = HomeMsg;
    type Change = HarkenChange;
    type Route = Route;
    type Request = HarkenRequest;
    type Screen = HomePage;

    fn holds(_: &State) -> Vec<HarkenView> {
        vec![
            HarkenView::LastPlayed,
            songs(),
            HarkenView::Playlists,
            HarkenView::Albums,
            HarkenView::TrackDetails,
        ]
    }

    fn update(s: &mut State, msg: HomeMsg, data: &Data<'_, HarkenView>) -> Vec<Command<Self>> {
        match msg {
            HomeMsg::Server { server } => {
                s.server = server;
                vec![]
            }
            HomeMsg::Resume => {
                let Some(last) = rows::<Play>(data, &HarkenView::LastPlayed).into_iter().next() else {
                    return vec![];
                };
                vec![Command::Request(HarkenRequest::Play {
                    tracks: vec![last.track],
                    at: 0,
                    from: last.from,
                    shuffle: false,
                })]
            }
            HomeMsg::PlayNew { id } => {
                let tracks: Vec<String> = newest(data).iter().map(|e| e.id.hex()).collect();
                let at = tracks.iter().position(|t| *t == id).unwrap_or(0) as i64;
                vec![Command::Request(HarkenRequest::Play {
                    tracks,
                    at,
                    from: Route::Library.url(),
                    shuffle: false,
                })]
            }
            HomeMsg::OpenPlaylist { id, name } => vec![Command::Open(Route::Playlist { id, name })],
            HomeMsg::OpenAlbum { key, title } => vec![Command::Open(Route::Album { key, title })],
            HomeMsg::SeeAll { kind } => vec![Command::Open(match kind.as_str() {
                "albums" => Route::Albums,
                _ => Route::Library,
            })],
        }
    }

    fn screen(s: &State, data: &Data<'_, HarkenView>) -> HomePage {
        let server = &s.server;
        let covers = words::Covers::read(server, data);
        let last = rows::<Play>(data, &HarkenView::LastPlayed).into_iter().next().map(|p| LastPlayed {
            under: [p.creator.clone(), p.album.clone()]
                .into_iter()
                .filter(|w| !w.is_empty())
                .collect::<Vec<_>>()
                .join(" · "),
            art: covers.track(&p.track),
            seed: p.title.clone(),
            title: p.title,
        });
        let newest: Vec<ArtTile> = newest(data)
            .iter()
            .map(|e| ArtTile {
                key: e.id.hex(),
                title: e.title.clone(),
                caption: e.creator.clone(),
                art: covers.track(&e.id.hex()),
                seed: e.title.clone(),
            })
            .collect();
        let playlists: Vec<PlaylistItem> = rows::<Playlist>(data, &HarkenView::Playlists)
            .into_iter()
            .map(|p| PlaylistItem {
                id: p.id.hex(),
                name: p.name,
            })
            .collect();
        let albums: Vec<ArtTile> = rows::<AlbumsEntry>(data, &HarkenView::Albums)
            .iter()
            .map(|a| album_tile(server, a))
            .collect();
        HomePage {
            empty: newest.is_empty() && albums.is_empty() && last.is_none(),
            last,
            newest,
            has_playlists: !playlists.is_empty(),
            playlists,
            has_albums: !albums.is_empty(),
            albums,
        }
    }
}
