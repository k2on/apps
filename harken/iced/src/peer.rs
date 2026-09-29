//! A replica, and what is maintained over it: the library as a view, the
//! sidebar, the index pages, and whatever page the sidebar picked.
//!
//! One of these exists for as long as the window does — signed in or not.
//! Whoever is signed in is the replica's business (`ark_client::Peer`
//! authors as them, or as nobody); what is drawn is this.
use std::collections::HashMap;

use ark_client::ark::store::Change;
use ark_client::{args, Args, Changes, Id, Patch, Update, Value};
use harken_domain::view::Item;

use crate::places::{Place, Source};
use crate::rows::{self, Album, Artist, Composer, Playlist, Recording, TrackDetail, Work};

/// One line of the sidebar: somewhere the cursor can be and something it can
/// open.
#[derive(Debug, Clone)]
pub struct Choice {
    pub source: Source,
    pub label: String,
    /// The tally drawn on the right, where there is one to draw.
    pub count: Option<i64>,
}

/// The library, maintained rather than re-read.
///
/// Hydrated once and then told what each change did, so a tap costs the rows
/// that moved instead of the whole list: an `ark_client::View` over the
/// domain's own `library` query, which the engine maintains like every
/// query (`docs/plan-v4.md` §1.5).
struct Maintained {
    view: ark_client::View,
}

impl Maintained {
    fn hydrate(peer: &ark_client::Peer, playlist: Id) -> (Maintained, Vec<Item>) {
        let view = peer
            .view("library", args([("playlist_id", Value::Id(playlist))]))
            .expect("library refuses nothing and faults on nothing");
        let items = view.rows().iter().map(Item::from_value).collect();
        (Maintained { view }, items)
    }

    /// Push what moved through the view and splice the rows it moved; a
    /// reset — a rebase, or the view read again — takes the rows whole.
    fn update(&mut self, peer: &ark_client::Peer, changes: &Changes, items: &mut Vec<Item>) {
        match self.view.update(peer, changes) {
            Ok(Update::Unchanged) => {}
            Ok(Update::Patched(patches)) => splice(items, &patches),
            Ok(Update::Reset) | Err(_) => *items = self.view.rows().iter().map(Item::from_value).collect(),
        }
    }
}

/// The tables a playlist lives in: a change confined to these moves nothing
/// but the lists.
const PLAYLIST_TABLES: [&str; 2] = ["playlist", "playlist_item"];

fn table_of(ch: &Change) -> &str {
    match ch {
        Change::Add(t, _) | Change::Remove(t, _) | Change::Edit(t, _, _) => t,
    }
}

/// The patches, in order, onto the decoded list: a node is the entry
/// `library` answers with, and that entry an [`Item`].
fn splice(items: &mut Vec<Item>, patches: &[Patch]) {
    for p in patches {
        match p {
            Patch::Insert { at, node } => {
                let at = (*at).min(items.len());
                items.insert(at, Item::from_value(node));
            }
            Patch::Remove { at } => {
                if *at < items.len() {
                    items.remove(*at);
                }
            }
            Patch::Update { at, node } => {
                if *at < items.len() {
                    items[*at] = Item::from_value(node);
                }
            }
        }
    }
}

/// A replica, and the lists a window draws from it.
pub struct Peer {
    pub client: ark_client::Peer,
    library: Maintained,
    /// What `view` draws of the library: decoded once per change, not per
    /// frame.
    pub items: Vec<Item>,
    /// The playlist the library is read against, which is what gives every
    /// row its `on_playlist`. The domain has no favorites of its own — a
    /// playlist named Favorites is just the first one. Zero when there is
    /// none, which is what a peer nobody is signed in on has.
    pub playlist: Id,
    /// What the sidebar picked, and the list that answers it.
    ///
    /// `Source::Library` is `items` itself, maintained. Everything else is
    /// one query per change — the maintained view is the list you look at
    /// most, and re-reading one album when something moves is a hundred rows.
    pub source: Source,
    pub shown: Vec<Item>,
    /// Which album each track is on, and the rest of what only a song has: a
    /// map beside the list, because the library row is kind-neutral. Joined
    /// in memory while drawing.
    pub details: HashMap<Id, TrackDetail>,
    /// The sidebar, flat, in the order it is drawn. Headings are derived
    /// while drawing rather than stored, so "line 9" means the same to the
    /// keyboard and to the eye.
    pub choices: Vec<Choice>,
    /// The index pages' contents, read when the library changes — and what a
    /// `#album/…` link resolves against, now that no sidebar line carries one.
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    pub composers: Vec<Composer>,
    /// What the current page shows when it is not a list of tracks: one
    /// composer's works, or one work's recordings (and that work's own row,
    /// for its header).
    pub works: Vec<Work>,
    pub recordings: Vec<Recording>,
    /// Bumped whenever the index lists are rebuilt: the window asks for covers
    /// when this moves, which is the *data* changing — not the wire, which
    /// was wrong both ways (the demo has no wire; a real client's first
    /// library comes out of its own replica).
    pub art_gen: u64,
}

