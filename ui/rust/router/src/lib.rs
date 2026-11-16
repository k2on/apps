//! Where an app is, one way on every shell: the one route model every app
//! declares its places in, and every shell reads.
//!
//! An app's places are its `Route` enum, declared once in its domain or
//! app crate (`harken_domain::routes`, `money_app::routes`, the docs'
//! `routes`) with `#[derive(Routes)]`, a pattern per variant as a web
//! router has them — `#[route("/album/:key")] Album { key: String, title:
//! String }` — and each route is an [`Address`]: a path that names the
//! content, and the query the UI's state over it. The address's URL
//! (`/album/water-music/`) is what the browser's address bar says, what the
//! desktop's says (arkui's router), the path of a phone's deep link
//! (`harken://album/water-music/`) and of its launch argument, a
//! notification's target, a static site's page and a restored back stack:
//! a route named on one device is the same route on another. The
//! conventions every app's URLs keep are ui/docs/app.md's "Addresses".
//!
//! **A row is named by its id.** A route to a row that has one — an
//! account, a category, a playlist — carries the id, and the name after it
//! only to be read (`/account/<id>/checking/`; [`Routable::key`] leaves it
//! out), so a renamed row keeps its routes and two rows of one name are two
//! routes. What has no row of its own is named by a name or a derived key
//! (`/album/water-music/`). Either way a shell resolves the route against
//! what it has, which also decides what a stale link does: land somewhere
//! that is a page, rather than on a heading with nothing under it.
//!
//! **One segment, one name.** Each `:name` is one segment, percent-encoded
//! ([`url::encode`]): a `/` in a name is `%2F`, so a key with slashes of its
//! own (`johann-sebastian-bach/bwv-988`) is one segment and survives.
//!
//! **Declared, not written by hand.** The derive writes the address, the
//! reading back and [`Routable`]; a test holds every variant to
//! [`assert_round_trips`].
//!
//! # A Folder of Routes: [`routes!`]
//!
//! An app's routes are its `src/app` folder, as Expo Router's are its
//! `app/`: the macro reads the folder as the crate compiles and writes the
//! `Route` enum (through the derive), its modules, and — with the app's
//! type — its screens and what a route does to them.
//!
//! ```ignore
//! // money/src/lib.rs
//! ark_router::routes! {
//!     /// Somewhere money can be.
//!     #[derive(Debug, Clone, ark::twin::Twin)]
//!     #[ark(ffi_eq)]
//!     pub enum Route in "src/app";
//!     /// money as every generated target runs it.
//!     pub type App = host::Money;
//! }
//! ```
//!
//! ```text
//! money/src/app/
//!   _layout.rs              the shell: tabs and sidebar (MoneyShell); its own modules in _layout/
//!   +not-found.rs           where a path that names nothing lands: `pub use crate::Route::Budget as Home;`
//!   (modals)/_layout.rs     the sheets, over every section
//!   (tabs)/(budget)/_layout.rs           the Budget tab's screen
//!   (tabs)/(budget)/budget.rs            /budget/
//!   (tabs)/(accounts)/account/_layout.rs the register
//!   (tabs)/(accounts)/account/[id].rs    /account/<id>/<slug>/
//!   (tabs)/(transactions)/inbox.rs       /inbox/, in the Transactions tab
//!   category/[id].rs                     /category/<id>/<slug>/: the app's own (the inspector)
//!   join.rs                              /join/?server=…&code=…
//! ```
//!
//! - **A route file** is a path: its folders but a `(group)`, then its
//!   name, `index.rs` its folder's own path. It declares its struct,
//!   `#[derive(ark_router::Route)]`, whose name is the variant's, whose
//!   docs are its docs and whose fields are what the route carries, typed
//!   ([`Param`]): `[id].rs` is the field `id` (`#[hex]` a row's id, which
//!   reads only 32 hex digits), `#[slug]` the readable slug after it,
//!   `#[query]` (`#[query = "key"]`) a query key, and any other field is
//!   no part of the address (a title). A dynamic route lists what a static
//!   export writes of it with `fn params() -> Vec<Self>` (Next.js's
//!   `generateStaticParams`): `Route::statics()` is those and every route
//!   that carries nothing.
//! - **`_layout.rs`** is a screen that lays out its folder: its runtime
//!   marked `#[ark_router::screen(view = view::budget)]` (its further views
//!   beside, `settings = view::settings`; `name = "…"` where its folder's
//!   name is not its name; `beside`), its own modules in `_layout/`. The
//!   root's is the shell (`shell`): the adaptive shell's tabs and sidebar,
//!   and the stack every other page is pushed on. `(modals)`' goes over
//!   every section (`sheets`). Each group in `(tabs)` is a tab, its layout
//!   that tab's screen. Every screen is at the crate's root by its name
//!   (`crate::budget`).
//! - **What a route does** (`ark_app::host::Routed`, which the app's
//!   `HostedApp::route` asks first): the tab it is in selected (its group
//!   under `(tabs)`, or `#[tab = "accounts/{id}"]`, its fields in braces;
//!   `ark_app::shell::Navigates::select`), the screen that shows it told
//!   (`#[shows(screen)]`, else its nearest layout's; `ark_app::App::shown`),
//!   and a route in no tab whose screen is not the shell pushed
//!   (`Navigates::push`). A route the shell shows outside a tab is the
//!   app's own: handed back.
//! - **`+not-found.rs`** says where an address that names nothing lands
//!   (`Routable::home`); without it, the root's `index.rs`.
//! - A folder beside a file of its name (`budget.rs` and `budget/`) is
//!   that file's own modules, not routes; any other file starting `_` or
//!   `+` is refused. A file added is read when the crate next compiles:
//!   touch `lib.rs` if nothing else of it changed.
extern crate self as ark_router;

