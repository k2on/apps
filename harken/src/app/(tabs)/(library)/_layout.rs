//! Library: everything, six ways — the person's playlists, the albums, the
//! people, every song in the order it arrived, the podcasts and the
//! sermons — a row of chips choosing which, each item opening the page it
//! stands for. The playlists' first row makes a new one, its name checked
//! by `create_playlist`'s own refinements as it is typed.

use ark_app::{App, Command, Data};
use harken_domain::*;

use crate::words::{self, rows};
use crate::Route;
use crate::{HarkenRequest, Route as R};

pub mod view;

#[cfg(test)]
mod tests;

/// Library, as `ark_app` runs it.
#[ark_router::screen(view = view::library)]
pub struct Library;

/// The six ways the library is shown, by their chips' tags.
pub const KINDS: [(&str, &str, &str); 6] = [
    ("playlists", "Playlists", "list-music"),
    ("albums", "Albums", "disc-3"),
    ("artists", "Artists", "mic-vocal"),
    ("songs", "Songs", "music"),
    ("podcasts", "Podcasts", "podcast"),
    ("sermons", "Sermons", "book-open"),
];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// The chip chosen (`KINDS`' tag); none is the playlists.
    pub kind: String,
    /// Where covers are fetched from: the server signed in to.
    pub server: String,
    /// A new playlist's name being typed.
    pub naming: Option<String>,
    /// What the domain said of the last name sent, until it is typed again.
    pub refused: Option<String>,
}

impl State {
    fn kind(&self) -> &str {
        if self.kind.is_empty() {
            "playlists"
        } else {
            &self.kind
        }
    }
}

#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub enum LibraryMsg {
    /// The library shown one of its six ways.
    Show {
        kind: String,
    },
    /// Covers fetched from this server.
    Server {
        server: String,
    },
    OpenPlaylist {
        id: String,
        name: String,
    },
    OpenAlbum {
        key: String,
        title: String,
    },
    OpenArtist {
        name: String,
    },
    OpenShow {
        key: String,
        title: String,
    },
    /// Every song played, from this one.
    Play {
        id: String,
    },
    /// A song next, or last of what is queued.
    Queue {
        id: String,
        next: bool,
    },
    /// A song put on a playlist or taken off one.
    AddTo {
        id: String,
    },
    NewPlaylist,
    Name {
        name: String,
    },
    SavePlaylist,
    Dismiss,
    /// Follow a show of the kind shown.
    Follow,
}

/// A chip of the six.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct KindChip {
    pub tag: String,
    pub label: String,
    pub glyph: String,
    pub selected: bool,
}

/// A playlist, as its row says it.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct PlaylistItem {
    pub id: String,
    pub name: String,
}

/// Something with a cover, as a tile says it: what it opens by, its cover
/// and its words.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct ArtTile {
    pub key: String,
    pub title: String,
    pub caption: String,
    pub art: String,
    pub seed: String,
}

/// A track, as its row says it.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct TrackItem {
    pub id: String,
    pub title: String,
    pub under: String,
    pub length: String,
    pub art: String,
    pub seed: String,
}

/// A new playlist's name, as its sheet shows it.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct NameForm {
    pub name: String,
    pub problem: Option<String>,
    pub can_save: bool,
}

/// Everything Library shows.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct LibraryPage {
    pub chips: Vec<KindChip>,
    pub on_playlists: bool,
    pub on_albums: bool,
    pub on_artists: bool,
    pub on_songs: bool,
    pub on_shows: bool,
    /// The row that follows a show of the kind shown: "Follow a Podcast".
    pub follow: String,
    pub playlists: Vec<PlaylistItem>,
    pub albums: Vec<ArtTile>,
    pub artists: Vec<ArtTile>,
    pub songs: Vec<TrackItem>,
    pub shows: Vec<ArtTile>,
    /// Nothing to show this way.
    pub empty: bool,
    pub empty_glyph: String,
    pub empty_title: String,
    pub empty_text: String,
    pub naming: Option<NameForm>,
}

/// The library as the songs are read: against no playlist.
fn songs_view() -> HarkenView {
    HarkenView::Library { playlist: [0; 16] }
}