impl Peer {
    /// Everything read once; everything after this is maintained.
    pub fn open(client: ark_client::Peer) -> Peer {
        let playlist = first_playlist(&client);
        let (library, items) = Maintained::hydrate(&client, playlist);
        let mut peer = Peer {
            client,
            library,
            items,
            playlist,
            source: Source::Library,
            shown: Vec::new(),
            details: HashMap::new(),
            choices: Vec::new(),
            albums: Vec::new(),
            artists: Vec::new(),
            composers: Vec::new(),
            works: Vec::new(),
            recordings: Vec::new(),
            art_gen: 0,
        };
        let _ = peer.client.take_changes();
        peer.reload_sidebar();
        peer.reload_shown();
        peer
    }

    /// A peer whose lists are left for the caller to fill: the library view
    /// hydrated and nothing else read. For tests, which copy the demo's lists
    /// rather than read them again per test — the domain's widest queries are
    /// seconds each in a debug build.
    #[cfg(test)]
    pub fn hydrated(client: ark_client::Peer) -> Peer {
        let playlist = first_playlist(&client);
        let (library, items) = Maintained::hydrate(&client, playlist);
        Peer {
            client,
            library,
            items,
            playlist,
            source: Source::Library,
            shown: Vec::new(),
            details: HashMap::new(),
            choices: Vec::new(),
            albums: Vec::new(),
            artists: Vec::new(),
            composers: Vec::new(),
            works: Vec::new(),
            recordings: Vec::new(),
            art_gen: 1,
        }
    }

    /// Make sure there is a playlist to read the library against: make
    /// "Favorites" if this replica has none.
    ///
    /// **When** is the window's question, not this one's (`App::default_playlist`):
    /// signed in, not before the log has been heard, because a later playlist
    /// of the same name comes back renamed — "Favorites (1)" — and every
    /// device making its own on its first run would leave a person one per
    /// device. A refusal is not worth a status line: it only means there is
    /// no playlist yet.
    pub fn ensure_playlist(&mut self) {
        if self.playlists().is_empty() && self.client.mutate("create_playlist", args([("name", Value::text("Favorites"))])).is_ok() {
            self.refresh();
        }
    }

    /// Bring everything up to date with whatever just happened. `true` when
    /// anything did.
    ///
    /// The whole library is never read here: `Applied` is the rows that moved
    /// and `Rebuilt` — a rebase rolled the optimistic store back — is a
    /// re-hydrate, which is the one thing no list of changes describes.
    pub fn refresh(&mut self) -> bool {
        let changes = self.client.take_changes();
        let lists_only = match &changes {
            Changes::Applied(chs) if chs.is_empty() => return false,
            Changes::Applied(chs) => chs.iter().all(|c| PLAYLIST_TABLES.contains(&table_of(c))),
            Changes::Rebuilt => false,
        };
        self.library.update(&self.client, &changes, &mut self.items);
        // What moved decides what is read again. A playlist toggle — the one
        // change a person makes here — touches no album, artist, composer or
        // song, and re-reading those is most of the cost of a change: they
        // are the domain's widest queries. So a change to the lists alone
        // re-reads the lists alone.
        match lists_only {
            true => self.reload_playlists(),
            false => self.reload_sidebar(),
        }
        self.reload_shown();
        true
    }

