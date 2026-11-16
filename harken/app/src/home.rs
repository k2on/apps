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
    /// A tile of what was added most recently pressed: an album's opens its
    /// page; a song on no album plays, the newest such songs from it.
    OpenNew {
        key: String,
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

/// What a tile of the newest names an album by: `album:<its key>`, apart
/// from a song's id.
const ALBUM: &str = "album:";

/// One of the newest: an album, its songs added most recently first, or a
/// song on no album.
enum New {
    Album(AlbumsEntry, Vec<LibraryEntry>),
    Song(LibraryEntry),
}

/// What was added most recently, the newest first, each album once — its
/// newest songs under it — so a shelf is never one cover again and again;
/// at most [`NEWEST`].
fn newest(data: &Data<'_, HarkenView>) -> Vec<New> {
    let mut all: Vec<LibraryEntry> = rows::<LibraryEntry>(data, &songs()).into_iter().filter(|e| e.kind != "episode").collect();
    all.sort_by_key(|e| std::cmp::Reverse((e.added_ms, e.pos)));
    let on: std::collections::BTreeMap<Id<Media>, String> = rows::<TrackDetailsEntry>(data, &HarkenView::TrackDetails)
        .into_iter()
        .filter(|d| !d.album_id.is_empty())
        .map(|d| (d.media_id, d.album_id))
        .collect();
    let albums: std::collections::BTreeMap<String, AlbumsEntry> = rows::<AlbumsEntry>(data, &HarkenView::Albums)
        .into_iter()
        .map(|a| (a.id.clone(), a))
        .collect();
    let mut out: Vec<New> = vec![];
    for e in all {
        let album = on.get(&e.id).and_then(|id| albums.get(id));
        match album {
            Some(a) => match out.iter().position(|n| matches!(n, New::Album(b, _) if b.id == a.id)) {
                Some(i) => {
                    if let New::Album(_, songs) = &mut out[i] {
                        songs.push(e)
                    }
                }
                None if out.len() < NEWEST => out.push(New::Album(a.clone(), vec![e])),
                None => {}
            },
            None if out.len() < NEWEST => out.push(New::Song(e)),
            None => {}
        }
    }
    out
}

/// The newest songs on no album, the newest first: what a loose song's
/// tile plays from.
fn loose(data: &Data<'_, HarkenView>) -> Vec<String> {
    newest(data)
        .into_iter()
        .filter_map(|n| match n {
            New::Song(e) => Some(e.id.hex()),
            New::Album(..) => None,
        })
        .collect()
}

/// Songs' titles as a tile's caption says them: the first two, then how
/// many more.
fn titles(songs: &[LibraryEntry]) -> String {
    let named: Vec<&str> = songs.iter().take(2).map(|e| e.title.as_str()).collect();
    match songs.len() {
        0..=2 => named.join(", "),
        n => format!("{}, {} more", named.join(", "), n - 2),
    }
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
            HomeMsg::OpenNew { key } => {
                if let Some(album) = key.strip_prefix(ALBUM) {
                    let title = rows::<AlbumsEntry>(data, &HarkenView::Albums)
                        .into_iter()
                        .find(|a| a.id == album)
                        .map(|a| a.name)
                        .unwrap_or_default();
                    return vec![Command::Open(Route::Album { key: album.into(), title })];
                }
                let tracks = loose(data);
                let at = tracks.iter().position(|t| *t == key).unwrap_or(0) as i64;
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
            .into_iter()
            .map(|n| match n {
                New::Album(a, songs) => ArtTile {
                    key: format!("{ALBUM}{}", a.id),
                    caption: titles(&songs),
                    ..album_tile(server, &a)
                },
                New::Song(e) => ArtTile {
                    key: e.id.hex(),
                    title: e.title.clone(),
                    caption: e.creator.clone(),
                    art: covers.track(&e.id.hex()),
                    seed: e.title.clone(),
                },
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
