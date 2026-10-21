//! A replica, and the lists a window draws from it — every one of them a
//! maintained view the client holds (§1.10), told what moved.
//!
//! One of these exists for as long as the window does — signed in or not.
//! Whoever is signed in is the replica's business (`client::Replica`
//! authors as them, or as nobody); what is drawn is this.
//!
//! **Nothing here reads a list again.** Every query is a view the client
//! keeps up to date (`docs/plan-v5.md` §1.10), so the peer holds a small
//! set of them keyed by query and input — the library, the sidebar's four
//! lists, the song details the table joins in, whatever the page shows, and
//! the playlists the picker asked about — and every change the client
//! makes or receives is pushed into each as it happens; [`Peer::refresh`]
//! takes what each one patched and splices it into the rows the screens
//! hold, or takes the list whole when the view could not be kept up or was
//! made for somebody else. A change costs each view the entries it touched,
//! so a playlist toggle no longer has to be told apart from a song arriving
//! to keep the sidebar cheap: the albums view is not touched by a toggle.
use std::collections::HashMap;

use crate::domain::Id as Key;
use crate::domain::{browse, CreatePlaylist, PlaylistInput, PlaylistsOf};
use crate::domain::{AlbumInput, Artist as ArtistInput, Library, RecordingInput, Recordings, Search, WorkInput, Works};
use ark::json::Json;
use ark_peer::Patch;

use crate::client::{Replica, Update};
use crate::places::{Place, Source};
use crate::rows::{Album, Artist, Composer, Id, Item, Playlist, Recording, TrackDetail, Work};

/// One line of the sidebar: somewhere the cursor can be and something it can
/// open.
#[derive(Debug, Clone)]
pub struct Choice {
    pub source: Source,
    pub label: String,
    /// The tally drawn on the right, where there is one to draw.
    pub count: Option<i64>,
}

/// No playlist: what the library is read against before there is one.
const NONE: Id = [0; 16];

/// An id as the domain's input types carry it.
pub fn key<T>(id: Id) -> Key<T> {
    Key::new(id)
}

/// One query held open: its name and input — the key a page's views are
/// kept or replaced by — who it was opened for, and the client's view of
/// it.
struct Open {
    name: &'static str,
    input: Vec<u8>,
    who: ark::peer::Who,
    /// The view, or — while its query refuses (somebody else's playlist,
    /// nobody signed in) — none, which is no rows.
    view: Option<usize>,
}

impl Open {
    /// Hydrate once. A refusal is no rows rather than an error: a page that
    /// is not yours is an empty page, as the query's answer was.
    fn hydrate(client: &mut Replica, name: &'static str, input: Vec<u8>) -> Open {
        let view = client.view(name, &input).ok();
        Open {
            name,
            input,
            who: client.me().clone(),
            view,
        }
    }

    fn is(&self, name: &str, input: &[u8]) -> bool {
        self.name == name && self.input == input
    }

    /// Held no more.
    fn close(self, client: &mut Replica) {
        if let Some(v) = self.view {
            client.unwatch(v);
        }
    }

    /// The list as the view holds it, each row its bytes.
    fn values(&self, client: &Replica) -> Vec<Vec<u8>> {
        self.view.map(|v| client.rows(v)).unwrap_or_default()
    }

    /// What one settle did to it. A view that faults is read again whole,
    /// which is what a reset is; so is one opened for somebody else; one
    /// refused at open is asked again when anything moved — the query it
    /// stands for would have been.
    fn update(&mut self, client: &mut Replica, moved: bool) -> Update {
        match self.view {
            Some(v) if self.who == *client.me() => match client.update(v) {
                Update::Reset => {
                    self.reopen(client);
                    Update::Reset
                }
                u => u,
            },
            Some(_) => {
                self.reopen(client);
                Update::Reset
            }
            None if !moved && self.who == *client.me() => Update::Unchanged,
            None => {
                self.reopen(client);
                match self.view {
                    Some(_) => Update::Reset,
                    None => Update::Unchanged,
                }
            }
        }
    }

    fn reopen(&mut self, client: &mut Replica) {
        let input = std::mem::take(&mut self.input);
        let old = std::mem::replace(self, Open::hydrate(client, self.name, input));
        old.close(client);
    }

