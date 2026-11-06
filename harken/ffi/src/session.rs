//! harken's views on the phones (`ark_peer::store`, `ark_ffi::Session`):
//! one [`HarkenSession`] over a peer holds the views the screens show — each
//! named by a [`HarkenView`], what a page is made of said by the domain
//! (`harken_domain::HarkenView`) — keeps them spliced on the bridge's
//! thread and tells the app which moved ([`Changed`]). A screen holds its
//! view while it shows ([`HarkenSession::hold`], a [`Holding`] let go when
//! dropped) and reads it: a list of tracks or of playlists as the domain's
//! own records ([`HarkenSession::tracks`], [`HarkenSession::playlists`]), and any other
//! view each row its JSON ([`HarkenSession::rows`]), which the generated types
//! read (those rows have no UniFFI records yet).

use std::sync::Arc;

use ark::format::Encode;
use ark_ffi::{Peer, Refusal, RowId, Twin};
use harken_domain::views::{HarkenView as D, Key};
use harken_domain::{LibraryEntry, Playlist, UniFfiTag};

/// A view the phones hold, or a page made of several: the domain's
/// (`harken_domain::HarkenView`), each id its hex. A playlist given is the
/// one a list of tracks is read against; the zero id is none.
#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum HarkenView {
    Library { playlist: RowId },
    Search { playlist: RowId, needle: String },
    Playlists,
    Albums,
    Artists,
    Composers,
    Shows,
    Subscriptions,
    Downloads,
    MyProgress,
    TrackDetails,
    LastPlayed,
    Playlist { id: RowId },
    PlaylistsOf { media: RowId },
    Album { playlist: RowId, id: String },
    AlbumCredits { id: String },
    Artist { playlist: RowId, name: String },
    Works { composer: String },
    Work { id: String },
    Recordings { work: String },
    Recording { playlist: RowId, id: String },
    Credits { recording: String },
    Show { playlist: RowId, id: String },
    ShowProgress { playlist: RowId, id: String },
    EpisodeDetails { playlist: RowId, id: String },
    ShowPeople { playlist: RowId, id: String },
    Appearances { playlist: RowId, name: String },
    People { media: RowId },
    Transcript { media: RowId },
    ShowPage { playlist: RowId, id: String },
    WorkPage { id: String },
    ArtistPage { playlist: RowId, name: String },
    AlbumPage { playlist: RowId, id: String },
    RecordingPage { playlist: RowId, id: String },
}

fn k(r: &RowId) -> Key {
    r.0
}

fn r(k: &Key) -> RowId {
    RowId(*k)
}

impl From<&HarkenView> for D {
    fn from(v: &HarkenView) -> D {
        use HarkenView as P;
        match v {
            P::Library { playlist } => D::Library { playlist: k(playlist) },
            P::Search { playlist, needle } => D::Search {
                playlist: k(playlist),
                needle: needle.clone(),
            },
            P::Playlists => D::Playlists,
            P::Albums => D::Albums,
            P::Artists => D::Artists,
            P::Composers => D::Composers,
            P::Shows => D::Shows,
            P::Subscriptions => D::Subscriptions,
            P::Downloads => D::Downloads,
            P::MyProgress => D::MyProgress,
            P::TrackDetails => D::TrackDetails,
            P::LastPlayed => D::LastPlayed,
            P::Playlist { id } => D::Playlist { id: k(id) },
            P::PlaylistsOf { media } => D::PlaylistsOf { media: k(media) },
            P::Album { playlist, id } => D::Album {
                playlist: k(playlist),
                id: id.clone(),
            },
            P::AlbumCredits { id } => D::AlbumCredits { id: id.clone() },
            P::Artist { playlist, name } => D::Artist {
                playlist: k(playlist),
                name: name.clone(),
            },
            P::Works { composer } => D::Works { composer: composer.clone() },
            P::Work { id } => D::Work { id: id.clone() },
            P::Recordings { work } => D::Recordings { work: work.clone() },
            P::Recording { playlist, id } => D::Recording {
                playlist: k(playlist),
                id: id.clone(),
            },
            P::Credits { recording } => D::Credits {
                recording: recording.clone(),
            },
            P::Show { playlist, id } => D::Show {
                playlist: k(playlist),
                id: id.clone(),
            },
            P::ShowProgress { playlist, id } => D::ShowProgress {
                playlist: k(playlist),
                id: id.clone(),
            },
            P::EpisodeDetails { playlist, id } => D::EpisodeDetails {
                playlist: k(playlist),
                id: id.clone(),
            },
            P::ShowPeople { playlist, id } => D::ShowPeople {
                playlist: k(playlist),
                id: id.clone(),
            },
            P::Appearances { playlist, name } => D::Appearances {
                playlist: k(playlist),
                name: name.clone(),
            },
            P::People { media } => D::People { media: k(media) },
            P::Transcript { media } => D::Transcript { media: k(media) },
            P::ShowPage { playlist, id } => D::ShowPage {
                playlist: k(playlist),
                id: id.clone(),
            },
            P::WorkPage { id } => D::WorkPage { id: id.clone() },
            P::ArtistPage { playlist, name } => D::ArtistPage {
                playlist: k(playlist),
                name: name.clone(),
            },
            P::AlbumPage { playlist, id } => D::AlbumPage {
                playlist: k(playlist),
                id: id.clone(),
            },
            P::RecordingPage { playlist, id } => D::RecordingPage {
                playlist: k(playlist),
                id: id.clone(),
            },
        }
    }
}

