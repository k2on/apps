//! Where harken can be: every route, one path each (`ark-router`), on every
//! shell — the desktop's and the browser's address bar, each phone's back
//! stack, a deep link, the listening session's "played from", Android
//! Auto's media ids.
//!
//! **A route carries names, not ids.** A playlist is identified by its name,
//! which is unique to the person whose it is; an album, a work, a recording
//! and a show by the key the domain derives, which has slashes of its own
//! (`johann-sebastian-bach/bwv-988`), and the title to draw beside it. What a
//! route is called is not which route it is: two routes are the same when
//! their paths are, so a route read back from a path — which has no title —
//! is the route it was written from.
//!
//! **A stale route lands on the library.** A link to a playlist since
//! deleted, or to an album not received yet, is resolved by the shell
//! against what it has, and falls back to [`Route::fallback`]: a page,
//! rather than a heading with nothing under it.
use ark_router::Routes;

/// Somewhere harken can be.
#[derive(Debug, Clone, Routes)]
#[cfg_attr(feature = "ffi", derive(uniffi::Enum), uniffi::export(Eq, Hash))]
pub enum Route {
    /// Every song, in library order: the library itself.
    #[home]
    #[route("/library")]
    Library,
    /// The index pages: they carry no name because they *are* the whole list.
    #[route("/albums")]
    Albums,
    #[route("/artists")]
    Artists,
    /// Everyone this library has a work *by*.
    #[route("/composers")]
    Composers,
    /// Every podcast, and every series of sermons.
    #[route("/podcasts")]
    Podcasts,
    #[route("/sermons")]
    Sermons,
    /// A playlist, by its name.
    #[route("/playlist/:name")]
    Playlist { name: String },
    /// An album by its key (`keys::album_id`), and its name: two records of
    /// one title are two albums.
    #[route("/album/:key")]
    Album { key: String, title: String },
    /// One person's page, whatever they did. An old `composer/…` link is
    /// their page too.
    #[route("/artist/:name")]
    #[route("/composer/:name")]
    Artist { name: String },
    /// One work and the performances of it: the derived key, and its title.
    #[route("/work/:key")]
    Work { key: String, title: String },
    /// One performance, by its key (the work's, `@`, who played it), and who
    /// played it, the only thing that tells two of them apart.
    #[route("/recording/:key")]
    Recording { key: String, title: String },
    /// One show and its episodes: its key (`podcast/full-time-nix`) and its
    /// title.
    #[route("/show/:key")]
    Show { key: String, title: String },
    /// What this client and its server do.
    #[route("/settings")]
    Settings,
    /// What is said in the episode playing, line by line.
    #[route("/transcript")]
    Transcript,
    /// The numbers behind the sync, on a phone.
    #[route("/diagnostics")]
    Diagnostics,
    /// Sign in to a server from a link or a QR code: `join?server=…&code=…`.
    /// Not a page: a shell hands it to the onboarding.
    #[route("/join?server=:server&code=:code")]
    Join { server: String, code: Option<String> },
}

impl Route {
    /// What the page is called: its own title, or the index's name. Empty
    /// for a route read back from a path until the shell has found it.
    pub fn title(&self) -> String {
        match self {
            Route::Library => "Songs".into(),
            Route::Albums => "Albums".into(),
            Route::Artists => "Artists".into(),
            Route::Composers => "Composers".into(),
            Route::Podcasts => "Podcasts".into(),
            Route::Sermons => "Sermons".into(),
            Route::Settings => "Settings".into(),
            Route::Transcript => "Transcript".into(),
            Route::Diagnostics => "Diagnostics".into(),
            Route::Join { .. } => "Sign In".into(),
            Route::Playlist { name } | Route::Artist { name } => name.clone(),
            Route::Album { title, .. } | Route::Work { title, .. } | Route::Recording { title, .. } | Route::Show { title, .. } => title.clone(),
        }
    }

