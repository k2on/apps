//! How an app's routes are spelled, as one string.
//!
//! An app's routes are its domain's `Route` enum (`harken_domain::routes`,
//! `money_domain::routes`), and each route has one path — its
//! [`Routable::path`]: `album/Water%20Music`, `account/Checking`, `budget`.
//! That one spelling is what the desktop's and the browser's address bar say
//! (arkui's router puts a `#` in front), and what a deep link, a
//! notification's target or a restored back stack would say, so a route
//! named on one device is the same route on another.
//!
//! **A path carries names, not ids.** An id is not something a person types;
//! `album/Water%20Music` is. So a shell resolves a name against what it has —
//! which also decides what a stale link does: land somewhere that is a page,
//! rather than on a heading with nothing under it.
//!
//! **One separator.** A path is a kind, or a kind, `/` and one
//! percent-encoded name ([`join`], [`split`]): a `/` in a name is `%2F`, so
//! the first `/` is always the separator, and a key with slashes of its own
//! (`johann-sebastian-bach/bwv-988`) survives.
//!
//! **Declared, not written by hand.** An app's enum derives [`Routes`],
//! a pattern per variant as a web router has them —
//! `#[route("/album/:key")] Album { key: String, title: String }` — and the
//! derive writes the path, the reading back and [`Routable`]; a test holds
//! every variant to [`assert_round_trips`].
extern crate self as ark_router;

pub mod url;

pub use ark_macros::Routes;

/// Every route reads back from its path as itself — from its path and from
/// the path as a URL's fragment — and its path read back is written the
/// same. A domain's test calls it with one of every variant, names with
/// awkward characters among them.
pub fn assert_round_trips<R: Routable + std::fmt::Debug>(routes: &[R]) {
    for route in routes {
        let path = route.path();
        let back = R::from_path(&path).unwrap_or_else(|| panic!("{path} reads back as nothing"));
        assert_eq!(&back, route, "round trip of {path}");
        assert_eq!(back.path(), path, "{path} reads back spelled otherwise");
        assert_eq!(R::from_path(&format!("#{path}")).as_ref(), Some(route), "#{path}, as a URL's fragment");
        assert!(!path.contains(['#', ' ']), "{path} would not survive a URL");
    }
}

/// What a route enum is to a router: a path, read back.
///
/// `from_path(&r.path()) == Some(r)` for every route an app makes: two
/// routes are the same route when their paths are, whatever else they carry
/// (a title is what a route is called, not which it is).
pub trait Routable: Clone + PartialEq {
    /// The path, with no `#` in front: `kind` or `kind/name`.
    fn path(&self) -> String;
    /// The route a path names, or `None` for a path that names nothing here.
    /// A leading `#` is allowed, so a URL's fragment reads as it is.
    fn from_path(path: &str) -> Option<Self>;
    /// Where a path that names nothing lands: the app's first page.
    fn home() -> Self;
}

/// `kind`, or `kind/name` with the name percent-encoded — so a `/` in a name
/// cannot read as the separator, a `#` cannot end a URL's fragment, and an
/// accent survives as the two bytes it is.
pub fn join(kind: &str, name: Option<&str>) -> String {
    let mut out = kind.to_string();
    if let Some(name) = name {
        out.push('/');
        url::encode(name, &mut out);
    }
    out
}