impl From<&D> for HarkenView {
    fn from(v: &D) -> HarkenView {
        use HarkenView as P;
        match v {
            D::Library { playlist } => P::Library { playlist: r(playlist) },
            D::Search { playlist, needle } => P::Search {
                playlist: r(playlist),
                needle: needle.clone(),
            },
            D::Playlists => P::Playlists,
            D::Albums => P::Albums,
            D::Artists => P::Artists,
            D::Composers => P::Composers,
            D::Shows => P::Shows,
            D::Subscriptions => P::Subscriptions,
            D::Downloads => P::Downloads,
            D::MyProgress => P::MyProgress,
            D::TrackDetails => P::TrackDetails,
            D::LastPlayed => P::LastPlayed,
            D::Playlist { id } => P::Playlist { id: r(id) },
            D::PlaylistsOf { media } => P::PlaylistsOf { media: r(media) },
            D::Album { playlist, id } => P::Album {
                playlist: r(playlist),
                id: id.clone(),
            },
            D::AlbumCredits { id } => P::AlbumCredits { id: id.clone() },
            D::Artist { playlist, name } => P::Artist {
                playlist: r(playlist),
                name: name.clone(),
            },
            D::Works { composer } => P::Works { composer: composer.clone() },
            D::Work { id } => P::Work { id: id.clone() },
            D::Recordings { work } => P::Recordings { work: work.clone() },
            D::Recording { playlist, id } => P::Recording {
                playlist: r(playlist),
                id: id.clone(),
            },
            D::Credits { recording } => P::Credits {
                recording: recording.clone(),
            },
            D::Show { playlist, id } => P::Show {
                playlist: r(playlist),
                id: id.clone(),
            },
            D::ShowProgress { playlist, id } => P::ShowProgress {
                playlist: r(playlist),
                id: id.clone(),
            },
            D::EpisodeDetails { playlist, id } => P::EpisodeDetails {
                playlist: r(playlist),
                id: id.clone(),
            },
            D::ShowPeople { playlist, id } => P::ShowPeople {
                playlist: r(playlist),
                id: id.clone(),
            },
            D::Appearances { playlist, name } => P::Appearances {
                playlist: r(playlist),
                name: name.clone(),
            },
            D::People { media } => P::People { media: r(media) },
            D::Transcript { media } => P::Transcript { media: r(media) },
            D::ShowPage { playlist, id } => P::ShowPage {
                playlist: r(playlist),
                id: id.clone(),
            },
            D::WorkPage { id } => P::WorkPage { id: id.clone() },
            D::ArtistPage { playlist, name } => P::ArtistPage {
                playlist: r(playlist),
                name: name.clone(),
            },
            D::AlbumPage { playlist, id } => P::AlbumPage {
                playlist: r(playlist),
                id: id.clone(),
            },
            D::RecordingPage { playlist, id } => P::RecordingPage {
                playlist: r(playlist),
                id: id.clone(),
            },
        }
    }
}

/// What the app is told when views it holds moved: it reads them again.
#[uniffi::export(with_foreign)]
pub trait Changed: Send + Sync {
    fn moved(&self, views: Vec<HarkenView>);
}