    /// Hydrate into `rows`, decoded by `read`.
    fn fill<T>(&self, client: &Replica, rows: &mut Vec<T>, read: fn(&[u8]) -> T) {
        *rows = self.values(client).iter().map(|b| read(b)).collect();
    }

    /// [`Open::update`], onto the rows a screen holds: what moved, if
    /// anything did.
    fn refresh<T>(&mut self, client: &mut Replica, moved: bool, rows: &mut Vec<T>, read: fn(&[u8]) -> T) -> Option<Moved> {
        match self.update(client, moved) {
            Update::Unchanged => None,
            Update::Patched(patches) => {
                splice(rows, &patches, read);
                Some(Moved::Patched(patches.len()))
            }
            Update::Reset => {
                self.fill(client, rows, read);
                Some(Moved::Reset)
            }
        }
    }
}

/// What one refresh did to one list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moved {
    /// This many patches, spliced.
    Patched(usize),
    /// Taken whole.
    Reset,
}

/// The patches, in order, onto the decoded list: `ark::plan::splice` with
/// the decoding said, because a row here is a type of this crate's.
fn splice<T>(rows: &mut Vec<T>, patches: &[Patch], read: fn(&[u8]) -> T) {
    for p in patches {
        match p {
            Patch::Insert { at, node } => {
                let at = (*at).min(rows.len());
                rows.insert(at, read(node));
            }
            Patch::Remove { at } => {
                if *at < rows.len() {
                    rows.remove(*at);
                }
            }
            Patch::Update { at, node } => {
                if *at < rows.len() {
                    rows[*at] = read(node);
                }
            }
        }
    }
}

/// The views the peer holds open. The first six live as long as the window;
/// the page's are whichever its source needs, opened when the source changes
/// and dropped with it; the picker's is the last track it was asked about.
struct Views {
    library: Open,
    playlists: Open,
    albums: Open,
    artists: Open,
    composers: Open,
    details: Open,
    /// The page's list of tracks (`playlist`, `album`, `artist`,
    /// `recording`).
    shown: Option<Open>,
    /// The page's works (`works`, or one `work` for a header).
    works: Option<Open>,
    /// The page's performances (`recordings`).
    recordings: Option<Open>,
    /// Which playlists one track is on (`playlists_of`).
    on: Option<Open>,
}