/// A song as a row says it.
pub fn track_item(covers: &words::Covers, e: &LibraryEntry) -> TrackItem {
    TrackItem {
        id: e.id.hex(),
        title: e.title.clone(),
        under: e.creator.clone(),
        length: words::length(e.duration_ms),
        art: covers.track(&e.id.hex()),
        seed: e.title.clone(),
    }
}

/// An album as a tile says it.
pub fn album_tile(server: &str, a: &AlbumsEntry) -> ArtTile {
    ArtTile {
        key: a.id.clone(),
        title: a.name.clone(),
        caption: if a.performer.is_empty() {
            a.creator.clone()
        } else {
            a.performer.clone()
        },
        art: words::art_url(server, &a.art),
        seed: a.name.clone(),
    }
}

/// Somebody as a tile says them.
pub fn artist_tile(server: &str, a: &ArtistsEntry) -> ArtTile {
    ArtTile {
        key: a.name.clone(),
        title: a.name.clone(),
        caption: words::tracks(a.tracks),
        art: words::art_url(server, &a.art),
        seed: a.name.clone(),
    }
}

/// A show as a tile says it.
pub fn show_tile(server: &str, s: &ShowsEntry) -> ArtTile {
    ArtTile {
        key: s.id.clone(),
        title: s.title.clone(),
        caption: s.author.clone(),
        art: words::art_url(server, &s.art),
        seed: s.title.clone(),
    }
}

impl App for Library {
    type Key = HarkenView;
    type State = State;
    type Msg = LibraryMsg;
    type Change = HarkenChange;
    type Route = Route;
    type Request = HarkenRequest;
    type Screen = LibraryPage;

    /// A list of the library's (`src/app/(tabs)/(library)`): the library
    /// shown its way.
    fn shown(route: &Route) -> Option<LibraryMsg> {
        let kind = match route {
            Route::Library => "songs",
            Route::Albums => "albums",
            Route::Artists | Route::Composers => "artists",
            Route::Podcasts => "podcasts",
            Route::Sermons => "sermons",
            _ => return None,
        };
        Some(LibraryMsg::Show { kind: kind.into() })
    }

    fn holds(s: &State) -> Vec<HarkenView> {
        match s.kind() {
            "albums" => vec![HarkenView::Albums],
            "artists" => vec![HarkenView::Artists],
            "songs" => [songs_view()].into_iter().chain(words::Covers::VIEWS).collect(),
            "podcasts" | "sermons" => vec![HarkenView::Shows],
            _ => vec![HarkenView::Playlists],
        }
    }

    fn update(s: &mut State, msg: LibraryMsg, data: &Data<'_, HarkenView>) -> Vec<Command<Self>> {
        match msg {
            LibraryMsg::Show { kind } => {
                s.kind = kind;
                vec![]
            }
            LibraryMsg::Server { server } => {
                s.server = server;
                vec![]
            }
            LibraryMsg::OpenPlaylist { id, name } => vec![Command::Open(R::Playlist { id, name })],
            LibraryMsg::OpenAlbum { key, title } => vec![Command::Open(R::Album { key, title })],
            LibraryMsg::OpenArtist { name } => vec![Command::Open(R::Artist { name })],
            LibraryMsg::OpenShow { key, title } => vec![Command::Open(R::Show { key, title })],
            LibraryMsg::Play { id } => {
                let songs: Vec<LibraryEntry> = rows(data, &songs_view());
                let tracks: Vec<String> = songs.iter().map(|e| e.id.hex()).collect();
                let at = tracks.iter().position(|t| *t == id).unwrap_or(0) as i64;
                vec![Command::Request(HarkenRequest::Play {
                    tracks,
                    at,
                    from: R::Library.url(),
                    shuffle: false,
                })]
            }
            LibraryMsg::Queue { id, next } => vec![Command::Request(HarkenRequest::Queue { track: id, next })],
            LibraryMsg::AddTo { id } => vec![Command::Request(HarkenRequest::AddTo { track: id })],
            LibraryMsg::Follow => {
                let kind = if s.kind() == "sermons" { "sermon" } else { "podcast" };
                vec![Command::Request(HarkenRequest::Follow { kind: kind.into() })]
            }
            LibraryMsg::NewPlaylist => {
                s.naming = Some(String::new());
                s.refused = None;
                vec![]
            }
            LibraryMsg::Name { name } => {
                s.naming = Some(name);
                s.refused = None;
                vec![]
            }
            LibraryMsg::SavePlaylist => {
                let Some(name) = s.naming.clone() else { return vec![] };
                let change = HarkenChange::CreatePlaylist(CreatePlaylist { name });
                if let Some(why) = change.problem() {
                    s.refused = Some(why);
                    return vec![];
                }
                s.naming = None;
                vec![Command::send(change)]
            }
            LibraryMsg::Dismiss => {
                s.naming = None;
                s.refused = None;
                vec![]
            }
        }
    }