/// The other half of [`join`]: the kind, and the decoded name after the
/// *first* `/` if there is one. A leading `#` (a URL's fragment) is dropped.
pub fn split(path: &str) -> (&str, Option<String>) {
    let body = path.trim_start_matches('#');
    match body.split_once('/') {
        Some((kind, name)) => (kind, Some(url::decode(name))),
        None => (body, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A name with every awkward character comes back as it went in, and the
    /// spelling escapes what a URL would otherwise read: falsified by writing
    /// the name unencoded in `join` (the round trip still passes; the
    /// spelling does not).
    #[test]
    fn a_name_survives_its_path() {
        for name in ["Water Music", "Boléro / Pavane #1", "johann-sebastian-bach/bwv-988", "100% & more?", ""] {
            let path = join("album", Some(name));
            assert_eq!(split(&path), ("album", Some(name.to_string())), "round trip of {path}");
            assert_eq!(
                split(&format!("#{path}")),
                ("album", Some(name.to_string())),
                "a fragment reads as its path"
            );
        }
        assert_eq!(join("album", Some("Boléro / Pavane #1")), "album/Bol%C3%A9ro%20%2F%20Pavane%20%231");
        assert_eq!(join("library", None), "library");
        assert_eq!(split("library"), ("library", None));
        assert_eq!(split("#"), ("", None));
    }

    #[derive(Debug, Clone, Routes)]
    enum Route {
        #[home]
        #[route("/library")]
        Library,
        #[route("/album/:key")]
        Album { key: String, title: String },
        #[route("/artist/:name")]
        #[route("/composer/:name")]
        Artist { name: String },
        #[route("/shelf/:room/:key")]
        Shelf { room: String, key: String },
        #[route("/join?server=:server&code=:code")]
        Join { server: String, code: Option<String> },
    }

    /// A query's fields are escaped, read in any order, and an optional one
    /// is left out when it is `None`. Falsified by not escaping a value: a
    /// server's own `?` and `&` would read as the route's.
    #[test]
    fn a_query_survives_its_path() {
        let join = |server: &str, code: Option<&str>| Route::Join {
            server: server.into(),
            code: code.map(String::from),
        };
        assert_round_trips(&[
            join("https://money.example.com", Some("a b&c=d")),
            join("http://192.168.1.4:8080/money/?x=1&y=2", None),
            join("localhost:3000", Some("ABC-123")),
        ]);
        assert_eq!(
            join("https://h:8443/p", Some("1")).path(),
            "join?server=https%3A%2F%2Fh%3A8443%2Fp&code=1"
        );
        assert_eq!(join("h", None).path(), "join?server=h");
        assert_eq!(Route::from_path("join?code=9&server=h"), Some(join("h", Some("9"))));
        assert_eq!(Route::from_path("join?server=h&code="), Some(join("h", None)));
        assert_eq!(Route::from_path("join?code=9"), None, "a server is needed");
        assert_eq!(Route::from_path("join"), None);
        assert_eq!(Route::from_path("library?x=1"), None, "a route with no query takes none");
    }

    /// The derive writes what `join` would, reads back every pattern, an old
    /// spelling and an old link's unescaped slash, and refuses what names
    /// nothing. Falsified by dropping the `is_empty` check: `album/` reads
    /// as an album with no key.
    #[test]
    fn the_derive_spells_and_reads() {
        let album = Route::Album {
            key: "music/Ravel/Boléro / Pavane #1".into(),
            title: "Boléro".into(),
        };
        assert_eq!(album.path(), join("album", Some("music/Ravel/Boléro / Pavane #1")));
        assert_round_trips(&[
            Route::Library,
            album.clone(),
            Route::Artist { name: "100% & more?".into() },
            Route::Shelf {
                room: "a/b".into(),
                key: "c d".into(),
            },
        ]);
        let Some(Route::Album { title, .. }) = Route::from_path(&album.path()) else {
            panic!()
        };
        assert_eq!(title, "", "a title is not in the path");
        assert_eq!(Route::from_path("composer/Bach"), Some(Route::Artist { name: "Bach".into() }));
        assert_eq!(
            Route::from_path("#album/johann-sebastian-bach/bwv-988"),
            Some(Route::Album {
                key: "johann-sebastian-bach/bwv-988".into(),
                title: String::new()
            })
        );
        for nothing in ["", "#", "album", "album/", "library/x", "nonsense", "shelf/a"] {
            assert_eq!(Route::from_path(nothing), None, "{nothing}");
        }
        assert_eq!(<Route as Routable>::home(), Route::Library);
    }
}
