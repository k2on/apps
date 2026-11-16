//! harken's sheets, raised by any screen (as money's are): a track's
//! playlists, to put it on one or take it off; and following a show by its
//! feed's address. The host raises them ([`SheetsMsg::AddTo`],
//! [`SheetsMsg::Follow`]); what they keep is a change of the domain's.

use ark_app::{App, Command, Data};
use harken_domain::{AddToPlaylist, HarkenChange, HarkenView, Id, Playlist, RemoveFromPlaylist, Subscribe};

use crate::words::rows;
use crate::Route;
use crate::{key_of, HarkenRequest};

pub mod view;

#[cfg(test)]
mod tests;

/// The sheets, as `ark_app` runs them.
#[ark_router::screen(view = view::sheets)]
pub struct Sheets;

/// Which sheet is up, and what is typed in it.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum State {
    #[default]
    None,
    /// A track's playlists (its id's hex).
    AddTo { track: String },
    /// A show to follow: its kind and the feed's address typed.
    Follow { kind: String, url: String, refused: Option<String> },
}

#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub enum SheetsMsg {
    /// A track's playlists, up.
    AddTo {
        track: String,
    },
    /// A playlist pressed: the track put on it, or taken off.
    Toggle {
        playlist: String,
    },
    /// Following a show (`podcast`, `sermon`), up.
    Follow {
        kind: String,
    },
    Url {
        url: String,
    },
    Subscribe,
    Dismiss,
}

/// A playlist as the sheet lists it: whether the track is on it.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct PlaylistChoice {
    pub id: String,
    pub name: String,
    pub on: bool,
    /// "Added" while it is on it.
    pub mark: String,
}

/// The follow sheet's form.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct FollowForm {
    pub title: String,
    pub url: String,
    pub problem: Option<String>,
    pub can_save: bool,
}

/// Everything the sheets show.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct SheetsPage {
    pub adding: bool,
    pub playlists: Vec<PlaylistChoice>,
    pub following: Option<FollowForm>,
}

fn subscribe(kind: &str, url: &str) -> HarkenChange {
    HarkenChange::Subscribe(Subscribe {
        kind: kind.into(),
        url: url.trim().into(),
    })
}

impl App for Sheets {
    type Key = HarkenView;
    type State = State;
    type Msg = SheetsMsg;
    type Change = HarkenChange;
    type Route = Route;
    type Request = HarkenRequest;
    type Screen = SheetsPage;

    fn holds(s: &State) -> Vec<HarkenView> {
        match s {
            State::AddTo { track } => match key_of(track) {
                Some(media) => vec![HarkenView::Playlists, HarkenView::PlaylistsOf { media }],
                None => vec![],
            },
            _ => vec![],
        }
    }

    fn update(s: &mut State, msg: SheetsMsg, data: &Data<'_, HarkenView>) -> Vec<Command<Self>> {
        match msg {
            SheetsMsg::AddTo { track } => *s = State::AddTo { track },
            SheetsMsg::Toggle { playlist } => {
                let State::AddTo { track } = s else { return vec![] };
                let (Some(media), Some(list)) = (key_of(track), key_of(&playlist)) else {
                    return vec![];
                };
                let on = rows::<Playlist>(data, &HarkenView::PlaylistsOf { media })
                    .iter()
                    .any(|p| p.id.hex() == playlist);
                let (playlist_id, media_id) = (Id::new(list), Id::new(media));
                return vec![Command::send(match on {
                    true => HarkenChange::RemoveFromPlaylist(RemoveFromPlaylist { playlist_id, media_id }),
                    false => HarkenChange::AddToPlaylist(AddToPlaylist { playlist_id, media_id }),
                })];
            }
            SheetsMsg::Follow { kind } => {
                *s = State::Follow {
                    kind,
                    url: String::new(),
                    refused: None,
                }
            }
            SheetsMsg::Url { url: typed } => {
                if let State::Follow { url, refused, .. } = s {
                    (*url, *refused) = (typed, None);
                }
            }
            SheetsMsg::Subscribe => {
                let State::Follow { kind, url, refused } = s else { return vec![] };
                let change = subscribe(kind, url);
                if let Some(why) = change.problem() {
                    *refused = Some(why);
                    return vec![];
                }
                *s = State::None;
                return vec![Command::send(change)];
            }
            SheetsMsg::Dismiss => *s = State::None,
        }
        vec![]
    }

    fn screen(s: &State, data: &Data<'_, HarkenView>) -> SheetsPage {
        let mut page = SheetsPage {
            adding: false,
            playlists: vec![],
            following: None,
        };
        match s {
            State::None => {}
            State::AddTo { track } => {
                let on: Vec<String> = key_of(track)
                    .map(|media| {
                        rows::<Playlist>(data, &HarkenView::PlaylistsOf { media })
                            .iter()
                            .map(|p| p.id.hex())
                            .collect()
                    })
                    .unwrap_or_default();
                page.adding = true;
                page.playlists = rows::<Playlist>(data, &HarkenView::Playlists)
                    .iter()
                    .map(|p| {
                        let here = on.contains(&p.id.hex());
                        PlaylistChoice {
                            id: p.id.hex(),
                            name: p.name.clone(),
                            on: here,
                            mark: if here { "Added" } else { "" }.into(),
                        }
                    })
                    .collect();
            }
            State::Follow { kind, url, refused } => {
                page.following = Some(FollowForm {
                    title: if kind == "sermon" { "Follow a Series" } else { "Follow a Podcast" }.into(),
                    url: url.clone(),
                    problem: refused.clone(),
                    can_save: subscribe(kind, url).problem().is_none(),
                })
            }
        }
        page
    }
}