    /// What a page of this kind is called, above its title: "Goldberg
    /// Variations" is a work and a record, and neither is obvious from the
    /// name alone. Empty for a page that needs no such word.
    pub fn kind(&self) -> &'static str {
        match self {
            Route::Album { .. } => "Album",
            Route::Artist { .. } => "Artist",
            Route::Work { .. } => "Work",
            Route::Recording { .. } => "Recording",
            Route::Show { key, .. } if key.starts_with("sermon/") => "Sermons",
            Route::Show { .. } => "Podcast",
            _ => "",
        }
    }

    /// The heading a route is drawn under in the desktop's sidebar, or `None`
    /// for a page navigated to from another page.
    pub fn heading(&self) -> Option<&'static str> {
        match self {
            Route::Library | Route::Albums | Route::Artists | Route::Composers => Some("Music"),
            Route::Podcasts | Route::Sermons => Some("Shows"),
            Route::Playlist { .. } => Some("Playlists"),
            _ => None,
        }
    }

    /// Whether the route is a root — one of the library's own lists, which a
    /// phone shows as its Library tab's and the desktop as its sidebar's —
    /// rather than a page pushed onto a back stack.
    pub fn is_root(&self) -> bool {
        matches!(
            self,
            Route::Library | Route::Albums | Route::Artists | Route::Composers | Route::Podcasts | Route::Sermons
        )
    }

    /// Where the route lands when what it names is not here (deleted, not
    /// received yet): the library, which is always a page. The desktop's
    /// rule, and now every shell's.
    pub fn fallback(&self) -> Route {
        Route::Library
    }

    /// The work a recording is a performance of, by its key: everything
    /// before the `@`. `None` for any other route, or a key with no `@`.
    pub fn work(&self) -> Option<Route> {
        match self {
            Route::Recording { key, .. } => key.split_once('@').map(|(work, _)| Route::Work {
                key: work.into(),
                title: String::new(),
            }),
            _ => None,
        }
    }
}

/// The route a path names, or none: `Route::from_path`, for the phones
/// (UniFFI writes no constructor on an enum).
#[cfg_attr(feature = "ffi", uniffi::export)]
pub fn route_from_path(path: String) -> Option<Route> {
    Route::from_path(&path)
}

#[cfg(feature = "ffi")]
#[uniffi::export]
impl Route {
    /// The path (`Route::path`): `album/Water%20Music`.
    #[uniffi::method(name = "path")]
    pub fn ffi_path(&self) -> String {
        self.path()
    }

    #[uniffi::method(name = "title")]
    pub fn ffi_title(&self) -> String {
        self.title()
    }

    #[uniffi::method(name = "kind")]
    pub fn ffi_kind(&self) -> String {
        self.kind().into()
    }

    #[uniffi::method(name = "heading")]
    pub fn ffi_heading(&self) -> Option<String> {
        self.heading().map(String::from)
    }

    #[uniffi::method(name = "is_root")]
    pub fn ffi_is_root(&self) -> bool {
        self.is_root()
    }

    #[uniffi::method(name = "fallback")]
    pub fn ffi_fallback(&self) -> Route {
        self.fallback()
    }