/// harken's views over a peer: `HarkenSession` in Kotlin and Swift, since
/// the domain has a `Session` of its own (who listens where).
#[derive(uniffi::Object)]
pub struct HarkenSession {
    inner: Arc<ark_ffi::Session<D>>,
}

impl HarkenSession {
    /// A view's rows read as `T` and handed over as its record; a row that
    /// is not one is left out.
    fn typed<T: Encode + Twin<UniFfiTag>>(&self, view: &HarkenView) -> Vec<T::Ffi> {
        let key = D::from(view);
        self.inner.read(|st, _| {
            st.rows::<Vec<u8>>(&key)
                .iter()
                .filter_map(|b| T::from_bytes(b).ok())
                .map(T::ffi)
                .collect()
        })
    }
}

/// A view held while a screen shows it: let go when dropped (or
/// [`Holding::release`]d).
#[derive(uniffi::Object)]
pub struct Holding {
    hold: ark_ffi::Hold<D>,
}

#[uniffi::export]
impl Holding {
    /// Held no more, now rather than when dropped.
    pub fn release(&self) {
        self.hold.close();
    }
}

#[uniffi::export]
impl HarkenSession {
    /// The views of `peer`, none held yet.
    #[uniffi::constructor]
    pub fn new(peer: Arc<Peer>) -> Arc<HarkenSession> {
        Arc::new(HarkenSession {
            inner: Arc::new(ark_ffi::Session::new(peer)),
        })
    }

    /// The peer it is over.
    pub fn peer(&self) -> Arc<Peer> {
        self.inner.peer().clone()
    }

    /// `view` held once more — opened and read the first time — until the
    /// holding is dropped.
    pub fn hold(&self, view: HarkenView) -> Arc<Holding> {
        Arc::new(Holding {
            hold: self.inner.hold((&view).into()),
        })
    }

    /// A number that grows whenever `view`'s rows moved — a page's,
    /// whenever any of its views' did; zero while nothing holds it.
    pub fn version(&self, view: HarkenView) -> u64 {
        self.inner.version(&(&view).into())
    }

    /// Told which views moved, on the bridge's thread, whenever any held
    /// did; none tells nobody.
    pub fn set_listener(&self, listener: Option<Arc<dyn Changed>>) {
        self.inner.set_listener(
            listener.map(|l| -> ark_ffi::session::Moved<D> { Arc::new(move |moved: Vec<D>| l.moved(moved.iter().map(HarkenView::from).collect())) }),
        )
    }

    /// The views of a page — itself, when it is one view — as the domain
    /// says the page is made of.
    pub fn parts(&self, view: HarkenView) -> Vec<HarkenView> {
        D::from(&view).views().iter().map(HarkenView::from).collect()
    }

    /// The tracks of a list of them (the library, a search, a playlist, an
    /// album, somebody's, a performance's, a show's, the episodes somebody
    /// is on), as the domain's records; none while nothing holds it.
    pub fn tracks(&self, view: HarkenView) -> Vec<<LibraryEntry as Twin<UniFfiTag>>::Ffi> {
        self.typed::<LibraryEntry>(&view)
    }

    /// The playlists (`Playlists`, `PlaylistsOf`), as the domain's records.
    pub fn playlists(&self, view: HarkenView) -> Vec<<Playlist as Twin<UniFfiTag>>::Ffi> {
        self.typed::<Playlist>(&view)
    }

    /// The rows of `view`, each its JSON; none while nothing holds it.
    pub fn rows(&self, view: HarkenView) -> Vec<String> {
        let key = D::from(&view);
        self.inner.read(|st, peer| {
            let Some(name) = st.name(&key) else { return vec![] };
            let row = |b: &Vec<u8>| peer.element_json(name, b).unwrap_or_default();
            st.rows::<Vec<u8>>(&key).iter().map(row).collect()
        })
    }

    /// The function `name` on its input's JSON: its intent's id, or the
    /// domain's sentence for why not — kept as [`HarkenSession::refusal`] until
    /// the next change is taken.
    pub fn send(&self, name: String, input: String) -> Result<String, Refusal> {
        self.inner.send(|p| p.mutate(name, input))
    }

    /// Why the last change sent was refused, while it is the last.
    pub fn refusal(&self) -> Option<String> {
        self.inner.refusal()
    }
}
