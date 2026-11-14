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
use crate::domain::{browse, shows, CreatePlaylist, EpisodeInput};
#[cfg(test)]
use crate::domain::{AlbumInput, Library, Works};
use crate::domain::{Artist as ArtistInput, RecordingInput, Recordings, ShowInput, WorkInput};
use ark_peer::store::{Opening, Store, ViewKey};

use crate::client::{Driven, Replica, Update};
use crate::domain::HarkenView;
use crate::routes::Route;
use crate::rows::{
    Album, Artist, Chapter, Composer, Credit, Cue, Download, EpisodeDetail, Id, Item, Listened, Person, Playlist, Recording, Show, Subscription,
    TrackDetail, Work,
};

/// One line of the sidebar: somewhere the cursor can be and something it can
/// open.
#[derive(Debug, Clone)]
pub struct Choice {
    pub source: Route,
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

/// What one refresh did to one list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moved {
    /// This many patches, spliced.
    Patched(usize),
    /// Taken whole.
    Reset,
}

impl Moved {
    fn of(u: &Update) -> Option<Moved> {
        match u {
            Update::Unchanged => None,
            Update::Patched(ps) => Some(Moved::Patched(ps.len())),
            Update::Reset => Some(Moved::Reset),
        }
    }
}

/// A view the window holds: the domain's key (`HarkenView`, which says
/// what a page is made of), its rows read as this window's own types.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct View(pub HarkenView);

impl ViewKey for View {
    fn parts(&self) -> Vec<View> {
        self.0.views().into_iter().map(View).collect()
    }

    fn open(&self) -> Option<Opening> {
        use HarkenView as V;
        let (name, input) = self.0.query()?;
        Some(match &self.0 {
            V::Library { .. }
            | V::Search { .. }
            | V::Playlist { .. }
            | V::Album { .. }
            | V::Artist { .. }
            | V::Recording { .. }
            | V::Show { .. }
            | V::Appearances { .. } => Opening::read(name, input, Item::from_bytes),
            V::Playlists | V::PlaylistsOf { .. } => Opening::read(name, input, Playlist::from_bytes),
            V::Albums => Opening::read(name, input, Album::from_bytes),
            V::Artists => Opening::read(name, input, Artist::from_bytes),
            V::Composers => Opening::read(name, input, Composer::from_bytes),
            V::Shows => Opening::read(name, input, Show::from_bytes),
            V::Subscriptions => Opening::read(name, input, Subscription::from_bytes),
            V::Downloads => Opening::read(name, input, Download::from_bytes),
            V::MyProgress | V::ShowProgress { .. } => Opening::read(name, input, Listened::from_bytes),
            V::TrackDetails => Opening::read(name, input, TrackDetail::from_bytes),
            V::Works { .. } | V::Work { .. } => Opening::read(name, input, Work::from_bytes),
            V::Recordings { .. } => Opening::read(name, input, Recording::from_bytes),
            V::Credits { .. } | V::AlbumCredits { .. } => Opening::read(name, input, Credit::from_bytes),
            V::EpisodeDetails { .. } => Opening::read(name, input, EpisodeDetail::from_bytes),
            _ => Opening::read(name, input, <[u8]>::to_vec),
        })
    }
}

/// The views the window holds as long as it is open, beside the library.
const ALWAYS: [HarkenView; 9] = [
    HarkenView::Playlists,
    HarkenView::Albums,
    HarkenView::Artists,
    HarkenView::Composers,
    HarkenView::Shows,
    HarkenView::Subscriptions,
    HarkenView::Downloads,
    HarkenView::MyProgress,
    HarkenView::TrackDetails,
];

/// What the window reads beside its views, at one state of the replica:
/// each episode's people, an episode's transcript, and what a person's
/// page's episodes are about — each read when it is first asked for, and
/// kept for the frames between.
#[derive(Default)]
struct Kept {
    at: Option<ark::hash::StateHash>,
    people: HashMap<Id, Vec<Person>>,
    transcript: Option<(Id, Vec<Cue>)>,
    /// A show's episodes' details, by its key.
    details: HashMap<String, Vec<EpisodeDetail>>,
}