    fn screen(s: &State, data: &Data<'_, HarkenView>) -> LibraryPage {
        let kind = s.kind();
        let server = &s.server;
        let covers = words::Covers::read(server, data);
        let mut page = LibraryPage {
            chips: KINDS
                .iter()
                .map(|(tag, label, glyph)| KindChip {
                    tag: tag.to_string(),
                    label: label.to_string(),
                    glyph: glyph.to_string(),
                    selected: *tag == kind,
                })
                .collect(),
            on_playlists: kind == "playlists",
            on_albums: kind == "albums",
            on_artists: kind == "artists",
            on_songs: kind == "songs",
            on_shows: kind == "podcasts" || kind == "sermons",
            follow: if kind == "sermons" { "Follow a Series" } else { "Follow a Podcast" }.into(),
            playlists: vec![],
            albums: vec![],
            artists: vec![],
            songs: vec![],
            shows: vec![],
            empty: false,
            empty_glyph: String::new(),
            empty_title: String::new(),
            empty_text: String::new(),
            naming: s.naming.as_ref().map(|name| {
                let problem = s.refused.clone();
                NameForm {
                    can_save: HarkenChange::CreatePlaylist(CreatePlaylist { name: name.clone() }).problem().is_none(),
                    name: name.clone(),
                    problem,
                }
            }),
        };
        let (glyph, title, text) = match kind {
            "albums" => {
                page.albums = rows::<AlbumsEntry>(data, &HarkenView::Albums)
                    .iter()
                    .map(|a| album_tile(server, a))
                    .collect();
                ("disc-3", "No Albums", "The albums your server finds show here.")
            }
            "artists" => {
                page.artists = rows::<ArtistsEntry>(data, &HarkenView::Artists)
                    .iter()
                    .map(|a| artist_tile(server, a))
                    .collect();
                ("mic-vocal", "No Artists", "Whoever made what your server finds shows here.")
            }
            "songs" => {
                page.songs = rows::<LibraryEntry>(data, &songs_view())
                    .iter()
                    .filter(|e| e.kind != "episode")
                    .map(|e| track_item(&covers, e))
                    .collect();
                ("music", "No Songs", "The songs your server finds show here.")
            }
            "podcasts" | "sermons" => {
                let want = if kind == "podcasts" { "podcast" } else { "sermon" };
                page.shows = rows::<ShowsEntry>(data, &HarkenView::Shows)
                    .iter()
                    .filter(|s| s.kind == want)
                    .map(|s| show_tile(server, s))
                    .collect();
                match kind {
                    "podcasts" => ("podcast", "No Podcasts", "Follow a podcast and its episodes show here."),
                    _ => ("book-open", "No Sermons", "Follow a series of sermons and its sermons show here."),
                }
            }
            _ => {
                page.playlists = rows::<Playlist>(data, &HarkenView::Playlists)
                    .iter()
                    .map(|p| PlaylistItem {
                        id: p.id.hex(),
                        name: p.name.clone(),
                    })
                    .collect();
                ("list-music", "No Playlists", "Make one with New Playlist.")
            }
        };
        page.empty = page.playlists.is_empty()
            && page.albums.is_empty()
            && page.artists.is_empty()
            && page.songs.is_empty()
            && page.shows.is_empty()
            && !page.on_playlists;
        page.empty_glyph = glyph.into();
        page.empty_title = title.into();
        page.empty_text = text.into();
        page
    }
}