/// A page slot, brought to what the source wants: kept when it is already
/// that query with that input, opened when it is not, dropped (and its rows
/// cleared) when the page wants nothing there.
fn want<T>(client: &mut Replica, slot: &mut Option<Open>, wanted: Option<(&'static str, Vec<u8>)>, rows: &mut Vec<T>, read: fn(&[u8]) -> T) -> bool {
    match wanted {
        None => {
            let had = slot.take().map(|o| o.close(client)).is_some();
            rows.clear();
            had
        }
        Some((name, a)) if slot.as_ref().is_some_and(|o| o.is(name, &a)) => false,
        Some((name, a)) => {
            let open = Open::hydrate(client, name, a);
            open.fill(client, rows, read);
            if let Some(old) = slot.replace(open) {
                old.close(client);
            }
            true
        }
    }
}

/// A page slot's settle, when the page has one there.
fn settle<T>(client: &mut Replica, slot: &mut Option<Open>, moved: bool, rows: &mut Vec<T>, read: fn(&[u8]) -> T) -> Option<Moved> {
    slot.as_mut().and_then(|o| o.refresh(client, moved, rows, read))
}

/// A replica, and the lists a window draws from it.
pub struct Peer {
    pub client: Replica,
    views: Views,
    /// What `view` draws of the library: decoded once per change, not per
    /// frame.
    pub items: Vec<Item>,
    /// The playlist the library is read against, which is what gives every
    /// row its `on_playlist`. The domain has no favorites of its own — a
    /// playlist named Favorites is just the first one. Zero when there is
    /// none, which is what a peer nobody is signed in on has.
    pub playlist: Id,
    /// The caller's playlists, in their order: the sidebar's last lines, and
    /// the picker's rows.
    playlists: Vec<Playlist>,
    /// What the sidebar picked, and the list that answers it.
    ///
    /// `Source::Library` is `items` itself. Every other list of tracks is the
    /// page's own view, opened when the source changes.
    pub source: Source,
    /// What the Songs page is narrowed to, as the search box has it typed:
    /// `None` is the whole library. While there is one, the page's list is
    /// the `search` view (`docs/plan-db.md` D4) rather than `items`.
    pub search: Option<String>,
    pub shown: Vec<Item>,
    /// Which album each track is on, and the rest of what only a song has: a
    /// map beside the list, because the library row is kind-neutral. Joined
    /// in memory while drawing.
    pub details: HashMap<Id, TrackDetail>,
    /// `track_details` in the view's order: what a patch's position means,
    /// so the map can be moved by the same patches.
    detail_rows: Vec<TrackDetail>,
    /// The sidebar, flat, in the order it is drawn. Headings are derived
    /// while drawing rather than stored, so "line 9" means the same to the
    /// keyboard and to the eye.
    pub choices: Vec<Choice>,
    /// The index pages' contents — and what a `#album/…` link resolves
    /// against, now that no sidebar line carries one.
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    pub composers: Vec<Composer>,
    /// What the current page shows when it is not a list of tracks: one
    /// composer's works, or one work's recordings (and that work's own row,
    /// for its header).
    pub works: Vec<Work>,
    pub recordings: Vec<Recording>,
    /// `playlists_of` for the track the picker was last opened on.
    on: Vec<Playlist>,
    /// What the last [`Peer::refresh`] moved, by query: the one place to
    /// see that a change cost the lists it touched and no others.
    pub moved: Vec<(&'static str, Moved)>,
    /// Bumped whenever a list that carries covers moved: the window asks for
    /// covers when this moves, which is the *data* changing.
    pub art_gen: u64,
    /// Bumped whenever the store moved at all — its state hash is not what
    /// the last refresh saw. The explorer diffs what it shows against what
    /// it last drew when this moves, and not on an update where nothing did.
    pub store_gen: u64,
    seen: ark::hash::StateHash,
}

/// `()`'s bytes: a query with no input.
fn none() -> Vec<u8> {
    ().to_bytes()
}

fn library_of(playlist: Id) -> Vec<u8> {
    Library { playlist_id: key(playlist) }.to_bytes()
}

impl Peer {
    /// Every list the window starts on, hydrated once; everything after this
    /// is maintained.
    pub fn open(mut client: Replica) -> Peer {
        let playlists = Open::hydrate(&mut client, "playlists", none());
        let mut rows = Vec::new();
        playlists.fill(&client, &mut rows, Playlist::from_bytes);
        let playlist = rows.first().map_or(NONE, |p: &Playlist| p.id);
        let views = Views {
            library: Open::hydrate(&mut client, "library", library_of(playlist)),
            playlists,
            albums: Open::hydrate(&mut client, "albums", none()),
            artists: Open::hydrate(&mut client, "artists", none()),
            composers: Open::hydrate(&mut client, "composers", none()),
            details: Open::hydrate(&mut client, "track_details", none()),
            shown: None,
            works: None,
            recordings: None,
            on: None,
        };
        let seen = client.state();
        let mut peer = Peer {
            client,
            views,
            items: Vec::new(),
            playlist,
            playlists: rows,
            source: Source::Library,
            search: None,
            shown: Vec::new(),
            details: HashMap::new(),
            detail_rows: Vec::new(),
            choices: Vec::new(),
            albums: Vec::new(),
            artists: Vec::new(),
            composers: Vec::new(),
            works: Vec::new(),
            recordings: Vec::new(),
            on: Vec::new(),
            moved: Vec::new(),
            art_gen: 1,
            store_gen: 1,
            seen,
        };
        let (c, v) = (&peer.client, &peer.views);
        v.library.fill(c, &mut peer.items, Item::from_bytes);
        v.albums.fill(c, &mut peer.albums, Album::from_bytes);
        v.artists.fill(c, &mut peer.artists, Artist::from_bytes);
        v.composers.fill(c, &mut peer.composers, Composer::from_bytes);
        v.details.fill(c, &mut peer.detail_rows, TrackDetail::from_bytes);
        peer.details = peer.detail_rows.iter().map(|t| (t.media_id, t.clone())).collect();
        // What the hydrate already read is not news.
        for v in [
            &peer.views.library,
            &peer.views.playlists,
            &peer.views.albums,
            &peer.views.artists,
            &peer.views.composers,
            &peer.views.details,
        ] {
            if let Some(v) = v.view {
                peer.client.update(v);
            }
        }
        peer.choose();
        peer
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
        let made = CreatePlaylist { name: "Favorites".into() };
        if self.playlists.is_empty() && self.client.mutate("create_playlist", &made).is_ok() {
            self.refresh();
        }
    }

    /// Hand what this peer did alone to the server at `url`
    /// (`docs/plan-alone.md` §4), in place: the replica goes back to its
    /// fork and its local history is pending again, and every open list is
    /// told that as changes — none, so the library does not blink. With no
    /// login the history stays nobody's until a sign-in makes it the
    /// signer's.
    pub fn join(&mut self, url: &str) {
        self.client.join();
        self.client.connect(url);
        self.refresh();
    }

    /// Bring every list up to date with whatever just happened. `true` when
    /// any of them moved.
    ///
    /// Nothing is read again here: each open view has been handed the
    /// client's changes as they happened (§1.10) and answers with the
    /// patches they made. A rebase is changes too: the inverse of what it
    /// undid, what landed, what it re-applied. A view the client dropped,
    /// or one opened for somebody else, is read whole again.
    pub fn refresh(&mut self) -> bool {
        let state = self.client.state();
        let moved = state != self.seen;
        if moved {
            self.seen = state;
            self.store_gen += 1;
        }
        let (c, v) = (&mut self.client, &mut self.views);
        let mut changed = Vec::new();
        let mut note = |name: &'static str, m: Option<Moved>| match m {
            Some(m) => {
                changed.push((name, m));
                true
            }
            None => false,
        };
        let lists = note("playlists", v.playlists.refresh(c, moved, &mut self.playlists, Playlist::from_bytes));
        let library = note("library", v.library.refresh(c, moved, &mut self.items, Item::from_bytes));
        let albums = note("albums", v.albums.refresh(c, moved, &mut self.albums, Album::from_bytes));
        let artists = note("artists", v.artists.refresh(c, moved, &mut self.artists, Artist::from_bytes));
        let composers = note("composers", v.composers.refresh(c, moved, &mut self.composers, Composer::from_bytes));
        let details = match v.details.update(c, moved) {
            Update::Unchanged => None,
            Update::Patched(patches) => {
                patch_details(&mut self.detail_rows, &mut self.details, &patches);
                Some(Moved::Patched(patches.len()))
            }
            Update::Reset => {
                v.details.fill(c, &mut self.detail_rows, TrackDetail::from_bytes);
                self.details = self.detail_rows.iter().map(|t| (t.media_id, t.clone())).collect();
                Some(Moved::Reset)
            }
        };
        let details = note("track_details", details);
        let name = |slot: &Option<Open>| slot.as_ref().map_or("", |o| o.name);
        let shown = note(name(&v.shown), settle(c, &mut v.shown, moved, &mut self.shown, Item::from_bytes));
        let works = note(name(&v.works), settle(c, &mut v.works, moved, &mut self.works, Work::from_bytes));
        let recordings = note(
            name(&v.recordings),
            settle(c, &mut v.recordings, moved, &mut self.recordings, Recording::from_bytes),
        );
        let on = note(name(&v.on), settle(c, &mut v.on, moved, &mut self.on, Playlist::from_bytes));
        self.moved = changed;

        // The playlist the library is read against can stop existing, and
        // routinely does: the default this peer made on its first run is
        // dropped on the rebase when another device's turns out to have been
        // first. Re-pointing costs a hydrate of the library and of a page
        // read against it, so only when it moved.
        let first = self.playlists.first().map_or(NONE, |p| p.id);
        let repointed = !self.playlists.iter().any(|p| p.id == self.playlist) && self.playlist != first;
        if repointed {
            self.playlist = first;
            let old = std::mem::replace(&mut self.views.library, Open::hydrate(&mut self.client, "library", library_of(first)));
            old.close(&mut self.client);
            self.views.library.fill(&self.client, &mut self.items, Item::from_bytes);
            self.open_page();
        }
        if lists || library || albums || artists || composers || repointed {
            self.choose();
        }
        if albums || artists || composers || works {
            self.art_gen = self.art_gen.wrapping_add(1);
        }
        if recordings {
            self.label_recording();
        }
        lists || library || albums || artists || composers || details || shown || works || recordings || on || repointed
    }

    /// The sidebar, from the lists it counts. Grouping is the domain's, so
    /// every client folds the library the same way.
    fn choose(&mut self) {
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
            (Source::Albums, "Albums", self.albums.len()),
            (Source::Artists, "Artists", self.artists.len()),
            (Source::Composers, "Composers", self.composers.len()),
        ] {
            if count > 0 {
                choices.push(Choice {
                    source,
                    label: label.into(),
                    count: Some(count as i64),
                });
            }
        }
        choices.extend(self.playlists.iter().map(|p| Choice {
            source: Source::Playlist(p.id, p.name.clone()),
            label: p.name.clone(),
            count: None,
        }));
        self.choices = choices;
    }

    /// The caller's playlists, as the sidebar holds them.
    pub fn playlists(&self) -> &[Playlist] {
        &self.playlists
    }

    /// Which of the caller's playlists a track is on: a view kept open on
    /// the last track asked about, so the picker reopened on the same track —
    /// after it made a playlist — reads nothing.
    pub fn playlists_of(&mut self, media: Id) -> Vec<Playlist> {
        let wanted = Some(("playlists_of", PlaylistsOf { media_id: key(media) }.to_bytes()));
        want(&mut self.client, &mut self.views.on, wanted, &mut self.on, Playlist::from_bytes);
        self.on.clone()
    }

    /// A query's rows, asked once through its stub; a refusal (a guard,
    /// nobody signed in) is no rows. For what is not a list on screen:
    /// resolving a link, and the tests, which hold the maintained lists to
    /// what the query answers.
    pub(crate) fn rows_of<E, T: From<E>>(answer: Result<Vec<E>, String>) -> Vec<T> {
        answer.map_or_else(|_| vec![], |es| es.into_iter().map(T::from).collect())
    }

    pub fn work(&self, id: &str) -> Vec<Work> {
        Peer::rows_of(self.client.answer(browse::work, WorkInput { id: id.into() }))
    }

    /// What a page's query answers, read once. The page holds views; these
    /// are what the tests compare them with.
    #[cfg(test)]
    pub fn works_of(&self, composer: &str) -> Vec<Work> {
        Peer::rows_of(self.client.answer(browse::works, Works { composer: composer.into() }))
    }

    #[cfg(test)]
    pub fn recordings_of(&self, work: &str) -> Vec<Recording> {
        Peer::rows_of(self.client.answer(browse::recordings, Recordings { work_id: work.into() }))
    }

    #[cfg(test)]
    pub fn credits(&self, recording: &str) -> Vec<crate::domain::CreditsEntry> {
        let input = crate::domain::Credits {
            recording_id: recording.into(),
        };
        self.client.answer(browse::credits, input).unwrap_or_default()
    }

    #[cfg(test)]
    pub fn album_rows(&self, name: &str) -> Vec<Item> {
        let input = AlbumInput {
            playlist_id: key(self.playlist),
            name: name.into(),
        };
        Peer::rows_of(self.client.answer(browse::album, input))
    }

    #[cfg(test)]
    pub fn track_details(&self) -> Vec<TrackDetail> {
        Peer::rows_of(self.client.answer(browse::track_details, ()))
    }

    /// The library as its query answers it now, against the playlist it is
    /// read against.
    #[cfg(test)]
    pub fn library_now(&self) -> Vec<Item> {
        Peer::rows_of(self.client.answer(
            browse::library,
            Library {
                playlist_id: key(self.playlist),
            },
        ))
    }

    /// What is true of a track as a song, or nothing if its kind has none.
    pub fn detail_of(&self, id: Id) -> TrackDetail {
        self.details.get(&id).cloned().unwrap_or(TrackDetail {
            media_id: id,
            ..TrackDetail::default()
        })
    }

    /// Open the views the page needs for whatever the sidebar picked, and
    /// drop the last page's. A view the new page shares with the old one —
    /// the same query with the same input, a work's recordings from its
    /// work page to one of them — is kept rather than read again.
    pub fn open_page(&mut self) {
        let pl = key(self.playlist);
        type Wanted = Option<(&'static str, Vec<u8>)>;
        let (shown, works, recordings): (Wanted, Wanted, Wanted) = match &self.source {
            // The library is `items` itself, unless a search narrows it: then
            // the page is the `search` view, kept like any other
            // (`docs/plan-db.md` D4).
            Source::Library => (
                self.search.as_deref().map(|n| {
                    let s = Search {
                        playlist_id: pl,
                        needle: n.into(),
                    };
                    ("search", s.to_bytes())
                }),
                None,
                None,
            ),
            Source::Albums | Source::Artists | Source::Composers => (None, None, None),
            Source::Works(composer) => (None, Some(("works", Works { composer: composer.clone() }.to_bytes())), None),
            // Both halves: the work itself, for the header, and its
            // performances. This page may have been reached without opening a
            // composer at all — a link somebody sent — so the row is read by
            // key rather than looked up in a composer's list.
            Source::Work(id, _) => (
                None,
                Some(("work", WorkInput { id: id.clone() }.to_bytes())),
                Some(("recordings", Recordings { work_id: id.clone() }.to_bytes())),
            ),
            Source::Playlist(id, _) => (Some(("playlist", PlaylistInput { playlist_id: key(*id) }.to_bytes())), None, None),
            Source::Album(name) => (
                Some((
                    "album",
                    AlbumInput {
                        playlist_id: pl,
                        name: name.clone(),
                    }
                    .to_bytes(),
                )),
                None,
                None,
            ),
            Source::Artist(name) => (
                Some((
                    "artist",
                    ArtistInput {
                        playlist_id: pl,
                        name: name.clone(),
                    }
                    .to_bytes(),
                )),
                None,
                None,
            ),
            // The one list ordered by the *work* rather than the release, and
            // every recording of that work, so the header can find this one —
            // the work's key is everything before the `@`.
            Source::Recording(id, _) => {
                let shown = Some((
                    "recording",
                    RecordingInput {
                        playlist_id: pl,
                        id: id.clone(),
                    }
                    .to_bytes(),
                ));
                match id.split_once('@') {
                    Some((work, _)) => (
                        shown,
                        Some(("work", WorkInput { id: work.into() }.to_bytes())),
                        Some(("recordings", Recordings { work_id: work.into() }.to_bytes())),
                    ),
                    None => (shown, None, None),
                }
            }
        };
        let (c, v) = (&mut self.client, &mut self.views);
        want(c, &mut v.shown, shown, &mut self.shown, Item::from_bytes);
        let works = want(c, &mut v.works, works, &mut self.works, Work::from_bytes);
        want(c, &mut v.recordings, recordings, &mut self.recordings, Recording::from_bytes);
        if works {
            self.art_gen = self.art_gen.wrapping_add(1);
        }
        self.label_recording();
    }

    /// A link arriving cold carries a recording's key and no label: the row
    /// knows who played it, and this is where the two meet.
    fn label_recording(&mut self) {
        if let Source::Recording(id, who) = &mut self.source {
            if who.is_empty() {
                if let Some(take) = self.recordings.iter().find(|r| r.id == *id) {
                    who.clone_from(&take.performers);
                }
            }
        }
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

    /// Narrow the Songs page to what holds `needle` in its title or its
    /// creator, case folded — or, given nothing (or `""`), widen it back to
    /// the library. `true` when that changed what the page shows.
    ///
    /// The box drives a view: each keystroke opens `search` over the new
    /// needle and drops the last one — one hydrate per keystroke rather than
    /// a view patched from the last needle's.
    pub fn search(&mut self, needle: Option<&str>) -> bool {
        let needle = needle.filter(|n| !n.is_empty()).map(str::to_string);
        if needle == self.search {
            return false;
        }
        self.search = needle;
        self.open_page();
        true
    }

    /// What the table is showing, whichever side it came from.
    pub fn rows(&self) -> &[Item] {
        match self.source {
            Source::Library if self.search.is_none() => &self.items,
            _ => &self.shown,
        }
    }

    /// Every album this artist made something on: `Album::creator` is whoever
    /// made its first track, the same fact an artist page selects on.
    pub fn albums_by(&self, artist: &str) -> Vec<&Album> {
        self.albums.iter().filter(|a| a.creator == artist).collect()
    }
}

/// The patches onto `track_details` as a list and as the map the table
/// reads: a track's detail is found by its media id while drawing, and a
/// patch names a position — so the list is kept beside the map to say which
/// row a position is.
fn patch_details(rows: &mut Vec<TrackDetail>, map: &mut HashMap<Id, TrackDetail>, patches: &[Patch]) {
    for p in patches {
        match p {
            Patch::Insert { at, node } => {
                let d = TrackDetail::from_bytes(node);
                map.insert(d.media_id, d.clone());
                rows.insert((*at).min(rows.len()), d);
            }
            Patch::Remove { at } => {
                if *at < rows.len() {
                    let d = rows.remove(*at);
                    map.remove(&d.media_id);
                }
            }
            Patch::Update { at, node } => {
                if *at < rows.len() {
                    let d = TrackDetail::from_bytes(node);
                    map.remove(&rows[*at].media_id);
                    map.insert(d.media_id, d.clone());
                    rows[*at] = d;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::AddToPlaylist;
    use crate::tests::{alone, signed_in, song};
    use ark::frames::{Frame, Verdict};
    use ark::peer::Who;
    use ark::schema::Space;

    fn on(playlist: Id, media: Id) -> AddToPlaylist {
        AddToPlaylist {
            playlist_id: key(playlist),
            media_id: key(media),
        }
    }

    /// The maintained list is the query, after every step — one change, a
    /// batch of them, and a playlist toggle that moves `playlist_pos`.
    ///
    /// Falsified by dropping the `Update` arm of `splice`: the toggle — which
    /// arrives as an update to the row it moved — leaves the old
    /// `playlist_pos`.
    #[test]
    fn the_maintained_library_is_the_query() {
        let mut peer = alone("me");
        peer.ensure_playlist();
        assert_ne!(peer.playlist, [0; 16], "Favorites was made");

        peer.client.mutate("add_song", &song("Air", "Bach", "Suites", "a")).unwrap();
        assert!(peer.refresh());
        assert_eq!(peer.items, peer.library_now());

        for (t, f) in [("Glue", "b"), ("Opal", "c"), ("Gosh", "d")] {
            peer.client.mutate("add_song", &song(t, "X", "Y", f)).unwrap();
        }
        peer.refresh();
        assert_eq!(peer.items, peer.library_now(), "a batch, each change pushed in turn");

        let id = peer.items[2].id;
        peer.client.mutate("add_to_playlist", &on(peer.playlist, id)).unwrap();
        peer.refresh();
        assert_eq!(peer.items, peer.library_now());
        assert!(peer.items[2].on_playlist());
        assert!(!peer.refresh(), "nothing moved, nothing read");

        // A song and its place on the playlist before the next refresh: the
        // song's entry is patched in, then its place.
        peer.client.mutate("add_song", &song("Late", "Z", "", "e")).unwrap();
        let late = peer.library_now().last().unwrap().id;
        peer.client.mutate("add_to_playlist", &on(peer.playlist, late)).unwrap();
        peer.refresh();
        assert_eq!(peer.items, peer.library_now(), "a song and its playlist entry in one refresh");
    }

    /// `docs/plan-alone.md` §4: a window alone joins a server in place. The
    /// replica is opened alone as nobody, as the desktop opens it, a
    /// playlist and songs are made and one put on it, and the join hands
    /// all of it back to pending: no open list moves through the join —
    /// the store is what it was — and each is the query after it. A
    /// sign-in then makes it the signer's (every view is the signer's
    /// afresh, so each is read again), and once an in-process hub has taken
    /// it all, the lists are the query and the hub's log has every change,
    /// the add naming the playlist made signed out included: an intent's id,
    /// and so the row it drew, is its origin's and survives the sign-in.
    ///
    /// Falsified by `Peer::join` opening every view afresh after the join (a
    /// new `who_epoch`): every list was `Reset` and "nothing moved" failed;
    /// and by `ark_client::Client::sign_in` authoring the pending intents
    /// again from a new origin: the log held 4, the add refused "not your
    /// playlist".
    #[test]
    fn a_window_alone_joins_a_server_in_place() {
        use ark_peer::{Config, Module, Peer as Hub};
        let mut client = crate::client::memory(crate::client::nobody(), None);
        client.alone();
        client.set_roles([crate::domain::LIBRARY]);
        let mut peer = Peer::open(client);
        peer.ensure_playlist();
        for (t, f) in [("Air", "a"), ("Glue", "b"), ("Opal", "c")] {
            peer.client.mutate("add_song", &song(t, "Bach", "Suites", f)).unwrap();
        }
        peer.refresh();
        let id = peer.items[1].id;
        peer.client.mutate("add_to_playlist", &on(peer.playlist, id)).unwrap();
        peer.refresh();
        assert_eq!(peer.client.status().link, "alone");
        let before = peer.items.clone();

        peer.join("ws://hub/sync");
        assert!(peer.moved.is_empty(), "nothing moved: {:?}", peer.moved);
        assert_eq!(peer.items, before, "the library does not blink");
        assert_eq!(peer.items, peer.library_now());
        assert_eq!(peer.client.pending_len(), 5);

        let d = crate::client::domain();
        let mut hub = Hub::serve(Config::new(Module::of(d)), Box::new(ark_peer::Memory::default()), 0);
        // The songs are the library's to write (`docs/plan-guards.md` D1),
        // and the server judges them under the roles of the login.
        let alice = Who {
            actor: "alice".into(),
            session: "alice@desktop".into(),
            roles: vec![crate::domain::LIBRARY.into()],
        };
        peer.client.login(alice.clone(), "alice@desktop:library".into());
        peer.refresh();
        let c = hub.accept(alice);
        peer.client.connected();
        loop {
            let up = peer.client.take_frames();
            let mut down = vec![];
            for f in up.iter().cloned() {
                down.extend(hub.frame(c, f, crate::client::now_ms()));
            }
            if up.is_empty() && down.is_empty() {
                break;
            }
            for (_, f) in down {
                peer.client.receive(f);
            }
        }
        peer.refresh();
        assert_eq!(peer.client.pending_len(), 0);
        assert_eq!(peer.items, peer.library_now());
        // An intent's id is its origin's (§1.6), whoever signs in: the
        // playlist made alone keeps its id through the sign-in, and the item
        // put on it names it still.
        assert_eq!(hub.log.log().head().0, 5, "the playlist, the three songs and the add");
        assert!(peer.client.take_rejections().is_empty());
        assert!(peer.items[1].on_playlist());
    }

    /// The playlist the library is read against can go — here the server
    /// refuses the one this peer made, with another still pending — and
    /// the library is re-pointed at the first that is left: a view with a
    /// new input, hydrated, and an album page read against the old one
    /// reopened against the new. The rest of the sidebar is patched, not
    /// re-read.
    ///
    /// Falsified by re-pointing only a peer that had no playlist
    /// (`self.playlist == NONE &&` in `refresh`): `playlist` stays on the one
    /// that went and "re-pointed at what is left" fails; and by not reopening
    /// the page on a re-point: "…and so is the page" fails.
    #[test]
    fn a_playlist_that_goes_repoints_the_library() {
        let mut peer = signed_in("me");
        peer.client.mutate("add_song", &song("Air", "Bach", "Suites", "a")).unwrap();
        let mine = CreatePlaylist { name: "Mine".into() };
        let made = peer.client.mutate("create_playlist", &mine).unwrap();
        peer.client.mutate("create_playlist", &CreatePlaylist { name: "Other".into() }).unwrap();
        peer.refresh();
        let [mine, other] = [0, 1].map(|i| peer.playlists()[i].id);
        assert_eq!(peer.playlist, mine, "the first is read against");
        let air = peer.items[0].id;
        peer.client.mutate("add_to_playlist", &on(other, air)).unwrap();
        peer.refresh();
        assert!(!peer.items[0].on_playlist(), "on Other, not on Mine");
        peer.source = Source::Album("Suites".into());
        peer.open_page();

        let reject = Frame::Verdict {
            space: Space::Durable,
            id: made,
            verdict: Verdict::Refused { why: "no".into() },
        };
        peer.client.receive(reject.enc());
        peer.refresh();
        assert_eq!(peer.playlist, other, "re-pointed at what is left");
        assert!(peer.items[0].on_playlist(), "the library is read against Other now");
        assert_eq!(peer.items, peer.library_now());
        assert!(peer.shown[0].on_playlist(), "…and so is the page");
        let lines: Vec<&str> = peer.choices.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(lines, ["Songs", "Albums", "Artists", "Other"]);
    }
}
