//! `routes!` over a folder of no app's (`tests/app`): each convention read
//! into the enum, its paths and what a static export writes.

ark_router::routes! {
    /// Somewhere the fixture can be.
    #[derive(Debug, Clone)]
    pub enum Route in "tests/app";
}

use ark_router::{assert_round_trips, Routable};

const ID: &str = "0123456789abcdef0123456789abcdef";

/// Each file's path is its folders' and its name, a group adding nothing
/// and an index its folder's own; `[x]` the field `x`, typed; a slug after
/// a row's id; the query its own; a path's own words before a field.
/// Falsified by keeping a group in the path: `/about/` reads as nothing.
#[test]
fn a_file_is_its_path() {
    let routes = [
        Route::Home,
        Route::About,
        Route::Album {
            key: "a/b c".into(),
            title: "B".into(),
        },
        Route::Account {
            id: ID.into(),
            name: "Joint / Savings".into(),
        },
        Route::Join {
            server: "https://h".into(),
            code: Some("9".into()),
        },
        Route::Page { n: 7 },
        Route::Search {
            text: Some("x y".into()),
            max_rows: Some(20),
        },
        Route::Search { text: None, max_rows: None },
        Route::Person { who: "Ada Lovelace".into() },
    ];
    assert_round_trips(&routes);
    assert_eq!(Route::Home.url(), "/");
    assert_eq!(Route::About.url(), "/about/");
    assert_eq!(
        Route::Account {
            id: ID.into(),
            name: "Joint / Savings".into()
        }
        .url(),
        format!("/account/{ID}/joint-savings/")
    );
    assert_eq!(Route::Page { n: 7 }.url(), "/page/7/");
    assert_eq!(Route::from_url("/page/seven/"), None, "a page is a number");
    assert_eq!(
        Route::Search {
            text: Some("x".into()),
            max_rows: Some(3)
        }
        .url(),
        "/search/?q=x&max-rows=3"
    );
    assert_eq!(
        Route::from_url("/search/?max-rows=many&q=x"),
        Some(Route::Search {
            text: Some("x".into()),
            max_rows: None
        })
    );
    assert_eq!(Route::from_url("/join/?code=1"), None, "a server is needed");
    assert_eq!(<Route as Routable>::home(), Route::Home, "the root's index is home");
    assert_eq!(
        Route::from_url("/people/everyone/"),
        Some(Route::Everyone),
        "a path's own word before a field"
    );
    assert_eq!(Route::from_url("/people/ada/"), Some(Route::Person { who: "ada".into() }));
    assert_eq!(Route::route_of("/nowhere/"), Route::Home);
}

/// A route file's struct is the route, and a static export writes each
/// route that carries nothing and each a file lists, a path's own words
/// first. Falsified by leaving `params()` out: no page is written.
#[test]
fn the_files_say_what_is_written() {
    assert_eq!(
        Route::from(app::account::id::Account {
            id: ID.into(),
            name: String::new()
        })
        .key(),
        format!("/account/{ID}/")
    );
    assert_eq!(Route::from(app::group::about::About), Route::About);
    let urls: Vec<String> = Route::statics().iter().map(Route::url).collect();
    assert_eq!(urls, ["/", "/about/", "/people/everyone/", "/page/1/", "/page/2/"]);
}