pub mod address;
pub mod param;
pub mod url;

pub use address::Address;
pub use ark_router_macros::folder as routes;
pub use ark_router_macros::{screen, Route, Routes};
pub use param::Param;

/// Every route reads back from its URL as itself — from its address, its
/// URL, its URL under a base, a phone's link and its URL with the UI's
/// query after it — and its URL read back is written the same, with a
/// slash at its end. A domain's test calls it with one of every variant,
/// names with awkward characters among them.
pub fn assert_round_trips<R: Routable + std::fmt::Debug>(routes: &[R]) {
    for route in routes {
        let url = route.url();
        let back = R::from_url(&url).unwrap_or_else(|| panic!("{url} reads back as nothing"));
        assert_eq!(&back, route, "round trip of {url}");
        assert_eq!(R::from_address(&route.address()).as_ref(), Some(route), "{url}'s address");
        // Read back, a route has no slug (its title is the shell's to find),
        // so it is written as its key.
        assert_eq!(back.url(), route.key(), "{url} reads back spelled otherwise");
        assert_eq!(R::from_url(&route.key()).as_ref(), Some(route), "{url}'s key reads as it");
        let under = route.address().url_under("/base/");
        assert_eq!(R::from_address(&Address::of_url(&under, "/base/")).as_ref(), Some(route), "{under}");
        assert_eq!(R::from_url(&format!("app:/{url}")).as_ref(), Some(route), "app:/{url}, a phone's link");
        let ui = route.address().with("inspector", "1");
        assert_eq!(R::from_address(&ui).as_ref(), Some(route), "{url} with the UI's query");
        let path = url.split('?').next().unwrap_or("");
        assert!(path.ends_with('/'), "{url} has no slash at its end");
        assert!(!url.contains(['#', ' ']), "{url} would not survive a URL");
    }
}

