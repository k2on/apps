//! harken in a car (CarPlay and Android Auto through the media player
//! module, ui/docs/app.md "harken's Port"): what the car asks — a branch of
//! the browse tree, a track chosen, tracks added — answered from the
//! library's own views, the ones the screens read, so a car with no signal
//! browses what the device has.
//!
//! **The tree.** The root is the library's kinds: Playlists, Albums,
//! Artists, Podcasts and Sermons. Each opens to its records, and a record
//! (its id the route of its page: `/album/<key>/`) to its tracks.
//!
//! **A track's id says where it was found**, so choosing it plays the list
//! it was in from there, as a tap on the page does: `t|<route>|<n>|<id>`.
//! A record chosen whole plays from its first track.
//!
//! **A view is held before it is read.** A branch whose view the host does
//! not hold yet is held, and answered on the next step, once it has rows.

use ark_app::host::Cx;
use ark_app::modules::media_player::{BrowseItem, MediaEvent, ROOT};
use harken_domain::{AlbumsEntry, ArtistsEntry, HarkenView, LibraryEntry, Playlist, ShowsEntry};

use super::Harken;
use crate::page::tracks_of;
use crate::routes::Route;
use crate::{words, HarkenRequest};

/// The root's branches: the library's kinds, by their id and their title.
pub const KINDS: [(&str, &str); 5] = [
    ("playlists", "Playlists"),
    ("albums", "Albums"),
    ("artists", "Artists"),
    ("podcasts", "Podcasts"),
    ("sermons", "Sermons"),
];

/// What the car asked that is not answered yet, and the views it needs.
#[derive(Default)]
pub struct Car {
    /// Branches asked for whose view was not held yet: answered at the
    /// next step, once the host has held it.
    waiting: Vec<String>,
    /// Every view the car has needed: the host holds them.
    pub held: Vec<HarkenView>,
}

/// A record's id in the tree: its page's route.
fn record(route: &Route) -> String {
    route.url()
}

/// A track's id: the list it was found in, where in it, and itself.
pub fn track_id(list: &str, at: usize, id: &str) -> String {
    format!("t|{list}|{at}|{id}")
}

/// What a track's id says: the list's route, where in it, the track.
fn parse_track(id: &str) -> Option<(String, usize, String)> {
    let mut p = id.strip_prefix("t|")?.rsplitn(3, '|');
    let track = p.next()?.to_string();
    let at = p.next()?.parse().ok()?;
    Some((p.next()?.to_string(), at, track))
}

/// The view a branch is read from.
fn view_of(parent: &str) -> Option<HarkenView> {
    Some(match parent {
        "playlists" => HarkenView::Playlists,
        "albums" => HarkenView::Albums,
        "artists" => HarkenView::Artists,
        "podcasts" | "sermons" => HarkenView::Shows,
        r => tracks_of(&Route::from_url(r)?)?,
    })
}

/// A list's tracks, in its order.
fn tracks(route: &Route, cx: &Cx<'_, Harken>) -> Vec<LibraryEntry> {
    tracks_of(route).map(|v| rows(cx, &v)).unwrap_or_default()
}

fn rows<T: ark::format::Encode>(cx: &Cx<'_, Harken>, key: &HarkenView) -> Vec<T> {
    cx.rows::<Vec<u8>>(key).iter().filter_map(|b| T::from_bytes(b).ok()).collect()
}

fn branch(id: String, title: &str, subtitle: &str, art: String) -> BrowseItem {
    BrowseItem {
        id,
        title: title.into(),
        subtitle: subtitle.into(),
        artwork: (!art.is_empty()).then_some(art),
        playable: true,
        browsable: true,
    }
}

impl Car {
    /// A step: the branches kept waiting answered (their views held since),
    /// then what the car asked now taken — a branch answered from what is
    /// held, or held and kept to answer; a choice played; an add queued.
    /// The answers, each a branch and what is under it.
    pub fn step(&mut self, asks: Vec<MediaEvent>, server: &str, cx: &mut Cx<'_, Harken>) -> Vec<(String, Vec<BrowseItem>)> {
        let mut out: Vec<(String, Vec<BrowseItem>)> = std::mem::take(&mut self.waiting)
            .into_iter()
            .map(|p| {
                let c = Car::children(&p, server, cx);
                (p, c)
            })
            .collect();
        for e in asks {
            if let Some(parent) = self.ask(e, cx) {
                let c = Car::children(&parent, server, cx);
                out.push((parent, c));
            }
        }
        out
    }