    /// The sidebar's playlists, read again, with everything else as it was.
    fn reload_playlists(&mut self) {
        let playlists = self.playlists();
        if !playlists.iter().any(|p| p.id == self.playlist) {
            // The list the view reads against went: the whole reload knows
            // how to re-point it.
            return self.reload_sidebar();
        }
        self.choices.retain(|c| !matches!(c.source, Source::Playlist(..)));
        self.choices.extend(playlists.into_iter().map(|p| Choice {
            source: Source::Playlist(p.id, p.name.clone()),
            label: p.name,
            count: None,
        }));
    }

    /// Run a query and read its rows; a refusal (a guard, nobody signed in)
    /// is no rows.
    fn ask<T>(&self, name: &str, a: Args, f: impl Fn(&Value) -> T) -> Vec<T> {
        match self.client.query(name, &a) {
            Ok(v) => rows::list(&v, f),
            Err(_) => Vec::new(),
        }
    }

    pub fn playlists(&self) -> Vec<Playlist> {
        self.ask("playlists", args([]), Playlist::from_value)
    }

    /// Which of the caller's playlists a track is on.
    pub fn playlists_of(&self, media: Id) -> Vec<Playlist> {
        self.ask("playlists_of", args([("media_id", Value::Id(media))]), Playlist::from_value)
    }

    fn read_items(&self, name: &str, a: Args) -> Vec<Item> {
        self.ask(name, a, Item::from_value)
    }

    pub fn work(&self, id: &str) -> Vec<Work> {
        self.ask("work", args([("id", Value::text(id))]), Work::from_value)
    }

    pub fn works_of(&self, composer: &str) -> Vec<Work> {
        self.ask("works", args([("composer", Value::text(composer))]), Work::from_value)
    }

    pub fn recordings_of(&self, work: &str) -> Vec<Recording> {
        self.ask("recordings", args([("work_id", Value::text(work))]), Recording::from_value)
    }

    pub fn credits(&self, recording: &str) -> Vec<rows::Credit> {
        self.ask("credits", args([("recording_id", Value::text(recording))]), rows::Credit::from_value)
    }

    pub fn album_rows(&self, name: &str) -> Vec<Item> {
        self.read_items("album", args([("playlist_id", Value::Id(self.playlist)), ("name", Value::text(name))]))
    }

    pub fn track_details(&self) -> Vec<TrackDetail> {
        self.ask("track_details", args([]), TrackDetail::from_value)
    }

    /// The sidebar's lists, read back: when something changed, never when
    /// something is drawn. Grouping is the domain's, so every client folds the
    /// library the same way.
    pub fn reload_sidebar(&mut self) {
        let playlists = self.playlists();
        let albums = self.ask("albums", args([]), Album::from_value);
        let artists = self.ask("artists", args([]), Artist::from_value);
        let composers = self.ask("composers", args([]), Composer::from_value);

        // The playlist the view is read against can stop existing, and
        // routinely does: the default this peer made on its first run is
        // dropped on the rebase when another device's turns out to have been
        // first. Re-pointing costs a hydrate, so only when it moved.
        let first = playlists.first().map_or([0; 16], |p| p.id);
        if !playlists.iter().any(|p| p.id == self.playlist) && self.playlist != first {
            self.playlist = first;
            let (library, items) = Maintained::hydrate(&self.client, first);
            self.library = library;
            self.items = items;
        }

        self.details = self.track_details().into_iter().map(|t| (t.media_id, t)).collect();

        // Music, then the lists somebody made. **A line is drawn only when it
        // has rows behind it**, which is what lets one schema serve every
        // genre: a pop library has no works, so no Composers line. Songs is
        // unconditional because it is the library.
        let mut choices = vec![Choice {
            source: Source::Library,
            label: "Songs".into(),
            count: Some(self.items.len() as i64),
        }];
        for (source, label, count) in [
            (Source::Albums, "Albums", albums.len()),
            (Source::Artists, "Artists", artists.len()),
            (Source::Composers, "Composers", composers.len()),
        ] {
            if count > 0 {
                choices.push(Choice {
                    source,
                    label: label.into(),
                    count: Some(count as i64),
                });
            }
        }
        choices.extend(playlists.into_iter().map(|p| Choice {
            source: Source::Playlist(p.id, p.name.clone()),
            label: p.name,
            count: None,
        }));
        self.choices = choices;
        self.albums = albums;
        self.artists = artists;
        self.composers = composers;
        self.art_gen = self.art_gen.wrapping_add(1);
    }

