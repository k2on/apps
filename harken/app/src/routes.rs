//! Where harken can be: every route, one path each (`ark-router`), on every
//! shell — the desktop's and the browser's address bar, each phone's back
//! stack, a deep link, the listening session's "played from", Android
//! Auto's media ids.
//!
//! **A row is named by its id.** A playlist carries its row's id and, after
//! it, a slug of its name only to read (`/playlist/<id>/road-trip/`), so a
//! renamed playlist keeps its page and two of one name (two people's) are
//! two routes. An album, a work, a recording and a show have no row of
//! their own: they are named by the key the domain derives, which has
//! slashes of its own (`johann-sebastian-bach/bwv-988`, one segment,
//! `%2F`), and the title to draw beside it. What a route is called is not
//! which route it is: two routes are the same when their keys are
//! (`Routable::key`, the URL without a slug), so a route read back from a
//! URL — which has no title — is the route it was written from.
//!
//! **A stale route lands on the library.** A link to a playlist since
//! deleted, or to an album not received yet, is resolved by the shell
//! against what it has, and falls back to [`Route::fallback`]: a page,
//! rather than a heading with nothing under it.
use ark_router::Routes;

/// Somewhere harken can be.
#[derive(Debug, Clone, Routes, ark::twin::Twin)]
#[ark(ffi_eq)]
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
    /// A playlist, by its id (`Id::hex`), its name after it only to read.
    #[route("/playlist/:id(hex)/~name")]
    Playlist { id: String, name: String },
    /// An album by its key (`keys::album_id`), and its name: two records of
    /// one title are two albums.
    #[route("/album/:key")]
    Album { key: String, title: String },
    /// One person's page, whatever they did.
    #[route("/artist/:name")]
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
    /// Sign in to a server from a link or a QR code: `/join/?server=…&code=…`.
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
            Route::Playlist { name, .. } | Route::Artist { name } => name.clone(),
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

    /// Whether the route is the playlist of this id (`Id::hex`) — what
    /// every shell finds a playlist's route by. So a renamed playlist keeps
    /// its page, and of two of one name only the one meant is found. The
    /// name is the shells' to pass; it is not which playlist.
    pub fn names(&self, id: &str, _name: &str) -> bool {
        matches!(self, Route::Playlist { id: mine, .. } if !mine.is_empty() && mine == id)
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

#[cfg(test)]
mod tests {
    use super::*;
    use ark_router::{assert_round_trips, Routable};

    fn keyed(key: &str, title: &str) -> (String, String) {
        (key.into(), title.into())
    }

    const ROAD: &str = "0000000000000000000000000000000a";
    const OTHER: &str = "0000000000000000000000000000000b";

    fn playlist(id: &str, name: &str) -> Route {
        Route::Playlist {
            id: id.into(),
            name: name.into(),
        }
    }

    /// Every variant survives its URL — a slash would read as two
    /// segments, a `#` would end the URL, an accent is two bytes — and a key
    /// with a slash of its own. Falsified by spelling `Diagnostics` as
    /// `/settings`: it reads back as Settings.
    #[test]
    fn every_route_survives_its_url() {
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
            playlist(ROAD, "Favorites"),
            playlist(ROAD, "100% & more? #2"),
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
        // The spelling itself: a playlist by its id, a key's slash one
        // segment's, a slash at the end.
        assert_eq!(playlist(ROAD, "Road trip").url(), format!("/playlist/{ROAD}/road-trip/"));
        assert_eq!(
            Route::Work {
                key: "johann-sebastian-bach/bwv-988".into(),
                title: "Goldberg Variations".into()
            }
            .url(),
            "/work/johann-sebastian-bach%2Fbwv-988/"
        );
        assert_eq!(Route::Library.url(), "/library/");
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
        let read = Route::from_url(&titled.url()).unwrap();
        assert_eq!(read.title(), "");
        assert_eq!(read, titled);
        let set: std::collections::HashSet<Route> = [titled].into();
        assert!(set.contains(&read));
    }

    /// A renamed playlist is the same route, still found; two playlists of
    /// one name are two; a playlist by its name is no route. Falsified by
    /// finding a playlist by its name.
    #[test]
    fn a_playlist_is_its_id() {
        let before = playlist(ROAD, "Road trip");
        assert_eq!(before, playlist(ROAD, "Summer"));
        assert_eq!(Route::from_url(&before.url()), Some(playlist(ROAD, "")));
        assert!(before.names(ROAD, "Summer") && !before.names(OTHER, "Road trip"));
        assert_ne!(playlist(ROAD, "Favorites"), playlist(OTHER, "Favorites"));
        assert_eq!(Route::from_url("/playlist/Road%20trip/"), None, "a name is no playlist's");
        assert!(!Route::Library.names(ROAD, "Songs"));
    }

    /// A URL nobody wrote names nothing, and the root is the library.
    #[test]
    fn a_url_that_means_nothing_is_none() {
        for url in ["/nonsense/Water%20Music/", "/album/", "/library/x/", "/composer/Antonio%20Vivaldi/"] {
            assert_eq!(Route::from_url(url), None, "{url}");
        }
        assert_eq!(<Route as Routable>::home(), Route::Library);
        assert_eq!(Route::route_of("/"), Route::Library);
        // A truncated escape is not a reason to fail: what decoded, decoded.
        assert_eq!(Route::from_url("/artist/Bol%/"), Some(Route::Artist { name: "Bol%".into() }));
    }

    /// What each route says of itself, as the shells drew it.
    #[test]
    fn titles_kinds_and_roots() {
        assert_eq!(Route::Library.title(), "Songs");
        assert_eq!(playlist(ROAD, "Sunday").title(), "Sunday");
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
        assert_eq!(playlist(ROAD, "a").heading(), Some("Playlists"));
        assert!(Route::Albums.is_root() && !playlist(ROAD, "a").is_root() && !Route::Settings.is_root());
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
