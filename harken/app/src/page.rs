//! A record's page: an album, somebody, a playlist or a show — its cover
//! beside its titles, Play and Shuffle under them, then its tracks (a
//! show's episodes) as rows, each played from there. A route names it;
//! a phone's stack holds one for each page pushed, the window one for the
//! place it shows.

use ark_app::{App, Command, Data};
use harken_domain::*;

use crate::library::{track_item, TrackItem};
use crate::routes::Route;
use crate::words::{self, rows};
use crate::{key_of, HarkenRequest};

pub mod view;

#[cfg(test)]
mod tests;

/// A record's page, as `ark_app` runs it.
pub struct Page;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// What it shows; none is nothing yet.
    pub route: Option<Route>,
    /// Where covers are fetched from.
    pub server: String,
}

#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub enum PageMsg {
    /// The page of this route.
    Show {
        route: Route,
    },
    /// Covers fetched from this server.
    Server {
        server: String,
    },
    /// Every track played from the first, in order or shuffled.
    PlayAll {
        shuffle: bool,
    },
    /// Every track played, from this one.
    Play {
        id: String,
    },
    Queue {
        id: String,
        next: bool,
    },
    AddTo {
        id: String,
    },
    /// Somebody's page, from whoever a record is by.
    OpenArtist {
        name: String,
    },
}

/// Everything a record's page shows.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct RecordPage {
    /// What it is: "Album", "Artist", "Playlist", "Podcast".
    pub kicker: String,
    pub title: String,
    /// Who it is by; empty for none.
    pub by: String,
    /// Its small print: a year, how many tracks.
    pub caption: String,
    pub art: String,
    pub seed: String,
    /// Round: a person.
    pub round: bool,
    pub tracks: Vec<TrackItem>,
    pub empty: bool,
}

/// The tracks' view of a record's route: an album's, a person's, a
/// playlist's or a show's; none for any other.
pub fn tracks_of(route: &Route) -> Option<HarkenView> {
    let none = [0; 16];
    Some(match route {
        Route::Album { key, .. } => HarkenView::Album {
            playlist: none,
            id: key.clone(),
        },
        Route::Artist { name } => HarkenView::Artist {
            playlist: none,
            name: name.clone(),
        },
        Route::Playlist { id, .. } => HarkenView::Playlist { id: key_of(id)? },
        Route::Show { key, .. } => HarkenView::Show {
            playlist: none,
            id: key.clone(),
        },
        _ => return None,
    })
}

impl State {
    /// The tracks' view of the page's route.
    fn tracks(&self) -> Option<HarkenView> {
        tracks_of(self.route.as_ref()?)
    }

    /// The view its header is read from.
    fn header(&self) -> Option<HarkenView> {
        Some(match self.route.as_ref()? {
            Route::Album { .. } => HarkenView::Albums,
            Route::Artist { .. } => HarkenView::Artists,
            Route::Playlist { .. } => HarkenView::Playlists,
            Route::Show { .. } => HarkenView::Shows,
            _ => return None,
        })
    }
}

impl App for Page {
    type Key = HarkenView;
    type State = State;
    type Msg = PageMsg;
    type Change = HarkenChange;
    type Route = Route;
    type Request = HarkenRequest;
    type Screen = RecordPage;

    fn holds(s: &State) -> Vec<HarkenView> {
        s.tracks().into_iter().chain(s.header()).collect()
    }