    /// What is true of a track as a song, or nothing if its kind has none.
    pub fn detail_of(&self, id: Id) -> TrackDetail {
        self.details.get(&id).cloned().unwrap_or(TrackDetail {
            media_id: id,
            ..TrackDetail::default()
        })
    }

    /// The list under the header, for whatever the sidebar picked. The
    /// library is maintained and costs nothing here; the others are a query,
    /// because a selection is a click and that is the place to pay.
    pub fn reload_shown(&mut self) {
        // Cleared first, so a page that is neither cannot show the last one's.
        self.works.clear();
        self.recordings.clear();
        let with_playlist = |p: &Peer, name: &str, key: &str, value: &str| {
            p.read_items(name, args([("playlist_id", Value::Id(p.playlist)), (key, Value::text(value))]))
        };
        self.shown = match self.source.clone() {
            Source::Library => Vec::new(),
            Source::Albums | Source::Artists | Source::Composers => Vec::new(),
            Source::Works(composer) => {
                self.works = self.works_of(&composer);
                Vec::new()
            }
            // Both halves: the work itself, for the header, and its
            // performances. This page may have been reached without opening a
            // composer at all — a link somebody sent — so the row is read by
            // key rather than looked up in a composer's list.
            Source::Work(id, _) => {
                self.works = self.work(&id);
                self.recordings = self.recordings_of(&id);
                Vec::new()
            }
            Source::Playlist(id, _) => self.read_items("playlist", args([("playlist_id", Value::Id(id))])),
            Source::Album(name) => self.album_rows(&name),
            Source::Artist(name) => with_playlist(self, "artist", "name", &name),
            // The one list ordered by the *work* rather than the release.
            Source::Recording(id, _) => {
                // Every recording of the same work, so the header can find
                // this one — the work's key is everything before the `@`.
                if let Some((work, _)) = id.split_once('@') {
                    self.works = self.work(work);
                    self.recordings = self.recordings_of(work);
                }
                // A link arriving cold carries the key and no label: the row
                // knows who played it, and this is where the two meet.
                if let Source::Recording(id, who) = &mut self.source {
                    if who.is_empty() {
                        if let Some(take) = self.recordings.iter().find(|r| r.id == *id) {
                            who.clone_from(&take.performers);
                        }
                    }
                }
                with_playlist(self, "recording", "id", &id)
            }
        };
    }

    /// A work's title by its key, or `None` if this peer has no such work. A
    /// read rather than a lookup in `works`, which is one composer's: a
    /// `#work/…` link can arrive before any composer has been opened.
    pub fn work_named(&self, id: &str) -> Option<String> {
        self.work(id).into_iter().next().map(|w| w.title)
    }

    /// The source a place names, against what this peer actually has. A link
    /// to a playlist since renamed, or to an album not received yet, lands on
    /// the library — a page, rather than a heading with nothing under it.
    pub fn source_of(&self, place: &Place) -> Source {
        match place {
            Place::Album(name) => match self.albums.iter().any(|a| a.name == *name) {
                true => Source::Album(name.clone()),
                false => Source::Library,
            },
            Place::Artist(name) => match self.artists.iter().any(|a| a.name == *name) {
                true => Source::Artist(name.clone()),
                false => Source::Library,
            },
            Place::Composer(name) => match self.composers.iter().any(|c| c.name == *name) {
                true => Source::Works(name.clone()),
                false => Source::Library,
            },
            // By *querying*, not against a list this peer holds: a link
            // arriving cold has no page yet, and the key is enough to ask with.
            Place::Work(id) => match self.work_named(id) {
                Some(title) => Source::Work(id.clone(), title),
                None => Source::Library,
            },
            Place::Recording(id) => match id.split_once('@').and_then(|(work, _)| self.work_named(work)) {
                Some(_) => Source::Recording(id.clone(), String::new()),
                None => Source::Library,
            },
            _ => self
                .choices
                .iter()
                .find(|c| c.source.place() == *place)
                .map(|c| c.source.clone())
                .unwrap_or(Source::Library),
        }
    }

