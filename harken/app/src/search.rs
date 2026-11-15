//! Search: the library answered as it is typed — the songs holding the
//! words (the domain's own `search`), and the albums and the people whose
//! names hold them, so the three answer one question; nothing typed, an
//! invitation to type.

use ark_app::{App, Command, Data};
use harken_domain::*;

use crate::library::{album_tile, artist_tile, track_item, ArtTile, TrackItem};
use crate::routes::Route;
use crate::words::rows;
use crate::HarkenRequest;

pub mod view;

#[cfg(test)]
mod tests;

/// How many songs a search lists.
pub const SONGS: usize = 25;

/// Search, as `ark_app` runs it.
pub struct Search;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// What is typed.
    pub needle: String,
    /// Where covers are fetched from.
    pub server: String,
}

#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub enum SearchMsg {
    Typed { text: String },
    Server { server: String },
    Play { id: String },
    OpenAlbum { key: String, title: String },
    OpenArtist { name: String },
}

/// Everything Search shows.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct SearchPage {
    pub needle: String,
    pub has_albums: bool,
    pub albums: Vec<ArtTile>,
    pub has_artists: bool,
    pub artists: Vec<ArtTile>,
    pub has_songs: bool,
    pub songs: Vec<TrackItem>,
    /// Nothing typed yet.
    pub idle: bool,
    /// Typed, and nothing holds it.
    pub nothing: bool,
    pub nothing_text: String,
}

fn needle(s: &State) -> String {
    s.needle.trim().to_string()
}

fn songs_view(s: &State) -> Option<HarkenView> {
    let n = needle(s);
    (!n.is_empty()).then_some(HarkenView::Search {
        playlist: [0; 16],
        needle: n,
    })
}

impl App for Search {
    type Key = HarkenView;
    type State = State;
    type Msg = SearchMsg;
    type Change = HarkenChange;
    type Route = Route;
    type Request = HarkenRequest;
    type Screen = SearchPage;

    fn holds(s: &State) -> Vec<HarkenView> {
        songs_view(s).into_iter().chain([HarkenView::Albums, HarkenView::Artists]).collect()
    }

    fn update(s: &mut State, msg: SearchMsg, data: &Data<'_, HarkenView>) -> Vec<Command<Self>> {
        match msg {
            SearchMsg::Typed { text } => {
                s.needle = text;
                vec![]
            }
            SearchMsg::Server { server } => {
                s.server = server;
                vec![]
            }
            SearchMsg::Play { id } => {
                let tracks: Vec<String> = songs_view(s)
                    .map(|v| rows::<LibraryEntry>(data, &v).iter().take(SONGS).map(|e| e.id.hex()).collect())
                    .unwrap_or_default();
                let at = tracks.iter().position(|t| *t == id).unwrap_or(0) as i64;
                vec![Command::Request(HarkenRequest::Play {
                    tracks,
                    at,
                    from: Route::Library.url(),
                    shuffle: false,
                })]
            }
            SearchMsg::OpenAlbum { key, title } => vec![Command::Open(Route::Album { key, title })],
            SearchMsg::OpenArtist { name } => vec![Command::Open(Route::Artist { name })],
        }
    }

    fn screen(s: &State, data: &Data<'_, HarkenView>) -> SearchPage {
        let n = needle(s);
        let lower = n.to_lowercase();
        let server = &s.server;
        let (albums, artists, songs) = match n.is_empty() {
            true => (vec![], vec![], vec![]),
            false => (
                rows::<AlbumsEntry>(data, &HarkenView::Albums)
                    .iter()
                    .filter(|a| a.name.to_lowercase().contains(&lower) || a.performer.to_lowercase().contains(&lower))
                    .map(|a| album_tile(server, a))
                    .collect::<Vec<_>>(),
                rows::<ArtistsEntry>(data, &HarkenView::Artists)
                    .iter()
                    .filter(|a| a.name.to_lowercase().contains(&lower))
                    .map(|a| artist_tile(server, a))
                    .collect::<Vec<_>>(),
                songs_view(s)
                    .map(|v| rows::<LibraryEntry>(data, &v).iter().take(SONGS).map(|e| track_item(server, e)).collect())
                    .unwrap_or_default(),
            ),
        };
        let nothing = !n.is_empty() && albums.is_empty() && artists.is_empty() && songs.is_empty();
        SearchPage {
            nothing_text: format!("Nothing in your library holds \u{201c}{n}\u{201d}."),
            needle: s.needle.clone(),
            has_albums: !albums.is_empty(),
            albums,
            has_artists: !artists.is_empty(),
            artists,
            has_songs: !songs.is_empty(),
            songs,
            idle: n.is_empty(),
            nothing,
        }
    }
}