    fn update(s: &mut State, msg: PageMsg, data: &Data<'_, HarkenView>) -> Vec<Command<Self>> {
        let all = |s: &State| -> Vec<String> {
            s.tracks()
                .map(|v| rows::<LibraryEntry>(data, &v).iter().map(|e| e.id.hex()).collect())
                .unwrap_or_default()
        };
        let from = |s: &State| s.route.as_ref().map(|r| r.url()).unwrap_or_default();
        match msg {
            PageMsg::Show { route } => {
                s.route = Some(route);
                vec![]
            }
            PageMsg::Server { server } => {
                s.server = server;
                vec![]
            }
            PageMsg::PlayAll { shuffle } => vec![Command::Request(HarkenRequest::Play {
                tracks: all(s),
                at: 0,
                from: from(s),
                shuffle,
            })],
            PageMsg::Play { id } => {
                let tracks = all(s);
                let at = tracks.iter().position(|t| *t == id).unwrap_or(0) as i64;
                vec![Command::Request(HarkenRequest::Play {
                    tracks,
                    at,
                    from: from(s),
                    shuffle: false,
                })]
            }
            PageMsg::Queue { id, next } => vec![Command::Request(HarkenRequest::Queue { track: id, next })],
            PageMsg::AddTo { id } => vec![Command::Request(HarkenRequest::AddTo { track: id })],
            PageMsg::OpenArtist { name } => vec![Command::Open(Route::Artist { name })],
        }
    }

    fn screen(s: &State, data: &Data<'_, HarkenView>) -> RecordPage {
        let server = &s.server;
        let tracks: Vec<LibraryEntry> = s.tracks().map(|v| rows(data, &v)).unwrap_or_default();
        let header = s.header();
        let mut p = RecordPage {
            kicker: String::new(),
            title: String::new(),
            by: String::new(),
            caption: String::new(),
            art: String::new(),
            seed: String::new(),
            round: false,
            tracks: tracks.iter().map(|e| track_item(server, e)).collect(),
            empty: tracks.is_empty(),
        };
        match (&s.route, header) {
            (Some(Route::Album { key, title }), Some(h)) => {
                let a = rows::<AlbumsEntry>(data, &h).into_iter().find(|a| a.id == *key);
                p.kicker = "Album".into();
                p.title = a.as_ref().map_or(title.clone(), |a| a.name.clone());
                p.by = a.as_ref().map_or(String::new(), |a| {
                    if a.performer.is_empty() {
                        a.creator.clone()
                    } else {
                        a.performer.clone()
                    }
                });
                p.caption = a.as_ref().map_or(String::new(), |a| {
                    [words::year(a.released), words::tracks(a.tracks)]
                        .into_iter()
                        .filter(|w| !w.is_empty())
                        .collect::<Vec<_>>()
                        .join(" · ")
                });
                p.art = a.as_ref().map_or(String::new(), |a| words::art_url(server, &a.art));
            }
            (Some(Route::Artist { name }), Some(h)) => {
                let a = rows::<ArtistsEntry>(data, &h).into_iter().find(|a| a.name == *name);
                p.kicker = "Artist".into();
                p.title = name.clone();
                p.caption = a.as_ref().map_or(String::new(), |a| words::tracks(a.tracks));
                p.art = a.as_ref().map_or(String::new(), |a| words::art_url(server, &a.art));
                p.round = true;
            }
            (Some(Route::Playlist { id, name }), Some(h)) => {
                let l = rows::<Playlist>(data, &h).into_iter().find(|l| l.id.hex() == *id);
                p.kicker = "Playlist".into();
                p.title = l.map_or(name.clone(), |l| l.name);
                p.caption = words::tracks(tracks.len() as i64);
            }
            (Some(Route::Show { key, title }), Some(h)) => {
                let show = rows::<ShowsEntry>(data, &h).into_iter().find(|x| x.id == *key);
                p.kicker = match show.as_ref().map(|x| x.kind.as_str()) {
                    Some("sermon") => "Sermons".into(),
                    _ => "Podcast".into(),
                };
                p.title = show.as_ref().map_or(title.clone(), |x| x.title.clone());
                p.by = show.as_ref().map_or(String::new(), |x| x.author.clone());
                p.caption = show.as_ref().map_or(String::new(), |x| words::episodes(x.episodes));
                p.art = show.as_ref().map_or(String::new(), |x| words::art_url(server, &x.art));
            }
            _ => {}
        }
        p.seed = p.title.clone();
        p
    }
}