/// What a route enum is to a router: an address, read back.
///
/// `from_address(&r.address()) == Some(r)` for every route an app makes:
/// two routes are the same route when their keys are, whatever else they
/// carry (a title is what a route is called, not which it is).
pub trait Routable: Clone + PartialEq {
    /// The address: the path that names it, and its own query.
    fn address(&self) -> Address;
    /// The route an address names, or `None` for one that names nothing
    /// here. A query key the route does not name is the UI's: passed over.
    fn from_address(address: &Address) -> Option<Self>;
    /// Which route it is: its URL without its readable slug
    /// (`/account/<id>/`, not `/account/<id>/checking/`). Two routes are
    /// the same when their keys are.
    fn key(&self) -> String {
        self.address().url()
    }
    /// Where an address that names nothing lands: the app's first page.
    fn home() -> Self;
    /// The URL, at the root: `/album/water-music/`.
    fn url(&self) -> String {
        self.address().url()
    }
    /// The route a URL names (any spelling `Address::of_url` reads).
    fn from_url(url: &str) -> Option<Self> {
        Self::from_address(&Address::of_url(url, "/"))
    }
    /// The route a URL names, or home: a link somebody typed wrong lands
    /// somewhere rather than nowhere.
    fn route_of(url: &str) -> Self {
        Self::from_url(url).unwrap_or_else(Self::home)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A name with every awkward character is one segment, and comes back
    /// as it went in. Falsified by writing the name unencoded: the `/` is
    /// two segments.
    #[test]
    fn a_name_survives_its_url() {
        for name in ["Water Music", "Boléro / Pavane #1", "johann-sebastian-bach/bwv-988", "100% & more?"] {
            let album = Route::Album {
                key: name.into(),
                title: String::new(),
            };
            assert_eq!(Route::from_url(&album.url()), Some(album.clone()), "round trip of {}", album.url());
        }
        let album = Route::Album {
            key: "Boléro / Pavane #1".into(),
            title: String::new(),
        };
        assert_eq!(album.url(), "/album/Bol%C3%A9ro%20%2F%20Pavane%20%231/");
        assert_eq!(Route::Library.url(), "/");
    }

    #[derive(Debug, Clone, Routes)]
    enum Route {
        #[home]
        #[route("/")]
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
        #[route("/account/:id(hex)/~name")]
        #[route("/account/:name")]
        Account { id: String, name: String },
    }

    const ID: &str = "0123456789abcdef0123456789abcdef";

    fn account(id: &str, name: &str) -> Route {
        Route::Account {
            id: id.into(),
            name: name.into(),
        }
    }

    /// A route by id is written with its name as a slug, is the same route
    /// whatever it is called, and reads back without the slug or with any
    /// other. An old link's name still reads, and is written as it was.
    /// Falsified by putting the slug in `key`: a rename is another route.
    #[test]
    fn an_id_is_which_and_a_slug_is_for_reading() {
        let checking = account(ID, "Joint / Savings #2");
        assert_eq!(checking.url(), format!("/account/{ID}/joint-savings-2/"));
        assert_eq!(checking.key(), format!("/account/{ID}/"));
        assert_round_trips(&[
            checking.clone(),
            account(ID, ""),
            account("", "Checking"),
            account("", "Joint / Savings #2"),
        ]);
        // Renamed, it is the same route, and a link with the old slug finds it.
        assert_eq!(account(ID, "Shared"), checking);
        assert_eq!(Route::from_url(&format!("/account/{ID}/old-name/")), Some(account(ID, "")));
        assert_eq!(Route::from_url(&format!("/account/{}/", ID.to_uppercase())), Some(account(ID, "")));
        // Two of one name are two routes.
        assert_ne!(account(ID, "Checking"), account(&ID.replace('0', "f"), "Checking"));
        // The old spelling: a name, written back as it was.
        assert_eq!(
            Route::from_url("/account/Joint%20%2F%20Savings%20%232/"),
            Some(account("", "Joint / Savings #2"))
        );
        assert_eq!(account("", "Checking").url(), "/account/Checking/");
        assert_ne!(account("", "Checking"), account(ID, "Checking"), "a name is not an id");
        assert_eq!(Route::from_url("/account/"), None);
    }

    /// A query's fields are escaped, read in any order, and an optional one
    /// is left out when it is `None`; a key the route does not name is the
    /// UI's. Falsified by not escaping a value: a server's own `?` and `&`
    /// would read as the route's.
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
            join("https://h:8443/p", Some("1")).url(),
            "/join/?server=https%3A%2F%2Fh%3A8443%2Fp&code=1"
        );
        assert_eq!(join("h", None).url(), "/join/?server=h");
        assert_eq!(Route::from_url("/join/?code=9&server=h"), Some(join("h", Some("9"))));
        assert_eq!(Route::from_url("/join/?server=h&code="), Some(join("h", None)));
        assert_eq!(Route::from_url("/join/?code=9"), None, "a server is needed");
        assert_eq!(Route::from_url("/join/"), None);
        assert_eq!(Route::from_url("/?inspector=1"), Some(Route::Library), "the UI's query is passed over");
    }

    /// The derive reads back every pattern, a link's unescaped slash in its
    /// last segment, and refuses what names nothing. Falsified by dropping
    /// the `is_empty` check: an empty segment reads as an album with no key.
    #[test]
    fn the_derive_spells_and_reads() {
        let album = Route::Album {
            key: "music/Ravel/Boléro / Pavane #1".into(),
            title: "Boléro".into(),
        };
        assert_eq!(album.url(), "/album/music%2FRavel%2FBol%C3%A9ro%20%2F%20Pavane%20%231/");
        assert_round_trips(&[
            Route::Library,
            album.clone(),
            Route::Artist { name: "100% & more?".into() },
            Route::Shelf {
                room: "a/b".into(),
                key: "c d".into(),
            },
        ]);
        let Some(Route::Album { title, .. }) = Route::from_url(&album.url()) else {
            panic!()
        };
        assert_eq!(title, "", "a title is not in the path");
        assert_eq!(Route::from_url("/composer/Bach/"), Some(Route::Artist { name: "Bach".into() }));
        assert_eq!(
            Route::from_url("/album/johann-sebastian-bach/bwv-988/"),
            Some(Route::Album {
                key: "johann-sebastian-bach/bwv-988".into(),
                title: String::new()
            })
        );
        for nothing in ["/album/", "/library/x/", "/nonsense/", "/shelf/a/", "/x/"] {
            assert_eq!(Route::from_url(nothing), None, "{nothing}");
        }
        assert_eq!(Route::from_url(""), Some(Route::Library), "the root");
        assert_eq!(<Route as Routable>::home(), Route::Library);
    }
}