/// A replica, and the lists a window draws from it.
pub struct Peer {
    /// The replica, on a thread of its own (`ark_peer::driven`): what it
    /// takes in from its server is never taken in on the window's.
    pub client: Driven,
    /// Every view the window holds (`ark_peer::store`).
    store: Store<View>,
    /// The library as it is read against the playlist.
    library: View,
    /// The page the source wants, as the domain names it.
    page: Option<View>,
    /// Which playlists the last track the picker asked about is on.
    on: Option<View>,
    /// The playlist the library is read against, which is what gives every
    /// row its `on_playlist`. The domain has no favorites of its own — a
    /// playlist named Favorites is just the first one. Zero when there is
    /// none, which is what a peer nobody is signed in on has.
    pub playlist: Id,
    /// What the sidebar picked, and the list that answers it.
    ///
    /// `Route::Library` is `items` itself. Every other list of tracks is the
    /// page's own view, opened when the source changes.
    pub source: Route,
    /// What the Songs page is narrowed to, as the search box has it typed:
    /// `None` is the whole library. While there is one, the page's list is
    /// the `search` view (`docs/plan-db.md` D4) rather than `items`.
    pub search: Option<String>,
    /// Which album each track is on, and the rest of what only a song has: a
    /// map beside the list, because the library row is kind-neutral. Joined
    /// in memory while drawing.
    pub details: HashMap<Id, TrackDetail>,
    /// The sidebar, flat, in the order it is drawn. Headings are derived
    /// while drawing rather than stored, so "line 9" means the same to the
    /// keyboard and to the eye.
    pub choices: Vec<Choice>,
    /// What the last [`Peer::refresh`] moved, by query: the one place to
    /// see that a change cost the lists it touched and no others.
    pub moved: Vec<(String, Moved)>,
    /// Bumped whenever a list that carries covers moved: the window asks for
    /// covers when this moves, which is the *data* changing.
    pub art_gen: u64,
    /// The store's state hash at the last refresh: the views settle against
    /// what moved since, and not on an update where nothing did.
    seen: ark::hash::StateHash,
    /// The chapters last read, of which episode and at which state.
    chapters: std::cell::RefCell<Option<(Id, ark::hash::StateHash, Vec<Chapter>)>>,
    /// The rest of what was read beside the views, and at which state.
    kept: std::cell::RefCell<Kept>,
}

fn library_of(playlist: Id) -> View {
    View(HarkenView::Library { playlist })
}

/// A view of the domain's, as the window keys it.
fn v(view: HarkenView) -> View {
    View(view)
}

impl Peer {
    /// Every list the window starts on, hydrated once; everything after this
    /// is maintained.
    pub fn open(mut client: Replica) -> Peer {
        let c = &mut client;
        let mut store = Store::new();
        store.hold(c, v(HarkenView::Playlists));
        let playlist = store.rows::<Playlist>(&v(HarkenView::Playlists)).first().map_or(NONE, |p| p.id);
        let library = library_of(playlist);
        store.hold(c, library.clone());
        for view in ALWAYS {
            store.hold(c, View(view));
        }
        // What the hydrate already read is not news.
        store.settle(c);
        let seen = client.state();
        let details = store
            .rows::<TrackDetail>(&v(HarkenView::TrackDetails))
            .iter()
            .map(|t| (t.media_id, t.clone()))
            .collect();
        let mut peer = Peer {
            client: Driven::new(client),
            store,
            library,
            page: None,
            on: None,
            playlist,
            source: Route::Library,
            search: None,
            details,
            choices: Vec::new(),
            moved: Vec::new(),
            art_gen: 1,
            seen,
            chapters: Default::default(),
            kept: Default::default(),
        };
        peer.choose();
        peer
    }