    /// What the table is showing, whichever side it came from.
    pub fn rows(&self) -> &[Item] {
        match self.source {
            Source::Library => &self.items,
            _ => &self.shown,
        }
    }

    /// Every album this artist made something on: `Album::creator` is whoever
    /// made its first track, the same fact an artist page selects on.
    pub fn albums_by(&self, artist: &str) -> Vec<&Album> {
        self.albums.iter().filter(|a| a.creator == artist).collect()
    }
}

/// The playlist a fresh peer reads its library against: the first of the
/// caller's, or none.
fn first_playlist(client: &ark_client::Peer) -> Id {
    match client.query("playlists", &args([])) {
        Ok(v) => rows::list(&v, Playlist::from_value).first().map_or([0; 16], |p| p.id),
        Err(_) => [0; 16],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_client::{Domain, Options};

    fn song(title: &str, artist: &str, album: &str, file: &str) -> Args {
        let t = |s: &str| Value::text(s);
        args([
            ("title", t(title)),
            ("artist", t(artist)),
            ("album", t(album)),
            ("duration_ms", Value::Int(1000)),
            ("file", t(file)),
            ("track", Value::Int(0)),
            ("part", t("")),
            ("catalogue", t("")),
            ("performer", t("")),
            ("bpm", Value::Int(0)),
            ("album_art", t("")),
            ("artist_art", t("")),
            ("disc", Value::Int(0)),
            ("work_title", t("")),
            ("movement_no", Value::Int(0)),
        ])
    }

    /// The maintained list is the query, after every step — one change, a
    /// batch of them, and a playlist toggle that moves `playlist_pos`.
    ///
    /// Falsified by dropping the `Update` arm of `splice`: the toggle — which
    /// arrives as an update to the row it moved — leaves the old
    /// `playlist_pos`. A batch is pushed against the final store, once per
    /// touched entry (`docs/plan-v4.md` §1.5).
    #[test]
    fn the_maintained_library_is_the_query() {
        let domain = Domain::new(&harken_domain::module());
        let client = ark_client::Peer::open_memory(domain, Options::alone("me")).unwrap();
        let mut peer = Peer::open(client);
        peer.ensure_playlist();
        assert_ne!(peer.playlist, [0; 16], "Favorites was made");
        let read = |p: &Peer| p.read_items("library", args([("playlist_id", Value::Id(p.playlist))]));

        peer.client.mutate("add_song", song("Air", "Bach", "Suites", "a")).unwrap();
        assert!(peer.refresh());
        assert_eq!(peer.items, read(&peer));

        for (t, f) in [("Glue", "b"), ("Opal", "c"), ("Gosh", "d")] {
            peer.client.mutate("add_song", song(t, "X", "Y", f)).unwrap();
        }
        peer.refresh();
        assert_eq!(peer.items, read(&peer), "a batch, each change against its own store");

        let id = peer.items[2].id;
        peer.client
            .mutate(
                "add_to_playlist",
                args([("playlist_id", Value::Id(peer.playlist)), ("media_id", Value::Id(id))]),
            )
            .unwrap();
        peer.refresh();
        assert_eq!(peer.items, read(&peer));
        assert!(peer.items[2].on_playlist());
        assert!(!peer.refresh(), "nothing moved, nothing read");

        // A song and its place on the playlist in one batch: the song's entry
        // is built once, against the final store, already carrying its place.
        peer.client.mutate("add_song", song("Late", "Z", "", "e")).unwrap();
        let late = peer.client.query("library", &args([("playlist_id", Value::Id(peer.playlist))])).unwrap();
        let late = rows::list(&late, Item::from_value).last().unwrap().id;
        peer.client
            .mutate(
                "add_to_playlist",
                args([("playlist_id", Value::Id(peer.playlist)), ("media_id", Value::Id(late))]),
            )
            .unwrap();
        peer.refresh();
        assert_eq!(peer.items, read(&peer), "a song and its playlist entry in one batch");
    }
}