    #[uniffi::method(name = "work")]
    pub fn ffi_work(&self) -> Option<Route> {
        self.work()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_router::{assert_round_trips, Routable};

    fn keyed(key: &str, title: &str) -> (String, String) {
        (key.into(), title.into())
    }

    /// Every variant survives its path — a slash would look like the
    /// separator, a `#` would end a URL's fragment, an accent is two bytes —
    /// and a key with a slash of its own. Falsified by spelling `Diagnostics`
    /// as `/settings`: it reads back as Settings.
    #[test]
    fn every_route_survives_its_path() {
        let (ak, at) = keyed("music/Ravel/Boléro / Pavane #1/bolero", "Boléro / Pavane #1");
        let (wk, wt) = keyed("johann-sebastian-bach/bwv-988", "Goldberg Variations");
        let (rk, rt) = keyed("johann-sebastian-bach/bwv-988@kimiko-ishizaka", "Kimiko Ishizaka");
        let (sk, st) = keyed("podcast/full-time-nix", "Full Time Nix");
        assert_round_trips(&[
            Route::Library,
            Route::Albums,
            Route::Artists,
            Route::Composers,
            Route::Podcasts,
            Route::Sermons,
            Route::Playlist { name: "Favorites".into() },
            Route::Playlist {
                name: "100% & more? #2".into(),
            },
            Route::Album { key: ak, title: at },
            Route::Artist {
                name: "Antonín Dvořák".into(),
            },
            Route::Work { key: wk, title: wt },
            Route::Recording { key: rk, title: rt },
            Route::Show { key: sk, title: st },
            Route::Settings,
            Route::Transcript,
            Route::Diagnostics,
            Route::Join {
                server: "https://harken.example.com:8443/listen/?x=1&y=2".into(),
                code: Some("a b&c=d".into()),
            },
            Route::Join {
                server: "192.168.1.4:3000".into(),
                code: None,
            },
        ]);
        // The spelling itself, which the listening session and every old
        // link already carry.
        assert_eq!(
            Route::Playlist {
                name: "100% & more? #2".into()
            }
            .path(),
            "playlist/100%25%20%26%20more%3F%20%232"
        );
        assert_eq!(
            Route::Work {
                key: "johann-sebastian-bach/bwv-988".into(),
                title: "Goldberg Variations".into()
            }
            .path(),
            "work/johann-sebastian-bach%2Fbwv-988"
        );
        assert_eq!(Route::Library.path(), "library");
    }

    /// A route is which page, not what it is called: one read back from its
    /// path, with no title, is the one it was written from, in a set too.
    /// Falsified by deriving `PartialEq`.
    #[test]
    fn a_title_is_not_which_route() {
        let titled = Route::Album {
            key: "x/y".into(),
            title: "Y".into(),
        };
        let read = Route::from_path(&titled.path()).unwrap();
        assert_eq!(read.title(), "");
        assert_eq!(read, titled);
        let set: std::collections::HashSet<Route> = [titled].into();
        assert!(set.contains(&read));
    }

    /// A path nobody wrote names nothing, and an old `composer/` link is the
    /// person's page.
    #[test]
    fn a_path_that_means_nothing_is_none() {
        for path in ["", "#", "nonsense/Water Music", "album", "album/", "library/x"] {
            assert_eq!(Route::from_path(path), None, "{path}");
        }
        assert_eq!(
            Route::from_path("#composer/Antonio%20Vivaldi"),
            Some(Route::Artist {
                name: "Antonio Vivaldi".into()
            })
        );
        assert_eq!(<Route as Routable>::home(), Route::Library);
        // A truncated escape is not a reason to fail: what decoded, decoded.
        assert_eq!(Route::from_path("artist/Bol%"), Some(Route::Artist { name: "Bol%".into() }));
    }

    /// What each route says of itself, as the shells drew it.
    #[test]
    fn titles_kinds_and_roots() {
        assert_eq!(Route::Library.title(), "Songs");
        assert_eq!(Route::Playlist { name: "Sunday".into() }.title(), "Sunday");
        assert_eq!(
            Route::Show {
                key: "sermon/x".into(),
                title: "X".into()
            }
            .kind(),
            "Sermons"
        );
        assert_eq!(
            Route::Show {
                key: "podcast/x".into(),
                title: "X".into()
            }
            .kind(),
            "Podcast"
        );
        assert_eq!(Route::Playlist { name: "a".into() }.heading(), Some("Playlists"));
        assert!(Route::Albums.is_root() && !Route::Playlist { name: "a".into() }.is_root() && !Route::Settings.is_root());
        assert_eq!(Route::Artist { name: "a".into() }.fallback(), Route::Library);
        let (key, title) = keyed("w/k@who", "");
        assert_eq!(
            Route::Recording { key, title }.work(),
            Some(Route::Work {
                key: "w/k".into(),
                title: String::new()
            })
        );
    }
}