    /// The rows of a view the window holds, as its type.
    fn of<T: 'static>(&self, view: HarkenView) -> &[T] {
        self.store.rows(&View(view))
    }

    /// The rows of the page's view in `role`, when the page has one there.
    fn page_of<T: 'static>(&self, role: fn(&HarkenView) -> Option<HarkenView>) -> &[T] {
        match self.page.as_ref().and_then(|p| role(&p.0)) {
            Some(view) => self.store.rows(&View(view)),
            None => &[],
        }
    }

    /// What `view` draws of the library: decoded once per change, not per
    /// frame.
    pub fn items(&self) -> &[Item] {
        self.store.rows(&self.library)
    }

    /// The page's list of tracks, when it is not the library.
    pub fn shown(&self) -> &[Item] {
        self.page_of(HarkenView::tracks)
    }

    /// The index pages' contents — and what a `#album/…` link resolves
    /// against, now that no sidebar line carries one.
    pub fn albums(&self) -> &[Album] {
        self.of(HarkenView::Albums)
    }

    pub fn artists(&self) -> &[Artist] {
        self.of(HarkenView::Artists)
    }

    /// The index pages' rows, to set by hand: a test's covers.
    #[cfg(test)]
    pub fn albums_mut(&mut self) -> &mut [Album] {
        self.store.rows_mut(&v(HarkenView::Albums))
    }

    #[cfg(test)]
    pub fn artists_mut(&mut self) -> &mut [Artist] {
        self.store.rows_mut(&v(HarkenView::Artists))
    }

    pub fn composers(&self) -> &[Composer] {
        self.of(HarkenView::Composers)
    }

    /// Every podcast and series of sermons, by title.
    pub fn shows(&self) -> &[Show] {
        self.of(HarkenView::Shows)
    }

    /// The page of the show of `kind` called `title`: an episode's, by the
    /// creator it carries.
    pub fn show_named(&self, kind: &str, title: &str) -> Option<Route> {
        let show = self.shows().iter().find(|s| s.kind == kind && s.title == title)?;
        Some(Route::Show {
            key: show.id.clone(),
            title: show.title.clone(),
        })
    }

    /// The feeds the library follows, by when each was added.
    pub fn subscriptions(&self) -> &[Subscription] {
        self.of(HarkenView::Subscriptions)
    }

    /// The feed a show is read from, when the library follows it.
    pub fn subscription_of(&self, show: &str) -> Option<&Subscription> {
        self.subscriptions().iter().find(|s| !s.show_id.is_empty() && s.show_id == show)
    }

    /// The files the server has not got yet.
    pub fn downloads(&self) -> &[Download] {
        self.of(HarkenView::Downloads)
    }

    /// Whether a media row's file is not on the server yet, and how it is
    /// going.
    pub fn download_of(&self, media: Id) -> Option<&Download> {
        self.downloads().iter().find(|d| d.media_id == media)
    }

    /// How far the signed-in person has got in an episode, when they have
    /// begun it.
    pub fn mine_of(&self, media: Id) -> Option<&Listened> {
        self.of::<Listened>(HarkenView::MyProgress).iter().find(|l| l.media_id == media)
    }

    /// What an episode of the page's show is about, and when it went out.
    pub fn episode_of(&self, media: Id) -> Option<&EpisodeDetail> {
        self.page_of::<EpisodeDetail>(HarkenView::episodes).iter().find(|e| e.media_id == media)
    }

    /// How far everybody has got in an episode of the page's show, the
    /// newest listening first.
    pub fn listened_to(&self, media: Id) -> Vec<&Listened> {
        self.page_of::<Listened>(HarkenView::listened)
            .iter()
            .filter(|l| l.media_id == media)
            .collect()
    }

    /// An episode's chapters, in order: read once per change of the
    /// replica, and kept for the frames between.
    pub fn chapters_of(&self, media: Id) -> Vec<Chapter> {
        let mut kept = self.chapters.borrow_mut();
        if let Some((m, at, c)) = kept.as_ref() {
            if *m == media && *at == self.seen {
                return c.clone();
            }
        }
        let read: Vec<Chapter> = Peer::rows_of(self.client.lock().answer(shows::chapters, EpisodeInput { media_id: key(media) }));
        *kept = Some((media, self.seen, read.clone()));
        read
    }

    /// What is kept beside the views, emptied once the replica has moved
    /// on from the state it was read at.
    fn kept(&self) -> std::cell::RefMut<'_, Kept> {
        let mut kept = self.kept.borrow_mut();
        if kept.at != Some(self.seen) {
            *kept = Kept {
                at: Some(self.seen),
                ..Kept::default()
            };
        }
        kept
    }

    /// Who is on an episode, in its feed's order: read once per change of
    /// the replica, and kept for the frames between.
    pub fn people_of(&self, media: Id) -> Vec<Person> {
        let input = EpisodeInput { media_id: key(media) };
        let mut kept = self.kept();
        kept.people
            .entry(media)
            .or_insert_with(|| Peer::rows_of(self.client.lock().answer(shows::people, input)))
            .clone()
    }

    /// An episode's transcript, line by line: read once per change of the
    /// replica, and kept for the frames between.
    pub fn transcript_of(&self, media: Id) -> Vec<Cue> {
        let mut kept = self.kept();
        if let Some((m, cues)) = &kept.transcript {
            if *m == media {
                return cues.clone();
            }
        }
        let read: Vec<Cue> = Peer::rows_of(self.client.lock().answer(shows::transcript, EpisodeInput { media_id: key(media) }));
        kept.transcript = Some((media, read.clone()));
        read
    }

    /// The episodes the page's person is on, newest first.
    pub fn appearances(&self) -> &[Item] {
        self.page_of(HarkenView::appearances)
    }

    /// Whether somebody is on any episode: a person's page to go to, for
    /// a link that names them.
    pub fn appears(&self, name: &str) -> bool {
        let input = ArtistInput {
            playlist_id: key(self.playlist),
            name: name.into(),
        };
        !Peer::rows_of::<_, Item>(self.client.lock().answer(shows::appearances, input)).is_empty()
    }

    /// Somebody as the newest episode they are on names them — their
    /// picture, their web page — while their page is open.
    pub fn person_named(&self, name: &str) -> Option<Person> {
        let newest = self.appearances().first()?;
        self.people_of(newest.id).into_iter().find(|p| p.name == name)
    }

    /// What an episode on the page is about, and when it went out: the
    /// show's page has it beside its list; a person's page reads it by the
    /// episode's show, once per change of the replica.
    pub fn about(&self, item: &Item) -> Option<EpisodeDetail> {
        if let Some(e) = self.episode_of(item.id) {
            return Some(e.clone());
        }
        let show = self.shows().iter().find(|s| s.kind == item.kind && s.title == item.creator)?;
        let input = ShowInput {
            playlist_id: key(self.playlist),
            id: show.id.clone(),
        };
        let mut kept = self.kept();
        let details = kept
            .details
            .entry(show.id.clone())
            .or_insert_with(|| Peer::rows_of(self.client.lock().answer(shows::episode_details, input)));
        details.iter().find(|e| e.media_id == item.id).cloned()
    }

    /// A row of the page, whichever of its lists it is on.
    pub fn row(&self, id: Id) -> Option<&Item> {
        self.list_of(id).iter().find(|i| i.id == id)
    }

    /// The list of the page a row is on — its tracks, or the episodes its
    /// person is on — which is the queue a play of it plays through.
    pub fn list_of(&self, id: Id) -> &[Item] {
        match self.appearances().iter().any(|i| i.id == id) {
            true => self.appearances(),
            false => self.rows(),
        }
    }

    /// The shows of one kind (`podcast`, `sermon`), by title.
    pub fn shows_of(&self, kind: &str) -> Vec<&Show> {
        self.shows().iter().filter(|s| s.kind == kind).collect()
    }

    /// What the current page shows when it is not a list of tracks: one
    /// composer's works, or one work's recordings (and that work's own row,
    /// for its header).
    pub fn works(&self) -> &[Work] {
        self.page_of(HarkenView::works)
    }

    pub fn recordings(&self) -> &[Recording] {
        self.page_of(HarkenView::recordings)
    }

    /// Who is on the recording or the album the page is, in billing order.
    pub fn on_recording(&self) -> &[Credit] {
        self.page_of(HarkenView::credits)
    }

    /// Make sure there is a playlist to read the library against: make
    /// "Favorites" if this replica has none.
    ///
    /// **Only alone** (`App::alone`). On a server a person's Favorites is the
    /// server's to make, at their first sign-in (`playlists.welcome`): a
    /// device making its own before it had heard the log would leave them a
    /// "Favorites (1)" per device. A refusal is not worth a status line: it only means there is
    /// no playlist yet.
    pub fn ensure_playlist(&mut self) {
        let made = CreatePlaylist { name: "Favorites".into() };
        if self.playlists().is_empty() && self.client.lock().mutate("create_playlist", &made).is_ok() {
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
        self.client.lock().join();
        self.client.lock().connect(url);
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
        let state = self.client.lock().state();
        self.seen = state;
        let moved = self.store.settle(&mut self.client.lock());
        // What each view did, by its function: a page is told by its views.
        self.moved = moved
            .iter()
            .filter_map(|k| Some((self.store.name(k)?.to_string(), Moved::of(self.store.last(k)?)?)))
            .collect();
        let did = |view: HarkenView| moved.contains(&View(view));
        let on_page = |role: fn(&HarkenView) -> Option<HarkenView>| {
            let view = self.page.as_ref().and_then(|p| role(&p.0));
            view.is_some_and(|view| moved.contains(&View(view)))
        };
        let lists = did(HarkenView::Playlists);
        let library = moved.contains(&self.library);
        let albums = did(HarkenView::Albums);
        let artists = did(HarkenView::Artists);
        let composers = did(HarkenView::Composers);
        let shows = did(HarkenView::Shows);
        let shown = on_page(HarkenView::tracks);
        let works = on_page(HarkenView::works);
        let recordings = on_page(HarkenView::recordings);
        let appearances = on_page(HarkenView::appearances);
        if did(HarkenView::TrackDetails) {
            self.details = self
                .of::<TrackDetail>(HarkenView::TrackDetails)
                .iter()
                .map(|t| (t.media_id, t.clone()))
                .collect();
        }

        // The playlist the library is read against can stop existing, and
        // routinely does: the default this peer made on its first run is
        // dropped on the rebase when another device's turns out to have been
        // first. Re-pointing costs a hydrate of the library and of a page
        // read against it, so only when it moved.
        let first = self.playlists().first().map_or(NONE, |p| p.id);
        let repointed = !self.playlists().iter().any(|p| p.id == self.playlist) && self.playlist != first;
        if repointed {
            self.playlist = first;
            let library = library_of(first);
            let old = std::mem::replace(&mut self.library, library.clone());
            self.swap(Some(old), Some(library));
            self.open_page();
        }
        if lists || library || albums || artists || composers || shows || repointed {
            self.choose();
        }
        // The page's episodes carry the faces of who is on them.
        if albums || artists || composers || shows || works || recordings || shown || appearances {
            self.art_gen = self.art_gen.wrapping_add(1);
        }
        if recordings {
            self.label_recording();
        }
        !moved.is_empty() || repointed
    }

    /// `new` held in place of `old`: held first, so what the two share is
    /// not opened again.
    fn swap(&mut self, old: Option<View>, new: Option<View>) -> bool {
        if old == new {
            return false;
        }
        let mut c = self.client.lock();
        if let Some(n) = new {
            self.store.hold(&mut c, n);
        }
        if let Some(o) = old {
            self.store.release(&mut c, &o);
        }
        true
    }

    /// The sidebar, from the lists it counts. Grouping is the domain's, so
    /// every client folds the library the same way.
    fn choose(&mut self) {
        // Music, then the lists somebody made. **A line is drawn only when it
        // has rows behind it**, which is what lets one schema serve every
        // genre: a pop library has no works, so no Composers line. Songs is
        // unconditional because it is the library.
        let mut choices = vec![Choice {
            source: Route::Library,
            label: "Songs".into(),
            count: Some(self.items().len() as i64),
        }];
        for (source, label, count) in [
            (Route::Albums, "Albums", self.albums().len()),
            (Route::Artists, "Artists", self.artists().len()),
            (Route::Composers, "Composers", self.composers().len()),
        ] {
            if count > 0 {
                choices.push(Choice {
                    source,
                    label: label.into(),
                    count: Some(count as i64),
                });
            }
        }
        // Then what is listened to rather than played: the podcasts and the
        // sermons, each a line even with no show of its kind yet — its page
        // is where the first one is followed from.
        for (source, label, kind) in [(Route::Podcasts, "Podcasts", "podcast"), (Route::Sermons, "Sermons", "sermon")] {
            let count = self.shows_of(kind).len();
            choices.push(Choice {
                source,
                label: label.into(),
                count: (count > 0).then_some(count as i64),
            });
        }
        choices.extend(self.playlists().iter().map(|p| Choice {
            source: p.route(),
            label: p.name.clone(),
            count: None,
        }));
        self.choices = choices;
    }

    /// The caller's playlists, as the sidebar holds them.
    pub fn playlists(&self) -> &[Playlist] {
        self.of(HarkenView::Playlists)
    }

    /// Which of the caller's playlists a track is on: a view kept open on
    /// the last track asked about, so the picker reopened on the same track —
    /// after it made a playlist — reads nothing.
    pub fn playlists_of(&mut self, media: Id) -> Vec<Playlist> {
        let wanted = Some(v(HarkenView::PlaylistsOf { media }));
        let old = std::mem::replace(&mut self.on, wanted.clone());
        self.swap(old, wanted.clone());
        wanted.map_or(vec![], |w| self.store.rows::<Playlist>(&w).to_vec())
    }

    /// A query's rows, asked once through its stub; a refusal (a guard,
    /// nobody signed in) is no rows. For what is not a list on screen:
    /// resolving a link, and the tests, which hold the maintained lists to
    /// what the query answers.
    pub(crate) fn rows_of<E, T: From<E>>(answer: Result<Vec<E>, String>) -> Vec<T> {
        answer.map_or_else(|_| vec![], |es| es.into_iter().map(T::from).collect())
    }

    pub fn work(&self, id: &str) -> Vec<Work> {
        Peer::rows_of(self.client.lock().answer(browse::work, WorkInput { id: id.into() }))
    }

    /// What a page's query answers, read once. The page holds views; these
    /// are what the tests compare them with.
    #[cfg(test)]
    pub fn works_of(&self, composer: &str) -> Vec<Work> {
        Peer::rows_of(self.client.lock().answer(browse::works, Works { composer: composer.into() }))
    }

    /// A work's performances, read once: the most complete first.
    pub fn recordings_of(&self, work: &str) -> Vec<Recording> {
        Peer::rows_of(self.client.lock().answer(browse::recordings, Recordings { work_id: work.into() }))
    }

    /// One performance's tracks, read once, in the order the work goes.
    pub fn recording_rows(&self, id: &str) -> Vec<Item> {
        let input = RecordingInput {
            playlist_id: key(self.playlist),
            id: id.into(),
        };
        Peer::rows_of(self.client.lock().answer(browse::recording, input))
    }

    #[cfg(test)]
    pub fn credits(&self, recording: &str) -> Vec<crate::domain::CreditsEntry> {
        let input = crate::domain::Credits {
            recording_id: recording.into(),
        };
        self.client.lock().answer(browse::credits, input).unwrap_or_default()
    }

    /// The key of the first album of that name the albums page lists.
    #[cfg(test)]
    pub fn album_id(&self, name: &str) -> String {
        self.albums()
            .iter()
            .find(|a| a.name == name)
            .unwrap_or_else(|| panic!("no album {name}"))
            .id
            .clone()
    }

    #[cfg(test)]
    pub fn album_rows(&self, id: &str) -> Vec<Item> {
        let input = AlbumInput {
            playlist_id: key(self.playlist),
            id: id.into(),
        };
        Peer::rows_of(self.client.lock().answer(browse::album, input))
    }

    #[cfg(test)]
    pub fn track_details(&self) -> Vec<TrackDetail> {
        Peer::rows_of(self.client.lock().answer(browse::track_details, ()))
    }

    /// The library as its query answers it now, against the playlist it is
    /// read against.
    #[cfg(test)]
    pub fn library_now(&self) -> Vec<Item> {
        Peer::rows_of(self.client.lock().answer(
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
        let playlist = self.playlist;
        use HarkenView as V;
        let page = match &self.source {
            // The library is `items` itself, unless a search narrows it: then
            // the page is the `search` view, kept like any other
            // (`docs/plan-db.md` D4).
            Route::Library => self.search.as_deref().map(|n| V::Search { playlist, needle: n.into() }),
            Route::Albums
            | Route::Artists
            | Route::Composers
            | Route::Podcasts
            | Route::Sermons
            | Route::Settings
            | Route::Transcript
            | Route::Diagnostics
            | Route::Join { .. } => None,
            Route::Show { key: id, .. } => Some(V::ShowPage { playlist, id: id.clone() }),
            // The work itself, for the header, and its performances: this
            // page may have been reached without opening a composer at all — a
            // link somebody sent — so the row is read by key.
            Route::Work { key: id, .. } => Some(V::WorkPage { id: id.clone() }),
            // By its id (an older link's by its name): one since deleted
            // opens nothing, and the router lands it on its fallback.
            Route::Playlist { .. } => self.playlist_of(&self.source).map(|p| V::Playlist { id: p.id }),
            Route::Album { key: id, .. } => Some(V::AlbumPage { playlist, id: id.clone() }),
            // Their tracks, the works they are by and the episodes they are on.
            Route::Artist { name } => Some(V::ArtistPage {
                playlist,
                name: name.clone(),
            }),
            // Ordered by the *work* rather than the release, and every
            // recording of that work, so the header can find this one.
            Route::Recording { key: id, .. } => Some(V::RecordingPage { playlist, id: id.clone() }),
        }
        .map(View);
        let old = std::mem::replace(&mut self.page, page.clone());
        if self.swap(old, page) {
            self.art_gen = self.art_gen.wrapping_add(1);
        }
        self.label_recording();
    }

    /// A link arriving cold carries a recording's key and no label: the row
    /// knows who played it, and this is where the two meet.
    fn label_recording(&mut self) {
        if let Route::Recording { key: id, title: who } = &mut self.source {
            if who.is_empty() {
                let takes = match self.page.as_ref().and_then(|p| p.0.recordings()) {
                    Some(view) => self.store.rows::<Recording>(&View(view)),
                    None => &[],
                };
                if let Some(take) = takes.iter().find(|r| r.id == *id) {
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

    /// The route as this peer has it — a route read from the address bar
    /// carries no title, and this finds it — or its fallback, the library,
    /// when this peer has not got what it names: a link to a playlist since
    /// deleted, or to an album not received yet, lands on a page rather than
    /// a heading with nothing under it.
    pub fn resolve(&self, route: &Route) -> Route {
        let found = match route {
            Route::Album { key, .. } => self.albums().iter().find(|a| a.id == *key).map(|a| Route::Album {
                key: a.id.clone(),
                title: a.name.clone(),
            }),
            // A person, whichever list they are on, or on an episode.
            Route::Artist { name } => {
                let known = self.artists().iter().any(|a| a.name == *name) || self.composers().iter().any(|c| c.name == *name);
                (known || self.appears(name)).then(|| route.clone())
            }
            Route::Transcript => Some(Route::Transcript),
            // By its id, so a renamed one is found under its new name; an
            // older link's, by the name it carries.
            Route::Playlist { .. } => self.playlist_of(route).map(Playlist::route),
            // By *querying*, not against a list this peer holds: a link
            // arriving cold has no page yet, and the key is enough to ask with.
            Route::Work { key, .. } => self.work_named(key).map(|title| Route::Work { key: key.clone(), title }),
            Route::Show { key, .. } => self.shows().iter().find(|s| s.id == *key).map(|s| Route::Show {
                key: key.clone(),
                title: s.title.clone(),
            }),
            Route::Recording { .. } => match route.work() {
                Some(Route::Work { key, .. }) => self.work_named(&key).map(|_| route.clone()),
                _ => None,
            },
            _ => self.choices.iter().find(|c| c.source == *route).map(|c| c.source.clone()),
        };
        found.unwrap_or_else(|| route.fallback())
    }

    /// The playlist of the caller's a route names: by its id, and only for
    /// an older link's, which has none, by its name (`Route::names`).
    pub fn playlist_of(&self, route: &Route) -> Option<&Playlist> {
        self.playlists().iter().find(|p| route.names(&p.hex(), &p.name))
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
            Route::Library if self.search.is_none() => self.items(),
            _ => self.shown(),
        }
    }

    /// Every album this artist made something on: `Album::creator` is whoever
    /// made its first track, the same fact an artist page selects on.
    pub fn albums_by(&self, artist: &str) -> Vec<&Album> {
        self.albums().iter().filter(|a| a.creator == artist).collect()
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

        peer.client.lock().mutate("add_song", &song("Air", "Bach", "Suites", "a")).unwrap();
        assert!(peer.refresh());
        assert_eq!(peer.items(), peer.library_now());

        for (t, f) in [("Glue", "b"), ("Opal", "c"), ("Gosh", "d")] {
            peer.client.lock().mutate("add_song", &song(t, "X", "Y", f)).unwrap();
        }
        peer.refresh();
        assert_eq!(peer.items(), peer.library_now(), "a batch, each change pushed in turn");

        let id = peer.items()[2].id;
        peer.client.lock().mutate("add_to_playlist", &on(peer.playlist, id)).unwrap();
        peer.refresh();
        assert_eq!(peer.items(), peer.library_now());
        assert!(peer.items()[2].on_playlist());
        assert!(!peer.refresh(), "nothing moved, nothing read");

        // A song and its place on the playlist before the next refresh: the
        // song's entry is patched in, then its place.
        peer.client.lock().mutate("add_song", &song("Late", "Z", "", "e")).unwrap();
        let late = peer.library_now().last().unwrap().id;
        peer.client.lock().mutate("add_to_playlist", &on(peer.playlist, late)).unwrap();
        peer.refresh();
        assert_eq!(peer.items(), peer.library_now(), "a song and its playlist entry in one refresh");
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
        peer.client.by_hand();
        peer.ensure_playlist();
        for (t, f) in [("Air", "a"), ("Glue", "b"), ("Opal", "c")] {
            peer.client.lock().mutate("add_song", &song(t, "Bach", "Suites", f)).unwrap();
        }
        peer.refresh();
        let id = peer.items()[1].id;
        peer.client.lock().mutate("add_to_playlist", &on(peer.playlist, id)).unwrap();
        peer.refresh();
        assert_eq!(peer.client.lock().status().link, "alone");
        let before = peer.items().to_vec();

        peer.join("ws://hub/sync");
        assert!(peer.moved.is_empty(), "nothing moved: {:?}", peer.moved);
        assert_eq!(peer.items(), before, "the library does not blink");
        assert_eq!(peer.items(), peer.library_now());
        assert_eq!(peer.client.lock().pending_len(), 5);

        let d = crate::client::domain();
        let mut hub = Hub::serve(Config::new(Module::of(d)), Box::new(ark_peer::Memory::default()), 0);
        // The songs are the library's to write (`docs/plan-guards.md` D1),
        // and the server judges them under the roles of the login.
        let alice = Who {
            actor: "alice".into(),
            session: "alice@desktop".into(),
            roles: vec![crate::domain::LIBRARY.into()],
        };
        peer.client.lock().login(alice.clone(), "alice@desktop:library".into());
        peer.refresh();
        let c = hub.accept(alice);
        peer.client.lock().connected();
        loop {
            let up = peer.client.lock().take_frames();
            let mut down = vec![];
            for f in up.iter().cloned() {
                down.extend(hub.frame(c, f, crate::client::now_ms()));
            }
            if up.is_empty() && down.is_empty() {
                break;
            }
            for (_, f) in down {
                peer.client.lock().receive(f);
            }
        }
        peer.refresh();
        assert_eq!(peer.client.lock().pending_len(), 0);
        assert_eq!(peer.items(), peer.library_now());
        // An intent's id is its origin's (§1.6), whoever signs in: the
        // playlist made alone keeps its id through the sign-in, and the item
        // put on it names it still.
        assert_eq!(hub.log.log().head().0, 5, "the playlist, the three songs and the add");
        assert!(peer.client.lock().take_rejections().is_empty());
        assert!(peer.items()[1].on_playlist());
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
        peer.client.lock().mutate("add_song", &song("Air", "Bach", "Suites", "a")).unwrap();
        let mine = CreatePlaylist { name: "Mine".into() };
        let made = peer.client.lock().mutate("create_playlist", &mine).unwrap();
        peer.client
            .lock()
            .mutate("create_playlist", &CreatePlaylist { name: "Other".into() })
            .unwrap();
        peer.refresh();
        let [mine, other] = [0, 1].map(|i| peer.playlists()[i].id);
        assert_eq!(peer.playlist, mine, "the first is read against");
        let air = peer.items()[0].id;
        peer.client.lock().mutate("add_to_playlist", &on(other, air)).unwrap();
        peer.refresh();
        assert!(!peer.items()[0].on_playlist(), "on Other, not on Mine");
        peer.source = Route::Album {
            key: peer.album_id("Suites"),
            title: "Suites".into(),
        };
        peer.open_page();

        let reject = Frame::Verdict {
            space: Space::Durable,
            id: made,
            verdict: Verdict::Refused { why: "no".into() },
        };
        peer.client.lock().receive(reject.enc());
        peer.refresh();
        assert_eq!(peer.playlist, other, "re-pointed at what is left");
        assert!(peer.items()[0].on_playlist(), "the library is read against Other now");
        assert_eq!(peer.items(), peer.library_now());
        assert!(peer.shown()[0].on_playlist(), "…and so is the page");
    }
}