    /// One ask taken; a branch that can be answered now, given back.
    fn ask(&mut self, e: MediaEvent, cx: &mut Cx<'_, Harken>) -> Option<String> {
        match e {
            MediaEvent::Browse { parent } => match view_of(&parent) {
                Some(v) if !self.held.contains(&v) => {
                    self.held.push(v);
                    if !self.waiting.contains(&parent) {
                        self.waiting.push(parent);
                    }
                }
                _ => return Some(parent),
            },
            MediaEvent::PlayItem { id } => {
                if let Some(r) = Car::play(&id, cx) {
                    cx.request(r);
                }
            }
            MediaEvent::Add { ids, next } => {
                for (_, _, track) in ids.iter().filter_map(|i| parse_track(i)) {
                    cx.request(HarkenRequest::Queue { track, next });
                }
            }
            _ => {}
        }
        None
    }

    /// The items under `parent`.
    pub fn children(parent: &str, server: &str, cx: &Cx<'_, Harken>) -> Vec<BrowseItem> {
        let art = |a: &str| words::art_url(server, a);
        match parent {
            ROOT => KINDS
                .iter()
                .map(|(id, title)| BrowseItem {
                    id: id.to_string(),
                    title: title.to_string(),
                    browsable: true,
                    ..BrowseItem::default()
                })
                .collect(),
            "playlists" => rows::<Playlist>(cx, &HarkenView::Playlists)
                .iter()
                .map(|p| {
                    let r = Route::Playlist {
                        id: p.id.hex(),
                        name: p.name.clone(),
                    };
                    branch(record(&r), &p.name, "", String::new())
                })
                .collect(),
            "albums" => rows::<AlbumsEntry>(cx, &HarkenView::Albums)
                .iter()
                .map(|a| {
                    let r = Route::Album {
                        key: a.id.clone(),
                        title: a.name.clone(),
                    };
                    let by = if a.performer.is_empty() { &a.creator } else { &a.performer };
                    branch(record(&r), &a.name, by, art(&a.art))
                })
                .collect(),
            "artists" => rows::<ArtistsEntry>(cx, &HarkenView::Artists)
                .iter()
                .map(|a| branch(record(&Route::Artist { name: a.name.clone() }), &a.name, "", art(&a.art)))
                .collect(),
            "podcasts" | "sermons" => {
                let want = if parent == "podcasts" { "podcast" } else { "sermon" };
                rows::<ShowsEntry>(cx, &HarkenView::Shows)
                    .iter()
                    .filter(|s| s.kind == want)
                    .map(|s| {
                        let r = Route::Show {
                            key: s.id.clone(),
                            title: s.title.clone(),
                        };
                        branch(record(&r), &s.title, &s.author, art(&s.art))
                    })
                    .collect()
            }
            r => match Route::from_url(r) {
                Some(route) => tracks(&route, cx)
                    .iter()
                    .enumerate()
                    .map(|(i, e)| BrowseItem {
                        id: track_id(r, i, &e.id.hex()),
                        title: e.title.clone(),
                        subtitle: e.creator.clone(),
                        artwork: None,
                        playable: true,
                        browsable: false,
                    })
                    .collect(),
                None => vec![],
            },
        }
    }

    /// What choosing `id` plays: a track from where it was found (the list
    /// as it is now; a list that has moved finds it by its id), or a record
    /// from its start.
    fn play(id: &str, cx: &Cx<'_, Harken>) -> Option<HarkenRequest> {
        let (list, at, track) = match parse_track(id) {
            Some(t) => t,
            None => (id.to_string(), 0, String::new()),
        };
        let route = Route::from_url(&list)?;
        let all: Vec<String> = tracks(&route, cx).iter().map(|e| e.id.hex()).collect();
        let at = match all.get(at) {
            Some(t) if track.is_empty() || *t == track => at,
            _ => all.iter().position(|t| *t == track).unwrap_or(0),
        };
        (!all.is_empty()).then_some(HarkenRequest::Play {
            tracks: all,
            at: at as i64,
            from: list,
            shuffle: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A track's id comes back as it went, its route's encoded slashes and
    /// all; a branch's id is no track's.
    #[test]
    fn a_track_id_is_read_back() {
        let id = track_id("/album/johann%2Fbwv-988/", 3, "ab12");
        assert_eq!(parse_track(&id), Some(("/album/johann%2Fbwv-988/".into(), 3, "ab12".into())));
        assert_eq!(parse_track("albums"), None);
    }
}
